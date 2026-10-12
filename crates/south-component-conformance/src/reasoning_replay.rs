use serde_json::Value;
use std::collections::HashMap;
use token_station_protocol::{Content, ContentPart, Message, Role, ToolCall};

const FAMILY: &str = "claude-signed-thinking";
const MAX_BLOCKS: usize = 128;

pub enum ReplayRef<'a> {
    Content(&'a ContentPart),
    Tool(&'a ToolCall),
}

pub fn validated_layout(message: &Message) -> Result<Option<Vec<ReplayRef<'_>>>, ()> {
    let family = message.extensions.get("reasoning_replay_protocol_family");
    let layout = message.extensions.get("reasoning_replay_block_layout");
    let (Some(family), Some(layout)) = (family, layout) else {
        return if family.is_none() && layout.is_none() { Ok(None) } else { Err(()) };
    };
    if family.as_str() != Some(FAMILY) {
        return Err(());
    }
    if message.role != Role::Assistant {
        return Err(());
    }
    let layout = layout.as_array().filter(|layout| layout.len() <= MAX_BLOCKS).ok_or(())?;
    let parts = match message.content.as_ref() {
        Some(Content::Parts(parts)) => parts.as_slice(),
        _ => return Err(()),
    };
    let mut part_refs = vec![false; parts.len()];
    let mut tool_refs = vec![false; message.tool_calls.len()];
    let mut tools = HashMap::with_capacity(message.tool_calls.len());
    for (index, call) in message.tool_calls.iter().enumerate() {
        if call.id.is_empty() || tools.insert(call.id.as_str(), (index, call)).is_some() {
            return Err(());
        }
    }
    let mut ordered = Vec::with_capacity(layout.len());
    for entry in layout {
        let object = entry.as_object().ok_or(())?;
        match object.get("kind").and_then(Value::as_str) {
            Some("content") if object.len() == 2 => {
                let ordinal = object
                    .get("ordinal")
                    .and_then(Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok())
                    .ok_or(())?;
                let referenced = part_refs.get_mut(ordinal).ok_or(())?;
                if *referenced {
                    return Err(());
                }
                *referenced = true;
                let part = parts.get(ordinal).ok_or(())?;
                match part {
                    ContentPart::Text { .. } | ContentPart::RedactedThinking { .. } => {}
                    ContentPart::Thinking { signature: Some(signature), .. }
                        if !signature.is_empty() => {}
                    ContentPart::Thinking { .. }
                    | ContentPart::ImageUrl { .. }
                    | ContentPart::Unknown(_) => return Err(()),
                }
                ordered.push(ReplayRef::Content(part));
            }
            Some("tool_call") if object.len() == 2 => {
                let call_id = object.get("call_id").and_then(Value::as_str).ok_or(())?;
                let (index, call) = tools.get(call_id).copied().ok_or(())?;
                if tool_refs[index] {
                    return Err(());
                }
                tool_refs[index] = true;
                ordered.push(ReplayRef::Tool(call));
            }
            _ => return Err(()),
        }
    }
    if part_refs.iter().any(|referenced| !referenced)
        || tool_refs.iter().any(|referenced| !referenced)
    {
        return Err(());
    }
    Ok(Some(ordered))
}

const OPENAI_FAMILY: &str = "openai-encrypted-reasoning";
const OPENAI_ITEMS: &str = "responses_opaque_reasoning_items";
const MAX_OPENAI_ITEM_BYTES: usize = 1024 * 1024;
const MAX_OPENAI_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

/// One entry of an `OpenAI`-family turn, in layout order.
pub enum OpenAiReplayRef<'a> {
    /// A reasoning item, as the north codec projected it.
    Item(&'a Value),
    /// An assistant text.
    Text(&'a str),
    /// A tool call.
    Tool(&'a ToolCall),
}

/// Whether the message carries the `OpenAI` encrypted reasoning family.
pub fn is_openai_family(message: &Message) -> bool {
    message.extensions.get("reasoning_replay_protocol_family").and_then(Value::as_str)
        == Some(OPENAI_FAMILY)
}

fn valid_item(item: &Value) -> Option<usize> {
    let object = item.as_object()?;
    let allowed = ["type", "id", "summary", "encrypted_content", "content"];
    if object.keys().any(|key| !allowed.contains(&key.as_str()))
        || object.get("type").and_then(Value::as_str) != Some("reasoning")
        || object.get("id").and_then(Value::as_str).is_none_or(str::is_empty)
        || !object.get("summary").is_some_and(Value::is_array)
        || object.get("content").is_some_and(|content| !content.is_array())
    {
        return None;
    }
    let encrypted = object.get("encrypted_content").and_then(Value::as_str)?;
    (!encrypted.is_empty()
        && !encrypted.starts_with("tsr.")
        && encrypted.len() <= MAX_OPENAI_ITEM_BYTES)
        .then_some(encrypted.len())
}

/// The ordered entries of an `OpenAI` encrypted reasoning turn (design record
/// `2026-10-11-responses-reasoning-replay-and-passthrough-suite.md` §2.1), `Ok(None)` for a
/// message without the family, `Err` for markers that do not form a valid turn: every item, text
/// part and tool call referenced exactly once; only unsigned thinking parts left unreferenced.
pub fn openai_layout(message: &Message) -> Result<Option<Vec<OpenAiReplayRef<'_>>>, ()> {
    if !is_openai_family(message) {
        return Ok(None);
    }
    let (Some(layout), Some(items)) = (
        message.extensions.get("reasoning_replay_block_layout").and_then(Value::as_array),
        message.extensions.get(OPENAI_ITEMS).and_then(Value::as_array),
    ) else {
        return Err(());
    };
    if message.role != Role::Assistant || layout.len() > MAX_BLOCKS {
        return Err(());
    }
    let mut total = 0usize;
    for item in items {
        total = total.checked_add(valid_item(item).ok_or(())?).ok_or(())?;
    }
    if total > MAX_OPENAI_MESSAGE_BYTES {
        return Err(());
    }
    let parts: &[ContentPart] = match message.content.as_ref() {
        None => &[],
        Some(Content::Parts(parts)) => parts.as_slice(),
        Some(Content::Text(_)) => return Err(()),
    };
    let mut part_refs = vec![false; parts.len()];
    let mut item_refs = vec![false; items.len()];
    let mut tool_refs = vec![false; message.tool_calls.len()];
    let mut tools = HashMap::with_capacity(message.tool_calls.len());
    for (index, call) in message.tool_calls.iter().enumerate() {
        if call.id.is_empty() || tools.insert(call.id.as_str(), (index, call)).is_some() {
            return Err(());
        }
    }
    let ordinal = |object: &serde_json::Map<String, Value>, key: &str| {
        object.get(key).and_then(Value::as_u64).and_then(|value| usize::try_from(value).ok())
    };
    let mut ordered = Vec::with_capacity(layout.len());
    for entry in layout {
        let object = entry.as_object().filter(|object| object.len() == 2).ok_or(())?;
        match object.get("kind").and_then(Value::as_str) {
            Some("reasoning_item") => {
                let index = ordinal(object, "index").ok_or(())?;
                let seen = item_refs.get_mut(index).ok_or(())?;
                if std::mem::replace(seen, true) {
                    return Err(());
                }
                ordered.push(OpenAiReplayRef::Item(&items[index]));
            }
            Some("content") => {
                let index = ordinal(object, "ordinal").ok_or(())?;
                let seen = part_refs.get_mut(index).ok_or(())?;
                if std::mem::replace(seen, true) {
                    return Err(());
                }
                let ContentPart::Text { text } = &parts[index] else { return Err(()) };
                ordered.push(OpenAiReplayRef::Text(text));
            }
            Some("tool_call") => {
                let call_id = object.get("call_id").and_then(Value::as_str).ok_or(())?;
                let (index, call) = tools.get(call_id).copied().ok_or(())?;
                if std::mem::replace(&mut tool_refs[index], true) {
                    return Err(());
                }
                ordered.push(OpenAiReplayRef::Tool(call));
            }
            _ => return Err(()),
        }
    }
    let unreferenced_ok = parts.iter().zip(&part_refs).all(|(part, referenced)| {
        *referenced || matches!(part, ContentPart::Thinking { signature: None, .. })
    });
    if !unreferenced_ok || item_refs.contains(&false) || tool_refs.contains(&false) {
        return Err(());
    }
    Ok(Some(ordered))
}
