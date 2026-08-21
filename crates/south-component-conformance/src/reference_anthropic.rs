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
use token_station_protocol::{
    Auth, ChatRequest, ChatResponse, Choice, Content, ContentPart, ErrorCode, ErrorEnvelope,
    Extensions, FinishReason, HttpMethod, HttpRequestDescriptor, HttpResponseParts, Message,
    ProviderApi, ProviderConfig, Role, SafeHeaders, StreamEvent, ToolCall, ToolChoice, Usage,
};

use crate::component::{ComponentResultV1, ProviderComponentV1, StreamParserV1};

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
        Some(Content::Parts(parts)) => {
            Some(Value::Array(parts.iter().map(part_to_block).collect()))
        }
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
                let mut blocks: Vec<Value> = match message.content.as_ref() {
                    // A bare string becomes the one text block it stands for;
                    // an empty one contributes no block, because Messages
                    // rejects an empty text block.
                    Some(Content::Text(text)) if !text.is_empty() => {
                        vec![json!({"type": "text", "text": text})]
                    }
                    Some(Content::Parts(parts)) => parts.iter().map(part_to_block).collect(),
                    _ => Vec::new(),
                };
                for call in &message.tool_calls {
                    if call.id.is_empty() {
                        return Err(capability(
                            "an assistant tool call has no id; Messages needs one to pair the \
                             result with the call",
                        ));
                    }
                    if call.name.is_empty() {
                        return Err(capability(
                            "an assistant tool call has no name; Messages needs one to name the \
                             tool",
                        ));
                    }
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": call.id,
                        "name": call.name,
                        // The IR keeps arguments as the exact string the model
                        // produced; Messages wants the parsed object. Text that
                        // does not parse travels as a string rather than being
                        // dropped — the upstream gets to reject it.
                        "input": serde_json::from_str::<Value>(&call.arguments)
                            .unwrap_or_else(|_| json!(call.arguments)),
                    }));
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

fn body_of(request: &ChatRequest) -> ComponentResultV1<Value> {
    let (system, messages) = conversation_of(request)?;
    let mut body = Map::new();
    if !request.model.is_empty() {
        body.insert("model".to_owned(), json!(request.model));
    }
    if !system.is_empty() {
        body.insert("system".to_owned(), json!(system.join("\n")));
    }
    body.insert("messages".to_owned(), Value::Array(messages));
    body.insert(
        "max_tokens".to_owned(),
        json!(request.sampling.max_output_tokens.map_or(DEFAULT_MAX_TOKENS, u64::from)),
    );
    if let Some(temperature) = request.sampling.temperature {
        body.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(top_p) = request.sampling.top_p {
        body.insert("top_p".to_owned(), json!(top_p));
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

fn usage_of(raw: &Value) -> Usage {
    let count = |name: &str| raw[name].as_u64().unwrap_or(0);
    Usage {
        input_tokens: count("input_tokens"),
        output_tokens: count("output_tokens"),
        cache_read_tokens: count("cache_read_input_tokens"),
        cache_write_tokens: count("cache_creation_input_tokens"),
        ..Usage::default()
    }
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

    fn events_of(&mut self, event: &str, data: &Value) -> Vec<StreamEvent> {
        match event {
            // D2: input-side counts are reported where the upstream reported
            // them. Folding is the consumer's job.
            "message_start" => {
                let usage = &data["message"]["usage"];
                if usage.is_object() {
                    return vec![StreamEvent::Usage { usage: usage_of(usage) }];
                }
                Vec::new()
            }
            "content_block_start" => {
                let block = &data["content_block"];
                if block["type"].as_str() != Some("tool_use") {
                    return Vec::new();
                }
                vec![StreamEvent::ToolCallDelta {
                    index: block_index(data),
                    id: block["id"].as_str().map(str::to_owned),
                    name: block["name"].as_str().map(str::to_owned),
                    arguments_delta: String::new(),
                }]
            }
            "content_block_delta" => {
                let delta = &data["delta"];
                match delta["type"].as_str() {
                    Some("text_delta") => text_event(delta["text"].as_str(), |text| {
                        StreamEvent::Delta { index: 0, content: text }
                    }),
                    Some("thinking_delta") => text_event(delta["thinking"].as_str(), |text| {
                        StreamEvent::ThinkingDelta { index: 0, thinking_delta: text }
                    }),
                    // D1: the signature arrives exactly once, in the stream.
                    Some("signature_delta") => {
                        text_event(delta["signature"].as_str(), |signature| {
                            StreamEvent::ThinkingSignatureDelta {
                                index: 0,
                                signature_delta: signature,
                            }
                        })
                    }
                    Some("input_json_delta") => vec![StreamEvent::ToolCallDelta {
                        index: block_index(data),
                        id: None,
                        name: None,
                        arguments_delta: delta["partial_json"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                    }],
                    _ => Vec::new(),
                }
            }
            "message_delta" => {
                let Some(reason) = data["delta"]["stop_reason"].as_str() else {
                    return Vec::new();
                };
                self.saw_finish = true;
                self.done_emitted = false;
                self.pending_finish_reason = Some(stop_reason_to_finish(reason));
                self.pending_stop_sequence =
                    data["delta"]["stop_sequence"].as_str().map(str::to_owned);
                if !data["usage"].is_object() {
                    return Vec::new();
                }
                let mut events: Vec<StreamEvent> = self.take_pending_finish().into_iter().collect();
                events.push(StreamEvent::Usage { usage: usage_of(&data["usage"]) });
                events.push(StreamEvent::Done { finish_reason: None, stop_sequence: None });
                self.done_emitted = true;
                events
            }
            _ => Vec::new(),
        }
    }
}

fn block_index(data: &Value) -> u32 {
    u32::try_from(data["index"].as_u64().unwrap_or(0)).unwrap_or(0)
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
            events.extend(self.events_of(event, &parsed));
        }
        Ok(events)
    }
}

impl ProviderComponentV1 for AnthropicReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "provider-anthropic".to_owned(),
            version: "1.0.1".to_owned(),
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
        if config.provider != "anthropic" {
            return Err(capability(format!("unsupported provider dialect `{}`", config.provider)));
        }
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
        descriptor.body = Some(body_of(request)?);
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

        let mut text: Vec<&str> = Vec::new();
        let mut thinking: Vec<ContentPart> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        for block in blocks {
            match block["type"].as_str() {
                Some("text") => text.push(block["text"].as_str().unwrap_or_default()),
                Some("thinking") => thinking.push(ContentPart::Thinking {
                    thinking: block["thinking"].as_str().unwrap_or_default().to_owned(),
                    // D1: the replay ticket travels untouched.
                    signature: block["signature"].as_str().map(str::to_owned),
                }),
                Some("redacted_thinking") => thinking.push(ContentPart::RedactedThinking {
                    data: block["data"].as_str().unwrap_or_default().to_owned(),
                }),
                Some("tool_use") => tool_calls.push(ToolCall {
                    id: block["id"].as_str().unwrap_or_default().to_owned(),
                    name: block["name"].as_str().unwrap_or_default().to_owned(),
                    arguments: block
                        .get("input")
                        .map_or_else(|| "{}".to_owned(), std::string::ToString::to_string),
                }),
                _ => {}
            }
        }

        // Whether the model produced text is "was there a text block", not
        // "is the joined text non-empty": a model that answered with an empty
        // string said something, and a tool-only turn did not.
        let had_text = !text.is_empty();
        let joined = text.concat();
        let content = if thinking.is_empty() {
            had_text.then_some(Content::Text(joined))
        } else {
            let mut parts = thinking;
            if had_text {
                parts.push(ContentPart::Text { text: joined });
            }
            Some(Content::Parts(parts))
        };

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
                    extensions: Extensions::new(),
                },
                finish_reason: raw["stop_reason"].as_str().map(stop_reason_to_finish),
            }],
            usage: usage_of(&raw["usage"]),
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
