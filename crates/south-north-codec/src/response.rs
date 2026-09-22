//! Outbound: canonical [`ChatResponse`] -> a client's non-streaming wire.
//!
//! Everything the wire needs but the IR does not carry — the `created` stamp,
//! an id to fall back on when the upstream did not supply one — arrives in
//! [`ResponseContext`]. That is not ceremony: it is what keeps this module a
//! function of its inputs, so a golden test can assert equality on the whole
//! envelope instead of masking the parts that move.

use serde_json::{Value, json};
use token_station_protocol::{ChatResponse, Content, ContentPart};

use crate::CodecError;

/// The facts the wire needs and the canonical response does not carry.
///
/// The host supplies them because the host is the only party allowed to read a
/// clock or mint an identifier (see the crate docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseContext {
    /// Unix seconds for the wire's `created` field.
    pub created: i64,
    /// Used when the upstream response carries no id of its own.
    ///
    /// Not optional: a response with no id at all is a worse outcome than one
    /// with a host-minted id, and making the caller supply it keeps the choice
    /// visible instead of hiding a generator in here.
    pub fallback_id: String,
}

/// Canonical [`ChatResponse`] -> `OpenAI` Chat completion envelope.
///
/// # Shape decisions worth naming
///
/// - **`id`**: the upstream id keeps its `chatcmpl-` prefix if it already has
///   one, and gains it otherwise, so `wire -> IR -> wire` is stable rather than
///   growing a prefix per round trip. An empty id falls back to the context's.
/// - **Empty text is `null`**, not `""` — the two mean the same thing on this
///   wire and clients that check for a missing content field expect `null`.
/// - **Reasoning text** is written back as `reasoning_content`: the IR keeps
///   thinking in ordered blocks, but this wire has exactly one slot for it, so
///   the blocks are concatenated in order. Block boundaries are not expressible
///   here; that loss is the wire's, not the codec's.
/// - **Usage sub-buckets are emitted only when non-zero.** A provider that
///   reports no cache activity produces the same three fields it always did, so
///   adding the buckets changed no existing response.
pub fn openai_chat_response(
    response: &ChatResponse,
    context: &ResponseContext,
) -> Result<Value, CodecError> {
    let mut choices: Vec<Value> = Vec::with_capacity(response.choices.len());
    for (index, choice) in response.choices.iter().enumerate() {
        let (thinking, text) = split_thinking_and_text(choice.message.content.as_ref());
        let mut message = json!({
            "role": "assistant",
            "content": if text.is_empty() { Value::Null } else { Value::String(text) },
        });
        if !thinking.is_empty() {
            message["reasoning_content"] = Value::String(thinking);
        }
        if !choice.message.tool_calls.is_empty() {
            let mut calls = Vec::with_capacity(choice.message.tool_calls.len());
            for call in &choice.message.tool_calls {
                // The arguments travel as the exact string the model produced.
                // They are still checked for well-formedness, because a client
                // that cannot parse them has no way to tell whether the model
                // or the gateway produced the garbage — and a codec that
                // substitutes `{}` to make the shape valid hands the client a
                // tool call the model never made.
                if serde_json::from_str::<Value>(&call.arguments).is_err() {
                    return Err(CodecError::unrenderable(
                        format!("choices[{index}].message.tool_calls[{}].arguments", calls.len()),
                        format!(
                            "tool call {:?} produced arguments that are not valid JSON",
                            call.name
                        ),
                    ));
                }
                calls.push(json!({
                    "id": call.id,
                    "type": "function",
                    "function": {"name": call.name, "arguments": call.arguments},
                }));
            }
            message["tool_calls"] = Value::Array(calls);
        }
        choices.push(json!({
            "index": choice.index,
            "message": message,
            "finish_reason": choice.finish_reason,
        }));
    }

    let mut usage = json!({
        "prompt_tokens": response.usage.input_tokens,
        "completion_tokens": response.usage.output_tokens,
        "total_tokens": response.usage.input_tokens + response.usage.output_tokens,
    });
    if response.usage.cache_read_tokens > 0 {
        usage["prompt_tokens_details"] = json!({"cached_tokens": response.usage.cache_read_tokens});
    }
    if response.usage.reasoning_tokens > 0 {
        usage["completion_tokens_details"] =
            json!({"reasoning_tokens": response.usage.reasoning_tokens});
    }

    Ok(json!({
        "id": prefixed_id(&response.id, "chatcmpl-", &context.fallback_id),
        "object": "chat.completion",
        "created": context.created,
        "model": response.model,
        "choices": choices,
        "usage": usage,
    }))
}

/// Apply a wire's id prefix without stacking it on every round trip.
pub(crate) fn prefixed_id(id: &str, prefix: &str, fallback: &str) -> String {
    let source = if id.is_empty() { fallback } else { id };
    if source.starts_with(prefix) { source.to_owned() } else { format!("{prefix}{source}") }
}

/// Split ordered IR content into (thinking text, visible text).
///
/// Blocks this wire cannot express — redacted thinking, unknown blocks — are
/// dropped here and nowhere else, so the loss has one address.
fn split_thinking_and_text(content: Option<&Content>) -> (String, String) {
    match content {
        None => (String::new(), String::new()),
        Some(Content::Text(text)) => (String::new(), text.clone()),
        Some(Content::Parts(parts)) => {
            let mut thinking = String::new();
            let mut text = String::new();
            for part in parts {
                match part {
                    ContentPart::Thinking { thinking: chunk, .. } => thinking.push_str(chunk),
                    ContentPart::Text { text: chunk } => text.push_str(chunk),
                    _ => {}
                }
            }
            (thinking, text)
        }
    }
}
