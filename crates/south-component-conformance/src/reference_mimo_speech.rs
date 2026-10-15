//! The native reference implementation of the `speech-mimo` component.
//!
//! Xiaomi `MiMo` text to speech (speech world batch V3-2, design record
//! `docs/design/2026-10-14-speech-v3-2-tts.md` §2; speech record §12 row 5). Transcribed from
//! token-station-server's native arm `TtsUpstream::Mimo` (`handler/audio/tts.rs:483-518`) and its
//! tables `mimo_tts_audio_format`, `resolve_mimo_voice` and `build_mimo_tts_body`
//! (`tts_providers.rs:451-637`):
//!
//! - **Request**: `POST v1/chat/completions` under the row endpoint, `bearer` on the configured
//!   slot, JSON `{"model", "messages": [user?, {"role": "assistant", "content": input}], "audio":
//!   {"format", "voice"?}, "stream": false}`; every other client member is dropped.
//! - **Format**: the client `response_format` lower-cased; absent, `wav` or `mp3` → `wav`
//!   (`audio/wav`), `pcm` or `pcm16` → `pcm16` (`audio/pcm`, little-endian, deliberately not
//!   `audio/L16`); `opus`, `aac`, `flac` and any other string refused with the native texts; a
//!   non-string is absent; an elided one is refused.
//! - **Model classes** from the upstream id lower-cased: *design* (`voicedesign`), *clone*
//!   (`voiceclone`), *preset* otherwise. **Voice**: absent, not a string or `""` is `mimo_default`;
//!   on a preset model the `OpenAI` names fold to `mimo_default`, anything else verbatim; on a clone
//!   or design model verbatim. A design model's `audio` carries no `voice`.
//! - **User message**: `instructions` when not blank after trimming (sent untrimmed); else, on a
//!   design model, the client `voice` as sent; added only when that text is not blank.
//! - **Immutable paths** `messages`, `stream`, `audio.format`, `model` (native `body_rules`).
//!   Characters from the request, `upper` equal to the count.
//! - **Response**: `json` with `/choices/*/message/audio/data` elided; `choices[0]`'s audio string
//!   is a base64 `inline` artifact typed by the requested format; missing, not a string or empty is
//!   `unknown`. The chat `usage` takes no part. A non-2xx carries the `OpenAI` error message.

use serde_json::{Map, Value, json};
use south_contracts::media::ResponseBodyFormV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechEncodingV1, SpeechMeteringFormV1,
    SpeechModelCapabilitiesV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::ProviderConfig;

use crate::speech_shared::{
    bearer, capabilities, descriptor, facts, holds_audio, inline_outcome, input, internal, invalid,
    is_blob, json_body, json_object, non_2xx, openai_error_message, prepared, required_forms,
    scalar_count, serves, state_characters, unknown, view_status, with_refs,
};
use crate::{ComponentResultV1, SpeechComponentV1, SpeechOutcomeV1};

/// The package name this reference is published as.
pub const NAME: &str = "speech-mimo";
/// The package version.
pub const VERSION: &str = "1.0.2";
/// The world this reference exports.
pub const WORLD: &str = "speech-adapter-v1";
/// The family, as a host's speech bindings name it.
pub const MIMO_SPEECH: &str = "mimo-speech";

const FORMS: [SpeechMeteringFormV1; 1] = [SpeechMeteringFormV1::Characters];
/// The native arm's `body_rules` (`tts.rs:65`).
const IMMUTABLE_BODY_PATHS: [&str; 4] = ["messages", "stream", "audio.format", "model"];
/// Every choice's audio string, so the base64 never enters the sandbox.
const ELISION_PATHS: [&str; 1] = ["/choices/*/message/audio/data"];
/// The audio the native arm decodes: the first choice's.
const AUDIO_POINTER: &str = "/choices/0/message/audio/data";
/// The voice an absent, non-string or empty `voice` resolves to.
const DEFAULT_VOICE: &str = "mimo_default";
const SUPPORTED: &str = "Supported: wav, pcm16 (mp3 is served as wav).";

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct MimoSpeechReferenceV1;

/// `mimo_tts_audio_format`: the upstream format word for a lower-cased client format.
fn format_word(format: &str) -> ComponentResultV1<&'static str> {
    let format = format.to_lowercase();
    match format.as_str() {
        "wav" | "mp3" => Ok("wav"),
        "pcm" | "pcm16" => Ok("pcm16"),
        "opus" | "aac" | "flac" => Err(invalid(format!(
            "Xiaomi MiMo TTS cannot emit response_format='{format}'. {SUPPORTED}"
        ))),
        _ => Err(invalid(format!("Unknown response_format='{format}'. {SUPPORTED}"))),
    }
}

/// `MimoTtsFormat::content_type`: `pcm16` is little-endian PCM, which `audio/L16` (big-endian,
/// RFC 2586) would mislabel.
fn media_type(word: &str) -> &'static str {
    if word == "pcm16" { "audio/pcm" } else { "audio/wav" }
}

/// The upstream format the client's `response_format` asks for.
fn upstream_format(body: &Map<String, Value>) -> ComponentResultV1<&'static str> {
    match body.get("response_format") {
        Some(value) if is_blob(value) => {
            Err(invalid("'response_format' is longer than the gateway accepts"))
        }
        Some(Value::String(format)) => format_word(format),
        _ => Ok("wav"),
    }
}

/// `map_openai_voice_to_mimo`: `OpenAI`'s standard names fold to the default preset.
fn preset_voice(voice: &str) -> &str {
    match voice.to_lowercase().as_str() {
        "alloy" | "echo" | "fable" | "onyx" | "nova" | "shimmer" | "ash" | "sage" | "coral"
        | "ballad" | "verse" => DEFAULT_VOICE,
        _ => voice,
    }
}

/// The value `audio.voice` carries (`resolve_mimo_voice`): an elided voice is forwarded as an
/// `as_is` reference, as native forwards a value its table does not map.
fn voice(raw: Option<&Value>, preset: bool) -> Value {
    match raw {
        Some(value) if is_blob(value) => with_refs(value),
        Some(Value::String(voice)) if !voice.is_empty() => {
            json!(if preset { preset_voice(voice) } else { voice.as_str() })
        }
        _ => json!(DEFAULT_VOICE),
    }
}

/// A text that may head the user message: an elided one counts as not blank (V3-2 record §2).
fn not_blank(value: Option<&Value>) -> Option<Value> {
    match value {
        Some(value) if is_blob(value) => Some(with_refs(value)),
        Some(Value::String(text)) if !text.trim().is_empty() => Some(json!(text)),
        _ => None,
    }
}

impl SpeechComponentV1 for MimoSpeechReferenceV1 {
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
        capabilities(config, MIMO_SPEECH, &FORMS)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        serves(config, MIMO_SPEECH)?;
        let forms = required_forms(context, &FORMS)?;
        let body = json_body(request)?;
        let text = input(body)?;
        let format = upstream_format(body)?;
        let model = context.upstream_model.to_lowercase();
        let design = model.contains("voicedesign");
        let preset = !design && !model.contains("voiceclone");

        let mut messages = Vec::new();
        let user = not_blank(body.get("instructions"))
            .or_else(|| if design { not_blank(body.get("voice")) } else { None });
        if let Some(content) = user {
            messages.push(json!({ "role": "user", "content": content }));
        }
        messages.push(json!({ "role": "assistant", "content": text }));
        let audio = if design {
            json!({ "format": format })
        } else {
            json!({ "format": format, "voice": voice(body.get("voice"), preset) })
        };
        let template = json!({
            "model": context.upstream_model,
            "messages": messages,
            "audio": audio,
            "stream": false,
        });
        let count = scalar_count(text);
        prepared(
            facts(count, &forms, media_type(format)),
            descriptor("v1/chat/completions", None, bearer(config), &template, &[])?,
            ResponseBodyFormV1::Json,
            &ELISION_PATHS,
            &IMMUTABLE_BODY_PATHS,
            &json!({ "format": format, "characters": count }),
        )
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
        let Some(body) = json_object(response) else {
            return Ok(unknown("a 2xx without a JSON object body"));
        };
        // `choices[0]` of an array, as native's `get(0)`: an object keyed "0" is not a choice.
        let audio = body
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.pointer("/message/audio/data"));
        if !holds_audio(audio) {
            return Ok(unknown("a 2xx without audio"));
        }
        let format =
            state["format"].as_str().ok_or_else(|| internal("the state has no audio format"))?;
        inline_outcome(
            AUDIO_POINTER,
            SpeechEncodingV1::Base64,
            media_type(format),
            state_characters(state)?,
        )
    }
}
