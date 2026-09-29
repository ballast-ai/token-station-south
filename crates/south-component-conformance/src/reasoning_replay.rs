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
