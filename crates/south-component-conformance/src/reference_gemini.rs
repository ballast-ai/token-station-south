//! The native reference implementation of the official Gemini provider
//! component.
//!
//! Design record: `docs/design/2026-08-22-gemini-provider-component.md`.
//!
//! Same shape as the other two references: gate ② is frozen against this
//! implementation, a `wasm32-wasip2` build of the same logic is what ships,
//! and the sandbox parity test proves the two agree.
//!
//! The dialect differs from both predecessors in ways that shape the code:
//!
//! - the **model is in the URL path** and the operation is a `:method` suffix
//!   (`…/models/{model}:generateContent`), with streaming selected by a
//!   different suffix plus `?alt=sse` rather than a body field;
//! - `system_instruction` is a sibling of `contents`, and takes parts;
//! - the assistant role is spelled `model`;
//! - tool results are keyed **by function name**, not by a call id, so a turn
//!   that answers a call has to know which call it answers;
//! - reasoning arrives as ordinary text parts carrying `"thought": true`.

use serde_json::{Map, Value, json};
use south_provider_api::{ComponentMetadataV1, PROVIDER_WORLD};
use token_station_protocol::{
    Auth, ChatRequest, ChatResponse, Choice, Content, ContentPart, ErrorCode, ErrorEnvelope,
    Extensions, FinishReason, HttpMethod, HttpRequestDescriptor, HttpResponseParts, Message,
    ProviderConfig, Role, SafeHeaders, StreamEvent, ToolCall, ToolChoice, Usage,
};

use crate::component::{ComponentResultV1, ProviderComponentV1, StreamParserV1};

/// The API version this component speaks. A wire constant of the dialect.
const API_VERSION: &str = "v1beta";

/// The reference component. Stateless; each stream gets its own parser.
#[derive(Debug, Default, Clone, Copy)]
pub struct GeminiReferenceV1;

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

/// `tool_call_id` → function name, from the tool calls the assistant made.
///
/// Gemini keys a `functionResponse` by **function name**; the IR carries only
/// the call id on a tool turn, and the name lives on the assistant turn that
/// made the call. Scanning first (rather than remembering while translating)
/// keeps a caller whose turns arrive out of order working: "the result follows
/// the call" is a protocol convention, not something this function can assume.
fn tool_call_names(request: &ChatRequest) -> std::collections::HashMap<&str, &str> {
    request
        .messages
        .iter()
        .flat_map(|message| message.tool_calls.iter())
        .map(|call| (call.id.as_str(), call.name.as_str()))
        .collect()
}

/// Gemini `parts` have no `type` discriminator at all, so an unmodelled part
/// from any other wire has no spelling here — not even a lossy one. It is
/// refused by name rather than pushed into `parts`, where the upstream would
/// reject the request for an unknown field or silently read an empty part.
/// (Renderer refusal, 0.15.0; see the design record.)
fn unmappable_part(value: &Value) -> ErrorEnvelope {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("<untyped>");
    capability(format!(
        "content block `{kind}` has no Gemini `parts` rendering; route the request to a \
         provider that speaks its wire"
    ))
}

fn part_to_gemini(part: &ContentPart) -> ComponentResultV1<Value> {
    Ok(match part {
        ContentPart::Text { text } => json!({"text": text}),
        ContentPart::ImageUrl { image_url } => {
            if let Some(rest) = image_url.url.strip_prefix("data:")
                && let Some((mime, data)) = rest.split_once(";base64,")
            {
                return Ok(json!({"inline_data": {"mime_type": mime, "data": data}}));
            }
            // A remote image is a URI reference. The mime type is not knowable
            // from the URL, and guessing one is worse than letting the upstream
            // sniff: a `.png` announced as jpeg is a wrong answer, an absent
            // announcement is a question.
            json!({"file_data": {"file_uri": image_url.url}})
        }
        // Gemini marks reasoning with a flag on an ordinary text part.
        ContentPart::Thinking { thinking, .. } => json!({"text": thinking, "thought": true}),
        ContentPart::RedactedThinking { data } => json!({"thoughtSignature": data}),
        ContentPart::Unknown(value) => return Err(unmappable_part(value)),
    })
}

/// The parts a turn contributes. Empty when the turn carried no content — the
/// caller drops such a turn rather than sending an empty `parts`, which the
/// upstream rejects.
fn content_to_parts(content: Option<&Content>) -> ComponentResultV1<Vec<Value>> {
    Ok(match content {
        Some(Content::Text(text)) => vec![json!({"text": text})],
        Some(Content::Parts(parts)) => {
            parts.iter().map(part_to_gemini).collect::<ComponentResultV1<Vec<Value>>>()?
        }
        None => Vec::new(),
    })
}

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

/// The `systemInstruction` texts and the `contents` array. Gemini models the
/// system prompt as a sibling of the conversation, while the IR carries system
/// turns inside `messages`.
fn conversation_of(request: &ChatRequest) -> ComponentResultV1<(Vec<&str>, Vec<Value>)> {
    let names = tool_call_names(request);
    let mut contents: Vec<Value> = Vec::new();
    let mut system: Vec<&str> = Vec::new();

    for message in &request.messages {
        match message.role {
            Role::System => system.extend(system_text_of(message.content.as_ref())),
            Role::User | Role::Assistant => {
                let mut parts = content_to_parts(message.content.as_ref())?;
                for call in &message.tool_calls {
                    if call.name.is_empty() {
                        return Err(capability(
                            "an assistant tool call has no name; Gemini keys a call by its name",
                        ));
                    }
                    parts.push(json!({
                        "functionCall": {
                            "name": call.name,
                            // The IR keeps arguments as the exact string the
                            // model produced; Gemini wants an object. Text that
                            // does not parse becomes an empty object rather than
                            // failing the turn: which function was called is the
                            // load-bearing half, and the upstream can still say
                            // the arguments are wrong.
                            "args": serde_json::from_str::<Value>(&call.arguments)
                                .ok()
                                .filter(Value::is_object)
                                .unwrap_or_else(|| json!({})),
                        }
                    }));
                }
                if parts.is_empty() {
                    continue;
                }
                let role = if message.role == Role::Assistant { "model" } else { "user" };
                contents.push(json!({"role": role, "parts": parts}));
            }
            Role::Tool => {
                let Some(name) =
                    message.tool_call_id.as_deref().and_then(|id| names.get(id).copied())
                else {
                    return Err(capability(
                        "a tool result references a call this exchange never made; Gemini keys a \
                         result by the called function's name, which only that call carries",
                    ));
                };
                let text = match message.content.as_ref() {
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
                };
                contents.push(json!({
                    "role": "user",
                    "parts": [{
                        "functionResponse": {"name": name, "response": {"content": text}}
                    }]
                }));
            }
        }
    }

    Ok((system, contents))
}

fn body_of(request: &ChatRequest) -> ComponentResultV1<Value> {
    let (system, contents) = conversation_of(request)?;
    let mut body = Map::new();
    body.insert("contents".to_owned(), Value::Array(contents));
    if !system.is_empty() {
        body.insert(
            "systemInstruction".to_owned(),
            json!({"parts": system.iter().map(|text| json!({"text": text})).collect::<Vec<_>>()}),
        );
    }

    let mut generation = Map::new();
    if let Some(temperature) = request.sampling.temperature {
        generation.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(top_p) = request.sampling.top_p {
        generation.insert("topP".to_owned(), json!(top_p));
    }
    if let Some(max) = request.sampling.max_output_tokens {
        generation.insert("maxOutputTokens".to_owned(), json!(max));
    }
    if !request.sampling.stop.is_empty() {
        generation.insert("stopSequences".to_owned(), json!(request.sampling.stop));
    }
    if !generation.is_empty() {
        body.insert("generationConfig".to_owned(), Value::Object(generation));
    }

    // Gemini has a NONE mode, but withholding the declarations says the same
    // thing to every model version, so `none` drops both.
    if request.tool_choice != Some(ToolChoice::None) && !request.tools.is_empty() {
        body.insert(
            "tools".to_owned(),
            json!([{
                "functionDeclarations": request
                    .tools
                    .iter()
                    .map(|tool| {
                        let mut declaration = Map::new();
                        declaration.insert("name".to_owned(), json!(tool.name));
                        if let Some(description) = &tool.description {
                            declaration.insert("description".to_owned(), json!(description));
                        }
                        declaration.insert("parameters".to_owned(), tool.parameters.clone());
                        Value::Object(declaration)
                    })
                    .collect::<Vec<_>>()
            }]),
        );
    }
    if let Some(mode) = match request.tool_choice.as_ref() {
        Some(ToolChoice::Auto) => Some(json!({"mode": "AUTO"})),
        Some(ToolChoice::Required) => Some(json!({"mode": "ANY"})),
        Some(ToolChoice::None) => Some(json!({"mode": "NONE"})),
        // An unmodelled string form is not guessed at: turning "the model
        // decides" into "must call" is the expensive direction to be wrong in.
        Some(ToolChoice::Other(value)) => value
            .get("function")
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
            .map(|name| json!({"mode": "ANY", "allowedFunctionNames": [name]})),
        None => None,
    } {
        body.insert("toolConfig".to_owned(), json!({"functionCallingConfig": mode}));
    }

    Ok(Value::Object(body))
}

// -- response ----------------------------------------------------------------

fn finish_reason_of(raw: &str, produced_tool_calls: bool) -> FinishReason {
    // A turn that produced a call finished because of the call, whatever the
    // upstream labelled it: Gemini reports `STOP` for tool turns, and a caller
    // reading the reason alone would never look at the calls.
    if produced_tool_calls {
        return FinishReason::ToolCalls;
    }
    match raw {
        "STOP" => FinishReason::Stop,
        "MAX_TOKENS" => FinishReason::Length,
        "SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST" => FinishReason::ContentFilter,
        // RECITATION / MALFORMED_FUNCTION_CALL / OTHER / … survive verbatim.
        other => FinishReason::Other(other.to_owned()),
    }
}

fn usage_of(meta: &Value) -> Usage {
    let count = |name: &str| meta[name].as_u64().unwrap_or(0);
    Usage {
        input_tokens: count("promptTokenCount"),
        output_tokens: count("candidatesTokenCount"),
        cache_read_tokens: count("cachedContentTokenCount"),
        reasoning_tokens: count("thoughtsTokenCount"),
        ..Usage::default()
    }
}

/// A synthetic call id, stable for a given response.
///
/// Gemini does not send one — it keys a call by the function's name — while the
/// IR requires a non-empty id and the caller has to quote it back. The position
/// plus the name is enough for the next turn's `tool_call_names` to find its
/// way back, and translating the same response twice yields the same id.
fn synthetic_call_id(position: usize, name: &str) -> String {
    format!("call_{position}_{name}")
}

// -- stream ------------------------------------------------------------------

/// One Gemini stream, mid-parse.
///
/// Gemini streams whole candidates whose `parts` carry the increment, so the
/// per-frame work is the same shape as the non-streaming parse. The terminal
/// bookkeeping matches the other components: a frame carrying `finishReason`
/// records it, and the terminal triple leaves on the frame that carries usage,
/// or at EOF.
#[derive(Debug, Default)]
struct GeminiSseParser {
    tail: Vec<u8>,
    tool_calls_seen: usize,
    saw_finish: bool,
    pending_finish: Option<FinishReason>,
    done_emitted: bool,
}

fn sse_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let newline = buffer.windows(2).position(|pair| pair == b"\n\n").map(|at| (at, at + 2));
    let crlf = buffer.windows(4).position(|quad| quad == b"\r\n\r\n").map(|at| (at, at + 4));
    match (newline, crlf) {
        (Some(newline), Some(crlf)) => Some(if newline.0 <= crlf.0 { newline } else { crlf }),
        (found, None) | (None, found) => found,
    }
}

impl GeminiSseParser {
    const fn take_pending_finish(&mut self) -> Option<StreamEvent> {
        if !self.saw_finish {
            return None;
        }
        self.saw_finish = false;
        Some(StreamEvent::Finish {
            finish_reason: self.pending_finish.take(),
            // Gemini does not report which stop sequence fired.
            stop_sequence: None,
        })
    }

    fn events_of(&mut self, frame: &Value) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        let candidate = &frame["candidates"][0];
        let mut produced_call = false;
        if let Some(parts) = candidate["content"]["parts"].as_array() {
            for part in parts {
                if let Some(call) = part.get("functionCall") {
                    let name = call["name"].as_str().unwrap_or_default();
                    events.push(StreamEvent::ToolCallDelta {
                        index: u32::try_from(self.tool_calls_seen).unwrap_or(u32::MAX),
                        id: Some(synthetic_call_id(self.tool_calls_seen, name)),
                        name: Some(name.to_owned()),
                        arguments_delta: call
                            .get("args")
                            .map_or_else(|| "{}".to_owned(), std::string::ToString::to_string),
                    });
                    self.tool_calls_seen += 1;
                    produced_call = true;
                    continue;
                }
                let Some(text) = part["text"].as_str().filter(|text| !text.is_empty()) else {
                    continue;
                };
                if part["thought"].as_bool() == Some(true) {
                    events.push(StreamEvent::ThinkingDelta {
                        index: 0,
                        thinking_delta: text.to_owned(),
                    });
                } else {
                    events.push(StreamEvent::Delta { index: 0, content: text.to_owned() });
                }
            }
        }
        if let Some(reason) = candidate["finishReason"].as_str() {
            self.saw_finish = true;
            self.done_emitted = false;
            self.pending_finish =
                Some(finish_reason_of(reason, produced_call || self.tool_calls_seen > 0));
        }
        if frame["usageMetadata"].is_object() {
            let finish = self.take_pending_finish();
            let terminal = finish.is_some();
            events.extend(finish);
            events.push(StreamEvent::Usage { usage: usage_of(&frame["usageMetadata"]) });
            if terminal {
                events.push(StreamEvent::Done { finish_reason: None, stop_sequence: None });
                self.done_emitted = true;
            }
        }
        events
    }
}

impl StreamParserV1 for GeminiSseParser {
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>> {
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
            let Some(data) =
                frame.lines().find_map(|line| line.strip_prefix("data:")).map(str::trim)
            else {
                continue;
            };
            let parsed: Value = serde_json::from_str(data).map_err(|_| {
                provider_protocol_error("the upstream sent a stream frame with invalid JSON")
            })?;
            events.extend(self.events_of(&parsed));
        }
        Ok(events)
    }
}

impl ProviderComponentV1 for GeminiReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "provider-gemini".to_owned(),
            version: "1.1.0".to_owned(),
            api_version: PROVIDER_WORLD.to_owned(),
        }
    }

    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<token_station_protocol::ModelCapability>> {
        Ok(config.models.clone())
    }

    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        if config.provider != "gemini" {
            return Err(capability(format!("unsupported provider dialect `{}`", config.provider)));
        }
        if request.model.is_empty() {
            return Err(capability(
                "Gemini addresses the model in the URL path, so a request without one has no \
                 target to send to",
            ));
        }
        // The model is in the path and the operation is a `:method` suffix;
        // streaming is a different suffix plus a query, not a body field.
        // `ProviderApi::resolve` covers four canonical shapes and none of them
        // is this one, so the URL is built from the endpoint's own text — which
        // is what `permits` authorizes against.
        let (method, query) = if request.stream {
            ("streamGenerateContent", "?alt=sse")
        } else {
            ("generateContent", "")
        };
        let url = format!(
            "{}/{API_VERSION}/models/{}:{method}{query}",
            config.base_url.as_str().trim_end_matches('/'),
            request.model
        );
        let mut descriptor = HttpRequestDescriptor::new(HttpMethod::Post, url);
        descriptor.headers =
            SafeHeaders::try_new([("content-type", "application/json")]).map_err(internal)?;
        descriptor.body = Some(body_of(request)?);
        descriptor.auth = match config.auth.clone() {
            Some(secret) => Some(Auth::header("x-goog-api-key", secret).map_err(internal)?),
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
        let Some(candidate) = raw["candidates"].as_array().and_then(|list| list.first()) else {
            return Err(provider_protocol_error("the upstream 2xx response has no candidates"));
        };

        let mut text: Vec<&str> = Vec::new();
        let mut thinking: Vec<ContentPart> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        if let Some(parts) = candidate["content"]["parts"].as_array() {
            for part in parts {
                if let Some(call) = part.get("functionCall") {
                    let name = call["name"].as_str().unwrap_or_default();
                    tool_calls.push(ToolCall {
                        id: synthetic_call_id(tool_calls.len(), name),
                        name: name.to_owned(),
                        arguments: call
                            .get("args")
                            .map_or_else(|| "{}".to_owned(), std::string::ToString::to_string),
                    });
                    continue;
                }
                let Some(body) = part["text"].as_str() else {
                    continue;
                };
                if part["thought"].as_bool() == Some(true) {
                    thinking.push(ContentPart::Thinking {
                        thinking: body.to_owned(),
                        signature: part["thoughtSignature"].as_str().map(str::to_owned),
                    });
                } else {
                    text.push(body);
                }
            }
        }

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

        let produced_tool_calls = !tool_calls.is_empty();
        Ok(ChatResponse {
            // Gemini does not echo a request id; the host supplies identity.
            id: String::new(),
            model: raw["modelVersion"].as_str().unwrap_or_default().to_owned(),
            choices: vec![Choice {
                index: 0,
                stop_sequence: None,
                message: Message {
                    role: Role::Assistant,
                    content,
                    tool_calls,
                    tool_call_id: None,
                    name: None,
                    extensions: Extensions::new(),
                },
                finish_reason: candidate["finishReason"]
                    .as_str()
                    .map(|raw| finish_reason_of(raw, produced_tool_calls)),
            }],
            usage: usage_of(&raw["usageMetadata"]),
            extensions: Extensions::new(),
        })
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        let raw: Value = serde_json::from_str(&parts.body).unwrap_or(Value::Null);
        let status = raw["error"]["status"].as_str().unwrap_or_default();
        let code = match status {
            "RESOURCE_EXHAUSTED" => ErrorCode::RateLimit,
            "UNAUTHENTICATED" | "PERMISSION_DENIED" => ErrorCode::Auth,
            "INVALID_ARGUMENT" | "NOT_FOUND" | "FAILED_PRECONDITION" => ErrorCode::InvalidRequest,
            "UNAVAILABLE" => ErrorCode::UpstreamUnavailable,
            "DEADLINE_EXCEEDED" => ErrorCode::Timeout,
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
        Box::new(GeminiSseParser::default())
    }
}
