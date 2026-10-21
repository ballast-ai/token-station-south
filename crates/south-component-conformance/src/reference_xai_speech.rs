//! The native reference implementation of the `speech-xai` component.
//!
//! xAI text to speech (speech world batch V3-1, design record
//! `docs/design/2026-10-14-speech-v3-1-tts.md` §3; speech record §12 row 2). Transcribed from
//! token-station-server's native arm `TtsUpstream::Xai` (`handler/audio/tts.rs:439-474`) and its
//! tables `map_openai_voice_to_xai` and `map_openai_response_format_to_xai`
//! (`tts_providers.rs:266-324`):
//!
//! - **Request**: `POST v1/tts` under the row endpoint, `bearer` on the configured slot, JSON
//!   `{"text", "voice_id", "language", "output_format"?}`.
//! - **`voice_id`**: the client `voice` lower-cased through the native table (xAI names pass,
//!   `OpenAI` names fold to the closest xAI voice), anything else verbatim; absent or not a string is
//!   `eve`. **`language`**: the client's when a string, else `en`.
//! - **`output_format`**: a client `output_format` (any JSON value, `null` included) is forwarded
//!   verbatim and `response_format` ignored; else a string `response_format` lower-cased through the
//!   native table (`mp3` omitted, `wav` / `pcm` 24 kHz, `mulaw` / `alaw` 8 kHz), `flac` / `opus` /
//!   `aac` and any other value refused with the native texts; a non-string is omitted.
//! - **Immutable paths** `text` (native `body_rules`, `tts.rs:64`). Characters from the request.
//! - **Response**: `binary`; an empty body is `unknown`; the artifact is typed by the upstream's
//!   `content-type` (§1). A non-2xx carries `{"code", "error"}`'s `error` as the provider message.
//!
//! **Recognition** (speech world batch V3-3, design record `docs/design/2026-10-14-speech-v3-3-asr.md`
//! §3; speech record §12 ASR row 2), transcribed from the native arm `AsrUpstream::Xai`
//! (`handler/audio.rs:94-95`, `:203-208`, `:278`, `:559-564`):
//!
//! - **Operations**: `transcribe` for every configured model, no `translate`; `seconds` exactly.
//! - **Request**: `POST v1/stt` under the row endpoint, `bearer`; every client part forwarded in
//!   order except every text part named `model`, which is removed (native strips it). The common
//!   refusals and the `OpenAI` format set as `speech-openai-compatible`.
//! - **Verbose rows** and **other rows** as `speech-openai-compatible`, except that only
//!   `duration` is read (xAI's usage shape is unknown).
//! - **Immutable form fields** `model`, `file`, `response_format`, `stream` (native `audio.rs:77`).

use serde_json::{Map, Value, json};
use south_contracts::media::ResponseBodyFormV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechMeteringFormV1, SpeechModelCapabilitiesV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::ProviderConfig;

use crate::asr_shared::{OpenAiAsr, openai_outcome, prepare_openai};
use crate::speech_shared::{
    FALLBACK_MEDIA_TYPE, bearer, binary_outcome, descriptor, facts, input, invalid, is_blob,
    json_body, non_2xx, prepared, required_forms, scalar_count, serves, view_status, with_refs,
};
use crate::{ComponentResultV1, SpeechComponentV1, SpeechOutcomeV1};

/// The package name this reference is published as.
pub const NAME: &str = "speech-xai";
/// The package version.
pub const VERSION: &str = "1.0.11";
/// The world this reference exports.
pub const WORLD: &str = "speech-adapter-v1";
/// The family, as a host's speech bindings name it.
pub const XAI_SPEECH: &str = "xai-speech";

const FORMS: [SpeechMeteringFormV1; 1] = [SpeechMeteringFormV1::Characters];
const IMMUTABLE_BODY_PATHS: [&str; 1] = ["text"];

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct XaiSpeechReferenceV1;

/// `map_openai_voice_to_xai`: lower-cased match, anything else verbatim.
fn voice_id(voice: &str) -> &str {
    match voice.to_lowercase().as_str() {
        "ara" => "ara",
        "eve" | "nova" | "shimmer" | "alloy" | "coral" => "eve",
        "leo" | "fable" => "leo",
        "rex" | "onyx" | "echo" => "rex",
        "sal" | "ash" | "sage" | "ballad" | "verse" => "sal",
        _ => voice,
    }
}

/// `map_openai_response_format_to_xai`.
fn output_format(format: &str) -> ComponentResultV1<Option<Value>> {
    let format = format.to_lowercase();
    Ok(match format.as_str() {
        "mp3" => None,
        "wav" => Some(json!({ "codec": "wav", "sample_rate": 24000 })),
        "pcm" => Some(json!({ "codec": "pcm", "sample_rate": 24000 })),
        "mulaw" => Some(json!({ "codec": "mulaw", "sample_rate": 8000 })),
        "alaw" => Some(json!({ "codec": "alaw", "sample_rate": 8000 })),
        "flac" | "opus" | "aac" => {
            return Err(invalid(format!(
                "xAI grok-tts does not support response_format='{format}'. Supported: mp3, wav, \
                 pcm, mulaw, alaw."
            )));
        }
        _ => {
            return Err(invalid(format!(
                "Unknown response_format='{format}'. Supported: mp3, wav, pcm, mulaw, alaw."
            )));
        }
    })
}

/// The upstream's `output_format`: the client's own when present, else the mapped
/// `response_format`.
fn upstream_output_format(body: &Map<String, Value>) -> ComponentResultV1<Option<Value>> {
    if let Some(passthrough) = body.get("output_format") {
        return Ok(Some(with_refs(passthrough)));
    }
    match body.get("response_format") {
        Some(value) if is_blob(value) => {
            Err(invalid("'response_format' is longer than the gateway accepts"))
        }
        Some(Value::String(format)) => output_format(format),
        _ => Ok(None),
    }
}

/// xAI's error body `{"code", "error"}` (record §3, unverified against a capture).
fn error_message(body: &Value) -> Option<String> {
    body["error"].as_str().map(str::to_owned)
}

impl SpeechComponentV1 for XaiSpeechReferenceV1 {
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
        crate::asr_shared::capabilities(config, XAI_SPEECH, &FORMS, false)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        serves(config, XAI_SPEECH)?;
        if context.operation.is_recognition() {
            let protocol = OpenAiAsr {
                path: "v1/stt".to_owned(),
                query: None,
                drops_model: true,
                translates: false,
                auth: bearer(config),
                upstream_model: &context.upstream_model,
            };
            return prepare_openai(config, request, context, &protocol);
        }
        let forms = required_forms(context, &FORMS)?;
        let body = json_body(request)?;
        let text = input(body)?;
        let voice = match body.get("voice") {
            Some(Value::String(voice)) => json!(voice_id(voice)),
            // An elided voice is no table name: forwarded verbatim, as native forwards it.
            Some(value) if is_blob(value) => with_refs(value),
            _ => json!("eve"),
        };
        let language = match body.get("language") {
            Some(value @ Value::String(_)) => value.clone(),
            Some(value) if is_blob(value) => with_refs(value),
            _ => json!("en"),
        };
        let mut template = Map::new();
        template.insert("text".to_owned(), json!(text));
        template.insert("voice_id".to_owned(), voice);
        template.insert("language".to_owned(), language);
        if let Some(format) = upstream_output_format(body)? {
            template.insert("output_format".to_owned(), format);
        }
        let count = scalar_count(text);
        prepared(
            facts(count, &forms, FALLBACK_MEDIA_TYPE),
            descriptor("v1/tts", None, bearer(config), &Value::Object(template), &[])?,
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
            Ok(openai_outcome(state, response, false))
        } else {
            binary_outcome(state, response)
        }
    }
}
