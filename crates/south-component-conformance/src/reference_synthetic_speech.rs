//! The native reference of the speech suite (`south.speech-component.v1`, speech record §11, step
//! S-S-3): a **synthetic** family, `synthetic-speech`, whose upstream API no vendor serves.
//!
//! The record asks for "a native reference" that passes every row; no single real speech dialect
//! exercises them all (SSML is one vendor's, `verbose_json` another's, the second hop a third's,
//! the token-priced event stream a fourth's), and the real packages are step S-S-4. So this
//! reference is a test double in the manner of the `t21-unseen-*` guests: one family whose models
//! each take one of the record's shapes, chosen by the model id's prefix — the only place in this
//! repository where a component picks behaviour from a model id, which a real package must not do
//! (§4: per-model operations come from the dialect words on the row). Nothing here is transcribed
//! from a native arm, and no host routes to it.
//!
//! | Model prefix | Request | Response form | Artifact or delivery | Metering |
//! |---|---|---|---|---|
//! | `tts-stream` | JSON to `v1/speech` | `binary`; `sse` (`stream_format: "sse"`) when the host requires `tokens` | `body`; `segments` over every `speech.audio.delta` event's `delta` | `characters` from the request; `tokens` from the `speech.audio.done` event's `usage` |
//! | `tts-inline` | JSON to `v1/speech/inline` | `json` | `inline` at `/audio/data`: `hex` mp3, `base64` PCM wrapped as WAV (24 kHz mono) or delivered as `audio/L16; rate=24000` | `characters` from `usage.characters`, else from the request |
//! | `tts-url` | JSON to `v1/speech/url` | `json` | `url` at `/output/audio/url` (second hop, `artifact_fetch`) | as `tts-inline` |
//! | `tts-ssml` | SSML to `v1/speech/ssml` | `binary` | `body` | `characters` from the request |
//! | `asr` | multipart to `v1/audio/transcriptions` or `/translations` | `json`, or `text` for a plain-text answer | the client's format, rendered | `seconds` |
//!
//! - **Synthesis** reads the northbound speech request `{input, voice?, response_format?}`. A
//!   missing or elided `input`, a format the model does not deliver, or (SSML) a missing `voice` is
//!   refused before admission. `characters.count` is the input's scalar count (Q5); `tts-inline`
//!   also declares `upper`, counting each non-ASCII scalar twice.
//! - **SSML** is `<speak version='1.0' xml:lang='{lang}'><voice name='{voice}'>{input}</voice>
//!   </speak>`, with the input and both attribute values escaped (§5); `lang` is the voice's first
//!   two `-` segments when it has three or more, else `en-US`. The body declares no immutable
//!   paths (`null`, §7).
//! - **Recognition** reads the multipart view: a `file` part, and `response_format` (absent is
//!   `json`). A model whose row carries `speech.verbose_json` always asks the upstream for
//!   `verbose_json` and renders `json` (`{"text"}`), `text`, `verbose_json` (the upstream body),
//!   `srt` and `vtt` (from `segments`) itself, billing the reported `duration`; a 2xx without one
//!   is `unknown`, since that format always carries it (§9, Q6). Any other model forwards `json` or
//!   `text` and bills `usage.seconds` when the JSON answer has it, `seconds: null` otherwise (the
//!   host's estimate, §9). `translate` is served only on a row carrying `speech.translate`.
//! - **Responses**: a 4xx other than 408 is `rejected`; any other non-2xx is `unknown`; a 2xx
//!   without audio, without a required token count, or with a malformed count is `unknown`.

use std::fmt::Write as _;

use serde_json::{Map, Value, json};
use south_contracts::JsonPointerV1;
use south_contracts::media::{
    MAX_MEDIA_TEXT_BODY_BYTES, MediaLimitsV1, MediaPartV1, MediaRequestDescriptorV1,
    MediaTransformV1, PathPatternV1, ResponseBodyFormV1,
};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechArtifactFormV1, SpeechArtifactV1, SpeechCallContextV1,
    SpeechCharacterMeterV1, SpeechCharacterSourceV1, SpeechCharactersV1, SpeechContainerV1,
    SpeechDialectWordV1, SpeechEncodingV1, SpeechFactsV1, SpeechMeteringFormV1, SpeechMeteringV1,
    SpeechModelCapabilitiesV1, SpeechOperationV1, SpeechSecondsBasisV1, SpeechSecondsMeterV1,
    SpeechSegmentV1, SpeechTokensV1, TranscriptDeliveryV1, speech_dialect_words_v1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

use crate::{ComponentResultV1, SpeechComponentV1, SpeechDeliveryV1, SpeechOutcomeV1};

/// The package name the reference reports.
pub const NAME: &str = "synthetic-speech";
/// The package version.
pub const VERSION: &str = "1.0.0";
/// The world this reference exports.
pub const WORLD: &str = "speech-adapter-v1";
/// The family, as a host's speech rows would name it.
pub const SYNTHETIC_SPEECH: &str = "synthetic-speech";

const BLOB_KEY: &str = "$south.blob";
const DELTA_EVENT: &str = "speech.audio.delta";
const DONE_EVENT: &str = "speech.audio.done";
const SSML: &str = "application/ssml+xml";
const PCM_RATE: u32 = 24_000;

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct SyntheticSpeechReferenceV1;

/// The shape a model takes, from its id's prefix.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Stream,
    Inline,
    Url,
    Ssml,
    Recognition,
}

impl Mode {
    fn of(model: &str) -> Option<Self> {
        [
            ("tts-stream", Self::Stream),
            ("tts-inline", Self::Inline),
            ("tts-url", Self::Url),
            ("tts-ssml", Self::Ssml),
            ("asr", Self::Recognition),
        ]
        .into_iter()
        .find_map(|(prefix, mode)| model.starts_with(prefix).then_some(mode))
    }
}

fn invalid(message: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, message)
}

fn capability(message: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Capability, 400, message)
}

fn internal(message: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, message)
}

fn unknown(reason: &str) -> SpeechOutcomeV1 {
    SpeechOutcomeV1::Unknown { reason: reason.to_owned(), error: None }
}

fn is_blob(value: &Value) -> bool {
    value.get(BLOB_KEY).is_some()
}

fn pointer(text: &str) -> ComponentResultV1<JsonPointerV1> {
    JsonPointerV1::parse(text).map_err(|_| internal("built an invalid pointer"))
}

/// One model's operations and metering forms, from its mode and its row's dialect words.
fn declaration(
    model: &str,
    words: &[SpeechDialectWordV1],
) -> Option<(Mode, Vec<SpeechOperationV1>, Vec<SpeechMeteringFormV1>)> {
    let mode = Mode::of(model)?;
    Some(match mode {
        Mode::Stream => (
            mode,
            vec![SpeechOperationV1::Synthesize],
            vec![SpeechMeteringFormV1::Characters, SpeechMeteringFormV1::Tokens],
        ),
        Mode::Inline | Mode::Url | Mode::Ssml => {
            (mode, vec![SpeechOperationV1::Synthesize], vec![SpeechMeteringFormV1::Characters])
        }
        Mode::Recognition => {
            let mut operations = vec![SpeechOperationV1::Transcribe];
            if words.contains(&SpeechDialectWordV1::Translate) {
                operations.push(SpeechOperationV1::Translate);
            }
            (mode, operations, vec![SpeechMeteringFormV1::Seconds])
        }
    })
}

/// The dialect words on the configuration's row for `model`; none when it has no row.
fn words_of(config: &ProviderConfig, model: &str) -> ComponentResultV1<Vec<SpeechDialectWordV1>> {
    let Some(row) = config.models.iter().find(|row| row.model == model) else {
        return Ok(Vec::new());
    };
    speech_dialect_words_v1(row.supported_parameters.iter().map(String::as_str))
        .map(|words| words.into_iter().collect())
        .map_err(|_| invalid("the model row carries a misspelt speech dialect word"))
}

fn auth_json(config: &ProviderConfig) -> String {
    config.auth.as_ref().map_or_else(String::new, |slot| {
        format!(
            r#","auth":{{"arm":"bearer","slot":{}}}"#,
            serde_json::to_string(slot.as_str()).unwrap_or_default()
        )
    })
}

fn descriptor(
    config: &ProviderConfig,
    path: &str,
    body: &str,
) -> ComponentResultV1<MediaRequestDescriptorV1> {
    let text = format!(r#"{{"method":"POST","path":"{path}"{},"body":{body}}}"#, auth_json(config));
    MediaRequestDescriptorV1::parse(&text, &MediaLimitsV1::V1)
        .map_err(|error| internal(format!("built an invalid descriptor: {error}")))
}

fn elision(paths: &[&str]) -> ComponentResultV1<Vec<PathPatternV1>> {
    paths
        .iter()
        .map(|path| PathPatternV1::parse(path).map_err(|_| internal("invalid elision path")))
        .collect()
}

/// XML escaping for both character data and attribute values (§5).
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\'' => out.push_str("&apos;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

/// The `xml:lang` a voice implies: its first two `-` segments when it has three or more.
fn voice_lang(voice: &str) -> String {
    let segments: Vec<&str> = voice.split('-').collect();
    if segments.len() >= 3 {
        format!("{}-{}", segments[0], segments[1])
    } else {
        "en-US".to_owned()
    }
}

/// The synthesis request: `input`, `voice` and `response_format` (default `mp3`).
struct Synthesis<'a> {
    input: &'a str,
    voice: Option<&'a str>,
    format: &'a str,
}

fn synthesis(request: &Value) -> ComponentResultV1<Synthesis<'_>> {
    let body = request
        .get("json")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("a speech request is a JSON body"))?;
    let input = match body.get("input") {
        Some(value) if is_blob(value) => return Err(invalid("'input' is longer than accepted")),
        Some(Value::String(input)) => input,
        _ => return Err(invalid("Missing 'input' field")),
    };
    let voice = match body.get("voice") {
        None | Some(Value::Null) => None,
        Some(Value::String(voice)) => Some(voice.as_str()),
        Some(_) => return Err(invalid("'voice' must be a string")),
    };
    let format = match body.get("response_format") {
        None | Some(Value::Null) => "mp3",
        Some(Value::String(format)) => format.as_str(),
        Some(_) => return Err(invalid("'response_format' must be a string")),
    };
    Ok(Synthesis { input, voice, format })
}

fn scalar_count(text: &str) -> u64 {
    u64::try_from(text.chars().count()).unwrap_or(u64::MAX)
}

/// The upstream's counting rule on `tts-inline`: each non-ASCII scalar counts twice.
fn weighted_count(text: &str) -> u64 {
    text.chars().map(|character| if character.is_ascii() { 1 } else { 2 }).sum()
}

fn synthesis_facts(
    characters: SpeechCharactersV1,
    forms: &[SpeechMeteringFormV1],
    media_type: &str,
) -> SpeechFactsV1 {
    SpeechFactsV1 {
        operation: SpeechOperationV1::Synthesize,
        characters: Some(characters),
        metering_forms: forms.to_vec(),
        reservation: None,
        output_media_type: Some(media_type.to_owned()),
    }
}

fn template_with_voice(mut template: Map<String, Value>, voice: Option<&str>) -> String {
    if let Some(voice) = voice {
        template.insert("voice".to_owned(), json!(voice));
    }
    Value::Object(template).to_string()
}

impl SyntheticSpeechReferenceV1 {
    fn prepare_stream(
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
        forms: &[SpeechMeteringFormV1],
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        let speech = synthesis(request)?;
        let media_type = match speech.format {
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            "opus" => "audio/opus",
            "flac" => "audio/flac",
            other => return Err(invalid(format!("response_format '{other}' is not supported"))),
        };
        let tokens = forms.contains(&SpeechMeteringFormV1::Tokens);
        let mut template = Map::new();
        template.insert("model".to_owned(), json!(context.upstream_model));
        template.insert("input".to_owned(), json!(speech.input));
        template.insert("format".to_owned(), json!(speech.format));
        if tokens {
            template.insert("stream_format".to_owned(), json!("sse"));
        }
        let template = template_with_voice(template, speech.voice);
        let (form, paths) = if tokens {
            (ResponseBodyFormV1::Sse, elision(&["/*/data/delta"])?)
        } else {
            (ResponseBodyFormV1::Binary, Vec::new())
        };
        let count = scalar_count(speech.input);
        Ok(PreparedSpeechCallV1 {
            facts: synthesis_facts(SpeechCharactersV1 { count, upper: None }, forms, media_type),
            descriptor: descriptor(
                config,
                "v1/speech",
                &format!(r#"{{"json":{{"template":{template}}}}}"#),
            )?,
            response_body_form: form,
            response_elision_paths: paths,
            immutable_body_paths: Some(vec![
                "model".to_owned(),
                "format".to_owned(),
                "stream_format".to_owned(),
            ]),
            immutable_form_fields: None,
            state: json!({
                "mode": if tokens { "sse" } else { "binary" },
                "media_type": media_type,
                "characters": count,
                "forms": forms,
            })
            .to_string(),
        })
    }

    fn prepare_inline(
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
        forms: &[SpeechMeteringFormV1],
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        let speech = synthesis(request)?;
        let (upstream_format, media_type) = match speech.format {
            "mp3" => ("mp3", "audio/mpeg"),
            "wav" => ("pcm", "audio/wav"),
            "pcm" => ("pcm", "audio/L16; rate=24000"),
            other => return Err(invalid(format!("response_format '{other}' is not supported"))),
        };
        let mut template = Map::new();
        template.insert("model".to_owned(), json!(context.upstream_model));
        template.insert("text".to_owned(), json!(speech.input));
        template.insert("format".to_owned(), json!(upstream_format));
        let template = template_with_voice(template, speech.voice);
        let count = scalar_count(speech.input);
        Ok(PreparedSpeechCallV1 {
            facts: synthesis_facts(
                SpeechCharactersV1 { count, upper: Some(weighted_count(speech.input)) },
                forms,
                media_type,
            ),
            descriptor: descriptor(
                config,
                "v1/speech/inline",
                &format!(r#"{{"json":{{"template":{template}}}}}"#),
            )?,
            response_body_form: ResponseBodyFormV1::Json,
            response_elision_paths: elision(&["/audio/data"])?,
            immutable_body_paths: Some(vec!["model".to_owned(), "format".to_owned()]),
            immutable_form_fields: None,
            state: json!({
                "mode": "inline",
                "client_format": speech.format,
                "media_type": media_type,
                "characters": count,
                "forms": forms,
            })
            .to_string(),
        })
    }

    fn prepare_url(
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
        forms: &[SpeechMeteringFormV1],
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        let speech = synthesis(request)?;
        // The upstream delivers WAV only, so an absent format is `wav` here.
        let format = if request.pointer("/json/response_format").is_none_or(Value::is_null) {
            "wav"
        } else {
            speech.format
        };
        if format != "wav" {
            return Err(invalid(format!("response_format '{format}' is not supported")));
        }
        let mut template = Map::new();
        template.insert("model".to_owned(), json!(context.upstream_model));
        template.insert("input".to_owned(), json!(speech.input));
        let template = template_with_voice(template, speech.voice);
        let count = scalar_count(speech.input);
        Ok(PreparedSpeechCallV1 {
            facts: synthesis_facts(SpeechCharactersV1 { count, upper: None }, forms, "audio/wav"),
            descriptor: descriptor(
                config,
                "v1/speech/url",
                &format!(r#"{{"json":{{"template":{template}}}}}"#),
            )?,
            response_body_form: ResponseBodyFormV1::Json,
            response_elision_paths: Vec::new(),
            immutable_body_paths: Some(vec!["model".to_owned()]),
            immutable_form_fields: None,
            state: json!({ "mode": "url", "characters": count, "forms": forms }).to_string(),
        })
    }

    fn prepare_ssml(
        config: &ProviderConfig,
        request: &Value,
        forms: &[SpeechMeteringFormV1],
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        let speech = synthesis(request)?;
        let voice = speech.voice.ok_or_else(|| invalid("Missing 'voice' field"))?;
        if speech.format != "mp3" {
            return Err(invalid(format!("response_format '{}' is not supported", speech.format)));
        }
        let ssml = format!(
            "<speak version='1.0' xml:lang='{}'><voice name='{}'>{}</voice></speak>",
            xml_escape(&voice_lang(voice)),
            xml_escape(voice),
            xml_escape(speech.input)
        );
        if ssml.len() > MAX_MEDIA_TEXT_BODY_BYTES {
            return Err(invalid("'input' is longer than accepted"));
        }
        let body = json!({ "text": { "media_type": SSML, "text": ssml } }).to_string();
        let count = scalar_count(speech.input);
        Ok(PreparedSpeechCallV1 {
            facts: synthesis_facts(SpeechCharactersV1 { count, upper: None }, forms, "audio/mpeg"),
            descriptor: descriptor(config, "v1/speech/ssml", &body)?,
            response_body_form: ResponseBodyFormV1::Binary,
            response_elision_paths: Vec::new(),
            immutable_body_paths: None,
            immutable_form_fields: None,
            state: json!({
                "mode": "binary",
                "media_type": "audio/mpeg",
                "characters": count,
                "forms": forms,
            })
            .to_string(),
        })
    }

    #[allow(clippy::too_many_lines, reason = "one multipart rewrite, read top to bottom")]
    fn prepare_recognition(
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
        forms: &[SpeechMeteringFormV1],
        verbose: bool,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        let parts = request
            .pointer("/multipart/parts")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("a transcription request is a multipart body"))?;
        if parts.iter().any(|part| part["kind"] == "text" && part.get("blob").is_some()) {
            return Err(invalid("a text field is longer than accepted"));
        }
        if !parts.iter().any(|part| part["kind"] == "file" && part["name"] == "file") {
            return Err(invalid("Missing 'file' field"));
        }
        let client = parts
            .iter()
            .find(|part| part["kind"] == "text" && part["name"] == "response_format")
            .and_then(|part| part["value"].as_str())
            .unwrap_or("json");
        let accepted: &[&str] = if verbose {
            &["json", "text", "verbose_json", "srt", "vtt"]
        } else {
            &["json", "text"]
        };
        if !accepted.contains(&client) {
            return Err(invalid(format!("response_format '{client}' is not supported")));
        }
        let upstream_format = if verbose { "verbose_json" } else { client };
        let mut out: Vec<MediaPartV1> = Vec::with_capacity(parts.len() + 2);
        let (mut model_set, mut format_set) = (false, false);
        for part in parts {
            let name = part["name"].as_str().ok_or_else(|| internal("a part has no name"))?;
            if part["kind"] == "file" {
                out.push(MediaPartV1::File {
                    name: name.to_owned(),
                    blob: serde_json::from_value(part["blob"].clone())
                        .map_err(|_| internal("a file part has no blob"))?,
                    transform: MediaTransformV1::AsIs,
                    filename: part["filename"].as_str().map(str::to_owned),
                    media_type: part["media_type"].as_str().map(str::to_owned),
                });
                continue;
            }
            let value = match name {
                "model" => {
                    model_set = true;
                    context.upstream_model.clone()
                }
                "response_format" => {
                    format_set = true;
                    upstream_format.to_owned()
                }
                _ => part["value"].as_str().unwrap_or_default().to_owned(),
            };
            out.push(MediaPartV1::Text { name: name.to_owned(), value });
        }
        if !model_set {
            out.push(MediaPartV1::Text {
                name: "model".to_owned(),
                value: context.upstream_model.clone(),
            });
        }
        if !format_set {
            out.push(MediaPartV1::Text {
                name: "response_format".to_owned(),
                value: upstream_format.to_owned(),
            });
        }
        let path = if context.operation == SpeechOperationV1::Translate {
            "v1/audio/translations"
        } else {
            "v1/audio/transcriptions"
        };
        let parts_json =
            serde_json::to_string(&out).map_err(|error| internal(error.to_string()))?;
        let form = if upstream_format == "text" {
            ResponseBodyFormV1::Text
        } else {
            ResponseBodyFormV1::Json
        };
        Ok(PreparedSpeechCallV1 {
            facts: SpeechFactsV1 {
                operation: context.operation,
                characters: None,
                metering_forms: forms.to_vec(),
                reservation: None,
                output_media_type: None,
            },
            descriptor: descriptor(
                config,
                path,
                &format!(r#"{{"multipart":{{"parts":{parts_json}}}}}"#),
            )?,
            response_body_form: form,
            response_elision_paths: Vec::new(),
            immutable_body_paths: None,
            immutable_form_fields: Some(vec![
                "file".to_owned(),
                "model".to_owned(),
                "response_format".to_owned(),
            ]),
            state: json!({
                "mode": "recognition",
                "client_format": client,
                "verbose": verbose,
                "forms": forms,
            })
            .to_string(),
        })
    }
}

/// A non-negative integer count, or an integral float such as `40.0`; anything else is not a
/// count. `None` when absent.
fn count(value: Option<&Value>) -> Result<Option<u64>, ()> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            if let Some(count) = number.as_u64() {
                return Ok(Some(count));
            }
            let float = number.as_f64().ok_or(())?;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            if float.is_finite() && float >= 0.0 && float.fract() == 0.0 && float < u64::MAX as f64
            {
                Ok(Some(float as u64))
            } else {
                Err(())
            }
        }
        Some(_) => Err(()),
    }
}

/// A finite, non-negative number of seconds. `None` when absent.
fn seconds(value: Option<&Value>) -> Result<Option<f64>, ()> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let seconds = number.as_f64().ok_or(())?;
            if seconds.is_finite() && seconds >= 0.0 { Ok(Some(seconds)) } else { Err(()) }
        }
        Some(_) => Err(()),
    }
}

fn error_envelope(status: u16, body: &Value) -> ErrorEnvelope {
    let code = match status {
        401 | 403 => ErrorCode::Auth,
        402 => ErrorCode::PaymentRequired,
        408 => ErrorCode::Timeout,
        429 => ErrorCode::RateLimit,
        500..=599 => ErrorCode::UpstreamUnavailable,
        _ => ErrorCode::InvalidRequest,
    };
    let message = match code {
        ErrorCode::Auth => "the upstream rejected the credential",
        ErrorCode::PaymentRequired => {
            "the upstream requires payment or the account is out of funds"
        }
        ErrorCode::RateLimit => "the upstream rate limited this request",
        ErrorCode::Timeout => "the upstream did not answer in time",
        ErrorCode::UpstreamUnavailable => "the upstream is unavailable",
        _ => "the upstream refused the request as malformed",
    };
    let mut envelope = ErrorEnvelope::new(code, status, message);
    envelope.provider_message = body["error"]["message"]
        .as_str()
        .filter(|message| message.chars().count() <= 256)
        .map(str::to_owned);
    envelope
}

/// `characters` from `usage.characters` when reported, else the request's count (§9).
fn characters(body: &Value, state: &Value) -> Result<SpeechCharacterMeterV1, ()> {
    match count(body.pointer("/usage/characters"))? {
        Some(value) => {
            Ok(SpeechCharacterMeterV1 { value, source: SpeechCharacterSourceV1::Upstream })
        }
        None => Ok(SpeechCharacterMeterV1 {
            value: state["characters"].as_u64().ok_or(())?,
            source: SpeechCharacterSourceV1::Request,
        }),
    }
}

fn request_characters(state: &Value) -> Option<SpeechCharacterMeterV1> {
    Some(SpeechCharacterMeterV1 {
        value: state["characters"].as_u64()?,
        source: SpeechCharacterSourceV1::Request,
    })
}

const fn audio(artifact: SpeechArtifactV1, metering: SpeechMeteringV1) -> SpeechOutcomeV1 {
    SpeechOutcomeV1::Succeeded { delivery: SpeechDeliveryV1::Audio(artifact), metering }
}

fn transcript(delivery: TranscriptDeliveryV1, seconds: Option<f64>) -> SpeechOutcomeV1 {
    SpeechOutcomeV1::Succeeded {
        delivery: SpeechDeliveryV1::Transcript(delivery),
        metering: SpeechMeteringV1 {
            seconds: seconds
                .map(|value| SpeechSecondsMeterV1 { value, basis: SpeechSecondsBasisV1::Reported }),
            ..SpeechMeteringV1::default()
        },
    }
}

/// `HH:MM:SS` plus the millisecond separator and milliseconds, as SRT (`,`) and `WebVTT` (`.`)
/// write a cue time.
fn cue_time(seconds: f64, separator: char) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let millis = (seconds * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}{separator}{:03}",
        millis / 3_600_000,
        millis / 60_000 % 60,
        millis / 1000 % 60,
        millis % 1000
    )
}

/// SRT or `WebVTT` cues from the upstream's `segments` (public formats, §9).
fn subtitles(body: &Value, vtt: bool) -> Option<String> {
    let mut out = String::from(if vtt { "WEBVTT\n\n" } else { "" });
    for (index, segment) in body["segments"].as_array()?.iter().enumerate() {
        let start = seconds(segment.get("start")).ok()??;
        let end = seconds(segment.get("end")).ok()??;
        let text = segment["text"].as_str()?.trim();
        let separator = if vtt { '.' } else { ',' };
        if !vtt {
            let _ = writeln!(out, "{}", index + 1);
        }
        let _ = write!(
            out,
            "{} --> {}\n{text}\n\n",
            cue_time(start, separator),
            cue_time(end, separator)
        );
    }
    Some(out)
}

impl SyntheticSpeechReferenceV1 {
    fn parse_stream(state: &Value, events: &[Value]) -> ComponentResultV1<SpeechOutcomeV1> {
        let mut items = Vec::new();
        for (index, event) in events.iter().enumerate() {
            if event["event"] != DELTA_EVENT {
                continue;
            }
            let delta = &event["data"]["delta"];
            if !is_blob(delta) {
                return Ok(unknown("an audio delta is not a string"));
            }
            if delta[BLOB_KEY]["bytes"].as_u64().unwrap_or(0) > 0 {
                items.push(SpeechSegmentV1 {
                    pointer: pointer(&format!("/{index}/data/delta"))?,
                    encoding: SpeechEncodingV1::Base64,
                });
            }
        }
        if items.is_empty() {
            return Ok(unknown("a 2xx without audio"));
        }
        let Some(usage) = events
            .iter()
            .rev()
            .find(|event| event["event"] == DONE_EVENT)
            .and_then(|event| event["data"].get("usage"))
            .filter(|usage| usage.is_object())
        else {
            return Ok(unknown("the stream reported no usage"));
        };
        let (Ok(input), Ok(output)) =
            (count(usage.get("input_tokens")), count(usage.get("output_tokens")))
        else {
            return Ok(unknown("a malformed token count"));
        };
        let (Some(input), Some(output)) = (input, output) else {
            return Ok(unknown("a token count is missing"));
        };
        let media_type = state["media_type"].as_str().unwrap_or("audio/mpeg").to_owned();
        Ok(audio(
            SpeechArtifactV1 {
                form: SpeechArtifactFormV1::Segments { items },
                media_type,
                container: None,
            },
            SpeechMeteringV1 {
                tokens: Some(SpeechTokensV1 {
                    input: Some(input),
                    output: Some(output),
                    input_text: None,
                    input_audio: None,
                }),
                ..SpeechMeteringV1::default()
            },
        ))
    }

    fn parse_inline(state: &Value, body: &Value) -> ComponentResultV1<SpeechOutcomeV1> {
        let data = &body["audio"]["data"];
        if !is_blob(data) || data[BLOB_KEY]["bytes"].as_u64().unwrap_or(0) == 0 {
            return Ok(unknown("a 2xx without audio"));
        }
        let Ok(characters) = characters(body, state) else {
            return Ok(unknown("a malformed character count"));
        };
        let media_type = state["media_type"].as_str().unwrap_or("audio/mpeg").to_owned();
        let (encoding, container) = match state["client_format"].as_str() {
            Some("wav") => (
                SpeechEncodingV1::Base64,
                Some(SpeechContainerV1::WavPcmS16le { sample_rate: PCM_RATE, channels: 1 }),
            ),
            Some("pcm") => (SpeechEncodingV1::Base64, None),
            _ => (SpeechEncodingV1::Hex, None),
        };
        Ok(audio(
            SpeechArtifactV1 {
                form: SpeechArtifactFormV1::Inline { pointer: pointer("/audio/data")?, encoding },
                media_type,
                container,
            },
            SpeechMeteringV1 { characters: Some(characters), ..SpeechMeteringV1::default() },
        ))
    }

    fn parse_url(state: &Value, body: &Value) -> ComponentResultV1<SpeechOutcomeV1> {
        if body.pointer("/output/audio/url").and_then(Value::as_str).is_none_or(str::is_empty) {
            return Ok(unknown("a 2xx without audio"));
        }
        let Ok(characters) = characters(body, state) else {
            return Ok(unknown("a malformed character count"));
        };
        Ok(audio(
            SpeechArtifactV1 {
                form: SpeechArtifactFormV1::Url {
                    pointer: pointer("/output/audio/url")?,
                    upgrade_insecure: false,
                },
                media_type: "audio/wav".to_owned(),
                container: None,
            },
            SpeechMeteringV1 { characters: Some(characters), ..SpeechMeteringV1::default() },
        ))
    }

    fn parse_recognition(state: &Value, view_body: &Value) -> SpeechOutcomeV1 {
        let client = state["client_format"].as_str().unwrap_or("json");
        if !state["verbose"].as_bool().unwrap_or(false) {
            if client == "text" {
                // The upstream answered the client's own plain-text format: delivered as is, and
                // no format of this answer carries a duration (§9).
                return transcript(
                    TranscriptDeliveryV1::Passthrough { media_type: "text/plain".to_owned() },
                    None,
                );
            }
            let body = &view_body["json"];
            let Ok(seconds) = seconds(body.pointer("/usage/seconds")) else {
                return unknown("a malformed duration");
            };
            return json_transcript(body).map_or_else(
                || unknown("a 2xx without a transcript"),
                |delivery| transcript(delivery, seconds),
            );
        }
        let body = &view_body["json"];
        let Ok(duration) = seconds(body.get("duration")) else {
            return unknown("a malformed duration");
        };
        // `verbose_json` always carries the duration; one without it cannot be billed (§9).
        let Some(duration) = duration else {
            return unknown("the upstream did not report a duration");
        };
        let delivery = match client {
            "json" => json_transcript(body),
            "verbose_json" => Some(TranscriptDeliveryV1::Passthrough {
                media_type: "application/json".to_owned(),
            }),
            "text" => body["text"].as_str().map(|text| TranscriptDeliveryV1::Text {
                text: text.to_owned(),
                media_type: "text/plain".to_owned(),
            }),
            "srt" | "vtt" => {
                subtitles(body, client == "vtt").map(|text| TranscriptDeliveryV1::Text {
                    text,
                    media_type: if client == "vtt" { "text/vtt" } else { "application/x-subrip" }
                        .to_owned(),
                })
            }
            _ => None,
        };
        match delivery {
            Some(TranscriptDeliveryV1::Text { text, .. })
                if text.len() > MediaLimitsV1::V1.inline_string_bytes =>
            {
                unknown("the rendered transcript is longer than a text delivery holds")
            }
            Some(delivery) => transcript(delivery, Some(duration)),
            None => unknown("a 2xx without the transcript the client asked for"),
        }
    }
}

/// `{"text"}` from the upstream's JSON, a reference when the host elided the text.
fn json_transcript(body: &Value) -> Option<TranscriptDeliveryV1> {
    let text = &body["text"];
    let template = if let Some(blob) = text.get(BLOB_KEY) {
        json!({ "text": { "$south.ref": { "blob": blob["id"], "transform": "as_is" } } })
    } else {
        json!({ "text": text.as_str()? })
    };
    Some(TranscriptDeliveryV1::Json { template: template.to_string() })
}

impl SpeechComponentV1 for SyntheticSpeechReferenceV1 {
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
        if config.provider != SYNTHETIC_SPEECH {
            return Err(capability(format!("this package serves the {SYNTHETIC_SPEECH} family")));
        }
        let mut models = Vec::new();
        for row in &config.models {
            let words = words_of(config, &row.model)?;
            if let Some((_, operations, metering_forms)) = declaration(&row.model, &words) {
                models.push(SpeechModelCapabilitiesV1 {
                    model: row.model.clone(),
                    operations,
                    metering_forms,
                });
            }
        }
        Ok(models)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        if config.provider != SYNTHETIC_SPEECH {
            return Err(capability(format!("this package serves the {SYNTHETIC_SPEECH} family")));
        }
        let model = &context.upstream_model;
        let words = words_of(config, model)?;
        let Some((mode, operations, model_forms)) = declaration(model, &words) else {
            return Err(capability(format!("the package does not serve the model '{model}'")));
        };
        if !operations.contains(&context.operation) {
            return Err(capability(format!(
                "the model '{model}' does not declare the operation {:?}",
                context.operation
            )));
        }
        if context.metering_required.is_empty() {
            return Err(capability("the host requires no metering form"));
        }
        if let Some(form) =
            context.metering_required.iter().find(|form| !model_forms.contains(form))
        {
            return Err(capability(format!("the model '{model}' cannot report {form:?} metering")));
        }
        let forms = &context.metering_required;
        match mode {
            Mode::Stream => Self::prepare_stream(config, request, context, forms),
            Mode::Inline => Self::prepare_inline(config, request, context, forms),
            Mode::Url => Self::prepare_url(config, request, context, forms),
            Mode::Ssml => Self::prepare_ssml(config, request, forms),
            Mode::Recognition => Self::prepare_recognition(
                config,
                request,
                context,
                forms,
                words.contains(&SpeechDialectWordV1::VerboseJson),
            ),
        }
    }

    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        let status =
            response["status"].as_u64().and_then(|status| u16::try_from(status).ok()).unwrap_or(0);
        let body = &response["body"];
        if !(200..300).contains(&status) {
            let error = error_envelope(status, &body["json"]);
            return Ok(if (400..500).contains(&status) && status != 408 {
                SpeechOutcomeV1::Rejected { error }
            } else {
                SpeechOutcomeV1::Unknown {
                    reason: format!("upstream status {status}"),
                    error: Some(error),
                }
            });
        }
        match state["mode"].as_str() {
            Some("binary") => {
                if body["opaque"]["bytes"].as_u64().unwrap_or(0) == 0 {
                    return Ok(unknown("a 2xx without audio"));
                }
                let Some(characters) = request_characters(state) else {
                    return Err(internal("the state has no character count"));
                };
                Ok(audio(
                    SpeechArtifactV1 {
                        form: SpeechArtifactFormV1::Body,
                        media_type: state["media_type"].as_str().unwrap_or("audio/mpeg").to_owned(),
                        container: None,
                    },
                    SpeechMeteringV1 {
                        characters: Some(characters),
                        ..SpeechMeteringV1::default()
                    },
                ))
            }
            Some("sse") => body["sse"].as_array().map_or_else(
                || Ok(unknown("a 2xx that is not an event stream")),
                |events| Self::parse_stream(state, events),
            ),
            Some("inline") => Self::parse_inline(state, &body["json"]),
            Some("url") => Self::parse_url(state, &body["json"]),
            Some("recognition") => Ok(Self::parse_recognition(state, body)),
            _ => Err(internal("the state names no mode")),
        }
    }
}
