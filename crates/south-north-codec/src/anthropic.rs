//! The Anthropic Messages family: request in, non-streaming response out.
//!
//! The shape differences from `OpenAI` Chat that actually cost something:
//!
//! - **Tool results are user content here, and their own turn in the IR.** A
//!   `tool_result` block answers the *previous* assistant turn, so it must be
//!   ordered ahead of whatever else the same user message carries; putting it
//!   after would reorder the conversation.
//! - **Tool arguments are an object here and a string there.** The IR keeps the
//!   string, because that is the form the tool on the other end receives.
//! - **The envelope carries exactly one message.** There is no `n`, so a
//!   response with any number of choices other than one cannot be rendered —
//!   see [`anthropic_message_response`].

use serde_json::{Value, json};
use token_station_protocol::{
    ChatRequest, ChatResponse, Content, ContentPart, Extensions, FinishReason, ImageUrl, Message,
    Reasoning, ReasoningMode, Role, Sampling, ToolCall, ToolChoice, ToolDef, Usage,
};

use crate::fidelity::{cache_control, optional_bool, refuse_reserved_keys};
use crate::{CodecError, ResponseContext, describe, reasoning_effort, response::prefixed_id};

/// Anthropic Messages request body -> canonical [`ChatRequest`].
///
/// # Roles
///
/// `user` and `assistant` are this protocol's own. `developer` and `system` are
/// the neighbouring ecosystem's word for the same thing and are accepted as
/// [`Role::System`] — a client that sends one gets the behaviour it expects
/// rather than a refusal it cannot act on. Anything else is refused with its
/// position: passing an undefined role upstream only moves the rejection
/// somewhere the client cannot read.
///
/// # Thinking blocks
///
/// Accepted unconditionally, signature included. Whether they are replayed to a
/// provider is a per-model host decision, and the `signature` is the ticket that
/// makes such a replay verifiable — dropping either here would make that
/// decision for the host, permanently.
///
/// # Canonical IR 4 (KC1)
///
/// Prompt-cache markers on system blocks, content blocks, tools and tool results, `top_k`, the
/// caller's `thinking` and `output_config.effort` (unconverted), tool `strict` and
/// `disable_parallel_tool_use` reach their typed IR fields. A marked system prompt keeps one part
/// per block; an unmarked one stays the joined string it always was. Every other top-level key
/// travels in `extensions` as data, except `metadata`: the caller's end-user id stays with the
/// host, which derives the value a component may send (KC-Q4). A top-level key that is an IR key
/// this wire does not define is refused (KC-Q5), as is an invalid marker anywhere (K6).
pub fn chat_request_from_anthropic_messages(body: &Value) -> Result<ChatRequest, CodecError> {
    refuse_reserved_keys(body, RESERVED_KEYS)?;
    let model = body.get("model").and_then(Value::as_str).unwrap_or_default().to_owned();
    let mut messages: Vec<Message> = Vec::new();

    if let Some(system) = body.get("system")
        && let Some(message) = system_message(system, "system")?
    {
        messages.push(message);
    }

    if let Some(raw_messages) = body.get("messages").and_then(Value::as_array) {
        for (index, raw) in raw_messages.iter().enumerate() {
            let role = raw.get("role").and_then(Value::as_str).unwrap_or("user");
            let content = raw.get("content").cloned().unwrap_or(Value::Null);
            match (role, &content) {
                ("assistant", Value::Array(parts)) => {
                    messages.push(assistant_from_blocks(parts, index)?);
                }
                ("user", Value::Array(parts)) => {
                    extend_with_user_blocks(&mut messages, parts, index)?;
                }
                ("user", Value::String(text)) => {
                    messages.push(Message::text(Role::User, text.clone()));
                }
                ("assistant", Value::String(text)) => {
                    messages.push(Message::text(Role::Assistant, text.clone()));
                }
                // A known role carrying a content shape this protocol does not
                // define (null, an object): there is no turn to build, and
                // inventing an empty one would add a message the client never
                // sent.
                ("user" | "assistant", _) => {}
                ("developer" | "system", _) => {
                    if let Some(message) =
                        system_message(&content, &format!("messages[{index}].content"))?
                    {
                        messages.push(message);
                    }
                }
                (unknown, _) => {
                    return Err(CodecError::unknown_value(
                        format!("messages[{index}].role"),
                        unknown,
                        "\"user\", \"assistant\", \"developer\" or \"system\"",
                    ));
                }
            }
        }
    }

    let mut request = ChatRequest::new(model, messages);
    request.sampling = sampling_from_wire(body)?;
    request.tools = tools_from_wire(body.get("tools"))?;
    request.stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    request.tool_choice = tool_choice_from_wire(body.get("tool_choice"));
    request.parallel_tool_calls = parallel_tool_calls_from_wire(body.get("tool_choice"))?;
    request.reasoning = reasoning_from_wire(body)?;
    if let Some(object) = body.as_object() {
        for (key, value) in object {
            if !CONSUMED_TOP_LEVEL_KEYS.contains(&key.as_str()) {
                request.extensions.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(request)
}

/// Top-level keys the decoder reads into typed IR fields, or keeps out of the IR (`metadata`,
/// KC-Q4). Every other key travels in `extensions` as data, as in the `OpenAI` Chat decode.
/// `output_config` is deliberately not here: its `effort` is typed, and the object stays as data.
const CONSUMED_TOP_LEVEL_KEYS: &[&str] = &[
    "model",
    "messages",
    "system",
    "max_tokens",
    "temperature",
    "top_p",
    "top_k",
    "stop_sequences",
    "stream",
    "tools",
    "tool_choice",
    "thinking",
    "metadata",
];

/// The `thinking.type` values the IR spells.
const MODES: &str = "\"enabled\", \"disabled\" or \"adaptive\"";

/// Canonical IR wire keys this protocol does not define (KC-Q5).
const RESERVED_KEYS: &[&str] = &[
    "reasoning_mode",
    "reasoning_effort",
    "reasoning_budget_tokens",
    "parallel_tool_calls",
    "sampling",
    "response_format",
    "host_values",
];

/// A system prompt (top-level `system`, or a `system` / `developer` turn) as one IR message.
///
/// Unmarked, it is the joined text it always was, so no unmarked request changes shape. When any
/// block carries a prompt-cache marker, each block with text becomes its own part with its own
/// marker (K2): joining would move the breakpoint to the end of the prompt.
fn system_message(value: &Value, path: &str) -> Result<Option<Message>, CodecError> {
    if let Value::Array(blocks) = value {
        let mut parts = Vec::new();
        let mut marked = false;
        for (index, block) in blocks.iter().enumerate() {
            let cache_control = cache_control(block, &format!("{path}[{index}]"))?;
            marked |= cache_control.is_some();
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                parts.push(ContentPart::Text { text: text.to_owned(), cache_control });
            }
        }
        if marked {
            return Ok(Some(Message {
                role: Role::System,
                content: Some(Content::Parts(parts)),
                tool_calls: Vec::new(),
                tool_call_id: None,
                name: None,
                extensions: Extensions::new(),
            }));
        }
    }
    let text = flatten_text(value);
    Ok((!text.is_empty()).then(|| Message::text(Role::System, text)))
}

/// The caller's reasoning configuration, unconverted (K5): `thinking.type` is the mode,
/// `thinking.budget_tokens` the budget, `output_config.effort` the effort.
fn reasoning_from_wire(body: &Value) -> Result<Reasoning, CodecError> {
    let mut reasoning = Reasoning::default();
    if let Some(thinking) = body.get("thinking").filter(|value| !value.is_null()) {
        if !thinking.is_object() {
            return Err(CodecError::unknown_value("thinking", describe(thinking), "an object"));
        }
        reasoning.mode = Some(match thinking.get("type") {
            Some(Value::String(kind)) if kind == "enabled" => ReasoningMode::Enabled,
            Some(Value::String(kind)) if kind == "disabled" => ReasoningMode::Disabled,
            Some(Value::String(kind)) if kind == "adaptive" => ReasoningMode::Adaptive,
            // Closed, as the kernel's mode is: a mode with no IR spelling (`between_tools`) is
            // refused rather than dropped, because dropping it turns thinking on (KC-Q2).
            Some(Value::String(kind)) => {
                return Err(CodecError::unknown_value("thinking.type", kind.as_str(), MODES));
            }
            Some(other) => {
                return Err(CodecError::unknown_value("thinking.type", describe(other), MODES));
            }
            None => return Err(CodecError::unknown_value("thinking.type", "absent", MODES)),
        });
        if let Some(budget) = thinking.get("budget_tokens").filter(|value| !value.is_null()) {
            let number = budget.as_u64().ok_or_else(|| {
                CodecError::out_of_range(
                    "thinking.budget_tokens",
                    describe(budget),
                    "a non-negative integer",
                )
            })?;
            reasoning.budget_tokens = Some(u32::try_from(number).map_err(|_| {
                CodecError::out_of_range(
                    "thinking.budget_tokens",
                    number.to_string(),
                    "at most u32::MAX",
                )
            })?);
        }
    }
    if let Some(effort) = body
        .get("output_config")
        .and_then(|config| config.get("effort"))
        .filter(|value| !value.is_null())
    {
        let Some(level) = effort.as_str() else {
            return Err(CodecError::unknown_value(
                "output_config.effort",
                describe(effort),
                "a string",
            ));
        };
        reasoning.effort = Some(reasoning_effort(level));
    }
    Ok(reasoning)
}

/// `tool_choice.disable_parallel_tool_use`, as the IR's `parallel_tool_calls` (KC-Q6).
fn parallel_tool_calls_from_wire(choice: Option<&Value>) -> Result<Option<bool>, CodecError> {
    let flag = optional_bool(
        choice.and_then(|choice| choice.get("disable_parallel_tool_use")),
        "tool_choice.disable_parallel_tool_use",
    )?;
    Ok(flag.map(|disable| !disable))
}

/// Canonical [`ChatResponse`] -> Anthropic Messages envelope.
///
/// # Exactly one choice
///
/// This wire has one message per response and no `n` to ask for more, so a
/// translated request never asks for more than one. A response carrying zero or
/// several is therefore an anomaly, and both ways of papering over it are worse
/// than refusing: rendering the first silently discards the rest, and rendering
/// zero hands the client an empty answer that looks like the model had nothing
/// to say.
///
/// # Tool ids are derived, not minted
///
/// A tool call with no id of its own gets `toolu_{message id}_{position}`. It is
/// derived rather than random so the same response renders byte for byte the
/// same every time — which is what makes a golden test possible, and what keeps
/// this crate free of a random source.
pub fn anthropic_message_response(
    response: &ChatResponse,
    context: &ResponseContext,
) -> Result<Value, CodecError> {
    if response.choices.len() != 1 {
        return Err(CodecError::unrenderable(
            "choices",
            format!(
                "the Messages envelope carries exactly one message; upstream returned {}",
                response.choices.len()
            ),
        ));
    }

    let message_id = prefixed_id(&response.id, "msg_", &context.fallback_id);
    let derived_id_stem = message_id.trim_start_matches("msg_").to_owned();
    let mut blocks: Vec<Value> = Vec::new();

    let choice = response.choices.first();
    if let Some(choice) = choice {
        if let Some(Content::Parts(parts)) = choice.message.content.as_ref() {
            for part in parts {
                match part {
                    ContentPart::Thinking { thinking, signature } => {
                        let mut block = json!({"type": "thinking", "thinking": thinking});
                        if let Some(signature) = signature {
                            block["signature"] = json!(signature);
                        }
                        blocks.push(block);
                    }
                    ContentPart::RedactedThinking { data } => {
                        blocks.push(json!({"type": "redacted_thinking", "data": data}));
                    }
                    // A block this codec does not model is written back as it
                    // arrived rather than dropped.
                    ContentPart::Unknown(value) => blocks.push(value.clone()),
                    ContentPart::Text { .. } | ContentPart::ImageUrl { .. } => {}
                }
            }
        }

        let text = visible_text(choice.message.content.as_ref());
        if !text.is_empty() {
            blocks.push(json!({"type": "text", "text": text}));
        }

        for (position, call) in choice.message.tool_calls.iter().enumerate() {
            let input = serde_json::from_str::<Value>(&call.arguments).map_err(|_| {
                CodecError::unrenderable(
                    format!("choices[0].message.tool_calls[{position}].arguments"),
                    format!("tool call {:?} produced arguments that are not valid JSON", call.name),
                )
            })?;
            let id = if call.id.is_empty() {
                format!("toolu_{derived_id_stem}_{position}")
            } else {
                call.id.clone()
            };
            blocks.push(json!({
                "type": "tool_use",
                "id": id,
                "name": call.name,
                "input": input,
            }));
        }
    }

    let mut usage = json!({
        "input_tokens": uncached_input_tokens(&response.usage),
        "output_tokens": response.usage.output_tokens,
    });
    // Emitted only when the provider reported them, so a response without cache
    // activity keeps the two fields it always had.
    if response.usage.cache_read_tokens > 0 {
        usage["cache_read_input_tokens"] = json!(response.usage.cache_read_tokens);
    }
    if response.usage.cache_write_tokens > 0 {
        usage["cache_creation_input_tokens"] = json!(response.usage.cache_write_tokens);
    }

    Ok(json!({
        "id": message_id,
        "type": "message",
        "role": "assistant",
        "model": response.model,
        "content": blocks,
        "stop_reason": stop_reason(choice.and_then(|choice| choice.finish_reason.as_ref())),
        "stop_sequence": choice
            .and_then(|choice| choice.stop_sequence.clone())
            .map_or(Value::Null, Value::String),
        "usage": usage,
    }))
}

/// Canonical finish reason -> this wire's `stop_reason` vocabulary.
///
/// A reason this codec does not model travels verbatim: the provider said
/// something specific, and replacing it with "stop" would report a normal
/// completion that did not happen.
pub(crate) fn stop_reason(reason: Option<&FinishReason>) -> Value {
    match reason {
        Some(FinishReason::Length) => json!("max_tokens"),
        Some(FinishReason::ToolCalls) => json!("tool_use"),
        Some(FinishReason::ContentFilter) => json!("content_filter"),
        Some(FinishReason::StopSequence) => json!("stop_sequence"),
        Some(FinishReason::Other(raw)) => json!(raw),
        Some(FinishReason::Stop) | None => json!("end_turn"),
    }
}

fn assistant_from_blocks(parts: &[Value], index: usize) -> Result<Message, CodecError> {
    let mut content: Vec<ContentPart> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    for (position, part) in parts.iter().enumerate() {
        let path = format!("messages[{index}].content[{position}]");
        let kind = part.get("type").and_then(Value::as_str);
        if matches!(kind, Some("tool_use" | "thinking" | "redacted_thinking")) {
            // The IR has no marker slot on a tool call or a reasoning block, so a valid marker
            // there is not carried (recorded in the KC1 record); an invalid one is still refused,
            // like any other (K6).
            cache_control(part, &path)?;
        }
        match kind {
            Some("tool_use") => tool_calls.push(ToolCall {
                id: part.get("id").and_then(Value::as_str).unwrap_or_default().to_owned(),
                name: part.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                // The IR keeps arguments as the string a tool receives; this
                // wire sends an object, so it is serialized once here rather
                // than re-serialized at every later hop.
                arguments: part.get("input").map_or_else(
                    || "{}".to_owned(),
                    |input| match input {
                        Value::String(raw) => raw.clone(),
                        other => other.to_string(),
                    },
                ),
            }),
            Some("thinking") => content.push(ContentPart::Thinking {
                thinking: part
                    .get("thinking")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                signature: part.get("signature").and_then(Value::as_str).map(str::to_owned),
            }),
            // Unreadable by design, but it still has to survive the round trip.
            Some("redacted_thinking") => content.push(ContentPart::RedactedThinking {
                data: part.get("data").and_then(Value::as_str).unwrap_or_default().to_owned(),
            }),
            _ => content.push(part_from_wire(part, &path)?),
        }
    }
    Ok(Message {
        role: Role::Assistant,
        content: Some(Content::Parts(content)),
        tool_calls,
        tool_call_id: None,
        name: None,
        extensions: Extensions::new(),
    })
}

fn extend_with_user_blocks(
    messages: &mut Vec<Message>,
    parts: &[Value],
    index: usize,
) -> Result<(), CodecError> {
    let mut rest: Vec<ContentPart> = Vec::new();
    let mut tool_results: Vec<Message> = Vec::new();
    for (position, part) in parts.iter().enumerate() {
        let path = format!("messages[{index}].content[{position}]");
        if part.get("type").and_then(Value::as_str) == Some("tool_result") {
            tool_results.push(Message {
                role: Role::Tool,
                content: Some(tool_result_content(part, &path)?),
                tool_calls: Vec::new(),
                tool_call_id: Some(
                    part.get("tool_use_id").and_then(Value::as_str).unwrap_or_default().to_owned(),
                ),
                name: None,
                extensions: Extensions::new(),
            });
        } else {
            rest.push(part_from_wire(part, &path)?);
        }
    }
    // Results answer the previous assistant turn, so they precede whatever else
    // this user message carries. The other order rewrites the conversation.
    messages.extend(tool_results);
    if !rest.is_empty() {
        messages.push(Message {
            role: Role::User,
            content: Some(Content::Parts(rest)),
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
            extensions: Extensions::new(),
        });
    }
    Ok(())
}

/// A `tool_result` block's content as IR.
///
/// Unmarked, it is the flattened text it always was. Marked (on the block or on an inner text
/// block), it becomes text parts, each inner marker on its part and the block's marker on the
/// last part (K2): flattening would lose where the cacheable prefix ends.
fn tool_result_content(block: &Value, path: &str) -> Result<Content, CodecError> {
    let block_marker = cache_control(block, path)?;
    let content = block.get("content").unwrap_or(&Value::Null);
    let mut parts = Vec::new();
    let mut marked = block_marker.is_some();
    match content {
        Value::String(text) => parts.push(ContentPart::text(text.clone())),
        Value::Array(inner) => {
            for (position, item) in inner.iter().enumerate() {
                let cache_control = cache_control(item, &format!("{path}.content[{position}]"))?;
                marked |= cache_control.is_some();
                // Text only, as the flattened form: a non-text block in a tool result has no
                // representation the other dialects accept, and the unmarked form drops it too.
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    parts.push(ContentPart::Text { text: text.to_owned(), cache_control });
                }
            }
        }
        _ => {}
    }
    if !marked {
        return Ok(Content::Text(flatten_text(content)));
    }
    if parts.is_empty() {
        parts.push(ContentPart::text(String::new()));
    }
    if let Some(marker) = block_marker
        && let Some(ContentPart::Text { cache_control, .. }) = parts.last_mut()
    {
        *cache_control = Some(marker);
    }
    Ok(Content::Parts(parts))
}

fn sampling_from_wire(body: &Value) -> Result<Sampling, CodecError> {
    let max_output_tokens = match body.get("max_tokens").filter(|value| !value.is_null()) {
        None => None,
        Some(value) => {
            let number = value.as_u64().ok_or_else(|| {
                CodecError::out_of_range("max_tokens", describe(value), "a non-negative integer")
            })?;
            Some(u32::try_from(number).map_err(|_| {
                CodecError::out_of_range("max_tokens", number.to_string(), "at most u32::MAX")
            })?)
        }
    };
    let top_k = match body.get("top_k").filter(|value| !value.is_null()) {
        None => None,
        Some(value) => {
            let number = value.as_u64().ok_or_else(|| {
                CodecError::out_of_range("top_k", describe(value), "a non-negative integer")
            })?;
            Some(u32::try_from(number).map_err(|_| {
                CodecError::out_of_range("top_k", number.to_string(), "at most u32::MAX")
            })?)
        }
    };
    Ok(Sampling {
        temperature: body.get("temperature").and_then(Value::as_f64),
        top_p: body.get("top_p").and_then(Value::as_f64),
        max_output_tokens,
        stop: body
            .get("stop_sequences")
            .and_then(Value::as_array)
            .map(|array| {
                array.iter().filter_map(|entry| entry.as_str().map(str::to_owned)).collect()
            })
            .unwrap_or_default(),
        top_k,
    })
}

fn tools_from_wire(tools: Option<&Value>) -> Result<Vec<ToolDef>, CodecError> {
    let Some(array) = tools.and_then(Value::as_array) else { return Ok(Vec::new()) };
    array
        .iter()
        .enumerate()
        .map(|(index, tool)| {
            let path = format!("tools[{index}]");
            let mut definition = ToolDef::new(
                tool.get("name").and_then(Value::as_str).unwrap_or_default(),
                tool.get("description").and_then(Value::as_str).map(str::to_owned),
                // Called `input_schema` on this wire and `parameters` on the
                // other; one concept, so one IR field.
                tool.get("input_schema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
            );
            definition.strict = optional_bool(tool.get("strict"), &format!("{path}.strict"))?;
            definition.cache_control = cache_control(tool, &path)?;
            Ok(definition)
        })
        .collect()
}

fn tool_choice_from_wire(choice: Option<&Value>) -> Option<ToolChoice> {
    match choice?.get("type").and_then(Value::as_str) {
        Some("auto") => Some(ToolChoice::Auto),
        Some("any") => Some(ToolChoice::Required),
        // A named tool keeps its name. The neighbouring protocol's object shape
        // is the IR's carrier for it, so the intent survives instead of being
        // downgraded to "any tool will do" — which would let the model call one
        // the client did not ask for.
        Some("tool") => Some(ToolChoice::Other(json!({
            "type": "function",
            "function": {"name": choice?.get("name").cloned().unwrap_or(Value::Null)}
        }))),
        _ => None,
    }
}

fn part_from_wire(part: &Value, path: &str) -> Result<ContentPart, CodecError> {
    // Checked for every block, the ones kept verbatim included: an invalid marker is refused
    // wherever it sits (K6), and a valid one on an unmodelled block travels with it.
    let cache_control = cache_control(part, path)?;
    Ok(match part.get("type").and_then(Value::as_str) {
        Some("text") => ContentPart::Text {
            text: part.get("text").and_then(Value::as_str).unwrap_or_default().to_owned(),
            cache_control,
        },
        Some("image") => {
            let Some(source) = part.get("source") else {
                return Ok(ContentPart::Unknown(part.clone()));
            };
            let url = match source.get("type").and_then(Value::as_str) {
                Some("base64") => format!(
                    "data:{};base64,{}",
                    source.get("media_type").and_then(Value::as_str).unwrap_or("image/jpeg"),
                    source.get("data").and_then(Value::as_str).unwrap_or_default()
                ),
                Some("url") => {
                    source.get("url").and_then(Value::as_str).unwrap_or_default().to_owned()
                }
                _ => return Ok(ContentPart::Unknown(part.clone())),
            };
            ContentPart::ImageUrl { image_url: ImageUrl { url, detail: None }, cache_control }
        }
        _ => ContentPart::Unknown(part.clone()),
    })
}

/// The text of a value that may be a string, a block array, or neither.
fn flatten_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn visible_text(content: Option<&Content>) -> String {
    match content {
        Some(Content::Text(text)) => text.clone(),
        Some(Content::Parts(parts)) => parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .concat(),
        None => String::new(),
    }
}

/// This wire's `input_tokens`: the prompt tokens that neither hit nor wrote the
/// cache.
///
/// The IR's `input_tokens` is the whole prompt, partitioned by the two cache
/// buckets (kernel `Usage::total`); Messages reports the cache buckets beside
/// an uncached count instead, so rendering subtracts them. Saturating: a
/// provider whose cache buckets exceed its prompt already broke the IR
/// contract, and this wire has no negative count to carry that.
pub(crate) const fn uncached_input_tokens(usage: &Usage) -> u64 {
    usage
        .input_tokens
        .saturating_sub(usage.cache_read_tokens)
        .saturating_sub(usage.cache_write_tokens)
}
