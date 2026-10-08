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

fn meta_count(meta: &Value, key: &str) -> ComponentResultV1<u64> {
    match meta.get(key) {
        None | Some(Value::Null) => Ok(0),
        Some(value) => value.as_u64().ok_or_else(|| {
            provider_protocol_error("the upstream usageMetadata has an invalid token count")
        }),
    }
}

fn meta_sum(left: u64, right: u64) -> ComponentResultV1<u64> {
    left.checked_add(right)
        .ok_or_else(|| provider_protocol_error("the upstream usageMetadata counts overflow"))
}

/// Usage is funds evidence (provider-adapter.wit, `parse-response`): a missing
/// count or a total that does not add up is a protocol error, never a zero (B1,
/// host-zero-vendor-boundary §6.2 item 1).
///
/// Gemini reports thoughts **outside** the candidates, and its total is
/// `prompt + toolUsePrompt + candidates + thoughts` (measured 2026-10-01,
/// host-zero-vendor-boundary §16 Q13). So the IR output is
/// `candidates + thoughts`, with `reasoning_tokens = thoughts` as its subset, and
/// the IR input is `prompt + toolUsePrompt`.
///
/// `promptTokenCount` and `totalTokenCount` are required. The JSON mapping of the
/// wire omits zero counts, so a missing `candidatesTokenCount` is zero only when
/// the total already closes without it; otherwise the candidates went
/// unreported, which is an error.
fn usage_of(meta: &Value) -> ComponentResultV1<Usage> {
    if !meta.is_object() {
        return Err(provider_protocol_error("a Gemini response must carry usageMetadata"));
    }
    let required = |key: &str| {
        meta.get(key).and_then(Value::as_u64).ok_or_else(|| {
            provider_protocol_error(
                "the upstream usageMetadata lacks a valid promptTokenCount or totalTokenCount",
            )
        })
    };
    let prompt = required("promptTokenCount")?;
    let total = required("totalTokenCount")?;
    let cached = meta_count(meta, "cachedContentTokenCount")?;
    if cached > prompt {
        return Err(provider_protocol_error(
            "the upstream cachedContentTokenCount exceeds promptTokenCount",
        ));
    }
    let input_tokens = meta_sum(prompt, meta_count(meta, "toolUsePromptTokenCount")?)?;
    let thoughts = meta_count(meta, "thoughtsTokenCount")?;
    let known = meta_sum(input_tokens, thoughts)?;
    let candidates = match meta.get("candidatesTokenCount") {
        Some(_) => meta_count(meta, "candidatesTokenCount")?,
        None if known < total => {
            return Err(provider_protocol_error(
                "the upstream usageMetadata lacks candidatesTokenCount",
            ));
        }
        None => 0,
    };
    let output_tokens = meta_sum(candidates, thoughts)?;
    if meta_sum(input_tokens, output_tokens)? != total {
        return Err(provider_protocol_error(
            "the upstream totalTokenCount does not equal prompt, tool-use prompt, candidates and thoughts",
        ));
    }
    Ok(Usage {
        input_tokens,
        output_tokens,
        cache_read_tokens: cached,
        reasoning_tokens: thoughts,
        ..Usage::default()
    })
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

/// The `usageMetadata` counts a stream is held to across chunks, in the order a
/// decrease is reported (the total last, so the specific bucket is named first).
const STREAM_USAGE_COUNTS: [&str; 6] = [
    "promptTokenCount",
    "candidatesTokenCount",
    "thoughtsTokenCount",
    "toolUsePromptTokenCount",
    "cachedContentTokenCount",
    "totalTokenCount",
];

/// One Gemini stream, mid-parse.
///
/// Gemini streams whole candidates whose `parts` carry the increment, so the
/// per-frame work is the same shape as the non-streaming parse.
///
/// **The terminal chunk is the one carrying `finishReason`, not the one
/// carrying `usageMetadata`** (host feedback SF27, host-zero-vendor-boundary
/// §13.10). Vertex AI puts `usageMetadata` on every chunk, and an intermediate
/// one may hold no count at all (`{"trafficType": "ON_DEMAND"}`) or running
/// counts; only the terminal chunk's is the usage of the exchange. So:
///
/// - the terminal chunk emits `Finish`, then `Usage` parsed by the same strict
///   [`usage_of`] as a non-streaming response when it carries `usageMetadata`,
///   then `Done`. Without `usageMetadata` it emits no `Usage` at all, never a
///   zero, and the host refuses the stream for want of evidence (gate ② row
///   `provider.stream.no-usage`);
/// - an intermediate `usageMetadata` is never emitted as usage. It must be an
///   object, and each count it carries must be a non-negative integer no lower
///   than the same count in an earlier chunk; the terminal chunk is held to
///   the same rule, an omitted count reading zero as in [`usage_of`];
/// - any frame after the terminal chunk is refused.
///
/// Emitting the running counts as usage would also fold to the right number in
/// a consumer that keeps the last non-zero report, but it would make every
/// consumer depend on that fold, and a stream cut before its terminal chunk
/// would leave a partial count that reads like evidence. Terminal-only is the
/// rule of the host's own wire parser (token-station-server 03 #90), so the
/// component and the host agree on which chunk is the evidence.
#[derive(Debug, Default)]
struct GeminiSseParser {
    tail: Vec<u8>,
    tool_calls_seen: usize,
    /// The highest value seen so far of each of [`STREAM_USAGE_COUNTS`].
    counts_seen: [u64; STREAM_USAGE_COUNTS.len()],
    terminated: bool,
}

fn sse_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let newline = buffer.windows(2).position(|pair| pair == b"\n\n").map(|at| (at, at + 2));
    let crlf = buffer.windows(4).position(|quad| quad == b"\r\n\r\n").map(|at| (at, at + 4));
    match (newline, crlf) {
        (Some(newline), Some(crlf)) => Some(if newline.0 <= crlf.0 { newline } else { crlf }),
        (found, None) | (None, found) => found,
    }
}

/// The finish reason of a terminal chunk: any candidate with a non-null
/// `finishReason` makes the chunk terminal, as in the host's wire rule.
fn stream_finish_reason(frame: &Value) -> ComponentResultV1<Option<&str>> {
    let Some(candidates) = frame["candidates"].as_array() else {
        return Ok(None);
    };
    for reason in candidates.iter().map(|candidate| &candidate["finishReason"]) {
        if reason.is_null() {
            continue;
        }
        return reason
            .as_str()
            .map(Some)
            .ok_or_else(|| provider_protocol_error("the upstream finishReason is not a string"));
    }
    Ok(None)
}

impl GeminiSseParser {
    /// Holds one chunk's `usageMetadata` to the cross-chunk rule. An
    /// intermediate chunk is judged only on the counts it carries; the
    /// terminal chunk's omitted counts read zero.
    fn observe_counts(&mut self, meta: &Value, terminal: bool) -> ComponentResultV1<()> {
        if !meta.is_object() {
            return Err(provider_protocol_error("the upstream usageMetadata is not an object"));
        }
        for (key, seen) in STREAM_USAGE_COUNTS.iter().zip(self.counts_seen.iter_mut()) {
            if !terminal && meta.get(*key).is_none_or(Value::is_null) {
                continue;
            }
            let value = meta_count(meta, key)?;
            if value < *seen {
                return Err(provider_protocol_error(
                    "the upstream usageMetadata counts decrease across the stream",
                ));
            }
            *seen = value;
        }
        Ok(())
    }

    fn events_of(&mut self, frame: &Value) -> ComponentResultV1<Vec<StreamEvent>> {
        if self.terminated {
            return Err(provider_protocol_error(
                "the upstream sent a stream frame after the terminal finishReason",
            ));
        }
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
                        block_index: 0,
                        thinking_delta: text.to_owned(),
                    });
                } else {
                    events.push(StreamEvent::Delta { index: 0, content: text.to_owned() });
                }
            }
        }
        let meta = frame.get("usageMetadata");
        let Some(reason) = stream_finish_reason(frame)? else {
            if let Some(meta) = meta {
                self.observe_counts(meta, false)?;
            }
            return Ok(events);
        };
        self.terminated = true;
        events.push(StreamEvent::Finish {
            finish_reason: Some(finish_reason_of(
                reason,
                produced_call || self.tool_calls_seen > 0,
            )),
            // Gemini does not report which stop sequence fired.
            stop_sequence: None,
        });
        if let Some(meta) = meta {
            let usage = usage_of(meta)?;
            self.observe_counts(meta, true)?;
            events.push(StreamEvent::Usage { usage });
        }
        events.push(StreamEvent::Done { finish_reason: None, stop_sequence: None });
        Ok(events)
    }
}

impl StreamParserV1 for GeminiSseParser {
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>> {
        // EOF adds nothing: the terminal chunk already closed the stream, and a
        // stream that never sent one gets no synthetic terminal.
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
            events.extend(self.events_of(&parsed)?);
        }
        Ok(events)
    }
}

impl ProviderComponentV1 for GeminiReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "provider-gemini".to_owned(),
            version: "1.1.11".to_owned(),
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
        if request.messages.iter().any(|message| {
            message.extensions.get("reasoning_replay_protocol_family").and_then(Value::as_str)
                == Some("claude-signed-thinking")
        }) {
            return Err(capability("the Gemini dialect cannot consume Claude reasoning replay"));
        }
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
            crate::url_segment::encode(&request.model)
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
            usage: usage_of(&raw["usageMetadata"])?,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(payload: &Value) -> String {
        format!("data: {payload}\n\n")
    }

    fn text_chunk(text: &str, usage: Option<Value>) -> Value {
        let mut chunk =
            json!({"candidates": [{"content": {"role": "model", "parts": [{"text": text}]}}]});
        if let Some(usage) = usage {
            chunk["usageMetadata"] = usage;
        }
        chunk
    }

    fn terminal_chunk(usage: Option<Value>) -> Value {
        let mut chunk = json!({"candidates": [{"content": {"role": "model", "parts": [{"text": ""}]},
                                                "finishReason": "STOP"}]});
        if let Some(usage) = usage {
            chunk["usageMetadata"] = usage;
        }
        chunk
    }

    /// Feeds every frame through one parser and then EOF; the first error wins.
    fn run(frames: &[Value]) -> ComponentResultV1<Vec<StreamEvent>> {
        let mut parser = GeminiSseParser::default();
        let mut events = Vec::new();
        for payload in frames {
            events.extend(parser.parse_chunk(frame(payload).as_bytes())?);
        }
        events.extend(parser.finish()?);
        Ok(events)
    }

    fn usages(events: &[StreamEvent]) -> Vec<&Usage> {
        events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::Usage { usage } => Some(usage),
                _ => None,
            })
            .collect()
    }

    fn refused(frames: &[Value], what: &str) {
        let error = run(frames).err().unwrap_or_else(|| panic!("{what}: must be refused"));
        assert_eq!(error.code, ErrorCode::ProviderProtocolError, "{what}");
    }

    const fn full_counts() -> (u64, u64) {
        (10, 5)
    }

    fn counts(prompt: u64, candidates: u64) -> Value {
        json!({"promptTokenCount": prompt, "candidatesTokenCount": candidates,
               "totalTokenCount": prompt + candidates})
    }

    #[test]
    fn vertex_count_less_intermediate_usage_metadata_is_ignored() {
        let traffic = json!({"trafficType": "ON_DEMAND"});
        let (prompt, candidates) = full_counts();
        let mut terminal = counts(prompt, candidates);
        terminal["trafficType"] = json!("ON_DEMAND");
        let events = run(&[
            text_chunk("18C ", Some(traffic.clone())),
            text_chunk("and sunny.", Some(traffic)),
            terminal_chunk(Some(terminal)),
        ])
        .expect("a Vertex stream parses");
        let usage = usages(&events);
        assert_eq!(usage.len(), 1, "usage leaves on the terminal chunk only");
        assert_eq!((usage[0].input_tokens, usage[0].output_tokens), (10, 5));
        assert!(matches!(events.last(), Some(StreamEvent::Done { .. })));
    }

    #[test]
    fn running_counts_on_intermediate_chunks_are_not_emitted_as_usage() {
        let events =
            run(&[text_chunk("18C ", Some(counts(10, 2))), terminal_chunk(Some(counts(10, 5)))])
                .expect("running counts that only grow parse");
        let usage = usages(&events);
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].output_tokens, 5);
    }

    #[test]
    fn a_terminal_chunk_without_usage_metadata_reports_no_usage() {
        // No usage event, never a zero: the host refuses the stream for want of
        // evidence (gate ② row `provider.stream.no-usage`).
        let events = run(&[
            text_chunk("hi", Some(json!({"trafficType": "ON_DEMAND"}))),
            terminal_chunk(None),
        ])
        .expect("the stream itself parses");
        assert!(usages(&events).is_empty());
        assert!(matches!(events.last(), Some(StreamEvent::Done { .. })));
    }

    #[test]
    fn the_terminal_usage_metadata_is_parsed_strictly() {
        refused(
            &[terminal_chunk(Some(json!({"trafficType": "ON_DEMAND"})))],
            "a terminal usageMetadata without counts",
        );
        refused(
            &[terminal_chunk(Some(json!({"promptTokenCount": 10, "candidatesTokenCount": 5,
                                         "totalTokenCount": 16})))],
            "a terminal total that does not add up",
        );
    }

    #[test]
    fn intermediate_counts_must_be_non_negative_integers() {
        for bad in [json!(-1), json!(1.5), json!("3")] {
            refused(
                &[
                    text_chunk("hi", Some(json!({"promptTokenCount": bad}))),
                    terminal_chunk(Some(counts(10, 5))),
                ],
                "an intermediate count that is not a non-negative integer",
            );
        }
        refused(
            &[text_chunk("hi", Some(json!("ON_DEMAND"))), terminal_chunk(Some(counts(10, 5)))],
            "an intermediate usageMetadata that is not an object",
        );
    }

    #[test]
    fn counts_never_decrease_across_chunks() {
        refused(
            &[
                text_chunk("a", Some(counts(10, 3))),
                text_chunk("b", Some(counts(10, 2))),
                terminal_chunk(Some(counts(10, 5))),
            ],
            "an intermediate count that decreases",
        );
        refused(
            &[text_chunk("a", Some(counts(10, 6))), terminal_chunk(Some(counts(10, 5)))],
            "a terminal count below an intermediate one",
        );
        // The JSON mapping omits a zero count; a terminal chunk that omits one an
        // earlier chunk reported is a decrease, not an omission.
        refused(
            &[
                text_chunk("a", Some(json!({"thoughtsTokenCount": 4}))),
                terminal_chunk(Some(counts(10, 5))),
            ],
            "a terminal chunk that omits a count an earlier chunk reported",
        );
    }

    #[test]
    fn a_chunk_after_the_terminal_one_is_refused() {
        refused(
            &[terminal_chunk(Some(counts(10, 5))), text_chunk("late", None)],
            "a content chunk after the terminal one",
        );
        refused(
            &[terminal_chunk(None), json!({"usageMetadata": counts(10, 5)})],
            "a usage-only chunk after a terminal chunk that carried none",
        );
    }

    #[test]
    fn any_candidate_with_a_finish_reason_makes_the_chunk_terminal() {
        let chunk = json!({"candidates": [
            {"content": {"role": "model", "parts": [{"text": "hi"}]}},
            {"content": {"role": "model", "parts": []}, "finishReason": "MAX_TOKENS"}
        ], "usageMetadata": counts(10, 5)});
        let events = run(&[chunk]).expect("parses");
        assert_eq!(usages(&events).len(), 1);
        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::Finish { finish_reason: Some(FinishReason::Length), .. }
        )));
    }
}
