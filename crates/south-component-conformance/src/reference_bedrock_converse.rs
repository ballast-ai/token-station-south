//! The native reference implementation of the AWS Bedrock Converse provider
//! component.
//!
//! # Why this dialect is the odd one out
//!
//! The other three provider components put a credential on the request
//! themselves: the descriptor names a header slot and the host fills it. Bedrock
//! `SigV4` cannot work that way — the signature covers the *finished* request
//! (method, host, path, canonical query, the selected headers, the SHA-256 of
//! the body), so nothing can be signed before everything else is settled, and
//! nothing may change after. South expresses that as the `host_signed` arm: a
//! fixed authentication *step* between `assemble` and `transport.send`.
//!
//! Two consequences this file must honour:
//!
//! 1. **The descriptor carries no auth.** `auth_arms` is `["host_signed"]`
//!    alone (the manifest schema refuses any second arm alongside it), and
//!    `descriptor.auth` stays `None`. The host's finalizer signs afterwards.
//! 2. **The request this function returns is the thing that gets signed.**
//!    There is no later opportunity to add a header or touch the URL.
//!
//! # The Bearer sibling
//!
//! Bedrock also accepts an API key presented as `Authorization: Bearer`. That is
//! a different auth arm, and `host_signed` admits no second arm, so it is a
//! separate package, `provider-bedrock-converse-bearer`, whose family is
//! `bedrock-bearer`: [`BedrockConverseBearerReferenceV1`] builds the same
//! request and parses the same responses and streams, and differs only in its
//! identity, its family and the descriptor's auth (host feedback SF16,
//! host-zero-vendor-boundary §13.6). A host picks the package by the row's
//! family, never by the shape of the stored credential.
//!
//! # Streaming: this package declares `stream_framing: aws-eventstream`
//!
//! Converse streams **AWS eventstream binary frames**, not SSE — the event name
//! lives in an `:event-type` frame header and each frame carries two CRC32s. The
//! package declares `stream_framing: aws-eventstream`, so the host deframes with
//! south's `AwsEventStreamDeframerV1` and feeds each message's canonical
//! re-encoding (`reencode_eventstream_v1`) as one SSE frame. So this parser
//! sees `event: contentBlockDelta` / `data: {…}` — exactly the shape every other
//! provider component parses — and needs no eventstream decoder, no CRC
//! dependency, and no arrangement private to this dialect. Exception and error
//! messages arrive as `event: exception:<type>` and `event: error:<code>` and
//! end the stream with `StreamEvent::Error`.
//!
//! `parse_chunk` still takes raw bytes and still buffers across split frames:
//! that contract is unchanged, and a caller holding real bytes may use it
//! directly. The two feeds are alternatives, not a conflict.
//!
//! # No clock, no identity
//!
//! Converse echoes neither a response id nor the model id, and no reference
//! implementation in this crate reads a clock. Both fields are therefore left
//! empty for the host to fill, exactly as the Gemini reference does.

use serde_json::{Map, Value, json};
use south_provider_api::{ComponentMetadataV1, PROVIDER_WORLD};
use std::collections::BTreeSet;
use token_station_protocol::{
    Auth, ChatRequest, ChatResponse, Choice, Content, ContentPart, ErrorCode, ErrorEnvelope,
    FinishReason, HttpMethod, HttpRequestDescriptor, HttpResponseParts, Message, ProviderConfig,
    Role, SafeHeaders, StreamEvent, ToolCall, ToolChoice, Usage,
};

use crate::anthropic_dialect::{Dialect, Thinking};
use crate::component::{ComponentResultV1, ProviderComponentV1, StreamParserV1};
use crate::reasoning_replay::{ReplayRef, validated_layout};

/// The provider dialect this component translates.
const DIALECT: &str = "bedrock";

/// The family of the Bearer sibling package (SF16): the same wire under another auth arm.
pub const BEARER_FAMILY: &str = "bedrock-bearer";

/// The media type Converse streams in: binary AWS eventstream, which the host deframes.
const EVENTSTREAM_MEDIA_TYPE: &str = "application/vnd.amazon.eventstream";

/// Image formats Converse accepts. An unknown media type is refused here
/// rather than forwarded — Bedrock would answer 400 anyway, and a local
/// refusal names the part instead of the whole request.
const IMAGE_FORMATS: [(&str, &str); 4] =
    [("image/png", "png"), ("image/jpeg", "jpeg"), ("image/gif", "gif"), ("image/webp", "webp")];

pub struct BedrockConverseReferenceV1;

/// The Bearer sibling (SF16): the `bedrock-bearer` family on the `bearer` arm. Everything but the
/// identity, the family and the descriptor's auth is [`BedrockConverseReferenceV1`]'s.
pub struct BedrockConverseBearerReferenceV1;

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

/// The `{text: …}` blocks of a message's content, in order.
///
/// Converse has no bare-string content: every block is typed, so a
/// [`Content::Text`] becomes a one-element array.
fn text_blocks(content: Option<&Content>, allow_replay: bool) -> Vec<Value> {
    match content {
        Some(Content::Text(text)) => vec![json!({"text": text})],
        Some(Content::Parts(parts)) => parts
            .iter()
            .filter_map(|part| part_to_block(part, allow_replay).ok().flatten())
            .collect(),
        None => Vec::new(),
    }
}

/// One IR content part as a Converse content block.
///
/// `Ok(None)` is a part this dialect has no block for (a thinking block on the
/// way *out*, say): dropped, not an error. `Err` is a part Converse would
/// reject, refused locally so the message names the part.
fn part_to_block(part: &ContentPart, allow_replay: bool) -> ComponentResultV1<Option<Value>> {
    match part {
        ContentPart::Text { text } => Ok(Some(json!({"text": text}))),
        ContentPart::ImageUrl { image_url } => {
            let url = image_url.url.as_str();
            // Converse takes inline bytes only. An `http(s)` image URL has no
            // representation here at all — saying so beats sending a request
            // the upstream will reject for a reason the caller cannot see.
            let Some(rest) = url.strip_prefix("data:") else {
                return Err(capability(
                    "Converse accepts inline image bytes only; fetch the image and inline it as a \
                     data URL",
                ));
            };
            let Some((media_type, encoded)) = rest.split_once(";base64,") else {
                return Err(capability("a Converse image data URL must be base64-encoded"));
            };
            let Some((_, format)) =
                IMAGE_FORMATS.iter().find(|(mime, _)| mime.eq_ignore_ascii_case(media_type))
            else {
                return Err(capability(format!(
                    "Converse supports png, jpeg, gif and webp images; `{media_type}` is not one"
                )));
            };
            // The base64 payload is passed through verbatim: this component
            // never decodes it, so it needs no base64 dependency.
            Ok(Some(json!({
                "image": {"format": format, "source": {"bytes": encoded}}
            })))
        }
        ContentPart::Thinking { thinking, signature } if allow_replay => {
            let Some(signature) = signature else {
                return Err(capability("a replayed Converse thinking block requires a signature"));
            };
            Ok(Some(json!({"reasoningContent":{"reasoningText":{
                "text":thinking,
                "signature":signature,
            }}})))
        }
        ContentPart::RedactedThinking { data } if allow_replay => {
            Ok(Some(json!({"reasoningContent":{"redactedContent":data}})))
        }
        ContentPart::Thinking { .. }
        | ContentPart::RedactedThinking { .. }
        | ContentPart::Unknown(_) => Ok(None),
    }
}

/// The `toolUse` block for one IR tool call.
///
/// IR keeps `arguments` as a string because providers stream it in fragments;
/// Converse wants a parsed object. A fragment that never became valid JSON
/// cannot be sent as an object, and inventing `{}` would silently change what
/// the model asked for — so that is a refusal.
fn tool_use_block(call: &ToolCall) -> ComponentResultV1<Value> {
    if call.id.is_empty() {
        return Err(capability(
            "Converse requires a non-empty toolUseId; an empty one is rejected upstream as a \
             generic validation error",
        ));
    }
    if call.name.is_empty() {
        return Err(capability("Converse requires a non-empty tool name"));
    }
    let input: Value = if call.arguments.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(&call.arguments).map_err(|_| {
            capability("Converse needs tool arguments as a JSON object; these do not parse")
        })?
    };
    Ok(json!({
        "toolUse": {"toolUseId": call.id, "name": call.name, "input": input}
    }))
}

/// Emits the tool results collected so far as **one** user message.
///
/// One message, not one per result: Converse refuses a separate user-role
/// message per tool result.
fn flush_tool_results(pending: &mut Vec<Value>, messages: &mut Vec<Value>) {
    if !pending.is_empty() {
        messages.push(json!({"role": "user", "content": std::mem::take(pending)}));
    }
}

/// Walks the IR conversation into Converse's `(system, messages)` split.
///
/// Two ordering rules of the dialect are enforced here, both of them things
/// Bedrock rejects rather than tolerates:
///
/// * a tool result answers the *previous* assistant turn, so it must be emitted
///   before any later user content;
/// * parallel tool results belong to **one** user message — Converse refuses a
///   separate user-role message per result.
fn conversation_of(request: &ChatRequest) -> ComponentResultV1<(Vec<Value>, Vec<Value>)> {
    let mut system: Vec<Value> = Vec::new();
    let mut messages: Vec<Value> = Vec::new();
    // Tool results seen since the last flush, all destined for one user message.
    let mut pending_results: Vec<Value> = Vec::new();
    // Every `toolUseId` an assistant turn has announced so far. Converse
    // requires a result to answer a call it can actually see; an id that names
    // nothing is refused here because the upstream answers it with a generic
    // validation error that says nothing about which id was wrong.
    let mut announced_calls: Vec<&str> = Vec::new();

    for message in &request.messages {
        match message.role {
            Role::System => {
                // Converse's system slot is text-only; a non-text part has no
                // representation and Bedrock 400s on one.
                for block in text_blocks(message.content.as_ref(), false) {
                    if block.get("text").is_some() {
                        system.push(block);
                    }
                }
            }
            Role::Tool => {
                let Some(tool_call_id) = message.tool_call_id.as_deref() else {
                    return Err(capability(
                        "a tool result must name the call it answers (tool_call_id)",
                    ));
                };
                if tool_call_id.is_empty() {
                    return Err(capability(
                        "Converse requires a non-empty toolUseId on a tool result; an empty one \
                         can never match a prior toolUse",
                    ));
                }
                if !announced_calls.contains(&tool_call_id) {
                    return Err(capability(format!(
                        "a Converse tool result must answer a toolUse announced earlier in the \
                         same conversation; `{tool_call_id}` names none"
                    )));
                }
                let content = text_blocks(message.content.as_ref(), false);
                pending_results.push(json!({
                    "toolResult": {
                        "toolUseId": tool_call_id,
                        // `status` is optional in Converse and deliberately
                        // omitted: the host does not know whether the tool
                        // failed, and guessing "success" would assert it did not.
                        "content": content,
                    }
                }));
            }
            Role::User => {
                flush_tool_results(&mut pending_results, &mut messages);
                messages.push(json!({"role": "user", "content": user_content(message)?}));
            }
            Role::Assistant => {
                flush_tool_results(&mut pending_results, &mut messages);
                let content = assistant_content(message, &mut announced_calls)?;
                // Bedrock 400s on an assistant turn whose content array is
                // empty, so an empty one is dropped rather than sent.
                if !content.is_empty() {
                    messages.push(json!({"role": "assistant", "content": content}));
                }
            }
        }
    }
    // A trailing tool result with no user turn after it: unusual, but legal.
    flush_tool_results(&mut pending_results, &mut messages);
    Ok((system, messages))
}

fn assistant_content<'a>(
    message: &'a Message,
    announced_calls: &mut Vec<&'a str>,
) -> ComponentResultV1<Vec<Value>> {
    let replay_layout = validated_layout(message)
        .map_err(|()| capability("invalid reasoning replay markers or layout"))?;
    let mut content = text_blocks(message.content.as_ref(), false);
    let Some(layout) = replay_layout else {
        for call in &message.tool_calls {
            content.push(tool_use_block(call)?);
            announced_calls.push(&call.id);
        }
        return Ok(content);
    };
    content.clear();
    for entry in layout {
        match entry {
            ReplayRef::Content(part) => {
                let block = part_to_block(part, true)?
                    .ok_or_else(|| capability("unsupported reasoning replay block"))?;
                content.push(block);
            }
            ReplayRef::Tool(call) => {
                content.push(tool_use_block(call)?);
                announced_calls.push(&call.id);
            }
        }
    }
    Ok(content)
}

/// A user turn's content blocks.
///
/// `Content::Text("")` becomes `[{"text": ""}]`. Bedrock's behaviour on an
/// empty text segment is **not established** — the upstream either 400s or
/// treats it as a no-op — so this passes it through rather than deciding on
/// the upstream's behalf. Changing this to a local refusal is a product
/// decision, not a protocol fact.
fn user_content(message: &Message) -> ComponentResultV1<Vec<Value>> {
    let mut content = Vec::new();
    match message.content.as_ref() {
        Some(Content::Text(text)) => content.push(json!({"text": text})),
        Some(Content::Parts(parts)) => {
            for part in parts {
                if let Some(block) = part_to_block(part, false)? {
                    content.push(block);
                }
            }
        }
        None => {}
    }
    Ok(content)
}

/// `inferenceConfig`, or `None` when the caller set nothing that belongs in it.
fn inference_config(
    request: &ChatRequest,
    dialect: Dialect,
    keeps_sampling: bool,
) -> Option<Value> {
    let sampling = &request.sampling;
    let mut config = Map::new();
    if let Some(max) = sampling.max_output_tokens {
        config.insert("maxTokens".to_owned(), json!(max));
    }
    // Dropped rather than approximated when the model or its thinking mode
    // rejects them (see `anthropic_dialect`).
    if keeps_sampling {
        if let Some(temperature) = sampling.temperature {
            config.insert("temperature".to_owned(), json!(temperature));
        }
        if let Some(top_p) = sampling.top_p
            && (sampling.temperature.is_none() || dialect.keeps_top_p_with_temperature())
        {
            config.insert("topP".to_owned(), json!(top_p));
        }
    }
    if !sampling.stop.is_empty() {
        config.insert("stopSequences".to_owned(), json!(sampling.stop));
    }
    (!config.is_empty()).then_some(Value::Object(config))
}

/// `toolConfig`, or `None` when tools must not be offered at all.
///
/// The `None` case carries the dialect's sharpest trap: Converse's
/// `toolChoice` has **no** `none` form, and an absent `toolChoice` means
/// `auto`. Sending `tools` while dropping only `toolChoice` would silently
/// upgrade "do not use tools" into "use them if you like" — so the whole
/// object is withheld instead.
fn tool_config(request: &ChatRequest) -> Option<Value> {
    if matches!(request.tool_choice, Some(ToolChoice::None)) || request.tools.is_empty() {
        return None;
    }
    let tools: Vec<Value> = request
        .tools
        .iter()
        .map(|tool| {
            let mut spec = Map::new();
            spec.insert("name".to_owned(), json!(tool.name));
            if let Some(description) = &tool.description {
                spec.insert("description".to_owned(), json!(description));
            }
            let schema = if tool.parameters.is_null() {
                json!({"type": "object", "properties": {}})
            } else {
                tool.parameters.clone()
            };
            spec.insert("inputSchema".to_owned(), json!({"json": schema}));
            json!({"toolSpec": Value::Object(spec)})
        })
        .collect();

    let mut config = Map::new();
    config.insert("tools".to_owned(), Value::Array(tools));
    match &request.tool_choice {
        Some(ToolChoice::Required) => {
            config.insert("toolChoice".to_owned(), json!({"any": {}}));
        }
        Some(ToolChoice::Auto) => {
            config.insert("toolChoice".to_owned(), json!({"auto": {}}));
        }
        // The object form the OpenAI wire uses for "call this one tool":
        // `{"type":"function","function":{"name":…}}`. Converse spells the same
        // thing `{"tool":{"name":…}}`, so it is **translated**, not passed
        // through — forwarding the caller's shape verbatim would send Converse a
        // `toolChoice` it does not define, and the upstream answers that with a
        // generic validation error. A shape with no `function.name` to read is
        // left out entirely rather than guessed at: an absent `toolChoice` means
        // `auto`, which is the same thing an unreadable choice would have to
        // fall back to anyway.
        Some(ToolChoice::Other(value)) => {
            if let Some(name) = value.get("function").and_then(|function| function.get("name")) {
                config.insert("toolChoice".to_owned(), json!({"tool": {"name": name}}));
            }
        }
        // Absent means `auto` in Converse, which is also what absent means in
        // IR — so nothing is written.
        Some(ToolChoice::None) | None => {}
    }
    Some(Value::Object(config))
}

fn body_of(request: &ChatRequest, dialect: Dialect) -> ComponentResultV1<Value> {
    let (system, messages) = conversation_of(request)?;
    // Converse has no default `maxTokens` of its own to measure a budget
    // against, so the budget form needs the caller's limit.
    let thinking = dialect.thinking(request, request.sampling.max_output_tokens.map(u64::from))?;
    let mut body = Map::new();
    if !system.is_empty() {
        body.insert("system".to_owned(), Value::Array(system));
    }
    // Written unconditionally, even when empty: the key is required.
    body.insert("messages".to_owned(), Value::Array(messages));
    if let Some(config) = inference_config(request, dialect, dialect.keeps_sampling(thinking)) {
        body.insert("inferenceConfig".to_owned(), config);
    }
    if let Some(config) = tool_config(request) {
        body.insert("toolConfig".to_owned(), config);
    }
    // Claude's own thinking fields ride in `additionalModelRequestFields`, the
    // Converse passthrough for model-specific request fields.
    if let Some(thinking) = thinking {
        let fields: Map<String, Value> = Thinking::fields(thinking)
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect();
        body.insert("additionalModelRequestFields".to_owned(), Value::Object(fields));
    }
    Ok(Value::Object(body))
}

/// Converse's `stopReason` as an IR finish reason.
///
/// The unknown arm keeps the raw string rather than collapsing it into `Stop`:
/// Bedrock has model-specific reasons (`malformed_tool_use`,
/// `model_context_window_exceeded`) that a caller can act on only if they
/// survive translation.
fn stop_reason_to_finish(raw: &str) -> FinishReason {
    match raw {
        "end_turn" => FinishReason::Stop,
        "tool_use" => FinishReason::ToolCalls,
        "max_tokens" => FinishReason::Length,
        "stop_sequence" => FinishReason::StopSequence,
        "content_filtered" | "guardrail_intervened" => FinishReason::ContentFilter,
        other => FinishReason::Other(other.to_owned()),
    }
}

/// Converse's `usage` object as IR usage.
///
/// The three token counts are required and `totalTokens` must equal the sum of
/// input, output and both (optional) cache buckets: a report that fails either
/// test is a protocol error, not a zero. Billing
/// reads these, so a silently-defaulted bucket is a wrong charge.
fn usage_of(raw: &Value) -> ComponentResultV1<Usage> {
    let field = |key: &str| -> Option<u64> { raw.get(key).and_then(Value::as_u64) };
    let (Some(input_tokens), Some(output_tokens), Some(total)) =
        (field("inputTokens"), field("outputTokens"), field("totalTokens"))
    else {
        return Err(provider_protocol_error(
            "a Converse usage report must carry inputTokens, outputTokens and totalTokens",
        ));
    };
    let cache_read_tokens = field("cacheReadInputTokens").unwrap_or_default();
    let cache_write_tokens = field("cacheWriteInputTokens").unwrap_or_default();
    // Converse's `inputTokens` counts only the prompt tokens that neither hit
    // nor wrote the cache; AWS defines the whole prompt as inputTokens +
    // cacheReadInputTokens + cacheWriteInputTokens, and `totalTokens` counts
    // it (a real cached capture: 10 + 4 + 5848 == 5862). The IR's
    // `input_tokens` is that whole prompt, partitioned by the cache buckets.
    let prompt = input_tokens
        .checked_add(cache_read_tokens)
        .and_then(|sum| sum.checked_add(cache_write_tokens))
        .filter(|prompt| prompt.checked_add(output_tokens) == Some(total));
    let Some(prompt) = prompt else {
        return Err(provider_protocol_error(
            "a Converse usage report's totalTokens must equal inputTokens, outputTokens and both cache buckets summed",
        ));
    };
    // Flat buckets: Converse has no TTL tier split, so the tier fields stay
    // zero rather than repeating the total.
    Ok(Usage {
        input_tokens: prompt,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        ..Usage::default()
    })
}

/// Where one SSE frame's payload ends and where the frame itself ends.
fn sse_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let newline = buffer.windows(2).position(|pair| pair == b"\n\n").map(|at| (at, at + 2));
    let crlf = buffer.windows(4).position(|quad| quad == b"\r\n\r\n").map(|at| (at, at + 4));
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

/// The content-block index an event names, which doubles as the tool-call index.
///
/// Converse counts *all* blocks, so a tool call's number can skip values when a
/// text block preceded it. That is fine: consumers only need fragments of one
/// call to share a number and different calls to differ, which holds.
fn block_index(data: &Value) -> ComponentResultV1<u32> {
    data.get("contentBlockIndex")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| provider_protocol_error("the upstream content block index is invalid"))
}

/// Converse's streaming half.
///
/// # Why this parses SSE and not eventstream frames
///
/// Converse streams AWS eventstream binary frames, but the **host** owns that
/// layer: after decoding, it holds `(event_type, payload_json)` and no original
/// bytes, so it feeds components through a seam that re-encodes each event as
/// one SSE frame (`SouthStreamParser::parse_event` on the host side). That is an
/// established path with production callers, not a private arrangement — so this
/// parser sees exactly what every other provider component sees, and needs no
/// eventstream decoder or CRC dependency of its own.
///
/// # The two-phase ending
///
/// Converse ends in two events, and IR has exactly that shape:
///
/// * `messageStop` carries `stopReason` but the stream is not over — `Finish`;
/// * `metadata` carries `usage` and is the terminal frame — `Usage` then `Done`.
///
/// Emitting `Done` on `messageStop` would announce a successful terminal state
/// before the token counts arrived, which is the failure this ordering exists to
/// prevent. So `Done` waits for `metadata`, and an EOF that never saw `metadata`
/// is a protocol error rather than a quiet success.
struct ConverseSseParser {
    tail: Vec<u8>,
    /// The finish reason `messageStop` announced, held until `metadata` lets
    /// `Done` go out.
    pending_finish: Option<FinishReason>,
    /// Whether `metadata` or a mid-stream failure has already closed the stream.
    closed: bool,
    open_blocks: BTreeSet<u32>,
    seen_blocks: BTreeSet<u32>,
}

impl ConverseSseParser {
    const fn new() -> Self {
        Self {
            tail: Vec::new(),
            pending_finish: None,
            closed: false,
            open_blocks: BTreeSet::new(),
            seen_blocks: BTreeSet::new(),
        }
    }

    fn open_block(&mut self, index: u32) -> ComponentResultV1<()> {
        if self.seen_blocks.insert(index) && self.open_blocks.insert(index) {
            Ok(())
        } else {
            Err(provider_protocol_error("the upstream opened a content block twice"))
        }
    }

    /// Real Converse streams announce `contentBlockStart` only for tool use (its `start`
    /// carries the id and name); a text or reasoning block begins with its first delta. Such
    /// a delta opens the block implicitly. A delta after the block stopped is still refused.
    fn open_block_for_delta(&mut self, index: u32) -> ComponentResultV1<()> {
        if self.open_blocks.contains(&index) {
            return Ok(());
        }
        if self.seen_blocks.insert(index) {
            self.open_blocks.insert(index);
            Ok(())
        } else {
            Err(provider_protocol_error(
                "the upstream sent a delta for a content block that already stopped",
            ))
        }
    }

    /// A block that was never announced may still stop (an empty text block); only a second
    /// stop of the same block is an error.
    fn close_block(&mut self, index: u32) -> ComponentResultV1<()> {
        if self.open_blocks.remove(&index) || self.seen_blocks.insert(index) {
            Ok(())
        } else {
            Err(provider_protocol_error("the upstream stopped a content block twice"))
        }
    }

    /// Ends the stream with the failure the upstream reported mid-stream.
    fn fail(&mut self, code: ErrorCode, provider_message: Option<&str>) -> Vec<StreamEvent> {
        self.closed = true;
        let mut error = ErrorEnvelope::new(code, 502, message_of(code));
        error.provider_message =
            provider_message.filter(|message| message.chars().count() <= 256).map(str::to_owned);
        vec![StreamEvent::Error { error }]
    }

    fn events_of(&mut self, event: &str, data: &Value) -> ComponentResultV1<Vec<StreamEvent>> {
        // Nothing follows the terminal `metadata` or a reported failure: the kernel's
        // `StreamEvent::Error` contract forbids events after it, and `Done` ends the exchange.
        if self.closed {
            return Ok(Vec::new());
        }
        match event {
            // Both are accounted for and carry nothing this side needs:
            // `messageStart` only announces the turn, `contentBlockStop` only
            // closes a block whose deltas already went out. The contract says
            // `messageStart` arrives once; a repeat is simply ignored.
            "messageStart" => Ok(Vec::new()),
            "contentBlockStop" => {
                self.close_block(block_index(data)?)?;
                Ok(Vec::new())
            }
            "contentBlockStart" => {
                let index = block_index(data)?;
                self.open_block(index)?;
                // Only a tool block opens with anything: `start.toolUse` carries
                // the id and name, and IR wants them on the call's **first**
                // fragment and never again. A text block's start says nothing.
                let Some(use_block) = data["start"].get("toolUse") else {
                    return Ok(Vec::new());
                };
                Ok(vec![StreamEvent::ToolCallDelta {
                    index,
                    id: use_block["toolUseId"].as_str().map(str::to_owned),
                    name: use_block["name"].as_str().map(str::to_owned),
                    arguments_delta: String::new(),
                }])
            }
            "contentBlockDelta" => {
                let index = block_index(data)?;
                self.open_block_for_delta(index)?;
                let delta = &data["delta"];
                if let Some(text) = delta["text"].as_str() {
                    // `Delta.index` is the *choice* index, not the block index:
                    // one Converse response is one choice.
                    return Ok(vec![StreamEvent::Delta { index: 0, content: text.to_owned() }]);
                }
                if let Some(fragment) = delta["toolUse"]["input"].as_str() {
                    // A JSON *fragment*, not an object — the completed call's
                    // input is an object, but the stream sends pieces of its
                    // text. IR keeps arguments as a string for exactly this.
                    return Ok(vec![StreamEvent::ToolCallDelta {
                        index,
                        id: None,
                        name: None,
                        arguments_delta: fragment.to_owned(),
                    }]);
                }
                // `reasoningContent` is mapped when it carries plain text and
                // dropped otherwise. The host's own translator drops it
                // outright; carrying the text is strictly more faithful, and the
                // `as_str` guard means an unexpected shape falls back to
                // dropping rather than guessing. The shape itself is attested
                // only by a fixture, never by production code, so it is read
                // defensively on purpose.
                if let Some(text) = delta["reasoningContent"]["text"].as_str() {
                    return Ok(vec![StreamEvent::ThinkingDelta {
                        index: 0,
                        block_index: index,
                        thinking_delta: text.to_owned(),
                    }]);
                }
                if let Some(signature) = delta["reasoningContent"]["signature"].as_str() {
                    return Ok(vec![StreamEvent::ThinkingSignatureDelta {
                        index: 0,
                        block_index: index,
                        signature_delta: signature.to_owned(),
                    }]);
                }
                if let Some(data_value) = delta["reasoningContent"]["redactedContent"].as_str() {
                    return Ok(vec![StreamEvent::RedactedThinking {
                        index: 0,
                        block_index: index,
                        data: data_value.to_owned(),
                    }]);
                }
                Ok(Vec::new())
            }
            "messageStop" => {
                if !self.open_blocks.is_empty() {
                    return Err(provider_protocol_error(
                        "the upstream ended the message before every content block stopped",
                    ));
                }
                let raw = data["stopReason"].as_str().unwrap_or("end_turn");
                let finish = stop_reason_to_finish(raw);
                self.pending_finish = Some(finish.clone());
                Ok(vec![StreamEvent::Finish { finish_reason: Some(finish), stop_sequence: None }])
            }
            "metadata" => {
                if !self.open_blocks.is_empty() {
                    return Err(provider_protocol_error(
                        "the upstream completed before every content block stopped",
                    ));
                }
                let usage = usage_of(&data["usage"])?;
                self.closed = true;
                Ok(vec![
                    StreamEvent::Usage { usage },
                    StreamEvent::Done {
                        finish_reason: self.pending_finish.take(),
                        stop_sequence: None,
                    },
                ])
            }
            // The canonical re-encoding's exception and error frames
            // (`south_contracts::reencode_eventstream_v1`): a mid-stream failure
            // the upstream reported. The stream ends here.
            exception if exception.starts_with("exception:") => {
                let name = &exception["exception:".len()..];
                let code = exception_code(name).unwrap_or(ErrorCode::Internal);
                Ok(self.fail(code, data["message"].as_str()))
            }
            error if error.starts_with("error:") => {
                Ok(self.fail(ErrorCode::UpstreamUnavailable, data["message"].as_str()))
            }
            // An event this dialect gains later. Ignored rather than refused: a
            // new event kind must not break an old component.
            _ => Ok(Vec::new()),
        }
    }
}

impl StreamParserV1 for ConverseSseParser {
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>> {
        if chunk.is_empty() {
            // A clean transport EOF, which the runtime spells as an empty
            // fragment. Reaching it without `metadata` means the stream was cut
            // before the token counts arrived — and the way to say so is to emit
            // **no `Done`**, which is exactly what happens here.
            //
            // Not an error, for two reasons. The absence of a terminal event is
            // already the contract's truncation signal, and the host settles an
            // exchange only after one. And an empty fragment is not reliably a
            // real EOF: the conformance suite's incrementality check re-runs a
            // stream split at every byte boundary, so the first chunk of the
            // split-at-zero run is empty mid-stream. A parser that treated that
            // as a fatal truncation would fail the check without any upstream
            // having misbehaved.
            return Ok(Vec::new());
        }
        self.tail.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some((payload_end, frame_end)) = sse_frame_boundary(&self.tail) {
            let frame = self.tail.drain(..frame_end).collect::<Vec<u8>>();
            let frame = std::str::from_utf8(&frame[..payload_end]).map_err(|_| {
                provider_protocol_error("the upstream sent a stream frame that is not UTF-8")
            })?;
            let (Some(event), Some(data)) = frame_fields(frame) else {
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

impl ProviderComponentV1 for BedrockConverseReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "provider-bedrock-converse".to_owned(),
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
        // Deliberately no auth: this is the `host_signed` arm, so the host's
        // finalizer signs the finished request afterwards. A credential value
        // never reaches this component.
        build_converse_request(request, config, DIALECT, None)
    }

    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        let raw: Value = serde_json::from_str(&parts.body).map_err(|_| {
            provider_protocol_error("the upstream returned invalid JSON in a 2xx response")
        })?;
        let Some(blocks) = raw["output"]["message"]["content"].as_array() else {
            return Err(provider_protocol_error(
                "a Converse 2xx response has no output.message.content array",
            ));
        };

        let mut content_parts = Vec::new();
        let mut layout = Vec::new();
        let mut has_reasoning = false;
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        for block in blocks {
            if let Some(chunk) = block.get("text").and_then(Value::as_str) {
                layout.push(json!({"kind":"content","ordinal":content_parts.len()}));
                content_parts.push(ContentPart::Text { text: chunk.to_owned() });
                continue;
            }
            if let Some(reasoning) = block.get("reasoningContent") {
                if let Some(reasoning_text) = reasoning.get("reasoningText") {
                    has_reasoning = true;
                    layout.push(json!({"kind":"content","ordinal":content_parts.len()}));
                    content_parts.push(ContentPart::Thinking {
                        thinking: reasoning_text["text"].as_str().unwrap_or_default().to_owned(),
                        signature: reasoning_text["signature"].as_str().map(str::to_owned),
                    });
                    continue;
                }
                if let Some(data) = reasoning.get("redactedContent").and_then(Value::as_str) {
                    has_reasoning = true;
                    layout.push(json!({"kind":"content","ordinal":content_parts.len()}));
                    content_parts.push(ContentPart::RedactedThinking { data: data.to_owned() });
                    continue;
                }
            }
            if let Some(use_block) = block.get("toolUse") {
                let arguments = match use_block.get("input") {
                    // Converse reports a completed call's input as an object;
                    // a string is tolerated because the streaming half of the
                    // same dialect sends fragments that way.
                    Some(Value::String(raw)) => raw.clone(),
                    Some(value) => serde_json::to_string(value).map_err(internal)?,
                    None => String::new(),
                };
                let id = use_block["toolUseId"].as_str().unwrap_or_default().to_owned();
                layout.push(json!({"kind":"tool_call","call_id":id}));
                tool_calls.push(ToolCall {
                    id,
                    name: use_block["name"].as_str().unwrap_or_default().to_owned(),
                    arguments,
                });
            }
        }

        let finish_reason = stop_reason_to_finish(raw["stopReason"].as_str().unwrap_or("end_turn"));
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
        let mut message_extensions = token_station_protocol::Extensions::new();
        if has_reasoning {
            message_extensions
                .insert("reasoning_replay_protocol_family".into(), json!("claude-signed-thinking"));
            message_extensions.insert("reasoning_replay_block_layout".into(), json!(layout));
        }

        Ok(ChatResponse {
            // Converse echoes neither a response id nor the model; the host
            // supplies identity, as it does for Gemini.
            id: String::new(),
            model: String::new(),
            choices: vec![Choice {
                index: 0,
                message: Message {
                    role: Role::Assistant,
                    content,
                    tool_calls,
                    tool_call_id: None,
                    name: None,
                    extensions: message_extensions,
                },
                finish_reason: Some(finish_reason),
                stop_sequence: None,
            }],
            usage: usage_of(&raw["usage"])?,
            extensions: token_station_protocol::Extensions::new(),
        })
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        let raw: Value = serde_json::from_str(&parts.body).unwrap_or(Value::Null);
        let code = exception_code(exception_name(parts, &raw)).unwrap_or(match parts.status {
            400 | 404 | 422 => ErrorCode::InvalidRequest,
            401 | 403 => ErrorCode::Auth,
            402 => ErrorCode::PaymentRequired,
            408 => ErrorCode::Timeout,
            429 => ErrorCode::RateLimit,
            500 | 502 | 503 | 504 => ErrorCode::UpstreamUnavailable,
            _ => ErrorCode::Internal,
        });
        let mut envelope = ErrorEnvelope::new(code, parts.status, message_of(code));
        envelope.provider_message = raw["message"]
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
        Box::new(ConverseSseParser::new())
    }
}

impl ProviderComponentV1 for BedrockConverseBearerReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "provider-bedrock-converse-bearer".to_owned(),
            version: "1.0.5".to_owned(),
            api_version: PROVIDER_WORLD.to_owned(),
        }
    }

    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<token_station_protocol::ModelCapability>> {
        BedrockConverseReferenceV1.model_capabilities(config)
    }

    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        // The host holds the key; this names the slot, presented as `Bearer`.
        build_converse_request(
            request,
            config,
            BEARER_FAMILY,
            config.auth.clone().map(Auth::bearer),
        )
    }

    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        BedrockConverseReferenceV1.parse_response(parts)
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        BedrockConverseReferenceV1.map_provider_error(parts)
    }

    fn stream_parser(&self) -> Box<dyn StreamParserV1> {
        BedrockConverseReferenceV1.stream_parser()
    }
}

/// The Converse request both packages build, for `family`, with `auth` as the descriptor's.
fn build_converse_request(
    request: &ChatRequest,
    config: &ProviderConfig,
    family: &str,
    auth: Option<Auth>,
) -> ComponentResultV1<HttpRequestDescriptor> {
    for message in &request.messages {
        validated_layout(message)
            .map_err(|()| capability("invalid reasoning replay markers or layout"))?;
    }
    if config.provider != family {
        return Err(capability(format!("unsupported provider dialect `{}`", config.provider)));
    }
    if request.model.is_empty() {
        return Err(capability(
            "Converse addresses the model in the URL path, so a request without one has no \
             target to send to",
        ));
    }
    if requests_reasoning_replay(request) && !model_allows_reasoning_replay(request, config) {
        return Err(capability(
            "reasoning replay requires the target model capability reasoning_replay.claude.v1",
        ));
    }
    let dialect = Dialect::of(request, config)?;
    dialect.refuse_forced_tool(request)?;
    // `ProviderApi::resolve` covers four canonical shapes and none of them
    // is this one — the model sits inside the path and the operation is the
    // last segment. So the URL is built from the endpoint's own text, which
    // is what `permits` authorizes against.
    // Streaming is a different last path segment, not a body field —
    // the same shape Gemini uses.
    let operation = if request.stream { "converse-stream" } else { "converse" };
    let url = format!(
        "{}/model/{}/{operation}",
        config.base_url.as_str().trim_end_matches('/'),
        crate::url_segment::encode(&request.model)
    );
    let mut descriptor = HttpRequestDescriptor::new(HttpMethod::Post, url);
    // The headers the host's native Converse arm sends (host feedback SF14), so
    // a request moved onto this component reaches the upstream byte for byte as
    // before: `accept` names the stream's binary eventstream or the JSON answer,
    // and `x-amzn-bedrock-accept` is the media type of the model's own output,
    // JSON on both operations.
    let accept = if request.stream { EVENTSTREAM_MEDIA_TYPE } else { "application/json" };
    descriptor.headers = SafeHeaders::try_new([
        ("content-type", "application/json"),
        ("accept", accept),
        ("x-amzn-bedrock-accept", "application/json"),
    ])
    .map_err(internal)?;
    descriptor.body = Some(body_of(request, dialect)?);
    descriptor.auth = auth;
    Ok(descriptor)
}

/// The Bedrock exception a non-2xx answer names, normalized: Bedrock names it in a header on some
/// paths and in the body's `__type` on others, possibly qualified (`…#ThrottlingException`) or
/// suffixed (`ThrottlingException:http://…`). Empty when neither names one.
pub(crate) fn exception_name<'a>(parts: &'a HttpResponseParts, body: &'a Value) -> &'a str {
    let exception = parts
        .headers
        .get("x-amzn-errortype")
        .map(String::as_str)
        .or_else(|| body["__type"].as_str())
        .unwrap_or_default();
    let exception = exception.rsplit('#').next().unwrap_or(exception);
    exception.split(':').next().unwrap_or(exception)
}

/// A Bedrock exception name, as a response header, a body `__type` or an eventstream
/// `:exception-type` names it. Streams spell it in lower camel case (`throttlingException`),
/// responses in upper camel case, so the first letter is compared without case.
pub(crate) fn exception_code(exception: &str) -> Option<ErrorCode> {
    let mut chars = exception.chars();
    let first = chars.next()?.to_ascii_uppercase();
    let name = format!("{first}{}", chars.as_str());
    Some(match name.as_str() {
        "ThrottlingException" => ErrorCode::RateLimit,
        "ModelTimeoutException" => ErrorCode::Timeout,
        "ServiceUnavailableException" | "ModelNotReadyException" => ErrorCode::UpstreamUnavailable,
        "ModelErrorException" | "InternalServerException" => ErrorCode::Capacity,
        "ModelStreamErrorException" => ErrorCode::TransportTruncated,
        "AccessDeniedException" | "UnrecognizedClientException" => ErrorCode::Auth,
        "ValidationException" | "ResourceNotFoundException" => ErrorCode::InvalidRequest,
        _ => return None,
    })
}

const fn message_of(code: ErrorCode) -> &'static str {
    match code {
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
        ErrorCode::Capacity | ErrorCode::Capability | ErrorCode::Internal => "the upstream failed",
    }
}
