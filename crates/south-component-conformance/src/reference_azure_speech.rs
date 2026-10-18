//! The native reference implementation of the `speech-azure` component.
//!
//! Azure Speech fast transcription (speech world batch V3-3, design record
//! `docs/design/2026-10-14-speech-v3-3-asr.md` §5; speech record §12 ASR row 4) and, from speech
//! world batch V4 (`docs/design/2026-10-15-speech-v4-tts.md` §4; speech record §12 row 9), SSML
//! synthesis (SV-Q1 (b): the same package, still at the unpublished 1.0.0). Transcribed from
//! token-station-server's native arms `AsrUpstream::AzureSpeech` (`handler/audio.rs:96-97`,
//! `:281-283`, `:534-551`; `handler/audio/multipart.rs:291-446`) and `TtsUpstream::AzureSpeech`
//! (`handler/audio/tts.rs:586-614`, `:1148-1173`; `tts_providers.rs:82-257`).
//!
//! **Synthesis** (record §4):
//!
//! - **Request**: `POST cognitiveservices/v1` under the row endpoint, the secret in
//!   `ocp-apim-subscription-key`, a `text` body of media type `application/ssml+xml` (the host's
//!   text executor renders the one `content-type`), and `x-microsoft-outputformat` from the format
//!   table: `mp3` (or absent) `audio-24khz-48kbitrate-mono-mp3` / `audio/mpeg`, `opus`
//!   `ogg-24khz-16bit-mono-opus` / `audio/ogg`, `wav` `riff-24khz-16bit-mono-pcm` / `audio/wav`,
//!   `pcm` `raw-24khz-16bit-mono-pcm` / `audio/pcm`, matched exactly (native, record §8 item 9);
//!   anything else refused with native's text.
//! - **Voice** (`map_openai_voice_to_mai`): the client's string trimmed, else `alloy`; a value with
//!   `:` is a full voice id; else the case-sensitive roster table, then `{name}:{upstream id}` when
//!   the name has a `lang-REGION-` prefix, else `en-US-{name}:{upstream id}`. An elided voice is
//!   refused.
//! - **SSML** (`build_mai_ssml`): `<speak version='1.0' xml:lang='{lang}'><voice
//!   name='{voice}'>{text}</voice></speak>`, the three values escaped by native's `xml_escape`,
//!   which drops the characters XML 1.0 forbids; no declaration, no `xmlns` (record §8 item 8). A
//!   document above the 1 MiB text body bound is refused before dispatch. No body path is
//!   immutable: request extras never touch SSML (`immutable_body_paths` `null`).
//! - **Response**: `binary`; an empty 2xx is `unknown`; otherwise the body, typed by the format
//!   table rather than the upstream's `content-type`, billed by the request's characters.
//!
//! **Transcription** (V3-3):
//!
//! - **Family and auth**: `azure-speech`, the secret in `ocp-apim-subscription-key`
//!   (`header_secret`).
//! - **Request**: `POST speechtotext/transcriptions:transcribe?api-version=2025-10-15` under the row
//!   endpoint (the sanctioned `api-version` parameter). Every file part named `file` becomes `audio`;
//!   every part named `model`, `language`, `prompt`, `temperature`, `response_format`,
//!   `timestamp_granularities[]` (native `AZURE_ASR_DROPPED_FIELDS`) or `definition` (§7 D9) is
//!   dropped; everything else is forwarded in order; the text part `definition`,
//!   `{"enhancedMode":{"enabled":true,"model":<upstream id>}}`, is appended with native's bytes.
//! - **Client format** (native `azure_asr_response_format`), trimmed: absent, `""` or `json`;
//!   `verbose_json`; `text`. `srt` / `vtt` and any other value are refused before dispatch with
//!   native's texts. The common refusals (a body that is not multipart, an elided text part, no file
//!   part `file`, `stream=true` / `1`) come first.
//! - **Response**: a JSON object (else `unknown`). Duration from `durationMilliseconds` / 1000, else
//!   `duration`, by the value rule; absent is `unknown` (native 502 and park). Text: the string
//!   `text` of every `combinedPhrases` entry joined by one space, else a top-level string `text`,
//!   else `""`; an elided phrase is `unknown`. Rendered as native `render_azure_asr_response`:
//!   `json` `{"text", "duration"}`, `verbose_json` `{"task": "transcribe", "duration", "text"}`,
//!   `text` as `text/plain; charset=utf-8`.
//! - **Immutable form fields** `audio`, `file`, `definition` (native `audio.rs:79`).
//! - **Errors**: the `OpenAI` shape's `message` (Azure Cognitive Services', **unverified**), else
//!   the body text, as the provider message.

#![allow(
    clippy::result_large_err,
    reason = "a rendering step fails with the outcome the call ends with, built once per round"
)]

use serde_json::{Map, Value, json};
use south_contracts::media::{
    MAX_MEDIA_TEXT_BODY_BYTES, MediaLimitsV1, MediaRequestDescriptorV1, ResponseBodyFormV1,
};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechCharactersV1, SpeechFactsV1,
    SpeechMeteringFormV1, SpeechModelCapabilitiesV1, SpeechOperationV1, SpeechSecondsBasisV1,
    TranscriptDeliveryV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::ProviderConfig;

use crate::asr_shared::{
    Duration, PartPlan, TEXT_MEDIA_TYPE, duration_of, json_delivery, malformed_duration,
    multipart_descriptor, not_an_object, ordered_object, prepared, recognition_forms,
    request_parts, rewrite_parts, succeeded, text_delivery, unreported_duration,
};
use crate::speech_shared::{
    BLOB_KEY, declared_body_outcome, input, internal, invalid, is_blob, json_body, json_object,
    non_2xx, openai_error_message, required_forms, scalar_count, serves, unknown, view_status,
};
use crate::{ComponentResultV1, SpeechComponentV1, SpeechOutcomeV1};

/// The package name this reference is published as.
pub const NAME: &str = "speech-azure";
/// The package version.
pub const VERSION: &str = "1.0.9";
/// The world this reference exports.
pub const WORLD: &str = "speech-adapter-v1";
/// The family, the provider type's word.
pub const AZURE_SPEECH: &str = "azure-speech";
/// The header the secret travels in (native `upstream.rs:197-201`; one of the closed
/// `SecretHeaderV1` names).
pub const SECRET_HEADER: &str = "ocp-apim-subscription-key";
/// The fast-transcription path (native `audio.rs:281-283`).
const TRANSCRIBE_PATH: &str = "speechtotext/transcriptions:transcribe";
/// The `api-version` native sends.
pub const API_VERSION: &str = "2025-10-15";
/// Native `AZURE_ASR_DROPPED_FIELDS`, and the client `definition` the component writes itself
/// (§7 D9, §8 P14).
const DROPPED_PARTS: [&str; 7] = [
    "model",
    "language",
    "prompt",
    "temperature",
    "response_format",
    "timestamp_granularities[]",
    "definition",
];
/// The form fields the gateway writes (native `audio.rs:79`).
const IMMUTABLE_FORM_FIELDS: [&str; 3] = ["audio", "file", "definition"];
/// The synthesis path (native `tts.rs:613`).
const SYNTHESIZE_PATH: &str = "cognitiveservices/v1";
/// The header the output format travels in (native `tts.rs:608-611`, lower-cased).
pub const OUTPUT_FORMAT_HEADER: &str = "x-microsoft-outputformat";
/// The SSML media type the text body declares.
const SSML: &str = "application/ssml+xml";
/// Synthesis reports characters (record §4.1).
const SYNTHESIS_FORMS: [SpeechMeteringFormV1; 1] = [SpeechMeteringFormV1::Characters];
/// The voice an absent, non-string or blank `voice` resolves to (native `tts.rs:592-597`).
const DEFAULT_VOICE: &str = "alloy";

/// Native `azure_tts_output_format`, matched exactly: the `X-Microsoft-OutputFormat` token and the
/// delivered media type of a format word.
fn output_format(word: &str) -> Option<(&'static str, &'static str)> {
    Some(match word {
        "mp3" => ("audio-24khz-48kbitrate-mono-mp3", "audio/mpeg"),
        "opus" => ("ogg-24khz-16bit-mono-opus", "audio/ogg"),
        "wav" => ("riff-24khz-16bit-mono-pcm", "audio/wav"),
        "pcm" => ("raw-24khz-16bit-mono-pcm", "audio/pcm"),
        _ => return None,
    })
}

/// The client's `response_format` as a table word: absent or not a string is `mp3`.
fn synthesis_format(body: &Map<String, Value>) -> ComponentResultV1<&'static str> {
    match body.get("response_format") {
        Some(value) if is_blob(value) => {
            Err(invalid("'response_format' is longer than the gateway accepts"))
        }
        Some(Value::String(format)) => match format.as_str() {
            "mp3" => Ok("mp3"),
            "opus" => Ok("opus"),
            "wav" => Ok("wav"),
            "pcm" => Ok("pcm"),
            other => Err(invalid(format!(
                "response_format '{other}' is not supported for MAI-Voice-2 (Azure Speech); use \
                 mp3, opus, wav, or pcm"
            ))),
        },
        _ => Ok("mp3"),
    }
}

/// The client's voice (native `tts.rs:592-597`): a string, trimmed, else `alloy`; an elided voice
/// cannot be a roster name and is refused.
fn client_voice(body: &Map<String, Value>) -> ComponentResultV1<&str> {
    match body.get("voice") {
        Some(value) if is_blob(value) => Err(invalid("'voice' is longer than the gateway accepts")),
        Some(Value::String(voice)) if !voice.trim().is_empty() => Ok(voice.trim()),
        _ => Ok(DEFAULT_VOICE),
    }
}

/// Native `has_locale_prefix`: a 2-3 letter lower-case language, a non-empty region of upper-case
/// letters and digits, and a non-empty rest, separated by `-`.
fn has_locale_prefix(name: &str) -> bool {
    let mut segments = name.split('-');
    matches!(
        (segments.next(), segments.next(), segments.next()),
        (Some(lang), Some(region), Some(rest))
            if (2..=3).contains(&lang.len())
                && lang.chars().all(|c| c.is_ascii_lowercase())
                && !region.is_empty()
                && region.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                && !rest.is_empty()
    )
}

/// Native `map_openai_voice_to_mai`: the full SSML voice id.
#[must_use]
pub fn voice_id(voice: &str, upstream_model: &str) -> String {
    if voice.contains(':') {
        return voice.to_owned();
    }
    let name = match voice {
        "alloy" | "sage" => "June",
        "echo" | "ash" => "Grant",
        "fable" | "ballad" => "Jasper",
        "onyx" | "verse" => "Reed",
        "nova" | "coral" => "Iris",
        "shimmer" => "Joy",
        other => other,
    };
    if has_locale_prefix(name) {
        format!("{name}:{upstream_model}")
    } else {
        format!("en-US-{name}:{upstream_model}")
    }
}

/// Native `voice_locale`: the voice id's leading `lang-REGION` pair, else `en-US`.
fn voice_locale(voice_id: &str) -> String {
    let name = voice_id.split(':').next().unwrap_or(voice_id);
    let mut segments = name.split('-');
    match (segments.next(), segments.next()) {
        (Some(lang), Some(region))
            if !lang.is_empty()
                && lang.chars().all(|c| c.is_ascii_alphabetic())
                && !region.is_empty()
                && region.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            format!("{lang}-{region}")
        }
        _ => "en-US".to_owned(),
    }
}

/// Native `xml_escape`, for text and attribute values alike.
///
/// The five XML metacharacters become entities, and the characters XML 1.0's `Char` production
/// forbids (U+0000 to U+0008, U+000B, U+000C, U+000E to U+001F, U+FFFE, U+FFFF)
/// dropped; tab, line feed and carriage return kept.
#[must_use]
pub fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\u{0000}'..='\u{0008}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000E}'..='\u{001F}'
            | '\u{FFFE}'
            | '\u{FFFF}' => {}
            _ => out.push(character),
        }
    }
    out
}

/// Native `build_mai_ssml`, exactly.
#[must_use]
pub fn ssml(text: &str, voice_id: &str) -> String {
    format!(
        "<speak version='1.0' xml:lang='{}'><voice name='{}'>{}</voice></speak>",
        xml_escape(&voice_locale(voice_id)),
        xml_escape(voice_id),
        xml_escape(text)
    )
}

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct AzureSpeechReferenceV1;

/// Native `azure_asr_response_format` over the first `response_format` part, trimmed.
fn client_format(parts: &[Value]) -> ComponentResultV1<&'static str> {
    match crate::asr_shared::first_text(parts, "response_format").map(str::trim) {
        None | Some("" | "json") => Ok("json"),
        Some("verbose_json") => Ok("verbose_json"),
        Some("text") => Ok("text"),
        Some(other @ ("srt" | "vtt")) => Err(invalid(format!(
            "response_format '{other}' is not supported for Azure MAI transcription: the upstream \
             reports no per-segment timestamps to build subtitle cues from. Supported: json \
             (default), verbose_json, text."
        ))),
        Some(other) => Err(invalid(format!(
            "Unknown response_format '{other}' for Azure MAI transcription. Supported: json \
             (default), verbose_json, text."
        ))),
    }
}

/// The `definition` part as native writes it (`multipart.rs:315-318`): compact, members in this
/// order, the upstream id escaped as `serde_json` escapes a string.
#[must_use]
pub fn definition(upstream_model: &str) -> String {
    format!(
        r#"{{"enhancedMode":{{"enabled":true,"model":{}}}}}"#,
        Value::String(upstream_model.to_owned())
    )
}

/// The duration (Q6, native `multipart.rs:439-444`): `durationMilliseconds` as a number divided by
/// 1000, else `duration` as a number, then the value rule.
fn reported_duration(body: &Map<String, Value>) -> Duration {
    if let Some(millis) = body.get("durationMilliseconds").and_then(Value::as_f64) {
        return duration_of(millis / 1000.0);
    }
    if let Some(seconds) = body.get("duration").filter(|value| value.is_number()) {
        return crate::asr_shared::duration(Some(seconds));
    }
    let present = |key: &str| body.get(key).is_some_and(|value| !value.is_null());
    if present("durationMilliseconds") || present("duration") {
        Duration::Malformed
    } else {
        Duration::Absent
    }
}

/// The transcript (native `multipart.rs:422-438`): the string `text` members of
/// `combinedPhrases[]` joined by one space; when that is empty, a top-level string `text`; else
/// `""`. `None` when a phrase or the top-level text needed was elided by the host.
fn transcript_text(body: &Map<String, Value>) -> Option<String> {
    let elided = |value: &Value| value.get(BLOB_KEY).is_some();
    let mut phrases = Vec::new();
    if let Some(entries) = body.get("combinedPhrases").and_then(Value::as_array) {
        for entry in entries {
            match entry.get("text") {
                Some(Value::String(text)) => phrases.push(text.as_str()),
                Some(value) if elided(value) => return None,
                _ => {}
            }
        }
    }
    let joined = phrases.join(" ");
    if !joined.is_empty() {
        return Some(joined);
    }
    match body.get("text") {
        Some(Value::String(text)) => Some(text.clone()),
        Some(value) if elided(value) => None,
        _ => Some(String::new()),
    }
}

/// Native `render_azure_asr_response` in the client's format.
fn render(
    format: &str,
    text: String,
    seconds: f64,
) -> Result<TranscriptDeliveryV1, SpeechOutcomeV1> {
    let text_json = || Value::String(text.clone()).to_string();
    let duration = json!(seconds).to_string();
    match format {
        "verbose_json" => json_delivery(ordered_object(&[
            ("task", "\"transcribe\"".to_owned()),
            ("duration", duration),
            ("text", text_json()),
        ])),
        "text" => text_delivery(text, TEXT_MEDIA_TYPE),
        _ => json_delivery(ordered_object(&[("text", text_json()), ("duration", duration)])),
    }
}

/// `prepare` for `synthesize` (record §4.2): the input, the format, the voice, then the bound.
fn prepare_synthesis(
    config: &ProviderConfig,
    request: &Value,
    context: &SpeechCallContextV1,
) -> ComponentResultV1<PreparedSpeechCallV1> {
    let forms = required_forms(context, &SYNTHESIS_FORMS)?;
    let body = json_body(request)?;
    let text = input(body)?;
    let format = synthesis_format(body)?;
    let voice = client_voice(body)?;
    let document = ssml(text, &voice_id(voice, &context.upstream_model));
    if document.len() > MAX_MEDIA_TEXT_BODY_BYTES {
        return Err(invalid("'input' is longer than the gateway accepts"));
    }
    let (token, media_type) =
        output_format(format).ok_or_else(|| internal("a format word without a table row"))?;
    let mut wire = Map::new();
    wire.insert("method".to_owned(), json!("POST"));
    wire.insert("path".to_owned(), json!(SYNTHESIZE_PATH));
    wire.insert("headers".to_owned(), json!([{ "name": OUTPUT_FORMAT_HEADER, "value": token }]));
    if let Some(slot) = &config.auth {
        wire.insert(
            "auth".to_owned(),
            json!({ "arm": "header_secret", "header": SECRET_HEADER, "slot": slot.as_str() }),
        );
    }
    wire.insert("body".to_owned(), json!({ "text": { "media_type": SSML, "text": document } }));
    let descriptor =
        MediaRequestDescriptorV1::parse(&Value::Object(wire).to_string(), &MediaLimitsV1::V1)
            .map_err(|error| internal(format!("built an invalid descriptor: {error}")))?;
    let count = scalar_count(text);
    Ok(PreparedSpeechCallV1 {
        facts: SpeechFactsV1 {
            operation: SpeechOperationV1::Synthesize,
            characters: Some(SpeechCharactersV1 { count, upper: Some(count) }),
            metering_forms: forms,
            reservation: None,
            output_media_type: Some(media_type.to_owned()),
        },
        descriptor,
        response_body_form: ResponseBodyFormV1::Binary,
        response_elision_paths: Vec::new(),
        // Record §7: the host must not inject body extras into SSML.
        immutable_body_paths: None,
        immutable_form_fields: None,
        state: json!({ "op": "synthesize", "format": format, "characters": count }).to_string(),
    })
}

impl SpeechComponentV1 for AzureSpeechReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: NAME.to_owned(),
            version: VERSION.to_owned(),
            api_version: WORLD.to_owned(),
        }
    }

    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<SpeechModelCapabilitiesV1>> {
        crate::asr_shared::capabilities(config, AZURE_SPEECH, &SYNTHESIS_FORMS, false)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        serves(config, AZURE_SPEECH)?;
        if context.operation == SpeechOperationV1::Synthesize {
            return prepare_synthesis(config, request, context);
        }
        let forms = recognition_forms(config, context, false)?;
        let parts = request_parts(request)?;
        let format = client_format(parts)?;
        let plan = PartPlan {
            renamed_file: &[("file", "audio")],
            dropped: &DROPPED_PARTS,
            appended: vec![("definition", definition(&context.upstream_model))],
            ..PartPlan::default()
        };
        let upstream_parts = rewrite_parts(parts, &plan)?;
        let auth = config.auth.as_ref().map(|slot| {
            json!({ "arm": "header_secret", "header": SECRET_HEADER, "slot": slot.as_str() })
        });
        Ok(prepared(
            context.operation,
            forms,
            multipart_descriptor(
                TRANSCRIBE_PATH,
                Some(json!([{ "name": "api-version", "value": API_VERSION }])),
                auth,
                &upstream_parts,
                &[],
            )?,
            ResponseBodyFormV1::Json,
            &IMMUTABLE_FORM_FIELDS,
            &json!({ "op": "transcribe", "format": format }),
        ))
    }

    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        let status = view_status(response);
        if !(200..300).contains(&status) {
            return Ok(non_2xx(status, response, openai_error_message));
        }
        if state["op"] == "synthesize" {
            let format = state["format"].as_str().unwrap_or_default();
            let (_, media_type) =
                output_format(format).ok_or_else(|| internal("the state has no audio format"))?;
            return declared_body_outcome(state, response, media_type);
        }
        let Some(body) = json_object(response) else { return Ok(not_an_object()) };
        let seconds = match reported_duration(body) {
            Duration::Reported(seconds) => seconds,
            Duration::Malformed => return Ok(malformed_duration()),
            Duration::Absent => return Ok(unreported_duration()),
        };
        let Some(text) = transcript_text(body) else {
            return Ok(unknown("a transcript phrase is longer than a delivery holds"));
        };
        if text.len() > MediaLimitsV1::V1.inline_string_bytes {
            return Ok(unknown("the rendered transcript is longer than a delivery holds"));
        }
        let format = state["format"].as_str().unwrap_or("json");
        Ok(match render(format, text, seconds) {
            Ok(delivery) => succeeded(delivery, Some((seconds, SpeechSecondsBasisV1::Reported))),
            Err(outcome) => outcome,
        })
    }
}
