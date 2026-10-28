//! Pieces the speech recognition references of speech world batch V3-3 share (design record
//! `docs/design/2026-10-14-speech-v3-3-asr.md` §1, decision 2): the declared operations and the
//! `seconds` form, the multipart request view and its part list, the client formats, the duration
//! value rule, the transcript rule, the delivered media types, the `srt` / `vtt` renderer and the
//! `ElevenLabs` cue grouping (SV-Q8), and the `OpenAI` audio protocol `speech-openai-compatible` and
//! `speech-xai` share (§2, §3).
//!
//! The request view is the host's multipart view `{"multipart": {"parts": [...]}}` (speech record
//! §5): a text part above the 1 MiB fallback threshold arrives elided (`blob` instead of `value`)
//! and is refused, since a multipart text part cannot carry a reference.

#![allow(
    clippy::result_large_err,
    reason = "a rendering step fails with the outcome the call ends with, built once per round"
)]

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde_json::{Map, Value, json};
use south_contracts::DeclaredQueryParameterV1;
use south_contracts::media::{
    MediaLimitsV1, MediaPartV1, MediaRequestDescriptorV1, MediaTransformV1, ResponseBodyFormV1,
};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechDialectWordV1, SpeechFactsV1,
    SpeechMeteringFormV1, SpeechMeteringV1, SpeechModelCapabilitiesV1, SpeechOperationV1,
    SpeechSecondsBasisV1, SpeechSecondsMeterV1, TranscriptDeliveryV1, is_speech_media_type,
    speech_dialect_words_v1,
};
use token_station_protocol::ProviderConfig;

use crate::speech_shared::{
    BLOB_KEY, capability, form_word, internal, invalid, json_object, operation_word, serves,
    unknown, with_refs,
};
use crate::{ComponentResultV1, SpeechDeliveryV1, SpeechOutcomeV1};

/// A JSON document's media type (the suite's `delivery_fits_format` compares it as a string).
pub const JSON_MEDIA_TYPE: &str = "application/json";
/// A rendered `text` document's media type (native Azure's label).
pub const TEXT_MEDIA_TYPE: &str = "text/plain; charset=utf-8";
/// A rendered `srt` document's media type.
pub const SRT_MEDIA_TYPE: &str = "application/x-subrip";
/// A rendered `vtt` document's media type.
pub const VTT_MEDIA_TYPE: &str = "text/vtt";
/// The `OpenAI` audio API's client formats (§1).
pub const OPENAI_FORMATS: [&str; 5] = ["json", "text", "srt", "verbose_json", "vtt"];
/// SV-Q8: a cue closes before a word that starts this many seconds or more after the previous
/// word ends.
pub const CUE_GAP_SECONDS: f64 = 1.0;
/// SV-Q8: a cue closes before a word that would make it last longer than this.
pub const CUE_MAX_SECONDS: f64 = 7.0;
/// SV-Q8: a cue closes before a word that would make its text longer than this many characters.
pub const CUE_MAX_CHARACTERS: usize = 84;
/// SV-Q8: a cue closes after a word ending in one of these.
pub const CUE_SENTENCE_END: [char; 6] = ['.', '!', '?', '\u{3002}', '\u{FF01}', '\u{FF1F}'];

/// The dialect words on the configuration's row for `model`; none when it has no row. A misspelt
/// word under the `speech.` prefix is refused rather than silently turning a capability off
/// (speech record §4).
pub fn row_words(
    config: &ProviderConfig,
    model: &str,
) -> ComponentResultV1<BTreeSet<SpeechDialectWordV1>> {
    let Some(row) = config.models.iter().find(|row| row.model == model) else {
        return Ok(BTreeSet::new());
    };
    speech_dialect_words_v1(row.supported_parameters.iter().map(String::as_str))
        .map_err(|_| invalid("the model row carries a misspelt speech dialect word"))
}

/// `model-capabilities` of a package that synthesizes with `synthesis` (none: it does not) and
/// transcribes: every configured model declares every operation of the package, `translate` only
/// when the package translates (`translates`) and the row carries `speech.translate`; the metering
/// forms are `synthesis` and `seconds` (§1, record §4).
pub fn capabilities(
    config: &ProviderConfig,
    family: &str,
    synthesis: &[SpeechMeteringFormV1],
    translates: bool,
) -> ComponentResultV1<Vec<SpeechModelCapabilitiesV1>> {
    serves(config, family)?;
    let mut models = Vec::with_capacity(config.models.len());
    for row in &config.models {
        let words = row_words(config, &row.model)?;
        let mut operations = Vec::with_capacity(3);
        if !synthesis.is_empty() {
            operations.push(SpeechOperationV1::Synthesize);
        }
        operations.push(SpeechOperationV1::Transcribe);
        if translates && words.contains(&SpeechDialectWordV1::Translate) {
            operations.push(SpeechOperationV1::Translate);
        }
        let mut metering_forms = synthesis.to_vec();
        metering_forms.push(SpeechMeteringFormV1::Seconds);
        models.push(SpeechModelCapabilitiesV1 {
            model: row.model.clone(),
            operations,
            metering_forms,
        });
    }
    Ok(models)
}

/// The facts' forms of a recognition call (§1): the operation must be one the package and the
/// model declare (`transcribe`; `translate` only when the package translates and the row carries
/// `speech.translate`), and the host must require exactly `seconds` (record §8: ASR is priced per
/// second).
pub fn recognition_forms(
    config: &ProviderConfig,
    context: &SpeechCallContextV1,
    translates: bool,
) -> ComponentResultV1<Vec<SpeechMeteringFormV1>> {
    let declared = match context.operation {
        SpeechOperationV1::Synthesize => false,
        SpeechOperationV1::Transcribe => true,
        SpeechOperationV1::Translate => {
            translates
                && row_words(config, &context.upstream_model)?
                    .contains(&SpeechDialectWordV1::Translate)
        }
    };
    if !declared {
        return Err(capability(format!(
            "the model '{}' does not declare the operation {}",
            context.upstream_model,
            operation_word(context.operation)
        )));
    }
    if context.metering_required.is_empty() {
        return Err(capability("the host requires no metering form"));
    }
    if let Some(form) =
        context.metering_required.iter().find(|form| **form != SpeechMeteringFormV1::Seconds)
    {
        return Err(capability(format!("this model cannot report {}", form_word(*form))));
    }
    Ok(vec![SpeechMeteringFormV1::Seconds])
}

/// Whether the row of the routed model carries `speech.verbose_json` (record §9).
pub fn verbose_row(config: &ProviderConfig, model: &str) -> ComponentResultV1<bool> {
    Ok(row_words(config, model)?.contains(&SpeechDialectWordV1::VerboseJson))
}

/// The multipart view's part list, refused before dispatch with the §1 texts when the body is not
/// multipart, a text part is elided, no file part is named `file`, or a `stream` part asks for
/// streaming (`true` in any ASCII case, or `1`, after trimming as the host's predicate does).
pub fn request_parts(request: &Value) -> ComponentResultV1<&[Value]> {
    let parts = request
        .pointer("/multipart/parts")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("a transcription request is a multipart body"))?;
    if parts.iter().any(|part| part["kind"] == "text" && part.get("value").is_none()) {
        return Err(invalid("a text field is longer than the gateway accepts"));
    }
    if !parts.iter().any(|part| part["kind"] == "file" && part["name"] == "file") {
        return Err(invalid("Missing 'file' field"));
    }
    let streaming = parts.iter().any(|part| {
        part["kind"] == "text"
            && part["name"] == "stream"
            && part["value"].as_str().is_some_and(|value| {
                let value = value.trim();
                value.eq_ignore_ascii_case("true") || value == "1"
            })
    });
    if streaming {
        return Err(invalid(
            "streaming transcription is not supported; re-submit without `stream` or with \
             `stream=false`",
        ));
    }
    Ok(parts.as_slice())
}

/// The value of the first text part named `name`.
pub fn first_text<'a>(parts: &'a [Value], name: &str) -> Option<&'a str> {
    parts
        .iter()
        .find(|part| part["kind"] == "text" && part["name"] == name)
        .and_then(|part| part["value"].as_str())
}

/// The client format of the `OpenAI` audio set, compared exactly (absent = `json`); any other
/// value is refused before dispatch (§1).
pub fn openai_format(parts: &[Value]) -> ComponentResultV1<&str> {
    let format = first_text(parts, "response_format").unwrap_or("json");
    if OPENAI_FORMATS.contains(&format) {
        Ok(format)
    } else {
        Err(invalid(format!(
            "Unknown response_format='{format}'. Supported: json, text, srt, verbose_json, vtt."
        )))
    }
}

/// How a package turns the client's parts into the upstream's (§1 "Part list"): every part not
/// named here is forwarded in order, a file part as an `as_is` reference with its file name and
/// media type, a text part with its value.
#[derive(Default)]
pub struct PartPlan<'a> {
    /// Text parts the package writes, `(client name, upstream name, value)`: every client text
    /// part named the first becomes the second with the value, in place; when the client sent
    /// none, the part is appended (in this order, before [`Self::appended`]).
    pub written: Vec<(&'a str, &'a str, String)>,
    /// Text parts renamed with their value unchanged, `(client name, upstream name)`.
    pub renamed_text: &'a [(&'a str, &'a str)],
    /// File parts renamed, `(client name, upstream name)`.
    pub renamed_file: &'a [(&'a str, &'a str)],
    /// Text parts removed.
    pub dropped_text: &'a [&'a str],
    /// Parts of either kind removed.
    pub dropped: &'a [&'a str],
    /// Text parts always appended at the end.
    pub appended: Vec<(&'a str, String)>,
}

fn renamed<'a>(pairs: &[(&'a str, &'a str)], name: &'a str) -> &'a str {
    pairs.iter().find(|(from, _)| *from == name).map_or(name, |(_, to)| *to)
}

/// The upstream part list the plan builds from the client's.
pub fn rewrite_parts(parts: &[Value], plan: &PartPlan<'_>) -> ComponentResultV1<Vec<MediaPartV1>> {
    let mut out: Vec<MediaPartV1> = Vec::with_capacity(parts.len() + plan.written.len() + 1);
    let mut written = vec![false; plan.written.len()];
    for part in parts {
        let name = part["name"].as_str().ok_or_else(|| internal("a part has no name"))?;
        if plan.dropped.contains(&name) {
            continue;
        }
        if part["kind"] == "file" {
            out.push(MediaPartV1::File {
                name: renamed(plan.renamed_file, name).to_owned(),
                blob: serde_json::from_value(part["blob"].clone())
                    .map_err(|_| internal("a file part has no blob"))?,
                transform: MediaTransformV1::AsIs,
                filename: part["filename"].as_str().map(str::to_owned),
                media_type: part["media_type"].as_str().map(str::to_owned),
            });
            continue;
        }
        if plan.dropped_text.contains(&name) {
            continue;
        }
        if let Some(index) = plan.written.iter().position(|(client, _, _)| *client == name) {
            written[index] = true;
            let (_, upstream, value) = &plan.written[index];
            out.push(MediaPartV1::Text { name: (*upstream).to_owned(), value: value.clone() });
            continue;
        }
        out.push(MediaPartV1::Text {
            name: renamed(plan.renamed_text, name).to_owned(),
            value: part["value"].as_str().unwrap_or_default().to_owned(),
        });
    }
    for ((_, upstream, value), seen) in plan.written.iter().zip(written) {
        if !seen {
            out.push(MediaPartV1::Text { name: (*upstream).to_owned(), value: value.clone() });
        }
    }
    for (name, value) in &plan.appended {
        out.push(MediaPartV1::Text { name: (*name).to_owned(), value: value.clone() });
    }
    Ok(out)
}

/// A multipart `POST` descriptor: no `content-type` (the host's encoder renders it from its
/// boundary), the auth arm when the configuration names a slot, the query when there is one.
pub fn multipart_descriptor(
    path: &str,
    query: Option<Value>,
    auth: Option<Value>,
    parts: &[MediaPartV1],
    declared: &[DeclaredQueryParameterV1],
) -> ComponentResultV1<MediaRequestDescriptorV1> {
    let mut wire = Map::new();
    wire.insert("method".to_owned(), json!("POST"));
    wire.insert("path".to_owned(), json!(path));
    if let Some(query) = query {
        wire.insert("query".to_owned(), query);
    }
    if let Some(auth) = auth {
        wire.insert("auth".to_owned(), auth);
    }
    let parts = serde_json::to_value(parts).map_err(|error| internal(error.to_string()))?;
    wire.insert("body".to_owned(), json!({ "multipart": { "parts": parts } }));
    MediaRequestDescriptorV1::parse_with_declared(
        &Value::Object(wire).to_string(),
        &MediaLimitsV1::V1,
        declared,
    )
    .map_err(|error| internal(format!("built an invalid descriptor: {error}")))
}

/// The prepared recognition call: the operation and `seconds`, no characters, no output media
/// type, no reservation (§1, decision 4), and the multipart body's immutable form fields.
pub fn prepared(
    operation: SpeechOperationV1,
    forms: Vec<SpeechMeteringFormV1>,
    descriptor: MediaRequestDescriptorV1,
    response_body_form: ResponseBodyFormV1,
    immutable: &[&str],
    state: &Value,
) -> PreparedSpeechCallV1 {
    PreparedSpeechCallV1 {
        facts: SpeechFactsV1 {
            operation,
            characters: None,
            metering_forms: forms,
            reservation: None,
            output_media_type: None,
        },
        descriptor,
        response_body_form,
        response_elision_paths: Vec::new(),
        immutable_body_paths: None,
        immutable_form_fields: Some(immutable.iter().map(|name| (*name).to_owned()).collect()),
        state: state.to_string(),
    }
}

/// A duration read by the value rule (§1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Duration {
    /// A JSON number, finite and above 0.
    Reported(f64),
    /// Absent, or JSON `null`.
    Absent,
    /// Present and not a usable number: `0`, negative, non-finite or not a number.
    Malformed,
}

/// The value rule: a duration is a JSON number that is finite and above 0 (§1).
pub fn duration(value: Option<&Value>) -> Duration {
    match value {
        None | Some(Value::Null) => Duration::Absent,
        Some(value) => value.as_f64().map_or(Duration::Malformed, duration_of),
    }
}

/// The value rule over a number already read: finite and above 0, else malformed.
pub const fn duration_of(seconds: f64) -> Duration {
    if seconds.is_finite() && seconds > 0.0 {
        Duration::Reported(seconds)
    } else {
        Duration::Malformed
    }
}

/// The upstream's transcript `text` (§1 "Transcript `text`").
pub enum Transcript<'a> {
    /// A string, `""` included (silent audio).
    Text(&'a str),
    /// A string above the fallback threshold, elided by the host.
    Elided(&'a Value),
    /// Absent, or not a string.
    Missing,
}

/// The transcript at `body.text`.
pub fn transcript(body: &Map<String, Value>) -> Transcript<'_> {
    match body.get("text") {
        Some(Value::String(text)) => Transcript::Text(text),
        Some(value) if value.get(BLOB_KEY).is_some() => Transcript::Elided(value),
        _ => Transcript::Missing,
    }
}

/// `unknown`: a 2xx without a transcript.
pub fn no_transcript() -> SpeechOutcomeV1 {
    unknown("a 2xx without a transcript")
}

/// `unknown`: a malformed duration.
pub fn malformed_duration() -> SpeechOutcomeV1 {
    unknown("a malformed duration")
}

/// `unknown`: a duration-bearing answer without one.
pub fn unreported_duration() -> SpeechOutcomeV1 {
    unknown("the upstream did not report a duration")
}

/// `unknown`: a 2xx whose JSON body is not an object.
pub fn not_an_object() -> SpeechOutcomeV1 {
    unknown("a 2xx that is not a JSON object")
}

/// A JSON object written with its members in the given order, each value as raw JSON text.
pub fn ordered_object(members: &[(&str, String)]) -> String {
    let mut out = String::from("{");
    for (index, (key, value)) in members.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&Value::String((*key).to_owned()).to_string());
        out.push(':');
        out.push_str(value);
    }
    out.push('}');
    out
}

/// The transcript as a template value: the string, or an `as_is` reference to the elided blob.
pub fn transcript_value(text: &Transcript<'_>) -> Option<String> {
    match text {
        Transcript::Text(text) => Some(Value::String((*text).to_owned()).to_string()),
        Transcript::Elided(value) => Some(with_refs(value).to_string()),
        Transcript::Missing => None,
    }
}

/// A `json` delivery of a template, `unknown` when the template is longer than a delivery holds.
pub fn json_delivery(template: String) -> Result<TranscriptDeliveryV1, SpeechOutcomeV1> {
    if template.len() > MediaLimitsV1::V1.inline_string_bytes {
        return Err(unknown("the rendered transcript is longer than a delivery holds"));
    }
    Ok(TranscriptDeliveryV1::Json { template })
}

/// `{"text": <text>}`, the text as a reference when the host elided it (S-S-1).
pub fn text_only_json(text: &Transcript<'_>) -> Result<TranscriptDeliveryV1, SpeechOutcomeV1> {
    let value = transcript_value(text).ok_or_else(no_transcript)?;
    json_delivery(ordered_object(&[("text", value)]))
}

/// A rendered text document, `unknown` above 1 MiB (a `text` delivery holds at most the fallback
/// threshold).
pub fn text_delivery(
    text: String,
    media_type: &str,
) -> Result<TranscriptDeliveryV1, SpeechOutcomeV1> {
    if text.len() > MediaLimitsV1::V1.inline_string_bytes {
        return Err(unknown("the rendered transcript is longer than a text delivery holds"));
    }
    Ok(TranscriptDeliveryV1::Text { text, media_type: media_type.to_owned() })
}

/// The `text` client format rendered from the transcript; an elided one is `unknown` (a `text`
/// delivery holds no reference).
pub fn plain_text(text: &Transcript<'_>) -> Result<TranscriptDeliveryV1, SpeechOutcomeV1> {
    match text {
        Transcript::Text(text) => text_delivery((*text).to_owned(), TEXT_MEDIA_TYPE),
        Transcript::Elided(_) => {
            Err(unknown("the transcript is longer than a text delivery holds"))
        }
        Transcript::Missing => Err(no_transcript()),
    }
}

/// A succeeded transcript with its seconds.
pub fn succeeded(
    delivery: TranscriptDeliveryV1,
    seconds: Option<(f64, SpeechSecondsBasisV1)>,
) -> SpeechOutcomeV1 {
    SpeechOutcomeV1::Succeeded {
        delivery: SpeechDeliveryV1::Transcript(delivery),
        metering: SpeechMeteringV1 {
            seconds: seconds.map(|(value, basis)| SpeechSecondsMeterV1 { value, basis }),
            ..SpeechMeteringV1::default()
        },
    }
}

/// The media type a text format's document is labelled with when the upstream names none.
pub fn format_media_type(format: &str) -> &'static str {
    match format {
        "srt" => SRT_MEDIA_TYPE,
        "vtt" => VTT_MEDIA_TYPE,
        "json" | "verbose_json" => JSON_MEDIA_TYPE,
        _ => TEXT_MEDIA_TYPE,
    }
}

/// The media type of a passed-through text document (§1, §8 P8): the upstream's `content-type`
/// when it is a speech media type, else its essence when that is one, else the format's type. A
/// label naming JSON is not echoed on a text document (native D4 labelled every answer JSON).
pub fn passthrough_text_media_type(response: &Value, format: &str) -> String {
    let fallback = format_media_type(format);
    let content_type = response["headers"].as_array().and_then(|headers| {
        headers.iter().find_map(|header| {
            let pair = header.as_array()?;
            if pair.first()?.as_str()? == "content-type" { pair.get(1)?.as_str() } else { None }
        })
    });
    let Some(content_type) = content_type else {
        return fallback.to_owned();
    };
    let essence = content_type.split(';').next().unwrap_or_default().trim();
    if essence.eq_ignore_ascii_case(JSON_MEDIA_TYPE) {
        return fallback.to_owned();
    }
    if is_speech_media_type(content_type) {
        content_type.to_owned()
    } else if is_speech_media_type(essence) {
        essence.to_owned()
    } else {
        fallback.to_owned()
    }
}

/// One subtitle cue.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    /// Its start, in seconds.
    pub start: f64,
    /// Its end, in seconds.
    pub end: f64,
    /// Its text, untrimmed.
    pub text: String,
}

/// A finite, non-negative number of seconds.
fn unit_time(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64).filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
}

/// `HH:MM:SS` plus the separator and milliseconds, from seconds rounded to the millisecond.
fn cue_time(seconds: f64, separator: char) -> String {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a finite, non-negative number of seconds"
    )]
    let millis = (seconds * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}{separator}{:03}",
        millis / 3_600_000,
        millis / 60_000 % 60,
        millis / 1000 % 60,
        millis % 1000
    )
}

/// An `srt` or `vtt` document from cues in order (public formats, record §9): an `srt` cue is its
/// 1-based index, `start --> end`, the text trimmed and a blank line; a `vtt` document starts
/// `WEBVTT` and a blank line and its cues have no index.
pub fn render_subtitles(cues: &[Cue], vtt: bool) -> String {
    let mut out = String::from(if vtt { "WEBVTT\n\n" } else { "" });
    let separator = if vtt { '.' } else { ',' };
    for (index, cue) in cues.iter().enumerate() {
        if !vtt {
            let _ = writeln!(out, "{}", index + 1);
        }
        let _ = write!(
            out,
            "{} --> {}\n{}\n\n",
            cue_time(cue.start, separator),
            cue_time(cue.end, separator),
            cue.text.trim()
        );
    }
    out
}

/// A rendered `srt` or `vtt` delivery.
pub fn subtitle_delivery(
    cues: &[Cue],
    format: &str,
) -> Result<TranscriptDeliveryV1, SpeechOutcomeV1> {
    let vtt = format == "vtt";
    text_delivery(render_subtitles(cues, vtt), if vtt { VTT_MEDIA_TYPE } else { SRT_MEDIA_TYPE })
}

/// One cue per `OpenAI` verbose segment (`start`, `end`, `text`); `None` when `segments` is not an
/// array or a segment's times or text are unusable.
pub fn segment_cues(segments: &Value) -> Option<Vec<Cue>> {
    segments
        .as_array()?
        .iter()
        .map(|segment| {
            Some(Cue {
                start: unit_time(segment.get("start"))?,
                end: unit_time(segment.get("end"))?,
                text: segment.get("text")?.as_str()?.to_owned(),
            })
        })
        .collect()
}

/// One timed `ElevenLabs` word.
#[derive(Clone, Debug, PartialEq)]
pub struct Word<'a> {
    /// Its text.
    pub text: &'a str,
    /// Its start, in seconds.
    pub start: f64,
    /// Its end, in seconds.
    pub end: f64,
    /// Its speaker, when the answer names one.
    pub speaker: Option<&'a Value>,
    /// The `spacing` text between the previous word and this one.
    pub spacing_before: String,
}

/// The `word` entries of an `ElevenLabs` `words` list (an entry without `type` counts as a word),
/// each with the `spacing` text before it; `audio_event` and other entries are skipped. `None` when
/// a word's times or text, or a spacing's text, are unusable.
pub fn elevenlabs_words(words: &[Value]) -> Option<Vec<Word<'_>>> {
    let mut out = Vec::new();
    let mut spacing = String::new();
    for entry in words {
        match entry.get("type").and_then(Value::as_str) {
            None | Some("word") => {
                out.push(Word {
                    text: entry.get("text")?.as_str()?,
                    start: unit_time(entry.get("start"))?,
                    end: unit_time(entry.get("end"))?,
                    speaker: entry.get("speaker_id"),
                    spacing_before: std::mem::take(&mut spacing),
                });
            }
            Some("spacing") => spacing.push_str(entry.get("text")?.as_str()?),
            Some(_) => {}
        }
    }
    Some(out)
}

/// SV-Q8: `ElevenLabs` words grouped into cues. A cue closes after a word ending (trailing
/// whitespace aside) in sentence punctuation, and before a word that starts at least
/// [`CUE_GAP_SECONDS`] after the previous one ends, that names another speaker, or that would make
/// the cue last longer than [`CUE_MAX_SECONDS`] or its text longer than [`CUE_MAX_CHARACTERS`]
/// characters. Spacing joins words within a cue and is dropped between cues.
pub fn word_cues(words: &[Word<'_>]) -> Vec<Cue> {
    let mut cues = Vec::new();
    let mut open: Option<(Cue, Option<&Value>)> = None;
    for word in words {
        if let Some((cue, speaker)) = &mut open {
            let candidate = format!("{}{}{}", cue.text, word.spacing_before, word.text);
            let closes = word.start - cue.end >= CUE_GAP_SECONDS
                || word.speaker != *speaker
                || word.end - cue.start > CUE_MAX_SECONDS
                || candidate.trim().chars().count() > CUE_MAX_CHARACTERS;
            if closes {
                if let Some((cue, _)) = open.take() {
                    cues.push(cue);
                }
            } else {
                cue.text = candidate;
                cue.end = word.end;
            }
        }
        if open.is_none() {
            open = Some((
                Cue { start: word.start, end: word.end, text: word.text.to_owned() },
                word.speaker,
            ));
        }
        if word.text.trim_end().ends_with(CUE_SENTENCE_END)
            && let Some((cue, _)) = open.take()
        {
            cues.push(cue);
        }
    }
    if let Some((cue, _)) = open {
        cues.push(cue);
    }
    cues
}

/// The `OpenAI` audio protocol of `speech-openai-compatible` and `speech-xai` (§2, §3).
pub struct OpenAiAsr<'a> {
    /// The upstream path of the operation.
    pub path: String,
    /// The upstream query, when the family's route has one (`speech-openai-compatible`'s
    /// `azure-openai-dated`, the sanctioned `api-version`).
    pub query: Option<Value>,
    /// Whether every `model` text part is removed (`speech-xai`) instead of set to the upstream id.
    pub drops_model: bool,
    /// Whether the package translates (`speech-openai-compatible`).
    pub translates: bool,
    /// The auth arm.
    pub auth: Option<Value>,
    /// The routed upstream model.
    pub upstream_model: &'a str,
}

/// The `immutable_form_fields` of both `OpenAI` audio packages (native `audio.rs:76-77`).
pub const OPENAI_IMMUTABLE_FORM_FIELDS: [&str; 4] = ["model", "file", "response_format", "stream"];

/// `prepare` of a recognition call over the `OpenAI` audio protocol (§2, §3).
pub fn prepare_openai(
    config: &ProviderConfig,
    request: &Value,
    context: &SpeechCallContextV1,
    protocol: &OpenAiAsr<'_>,
) -> ComponentResultV1<PreparedSpeechCallV1> {
    let forms = recognition_forms(config, context, protocol.translates)?;
    let parts = request_parts(request)?;
    let format = openai_format(parts)?;
    // A client format that carries no duration (`text`, `srt`, `vtt`) is always asked of the upstream
    // as `verbose_json` and rendered here, as a `speech.verbose_json` row is (#293, lv 2026-10-28):
    // otherwise the answer has no duration and the host can only estimate the seconds it bills.
    // `json` and `verbose_json` are asked as sent; their answers carry the duration themselves.
    let verbose =
        verbose_row(config, protocol.upstream_model)? || matches!(format, "text" | "srt" | "vtt");
    let mut plan = PartPlan::default();
    if protocol.drops_model {
        plan.dropped_text = &["model"];
    } else {
        plan.written.push(("model", "model", protocol.upstream_model.to_owned()));
    }
    if verbose {
        plan.written.push(("response_format", "response_format", "verbose_json".to_owned()));
    }
    let upstream_parts = rewrite_parts(parts, &plan)?;
    let form = if verbose || matches!(format, "json" | "verbose_json") {
        ResponseBodyFormV1::Json
    } else {
        ResponseBodyFormV1::Text
    };
    Ok(prepared(
        context.operation,
        forms,
        multipart_descriptor(
            &protocol.path,
            protocol.query.clone(),
            protocol.auth.clone(),
            &upstream_parts,
            &[],
        )?,
        form,
        &OPENAI_IMMUTABLE_FORM_FIELDS,
        &json!({ "op": operation_word(context.operation), "format": format, "verbose": verbose }),
    ))
}

/// The 2xx of a recognition call over the `OpenAI` audio protocol (§2, §3). `usage_seconds`: a
/// non-verbose `json` answer without `duration` may report `usage.seconds` with `usage.type`
/// `"duration"` (`speech-openai-compatible` only).
pub fn openai_outcome(state: &Value, response: &Value, usage_seconds: bool) -> SpeechOutcomeV1 {
    let format = state["format"].as_str().unwrap_or("json");
    if state["verbose"].as_bool().unwrap_or(false) {
        return verbose_outcome(format, response);
    }
    if !matches!(format, "json" | "verbose_json") {
        // `prepare_openai` no longer prepares this state (#293: a text format is always asked as
        // `verbose_json`). Should a state still name a text format without `verbose`, the upstream
        // answered that format itself: delivered unchanged, and no text format carries a duration
        // (record §9 case 1).
        return succeeded(
            TranscriptDeliveryV1::Passthrough {
                media_type: passthrough_text_media_type(response, format),
            },
            None,
        );
    }
    let Some(body) = json_object(response) else { return not_an_object() };
    let seconds = match duration(body.get("duration")) {
        Duration::Reported(seconds) => Some(seconds),
        Duration::Malformed => return malformed_duration(),
        Duration::Absent
            if usage_seconds
                && body.get("usage").is_some_and(|usage| usage["type"] == "duration") =>
        {
            match duration(body.get("usage").and_then(|usage| usage.get("seconds"))) {
                Duration::Reported(seconds) => Some(seconds),
                Duration::Malformed => return malformed_duration(),
                Duration::Absent => None,
            }
        }
        Duration::Absent => None,
    };
    if matches!(transcript(body), Transcript::Missing) {
        return no_transcript();
    }
    succeeded(
        TranscriptDeliveryV1::Passthrough { media_type: JSON_MEDIA_TYPE.to_owned() },
        seconds.map(|seconds| (seconds, SpeechSecondsBasisV1::Reported)),
    )
}

/// A verbose row's answer, rendered in the client's format (§2 "Verbose rows").
fn verbose_outcome(format: &str, response: &Value) -> SpeechOutcomeV1 {
    let Some(body) = json_object(response) else { return not_an_object() };
    let seconds = match duration(body.get("duration")) {
        Duration::Reported(seconds) => seconds,
        Duration::Malformed => return malformed_duration(),
        Duration::Absent => return unreported_duration(),
    };
    let text = transcript(body);
    let delivery = match format {
        "text" => plain_text(&text),
        "verbose_json" => match text {
            Transcript::Missing => Err(no_transcript()),
            _ => Ok(TranscriptDeliveryV1::Passthrough { media_type: JSON_MEDIA_TYPE.to_owned() }),
        },
        "srt" | "vtt" => body.get("segments").and_then(segment_cues).map_or_else(
            || Err(unknown("a 2xx without the segments a subtitle format needs")),
            |cues| subtitle_delivery(&cues, format),
        ),
        _ => text_only_json(&text),
    };
    match delivery {
        Ok(delivery) => succeeded(delivery, Some((seconds, SpeechSecondsBasisV1::Reported))),
        Err(outcome) => outcome,
    }
}
