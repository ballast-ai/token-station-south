//! The native reference implementation of the `speech-azure` component.
//!
//! Azure Speech fast transcription (speech world batch V3-3, design record
//! `docs/design/2026-10-14-speech-v3-3-asr.md` §5; speech record §12 ASR row 4). Transcribed from
//! token-station-server's native arm `AsrUpstream::AzureSpeech` (`handler/audio.rs:96-97`, `:281-283`,
//! `:534-551`; `handler/audio/multipart.rs:291-446`). `transcribe` only: Azure SSML synthesis
//! arrives as a later version of this package (SV-Q1 (b)).
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
use south_contracts::media::{MediaLimitsV1, ResponseBodyFormV1};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechModelCapabilitiesV1, SpeechSecondsBasisV1,
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
    BLOB_KEY, invalid, json_object, non_2xx, openai_error_message, serves, unknown, view_status,
};
use crate::{ComponentResultV1, SpeechComponentV1, SpeechOutcomeV1};

/// The package name this reference is published as.
pub const NAME: &str = "speech-azure";
/// The package version.
pub const VERSION: &str = "1.0.0";
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
        crate::asr_shared::capabilities(config, AZURE_SPEECH, &[], false)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        serves(config, AZURE_SPEECH)?;
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
