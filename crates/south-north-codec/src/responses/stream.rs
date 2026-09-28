use super::output::{reasoning_item, response_object};
use super::tools::{LOCAL_SHELL, ToolKind, ToolMap, custom_input, restore_map, tool_item};
use super::{ResponsesContext, ResponsesFrame, ResponsesReasoningMode, invalid};
use crate::CodecError;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use token_station_protocol::{
    ChatResponse, Choice, Content, ContentPart, ErrorCode, Extensions, FinishReason, Message, Role,
    StreamEvent, ToolCall, Usage,
};
#[derive(Debug, Clone)]
struct Slot {
    kind: u8,
    index: u32,
    source_index: u32,
    id: String,
    text: String,
    signature: String,
    call_id: String,
    name: String,
    closed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamStatus {
    Active,
    AwaitingError,
    Completed,
    Failed,
}
/// One stream's render state. Hosts own lifetime and stream tables.
#[derive(Debug, Clone)]
pub struct ResponsesSseState {
    context: ResponsesContext,
    status: StreamStatus,
    created: bool,
    sequence: u64,
    slots: Vec<Slot>,
    lookup: BTreeMap<(u8, u32), usize>,
    usage: Usage,
    finish: Option<FinishReason>,
    restore: Option<ToolMap>,
}
impl ResponsesSseState {
    #[must_use]
    pub fn new(context: ResponsesContext) -> Self {
        Self {
            context,
            status: StreamStatus::Active,
            created: false,
            sequence: 0,
            slots: Vec::new(),
            lookup: BTreeMap::new(),
            usage: Usage::default(),
            finish: None,
            restore: None,
        }
    }
    #[must_use]
    pub const fn is_terminated(&self) -> bool {
        matches!(self.status, StreamStatus::Completed | StreamStatus::Failed)
    }
    /// A successful terminal IR snapshot for host-owned continuation history.
    #[must_use]
    pub fn terminal_response(&self) -> Option<ChatResponse> {
        if self.status != StreamStatus::Completed {
            return None;
        }
        let mut messages: BTreeMap<u32, Message> = BTreeMap::new();
        // Preserve the community continuation order: thinking, text, then calls.
        for kind in [1, 0, 2] {
            for slot in self.slots.iter().filter(|slot| slot.kind == kind) {
                let message = messages.entry(slot.source_index).or_insert_with(|| {
                    let mut message = Message::text(Role::Assistant, "");
                    message.content = None;
                    message
                });
                if slot.kind == 2 {
                    message.tool_calls.push(ToolCall {
                        id: slot.call_id.clone(),
                        name: slot.name.clone(),
                        arguments: slot.text.clone(),
                    });
                } else {
                    let part = if slot.kind == 1 {
                        ContentPart::Thinking {
                            thinking: slot.text.clone(),
                            signature: (!slot.signature.is_empty()).then(|| slot.signature.clone()),
                        }
                    } else {
                        ContentPart::Text { text: slot.text.clone() }
                    };
                    if let Some(Content::Parts(parts)) = &mut message.content {
                        parts.push(part);
                    } else {
                        message.content = Some(Content::Parts(vec![part]));
                    }
                }
            }
        }
        Some(ChatResponse {
            id: self.context.response_id.clone(),
            model: self.context.model.clone(),
            choices: messages
                .into_iter()
                .map(|(index, message)| Choice {
                    index,
                    message,
                    finish_reason: self.finish.clone(),
                    stop_sequence: None,
                })
                .collect(),
            usage: self.usage,
            extensions: Extensions::new(),
        })
    }
    fn start(&mut self, out: &mut Vec<ResponsesFrame>) -> Result<(), CodecError> {
        if !self.created {
            self.created = true;
            self.emit("response.created", json!({"response":{"id":self.context.response_id,"object":"response","created_at":self.context.created_at,"model":self.context.model,"status":"in_progress","output":[]}}), out)?;
        }
        Ok(())
    }
    fn emit(
        &mut self,
        kind: &str,
        mut data: Value,
        out: &mut Vec<ResponsesFrame>,
    ) -> Result<(), CodecError> {
        data["type"] = json!(kind);
        data["sequence_number"] = json!(self.sequence);
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("sequence_number", "stream event count overflow"))?;
        out.push(ResponsesFrame { event: kind.into(), data });
        Ok(())
    }
    fn slot(&mut self, kind: u8, index: u32) -> Result<(usize, bool), CodecError> {
        if let Some(&position) = self.lookup.get(&(kind, index))
            && !self.slots[position].closed
        {
            return Ok((position, false));
        }
        let position = self.slots.len();
        let output_index = u32::try_from(position)
            .map_err(|_| invalid("output_index", "too many output items"))?;
        let prefix = match kind {
            0 => "msg",
            1 => "rs",
            _ => "fc",
        };
        let id = format!("{prefix}_{}_{}", self.context.response_id, output_index);
        self.slots.push(Slot {
            kind,
            index: output_index,
            source_index: index,
            id,
            text: String::new(),
            signature: String::new(),
            call_id: String::new(),
            name: String::new(),
            closed: false,
        });
        self.lookup.insert((kind, index), position);
        Ok((position, true))
    }
    fn item(&self, slot: &Slot) -> Result<Value, CodecError> {
        match slot.kind {
            0 => Ok(
                json!({"type":"message","id":slot.id,"role":"assistant","status":"completed","content":[{"type":"output_text","text":slot.text}]}),
            ),
            1 => Ok(reasoning_item(
                &slot.id,
                &slot.text,
                (!slot.signature.is_empty()).then_some(slot.signature.as_str()),
                &self.context,
            )),
            _ => tool_item(
                &slot.call_id,
                &slot.name,
                &slot.text,
                "completed",
                self.restore.as_ref().ok_or_else(|| invalid("tools", "missing render context"))?,
            ),
        }
    }
    fn close(&mut self, position: usize, out: &mut Vec<ResponsesFrame>) -> Result<(), CodecError> {
        if self.slots[position].closed {
            return Ok(());
        }
        let slot = self.slots[position].clone();
        let item = self.item(&slot)?;
        match slot.kind {
            0 => {
                self.emit("response.output_text.done",json!({"item_id":slot.id,"output_index":slot.index,"content_index":0,"text":slot.text}),out)?;
                self.emit("response.content_part.done",json!({"item_id":slot.id,"output_index":slot.index,"content_index":0,"part":{"type":"output_text","text":slot.text}}),out)?;
            }
            1 => {
                if self.context.reasoning == ResponsesReasoningMode::RawContent {
                    self.emit("response.reasoning_text.done",json!({"item_id":slot.id,"output_index":slot.index,"content_index":0,"text":slot.text}),out)?;
                } else {
                    self.emit("response.reasoning_summary_text.done",json!({"item_id":slot.id,"output_index":slot.index,"summary_index":0,"text":slot.text}),out)?;
                    self.emit("response.reasoning_summary_part.done",json!({"item_id":slot.id,"output_index":slot.index,"summary_index":0,"part":{"type":"summary_text","text":slot.text}}),out)?;
                }
            }
            _ => {
                let kind = self.restore.as_ref().and_then(|m| m.get(&slot.name));
                match kind {
                    Some(ToolKind::Custom) => {
                        let input = custom_input(&slot.text);
                        let input = input.as_str().ok_or_else(|| {
                            invalid(
                                "tool_calls.arguments.input",
                                "custom tool input must be a string",
                            )
                        })?;
                        if !input.is_empty() {
                            self.emit("response.custom_tool_call_input.delta",json!({"item_id":item["id"],"output_index":slot.index,"delta":input}),out)?;
                        }
                        self.emit(
                            "response.custom_tool_call_input.done",
                            json!({"item_id":item["id"],"output_index":slot.index,"input":input}),
                            out,
                        )?;
                    }
                    Some(ToolKind::Shell) => self.emit(
                        "response.output_item.added",
                        json!({"output_index":slot.index,"item":item}),
                        out,
                    )?,
                    _ => self.emit(
                        "response.function_call_arguments.done",
                        json!({"item_id":slot.id,"output_index":slot.index,"arguments":slot.text}),
                        out,
                    )?,
                }
            }
        }
        self.emit(
            "response.output_item.done",
            json!({"output_index":slot.index,"item":item}),
            out,
        )?;
        self.slots[position].closed = true;
        Ok(())
    }
    fn close_raw_reasoning(&mut self, out: &mut Vec<ResponsesFrame>) -> Result<(), CodecError> {
        if self.context.reasoning == ResponsesReasoningMode::RawContent {
            for position in 0..self.slots.len() {
                if self.slots[position].kind == 1 {
                    self.close(position, out)?;
                }
            }
        }
        Ok(())
    }
}
/// Render events without transport I/O; terminal events are idempotent.
///
/// An empty batch explicitly starts an active stream once, emitting created.
/// After a mapping error only an explicit Error can terminate the stream;
/// empty batches and normal events then produce no output.
pub fn responses_frames(
    events: &[StreamEvent],
    state: &mut ResponsesSseState,
) -> Result<Vec<ResponsesFrame>, CodecError> {
    let sequence = state.sequence;
    let created = state.created;
    let result = render(events, state);
    if result.is_err() {
        // No frames from a failed call reach the transport. Only an explicit
        // Error may proceed, so accumulated content need not be cloned back.
        state.sequence = sequence;
        state.created = created;
        state.status = StreamStatus::AwaitingError;
    }
    result
}
fn render(
    events: &[StreamEvent],
    state: &mut ResponsesSseState,
) -> Result<Vec<ResponsesFrame>, CodecError> {
    let mut out = Vec::new();
    if events.is_empty() && state.status == StreamStatus::Active {
        state.start(&mut out)?;
    }
    for event in events {
        if state.is_terminated() {
            break;
        }
        if state.status == StreamStatus::AwaitingError
            && !matches!(event, StreamEvent::Error { .. })
        {
            continue;
        }
        if state.restore.is_none() && !matches!(event, StreamEvent::Error { .. }) {
            state.restore = Some(restore_map(&state.context.inbound_tools)?);
        }
        state.start(&mut out)?;
        match event {
            StreamEvent::Delta { index, content } => {
                state.close_raw_reasoning(&mut out)?;
                let (position, new) = state.slot(0, *index)?;
                let item_id = state.slots[position].id.clone();
                let output_index = state.slots[position].index;
                if new {
                    state.emit("response.output_item.added",json!({"output_index":output_index,"item":{"type":"message","id":item_id,"status":"in_progress","role":"assistant","content":[]}}),&mut out)?;
                    state.emit("response.content_part.added",json!({"item_id":item_id,"output_index":output_index,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),&mut out)?;
                }
                state.slots[position].text.push_str(content);
                state.emit("response.output_text.delta",json!({"item_id":item_id,"output_index":output_index,"content_index":0,"delta":content}),&mut out)?;
            }
            StreamEvent::ThinkingDelta { index, thinking_delta, .. } => {
                let (position, new) = state.slot(1, *index)?;
                let item_id = state.slots[position].id.clone();
                let output_index = state.slots[position].index;
                if new {
                    state.emit("response.output_item.added",json!({"output_index":output_index,"item":{"type":"reasoning","id":item_id,"status":"in_progress","summary":[]}}),&mut out)?;
                    if state.context.reasoning == ResponsesReasoningMode::Summary {
                        state.emit("response.reasoning_summary_part.added",json!({"item_id":item_id,"output_index":output_index,"summary_index":0,"part":{"type":"summary_text","text":""}}),&mut out)?;
                    }
                }
                state.slots[position].text.push_str(thinking_delta);
                let (raw, field) = if state.context.reasoning == ResponsesReasoningMode::RawContent
                {
                    ("response.reasoning_text.delta", "content_index")
                } else {
                    ("response.reasoning_summary_text.delta", "summary_index")
                };
                let mut payload =
                    json!({"item_id":item_id,"output_index":output_index,"delta":thinking_delta});
                payload[field] = json!(0);
                state.emit(raw, payload, &mut out)?;
            }
            StreamEvent::ThinkingSignatureDelta { index, signature_delta, .. } => {
                if state.context.render_legacy_encrypted_reasoning {
                    let (position, new) = state.slot(1, *index)?;
                    if new {
                        let slot = &state.slots[position];
                        let payload = json!({"output_index":slot.index,"item":{"type":"reasoning","id":slot.id,"status":"in_progress","summary":[]}});
                        state.emit("response.output_item.added", payload, &mut out)?;
                    }
                    state.slots[position].signature.push_str(signature_delta);
                }
            }
            StreamEvent::RedactedThinking { .. } => {}
            StreamEvent::ToolCallDelta { index, id, name, arguments_delta } => {
                render_tool(state, *index, id.as_ref(), name.as_ref(), arguments_delta, &mut out)?;
            }
            StreamEvent::Usage { usage } => state.usage.absorb(*usage),
            StreamEvent::Finish { finish_reason, .. } => state.finish.clone_from(finish_reason),
            StreamEvent::Done { finish_reason, .. } => {
                for position in 0..state.slots.len() {
                    state.close(position, &mut out)?;
                }
                let output = state
                    .slots
                    .iter()
                    .map(|slot| state.item(slot))
                    .collect::<Result<Vec<_>, _>>()?;
                if finish_reason.is_some() {
                    state.finish.clone_from(finish_reason);
                }
                let response =
                    response_object(&state.context, &output, state.usage, state.finish.as_ref())?;
                let kind = if response["status"] == "incomplete" {
                    "response.incomplete"
                } else {
                    "response.completed"
                };
                state.emit(kind, json!({"response":response}), &mut out)?;
                state.status = StreamStatus::Completed;
            }
            StreamEvent::Error { error } => {
                let response = json!({"id":state.context.response_id,"object":"response","created_at":state.context.created_at,"model":state.context.model,"status":"failed","output":[],"error":{"type":"server_error","code":error_code(error.code),"message":error.message}});
                state.emit("response.failed", json!({"response":response}), &mut out)?;
                state.status = StreamStatus::Failed;
            }
        }
    }
    Ok(out)
}

fn render_tool(
    state: &mut ResponsesSseState,
    index: u32,
    id: Option<&String>,
    name: Option<&String>,
    arguments_delta: &str,
    out: &mut Vec<ResponsesFrame>,
) -> Result<(), CodecError> {
    state.close_raw_reasoning(out)?;
    let (position, new) = state.slot(2, index)?;
    if new {
        let fallback = format!("call_{}_{}", state.context.response_id, index);
        let call_id = if state.context.allow_incomplete_tool_calls {
            id.cloned().unwrap_or(fallback)
        } else {
            id.cloned().ok_or_else(|| invalid("tool_call.id", "first fragment requires an id"))?
        };
        let call_name = if state.context.allow_incomplete_tool_calls {
            name.cloned().unwrap_or_default()
        } else {
            name.cloned()
                .ok_or_else(|| invalid("tool_call.name", "first fragment requires a name"))?
        };
        let map = state.restore.as_ref().ok_or_else(|| invalid("tools", "missing context"))?;
        let custom = matches!(map.get(&call_name), Some(ToolKind::Custom));
        let item_id = format!("{}_{call_id}", if custom { "ctc" } else { "fc" });
        state.slots[position].call_id.clone_from(&call_id);
        state.slots[position].name.clone_from(&call_name);
        state.slots[position].id = item_id;
        if call_name != LOCAL_SHELL {
            let item = tool_item(&call_id, &call_name, "", "in_progress", map)?;
            let output_index = state.slots[position].index;
            state.emit(
                "response.output_item.added",
                json!({"output_index":output_index,"item":item}),
                out,
            )?;
        }
    } else {
        let slot = &mut state.slots[position];
        if id.is_some_and(|id| id != &slot.call_id) {
            return Err(invalid("tool_call.id", "identity changed between fragments"));
        }
        if let Some(name) = name {
            if slot.name.is_empty() && state.context.allow_incomplete_tool_calls {
                slot.name.clone_from(name);
            } else if name != &slot.name {
                return Err(invalid("tool_call.name", "identity changed between fragments"));
            }
        }
    }
    state.slots[position].text.push_str(arguments_delta);
    let slot = &state.slots[position];
    let custom =
        state.restore.as_ref().is_some_and(|m| matches!(m.get(&slot.name), Some(ToolKind::Custom)));
    if !custom && slot.name != LOCAL_SHELL {
        let payload = json!({"item_id":slot.id,"output_index":slot.index,"delta":arguments_delta});
        state.emit("response.function_call_arguments.delta", payload, out)?;
    }

    Ok(())
}

const fn error_code(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::InvalidRequest => "invalid_request",
        ErrorCode::Auth => "authentication_error",
        ErrorCode::PaymentRequired => "insufficient_quota",
        ErrorCode::RateLimit => "rate_limit_exceeded",
        ErrorCode::Capacity => "server_overloaded",
        ErrorCode::Capability => "unsupported_capability",
        ErrorCode::ContextLength => "context_length_exceeded",
        ErrorCode::ContentPolicy => "invalid_prompt",
        ErrorCode::UpstreamUnavailable | ErrorCode::TransportTruncated => "server_error",
        ErrorCode::ProviderProtocolError => "upstream_protocol_error",
        ErrorCode::Timeout => "timeout",
        ErrorCode::Internal => "internal_error",
    }
}
