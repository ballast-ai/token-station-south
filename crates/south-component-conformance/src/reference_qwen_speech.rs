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
//!
//! **Transcription** (1.1.0, design record `docs/design/2026-10-21-speech-qwen-asr.md`): Qwen-ASR
//! (`qwen3-asr-flash`) through the same synchronous path, read from the host's multipart view.
//!
//! - **Request**: JSON `{"model", "input": {"messages": [{"role": "system", "content": [{"text":
//!   <prompt>}]}?, {"role": "user", "content": [{"audio": <data URL>}]}]}, "parameters"?:
//!   {"asr_options": {"language"?, "enable_itn"?}}}`. The audio is a `$south.ref` to the `file`
//!   part's blob with the `data_url` transform (media contract 1), which the host expands. The text
//!   parts `language` (as given) and `enable_itn` (`true` / `false`, trimmed, any ASCII case) fill
//!   `asr_options`, `prompt` the system message (an approximation: Qwen reads it as recognition
//!   context); empty values are not sent, every other part is dropped. Immutable paths `model`,
//!   `input`, so request extras can reach `parameters.asr_options`.
//! - **Audio**: typed by the part's media type when it is `audio/*` or `video/*` (lower-cased),
//!   else, when the part has none or `application/octet-stream`, by the file name's extension;
//!   anything else is refused. A data URL longer than 10,000,000 bytes (the documented 10 MB, read
//!   as the smaller unit) is refused before dispatch. The call reserves at most 300 seconds (the
//!   documented 5-minute cap).
//! - **Client format**: `json` (absent) or `text`, exactly; `verbose_json`, `srt` and `vtt` are
//!   refused (the synchronous API returns no timestamps), as is any other value; `stream=true` is
//!   refused. `translate` is not declared.
//! - **Response**: `json`. A non-2xx and a 2xx carrying a `DashScope` `code` read as for
//!   synthesis; a body that is not an object is `unknown`. Seconds: `usage.seconds` when a
//!   non-negative integer (`reported`), `unknown` when absent, `null` or anything else, never a
//!   fallback. Transcript: `output.choices[0].message.content[0].text`, a string (`""` included),
//!   else `unknown`; delivered as `{"text"}` or plain text.

use serde_json::{Map, Value, json};
use south_contracts::media::ResponseBodyFormV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechCharacterMeterV1, SpeechCharacterSourceV1,
    SpeechCharactersV1, SpeechFactsV1, SpeechMeteringFormV1, SpeechModelCapabilitiesV1,
    SpeechOperationV1, SpeechReservationV1, SpeechSecondsBasisV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

use crate::asr_shared::{
    Transcript, first_text, malformed_duration, no_transcript, not_an_object, plain_text,
    recognition_forms, request_parts, succeeded, text_only_json, unreported_duration,
};
use crate::bailian_image::dashscope_error;
use crate::speech_shared::{
    BLOB_KEY, bearer, descriptor, facts, input, internal, invalid, is_blob, json_body, json_object,
    non_2xx_with, prepared, required_forms, scalar_count, serves, state_characters, status_code,
    unknown, url_outcome, view_status, with_refs,
};
use crate::{ComponentResultV1, SpeechComponentV1, SpeechOutcomeV1};

/// The package name this reference is published as.
pub const NAME: &str = "speech-qwen";
/// The package version.
pub const VERSION: &str = "1.1.4";
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
/// A transcription body's immutable paths: the routed model and the audio (record §3.1).
const ASR_IMMUTABLE_BODY_PATHS: [&str; 2] = ["model", "input"];
/// The longest data URL the upstream reads: 10 MB, read as the smaller, decimal unit (record §3.4).
const MAX_AUDIO_DATA_URL_BYTES: u64 = 10_000_000;
/// The longest audio the synchronous API transcribes, in seconds (5 minutes; record §3.4).
const MAX_AUDIO_SECONDS: u64 = 300;
/// The media type a part with none, or with this one, is typed from its file name.
const OCTET_STREAM: &str = "application/octet-stream";
/// File name extensions (lower-cased) and the media type each audio file is sent as.
const EXTENSION_MEDIA_TYPES: [(&str, &str); 12] = [
    ("aac", "audio/aac"),
    ("amr", "audio/amr"),
    ("flac", "audio/flac"),
    ("m4a", "audio/mp4"),
    ("mp3", "audio/mpeg"),
    ("mp4", "video/mp4"),
    ("mpeg", "audio/mpeg"),
    ("mpga", "audio/mpeg"),
    ("ogg", "audio/ogg"),
    ("opus", "audio/opus"),
    ("wav", "audio/wav"),
    ("webm", "audio/webm"),
];
/// Where the transcript is under `output`.
const TRANSCRIPT_POINTER: &str = "/choices/0/message/content/0/text";

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

/// The media type the audio's data URL carries (record §3.3): the part's own `audio/*` or
/// `video/*` type, lower-cased; else, when the part has none or `application/octet-stream`, the
/// type of the file name's extension; else refused.
fn audio_media_type(file: &Value) -> ComponentResultV1<String> {
    match file["media_type"].as_str().map(str::to_ascii_lowercase) {
        Some(media_type) if media_type != OCTET_STREAM => {
            if media_type.starts_with("audio/") || media_type.starts_with("video/") {
                Ok(media_type)
            } else {
                Err(invalid(format!(
                    "the audio file's media type '{media_type}' is not an audio type"
                )))
            }
        }
        _ => file["filename"]
            .as_str()
            .and_then(|name| name.rsplit_once('.'))
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .and_then(|extension| {
                EXTENSION_MEDIA_TYPES.iter().find(|(known, _)| *known == extension)
            })
            .map(|(_, media_type)| (*media_type).to_owned())
            .ok_or_else(|| {
                invalid(
                    "cannot tell the audio file's format: send it with an audio media type or a \
                     file name with an audio extension",
                )
            }),
    }
}

/// The length of `data:<media_type>;base64,<payload>` for a file of `bytes` bytes.
fn data_url_bytes(media_type: &str, bytes: u64) -> u64 {
    let prefix = u64::try_from("data:;base64,".len() + media_type.len()).unwrap_or(u64::MAX);
    bytes.div_ceil(3).saturating_mul(4).saturating_add(prefix)
}

/// The client format (record §3.5): `json` (absent) or `text`, compared exactly.
fn asr_format(parts: &[Value]) -> ComponentResultV1<&'static str> {
    match first_text(parts, "response_format") {
        None | Some("json") => Ok("json"),
        Some("text") => Ok("text"),
        Some(format @ ("verbose_json" | "srt" | "vtt")) => Err(invalid(format!(
            "Bailian Qwen-ASR cannot return response_format='{format}': its synchronous API \
             returns no timestamps. Supported: json, text."
        ))),
        Some(format) => {
            Err(invalid(format!("Unknown response_format='{format}'. Supported: json, text.")))
        }
    }
}

/// `enable_itn` (record §3.2): `true` or `false`, trimmed, in any ASCII case; absent or empty is
/// not sent.
fn enable_itn(parts: &[Value]) -> ComponentResultV1<Option<bool>> {
    match first_text(parts, "enable_itn").map(str::trim) {
        None | Some("") => Ok(None),
        Some(value) if value.eq_ignore_ascii_case("true") => Ok(Some(true)),
        Some(value) if value.eq_ignore_ascii_case("false") => Ok(Some(false)),
        Some(_) => Err(invalid("'enable_itn' must be true or false")),
    }
}

/// A non-empty text part's value.
fn non_empty<'a>(parts: &'a [Value], name: &str) -> Option<&'a str> {
    first_text(parts, name).filter(|value| !value.is_empty())
}

/// `prepare` of a transcription (record §3).
fn prepare_transcription(
    config: &ProviderConfig,
    request: &Value,
    context: &SpeechCallContextV1,
) -> ComponentResultV1<PreparedSpeechCallV1> {
    let forms = recognition_forms(config, context, false)?;
    let parts = request_parts(request)?;
    let format = asr_format(parts)?;
    let file = parts
        .iter()
        .find(|part| part["kind"] == "file" && part["name"] == "file")
        .ok_or_else(|| invalid("Missing 'file' field"))?;
    let media_type = audio_media_type(file)?;
    let bytes = file["bytes"].as_u64().ok_or_else(|| internal("the file part has no size"))?;
    if data_url_bytes(&media_type, bytes) > MAX_AUDIO_DATA_URL_BYTES {
        return Err(invalid(
            "the audio file is larger than Bailian Qwen-ASR accepts (10 MB once base64-encoded)",
        ));
    }
    let mut options = Map::new();
    if let Some(language) = non_empty(parts, "language") {
        options.insert("language".to_owned(), json!(language));
    }
    if let Some(itn) = enable_itn(parts)? {
        options.insert("enable_itn".to_owned(), json!(itn));
    }
    let mut messages = Vec::with_capacity(2);
    if let Some(prompt) = non_empty(parts, "prompt") {
        messages.push(json!({ "role": "system", "content": [{ "text": prompt }] }));
    }
    let audio = json!({ "$south.ref": {
        "blob": file["blob"], "transform": "data_url", "media_type": media_type,
    } });
    messages.push(json!({ "role": "user", "content": [{ "audio": audio }] }));
    let mut template =
        json!({ "model": context.upstream_model, "input": { "messages": messages } });
    if !options.is_empty() {
        template["parameters"] = json!({ "asr_options": options });
    }
    let call_facts = SpeechFactsV1 {
        operation: SpeechOperationV1::Transcribe,
        characters: None,
        metering_forms: forms,
        reservation: Some(SpeechReservationV1 {
            max_output_tokens: None,
            max_seconds: Some(MAX_AUDIO_SECONDS),
        }),
        output_media_type: None,
    };
    prepared(
        call_facts,
        descriptor(GENERATION_PATH, None, bearer(config), &template, &[])?,
        ResponseBodyFormV1::Json,
        &[],
        &ASR_IMMUTABLE_BODY_PATHS,
        &json!({ "op": "transcribe", "format": format }),
    )
}

/// The 2xx of a transcription (record §3.6), after the `code` check.
fn transcription_outcome(state: &Value, body: &Map<String, Value>) -> SpeechOutcomeV1 {
    let seconds = match body.get("usage").and_then(|usage| usage.get("seconds")) {
        None | Some(Value::Null) => return unreported_duration(),
        Some(value) => match value.as_u64() {
            Some(seconds) => whole_seconds(seconds),
            None => return malformed_duration(),
        },
    };
    let text = match body.get("output").and_then(|output| output.pointer(TRANSCRIPT_POINTER)) {
        Some(Value::String(text)) => Transcript::Text(text),
        Some(value) if value.get(BLOB_KEY).is_some() => Transcript::Elided(value),
        _ => return no_transcript(),
    };
    let delivery =
        if state["format"] == "text" { plain_text(&text) } else { text_only_json(&text) };
    match delivery {
        Ok(delivery) => succeeded(delivery, Some((seconds, SpeechSecondsBasisV1::Reported))),
        Err(outcome) => outcome,
    }
}

/// A whole number of audio seconds as the meter's value.
#[allow(clippy::cast_precision_loss, reason = "an audio duration in seconds is far below 2^53")]
const fn whole_seconds(seconds: u64) -> f64 {
    seconds as f64
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
        crate::asr_shared::capabilities(config, BAILIAN_QWEN_SPEECH, &FORMS, false)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        serves(config, BAILIAN_QWEN_SPEECH)?;
        if context.operation != SpeechOperationV1::Synthesize {
            return prepare_transcription(config, request, context);
        }
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
        let transcribing = state["op"] == "transcribe";
        let Some(body) = json_object(response) else {
            return Ok(if transcribing {
                not_an_object()
            } else {
                unknown("a 2xx without a JSON object body")
            });
        };
        if carries_code(body) {
            return Ok(SpeechOutcomeV1::Unknown {
                reason: "a 2xx carrying a DashScope error code".to_owned(),
                error: Some(dashscope_error(502, ErrorCode::ProviderProtocolError, json)),
            });
        }
        if transcribing {
            return Ok(transcription_outcome(state, body));
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
