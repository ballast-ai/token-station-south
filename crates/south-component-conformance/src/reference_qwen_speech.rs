//! The native reference implementation of the `speech-qwen` component.
//!
//! Bailian Qwen-TTS through `DashScope` multimodal generation (speech world batch V4, design record
//! `docs/design/2026-10-15-speech-v4-tts.md` §2; speech record §12 row 7). Transcribed from
//! token-station-server's native arm `TtsUpstream::Bailian` (`handler/audio/tts.rs:526-547`,
//! `:1059-1082`) and its tables `qwen_tts_audio_format`, `resolve_qwen_voice`,
//! `build_qwen_tts_body` and `qwen_tts_audio_url` (`tts_providers.rs:638-734`):
//!
//! - **Request**: `POST api/v1/services/aigc/multimodal-generation/generation` under the row
//!   endpoint (the `DashScope` origin `image-qwen` uses), `bearer` on the configured slot, no
//!   `X-DashScope-Async`. JSON `{"model", "input": {"text", "voice", "language_type"?,
//!   "instructions"?}}`; every other client member is dropped.
//! - **Voice**: absent, not a string or `""` is `Cherry`; the eleven `OpenAI` names (any case) fold
//!   to it; anything else is sent verbatim. **`language_type`**: the client's string, else omitted.
//!   **`instructions`**: the client's string, only when the upstream id contains `instruct`.
//! - **Format**: lower-cased; absent, `wav` or `mp3` (served as WAV, the upstream's only output);
//!   any other string refused with native's text. Nothing is sent upstream for it.
//! - **Immutable paths** `input.text`, `model` (native `body_rules`). Characters: `count` the scalar
//!   count, `upper` the weighted count (ASCII 1, other scalars 2; native `weighted_upper_chars`).
//! - **Response**: `json`, nothing elided (the audio is behind a URL). On a 2xx, in order: a body
//!   that is not a JSON object is `unknown`; a non-empty string `code` is `unknown` with a 502
//!   `provider_protocol_error` envelope (I3-1 decision 8); `usage.characters` a positive integer is
//!   the upstream's count, absent or `null` the request's, `0` or anything else `unknown` (SV-Q7);
//!   `output.audio.url` not a non-empty string is `unknown`; otherwise a `url` artifact at
//!   `/output/audio/url`, `audio/wav`, with `upgrade_insecure` (SV-Q10: the upstream writes
//!   `http://` OSS links the host upgrades before its unchanged safe fetch rules). A non-2xx reads
//!   the `DashScope` `{"code", "message"}` answer `image-qwen` reads (`bailian_image`), else the
//!   body text.

use serde_json::{Map, Value, json};
use south_contracts::media::ResponseBodyFormV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechCharacterMeterV1, SpeechCharacterSourceV1,
    SpeechCharactersV1, SpeechMeteringFormV1, SpeechModelCapabilitiesV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

use crate::bailian_image::dashscope_error;
use crate::speech_shared::{
    bearer, capabilities, descriptor, facts, input, invalid, is_blob, json_body, non_2xx_with,
    prepared, required_forms, scalar_count, serves, state_characters, status_code, unknown,
    url_outcome, view_status, with_refs,
};
use crate::{ComponentResultV1, SpeechComponentV1, SpeechOutcomeV1};

/// The package name this reference is published as.
pub const NAME: &str = "speech-qwen";
/// The package version.
pub const VERSION: &str = "1.0.1";
/// The world this reference exports.
pub const WORLD: &str = "speech-adapter-v1";
/// The family, as a host's speech bindings name it.
pub const BAILIAN_QWEN_SPEECH: &str = "bailian-qwen-speech";

const FORMS: [SpeechMeteringFormV1; 1] = [SpeechMeteringFormV1::Characters];
/// The synchronous multimodal-generation path (native `tts.rs:545`).
const GENERATION_PATH: &str = "api/v1/services/aigc/multimodal-generation/generation";
/// The native arm's `body_rules` (`tts.rs:66`).
const IMMUTABLE_BODY_PATHS: [&str; 2] = ["input.text", "model"];
/// Where the audio URL is.
const AUDIO_URL_POINTER: &str = "/output/audio/url";
/// The upstream only returns WAV (native labels the fetched bytes so, `tts.rs:1079`).
const MEDIA_TYPE: &str = "audio/wav";
/// The voice an absent, non-string or empty `voice` resolves to.
const DEFAULT_VOICE: &str = "Cherry";

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct QwenSpeechReferenceV1;

/// `qwen_tts_audio_format`: absent, `wav` and `mp3` are served (as WAV); any other string is
/// refused naming the lower-cased value; a non-string is absent (native `as_str`).
fn check_format(body: &Map<String, Value>) -> ComponentResultV1<()> {
    match body.get("response_format") {
        Some(value) if is_blob(value) => {
            Err(invalid("'response_format' is longer than the gateway accepts"))
        }
        Some(Value::String(format)) => match format.to_lowercase().as_str() {
            "wav" | "mp3" => Ok(()),
            other => Err(invalid(format!(
                "Bailian Qwen3-TTS cannot emit response_format='{other}'. Supported: wav (mp3 is \
                 served as wav)."
            ))),
        },
        _ => Ok(()),
    }
}

/// `resolve_qwen_voice`: an elided voice is forwarded as an `as_is` reference, as native forwards a
/// value its table does not map.
fn voice(raw: Option<&Value>) -> Value {
    match raw {
        Some(value) if is_blob(value) => with_refs(value),
        Some(Value::String(voice)) if !voice.is_empty() => match voice.to_lowercase().as_str() {
            "alloy" | "echo" | "fable" | "onyx" | "nova" | "shimmer" | "ash" | "sage" | "coral"
            | "ballad" | "verse" => json!(DEFAULT_VOICE),
            _ => json!(voice),
        },
        _ => json!(DEFAULT_VOICE),
    }
}

/// A client string member forwarded as sent (an elided one as an `as_is` reference); `None` for
/// any other type or when absent (native `as_str`).
fn string_member(raw: Option<&Value>) -> Option<Value> {
    match raw {
        Some(value) if is_blob(value) => Some(with_refs(value)),
        Some(value @ Value::String(_)) => Some(value.clone()),
        _ => None,
    }
}

/// The weighted count of `input`: each ASCII scalar 1, any other 2 (native `weighted_upper_chars`,
/// the bound of Bailian's reservation, `tts.rs:300-303`, `:805-806`).
fn weighted_count(text: &str) -> u64 {
    text.chars().map(|character| if character.is_ascii() { 1 } else { 2 }).sum()
}

/// Whether a 2xx body names a `DashScope` error code: a non-empty string `code` (record §2.2 step 2).
fn carries_code(body: &Map<String, Value>) -> bool {
    body.get("code").and_then(Value::as_str).is_some_and(|code| !code.is_empty())
}

/// `usage.characters` (record §2.2 step 3, SV-Q7): `Ok(None)` when absent or `null`, the count when
/// a positive integer, `Err` for `0` or any other value.
fn usage_characters(body: &Map<String, Value>) -> Result<Option<u64>, &'static str> {
    match body.get("usage").and_then(|usage| usage.get("characters")) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => match value.as_u64() {
            Some(0) => Err("the upstream reported 0 characters for delivered audio"),
            Some(characters) => Ok(Some(characters)),
            None => Err("a malformed character count"),
        },
    }
}

impl SpeechComponentV1 for QwenSpeechReferenceV1 {
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
        capabilities(config, BAILIAN_QWEN_SPEECH, &FORMS)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        serves(config, BAILIAN_QWEN_SPEECH)?;
        let forms = required_forms(context, &FORMS)?;
        let body = json_body(request)?;
        let text = input(body)?;
        check_format(body)?;
        let mut upstream_input = Map::new();
        upstream_input.insert("text".to_owned(), json!(text));
        upstream_input.insert("voice".to_owned(), voice(body.get("voice")));
        if let Some(language_type) = string_member(body.get("language_type")) {
            upstream_input.insert("language_type".to_owned(), language_type);
        }
        // Native reads the model class from the upstream id (`upstream_model.contains("instruct")`,
        // case-sensitive): a plain model refuses the unknown field.
        if context.upstream_model.contains("instruct")
            && let Some(instructions) = string_member(body.get("instructions"))
        {
            upstream_input.insert("instructions".to_owned(), instructions);
        }
        let template = json!({ "model": context.upstream_model, "input": upstream_input });
        let count = scalar_count(text);
        let mut call_facts = facts(count, &forms, MEDIA_TYPE);
        call_facts.characters =
            Some(SpeechCharactersV1 { count, upper: Some(weighted_count(text)) });
        prepared(
            call_facts,
            descriptor(GENERATION_PATH, None, bearer(config), &template, &[])?,
            ResponseBodyFormV1::Json,
            &[],
            &IMMUTABLE_BODY_PATHS,
            &json!({ "characters": count }),
        )
    }

    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        let status = view_status(response);
        let json = response.pointer("/body/json").unwrap_or(&Value::Null);
        if !(200..300).contains(&status) {
            let error = dashscope_error(status, status_code(status), json);
            return Ok(non_2xx_with(status, response, error));
        }
        let Some(body) = json.as_object() else {
            return Ok(unknown("a 2xx without a JSON object body"));
        };
        if carries_code(body) {
            return Ok(SpeechOutcomeV1::Unknown {
                reason: "a 2xx carrying a DashScope error code".to_owned(),
                error: Some(dashscope_error(502, ErrorCode::ProviderProtocolError, json)),
            });
        }
        let characters = match usage_characters(body) {
            Ok(Some(value)) => {
                SpeechCharacterMeterV1 { value, source: SpeechCharacterSourceV1::Upstream }
            }
            Ok(None) => state_characters(state)?,
            Err(reason) => return Ok(unknown(reason)),
        };
        if json.pointer(AUDIO_URL_POINTER).and_then(Value::as_str).is_none_or(str::is_empty) {
            return Ok(unknown("a 2xx without audio"));
        }
        url_outcome(AUDIO_URL_POINTER, MEDIA_TYPE, true, characters)
    }
}
