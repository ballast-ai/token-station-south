//! Inbound: a client's request wire -> canonical [`ChatRequest`].
//!
//! The rule that shapes every decision below: **what the client sent survives**.
//! A field the IR models goes to its typed slot; a field it does not model goes
//! to `extensions` verbatim; a block type or role the protocol does not define
//! is *refused*, not dropped. Dropping is the failure mode that costs the most
//! to diagnose later, because the request still succeeds — just without the part
//! that mattered.

use serde_json::{Value, json};
use token_station_protocol::{
    ChatRequest, Content, ContentPart, Extensions, ImageUrl, Message, Role, Sampling, ToolCall,
    ToolChoice, ToolDef,
};

use crate::{CodecError, describe};

/// Top-level `OpenAI` Chat keys that already have a typed home in the IR.
///
/// Everything else goes to `extensions`. Keys on this list must not also be
/// copied there: `extensions` is flattened on serialization, so a key present in
/// both places would appear twice on the wire.
///
/// `response_format` is on the list even though it may fail to parse. The IR
/// models three shapes (`text` / `json_object` / `json_schema`); a fourth shape
/// leaves the typed slot empty and this list keeps it out of `extensions` as
/// well. That loss is deliberate and registered — the alternative is a duplicate
/// key — and a host that must not lose it validates the field before calling.
const TYPED_TOP_LEVEL_KEYS: &[&str] = &[
    "model",
    "messages",
    "tools",
    "tool_choice",
    "response_format",
    "temperature",
    "top_p",
    "max_completion_tokens",
    "max_tokens",
    "stop",
    "stream",
];

/// `OpenAI` Chat request body -> canonical [`ChatRequest`].
///
/// # The output cap
///
/// `max_completion_tokens` wins over the legacy `max_tokens` when both are
/// present; a host that treats disagreement between the two as a client error
/// rejects before calling here. A value the IR cannot represent is **reported,
/// not clamped** — clamping substitutes a number the client never asked for, and
/// the client learns about it only by receiving a truncated answer.
///
/// # Roles
///
/// `developer` and `system` both map to [`Role::System`] — they are the same
/// concept in the `OpenAI` ecosystem, and refusing `developer` would make a
/// perfectly ordinary request fail. Any other role is refused with its position,
/// because guessing on the client's behalf is how a turn silently changes
/// meaning.
///
/// # Reasoning text
///
/// An assistant message's `reasoning_content` becomes a leading
/// [`ContentPart::Thinking`] block — leading, because it is produced before the
/// visible answer and the render side concatenates blocks in order. Inbound
/// acceptance is unconditional: whether that text is ever replayed upstream is
/// the host's per-model decision, and normalization must not pre-empt it by
/// discarding the block here.
pub fn chat_request_from_openai_chat(body: &Value) -> Result<ChatRequest, CodecError> {
    let model = body.get("model").and_then(Value::as_str).unwrap_or_default().to_owned();

    let mut messages: Vec<Message> = Vec::new();
    if let Some(raw_messages) = body.get("messages").and_then(Value::as_array) {
        for (index, raw) in raw_messages.iter().enumerate() {
            let role = raw.get("role").and_then(Value::as_str).unwrap_or_default();
            let content = content_from_wire(raw.get("content"));
            match role {
                "system" | "developer" => messages.push(message(Role::System, content, raw)),
                "user" => messages.push(message(Role::User, content, raw)),
                "assistant" => {
                    let mut assistant = message(Role::Assistant, content, raw);
                    if let Some(reasoning) = raw
                        .get("reasoning_content")
                        .and_then(Value::as_str)
                        .filter(|text| !text.is_empty())
                    {
                        prepend_thinking(&mut assistant, reasoning);
                    }
                    assistant.tool_calls = tool_calls_from_wire(raw.get("tool_calls"));
                    messages.push(assistant);
                }
                "tool" => {
                    let mut tool = message(Role::Tool, content, raw);
                    tool.tool_call_id =
                        raw.get("tool_call_id").and_then(Value::as_str).map(str::to_owned);
                    messages.push(tool);
                }
                unknown => {
                    return Err(CodecError::unknown_value(
                        format!("messages[{index}].role"),
                        unknown,
                        "\"system\", \"developer\", \"user\", \"assistant\" or \"tool\"",
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
    request.response_format =
        body.get("response_format").and_then(|value| serde_json::from_value(value.clone()).ok());

    if let Some(object) = body.as_object() {
        for (key, value) in object {
            if TYPED_TOP_LEVEL_KEYS.contains(&key.as_str()) {
                continue;
            }
            request.extensions.insert(key.clone(), value.clone());
        }
    }

    Ok(request)
}

fn sampling_from_wire(body: &Value) -> Result<Sampling, CodecError> {
    // The canonical key wins; the legacy one is the fallback. A host that
    // rejects the two disagreeing does so before calling here.
    let (cap_field, cap_value) = body.get("max_completion_tokens").map_or_else(
        || ("max_tokens", body.get("max_tokens")),
        |value| ("max_completion_tokens", Some(value)),
    );
    let max_output_tokens = match cap_value.filter(|value| !value.is_null()) {
        None => None,
        Some(value) => {
            let number = value.as_u64().ok_or_else(|| {
                CodecError::out_of_range(cap_field, describe(value), "a non-negative integer")
            })?;
            Some(u32::try_from(number).map_err(|_| {
                CodecError::out_of_range(cap_field, number.to_string(), "at most u32::MAX")
            })?)
        }
    };

    Ok(Sampling {
        temperature: body.get("temperature").and_then(Value::as_f64),
        top_p: body.get("top_p").and_then(Value::as_f64),
        max_output_tokens,
        // Both shapes are legal on this wire; the IR keeps one.
        stop: match body.get("stop") {
            Some(Value::String(one)) => vec![one.clone()],
            Some(Value::Array(many)) => {
                many.iter().filter_map(|entry| entry.as_str().map(str::to_owned)).collect()
            }
            _ => Vec::new(),
        },
    })
}

fn tools_from_wire(tools: Option<&Value>) -> Vec<ToolDef> {
    tools
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .map(|tool| {
                    // The definition normally nests under `function`; falling
                    // back to the top level keeps a slightly-off shape from
                    // turning the whole tool into an empty shell.
                    let source = tool.get("function").unwrap_or(tool);
                    ToolDef {
                        name: source
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        description: source
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        parameters: source
                            .get("parameters")
                            .cloned()
                            .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn tool_choice_from_wire(choice: Option<&Value>) -> Option<ToolChoice> {
    let choice = choice?;
    match choice.as_str() {
        Some("auto") => Some(ToolChoice::Auto),
        Some("none") => Some(ToolChoice::None),
        Some("required") => Some(ToolChoice::Required),
        // An unrecognised string is carried whole rather than guessed at; the
        // render side writes it back byte for byte.
        Some(_) => Some(ToolChoice::Other(choice.clone())),
        None => match choice {
            Value::Null => None,
            other => Some(ToolChoice::Other(other.clone())),
        },
    }
}

fn message(role: Role, content: Option<Content>, raw: &Value) -> Message {
    Message {
        role,
        content,
        tool_calls: Vec::new(),
        tool_call_id: None,
        name: raw.get("name").and_then(Value::as_str).map(str::to_owned),
        extensions: Extensions::new(),
    }
}

fn prepend_thinking(message: &mut Message, reasoning: &str) {
    let thinking = ContentPart::Thinking { thinking: reasoning.to_owned(), signature: None };
    match message.content.take() {
        Some(Content::Parts(mut parts)) => {
            parts.insert(0, thinking);
            message.content = Some(Content::Parts(parts));
        }
        Some(Content::Text(text)) => {
            message.content = Some(Content::Parts(vec![thinking, ContentPart::Text { text }]));
        }
        None => message.content = Some(Content::Parts(vec![thinking])),
    }
}

fn content_from_wire(content: Option<&Value>) -> Option<Content> {
    match content {
        Some(Value::String(text)) => Some(Content::Text(text.clone())),
        Some(Value::Array(parts)) => {
            Some(Content::Parts(parts.iter().map(part_from_wire).collect()))
        }
        // Explicit null and absent are the same absence, and the render side
        // writes `null` back for it, so the round trip closes.
        Some(Value::Null) | None => None,
        // Any other scalar on a legal role is not guessed at: it is carried
        // whole so nothing is invented and nothing is lost.
        Some(other) => Some(Content::Parts(vec![ContentPart::Unknown(other.clone())])),
    }
}

fn part_from_wire(part: &Value) -> ContentPart {
    match part.get("type").and_then(Value::as_str) {
        Some("text") => ContentPart::Text {
            text: part.get("text").and_then(Value::as_str).unwrap_or_default().to_owned(),
        },
        Some("image_url") => {
            let Some(image) = part.get("image_url") else {
                return ContentPart::Unknown(part.clone());
            };
            // No url means no image: carrying the block whole lets the upstream
            // rule on it, rather than sending it an image with an empty source.
            let Some(url) = image.get("url").and_then(Value::as_str) else {
                return ContentPart::Unknown(part.clone());
            };
            ContentPart::ImageUrl {
                image_url: ImageUrl {
                    url: url.to_owned(),
                    detail: image.get("detail").and_then(Value::as_str).map(str::to_owned),
                },
            }
        }
        _ => ContentPart::Unknown(part.clone()),
    }
}

fn tool_calls_from_wire(tool_calls: Option<&Value>) -> Vec<ToolCall> {
    tool_calls
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .map(|call| {
                    let source = call.get("function").unwrap_or(call);
                    ToolCall {
                        id: call.get("id").and_then(Value::as_str).unwrap_or_default().to_owned(),
                        name: source
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        // Kept as the exact string the model produced: the IR
                        // never parses or re-orders arguments, because the tool
                        // on the other end may care about both.
                        arguments: source
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}
