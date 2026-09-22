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
    Role, Sampling, ToolCall, ToolChoice, ToolDef,
};

use crate::{CodecError, ResponseContext, describe, response::prefixed_id};

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
pub fn chat_request_from_anthropic_messages(body: &Value) -> Result<ChatRequest, CodecError> {
    let model = body.get("model").and_then(Value::as_str).unwrap_or_default().to_owned();
    let mut messages: Vec<Message> = Vec::new();

    if let Some(system) = body.get("system") {
        let text = flatten_text(system);
        if !text.is_empty() {
            messages.push(Message::text(Role::System, text));
        }
    }

    if let Some(raw_messages) = body.get("messages").and_then(Value::as_array) {
        for (index, raw) in raw_messages.iter().enumerate() {
            let role = raw.get("role").and_then(Value::as_str).unwrap_or("user");
            let content = raw.get("content").cloned().unwrap_or(Value::Null);
            match (role, &content) {
                ("assistant", Value::Array(parts)) => {
                    messages.push(assistant_from_blocks(parts));
                }
                ("user", Value::Array(parts)) => extend_with_user_blocks(&mut messages, parts),
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
                    let text = flatten_text(&content);
                    if !text.is_empty() {
                        messages.push(Message::text(Role::System, text));
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
    request.tools = tools_from_wire(body.get("tools"));
    request.stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    request.tool_choice = tool_choice_from_wire(body.get("tool_choice"));
    Ok(request)
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
        "input_tokens": response.usage.input_tokens,
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

fn assistant_from_blocks(parts: &[Value]) -> Message {
    let mut content: Vec<ContentPart> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    for part in parts {
        match part.get("type").and_then(Value::as_str) {
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
            _ => content.push(part_from_wire(part)),
        }
    }
    Message {
        role: Role::Assistant,
        content: Some(Content::Parts(content)),
        tool_calls,
        tool_call_id: None,
        name: None,
        extensions: Extensions::new(),
    }
}

fn extend_with_user_blocks(messages: &mut Vec<Message>, parts: &[Value]) {
    let mut rest: Vec<ContentPart> = Vec::new();
    let mut tool_results: Vec<Message> = Vec::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) == Some("tool_result") {
            tool_results.push(Message {
                role: Role::Tool,
                content: Some(Content::Text(flatten_text(
                    part.get("content").unwrap_or(&Value::Null),
                ))),
                tool_calls: Vec::new(),
                tool_call_id: Some(
                    part.get("tool_use_id").and_then(Value::as_str).unwrap_or_default().to_owned(),
                ),
                name: None,
                extensions: Extensions::new(),
            });
        } else {
            rest.push(part_from_wire(part));
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
    })
}

fn tools_from_wire(tools: Option<&Value>) -> Vec<ToolDef> {
    tools
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .map(|tool| ToolDef {
                    name: tool.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    description: tool.get("description").and_then(Value::as_str).map(str::to_owned),
                    // Called `input_schema` on this wire and `parameters` on the
                    // other; one concept, so one IR field.
                    parameters: tool
                        .get("input_schema")
                        .cloned()
                        .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
                })
                .collect()
        })
        .unwrap_or_default()
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

fn part_from_wire(part: &Value) -> ContentPart {
    match part.get("type").and_then(Value::as_str) {
        Some("text") => ContentPart::Text {
            text: part.get("text").and_then(Value::as_str).unwrap_or_default().to_owned(),
        },
        Some("image") => {
            let Some(source) = part.get("source") else {
                return ContentPart::Unknown(part.clone());
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
                _ => return ContentPart::Unknown(part.clone()),
            };
            ContentPart::ImageUrl { image_url: ImageUrl { url, detail: None } }
        }
        _ => ContentPart::Unknown(part.clone()),
    }
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
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .concat(),
        None => String::new(),
    }
}
