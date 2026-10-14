//! The native reference implementation of the `speech-elevenlabs` component.
//!
//! `ElevenLabs` text to speech on the `OpenAI` speech surface (speech world batch V3-1, design record
//! `docs/design/2026-10-14-speech-v3-1-tts.md` §5; speech record §12 row 4). Transcribed from
//! token-station-server's native arm `TtsUpstream::ElevenLabs` (`handler/audio/tts.rs:643-663`)
//! and its tables `resolve_elevenlabs_voice`, `map_voice_name_to_elevenlabs_id` and
//! `elevenlabs_output_format` (`tts_providers.rs:370-442`):
//!
//! - **Request**: `POST v1/text-to-speech/{voice_id}` under the row endpoint with the query
//!   `output_format=mp3_44100_128`, the secret in `xi-api-key` (`header_secret`), JSON
//!   `{"text", "model_id"}` (`model_id` the upstream id).
//! - **`output_format`**: `response_format` absent, not a string or `mp3` in any case is
//!   `mp3_44100_128`; any other string is refused with the native text.
//! - **`voice_id`**: `voice` absent, not a string or blank after trimming is the native default
//!   `9BWtsMINqrJLrRacOk9x`; otherwise the trimmed value lower-cased through the native table (the
//!   `OpenAI` names and the twenty default voice names), anything else as trimmed. A resolved id
//!   outside `[A-Za-z0-9._~-]{1,128}`, or `.` / `..`, is refused before dispatch (SV-Q4): native
//!   interpolated it into the path unencoded (record §9 item 1).
//! - **Declared query parameter**: `output_format` (`token` syntax) is declared in the manifest's
//!   `query_parameters`, so the descriptor needs `contracts.media` 3 (record §6) and is built with
//!   that declaration.
//! - **Immutable paths** `text`, `model_id` (native `body_rules`, `tts.rs:77`). Characters from the
//!   request.
//! - **Response**: `binary`, typed by the upstream's `content-type`; an empty body is `unknown`. A
//!   non-2xx carries `detail.message`, a string `detail`, or a validation list's first `msg` as the
//!   provider message, else the body text.
//!
//! **Recognition** (speech world batch V3-3, design record `docs/design/2026-10-14-speech-v3-3-asr.md`
//! §4, SV-Q8, SV-Q9; speech record §12 ASR row 3), transcribed from the native arm
//! `AsrUpstream::ElevenLabs` (`handler/audio.rs:92-93`, `:277`; `handler/audio/multipart.rs:13-22`,
//! `:456-476`):
//!
//! - **Operations**: `transcribe` for every configured model, no `translate`; `seconds` exactly.
//! - **Request**: `POST v1/speech-to-text` under the row endpoint, the secret in `xi-api-key`, no
//!   query. Every text part named `model` becomes `model_id` with the upstream id (appended when
//!   absent), every text part named `language` becomes `language_code`, `timestamps_granularity` is
//!   set to `word` (replaced or appended), everything else is forwarded. The common refusals and the
//!   `OpenAI` format set; `json` response.
//! - **Duration**: the last `words` entry's `end`, `basis = last_timestamp` (labelled estimated by
//!   the host, Q12); `words` absent, not an array or empty, or the last `end` absent or `null`, is
//!   `seconds: null`; a malformed `end` is `unknown`.
//! - **Deliveries**: `json` `{"text"}`; `text`; `srt` / `vtt` rendered from the words grouped into
//!   cues (SV-Q8, `asr_shared::word_cues`); `verbose_json` in the `OpenAI` verbose shape
//!   `{"task", "language", "duration", "text", "words"}` (SV-Q9).
//! - **Immutable form fields** `model_id`, `file`, `timestamps_granularity` (native `audio.rs:78`).

#![allow(
    clippy::result_large_err,
    reason = "a rendering step fails with the outcome the call ends with, built once per round"
)]

use serde_json::{Map, Value, json};
use south_contracts::media::ResponseBodyFormV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechMeteringFormV1, SpeechModelCapabilitiesV1,
    SpeechSecondsBasisV1, TranscriptDeliveryV1,
};
use south_contracts::{DeclaredQueryParameterV1, QueryValueSyntaxV1};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::ProviderConfig;

use crate::asr_shared::{
    Duration, PartPlan, Transcript, duration, elevenlabs_words, json_delivery, malformed_duration,
    multipart_descriptor, no_transcript, not_an_object, openai_format, ordered_object, plain_text,
    recognition_forms, request_parts, rewrite_parts, subtitle_delivery, succeeded, text_only_json,
    transcript, transcript_value, word_cues,
};
use crate::speech_shared::{
    FALLBACK_MEDIA_TYPE, binary_outcome, descriptor, facts, input, internal, invalid, is_blob,
    json_body, json_object, non_2xx, operation_word, prepared, required_forms, scalar_count,
    serves, unknown, view_status,
};
use crate::{ComponentResultV1, SpeechComponentV1, SpeechOutcomeV1};

/// The package name this reference is published as.
pub const NAME: &str = "speech-elevenlabs";
/// The package version.
pub const VERSION: &str = "1.0.0";
/// The world this reference exports.
pub const WORLD: &str = "speech-adapter-v1";
/// The family, as a host's speech bindings name it.
pub const ELEVENLABS_SPEECH: &str = "elevenlabs-speech";
/// The header the secret travels in (native `upstream.rs:87-90`; one of the closed
/// `SecretHeaderV1` names).
pub const SECRET_HEADER: &str = "xi-api-key";
/// The declared query parameter the manifest names.
pub const OUTPUT_FORMAT: &str = "output_format";
/// The one `output_format` the `OpenAI` surface asks for (native `elevenlabs_output_format`).
const MP3_OUTPUT_FORMAT: &str = "mp3_44100_128";
/// Native `ELEVENLABS_DEFAULT_VOICE_ID` (Aria).
pub const DEFAULT_VOICE_ID: &str = "9BWtsMINqrJLrRacOk9x";
/// The longest `voice_id` the package sends (SV-Q4).
const MAX_VOICE_ID_BYTES: usize = 128;

const FORMS: [SpeechMeteringFormV1; 1] = [SpeechMeteringFormV1::Characters];
const IMMUTABLE_BODY_PATHS: [&str; 2] = ["text", "model_id"];
/// The recognition path (native `audio.rs:277`).
const STT_PATH: &str = "v1/speech-to-text";
/// The form fields the gateway writes on the recognition path (native `audio.rs:78`).
const ASR_IMMUTABLE_FORM_FIELDS: [&str; 3] = ["model_id", "file", "timestamps_granularity"];

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct ElevenLabsSpeechReferenceV1;

/// `map_voice_name_to_elevenlabs_id`: an `OpenAI` name or a default voice name, in any case, to its
/// id; anything else as given.
fn mapped_voice_id(voice: &str) -> &str {
    match voice.to_lowercase().as_str() {
        "alloy" | "aria" => "9BWtsMINqrJLrRacOk9x",
        "nova" | "sarah" => "EXAVITQu4vr4xnSDxMaL",
        "shimmer" | "laura" => "FGY2WhTYpPnrIDTdsKH5",
        "coral" | "charlotte" => "XB0fDUnXU5powFXDhCwa",
        "sage" | "alice" => "Xb7hH8MSUJpSbSDYk0k2",
        "echo" | "roger" => "CwhRBWXzGAHq8TQ4Fs17",
        "fable" | "george" => "JBFqnCBsd6RMkjVDRZzb",
        "onyx" | "brian" => "nPczCjzI2devNBz1zQrb",
        "ash" | "charlie" => "IKne3meq5aSn9XLyUdCD",
        "ballad" | "will" => "bIHbv24MWmeRgasZH58o",
        "verse" | "daniel" => "onwK4e9ZLuTAKqWW03F9",
        "callum" => "N2lVS1w4EtoT3dr4eOWO",
        "river" => "SAz9YHcvj6GT2YYXdXww",
        "liam" => "TX3LPaxmHKxFdv7VOQHJ",
        "matilda" => "XrExE9yKIg1WjnnlVkGX",
        "jessica" => "cgSgspJ2msm6clMCkdW9",
        "eric" => "cjVigY5qzO86Huf0OWal",
        "chris" => "iP95p4xoKVk53GoZ742B",
        "lily" => "pFZP5JQG7iQjIQuC4Bku",
        "bill" => "pqHfZKP75CvOlQylNhV4",
        _ => voice,
    }
}

/// SV-Q4: a `voice_id` the package sends is one path segment of unreserved bytes, at most 128
/// long, and not a dot segment.
fn is_voice_id(voice_id: &str) -> bool {
    (1..=MAX_VOICE_ID_BYTES).contains(&voice_id.len())
        && voice_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-'))
        && voice_id != "."
        && voice_id != ".."
}

fn voice_refused() -> token_station_protocol::ErrorEnvelope {
    invalid(
        "the ElevenLabs voice_id must be 1 to 128 characters of A-Z, a-z, 0-9, '.', '_', '~' or \
         '-' and not '.' or '..'",
    )
}

/// `resolve_elevenlabs_voice`, then the SV-Q4 grammar.
fn voice_id(body: &Map<String, Value>) -> ComponentResultV1<String> {
    let resolved = match body.get("voice") {
        Some(Value::String(voice)) => {
            let trimmed = voice.trim();
            if trimmed.is_empty() { DEFAULT_VOICE_ID } else { mapped_voice_id(trimmed) }
        }
        // An elided voice is longer than any id the grammar admits.
        Some(value) if is_blob(value) => return Err(voice_refused()),
        _ => DEFAULT_VOICE_ID,
    };
    if is_voice_id(resolved) { Ok(resolved.to_owned()) } else { Err(voice_refused()) }
}

/// `elevenlabs_output_format`: `mp3` (any case) or absent; a non-string is absent.
fn check_response_format(body: &Map<String, Value>) -> ComponentResultV1<()> {
    match body.get("response_format") {
        Some(value) if is_blob(value) => {
            Err(invalid("'response_format' is longer than the gateway accepts"))
        }
        Some(Value::String(format)) => {
            let format = format.to_lowercase();
            if format == "mp3" {
                Ok(())
            } else {
                Err(invalid(format!(
                    "ElevenLabs /v1/audio/speech supports response_format 'mp3' only (got \
                     '{format}')"
                )))
            }
        }
        _ => Ok(()),
    }
}

/// The package's declared query parameters, as its manifest declares them.
pub fn declared_query_parameters() -> ComponentResultV1<Vec<DeclaredQueryParameterV1>> {
    DeclaredQueryParameterV1::try_new(OUTPUT_FORMAT, QueryValueSyntaxV1::Token)
        .map(|parameter| vec![parameter])
        .map_err(|_| internal("the output_format declaration is invalid"))
}

/// `ElevenLabs`' error bodies (record §5, unverified against captures): `{"detail": {"message"}}`,
/// `{"detail": "<text>"}` or a validation list whose first entry has `msg`.
fn error_message(body: &Value) -> Option<String> {
    let detail = &body["detail"];
    detail["message"]
        .as_str()
        .or_else(|| detail.as_str())
        .or_else(|| detail.get(0).and_then(|first| first["msg"].as_str()))
        .map(str::to_owned)
}

/// The `header_secret` arm on the configured slot.
fn secret_auth(config: &ProviderConfig) -> Option<Value> {
    config.auth.as_ref().map(
        |slot| json!({ "arm": "header_secret", "header": SECRET_HEADER, "slot": slot.as_str() }),
    )
}

/// `prepare` of a transcription (§4).
fn prepare_recognition(
    config: &ProviderConfig,
    request: &Value,
    context: &SpeechCallContextV1,
) -> ComponentResultV1<PreparedSpeechCallV1> {
    let forms = recognition_forms(config, context, false)?;
    let parts = request_parts(request)?;
    let format = openai_format(parts)?;
    let plan = PartPlan {
        written: vec![
            ("model", "model_id", context.upstream_model.clone()),
            ("timestamps_granularity", "timestamps_granularity", "word".to_owned()),
        ],
        renamed_text: &[("language", "language_code")],
        ..PartPlan::default()
    };
    let upstream_parts = rewrite_parts(parts, &plan)?;
    Ok(crate::asr_shared::prepared(
        context.operation,
        forms,
        multipart_descriptor(
            STT_PATH,
            None,
            secret_auth(config),
            &upstream_parts,
            &declared_query_parameters()?,
        )?,
        ResponseBodyFormV1::Json,
        &ASR_IMMUTABLE_FORM_FIELDS,
        &json!({ "op": operation_word(context.operation), "format": format }),
    ))
}

/// The `words` list as JSON for the `verbose_json` rendering (SV-Q9): one `{"word", "start",
/// "end"}` per word entry; `[]` when the answer has no list.
fn verbose_words(words: Option<&Value>) -> Result<String, SpeechOutcomeV1> {
    let entries = match words {
        None | Some(Value::Null) => return Ok("[]".to_owned()),
        Some(Value::Array(entries)) => entries,
        Some(_) => return Err(unknown("a 2xx whose words are not a list")),
    };
    let words = elevenlabs_words(entries).ok_or_else(|| unknown("a word without usable timing"))?;
    let rendered: Vec<String> = words
        .iter()
        .map(|word| {
            ordered_object(&[
                ("word", Value::String(word.text.to_owned()).to_string()),
                ("start", json!(word.start).to_string()),
                ("end", json!(word.end).to_string()),
            ])
        })
        .collect();
    Ok(format!("[{}]", rendered.join(",")))
}

/// SV-Q9: the `OpenAI` verbose shape rendered from the answer; `language` when the answer names
/// one, `duration` when the seconds are not `null`.
fn verbose_json(
    body: &Map<String, Value>,
    text: &Transcript<'_>,
    seconds: Option<f64>,
) -> Result<TranscriptDeliveryV1, SpeechOutcomeV1> {
    let text = transcript_value(text).ok_or_else(no_transcript)?;
    let mut members = vec![("task", "\"transcribe\"".to_owned())];
    if let Some(language) = body.get("language_code").and_then(Value::as_str) {
        members.push(("language", Value::String(language.to_owned()).to_string()));
    }
    if let Some(seconds) = seconds {
        members.push(("duration", json!(seconds).to_string()));
    }
    members.push(("text", text));
    members.push(("words", verbose_words(body.get("words"))?));
    json_delivery(ordered_object(&members))
}

/// The 2xx of a transcription (§4).
fn recognition_outcome(state: &Value, response: &Value) -> SpeechOutcomeV1 {
    let format = state["format"].as_str().unwrap_or("json");
    let Some(body) = json_object(response) else { return not_an_object() };
    let words = body.get("words");
    let last = words.and_then(Value::as_array).and_then(|entries| entries.last());
    let seconds = match last.map(|last| duration(last.get("end"))) {
        None | Some(Duration::Absent) => None,
        Some(Duration::Reported(seconds)) => Some(seconds),
        Some(Duration::Malformed) => return malformed_duration(),
    };
    let text = transcript(body);
    let delivery = match format {
        "text" => plain_text(&text),
        "srt" | "vtt" => match words {
            None | Some(Value::Null) => subtitle_delivery(&[], format),
            Some(Value::Array(entries)) => elevenlabs_words(entries).map_or_else(
                || Err(unknown("a word without usable timing")),
                |words| subtitle_delivery(&word_cues(&words), format),
            ),
            Some(_) => Err(unknown("a 2xx whose words are not a list")),
        },
        "verbose_json" => verbose_json(body, &text, seconds),
        _ => text_only_json(&text),
    };
    match delivery {
        Ok(delivery) => succeeded(
            delivery,
            seconds.map(|seconds| (seconds, SpeechSecondsBasisV1::LastTimestamp)),
        ),
        Err(outcome) => outcome,
    }
}

impl SpeechComponentV1 for ElevenLabsSpeechReferenceV1 {
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
        crate::asr_shared::capabilities(config, ELEVENLABS_SPEECH, &FORMS, false)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        serves(config, ELEVENLABS_SPEECH)?;
        if context.operation.is_recognition() {
            return prepare_recognition(config, request, context);
        }
        let forms = required_forms(context, &FORMS)?;
        let body = json_body(request)?;
        let text = input(body)?;
        // Native order: the format is judged before the voice (which native never refused).
        check_response_format(body)?;
        let voice_id = voice_id(body)?;
        let auth = secret_auth(config);
        let template = json!({ "text": text, "model_id": context.upstream_model });
        let count = scalar_count(text);
        prepared(
            facts(count, &forms, FALLBACK_MEDIA_TYPE),
            descriptor(
                &format!("v1/text-to-speech/{voice_id}"),
                Some(json!([{ "name": OUTPUT_FORMAT, "value": MP3_OUTPUT_FORMAT }])),
                auth,
                &template,
                &declared_query_parameters()?,
            )?,
            ResponseBodyFormV1::Binary,
            &[],
            &IMMUTABLE_BODY_PATHS,
            &json!({ "form": "binary", "characters": count }),
        )
    }

    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        let status = view_status(response);
        if !(200..300).contains(&status) {
            Ok(non_2xx(status, response, error_message))
        } else if state.get("op").is_some() {
            Ok(recognition_outcome(state, response))
        } else {
            binary_outcome(state, response)
        }
    }
}
