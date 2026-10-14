//! The `OpenAI` speech protocol pieces `speech-openai-compatible` and `speech-groq` share (speech
//! world batch V3-1 record §2, §4): the forwarded client body, the client format's media type, and
//! the reader of the token-priced `sse` answer.
//!
//! Transcribed from token-station-server's native arms: the body is the client's JSON with `model`
//! replaced (`tts.rs:664-672`, `:621-637`), the media type table is `openai_tts_content_type`
//! (`tts_providers.rs:72-81`) and the event reader is `decode_openai_tts_sse`
//! (`tts_providers.rs:7-62`), with the record's differences: malformed counts and empty audio are
//! `unknown` (§10 P2, P3) and the input breakdown is reported only when it closes (P10).

use serde_json::{Map, Value};
use south_contracts::JsonPointerV1;
use south_contracts::speech::{
    MAX_SPEECH_SEGMENTS, SpeechArtifactFormV1, SpeechArtifactV1, SpeechEncodingV1,
    SpeechMeteringV1, SpeechSegmentV1, SpeechTokensV1,
};

use crate::speech_shared::{BLOB_KEY, internal, state_characters, unknown, with_refs};
use crate::{ComponentResultV1, SpeechDeliveryV1, SpeechOutcomeV1};

/// The event that carries one base64 audio fragment in `audio`.
pub const DELTA_EVENT: &str = "speech.audio.delta";
/// The event that carries the call's `usage`.
pub const DONE_EVENT: &str = "speech.audio.done";
/// The response elision path of the `sse` form: every event's `data.audio` (§2).
pub const AUDIO_ELISION_PATH: &str = "/*/data/audio";

/// `openai_tts_content_type` over the client's `response_format` (lower-cased by the caller, absent
/// or not a string = `mp3`).
pub fn media_type_of(format: Option<&str>) -> &'static str {
    match format {
        Some("wav") => "audio/wav",
        Some("flac") => "audio/flac",
        Some("opus") => "audio/opus",
        Some("aac") => "audio/aac",
        Some("pcm") => "audio/pcm",
        _ => "audio/mpeg",
    }
}

/// The client's `response_format` lower-cased (ASCII, as `openai_tts_content_type`), when it is a
/// string the component can read; an elided one is not read and counts as absent.
pub fn client_format(body: &Map<String, Value>) -> Option<String> {
    body.get("response_format").and_then(Value::as_str).map(str::to_ascii_lowercase)
}

/// The client's body forwarded: every member as sent (an elided one as an `as_is` reference),
/// `model` set to the upstream id.
pub fn forwarded_body(body: &Map<String, Value>, upstream_model: &str) -> Map<String, Value> {
    let mut out: Map<String, Value> =
        body.iter().map(|(key, value)| (key.clone(), with_refs(value))).collect();
    out.insert("model".to_owned(), Value::String(upstream_model.to_owned()));
    out
}

/// The byte length of an audio fragment in the view: a placeholder's `bytes`, or a plain string's
/// length from a host that did not elide it; `None` for anything that is not a string.
fn fragment_bytes(audio: &Value) -> Option<u64> {
    if let Some(blob) = audio.get(BLOB_KEY) {
        return blob["bytes"].as_u64();
    }
    audio.as_str().map(|text| u64::try_from(text.len()).unwrap_or(u64::MAX))
}

/// A `usage` the native reader keeps: `input_tokens` and `output_tokens` both non-negative JSON
/// integers, not both 0 (`tts_providers.rs:37-48`; negative and fractional counts are not counts,
/// §10 P3).
fn valid_usage(usage: &Value) -> Option<(u64, u64)> {
    let input = usage.get("input_tokens")?.as_u64()?;
    let output = usage.get("output_tokens")?.as_u64()?;
    (input > 0 || output > 0).then_some((input, output))
}

/// `input_token_details.text_tokens` and `audio_tokens`, only when both are counts and they sum to
/// `input` (I3-2 GI-Q4 precedent, §10 P10).
fn breakdown(usage: &Value, input: u64) -> (Option<u64>, Option<u64>) {
    let details = &usage["input_token_details"];
    match (details["text_tokens"].as_u64(), details["audio_tokens"].as_u64()) {
        (Some(text), Some(audio)) if text.checked_add(audio) == Some(input) => {
            (Some(text), Some(audio))
        }
        _ => (None, None),
    }
}

/// The token-priced answer (§2): a `segments` artifact over every `speech.audio.delta` event's
/// `audio` string, in event order, and `tokens` from the last `speech.audio.done` event whose
/// `usage` the native reader keeps. Events whose `data` is not an object, `[DONE]` and other
/// events are skipped. No valid usage, no delta or only empty deltas is `unknown`. `characters` is
/// reported too when the state carries the request's count, which it does only when the host
/// requires that form as well.
pub fn sse_outcome(state: &Value, response: &Value) -> ComponentResultV1<SpeechOutcomeV1> {
    let Some(events) = response["body"]["sse"].as_array() else {
        return Ok(unknown("a 2xx that is not an event stream"));
    };
    let mut items = Vec::new();
    let mut audio_bytes: u64 = 0;
    let mut usage = None;
    for (index, event) in events.iter().enumerate() {
        let data = &event["data"];
        if !data.is_object() {
            continue;
        }
        match data["type"].as_str() {
            Some(DELTA_EVENT) => {
                if let Some(bytes) = fragment_bytes(&data["audio"]) {
                    items.push(SpeechSegmentV1 {
                        pointer: JsonPointerV1::parse(&format!("/{index}/data/audio"))
                            .map_err(|_| internal("built an invalid pointer"))?,
                        encoding: SpeechEncodingV1::Base64,
                    });
                    audio_bytes = audio_bytes.saturating_add(bytes);
                }
            }
            Some(DONE_EVENT) => {
                if let Some(counts) = data.get("usage").and_then(valid_usage) {
                    usage = Some((counts, &data["usage"]));
                }
            }
            _ => {}
        }
    }
    let Some(((input, output), usage)) = usage else {
        return Ok(unknown("the event stream reported no valid token usage"));
    };
    if items.is_empty() || audio_bytes == 0 {
        return Ok(unknown("a 2xx without audio"));
    }
    if items.len() > MAX_SPEECH_SEGMENTS {
        return Ok(unknown("more audio fragments than one artifact carries"));
    }
    let (input_text, input_audio) = breakdown(usage, input);
    let characters =
        if state.get("characters").is_some() { Some(state_characters(state)?) } else { None };
    Ok(SpeechOutcomeV1::Succeeded {
        delivery: SpeechDeliveryV1::Audio(SpeechArtifactV1 {
            form: SpeechArtifactFormV1::Segments { items },
            media_type: media_type_of(state["format"].as_str()).to_owned(),
            container: None,
        }),
        metering: SpeechMeteringV1 {
            characters,
            tokens: Some(SpeechTokensV1 {
                input: Some(input),
                output: Some(output),
                input_text,
                input_audio,
            }),
            ..SpeechMeteringV1::default()
        },
    })
}
