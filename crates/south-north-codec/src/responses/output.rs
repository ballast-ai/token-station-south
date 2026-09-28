use super::replay::carrier_from_message;
use super::tools::{ToolMap, restore_map, tool_item};
use super::{ResponsesContext, ResponsesReasoningMode, decode_reasoning_replay_carrier, invalid};
use crate::CodecError;
use serde_json::{Value, json};
use token_station_protocol::{ChatResponse, Choice, Content, ContentPart, FinishReason, Usage};
/// Render an IR response using only host-supplied identity and time.
pub fn responses_response(
    response: &ChatResponse,
    context: &ResponsesContext,
) -> Result<Value, CodecError> {
    let restore = restore_map(&context.inbound_tools)?;
    let mut output = Vec::new();
    for choice in &response.choices {
        output.extend(choice_output(choice, context, &restore)?);
    }
    let finish = response
        .choices
        .iter()
        .find_map(|c| {
            c.finish_reason
                .as_ref()
                .filter(|r| matches!(r, FinishReason::Length | FinishReason::ContentFilter))
        })
        .or_else(|| response.choices.first().and_then(|c| c.finish_reason.as_ref()));
    response_object(context, &output, response.usage, finish)
}

fn choice_output(
    choice: &Choice,
    context: &ResponsesContext,
    restore: &ToolMap,
) -> Result<Vec<Value>, CodecError> {
    let mut output = Vec::new();
    let mut texts = Vec::new();
    let mut thinking = String::new();
    let mut signature = None;
    let replay_items =
        choice.message.extensions.get("responses_reasoning_replay_items").and_then(Value::as_array);
    let generated_carrier = carrier_from_message(&choice.message)?;
    let parts = match &choice.message.content {
        None => Vec::new(),
        Some(Content::Text(text)) => vec![ContentPart::Text { text: text.clone() }],
        Some(Content::Parts(parts)) => parts.clone(),
    };
    for part in parts {
        match part {
            ContentPart::Text { text } => texts.push(json!({"type":"output_text","text":text})),
            ContentPart::Thinking { thinking: text, signature: part_signature } => {
                if replay_items.is_none() && generated_carrier.is_none() {
                    thinking.push_str(&text);
                    if part_signature.is_some() {
                        signature = part_signature;
                    }
                }
            }
            ContentPart::RedactedThinking { data } => {
                if replay_items.is_none() && generated_carrier.is_none() {
                    signature = Some(data);
                }
            }
            ContentPart::Unknown(value) => texts.push(value),
            ContentPart::ImageUrl { .. } => {
                return Err(super::unsupported(
                    "choices.message.content",
                    "text, thinking, redacted thinking, opaque content",
                ));
            }
        }
    }
    if let Some(items) = replay_items {
        output.extend(replay_items_output(items, context)?);
    } else if let Some(encoded) = generated_carrier {
        let summary = replay_summary(&encoded)?;
        let mut replay = reasoning_item(
            &format!("rs_{}_{}", context.response_id, choice.index),
            &summary,
            None,
            context,
        );
        replay["encrypted_content"] = json!(encoded);
        output.push(replay);
    }
    if !thinking.is_empty() || (context.render_legacy_encrypted_reasoning && signature.is_some()) {
        output.push(reasoning_item(
            &format!("rs_{}_{}", context.response_id, choice.index),
            &thinking,
            signature.as_deref(),
            context,
        ));
    }
    if !texts.is_empty() {
        output.push(json!({"type":"message","id":format!("msg_{}_{}",context.response_id,choice.index),"role":"assistant","status":"completed","content":texts}));
    }
    for call in &choice.message.tool_calls {
        output.push(tool_item(&call.id, &call.name, &call.arguments, "completed", restore)?);
    }
    Ok(output)
}

fn replay_items_output(
    items: &[Value],
    context: &ResponsesContext,
) -> Result<Vec<Value>, CodecError> {
    items
        .iter()
        .map(|item| {
            let encoded = item["encrypted_content"].as_str().ok_or_else(|| {
                CodecError::ReasoningReplayInvalid { field: "encrypted_content".to_owned() }
            })?;
            let summary = replay_summary(encoded)?;
            let id = item["id"].as_str().ok_or_else(|| CodecError::ReasoningReplayInvalid {
                field: "encrypted_content".to_owned(),
            })?;
            let mut replay = reasoning_item(id, &summary, None, context);
            replay["encrypted_content"] = json!(encoded);
            Ok(replay)
        })
        .collect()
}

fn replay_summary(encoded: &str) -> Result<String, CodecError> {
    Ok(decode_reasoning_replay_carrier(encoded)?
        .blocks()
        .iter()
        .filter_map(|block| match block {
            super::ReasoningReplayBlock::Thinking { thinking, .. } => Some(thinking.as_str()),
            _ => None,
        })
        .collect())
}
pub(super) fn reasoning_item(
    id: &str,
    text: &str,
    signature: Option<&str>,
    context: &ResponsesContext,
) -> Value {
    let mut item = match context.reasoning {
        ResponsesReasoningMode::RawContent => {
            json!({"type":"reasoning","id":id,"summary":[],"content":[{"type":"reasoning_text","text":text}]})
        }
        ResponsesReasoningMode::Summary => {
            json!({"type":"reasoning","id":id,"status":"completed","summary":[{"type":"summary_text","text":text}]})
        }
    };
    if context.render_legacy_encrypted_reasoning {
        item["encrypted_content"] = signature.map_or(Value::Null, |s| json!(s));
    }
    item
}
pub(super) fn response_object(
    context: &ResponsesContext,
    output: &[Value],
    usage: Usage,
    finish: Option<&FinishReason>,
) -> Result<Value, CodecError> {
    let total = usage
        .input_tokens
        .checked_add(usage.output_tokens)
        .ok_or_else(|| invalid("usage.total_tokens", "token total overflow"))?;
    let incomplete = match finish {
        Some(FinishReason::Length) => Some("max_output_tokens"),
        Some(FinishReason::ContentFilter) => Some("content_filter"),
        _ => None,
    };
    let text = output
        .iter()
        .filter(|v| v["type"] == "message")
        .flat_map(|v| v["content"].as_array().into_iter().flatten())
        .filter_map(|v| v["text"].as_str())
        .collect::<String>();
    Ok(
        json!({"id":context.response_id,"object":"response","created_at":context.created_at,"model":context.model,"status":if incomplete.is_some(){"incomplete"}else{"completed"},"error":null,"incomplete_details":incomplete.map(|reason|json!({"reason":reason})),"output":output,"output_text":text,"usage":{"input_tokens":usage.input_tokens,"output_tokens":usage.output_tokens,"total_tokens":total,"input_tokens_details":{"cached_tokens":usage.cache_read_tokens,"cache_write_tokens":usage.cache_write_tokens},"output_tokens_details":{"reasoning_tokens":usage.reasoning_tokens}}}),
    )
}
