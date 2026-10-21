//! The native reference implementation of the `speech-minimax` component.
//!
//! `MiniMax` Speech T2A v2 (speech world batch V3-2, design record
//! `docs/design/2026-10-14-speech-v3-2-tts.md` §3; speech record §12 row 6). Transcribed from
//! token-station-server's native arm `TtsUpstream::Minimax` (`handler/audio/tts.rs:555-580`), its
//! tables `minimax_tts_audio_format`, `resolve_minimax_voice`, `build_minimax_tts_body` and
//! `decode_minimax_tts_audio` (`tts_providers.rs:814-845`, `:980-1162`), and the `GroupId` arm of
//! `engine/upstream.rs`:
//!
//! - **Request**: `POST v1/t2a_v2` under the row endpoint, `bearer` on the configured slot,
//!   `?GroupId=` only when the family's config key `group_id` is set (digits, IM-Q3). JSON
//!   `{"model", "text", "stream": false, "output_format": "hex", "voice_setting", "audio_setting"}`;
//!   every other client member is dropped.
//! - **`voice_setting`**: the client's object (any other type ignored) with `voice_id` overwritten
//!   by the resolved voice; a `null` `speed` removed; then, when it has no `speed` and the client's
//!   top-level `speed` is a JSON number, that number **as a float**. **`audio_setting`**: the
//!   client's object with `format` overwritten.
//! - **Voice**: absent, not a string or `""` is `Wise_Woman`; the `OpenAI` names fold to it, anything
//!   else verbatim. **Format**: lower-cased; absent or `mp3` (`audio/mpeg`), `wav`, `flac`, `pcm`
//!   (`audio/pcm`); `opus`, `aac` and any other string refused with the native texts.
//! - **Immutable paths** `text`, `output_format`, `stream`, `audio_setting.format`,
//!   `voice_setting.voice_id`, `model` (native `body_rules`). Characters: `count` the scalar count,
//!   `upper` the weighted count (ASCII 1, other scalars 2; native `weighted_upper_chars`).
//! - **Response**: `json` with `/data/audio` elided. On a 2xx, in order: `base_resp` (the table
//!   shared with `image-minimax`), the hex audio (missing, not a string or empty is `unknown`), then
//!   `extra_info.usage_characters`: a positive integer is the upstream's count, absent or `null` is
//!   the request's, `0` or anything else is `unknown` (SV-Q7). A non-2xx carries
//!   `base_resp.status_msg`.

use serde_json::{Map, Value, json};
use south_contracts::media::ResponseBodyFormV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechCharacterMeterV1, SpeechCharacterSourceV1,
    SpeechCharactersV1, SpeechEncodingV1, SpeechMeteringFormV1, SpeechModelCapabilitiesV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::ProviderConfig;

use crate::minimax_shared::{
    BaseRespV1, MALFORMED_STATUS_CODE, base_resp, failure_reason, status_msg,
};
use crate::speech_shared::{
    bearer, capabilities, descriptor, facts, holds_audio, inline_outcome, input, internal, invalid,
    is_blob, json_body, non_2xx, prepared, required_forms, scalar_count, serves, state_characters,
    unknown, view_status, with_refs,
};
use crate::{ComponentResultV1, SpeechComponentV1, SpeechOutcomeV1};

/// The package name this reference is published as.
pub const NAME: &str = "speech-minimax";
/// The package version.
pub const VERSION: &str = "1.0.11";
/// The world this reference exports.
pub const WORLD: &str = "speech-adapter-v1";
/// The family, as a host's speech bindings name it.
pub const MINIMAX_SPEECH: &str = "minimax-speech";
/// The family's one config key: the account's group id, sent as `?GroupId=` when set (IM-Q3).
pub const GROUP_ID: &str = "group_id";

const FORMS: [SpeechMeteringFormV1; 1] = [SpeechMeteringFormV1::Characters];
/// The native arm's `body_rules` (`tts.rs:67-74`).
const IMMUTABLE_BODY_PATHS: [&str; 6] =
    ["text", "output_format", "stream", "audio_setting.format", "voice_setting.voice_id", "model"];
/// The hex audio, so it never enters the sandbox (a 10-minute mp3 is about 19 MB as hex).
const AUDIO_POINTER: &str = "/data/audio";
/// The voice an absent, non-string or empty `voice` resolves to.
const DEFAULT_VOICE: &str = "Wise_Woman";
const SUPPORTED: &str = "Supported: mp3, wav, flac, pcm.";

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct MiniMaxSpeechReferenceV1;

/// `minimax_tts_audio_format`: the upstream format word for a lower-cased client format.
fn format_word(format: &str) -> ComponentResultV1<&'static str> {
    let format = format.to_lowercase();
    match format.as_str() {
        "mp3" => Ok("mp3"),
        "wav" => Ok("wav"),
        "flac" => Ok("flac"),
        "pcm" => Ok("pcm"),
        "opus" | "aac" => {
            Err(invalid(format!("MiniMax T2A cannot emit response_format='{format}'. {SUPPORTED}")))
        }
        _ => Err(invalid(format!("Unknown response_format='{format}'. {SUPPORTED}"))),
    }
}

/// `MinimaxTtsFormat::content_type`: `pcm` is headerless little-endian PCM, which `audio/L16`
/// (big-endian, RFC 2586) would mislabel.
fn media_type(word: &str) -> &'static str {
    match word {
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "pcm" => "audio/pcm",
        _ => "audio/mpeg",
    }
}

/// The upstream format the client's `response_format` asks for.
fn upstream_format(body: &Map<String, Value>) -> ComponentResultV1<&'static str> {
    match body.get("response_format") {
        Some(value) if is_blob(value) => {
            Err(invalid("'response_format' is longer than the gateway accepts"))
        }
        Some(Value::String(format)) => format_word(format),
        _ => Ok("mp3"),
    }
}

/// `resolve_minimax_voice`: an elided voice is forwarded as an `as_is` reference, as native
/// forwards a value its table does not map.
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

/// The client's settings object, its members forwarded (an elided string as an `as_is`
/// reference); any other type, an elided string included, is ignored (native `as_object`).
fn settings(raw: Option<&Value>) -> Map<String, Value> {
    match raw {
        Some(value @ Value::Object(_)) if !is_blob(value) => match with_refs(value) {
            Value::Object(members) => members,
            _ => Map::new(),
        },
        _ => Map::new(),
    }
}

/// `build_minimax_tts_body`'s `voice_setting`.
fn voice_setting(body: &Map<String, Value>) -> Map<String, Value> {
    let mut setting = settings(body.get("voice_setting"));
    setting.insert("voice_id".to_owned(), voice(body.get("voice")));
    if setting.get("speed").is_some_and(Value::is_null) {
        setting.remove("speed");
    }
    if !setting.contains_key("speed")
        && let Some(speed) = body.get("speed").and_then(Value::as_f64)
    {
        // Native `json!(f64)`: a client `1` is sent as `1.0`.
        setting.insert("speed".to_owned(), json!(speed));
    }
    setting
}

/// The weighted count of `input`: each ASCII scalar 1, any other 2 (native `weighted_upper_chars`,
/// the bound of `MiniMax`'s reservation).
fn weighted_count(text: &str) -> u64 {
    text.chars().map(|character| if character.is_ascii() { 1 } else { 2 }).sum()
}

/// `extra_info.usage_characters` (record §9, SV-Q7): `Ok(None)` when absent or `null`, the count
/// when a positive integer, `Err` for `0` or any other value.
fn usage_characters(body: &Map<String, Value>) -> Result<Option<u64>, &'static str> {
    match body.get("extra_info").and_then(|extra| extra.get("usage_characters")) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => match value.as_u64() {
            Some(0) => Err("the upstream reported 0 characters for delivered audio"),
            Some(characters) => Ok(Some(characters)),
            None => Err("a malformed character count"),
        },
    }
}

impl SpeechComponentV1 for MiniMaxSpeechReferenceV1 {
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
        capabilities(config, MINIMAX_SPEECH, &FORMS)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        serves(config, MINIMAX_SPEECH)?;
        let forms = required_forms(context, &FORMS)?;
        let body = json_body(request)?;
        let text = input(body)?;
        let format = upstream_format(body)?;
        let mut audio_setting = settings(body.get("audio_setting"));
        audio_setting.insert("format".to_owned(), json!(format));
        let template = json!({
            "model": context.upstream_model,
            "text": text,
            "stream": false,
            "output_format": "hex",
            "voice_setting": voice_setting(body),
            "audio_setting": audio_setting,
        });
        let query = config
            .declared
            .get(GROUP_ID)
            .map(|group| json!([{ "name": "GroupId", "value": group }]));
        let count = scalar_count(text);
        let mut call_facts = facts(count, &forms, media_type(format));
        call_facts.characters =
            Some(SpeechCharactersV1 { count, upper: Some(weighted_count(text)) });
        prepared(
            call_facts,
            descriptor("v1/t2a_v2", query, bearer(config), &template, &[])?,
            ResponseBodyFormV1::Json,
            &[AUDIO_POINTER],
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
            return Ok(non_2xx(status, response, status_msg));
        }
        let json = &response["body"]["json"];
        let Some(body) = json.as_object() else {
            return Ok(unknown("a 2xx without a JSON object body"));
        };
        match base_resp(json) {
            BaseRespV1::Success => {}
            BaseRespV1::Malformed => return Ok(unknown(MALFORMED_STATUS_CODE)),
            BaseRespV1::Refused(error) => return Ok(SpeechOutcomeV1::Rejected { error }),
            BaseRespV1::Failed { code, error } => {
                return Ok(SpeechOutcomeV1::Unknown {
                    reason: failure_reason(code),
                    error: Some(error),
                });
            }
        }
        let audio = body.get("data").and_then(|data| data.get("audio"));
        if !holds_audio(audio) {
            return Ok(unknown("a 2xx without audio"));
        }
        let characters = match usage_characters(body) {
            Ok(Some(value)) => {
                SpeechCharacterMeterV1 { value, source: SpeechCharacterSourceV1::Upstream }
            }
            Ok(None) => state_characters(state)?,
            Err(reason) => return Ok(unknown(reason)),
        };
        let format =
            state["format"].as_str().ok_or_else(|| internal("the state has no audio format"))?;
        inline_outcome(AUDIO_POINTER, SpeechEncodingV1::Hex, media_type(format), characters)
    }
}
