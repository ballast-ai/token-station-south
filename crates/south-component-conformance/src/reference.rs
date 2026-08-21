//! The native reference implementation of the official OpenAI-compatible
//! provider component.
//!
//! Gate ② is built and frozen against this implementation (S2); the S3
//! runtime then proves a sandboxed build of the same logic gives identical
//! output, and S4 packages it as the first official component. Ported from
//! the community host's official adapter with two structural changes for the
//! v2 ABI: stream state lives in a per-stream parser instance instead of a
//! guest-global, and the SSE tail buffers **bytes** (S0 ruling D2) so a
//! socket split inside a UTF-8 sequence is buffered rather than corrupted.
//!
//! Everything here is protocol translation — routing, budget, billing and
//! the credential itself stay in the host.

use serde_json::{Map, Value, json};
use south_provider_api::{ComponentMetadataV1, PROVIDER_WORLD};
use token_station_protocol::{
    Auth, ChatRequest, ChatResponse, Choice, Content, ContentPart, ErrorCode, ErrorEnvelope,
    Extensions, FinishReason, HttpMethod, HttpRequestDescriptor, HttpResponseParts, Message,
    ProviderApi, ProviderConfig, ResponseFormat, Role, SafeHeaders, StreamEvent, ToolCall, Usage,
};

use crate::component::{ComponentResultV1, ProviderComponentV1, StreamParserV1};

/// The reference component. Stateless; each stream gets its own parser.
#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAiCompatibleReferenceV1;

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

// -- translation -------------------------------------------------------------

fn finish_reason(raw: Option<&str>) -> Option<FinishReason> {
    match raw? {
        "stop" => Some(FinishReason::Stop),
        "length" => Some(FinishReason::Length),
        "tool_calls" => Some(FinishReason::ToolCalls),
        "content_filter" => Some(FinishReason::ContentFilter),
        // Unknown reasons survive verbatim instead of vanishing.
        other => Some(FinishReason::Other(other.to_owned())),
    }
}

fn index_of(value: &Value) -> u32 {
    u32::try_from(value.as_u64().unwrap_or(0)).unwrap_or(0)
}

fn anthropic_reasoning_history(message: &Message) -> Option<String> {
    let blocks = message.extensions.get("anthropic_thinking_blocks")?.as_array()?;
    let thinking = blocks
        .iter()
        .filter_map(|entry| entry.pointer("/block/thinking").and_then(Value::as_str))
        .collect::<String>();
    (!thinking.is_empty()).then_some(thinking)
}

/// Whether this model wants the provider-private `reasoning_content` field
/// replayed on assistant turns.
///
/// R2 (host-parity slice). The field is not universally accepted: legacy
/// reasoner-style upstreams **reject** a request carrying it, which is why the
/// enterprise host gates it per model. Emitting it unconditionally whenever the
/// IR happens to hold a thinking block sends a field that is known to be
/// refused.
///
/// The gate is `ProviderConfig.models[].supported_parameters`, which is inside
/// the S0 §6 policy fence — a component may read it, so this needs no new IR
/// field and no `extensions` smuggling (S0 ruling D5).
///
/// Undeclared models keep the previous DeepSeek-prefix heuristic: a host that
/// ships no catalog (the community host's usual shape) sees no behaviour
/// change, while a host that declares one gets exact control. Declared wins:
/// a model listed with no `reasoning_content` parameter is a deliberate "no".
fn wants_reasoning_content(request: &ChatRequest, config: &ProviderConfig) -> bool {
    config.models.iter().find(|model| model.model == request.model).map_or_else(
        || request.model.get(..8).is_some_and(|prefix| prefix.eq_ignore_ascii_case("deepseek")),
        |model| model.supported_parameters.contains("reasoning_content"),
    )
}

/// Renders one canonical message into the `OpenAI` Chat Completions shape.
///
/// Infallible since the host-parity slice: the only failure this ever had was
/// refusing `RedactedThinking`, and that block is now dropped (R3).
fn message_to_openai_for_model(message: &Message, requires_reasoning_content: bool) -> Value {
    let mut out = Map::new();
    out.insert("role".to_owned(), json!(message.role));
    // R5 (host-parity slice): the key is always written, `null` when the
    // canonical content is absent. Omitting it is a third spelling of "empty"
    // that some compatible providers treat differently from `null`.
    if message.content.is_none() {
        out.insert("content".to_owned(), Value::Null);
    }
    if let Some(content) = &message.content {
        match content {
            Content::Text(text) => {
                out.insert("content".to_owned(), json!(text));
            }
            Content::Parts(parts) => {
                let mut reasoning = String::new();
                let mut multimodal = Vec::new();
                for part in parts {
                    match part {
                        ContentPart::Text { text } => {
                            multimodal.push(json!({"type": "text", "text": text}));
                        }
                        ContentPart::ImageUrl { image_url } => {
                            multimodal.push(json!({
                                "type": "image_url",
                                "image_url": image_url
                            }));
                        }
                        ContentPart::Thinking { thinking, .. } => {
                            reasoning.push_str(thinking);
                        }
                        // R3 (host-parity slice): an encrypted reasoning block
                        // is dropped, not refused. Refusing turned a request
                        // the enterprise host serves today into a 400, and a
                        // migration that only swaps implementations must not
                        // change which requests succeed. The block carries no
                        // OpenAI-compatible spelling, so dropping it is the
                        // same lossy-but-working choice the host already makes.
                        ContentPart::RedactedThinking { .. } => {}
                        ContentPart::Unknown(value) => {
                            multimodal.push(value.clone());
                        }
                    }
                }
                // R1 + R5 (host-parity slice): `Content::Parts` always renders
                // as an array and `Content::Text` always as a string, so the
                // Text/Parts distinction the IR draws survives the boundary.
                //
                // The previous shape collapsed a text-only parts array to a
                // bare string and an all-thinking parts array to `null`. Both
                // are lossy: the IR states the difference and this is the
                // adapter that must not lose it. The enterprise host has
                // rendered it this way in production all along; this adopts
                // its behaviour rather than inventing one.
                //
                // Consequence worth knowing: an assistant turn whose content
                // is nothing but thinking blocks now renders `content: []`.
                // OpenAI documents assistant `content` as string-or-null, so
                // hosts that previously saw `null` here see an empty array
                // instead. See the design record's migration note.
                out.insert("content".to_owned(), Value::Array(multimodal));
                // R2: same gate as the tool-call placeholder below — a model
                // that does not declare the field never receives it, however
                // much thinking the IR carries.
                if requires_reasoning_content && !reasoning.is_empty() {
                    out.insert("reasoning_content".to_owned(), json!(reasoning));
                }
            }
        }
    }
    if !message.tool_calls.is_empty() {
        // OpenAI permits assistant tool-call messages with `content: null`;
        // some compatible providers (DeepSeek) reject the same message when
        // the key is absent entirely. Canonical `None` means no assistant
        // text here, so writing null is lossless.
        if message.role == Role::Assistant && !out.contains_key("content") {
            out.insert("content".to_owned(), Value::Null);
        }
        if message.role == Role::Assistant
            && requires_reasoning_content
            && !out.contains_key("reasoning_content")
        {
            // DeepSeek thinking-mode tool continuations require this private
            // field on the historical assistant tool call. Restore genuine
            // Anthropic thinking when it survived the inbound translation; an
            // empty placeholder is the documented no-thinking history shape.
            out.insert(
                "reasoning_content".to_owned(),
                json!(anthropic_reasoning_history(message).unwrap_or_default()),
            );
        }
        let calls: Vec<Value> = message
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": {"name": call.name, "arguments": call.arguments},
                })
            })
            .collect();
        out.insert("tool_calls".to_owned(), Value::Array(calls));
    }
    if let Some(id) = &message.tool_call_id {
        out.insert("tool_call_id".to_owned(), json!(id));
    }
    if let Some(name) = &message.name {
        out.insert("name".to_owned(), json!(name));
    }
    Value::Object(out)
}

/// Infallible since the host-parity slice: its only failure came from message
/// rendering, which no longer has one (R3).
fn body_of(request: &ChatRequest, config: &ProviderConfig) -> Value {
    let mut body = Map::new();
    body.insert("model".to_owned(), json!(request.model));
    let requires_reasoning_content = wants_reasoning_content(request, config);
    body.insert(
        "messages".to_owned(),
        Value::Array(
            request
                .messages
                .iter()
                .map(|message| message_to_openai_for_model(message, requires_reasoning_content))
                .collect(),
        ),
    );
    if request.stream {
        body.insert("stream".to_owned(), json!(true));
        body.insert("stream_options".to_owned(), json!({"include_usage": true}));
    }
    if let Some(temperature) = request.sampling.temperature {
        body.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(top_p) = request.sampling.top_p {
        body.insert("top_p".to_owned(), json!(top_p));
    }
    if let Some(max_tokens) = request.sampling.max_output_tokens {
        body.insert("max_tokens".to_owned(), json!(max_tokens));
    }
    if !request.sampling.stop.is_empty() {
        body.insert("stop".to_owned(), json!(request.sampling.stop));
    }
    if let Some(tool_choice) = &request.tool_choice {
        body.insert("tool_choice".to_owned(), json!(tool_choice));
    }
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                let mut function = Map::new();
                function.insert("name".to_owned(), json!(tool.name));
                function.insert("description".to_owned(), json!(tool.description));
                function.insert("parameters".to_owned(), tool.parameters.clone());
                if let Some(strict) = request
                    .extensions
                    .get("responses_tool_strict")
                    .and_then(|strict| strict.get(&tool.name))
                    .and_then(Value::as_bool)
                {
                    function.insert("strict".to_owned(), json!(strict));
                }
                json!({"type": "function", "function": function})
            })
            .collect();
        body.insert("tools".to_owned(), Value::Array(tools));
        // `parallel_tool_calls` has no first-class IR field; it arrives via
        // the extensions passthrough and the wire only accepts it alongside
        // `tools`.
        if let Some(parallel) =
            request.extensions.get("parallel_tool_calls").and_then(Value::as_bool)
        {
            body.insert("parallel_tool_calls".to_owned(), json!(parallel));
        }
    }
    if let Some(format) = &request.response_format {
        let format = match format {
            ResponseFormat::Text => json!({"type": "text"}),
            ResponseFormat::JsonObject => json!({"type": "json_object"}),
            ResponseFormat::JsonSchema { json_schema } => {
                json!({"type": "json_schema", "json_schema": json_schema})
            }
        };
        body.insert("response_format".to_owned(), format);
    }
    Value::Object(body)
}

/// Whether `reasoning_effort` may be sent for the chosen model. Native
/// `OpenAI` clients keep the optimistic behavior when a model declares no
/// parameter set; a translated Anthropic adaptive/enabled thinking
/// preference requires an explicit declaration before receiving the field.
fn reasoning_effort_allowed(request: &ChatRequest, config: &ProviderConfig) -> bool {
    let requires_explicit_capability = request
        .extensions
        .get("anthropic_thinking")
        .and_then(|thinking| thinking.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|kind| matches!(kind, "adaptive" | "enabled"));

    config.models.iter().find(|capability| capability.model == request.model).map_or(
        !requires_explicit_capability,
        |capability| {
            capability.supported_parameters.contains("reasoning_effort")
                || (!requires_explicit_capability && capability.supported_parameters.is_empty())
        },
    )
}

/// Normalizes the wire usage object in the `OpenAI` dialect's native cache
/// convention (S0 ruling D1): `cache_read_tokens` is the cached **subset** of
/// `input_tokens`, never a disjoint bucket.
fn usage_of(raw: &Value) -> Usage {
    Usage {
        input_tokens: raw["prompt_tokens"].as_u64().unwrap_or(0),
        output_tokens: raw["completion_tokens"].as_u64().unwrap_or(0),
        cache_read_tokens: raw["prompt_tokens_details"]["cached_tokens"]
            .as_u64()
            .or_else(|| raw["prompt_cache_hit_tokens"].as_u64())
            .unwrap_or(0),
        cache_write_tokens: raw["prompt_tokens_details"]["cache_write_tokens"]
            .as_u64()
            .unwrap_or(0),
        reasoning_tokens: raw["completion_tokens_details"]["reasoning_tokens"]
            .as_u64()
            .unwrap_or(0),
        ..Usage::default()
    }
}

/// Tracks finish arrival separately from its canonical mapping, so a
/// downstream consumer never sees its stream close before the final
/// accounting event.
#[derive(Default)]
struct PendingFinish {
    seen: bool,
    reason: Option<FinishReason>,
    done_emitted: bool,
}

impl PendingFinish {
    fn record(&mut self, raw: &str) {
        self.seen = true;
        self.reason = finish_reason(Some(raw));
        self.done_emitted = false;
    }

    const fn take_finish(&mut self) -> Option<StreamEvent> {
        if !self.seen {
            return None;
        }
        self.seen = false;
        Some(StreamEvent::Finish {
            finish_reason: self.reason.take(),
            // The openai chat wire has no stop-sequence report slot.
            stop_sequence: None,
        })
    }

    fn finish_marker(&mut self) -> Vec<StreamEvent> {
        if self.done_emitted {
            self.done_emitted = false;
            return Vec::new();
        }
        let mut events = self.take_finish().into_iter().collect::<Vec<_>>();
        events.push(StreamEvent::Done { finish_reason: None, stop_sequence: None });
        self.seen = false;
        self.reason = None;
        self.done_emitted = true;
        events
    }

    fn flush_pending_finish(&mut self) -> Vec<StreamEvent> {
        if !self.seen {
            return Vec::new();
        }
        self.finish_marker()
    }
}

/// One complete SSE frame's worth of events. A finish reason is held until
/// cumulative usage arrives (or the provider emits `[DONE]`).
fn events_of_frame(
    payload: &str,
    pending_finish: &mut PendingFinish,
) -> ComponentResultV1<Vec<StreamEvent>> {
    let raw: Value = serde_json::from_str(payload)
        .map_err(|_| provider_protocol_error("the upstream emitted invalid SSE JSON"))?;
    if raw.get("error").is_some_and(|error| !error.is_null()) {
        return Err(provider_protocol_error(
            "the upstream embedded an error in a successful SSE response",
        ));
    }
    let mut events = Vec::new();

    let usage = match raw.get("usage") {
        None | Some(Value::Null) => None,
        Some(usage @ Value::Object(_)) => Some(usage),
        Some(_) => {
            return Err(provider_protocol_error(
                "the upstream SSE event has an invalid usage field",
            ));
        }
    };
    let choices = match raw.get("choices") {
        Some(Value::Array(choices)) => Some(choices),
        None if usage.is_some() => None,
        _ => {
            return Err(provider_protocol_error(
                "the upstream SSE event has an invalid choices field",
            ));
        }
    };
    if choices.is_some_and(Vec::is_empty) && usage.is_none() {
        // S5 (host-parity slice): an event with no choices and no usage is
        // ignored, not refused.
        //
        // Refusing it terminates the whole stream, and this exact shape is not
        // exotic: Azure OpenAI opens a stream with a `prompt_filter_results`
        // frame that carries an empty `choices` array — and `azure-openai-v1`
        // is a dialect this component's own manifest declares. Ordinary
        // keepalive frames look the same. The enterprise host has always
        // ignored them.
        return Ok(events);
    }
    for choice in choices.into_iter().flatten() {
        let index = index_of(&choice["index"]);
        let delta = &choice["delta"];

        // S4 (host-parity slice): the ecosystem spells the reasoning delta two
        // ways — `reasoning_content` (DeepSeek) and a bare `reasoning`
        // (Qwen, OpenWebUI and others). Reading only the first silently drops
        // every thinking token from the second family: no error, no event,
        // the client simply never sees the reasoning. Both are read here.
        //
        // S1 (host-parity slice): a zero-length delta carries nothing. OpenAI's
        // opening frame is `delta:{role:"assistant",content:""}` by convention,
        // and emitting an event for it makes a northbound renderer open an
        // empty content block, shifting every block index after it. Suppressed
        // for text and reasoning alike.
        let thinking = delta["reasoning_content"]
            .as_str()
            .or_else(|| delta["reasoning"].as_str())
            .filter(|delta| !delta.is_empty());
        if let Some(thinking) = thinking {
            events.push(StreamEvent::ThinkingDelta { index, thinking_delta: thinking.to_owned() });
        }
        if let Some(text) = delta["content"].as_str().filter(|text| !text.is_empty()) {
            events.push(StreamEvent::Delta { index, content: text.to_owned() });
        }
        for call in delta["tool_calls"].as_array().into_iter().flatten() {
            events.push(StreamEvent::ToolCallDelta {
                // The tool call's own index, not the choice's: a single
                // choice may stream several calls at once.
                index: index_of(&call["index"]),
                id: call["id"].as_str().map(str::to_owned),
                name: call["function"]["name"].as_str().map(str::to_owned),
                arguments_delta: call["function"]["arguments"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            });
        }
        if let Some(reason) = choice["finish_reason"].as_str() {
            pending_finish.record(reason);
        }
    }

    if let Some(usage) = usage {
        let finish = pending_finish.take_finish();
        let has_finish = finish.is_some();
        if let Some(finish) = finish {
            events.push(finish);
        }
        events.push(StreamEvent::Usage { usage: usage_of(usage) });
        if has_finish {
            events.push(StreamEvent::Done { finish_reason: None, stop_sequence: None });
            pending_finish.done_emitted = true;
        }
    }
    Ok(events)
}

/// Finds the earliest SSE frame separator in a **byte** buffer.
fn sse_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(left), Some(right)) if left <= right => Some((left, 2)),
        (_, Some(right)) => Some((right, 4)),
        (Some(left), None) => Some((left, 2)),
        (None, None) => None,
    }
}

/// One stream's parser: an unparsed byte tail and the held finish state.
#[derive(Default)]
struct OpenAiSseParser {
    tail: Vec<u8>,
    pending_finish: PendingFinish,
}

impl StreamParserV1 for OpenAiSseParser {
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>> {
        if chunk.is_empty() {
            return Ok(self.pending_finish.flush_pending_finish());
        }
        self.tail.extend_from_slice(chunk);

        let mut events = Vec::new();
        while let Some((end, separator)) = sse_frame_boundary(&self.tail) {
            // A complete frame must be UTF-8; a split mid-sequence never gets
            // here because the separator search would not have matched yet.
            let frame = match std::str::from_utf8(&self.tail[..end]) {
                Ok(frame) => frame.replace("\r\n", "\n"),
                Err(_) => {
                    return Err(provider_protocol_error(
                        "the upstream emitted a non-UTF-8 SSE frame",
                    ));
                }
            };
            self.tail.drain(..end + separator);

            let mut event_name = None;
            let mut data_lines = Vec::new();
            for line in frame.lines() {
                if line.starts_with(':') {
                    continue;
                }
                if let Some(value) = line.strip_prefix("event:") {
                    event_name = Some(value.strip_prefix(' ').unwrap_or(value));
                }
                if let Some(value) = line.strip_prefix("data:") {
                    data_lines.push(value.strip_prefix(' ').unwrap_or(value));
                }
            }
            if event_name == Some("error") {
                events.push(StreamEvent::Error {
                    error: provider_protocol_error("the upstream emitted an SSE error event"),
                });
                continue;
            }
            if data_lines.is_empty() {
                continue;
            }
            let payload = data_lines.join("\n");
            if payload == "[DONE]" {
                events.extend(self.pending_finish.finish_marker());
                continue;
            }
            events.extend(events_of_frame(&payload, &mut self.pending_finish)?);
        }
        Ok(events)
    }
}

impl ProviderComponentV1 for OpenAiCompatibleReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "provider-openai-compatible".to_owned(),
            version: "1.0.0".to_owned(),
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
        let mut descriptor = HttpRequestDescriptor::new(
            HttpMethod::Post,
            config.base_url.resolve(ProviderApi::ChatCompletions),
        );
        descriptor.headers =
            SafeHeaders::try_new([("content-type", "application/json")]).map_err(internal)?;
        let mut body = body_of(request, config);
        // `reasoning_effort` arrives through the extensions passthrough;
        // render it when the chosen model allows it.
        if let Some(effort) = request.extensions.get("reasoning_effort").and_then(Value::as_str)
            && reasoning_effort_allowed(request, config)
            && let Value::Object(map) = &mut body
        {
            map.insert("reasoning_effort".to_owned(), json!(effort));
        }
        descriptor.body = Some(body);
        // The host holds the value; this names the slot and the fixed
        // presentation selected by the trusted provider dialect.
        descriptor.auth = match (config.provider.as_str(), config.auth.clone()) {
            ("openai-compatible", secret) => secret.map(Auth::bearer),
            ("azure-openai-v1", Some(secret)) => {
                Some(Auth::header("api-key", secret).map_err(internal)?)
            }
            ("azure-openai-v1", None) => None,
            (dialect, _) => {
                return Err(capability(format!("unsupported provider dialect `{dialect}`")));
            }
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
        let raw_choices = raw["choices"].as_array().ok_or_else(|| {
            provider_protocol_error("the upstream 2xx response has no choices array")
        })?;
        if raw_choices.is_empty() {
            return Err(provider_protocol_error("the upstream 2xx response contains no choices"));
        }

        let mut choices = Vec::new();
        for choice in raw_choices {
            let message = &choice["message"];
            let tool_calls = message["tool_calls"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|call| ToolCall {
                    id: call["id"].as_str().unwrap_or_default().to_owned(),
                    name: call["function"]["name"].as_str().unwrap_or_default().to_owned(),
                    arguments: call["function"]["arguments"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                })
                .collect();

            choices.push(Choice {
                index: index_of(&choice["index"]),
                // The openai chat wire has no stop-sequence report slot.
                stop_sequence: None,
                message: Message {
                    role: Role::Assistant,
                    // A reasoning report lifts content into parts form with
                    // the thinking block first; plain responses keep the
                    // bare-string shape.
                    // P1 (host-parity slice): the non-streaming twin of S4 —
                    // both reasoning spellings are read. Fixing only the
                    // streaming side would leave "thinking appears when
                    // streaming, vanishes when buffered", which is harder to
                    // diagnose than either failure alone.
                    content: match (
                        message["reasoning_content"]
                            .as_str()
                            .or_else(|| message["reasoning"].as_str())
                            .filter(|s| !s.is_empty()),
                        message["content"].as_str(),
                    ) {
                        (Some(thinking), text) => Some(Content::Parts({
                            let mut parts = vec![ContentPart::Thinking {
                                thinking: thinking.to_owned(),
                                signature: None,
                            }];
                            if let Some(text) = text.filter(|t| !t.is_empty()) {
                                parts.push(ContentPart::Text { text: text.to_owned() });
                            }
                            parts
                        })),
                        (None, text) => text.map(|t| Content::Text(t.to_owned())),
                    },
                    tool_calls,
                    tool_call_id: None,
                    name: None,
                    extensions: Extensions::new(),
                },
                finish_reason: finish_reason(choice["finish_reason"].as_str()),
            });
        }

        let usage = usage_of(&raw["usage"]);
        Ok(ChatResponse {
            id: raw["id"].as_str().unwrap_or_default().to_owned(),
            model: raw["model"].as_str().unwrap_or_default().to_owned(),
            choices,
            usage,
            extensions: Extensions::new(),
        })
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        let raw: Value = serde_json::from_str(&parts.body).unwrap_or(Value::Null);

        let provider_code = raw["error"]["code"].as_str().unwrap_or_default().to_ascii_lowercase();
        let code = if provider_code == "content_policy_violation" {
            ErrorCode::ContentPolicy
        } else if provider_code.contains("context_length")
            || provider_code.contains("maximum_context")
        {
            ErrorCode::ContextLength
        } else {
            match parts.status {
                400 | 404 | 422 => ErrorCode::InvalidRequest,
                401 | 403 => ErrorCode::Auth,
                402 => ErrorCode::PaymentRequired,
                408 => ErrorCode::Timeout,
                429 => ErrorCode::RateLimit,
                529 => ErrorCode::Capacity,
                500 | 502 | 503 | 504 => ErrorCode::UpstreamUnavailable,
                _ => ErrorCode::Internal,
            }
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
        Box::new(OpenAiSseParser::default())
    }
}
