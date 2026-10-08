//! The native reference implementation of the official Anthropic Messages
//! provider component.
//!
//! Design record: `docs/design/2026-08-22-anthropic-provider-component.md`.
//!
//! Same shape as [`crate::reference`]: gate ② is frozen against this
//! implementation, a `wasm32-wasip2` build of the same logic is what ships,
//! and the sandbox parity test proves the two agree. Everything here is
//! protocol translation — routing, budget, billing and the credential itself
//! stay in the host.
//!
//! The dialect differs from OpenAI-compatible in four ways that shape the
//! code below: `system` is a sibling of `messages` rather than a message;
//! content is always typed blocks; tool calls and tool results are blocks
//! rather than sibling fields; and reasoning arrives as `thinking` blocks
//! carrying a replay `signature` that a later turn must return untouched.

use serde_json::{Map, Value, json};
use south_provider_api::{ComponentMetadataV1, PROVIDER_WORLD};
use std::collections::BTreeSet;
use token_station_protocol::{
    Auth, ChatRequest, ChatResponse, Choice, Content, ContentPart, ErrorCode, ErrorEnvelope,
    Extensions, FinishReason, HttpMethod, HttpRequestDescriptor, HttpResponseParts, Message,
    ProviderApi, ProviderConfig, Role, SafeHeaders, StreamEvent, ToolCall, ToolChoice, Usage,
};

use crate::anthropic_dialect::Dialect;
use crate::component::{ComponentResultV1, ProviderComponentV1, StreamParserV1};
use crate::reasoning_replay::{ReplayRef, validated_layout};

/// The version of the Messages API this component speaks. A wire-protocol
/// constant of the dialect, not an operator setting (design record D5).
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic requires `max_tokens`; this is what the component sends when the
/// caller did not ask for a limit.
const DEFAULT_MAX_TOKENS: u64 = 4096;

/// The reference component. Stateless; each stream gets its own parser.
#[derive(Debug, Default, Clone, Copy)]
pub struct AnthropicReferenceV1;

// -- error plumbing ----------------------------------------------------------

fn internal(detail: impl std::fmt::Display) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string())
}

fn capability(detail: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Capability, 400, detail)
}

const REPLAY_CAPABILITY: &str = "reasoning_replay.claude.v1";

fn requests_reasoning_replay(request: &ChatRequest) -> bool {
    request.messages.iter().any(|message| {
        message.extensions.get("reasoning_replay_protocol_family").and_then(Value::as_str)
            == Some("claude-signed-thinking")
    })
}

fn model_allows_reasoning_replay(request: &ChatRequest, config: &ProviderConfig) -> bool {
    config.models.iter().any(|model| {
        model.model == request.model && model.supported_parameters.contains(REPLAY_CAPABILITY)
    })
}

fn provider_protocol_error(message: &'static str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message)
}

// -- request -----------------------------------------------------------------

/// Text a `system` message contributes. Messages models `system` as a string,
/// so only text survives (design record D6); empty text contributes nothing
/// and is skipped so the caller never gets a blank line it did not write.
fn system_text_of(content: Option<&Content>) -> Vec<&str> {
    match content {
        Some(Content::Text(text)) if !text.is_empty() => vec![text.as_str()],
        Some(Content::Parts(parts)) => parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text } if !text.is_empty() => Some(text.as_str()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn part_to_block(part: &ContentPart) -> Value {
    match part {
        ContentPart::Text { text } => json!({"type": "text", "text": text}),
        ContentPart::ImageUrl { image_url } => {
            if let Some(rest) = image_url.url.strip_prefix("data:")
                && let Some((media_type, data)) = rest.split_once(";base64,")
            {
                return json!({
                    "type": "image",
                    "source": {"type": "base64", "media_type": media_type, "data": data}
                });
            }
            json!({"type": "image", "source": {"type": "url", "url": image_url.url}})
        }
        // D1: the signature is the upstream's replay ticket and travels
        // untouched. A block whose signature the caller never received simply
        // has none.
        ContentPart::Thinking { thinking, signature } => {
            let mut block = json!({"type": "thinking", "thinking": thinking});
            if let Some(signature) = signature {
                block["signature"] = json!(signature);
            }
            block
        }
        ContentPart::RedactedThinking { data } => {
            json!({"type": "redacted_thinking", "data": data})
        }
        // A part this crate does not model travels verbatim; the upstream
        // decides whether it is acceptable.
        ContentPart::Unknown(value) => value.clone(),
    }
}

fn assistant_part_to_block(part: &ContentPart, replay: bool) -> Option<Value> {
    match part {
        ContentPart::Thinking { thinking, signature } => {
            let mut block = json!({"type": "thinking", "thinking": thinking});
            if replay {
                block["signature"] = json!(signature.as_deref()?);
            }
            Some(block)
        }
        ContentPart::RedactedThinking { data } => {
            replay.then(|| json!({"type": "redacted_thinking", "data": data}))
        }
        _ => Some(part_to_block(part)),
    }
}

/// `None` when the turn carried no content at all, so the caller can omit the
/// field rather than invent one.
///
/// Messages requires `content` on every message, so a turn without it is a
/// request the upstream refuses either way. Sending `""` would make the
/// component the author of content the caller never wrote, and would report
/// the refusal against a body that is not the one the caller composed.
/// Omitting keeps the request the caller's.
fn content_to_blocks(content: Option<&Content>) -> Option<Value> {
    match content {
        // Messages accepts a bare string as well as a block array, and a bare
        // string is what a plain turn should stay.
        Some(Content::Text(text)) => Some(json!(text)),
        Some(Content::Parts(parts)) => Some(Value::Array(
            parts.iter().filter_map(|part| assistant_part_to_block(part, false)).collect(),
        )),
        None => None,
    }
}

/// Text a `tool` message contributes to `tool_result.content`.
fn tool_result_text(content: Option<&Content>) -> String {
    match content {
        Some(Content::Text(text)) => text.clone(),
        Some(Content::Parts(parts)) => parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .concat(),
        None => String::new(),
    }
}

fn tool_use_block(call: &ToolCall) -> ComponentResultV1<Value> {
    if call.id.is_empty() {
        return Err(capability(
            "an assistant tool call has no id; Messages needs one to pair the result with the call",
        ));
    }
    if call.name.is_empty() {
        return Err(capability(
            "an assistant tool call has no name; Messages needs one to name the tool",
        ));
    }
    Ok(json!({
        "type": "tool_use",
        "id": call.id,
        "name": call.name,
        "input": serde_json::from_str::<Value>(&call.arguments)
            .unwrap_or_else(|_| json!(call.arguments)),
    }))
}

/// The `system` string and the `messages` array, which Messages models as
/// siblings even though the IR carries system turns inside `messages`.
fn conversation_of(request: &ChatRequest) -> ComponentResultV1<(Vec<&str>, Vec<Value>)> {
    let mut system: Vec<&str> = Vec::new();
    let mut messages: Vec<Value> = Vec::new();

    for message in &request.messages {
        match message.role {
            Role::System => system.extend(system_text_of(message.content.as_ref())),
            Role::User => {
                let mut turn = Map::new();
                turn.insert("role".to_owned(), json!("user"));
                if let Some(content) = content_to_blocks(message.content.as_ref()) {
                    turn.insert("content".to_owned(), content);
                }
                messages.push(Value::Object(turn));
            }
            Role::Assistant => {
                let replay_layout = validated_layout(message)
                    .map_err(|()| capability("invalid reasoning replay markers or layout"))?;
                let mut blocks: Vec<Value> = match message.content.as_ref() {
                    // A bare string becomes the one text block it stands for;
                    // an empty one contributes no block, because Messages
                    // rejects an empty text block.
                    Some(Content::Text(text)) if !text.is_empty() => {
                        vec![json!({"type": "text", "text": text})]
                    }
                    Some(Content::Parts(parts)) => parts
                        .iter()
                        .filter_map(|part| assistant_part_to_block(part, false))
                        .collect(),
                    _ => Vec::new(),
                };
                if let Some(layout) = replay_layout {
                    blocks.clear();
                    for entry in layout {
                        match entry {
                            ReplayRef::Content(part) => {
                                blocks.push(assistant_part_to_block(part, true).ok_or_else(
                                    || capability("unsupported reasoning replay block"),
                                )?);
                            }
                            ReplayRef::Tool(call) => blocks.push(tool_use_block(call)?),
                        }
                    }
                } else {
                    for call in &message.tool_calls {
                        blocks.push(tool_use_block(call)?);
                    }
                }
                messages.push(json!({"role": "assistant", "content": blocks}));
            }
            Role::Tool => {
                let Some(id) = message.tool_call_id.as_deref().filter(|id| !id.is_empty()) else {
                    return Err(capability(
                        "a tool result has no tool_call_id; Messages needs one to reference the \
                         call it answers",
                    ));
                };
                messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": tool_result_text(message.content.as_ref()),
                    }],
                }));
            }
        }
    }

    Ok((system, messages))
}

/// The tool declarations and the choice, which travel together: Messages has
/// no "no tools this turn" choice, so withholding the choice while keeping
/// the declarations would promote "forbidden" to "the model decides".
fn tools_of(request: &ChatRequest) -> (Option<Value>, Option<Value>) {
    if request.tool_choice == Some(ToolChoice::None) {
        return (None, None);
    }
    let declarations = (!request.tools.is_empty()).then(|| {
        Value::Array(
            request
                .tools
                .iter()
                .map(|tool| {
                    let mut declaration = Map::new();
                    declaration.insert("name".to_owned(), json!(tool.name));
                    if let Some(description) = &tool.description {
                        declaration.insert("description".to_owned(), json!(description));
                    }
                    declaration.insert("input_schema".to_owned(), tool.parameters.clone());
                    Value::Object(declaration)
                })
                .collect(),
        )
    });
    let choice = match request.tool_choice.as_ref() {
        Some(ToolChoice::Auto) => Some(json!({"type": "auto"})),
        Some(ToolChoice::Required) => Some(json!({"type": "any"})),
        Some(ToolChoice::Other(value)) => value
            .get("function")
            .and_then(|function| function.get("name"))
            .map(|name| json!({"type": "tool", "name": name})),
        Some(ToolChoice::None) | None => None,
    };
    (declarations, choice)
}

fn body_of(request: &ChatRequest, dialect: Dialect) -> ComponentResultV1<Value> {
    let (system, messages) = conversation_of(request)?;
    let max_tokens = request.sampling.max_output_tokens.map_or(DEFAULT_MAX_TOKENS, u64::from);
    let thinking = dialect.thinking(request, Some(max_tokens))?;
    let mut body = Map::new();
    if !request.model.is_empty() {
        body.insert("model".to_owned(), json!(request.model));
    }
    if !system.is_empty() {
        body.insert("system".to_owned(), json!(system.join("\n")));
    }
    body.insert("messages".to_owned(), Value::Array(messages));
    body.insert("max_tokens".to_owned(), json!(max_tokens));
    // Dropped rather than approximated when the model or its thinking mode
    // rejects them (see `anthropic_dialect`).
    if dialect.keeps_sampling(thinking) {
        if let Some(temperature) = request.sampling.temperature {
            body.insert("temperature".to_owned(), json!(temperature));
        }
        if let Some(top_p) = request.sampling.top_p
            && (request.sampling.temperature.is_none() || dialect.keeps_top_p_with_temperature())
        {
            body.insert("top_p".to_owned(), json!(top_p));
        }
    }
    if !request.sampling.stop.is_empty() {
        body.insert("stop_sequences".to_owned(), json!(request.sampling.stop));
    }
    if request.stream {
        body.insert("stream".to_owned(), json!(true));
    }

    let (declarations, choice) = tools_of(request);
    if let Some(declarations) = declarations {
        body.insert("tools".to_owned(), declarations);
    }
    if let Some(choice) = choice {
        body.insert("tool_choice".to_owned(), choice);
    }
    for (key, value) in thinking.into_iter().flat_map(crate::anthropic_dialect::Thinking::fields) {
        body.insert(key.to_owned(), value);
    }

    Ok(Value::Object(body))
}

// -- response ----------------------------------------------------------------

fn stop_reason_to_finish(raw: &str) -> FinishReason {
    match raw {
        "end_turn" => FinishReason::Stop,
        "max_tokens" => FinishReason::Length,
        "tool_use" => FinishReason::ToolCalls,
        "stop_sequence" => FinishReason::StopSequence,
        // D4: a reason this crate does not model survives verbatim rather
        // than being reported as a normal finish.
        other => FinishReason::Other(other.to_owned()),
    }
}

/// Messages' own usage buckets, as the wire reports them.
///
/// Messages' `input_tokens` counts only the prompt tokens that neither hit nor
/// wrote the cache; the two cache buckets sit beside it. The IR's
/// `input_tokens` is the whole prompt with the cache buckets partitioning it
/// (kernel `Usage::total`), so the IR count is the sum of the three.
#[derive(Debug, Clone, Copy, Default)]
struct WireUsage {
    uncached_input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    cache_write_5m: u64,
    cache_write_1h: u64,
}

/// Which counts a usage report must carry (B1, host-zero-vendor-boundary §6.2
/// item 1). A complete message reports both sides; a stream reports the input
/// side in `message_start` and the output side in `message_delta`.
#[derive(Debug, Clone, Copy)]
enum UsageReport {
    Message,
    StreamStart,
    StreamDelta,
}

fn wire_count(raw: &Value, key: &str, required: bool) -> ComponentResultV1<u64> {
    match raw.get(key) {
        None | Some(Value::Null) if required => {
            Err(provider_protocol_error("the upstream usage lacks a required token count"))
        }
        None | Some(Value::Null) => Ok(0),
        Some(value) => value.as_u64().ok_or_else(|| {
            provider_protocol_error("the upstream usage has an invalid token count")
        }),
    }
}

impl WireUsage {
    /// Usage is funds evidence (provider-adapter.wit, `parse-response`): a
    /// missing required count or a cache-tier breakdown that does not add up is
    /// a protocol error, never a zero.
    fn of(raw: &Value, report: UsageReport) -> ComponentResultV1<Self> {
        if !raw.is_object() {
            return Err(provider_protocol_error("a Messages response must carry a usage object"));
        }
        let (input_required, output_required) = match report {
            UsageReport::Message => (true, true),
            UsageReport::StreamStart => (true, false),
            UsageReport::StreamDelta => (false, true),
        };
        let mut usage = Self {
            uncached_input: wire_count(raw, "input_tokens", input_required)?,
            output: wire_count(raw, "output_tokens", output_required)?,
            cache_read: wire_count(raw, "cache_read_input_tokens", false)?,
            cache_write: wire_count(raw, "cache_creation_input_tokens", false)?,
            ..Self::default()
        };
        match raw.get("cache_creation") {
            None | Some(Value::Null) => {}
            Some(tiers @ Value::Object(_)) => {
                if raw.get("cache_creation_input_tokens").is_none_or(Value::is_null) {
                    return Err(provider_protocol_error(
                        "the upstream cache_creation breakdown lacks its cache_creation_input_tokens total",
                    ));
                }
                usage.cache_write_5m = wire_count(tiers, "ephemeral_5m_input_tokens", true)?;
                usage.cache_write_1h = wire_count(tiers, "ephemeral_1h_input_tokens", true)?;
                if usage.cache_write_5m.checked_add(usage.cache_write_1h) != Some(usage.cache_write)
                {
                    return Err(provider_protocol_error(
                        "the upstream cache_creation tiers do not add up to cache_creation_input_tokens",
                    ));
                }
            }
            Some(_) => {
                return Err(provider_protocol_error(
                    "the upstream usage has an invalid cache_creation field",
                ));
            }
        }
        usage
            .uncached_input
            .checked_add(usage.cache_read)
            .and_then(|sum| sum.checked_add(usage.cache_write))
            .ok_or_else(|| provider_protocol_error("the upstream usage counts overflow"))?;
        Ok(usage)
    }

    /// Fold a later report in, last-nonzero-wins per wire field.
    ///
    /// The fold happens on the wire buckets, not on the IR sum: a
    /// `message_delta` that repeats `input_tokens` without the cache fields
    /// would otherwise shrink the prompt total it reports.
    const fn absorb(&mut self, later: Self) {
        const fn keep(slot: &mut u64, later: u64) {
            if later != 0 {
                *slot = later;
            }
        }
        keep(&mut self.uncached_input, later.uncached_input);
        keep(&mut self.output, later.output);
        keep(&mut self.cache_read, later.cache_read);
        keep(&mut self.cache_write, later.cache_write);
        keep(&mut self.cache_write_5m, later.cache_write_5m);
        keep(&mut self.cache_write_1h, later.cache_write_1h);
    }

    fn to_ir(self) -> Usage {
        Usage {
            input_tokens: self
                .uncached_input
                .saturating_add(self.cache_read)
                .saturating_add(self.cache_write),
            output_tokens: self.output,
            cache_read_tokens: self.cache_read,
            cache_write_tokens: self.cache_write,
            cache_write_5m_tokens: self.cache_write_5m,
            cache_write_1h_tokens: self.cache_write_1h,
            ..Usage::default()
        }
    }
}

fn usage_of(raw: &Value) -> ComponentResultV1<Usage> {
    WireUsage::of(raw, UsageReport::Message).map(WireUsage::to_ir)
}

// -- stream ------------------------------------------------------------------

/// One Messages stream, mid-parse.
///
/// The terminal bookkeeping is the whole of the state: a `message_delta`
/// records the stop reason, and the terminal triple leaves on the frame that
/// carries usage, or at EOF (design record D3).
#[derive(Debug, Default)]
struct AnthropicSseParser {
    tail: Vec<u8>,
    saw_finish: bool,
    pending_finish_reason: Option<FinishReason>,
    pending_stop_sequence: Option<String>,
    done_emitted: bool,
    open_blocks: BTreeSet<u32>,
    seen_blocks: BTreeSet<u32>,
    /// Every usage report so far, folded on the wire buckets.
    usage: WireUsage,
}

/// The end of the first complete SSE frame in `buffer`, as
/// `(payload_end, frame_end)`.
fn sse_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let newline = buffer.windows(2).position(|pair| pair == b"\n\n").map(|at| (at, at + 2));
    let crlf = buffer.windows(4).position(|quad| quad == b"\r\n\r\n").map(|at| (at, at + 4));
    // Whichever separator closes the earlier frame wins; either alone is the
    // answer when only one is present.
    match (newline, crlf) {
        (Some(newline), Some(crlf)) => Some(if newline.0 <= crlf.0 { newline } else { crlf }),
        (found, None) | (None, found) => found,
    }
}

/// The `event:` and `data:` values of one frame.
fn frame_fields(frame: &str) -> (Option<&str>, Option<&str>) {
    let mut event = None;
    let mut data = None;
    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim());
        } else if let Some(value) = line.strip_prefix("data:") {
            data = Some(value.trim());
        }
    }
    (event, data)
}

impl AnthropicSseParser {
    fn open_block(&mut self, index: u32) -> ComponentResultV1<()> {
        if self.seen_blocks.insert(index) && self.open_blocks.insert(index) {
            Ok(())
        } else {
            Err(provider_protocol_error("the upstream opened a content block twice"))
        }
    }

    fn require_open_block(&self, index: u32) -> ComponentResultV1<()> {
        if self.open_blocks.contains(&index) {
            Ok(())
        } else {
            Err(provider_protocol_error(
                "the upstream sent a delta for a content block that is not open",
            ))
        }
    }

    fn close_block(&mut self, index: u32) -> ComponentResultV1<()> {
        if self.open_blocks.remove(&index) {
            Ok(())
        } else {
            Err(provider_protocol_error("the upstream stopped a content block that is not open"))
        }
    }

    fn require_all_blocks_closed(&self) -> ComponentResultV1<()> {
        if self.open_blocks.is_empty() {
            Ok(())
        } else {
            Err(provider_protocol_error(
                "the upstream ended the message before every content block stopped",
            ))
        }
    }

    const fn take_pending_finish(&mut self) -> Option<StreamEvent> {
        if !self.saw_finish {
            return None;
        }
        self.saw_finish = false;
        Some(StreamEvent::Finish {
            finish_reason: self.pending_finish_reason.take(),
            stop_sequence: self.pending_stop_sequence.take(),
        })
    }

    /// One usage report, carrying the whole usage so far.
    ///
    /// Design record D2 left folding to the consumer, and consumers still fold
    /// (kernel `Usage::absorb`). But the IR's `input_tokens` is a sum of three
    /// wire buckets, and a sum cannot be folded field-wise: a later frame
    /// repeating `input_tokens` without the cache fields would report a smaller
    /// prompt and win. So the wire buckets are folded here and every report
    /// carries the whole-so-far usage, which a last-nonzero-wins consumer
    /// absorbs to the same result.
    fn report_usage(&mut self, raw: &Value, report: UsageReport) -> ComponentResultV1<StreamEvent> {
        self.usage.absorb(WireUsage::of(raw, report)?);
        Ok(StreamEvent::Usage { usage: self.usage.to_ir() })
    }

    fn events_of(&mut self, event: &str, data: &Value) -> ComponentResultV1<Vec<StreamEvent>> {
        match event {
            "message_start" => {
                let usage = &data["message"]["usage"];
                if usage.is_object() {
                    return Ok(vec![self.report_usage(usage, UsageReport::StreamStart)?]);
                }
                Ok(Vec::new())
            }
            "content_block_start" => {
                let index = block_index(data)?;
                self.open_block(index)?;
                let block = &data["content_block"];
                match block["type"].as_str() {
                    Some("redacted_thinking") => {
                        return Ok(block["data"].as_str().map_or_else(Vec::new, |value| {
                            vec![StreamEvent::RedactedThinking {
                                index: 0,
                                block_index: index,
                                data: value.to_owned(),
                            }]
                        }));
                    }
                    Some("thinking") => {
                        return Ok(vec![StreamEvent::ThinkingDelta {
                            index: 0,
                            block_index: index,
                            thinking_delta: String::new(),
                        }]);
                    }
                    Some("tool_use") => {}
                    _ => return Ok(Vec::new()),
                }
                Ok(vec![StreamEvent::ToolCallDelta {
                    index,
                    id: block["id"].as_str().map(str::to_owned),
                    name: block["name"].as_str().map(str::to_owned),
                    arguments_delta: String::new(),
                }])
            }
            "content_block_delta" => {
                let index = block_index(data)?;
                self.require_open_block(index)?;
                let delta = &data["delta"];
                Ok(match delta["type"].as_str() {
                    Some("text_delta") => text_event(delta["text"].as_str(), |text| {
                        StreamEvent::Delta { index: 0, content: text }
                    }),
                    Some("thinking_delta") => {
                        text_event(delta["thinking"].as_str(), |text| StreamEvent::ThinkingDelta {
                            index: 0,
                            block_index: index,
                            thinking_delta: text,
                        })
                    }
                    // D1: the signature arrives exactly once, in the stream.
                    Some("signature_delta") => {
                        text_event(delta["signature"].as_str(), |signature| {
                            StreamEvent::ThinkingSignatureDelta {
                                index: 0,
                                block_index: index,
                                signature_delta: signature,
                            }
                        })
                    }
                    Some("input_json_delta") => vec![StreamEvent::ToolCallDelta {
                        index,
                        id: None,
                        name: None,
                        arguments_delta: delta["partial_json"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                    }],
                    _ => Vec::new(),
                })
            }
            "content_block_stop" => {
                self.close_block(block_index(data)?)?;
                Ok(Vec::new())
            }
            "message_delta" => {
                self.require_all_blocks_closed()?;
                let Some(reason) = data["delta"]["stop_reason"].as_str() else {
                    return Ok(Vec::new());
                };
                self.saw_finish = true;
                self.done_emitted = false;
                self.pending_finish_reason = Some(stop_reason_to_finish(reason));
                self.pending_stop_sequence =
                    data["delta"]["stop_sequence"].as_str().map(str::to_owned);
                if !data["usage"].is_object() {
                    return Ok(Vec::new());
                }
                let mut events: Vec<StreamEvent> = self.take_pending_finish().into_iter().collect();
                events.push(self.report_usage(&data["usage"], UsageReport::StreamDelta)?);
                events.push(StreamEvent::Done { finish_reason: None, stop_sequence: None });
                self.done_emitted = true;
                Ok(events)
            }
            _ => Ok(Vec::new()),
        }
    }
}

fn block_index(data: &Value) -> ComponentResultV1<u32> {
    data.get("index")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| provider_protocol_error("the upstream content block index is invalid"))
}

/// An empty delta carries nothing; emitting an event for it would put an empty
/// fragment on a stream that the upstream never sent.
fn text_event(raw: Option<&str>, build: impl FnOnce(String) -> StreamEvent) -> Vec<StreamEvent> {
    raw.filter(|text| !text.is_empty()).map_or_else(Vec::new, |text| vec![build(text.to_owned())])
}

impl StreamParserV1 for AnthropicSseParser {
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>> {
        // The runtime spells a clean transport EOF as an empty fragment, which
        // a successful socket read can never produce (design record D3).
        if chunk.is_empty() {
            if self.done_emitted {
                self.done_emitted = false;
                return Ok(Vec::new());
            }
            let Some(finish) = self.take_pending_finish() else {
                return Ok(Vec::new());
            };
            self.done_emitted = true;
            return Ok(vec![
                finish,
                StreamEvent::Done { finish_reason: None, stop_sequence: None },
            ]);
        }

        self.tail.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some((payload_end, frame_end)) = sse_frame_boundary(&self.tail) {
            let frame = self.tail.drain(..frame_end).collect::<Vec<u8>>();
            let frame = std::str::from_utf8(&frame[..payload_end]).map_err(|_| {
                provider_protocol_error("the upstream sent a stream frame that is not UTF-8")
            })?;
            let (event, data) = frame_fields(frame);
            let (Some(event), Some(data)) = (event, data) else {
                continue;
            };
            let parsed: Value = serde_json::from_str(data).map_err(|_| {
                provider_protocol_error("the upstream sent a stream frame with invalid JSON")
            })?;
            events.extend(self.events_of(event, &parsed)?);
        }
        Ok(events)
    }
}

impl ProviderComponentV1 for AnthropicReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "provider-anthropic".to_owned(),
            version: "1.0.13".to_owned(),
            api_version: PROVIDER_WORLD.to_owned(),
        }
    }

    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<token_station_protocol::ModelCapability>> {
        // No network, so the upstream's own catalog is unreachable. What its
        // operator declared is all there is.
        Ok(config.models.clone())
    }

    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        for message in &request.messages {
            validated_layout(message)
                .map_err(|()| capability("invalid reasoning replay markers or layout"))?;
        }
        if config.provider != "anthropic" {
            return Err(capability(format!("unsupported provider dialect `{}`", config.provider)));
        }
        if requests_reasoning_replay(request) && !model_allows_reasoning_replay(request, config) {
            return Err(capability(
                "reasoning replay requires the target model capability reasoning_replay.claude.v1",
            ));
        }
        let dialect = Dialect::of(request, config)?;
        dialect.refuse_forced_tool(request)?;
        let mut descriptor = HttpRequestDescriptor::new(
            HttpMethod::Post,
            config.base_url.resolve(ProviderApi::Messages),
        );
        descriptor.headers = SafeHeaders::try_new([
            ("content-type", "application/json"),
            // D5: a wire-protocol constant of the dialect.
            ("anthropic-version", ANTHROPIC_VERSION),
        ])
        .map_err(internal)?;
        descriptor.body = Some(body_of(request, dialect)?);
        // The host holds the value; this names the slot and the presentation
        // the dialect fixes.
        descriptor.auth = match config.auth.clone() {
            Some(secret) => Some(Auth::header("x-api-key", secret).map_err(internal)?),
            None => None,
        };
        Ok(descriptor)
    }

    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        let raw: Value = serde_json::from_str(&parts.body).map_err(|_| {
            provider_protocol_error("the upstream returned invalid JSON in a 2xx response")
        })?;
        if raw.get("error").is_some_and(|error| !error.is_null()) {
            return Err(provider_protocol_error(
                "the upstream embedded an error in a successful response",
            ));
        }
        let Some(blocks) = raw["content"].as_array() else {
            return Err(provider_protocol_error("the upstream 2xx response has no content array"));
        };

        let mut content_parts = Vec::new();
        let mut layout = Vec::new();
        let mut has_reasoning = false;
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        for block in blocks {
            match block["type"].as_str() {
                Some("text") => {
                    layout.push(json!({"kind":"content","ordinal":content_parts.len()}));
                    content_parts.push(ContentPart::Text {
                        text: block["text"].as_str().unwrap_or_default().to_owned(),
                    });
                }
                Some("thinking") => {
                    has_reasoning = true;
                    layout.push(json!({"kind":"content","ordinal":content_parts.len()}));
                    content_parts.push(ContentPart::Thinking {
                        thinking: block["thinking"].as_str().unwrap_or_default().to_owned(),
                        signature: block["signature"].as_str().map(str::to_owned),
                    });
                }
                Some("redacted_thinking") => {
                    has_reasoning = true;
                    layout.push(json!({"kind":"content","ordinal":content_parts.len()}));
                    content_parts.push(ContentPart::RedactedThinking {
                        data: block["data"].as_str().unwrap_or_default().to_owned(),
                    });
                }
                Some("tool_use") => {
                    let id = block["id"].as_str().unwrap_or_default().to_owned();
                    layout.push(json!({"kind":"tool_call","call_id":id}));
                    tool_calls.push(ToolCall {
                        id,
                        name: block["name"].as_str().unwrap_or_default().to_owned(),
                        arguments: block
                            .get("input")
                            .map_or_else(|| "{}".to_owned(), std::string::ToString::to_string),
                    });
                }
                _ => {}
            }
        }

        // Whether the model produced text is "was there a text block", not
        // "is the joined text non-empty": a model that answered with an empty
        // string said something, and a tool-only turn did not.
        let content = if content_parts.is_empty() {
            None
        } else if !has_reasoning {
            Some(Content::Text(
                content_parts
                    .into_iter()
                    .filter_map(|part| match part {
                        ContentPart::Text { text } => Some(text),
                        _ => None,
                    })
                    .collect(),
            ))
        } else {
            Some(Content::Parts(content_parts))
        };
        let mut message_extensions = Extensions::new();
        if has_reasoning {
            message_extensions
                .insert("reasoning_replay_protocol_family".into(), json!("claude-signed-thinking"));
            message_extensions.insert("reasoning_replay_block_layout".into(), json!(layout));
        }

        Ok(ChatResponse {
            id: raw["id"].as_str().unwrap_or_default().to_owned(),
            model: raw["model"].as_str().unwrap_or_default().to_owned(),
            choices: vec![Choice {
                index: 0,
                // Messages reports which sequence fired; the IR has a slot for
                // it, so it is not lost.
                stop_sequence: raw["stop_sequence"].as_str().map(str::to_owned),
                message: Message {
                    role: Role::Assistant,
                    content,
                    tool_calls,
                    tool_call_id: None,
                    name: None,
                    extensions: message_extensions,
                },
                finish_reason: raw["stop_reason"].as_str().map(stop_reason_to_finish),
            }],
            usage: usage_of(&raw["usage"])?,
            extensions: Extensions::new(),
        })
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        let raw: Value = serde_json::from_str(&parts.body).unwrap_or(Value::Null);
        let provider_type = raw["error"]["type"].as_str().unwrap_or_default();
        let code = match provider_type {
            "overloaded_error" => ErrorCode::Capacity,
            "rate_limit_error" => ErrorCode::RateLimit,
            "authentication_error" | "permission_error" => ErrorCode::Auth,
            "invalid_request_error" | "not_found_error" => ErrorCode::InvalidRequest,
            _ => match parts.status {
                400 | 404 | 422 => ErrorCode::InvalidRequest,
                401 | 403 => ErrorCode::Auth,
                402 => ErrorCode::PaymentRequired,
                408 => ErrorCode::Timeout,
                429 => ErrorCode::RateLimit,
                529 => ErrorCode::Capacity,
                500 | 502 | 503 | 504 => ErrorCode::UpstreamUnavailable,
                _ => ErrorCode::Internal,
            },
        };
        let message = match code {
            ErrorCode::InvalidRequest => "the upstream refused the request as malformed",
            ErrorCode::Auth => "the upstream rejected the credential",
            ErrorCode::PaymentRequired => {
                "the upstream requires payment or the account is out of funds"
            }
            ErrorCode::RateLimit => "the upstream rate limited this request",
            ErrorCode::ContentPolicy => "the upstream refused on content-policy grounds",
            ErrorCode::ContextLength => "the request exceeds the model's context window",
            ErrorCode::Timeout => "the upstream did not answer in time",
            ErrorCode::UpstreamUnavailable => "the upstream is unavailable",
            ErrorCode::TransportTruncated => "the upstream connection dropped mid-response",
            ErrorCode::ProviderProtocolError => "the upstream answered with an invalid body",
            ErrorCode::Capacity | ErrorCode::Capability | ErrorCode::Internal => {
                "the upstream failed"
            }
        };
        let mut envelope = ErrorEnvelope::new(code, parts.status, message);
        envelope.provider_message = raw["error"]["message"]
            .as_str()
            .filter(|message| message.chars().count() <= 256)
            .map(str::to_owned);
        envelope.retry_after_ms = parts
            .headers
            .get("retry-after")
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| seconds.saturating_mul(1000));
        Ok(envelope)
    }

    fn stream_parser(&self) -> Box<dyn StreamParserV1> {
        Box::new(AnthropicSseParser::default())
    }
}
