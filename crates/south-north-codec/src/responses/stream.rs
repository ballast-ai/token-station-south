use super::output::{reasoning_item, response_object};
use super::tools::{LOCAL_SHELL, ToolKind, ToolMap, custom_input, restore_map, tool_item};
use super::{
    ReasoningReplayBlock, ReasoningReplayCarrier, ResponsesContext, ResponsesFrame,
    ResponsesReasoningMode, encode_reasoning_replay_carrier, invalid,
};
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
#[derive(Debug, Clone)]
enum ReplayBlockState {
    Thinking { thinking: String, signature: String },
    Redacted { data: String },
}
#[derive(Debug, Clone, Copy)]
enum ReplayLayout {
    Reasoning { index: u32, block_index: u32 },
    Text { position: usize },
    Tool { position: usize },
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
    replay_blocks: BTreeMap<(u32, u32), ReplayBlockState>,
    replay_layout: Vec<ReplayLayout>,
    last_replay_block: BTreeMap<u32, u32>,
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
            replay_blocks: BTreeMap::new(),
            replay_layout: Vec::new(),
            last_replay_block: BTreeMap::new(),
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
        if let Some(response) = self.replay_terminal_response() {
            return Some(response);
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
    fn replay_terminal_response(&self) -> Option<ChatResponse> {
        self.replay_carrier().ok().flatten()?;
        let choice_index = self.replay_blocks.keys().next().map_or(0, |(index, _)| *index);
        let mut parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut layout = Vec::new();
        for entry in &self.replay_layout {
            match *entry {
                ReplayLayout::Reasoning { index, block_index } => {
                    let block = self.replay_blocks.get(&(index, block_index))?;
                    layout.push(json!({"kind":"content","ordinal":parts.len()}));
                    parts.push(match block {
                        ReplayBlockState::Thinking { thinking, signature } => {
                            ContentPart::Thinking {
                                thinking: thinking.clone(),
                                signature: Some(signature.clone()),
                            }
                        }
                        ReplayBlockState::Redacted { data } => {
                            ContentPart::RedactedThinking { data: data.clone() }
                        }
                    });
                }
                ReplayLayout::Text { position } => {
                    let slot = self.slots.get(position)?;
                    layout.push(json!({"kind":"content","ordinal":parts.len()}));
                    parts.push(ContentPart::Text { text: slot.text.clone() });
                }
                ReplayLayout::Tool { position } => {
                    let slot = self.slots.get(position)?;
                    layout.push(json!({"kind":"tool_call","call_id":slot.call_id}));
                    tool_calls.push(ToolCall {
                        id: slot.call_id.clone(),
                        name: slot.name.clone(),
                        arguments: slot.text.clone(),
                    });
                }
            }
        }
        let mut extensions = Extensions::new();
        extensions
            .insert("reasoning_replay_protocol_family".into(), json!("claude-signed-thinking"));
        extensions.insert("reasoning_replay_block_layout".into(), json!(layout));
        Some(ChatResponse {
            id: self.context.response_id.clone(),
            model: self.context.model.clone(),
            choices: vec![Choice {
                index: choice_index,
                message: Message {
                    role: Role::Assistant,
                    content: Some(Content::Parts(parts)),
                    tool_calls,
                    tool_call_id: None,
                    name: None,
                    extensions,
                },
                finish_reason: self.finish.clone(),
                stop_sequence: None,
            }],
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
            1 => {
                let mut item = reasoning_item(
                    &slot.id,
                    &slot.text,
                    (!slot.signature.is_empty()).then_some(slot.signature.as_str()),
                    &self.context,
                );
                if let Some(carrier) = self.replay_carrier()? {
                    item["encrypted_content"] = json!(carrier);
                }
                Ok(item)
            }
            _ => tool_item(
                &slot.call_id,
                &slot.name,
                &slot.text,
                "completed",
                self.restore.as_ref().ok_or_else(|| invalid("tools", "missing render context"))?,
            ),
        }
    }
    fn replay_invalid() -> CodecError {
        CodecError::ReasoningReplayInvalid { field: "encrypted_content".to_owned() }
    }
    fn ensure_replay_block_order(
        &mut self,
        index: u32,
        block_index: u32,
    ) -> Result<(), CodecError> {
        if self.replay_blocks.contains_key(&(index, block_index)) {
            return Ok(());
        }
        if self.last_replay_block.get(&index).is_some_and(|last| block_index <= *last) {
            return Err(Self::replay_invalid());
        }
        self.last_replay_block.insert(index, block_index);
        self.replay_layout.push(ReplayLayout::Reasoning { index, block_index });
        Ok(())
    }
    fn replay_carrier(&self) -> Result<Option<String>, CodecError> {
        let replay_active = self.replay_blocks.values().any(|block| match block {
            ReplayBlockState::Thinking { signature, .. } => !signature.is_empty(),
            ReplayBlockState::Redacted { .. } => true,
        });
        if !replay_active {
            return Ok(None);
        }
        let mut blocks = Vec::new();
        let mut text_ordinal = 0u32;
        for entry in &self.replay_layout {
            match *entry {
                ReplayLayout::Reasoning { index, block_index } => {
                    match self
                        .replay_blocks
                        .get(&(index, block_index))
                        .ok_or_else(Self::replay_invalid)?
                    {
                        ReplayBlockState::Thinking { thinking, signature } => {
                            if signature.is_empty() {
                                return Err(Self::replay_invalid());
                            }
                            blocks.push(ReasoningReplayBlock::Thinking {
                                thinking: thinking.clone(),
                                signature: signature.clone(),
                            });
                        }
                        ReplayBlockState::Redacted { data } => blocks
                            .push(ReasoningReplayBlock::RedactedThinking { data: data.clone() }),
                    }
                }
                ReplayLayout::Text { position } => {
                    if self.slots.get(position).is_none_or(|slot| slot.kind != 0) {
                        return Err(Self::replay_invalid());
                    }
                    blocks.push(ReasoningReplayBlock::TextRef { ordinal: text_ordinal });
                    text_ordinal = text_ordinal.checked_add(1).ok_or_else(Self::replay_invalid)?;
                }
                ReplayLayout::Tool { position } => {
                    let call_id = self
                        .slots
                        .get(position)
                        .filter(|slot| slot.kind == 2 && !slot.call_id.is_empty())
                        .map(|slot| slot.call_id.clone())
                        .ok_or_else(Self::replay_invalid)?;
                    blocks.push(ReasoningReplayBlock::ToolCallRef { call_id });
                }
            }
        }
        encode_reasoning_replay_carrier(&ReasoningReplayCarrier::claude(blocks)).map(Some)
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
        if self.replay_carrier()?.is_some() {
            return Ok(());
        }
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
                    state.replay_layout.push(ReplayLayout::Text { position });
                    state.emit("response.output_item.added",json!({"output_index":output_index,"item":{"type":"message","id":item_id,"status":"in_progress","role":"assistant","content":[]}}),&mut out)?;
                    state.emit("response.content_part.added",json!({"item_id":item_id,"output_index":output_index,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),&mut out)?;
                }
                state.slots[position].text.push_str(content);
                state.emit("response.output_text.delta",json!({"item_id":item_id,"output_index":output_index,"content_index":0,"delta":content}),&mut out)?;
            }
            StreamEvent::ThinkingDelta { index, block_index, thinking_delta } => {
                render_thinking_delta(state, *index, *block_index, thinking_delta, &mut out)?;
            }
            StreamEvent::ThinkingSignatureDelta { index, block_index, signature_delta } => {
                render_signature_delta(state, *index, *block_index, signature_delta, &mut out)?;
            }
            StreamEvent::RedactedThinking { index, block_index, data } => {
                render_redacted(state, *index, *block_index, data, &mut out)?;
            }
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

fn render_thinking_delta(
    state: &mut ResponsesSseState,
    index: u32,
    block_index: u32,
    thinking_delta: &str,
    out: &mut Vec<ResponsesFrame>,
) -> Result<(), CodecError> {
    state.ensure_replay_block_order(index, block_index)?;
    match state.replay_blocks.entry((index, block_index)) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(ReplayBlockState::Thinking {
                thinking: thinking_delta.to_owned(),
                signature: String::new(),
            });
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => match entry.get_mut() {
            ReplayBlockState::Thinking { thinking, .. } => thinking.push_str(thinking_delta),
            ReplayBlockState::Redacted { .. } => {
                return Err(ResponsesSseState::replay_invalid());
            }
        },
    }
    let (position, new) = state.slot(1, index)?;
    let item_id = state.slots[position].id.clone();
    let output_index = state.slots[position].index;
    if new {
        state.emit("response.output_item.added",json!({"output_index":output_index,"item":{"type":"reasoning","id":item_id,"status":"in_progress","summary":[]}}),out)?;
        if state.context.reasoning == ResponsesReasoningMode::Summary {
            state.emit("response.reasoning_summary_part.added",json!({"item_id":item_id,"output_index":output_index,"summary_index":0,"part":{"type":"summary_text","text":""}}),out)?;
        }
    }
    state.slots[position].text.push_str(thinking_delta);
    let (raw, field) = if state.context.reasoning == ResponsesReasoningMode::RawContent {
        ("response.reasoning_text.delta", "content_index")
    } else {
        ("response.reasoning_summary_text.delta", "summary_index")
    };
    let mut payload = json!({"item_id":item_id,"output_index":output_index,"delta":thinking_delta});
    payload[field] = json!(0);
    state.emit(raw, payload, out)
}

fn render_signature_delta(
    state: &mut ResponsesSseState,
    index: u32,
    block_index: u32,
    signature_delta: &str,
    out: &mut Vec<ResponsesFrame>,
) -> Result<(), CodecError> {
    let Some(ReplayBlockState::Thinking { signature, .. }) =
        state.replay_blocks.get_mut(&(index, block_index))
    else {
        return Err(ResponsesSseState::replay_invalid());
    };
    signature.push_str(signature_delta);
    let (position, new) = state.slot(1, index)?;
    if new {
        let slot = &state.slots[position];
        let payload = json!({"output_index":slot.index,"item":{"type":"reasoning","id":slot.id,"status":"in_progress","summary":[]}});
        state.emit("response.output_item.added", payload, out)?;
    }
    if state.context.render_legacy_encrypted_reasoning {
        state.slots[position].signature.push_str(signature_delta);
    }
    Ok(())
}

fn render_redacted(
    state: &mut ResponsesSseState,
    index: u32,
    block_index: u32,
    data: &str,
    out: &mut Vec<ResponsesFrame>,
) -> Result<(), CodecError> {
    state.ensure_replay_block_order(index, block_index)?;
    if state
        .replay_blocks
        .insert((index, block_index), ReplayBlockState::Redacted { data: data.to_owned() })
        .is_some()
    {
        return Err(ResponsesSseState::replay_invalid());
    }
    let (position, new) = state.slot(1, index)?;
    if new {
        let slot = &state.slots[position];
        let payload = json!({"output_index":slot.index,"item":{"type":"reasoning","id":slot.id,"status":"in_progress","summary":[]}});
        state.emit("response.output_item.added", payload, out)?;
    }
    Ok(())
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
        state.replay_layout.push(ReplayLayout::Tool { position });
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
