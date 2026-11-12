//! The `bailian-minimax-speech` family of `speech-minimax` (1.2.0, design record
//! `docs/design/2026-11-10-speech-bailian-synthesizer-and-minimax.md` §3): `MiniMax` synthesis hosted
//! by Model Studio, through `DashScope` multimodal generation with `MiniMax`'s own body under `input`.
//!
//! There is no native arm to transcribe: the request and the answer follow Model Studio's published
//! page (record §6), and every upstream fact is unverified until the check after cutover.
//!
//! - **Request**: `POST api/v1/services/aigc/multimodal-generation/generation` under the row
//!   endpoint, `bearer`, no `X-DashScope-SSE`, no `?GroupId=` (the family has no config key). JSON
//!   `{"model", "input": {"text", "output_format": "hex", "voice_setting", "audio_setting"}}`;
//!   `voice_setting` and `audio_setting` are built by the `minimax-speech` rules unchanged; `stream`
//!   is not sent; every other client member is dropped.
//! - **Immutable paths** `model`, `input.text`, `input.output_format`, `input.audio_setting.format`,
//!   `input.voice_setting.voice_id`.
//! - **Response**: `json` with `/output/data/audio` elided. On a 2xx, in order: a body that is not
//!   a JSON object is `unknown`; a non-empty string `code` is `unknown` with a 502 (V4 P5);
//!   `output.base_resp` by the table shared with `minimax-speech`; the hex audio at
//!   `output.data.audio` (missing, not a string or empty is `unknown`); then the characters:
//!   `usage.characters`, else `output.extra_info.usage_characters`, else the request's count, a `0`
//!   or malformed value in the first one present being `unknown`. `usage.count` (a cloned voice's
//!   one-off unlock) is not read (SV-Q15). A non-2xx reads the `DashScope` `{"code", "message"}`
//!   answer.

use serde_json::{Map, Value, json};
use south_contracts::media::ResponseBodyFormV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechCharacterMeterV1, SpeechCharacterSourceV1,
    SpeechCharactersV1, SpeechEncodingV1,
};
use token_station_protocol::{ErrorCode, ProviderConfig};

use super::{FORMS, media_type, settings, upstream_format, voice_setting, weighted_count};
use crate::bailian_image::{carries_dashscope_code, dashscope_error};
use crate::minimax_shared::{BaseRespV1, MALFORMED_STATUS_CODE, base_resp, failure_reason};
use crate::speech_shared::{
    bearer, descriptor, facts, holds_audio, inline_outcome, input, internal, json_body,
    non_2xx_with, prepared, required_forms, scalar_count, state_characters, status_code, unknown,
};
use crate::{ComponentResultV1, SpeechOutcomeV1};

/// The synchronous multimodal-generation path (record §3.2).
const GENERATION_PATH: &str = "api/v1/services/aigc/multimodal-generation/generation";
/// Record §3.2.
const IMMUTABLE_BODY_PATHS: [&str; 5] = [
    "model",
    "input.text",
    "input.output_format",
    "input.audio_setting.format",
    "input.voice_setting.voice_id",
];
/// The hex audio, so it never enters the sandbox.
const AUDIO_POINTER: &str = "/output/data/audio";

/// `prepare` of the `bailian-minimax-speech` family (record §3.2).
pub(super) fn prepare(
    config: &ProviderConfig,
    request: &Value,
    context: &SpeechCallContextV1,
) -> ComponentResultV1<PreparedSpeechCallV1> {
    let forms = required_forms(context, &FORMS)?;
    let body = json_body(request)?;
    let text = input(body)?;
    let format = upstream_format(body)?;
    let mut audio_setting = settings(body.get("audio_setting"));
    audio_setting.insert("format".to_owned(), json!(format));
    let template = json!({
        "model": context.upstream_model,
        "input": {
            "text": text,
            "output_format": "hex",
            "voice_setting": voice_setting(body),
            "audio_setting": audio_setting,
        },
    });
    let count = scalar_count(text);
    let mut call_facts = facts(count, &forms, media_type(format));
    call_facts.characters = Some(SpeechCharactersV1 { count, upper: Some(weighted_count(text)) });
    prepared(
        call_facts,
        descriptor(GENERATION_PATH, None, bearer(config), &template, &[])?,
        ResponseBodyFormV1::Json,
        &[AUDIO_POINTER],
        &IMMUTABLE_BODY_PATHS,
        &json!({ "family": super::BAILIAN_MINIMAX_SPEECH, "format": format, "characters": count }),
    )
}

/// A character count the upstream reported (SV-Q7): `Ok(None)` when absent or `null`, the count
/// when a positive integer, `Err` for `0` or any other value.
fn reported(value: Option<&Value>) -> Result<Option<u64>, &'static str> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => match value.as_u64() {
            Some(0) => Err("the upstream reported 0 characters for delivered audio"),
            Some(characters) => Ok(Some(characters)),
            None => Err("a malformed character count"),
        },
    }
}

/// The characters (record §3.3 step 5): `usage.characters`, else
/// `output.extra_info.usage_characters`; the first one present is judged.
fn usage_characters(body: &Map<String, Value>) -> Result<Option<u64>, &'static str> {
    if let Some(characters) = reported(body.get("usage").and_then(|usage| usage.get("characters")))?
    {
        return Ok(Some(characters));
    }
    reported(
        body.get("output")
            .and_then(|output| output.get("extra_info"))
            .and_then(|extra| extra.get("usage_characters")),
    )
}

/// `parse-response` of the `bailian-minimax-speech` family (record §3.3).
pub(super) fn parse_response(
    state: &Value,
    response: &Value,
    status: u16,
) -> ComponentResultV1<SpeechOutcomeV1> {
    let json = response.pointer("/body/json").unwrap_or(&Value::Null);
    if !(200..300).contains(&status) {
        let error = dashscope_error(status, status_code(status), json);
        return Ok(non_2xx_with(status, response, error));
    }
    let Some(body) = json.as_object() else {
        return Ok(unknown("a 2xx without a JSON object body"));
    };
    if carries_dashscope_code(body) {
        return Ok(SpeechOutcomeV1::Unknown {
            reason: "a 2xx carrying a DashScope error code".to_owned(),
            error: Some(dashscope_error(502, ErrorCode::ProviderProtocolError, json)),
        });
    }
    let output = body.get("output").unwrap_or(&Value::Null);
    match base_resp(output) {
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
    if !holds_audio(output.get("data").and_then(|data| data.get("audio"))) {
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
