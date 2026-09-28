use super::{
    LOCAL_SHELL, ReasoningReplayBlock, ResponsesRequestOptions, decode_reasoning_replay_carrier,
    invalid, namespace_name, required_str, tool_definitions,
};
use crate::CodecError;
use serde_json::{Value, json};
use token_station_protocol::{
    ChatRequest, Content, ContentPart, ImageUrl, Message, ResponseFormat, Role, ToolCall,
    ToolChoice,
};

/// Parse Responses client input directly into canonical IR. Admission remains host-owned.
pub fn chat_request_from_responses(
    body: &Value,
    options: &ResponsesRequestOptions,
) -> Result<ChatRequest, CodecError> {
    if !body.is_object() {
        return Err(invalid("request", "must be an object"));
    }
    let mut request = ChatRequest::new(required_str(&body["model"], "model")?, Vec::new());
    if let Some(previous) = body.get("previous_response_id").filter(|v| !v.is_null()) {
        let previous = required_str(previous, "previous_response_id")?;
        if previous.is_empty() || previous.len() > 256 {
            return Err(invalid("previous_response_id", "must contain 1 to 256 bytes"));
        }
    }
    if let Some(instructions) = body.get("instructions").filter(|v| !v.is_null()) {
        let mut message = Message::text(Role::System, required_str(instructions, "instructions")?);
        message.extensions.insert("responses_transient_instructions".into(), json!(true));
        request.messages.push(message);
    }
    let (input, field) = if options.allow_messages && body.get("messages").is_some() {
        (&body["messages"], "messages")
    } else {
        (&body["input"], "input")
    };
    let messages = match input {
        Value::String(text) if field == "input" => vec![Message::text(Role::User, text)],
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, item)| input_item(item, &format!("{field}[{i}]"), *options))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(invalid(field, "must be a string or an array of input items")),
    };
    if messages.is_empty() && !options.allow_empty_input {
        return Err(invalid(field, "must contain conversation history"));
    }
    for mut message in messages {
        if message.role == Role::Assistant
            && request.messages.last().is_some_and(|m| m.role == Role::Assistant)
        {
            if let Some(previous) = request.messages.last_mut() {
                merge_content(&mut previous.content, message.content.take());
                previous.tool_calls.append(&mut message.tool_calls);
                merge_extensions(&mut previous.extensions, message.extensions);
            }
        } else {
            request.messages.push(message);
        }
    }
    for message in &mut request.messages {
        materialize_replay_message(message)?;
    }
    let (tools, extensions) = tool_definitions(body.get("tools").unwrap_or(&Value::Null))?;
    request.tools = tools;
    request.extensions = extensions;
    request.tool_choice = tool_choice(body.get("tool_choice"), !request.tools.is_empty())?;
    if let Some(reasoning) = body.get("reasoning").filter(|v| !v.is_null()) {
        if !reasoning.is_object() {
            return Err(invalid("reasoning", "must be an object"));
        }
        if let Some(effort) = reasoning.get("effort").filter(|v| !v.is_null()) {
            request.extensions.insert(
                "reasoning_effort".into(),
                json!(required_str(effort, "reasoning.effort")?),
            );
        }
        if let Some(summary) = reasoning.get("summary").filter(|v| !v.is_null()) {
            request.extensions.insert("responses_reasoning_summary".into(), summary.clone());
        }
    }
    if let Some(parallel) = body.get("parallel_tool_calls").filter(|v| !v.is_null()) {
        let parallel = parallel
            .as_bool()
            .ok_or_else(|| invalid("parallel_tool_calls", "must be a boolean"))?;
        request.extensions.insert("parallel_tool_calls".into(), json!(parallel));
    }
    request.response_format = response_format(body)?;
    if let Some(cap) = body
        .get("max_output_tokens")
        .or_else(|| options.allow_call_aliases.then(|| body.get("max_tokens")).flatten())
        .filter(|v| !v.is_null())
    {
        request.sampling.max_output_tokens =
            Some(cap.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| {
                invalid("max_output_tokens", "must be an unsigned 32-bit integer")
            })?);
    }
    request.sampling.temperature = optional_number(body, "temperature")?;
    request.sampling.top_p = optional_number(body, "top_p")?;
    if let Some(stop) = body.get("stop").filter(|v| !v.is_null()) {
        request.sampling.stop = match stop {
            Value::String(s) => vec![s.clone()],
            Value::Array(a) => a
                .iter()
                .enumerate()
                .map(|(i, v)| required_str(v, &format!("stop[{i}]")).map(str::to_owned))
                .collect::<Result<_, _>>()?,
            _ => return Err(invalid("stop", "must be a string or string array")),
        };
    }
    request.stream = match body.get("stream") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or_else(|| invalid("stream", "must be a boolean"))?,
    };
    Ok(request)
}
fn optional_number(body: &Value, field: &str) -> Result<Option<f64>, CodecError> {
    body.get(field)
        .filter(|v| !v.is_null())
        .map(|v| v.as_f64().ok_or_else(|| invalid(field, "must be a number")))
        .transpose()
}
fn response_format(body: &Value) -> Result<Option<ResponseFormat>, CodecError> {
    let Some(text) = body.get("text").filter(|v| !v.is_null()) else { return Ok(None) };
    if !text.is_object() {
        return Err(invalid("text", "must be an object"));
    }
    let Some(format) = text.get("format").filter(|v| !v.is_null()) else { return Ok(None) };
    if !format.is_object() {
        return Err(invalid("text.format", "must be an object"));
    }
    Ok(Some(match required_str(&format["type"], "text.format.type")? {
        "text" => ResponseFormat::Text,
        "json_object" => ResponseFormat::JsonObject,
        "json_schema" => {
            if !format["schema"].is_object() {
                return Err(invalid("text.format.schema", "must be an object"));
            }
            let mut schema = json!({"schema":format["schema"]});
            for key in ["name", "description", "strict"] {
                if let Some(v) = format.get(key) {
                    schema[key] = v.clone();
                }
            }
            ResponseFormat::JsonSchema { json_schema: schema }
        }
        _ => {
            return Err(CodecError::unknown_value(
                "text.format.type",
                "a string",
                "text, json_object, json_schema",
            ));
        }
    }))
}
fn tool_choice(value: Option<&Value>, has_tools: bool) -> Result<Option<ToolChoice>, CodecError> {
    let choice = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(s)) => match s.as_str() {
            "auto" => ToolChoice::Auto,
            "none" => ToolChoice::None,
            "required" => ToolChoice::Required,
            _ => {
                return Err(super::unsupported(
                    "tool_choice",
                    "auto, none, required, named function",
                ));
            }
        },
        Some(v) if v["type"] == "function" => {
            let name = required_str(&v["name"], "tool_choice.name")?;
            let name = if let Some(ns) = v.get("namespace") {
                namespace_name(required_str(ns, "tool_choice.namespace")?, name, "tool_choice")?
            } else {
                name.to_owned()
            };
            ToolChoice::Other(json!({"type":"function","function":{"name":name}}))
        }
        Some(_) => {
            return Err(invalid("tool_choice", "must name a function or a supported selection"));
        }
    };
    if !has_tools {
        return match choice {
            ToolChoice::Auto | ToolChoice::None => Ok(None),
            _ => Err(invalid("tool_choice", "tool_choice requires at least one executable tool")),
        };
    }
    Ok(Some(choice))
}
fn input_item(
    item: &Value,
    path: &str,
    options: ResponsesRequestOptions,
) -> Result<Message, CodecError> {
    let kind = item.get("type").and_then(Value::as_str);
    if kind.is_none() && item.get("role").is_none() {
        return Err(invalid(format!("{path}.type"), "input item declares no type"));
    }
    let mut message = Message::text(Role::Assistant, "");
    message.content = None;
    match kind {
        Some("message") | None => {
            message_input(item, path, options, &mut message)?;
        }
        Some("reasoning") => reasoning_input(item, path, &mut message)?,
        Some("function_call" | "custom_tool_call" | "tool_search_call" | "local_shell_call") => {
            let kind = kind.unwrap_or_default();
            let fallback = if kind == "tool_search_call" || options.allow_call_aliases {
                item.get("id")
            } else {
                None
            };
            let id = required_str(
                item.get("call_id").or(fallback).unwrap_or(&Value::Null),
                &format!("{path}.call_id"),
            )?;
            let (name, arguments) = match kind {
                "function_call" => {
                    let name = required_str(&item["name"], &format!("{path}.name"))?;
                    let name = if let Some(ns) = item.get("namespace") {
                        namespace_name(required_str(ns, &format!("{path}.namespace"))?, name, path)?
                    } else {
                        name.to_owned()
                    };
                    let args = if options.allow_call_aliases {
                        item.get("arguments").map_or_else(
                            || "{}".into(),
                            |v| v.as_str().map_or_else(|| v.to_string(), str::to_owned),
                        )
                    } else {
                        required_str(&item["arguments"], &format!("{path}.arguments"))?.to_owned()
                    };
                    (name, args)
                }
                "custom_tool_call" => (
                    required_str(&item["name"], &format!("{path}.name"))?.to_owned(),
                    json!({"input":item.get("input").cloned().unwrap_or_else(|| json!(""))})
                        .to_string(),
                ),
                "tool_search_call" => (
                    "tool_search".into(),
                    item.get("arguments").cloned().unwrap_or_else(|| json!({})).to_string(),
                ),
                _ => {
                    if !item["action"].is_object() {
                        return Err(invalid(format!("{path}.action"), "must be an object"));
                    }
                    (LOCAL_SHELL.into(), json!({"action":item["action"]}).to_string())
                }
            };
            message.tool_calls.push(ToolCall { id: id.to_owned(), name, arguments });
        }
        Some(
            "function_call_output"
            | "custom_tool_call_output"
            | "tool_search_output"
            | "local_shell_call_output",
        ) => {
            tool_output(item, path, kind, options, &mut message)?;
        }
        _ => {
            return Err(CodecError::unknown_value(
                format!("{path}.type"),
                "an unsupported item",
                "message, function_call, function_call_output, custom_tool_call, custom_tool_call_output, tool_search_call, tool_search_output, local_shell_call, local_shell_call_output, reasoning",
            ));
        }
    }
    Ok(message)
}
fn tool_output(
    item: &Value,
    path: &str,
    kind: Option<&str>,
    options: ResponsesRequestOptions,
    message: &mut Message,
) -> Result<(), CodecError> {
    let fallback = if kind == Some("local_shell_call_output") {
        item.get("id")
    } else if options.allow_call_aliases {
        item.get("tool_call_id")
    } else {
        None
    };
    message.role = Role::Tool;
    message.tool_call_id = Some(
        required_str(
            item.get("call_id").or(fallback).unwrap_or(&Value::Null),
            &format!("{path}.call_id"),
        )?
        .to_owned(),
    );
    if kind == Some("local_shell_call_output") {
        message.name = Some(LOCAL_SHELL.into());
        message.content = Some(Content::Text(
            required_str(&item["output"], &format!("{path}.output"))?.to_owned(),
        ));
    } else if options.allow_call_aliases && kind == Some("function_call_output") {
        let text = item.get("output").map_or_else(String::new, |value| {
            value.as_str().map_or_else(|| value.to_string(), str::to_owned)
        });
        message.content = Some(Content::Text(text));
    } else {
        message.content = content(&item["output"], &format!("{path}.output"), options)?;
    }
    Ok(())
}

fn content(
    value: &Value,
    path: &str,
    options: ResponsesRequestOptions,
) -> Result<Option<Content>, CodecError> {
    match value {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(Content::Text(s.clone()))),
        Value::Array(a) => {
            let mut parts = Vec::new();
            for (i, p) in a.iter().enumerate() {
                let field = format!("{path}[{i}]");
                parts.push(match p["type"].as_str() {
                    Some("text" | "input_text" | "output_text")
                        if options.preserve_unknown_content && !p["text"].is_string() =>
                    {
                        ContentPart::Unknown(p.clone())
                    }
                    Some("text" | "input_text" | "output_text") => ContentPart::Text {
                        text: required_str(&p["text"], &format!("{field}.text"))?.to_owned(),
                    },
                    Some("input_image" | "image_url")
                        if p["type"] == "input_image" || options.preserve_unknown_content =>
                    {
                        let file_id = p.get("file_id").or_else(|| p["image_url"].get("file_id"));
                        if file_id.is_some_and(|value| !value.is_null()) {
                            if options.preserve_unknown_content {
                                let mut image = json!({"file_id": file_id});
                                if let Some(detail) =
                                    p.get("detail").or_else(|| p["image_url"].get("detail"))
                                {
                                    image["detail"] = detail.clone();
                                }
                                ContentPart::Unknown(json!({"type":"image_url","image_url":image}))
                            } else {
                                return Err(super::unsupported(
                                    format!("{field}.file_id"),
                                    "host-resolved image URLs",
                                ));
                            }
                        } else {
                            let image = &p["image_url"];
                            let url = if image.is_string() { image } else { &image["url"] };
                            ContentPart::ImageUrl {
                                image_url: ImageUrl {
                                    url: required_str(url, &format!("{field}.image_url"))?
                                        .to_owned(),
                                    detail: p
                                        .get("detail")
                                        .or_else(|| image.get("detail"))
                                        .and_then(Value::as_str)
                                        .map(str::to_owned),
                                },
                            }
                        }
                    }
                    _ if options.preserve_unknown_content => ContentPart::Unknown(p.clone()),
                    _ => {
                        return Err(super::unsupported(
                            format!("{field}.type"),
                            "input_text, output_text, input_image",
                        ));
                    }
                });
            }
            if parts.is_empty() {
                Ok(None)
            } else if !options.preserve_text_parts
                && parts.iter().all(|p| matches!(p, ContentPart::Text { .. }))
            {
                let text = parts
                    .into_iter()
                    .filter_map(
                        |p| if let ContentPart::Text { text } = p { Some(text) } else { None },
                    )
                    .collect::<String>();
                Ok(Some(Content::Text(text)))
            } else {
                Ok(Some(Content::Parts(parts)))
            }
        }
        _ => Err(invalid(path, "must be a string or content array")),
    }
}
fn merge_content(target: &mut Option<Content>, incoming: Option<Content>) {
    let Some(incoming) = incoming else { return };
    let Some(existing) = target.take() else {
        *target = Some(incoming);
        return;
    };
    *target = Some(match (existing, incoming) {
        (Content::Text(mut a), Content::Text(b)) => {
            if !a.is_empty() && !b.is_empty() {
                a.push('\n');
            }
            a.push_str(&b);
            Content::Text(a)
        }
        (a, b) => {
            let mut parts = match a {
                Content::Text(text) => vec![ContentPart::Text { text }],
                Content::Parts(parts) => parts,
            };
            match b {
                Content::Text(text) => parts.push(ContentPart::Text { text }),
                Content::Parts(mut other) => parts.append(&mut other),
            }
            Content::Parts(parts)
        }
    });
}

fn merge_extensions(
    target: &mut token_station_protocol::Extensions,
    incoming: token_station_protocol::Extensions,
) {
    for (key, value) in incoming {
        if key == "responses_reasoning_replay_items" {
            let entry = target.entry(key).or_insert_with(|| json!([]));
            if let (Some(existing), Some(mut added)) =
                (entry.as_array_mut(), value.as_array().cloned())
            {
                existing.append(&mut added);
            }
        } else {
            target.insert(key, value);
        }
    }
}

fn materialize_replay_message(message: &mut Message) -> Result<(), CodecError> {
    let Some(items) =
        message.extensions.get("responses_reasoning_replay_items").and_then(Value::as_array)
    else {
        return Ok(());
    };
    let texts = match message.content.take() {
        None => Vec::new(),
        Some(Content::Text(text)) => vec![text],
        Some(Content::Parts(parts)) => parts
            .into_iter()
            .filter_map(|part| match part {
                ContentPart::Text { text } => Some(text),
                _ => None,
            })
            .collect(),
    };
    let mut parts = Vec::new();
    let mut text_refs = vec![0u8; texts.len()];
    let mut tool_refs = vec![0u8; message.tool_calls.len()];
    for item in items {
        let encoded = item["encrypted_content"].as_str().ok_or_else(|| {
            CodecError::ReasoningReplayInvalid { field: "encrypted_content".to_owned() }
        })?;
        for block in decode_reasoning_replay_carrier(encoded)?.blocks() {
            match block {
                ReasoningReplayBlock::Thinking { thinking, signature } => {
                    parts.push(ContentPart::Thinking {
                        thinking: thinking.clone(),
                        signature: Some(signature.clone()),
                    });
                }
                ReasoningReplayBlock::RedactedThinking { data } => {
                    parts.push(ContentPart::RedactedThinking { data: data.clone() });
                }
                ReasoningReplayBlock::TextRef { ordinal } => {
                    let index = usize::try_from(*ordinal).map_err(|_| {
                        CodecError::ReasoningReplayInvalid { field: "encrypted_content".to_owned() }
                    })?;
                    let Some(text) = texts.get(index) else {
                        return Err(CodecError::ReasoningReplayInvalid {
                            field: "encrypted_content".to_owned(),
                        });
                    };
                    text_refs[index] = text_refs[index].saturating_add(1);
                    parts.push(ContentPart::Text { text: text.clone() });
                }
                ReasoningReplayBlock::ToolCallRef { call_id } => {
                    let Some(index) =
                        message.tool_calls.iter().position(|call| &call.id == call_id)
                    else {
                        return Err(CodecError::ReasoningReplayInvalid {
                            field: "encrypted_content".to_owned(),
                        });
                    };
                    tool_refs[index] = tool_refs[index].saturating_add(1);
                }
            }
        }
    }
    if text_refs.iter().any(|count| *count != 1) || tool_refs.iter().any(|count| *count != 1) {
        return Err(CodecError::ReasoningReplayInvalid { field: "encrypted_content".to_owned() });
    }
    message.content = (!parts.is_empty()).then_some(Content::Parts(parts));
    Ok(())
}

fn message_input(
    item: &Value,
    path: &str,
    options: ResponsesRequestOptions,
    message: &mut Message,
) -> Result<(), CodecError> {
    message.role = match required_str(&item["role"], &format!("{path}.role"))? {
        "system" | "developer" => Role::System,
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "tool" if options.allow_messages => Role::Tool,
        _ => {
            return Err(invalid(format!("{path}.role"), "unsupported message role"));
        }
    };
    message.content = content(&item["content"], &format!("{path}.content"), options)?;
    if options.allow_messages {
        if let Some(reasoning) = item.get("reasoning_content").filter(|v| !v.is_null()) {
            let thinking = required_str(reasoning, &format!("{path}.reasoning_content"))?;
            merge_content(
                &mut message.content,
                Some(Content::Parts(vec![ContentPart::Thinking {
                    thinking: thinking.to_owned(),
                    signature: None,
                }])),
            );
        }
        if let Some(id) = item.get("tool_call_id") {
            message.tool_call_id =
                Some(required_str(id, &format!("{path}.tool_call_id"))?.to_owned());
        }
        if let Some(calls) = item.get("tool_calls") {
            for (i, c) in calls
                .as_array()
                .ok_or_else(|| invalid(format!("{path}.tool_calls"), "must be an array"))?
                .iter()
                .enumerate()
            {
                message.tool_calls.push(ToolCall {
                    id: required_str(&c["id"], &format!("{path}.tool_calls[{i}].id"))?.to_owned(),
                    name: required_str(
                        &c["function"]["name"],
                        &format!("{path}.tool_calls[{i}].function.name"),
                    )?
                    .to_owned(),
                    arguments: required_str(
                        &c["function"]["arguments"],
                        &format!("{path}.tool_calls[{i}].function.arguments"),
                    )?
                    .to_owned(),
                });
            }
        }
    }

    Ok(())
}

fn reasoning_input(item: &Value, path: &str, message: &mut Message) -> Result<(), CodecError> {
    for field in ["content", "summary"] {
        if item.get(field).is_some_and(|v| !v.is_null() && !v.is_array()) {
            return Err(invalid(format!("{path}.{field}"), "must be an array"));
        }
    }
    let source = if item.get("content").and_then(Value::as_array).is_some_and(|a| !a.is_empty()) {
        "content"
    } else {
        "summary"
    };
    if let Some(parts) = item.get(source).filter(|v| !v.is_null()) {
        let parts = parts
            .as_array()
            .ok_or_else(|| invalid(format!("{path}.{source}"), "must be an array"))?;
        let mut thinking = Vec::new();
        for (i, p) in parts.iter().enumerate() {
            let expected = if source == "content" { "reasoning_text" } else { "summary_text" };
            if p["type"] != expected {
                return Err(super::unsupported(
                    format!("{path}.{source}[{i}].type"),
                    "reasoning_text, summary_text",
                ));
            }
            thinking.push(ContentPart::Thinking {
                thinking: required_str(&p["text"], &format!("{path}.{source}[{i}].text"))?
                    .to_owned(),
                signature: None,
            });
        }
        if !thinking.is_empty() {
            message.content = Some(Content::Parts(thinking));
        }
    }
    for (key, extension) in [
        ("id", "responses_reasoning_id"),
        ("encrypted_content", "responses_reasoning_encrypted_content"),
    ] {
        if let Some(v) = item.get(key).filter(|v| !v.is_null()) {
            message
                .extensions
                .insert(extension.into(), json!(required_str(v, &format!("{path}.{key}"))?));
        }
    }
    if let Some(encoded) = item.get("encrypted_content").and_then(Value::as_str)
        && encoded.starts_with("tsr.c1.")
    {
        decode_reasoning_replay_carrier(encoded)?;
        let id = required_str(&item["id"], &format!("{path}.id"))?;
        message.extensions.remove("responses_reasoning_encrypted_content");
        message.extensions.insert(
            "responses_reasoning_replay_items".into(),
            json!([{"id":id,"encrypted_content":encoded}]),
        );
        message
            .extensions
            .insert("reasoning_replay_protocol_family".into(), json!("claude-signed-thinking"));
    }

    Ok(())
}
