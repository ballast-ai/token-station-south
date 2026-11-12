//! The `bailian-speech-synthesizer` family of `speech-qwen` (1.3.0, design record
//! `docs/design/2026-11-10-speech-bailian-synthesizer-and-minimax.md` §2): Model Studio's HTTP
//! `SpeechSynthesizer` API, which serves Qwen-Audio-TTS and `CosyVoice`.
//!
//! Unlike the Qwen-TTS family there is no native arm to transcribe: the request and the answer
//! follow Model Studio's published pages (record §6), and every upstream fact is unverified until
//! the check after cutover.
//!
//! - **Request**: `POST api/v1/services/audio/tts/SpeechSynthesizer` under the row endpoint,
//!   `bearer`, no `X-DashScope-SSE` (the non-streaming answer). JSON `{"model", "input": {"text",
//!   "voice", "format", "rate"?, "instruction"?}}`; every other client member is dropped.
//! - **Voice**: required (SV-Q14). Absent, not a string, `""` or one of the eleven `OpenAI` names
//!   (any case) is refused before admission; any other string is sent verbatim, an elided one as an
//!   `as_is` reference.
//! - **Format**: lower-cased; absent or `mp3` (`audio/mpeg`), `wav`, `pcm`, `opus`; any other string
//!   is refused naming the four; a non-string is absent. `sample_rate` is not sent.
//! - **`speed` → `rate`**: a JSON number, sent as a float; out-of-range values are sent (the
//!   upstream's 4xx is released by the host's P29 allow-list). **`instructions` → `instruction`**:
//!   the client's string, when it is one.
//! - **Immutable paths** `model`, `input.text`, `input.format`, `input.voice`. Characters: `count`
//!   the scalar count, `upper` the weighted count (ASCII 1, other scalars 2), as for Qwen-TTS.
//! - **Response**: `json`, nothing elided. The 2xx reading is the Qwen-TTS one, typed by the
//!   requested format; a non-2xx reads the `DashScope` `{"code", "message"}` answer.

use serde_json::{Map, Value, json};
use south_contracts::media::ResponseBodyFormV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechCharactersV1, SpeechMeteringFormV1,
};
use token_station_protocol::ProviderConfig;

use super::{FORMS, weighted_count};
use crate::ComponentResultV1;
use crate::speech_shared::{
    bearer, descriptor, facts, input, invalid, is_blob, json_body, prepared, required_forms,
    scalar_count, with_refs,
};

/// The non-streaming `SpeechSynthesizer` path (record §2.2).
const SPEECH_SYNTHESIZER_PATH: &str = "api/v1/services/audio/tts/SpeechSynthesizer";
/// Record §2.2.
const IMMUTABLE_BODY_PATHS: [&str; 4] = ["model", "input.text", "input.format", "input.voice"];
const SUPPORTED: &str = "Supported: mp3, wav, pcm, opus.";
/// The names an `OpenAI` client sends, which name no `SpeechSynthesizer` voice.
const OPENAI_VOICES: [&str; 11] = [
    "alloy", "echo", "fable", "onyx", "nova", "shimmer", "ash", "sage", "coral", "ballad", "verse",
];

/// The upstream `format` the client's `response_format` asks for (record §2.2).
fn format_word(body: &Map<String, Value>) -> ComponentResultV1<&'static str> {
    match body.get("response_format") {
        Some(value) if is_blob(value) => {
            Err(invalid("'response_format' is longer than the gateway accepts"))
        }
        Some(Value::String(format)) => match format.to_lowercase().as_str() {
            "mp3" => Ok("mp3"),
            "wav" => Ok("wav"),
            "pcm" => Ok("pcm"),
            "opus" => Ok("opus"),
            other => Err(invalid(format!(
                "Bailian SpeechSynthesizer cannot emit response_format='{other}'. {SUPPORTED}"
            ))),
        },
        _ => Ok("mp3"),
    }
}

/// The delivered media type of an upstream `format` (record §2.2; whether `opus` arrives in an Ogg
/// container is `opus_media_type_observed`).
pub(super) fn media_type(word: &str) -> &'static str {
    match word {
        "wav" => "audio/wav",
        "pcm" => "audio/pcm",
        "opus" => "audio/opus",
        _ => "audio/mpeg",
    }
}

/// The voice (record §2.2, SV-Q14): the client must name one of the model's voices.
fn voice(raw: Option<&Value>) -> ComponentResultV1<Value> {
    match raw {
        Some(value) if is_blob(value) => Ok(with_refs(value)),
        Some(Value::String(voice)) if !voice.is_empty() => {
            if OPENAI_VOICES.contains(&voice.to_lowercase().as_str()) {
                Err(invalid(format!(
                    "Bailian SpeechSynthesizer has no voice '{voice}': name one of the model's \
                     voices."
                )))
            } else {
                Ok(json!(voice))
            }
        }
        _ => Err(invalid(
            "Bailian SpeechSynthesizer needs a 'voice': name one of the model's voices.",
        )),
    }
}

/// `prepare` of the `bailian-speech-synthesizer` family (record §2.2, §2.3).
pub(super) fn prepare(
    config: &ProviderConfig,
    request: &Value,
    context: &SpeechCallContextV1,
) -> ComponentResultV1<PreparedSpeechCallV1> {
    let forms: Vec<SpeechMeteringFormV1> = required_forms(context, &FORMS)?;
    let body = json_body(request)?;
    let text = input(body)?;
    let format = format_word(body)?;
    let mut upstream_input = Map::new();
    upstream_input.insert("text".to_owned(), json!(text));
    upstream_input.insert("voice".to_owned(), voice(body.get("voice"))?);
    upstream_input.insert("format".to_owned(), json!(format));
    if let Some(speed) = body.get("speed").and_then(Value::as_f64) {
        // A client `1` is sent as `1.0`, as V3-2 sends MiniMax's `speed`.
        upstream_input.insert("rate".to_owned(), json!(speed));
    }
    match body.get("instructions") {
        Some(value) if is_blob(value) => {
            upstream_input.insert("instruction".to_owned(), with_refs(value));
        }
        Some(value @ Value::String(_)) => {
            upstream_input.insert("instruction".to_owned(), value.clone());
        }
        _ => {}
    }
    let template = json!({ "model": context.upstream_model, "input": upstream_input });
    let count = scalar_count(text);
    let mut call_facts = facts(count, &forms, media_type(format));
    call_facts.characters = Some(SpeechCharactersV1 { count, upper: Some(weighted_count(text)) });
    prepared(
        call_facts,
        descriptor(SPEECH_SYNTHESIZER_PATH, None, bearer(config), &template, &[])?,
        ResponseBodyFormV1::Json,
        &[],
        &IMMUTABLE_BODY_PATHS,
        &json!({ "family": super::BAILIAN_SPEECH_SYNTHESIZER, "format": format, "characters": count }),
    )
}
