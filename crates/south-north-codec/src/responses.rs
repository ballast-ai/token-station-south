//! Responses client wire mappings with explicit host facts and per-stream state.
mod output;
mod replay;
mod request;
mod stream;
mod tools;
use crate::CodecError;
pub use output::responses_response;
pub use replay::{
    CLAUDE_REASONING_REPLAY_CAPABILITY, ReasoningReplayBlock, ReasoningReplayCarrier,
    decode_reasoning_replay_carrier, encode_reasoning_replay_carrier,
};
pub use request::chat_request_from_responses;
use serde_json::Value;
pub use stream::{ResponsesSseState, responses_frames};
use token_station_protocol::{ChatResponse, StreamEvent};
use tools::{LOCAL_SHELL, namespace_name, tool_definitions};
/// Independent compatibility allowances for already-supported client shapes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Independent compatibility behaviors must not be coupled to a host mode."
)]
pub struct ResponsesRequestOptions {
    /// Accept a top-level Chat-shaped messages list, taking precedence over input.
    pub allow_messages: bool,
    /// Accept an empty input list for a host that resolves continuation history.
    pub allow_empty_input: bool,
    /// Accept legacy call identifiers and JSON-valued arguments/results.
    pub allow_call_aliases: bool,
    /// Carry unmodeled content through the existing IR Unknown variant.
    pub preserve_unknown_content: bool,
    /// Keep a text-only input array as IR parts instead of collapsing it to text.
    pub preserve_text_parts: bool,
}
/// Presentation only; neither mode grants reasoning replay authorization.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResponsesReasoningMode {
    RawContent,
    #[default]
    Summary,
}
/// Host-supplied response identity, time, and protocol restoration facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsesContext {
    pub response_id: String,
    pub model: String,
    pub created_at: i64,
    pub inbound_tools: Value,
    pub reasoning: ResponsesReasoningMode,
    /// Preserve legacy tolerance for tool identity arriving late.
    pub allow_incomplete_tool_calls: bool,
    /// Preserve an existing host's signature-as-encrypted presentation only.
    /// This does not establish carrier provenance or cross-provider compatibility.
    pub render_legacy_encrypted_reasoning: bool,
}
/// One SSE event and its complete JSON data payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsesFrame {
    pub event: String,
    pub data: Value,
}
fn invalid(field: impl Into<String>, detail: &str) -> CodecError {
    CodecError::unrenderable(field, detail)
}
fn unsupported(field: impl Into<String>, expected: &'static str) -> CodecError {
    CodecError::unknown_value(field, "an unsupported value", expected)
}
fn required_str<'a>(value: &'a Value, field: &str) -> Result<&'a str, CodecError> {
    value.as_str().ok_or_else(|| invalid(field, "must be a string"))
}
/// JSON facade for a WIT caller; uses exactly the typed request mapping.
pub fn responses_request_json(
    body: &str,
    options: &ResponsesRequestOptions,
) -> Result<String, CodecError> {
    let body = serde_json::from_str(body).map_err(|_| invalid("request", "invalid JSON"))?;
    serde_json::to_string(&chat_request_from_responses(&body, options)?)
        .map_err(|_| invalid("request", "IR serialization failed"))
}
/// Decode the IR response once and serialize the typed renderer's wire value.
pub fn responses_response_json(
    response: &str,
    context: &ResponsesContext,
) -> Result<String, CodecError> {
    let response: ChatResponse = serde_json::from_str(response)
        .map_err(|_| invalid("response", "invalid canonical response JSON"))?;
    Ok(responses_response(&response, context)?.to_string())
}
/// Render one WIT event while retaining typed state; the returned JSON is {data: SSE}.
pub fn responses_event_json(
    event: &str,
    state: &mut ResponsesSseState,
) -> Result<String, CodecError> {
    let event: StreamEvent = serde_json::from_str(event)
        .map_err(|_| invalid("event", "invalid canonical event JSON"))?;
    let frames = responses_frames(&[event], state)?;
    let mut data = String::new();
    for frame in frames {
        use std::fmt::Write;
        write!(data, "event: {}\ndata: {}\n\n", frame.event, frame.data)
            .map_err(|_| invalid("event", "SSE formatting failed"))?;
    }
    Ok(serde_json::json!({"data":data}).to_string())
}
