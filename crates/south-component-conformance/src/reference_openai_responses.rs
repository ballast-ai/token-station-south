//! The native reference implementation of the `OpenAI` Responses upstream component
//! (`provider-openai-responses`, family `openai-responses`).
//!
//! Design record: `docs/design/2026-09-30-openai-responses-upstream-component.md` (accepted
//! 2026-10-10). This is its step R1: the `openai-responses` family only. The `openai-codex`
//! family, its credential recipe and its request rules are step R3.
//!
//! - **Request (§4).** The IR maps onto a stateless Responses body: the leading system messages
//!   become `instructions`, every other message an `input` item, and `store` is always `false`
//!   (D4, R-Q4); `previous_response_id` is never sent. Inputs whose token count the request does
//!   not show (`input_file`, any `file_id`) are refused (D10). The component acts on exactly the
//!   three `extensions` keys the `OpenAI`-compatible reference reads (R-Q15).
//! - **Response (§5).** `completed`, or `incomplete` for `max_output_tokens` (finish reason
//!   `length`, R-Q2) and `content_filter` (finish reason `content_filter`); every other status or
//!   reason, and every output item this dialect cannot represent, is a protocol error. Refusal
//!   parts are ordinary text (R-Q10).
//! - **Stream (§6).** The component splits its own SSE stream ([`crate::sse_split`]) and enforces
//!   the host's evidence rules unchanged in strength: a closed event list (R-Q9: an unknown
//!   `response.*` type is an error), strictly increasing `sequence_number`, one `response.id`, the
//!   identity of every item and part, exactly one terminal frame, and a clean end of stream with
//!   no terminal is `transport_truncated`. Failure frames — `response.failed`, `error`, and any
//!   frame carrying a top-level `error` object — are recognised first and end the stream with
//!   `StreamEvent::Error` (§6.4). `block_index` is a first-seen counter (R-Q11).
//! - **Usage (§7).** Strict: `input_tokens`, `output_tokens` and `total_tokens` are required and
//!   must add up; the cached and cache-write buckets are subsets of the input and reasoning a
//!   subset of the output; any non-zero `tool_usage` counter is refused (R-Q3).

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use south_provider_api::{ComponentMetadataV1, PROVIDER_WORLD};
use token_station_protocol::{
    Auth, ChatRequest, ChatResponse, Choice, Content, ContentPart, ErrorCode, ErrorEnvelope,
    Extensions, FinishReason, HttpMethod, HttpRequestDescriptor, HttpResponseParts, Message,
    ProviderApi, ProviderConfig, ResponseFormat, Role, SafeHeaders, StreamEvent, ToolCall,
    ToolChoice, Usage,
};

use crate::component::{ComponentResultV1, ProviderComponentV1, StreamParserV1};
use crate::reference::OpenAiCompatibleReferenceV1;
use crate::reference_anthropic::{message_of, provider_protocol_error};
use crate::responses_vocabulary::{event, extension, incomplete_reason, item, part};
use crate::sse_split::{SseFrameV1, SseSplitErrorV1, SseSplitterV1};

/// The family this package serves in v1.
pub const FAMILY: &str = "openai-responses";

/// The package's name.
pub const NAME: &str = "provider-openai-responses";

/// The package's version.
pub const VERSION: &str = "1.0.0";

/// The reference component. Stateless; each stream gets its own parser.
#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAiResponsesReferenceV1;

fn internal(detail: impl std::fmt::Display) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string())
}

fn capability(detail: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Capability, 400, detail)
}

fn protocol(message: &'static str) -> ErrorEnvelope {
    provider_protocol_error(message)
}

// -- request -----------------------------------------------------------------

/// The refusal for a part this wire cannot spell (renderer refusal, 0.15.0).
fn unmappable(value: &Value) -> ErrorEnvelope {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("<untyped>");
    capability(format!(
        "content block `{kind}` has no OpenAI Responses rendering; route the request to a \
         provider that speaks its wire"
    ))
}

fn unmappable_ir(kind: &str, role: &str) -> ErrorEnvelope {
    capability(format!(
        "a `{kind}` part in a {role} message has no OpenAI Responses rendering; route the request \
         to a provider that speaks its wire"
    ))
}

/// D10 (§4.2): references whose token count the request does not show.
fn refused_reference(value: &Value) -> Option<ErrorEnvelope> {
    if value.get("type").and_then(Value::as_str) == Some(part::INPUT_FILE) {
        return Some(capability(
            "an `input_file` part names a document the upstream fetches or stores, whose size \
             the request does not show; this dialect refuses it",
        ));
    }
    let names_file = |value: &Value| value.get("file_id").is_some_and(|id| !id.is_null());
    if names_file(value) || value.get("image_url").is_some_and(names_file) {
        return Some(capability(
            "a `file_id` names a file stored in the upstream account, whose size the request \
             does not show; this dialect refuses it",
        ));
    }
    None
}

/// The text of a message whose parts must all be text, concatenated.
fn text_only(content: Option<&Content>, role: &str) -> ComponentResultV1<String> {
    match content {
        None => Ok(String::new()),
        Some(Content::Text(text)) => Ok(text.clone()),
        Some(Content::Parts(parts)) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    ContentPart::Text { text: piece } => text.push_str(piece),
                    ContentPart::ImageUrl { .. } => return Err(unmappable_ir("image_url", role)),
                    ContentPart::Thinking { .. } => return Err(unmappable_ir("thinking", role)),
                    ContentPart::RedactedThinking { .. } => {
                        return Err(unmappable_ir("redacted_thinking", role));
                    }
                    ContentPart::Unknown(value) => {
                        return Err(refused_reference(value).unwrap_or_else(|| unmappable(value)));
                    }
                }
            }
            Ok(text)
        }
    }
}

fn input_text(text: &str) -> Value {
    json!({"type": part::INPUT_TEXT, "text": text})
}

fn user_content(content: Option<&Content>) -> ComponentResultV1<Vec<Value>> {
    match content {
        None => Ok(Vec::new()),
        Some(Content::Text(text)) => Ok(vec![input_text(text)]),
        Some(Content::Parts(parts)) => parts
            .iter()
            .map(|content_part| match content_part {
                ContentPart::Text { text } => Ok(input_text(text)),
                ContentPart::ImageUrl { image_url } => {
                    let mut image = json!({"type": part::INPUT_IMAGE, "image_url": image_url.url});
                    if let Some(detail) = &image_url.detail {
                        image["detail"] = json!(detail);
                    }
                    Ok(image)
                }
                ContentPart::Unknown(value) => {
                    if let Some(refusal) = refused_reference(value) {
                        return Err(refusal);
                    }
                    if value.get("type").and_then(Value::as_str) == Some(part::INPUT_AUDIO) {
                        // Responses vocabulary the IR does not model: forwarded unchanged.
                        return Ok(value.clone());
                    }
                    Err(unmappable(value))
                }
                ContentPart::Thinking { .. } => Err(unmappable_ir("thinking", "user")),
                ContentPart::RedactedThinking { .. } => {
                    Err(unmappable_ir("redacted_thinking", "user"))
                }
            })
            .collect(),
    }
}

/// An assistant turn's text, with thinking left out (§4.5: the wire takes reasoning only as items
/// the upstream itself issued).
fn assistant_text(content: Option<&Content>) -> ComponentResultV1<String> {
    match content {
        None => Ok(String::new()),
        Some(Content::Text(text)) => Ok(text.clone()),
        Some(Content::Parts(parts)) => {
            let mut text = String::new();
            for content_part in parts {
                match content_part {
                    ContentPart::Text { text: piece } => text.push_str(piece),
                    ContentPart::Thinking { .. } | ContentPart::RedactedThinking { .. } => {}
                    ContentPart::ImageUrl { .. } => {
                        return Err(unmappable_ir("image_url", "assistant"));
                    }
                    ContentPart::Unknown(value) => {
                        return Err(refused_reference(value).unwrap_or_else(|| unmappable(value)));
                    }
                }
            }
            Ok(text)
        }
    }
}

fn input_items(messages: &[Message]) -> ComponentResultV1<Vec<Value>> {
    let mut items = Vec::new();
    for message in messages {
        match message.role {
            Role::System => {
                let text = text_only(message.content.as_ref(), "system")?;
                items.push(json!({"role": "system", "content": [input_text(&text)]}));
            }
            Role::User => {
                items.push(
                    json!({"role": "user", "content": user_content(message.content.as_ref())?}),
                );
            }
            Role::Assistant => {
                let text = assistant_text(message.content.as_ref())?;
                if !text.is_empty() {
                    items.push(json!({
                        "role": "assistant",
                        "content": [{"type": part::OUTPUT_TEXT, "text": text}],
                    }));
                }
                for call in &message.tool_calls {
                    items.push(json!({
                        "type": item::FUNCTION_CALL,
                        "call_id": call.id,
                        "name": call.name,
                        "arguments": call.arguments,
                    }));
                }
            }
            Role::Tool => {
                let Some(call_id) = &message.tool_call_id else {
                    return Err(capability(
                        "a tool result without a tool_call_id cannot be matched to its call on \
                         the OpenAI Responses wire",
                    ));
                };
                let output = text_only(message.content.as_ref(), "tool")?;
                items.push(json!({
                    "type": item::FUNCTION_CALL_OUTPUT,
                    "call_id": call_id,
                    "output": output,
                }));
            }
        }
    }
    Ok(items)
}

fn tool_choice(choice: &ToolChoice) -> ComponentResultV1<Value> {
    Ok(match choice {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::None => json!("none"),
        ToolChoice::Required => json!("required"),
        ToolChoice::Other(value) => {
            // The Chat form the north codec emits, and Anthropic's named-tool form.
            let name = match value.get("type").and_then(Value::as_str) {
                Some("function") => value.pointer("/function/name").and_then(Value::as_str),
                Some("tool") => value.get("name").and_then(Value::as_str),
                _ => None,
            };
            let Some(name) = name else {
                return Err(capability(
                    "this tool_choice has no OpenAI Responses rendering; the wire names a \
                     function, or takes auto, none or required",
                ));
            };
            json!({"type": "function", "name": name})
        }
    })
}

fn text_format(format: &ResponseFormat) -> ComponentResultV1<Value> {
    Ok(match format {
        ResponseFormat::Text => json!({"type": "text"}),
        ResponseFormat::JsonObject => json!({"type": "json_object"}),
        ResponseFormat::JsonSchema { json_schema } => {
            let Some(schema) = json_schema.as_object() else {
                return Err(capability("a json_schema response format must be an object"));
            };
            let mut out = Map::new();
            out.insert("type".to_owned(), json!("json_schema"));
            for key in ["name", "schema", "strict", "description"] {
                if let Some(value) = schema.get(key) {
                    out.insert(key.to_owned(), value.clone());
                }
            }
            Value::Object(out)
        }
    })
}

/// The per-model gate the `OpenAI`-compatible reference applies (record §4.1): sent when the model
/// declares `reasoning_effort` or declares no parameter set; an effort that came from an Anthropic
/// thinking translation needs the explicit declaration.
fn reasoning_effort_allowed(request: &ChatRequest, config: &ProviderConfig) -> bool {
    let requires_explicit_capability = request
        .extensions
        .get("anthropic_thinking")
        .and_then(|thinking| thinking.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|kind| matches!(kind, "adaptive" | "enabled"));
    config.models.iter().find(|capability| capability.model == request.model).map_or(
        !requires_explicit_capability,
        |capability| {
            capability.supported_parameters.contains(extension::REASONING_EFFORT)
                || (!requires_explicit_capability && capability.supported_parameters.is_empty())
        },
    )
}

fn body_of(request: &ChatRequest, config: &ProviderConfig) -> ComponentResultV1<Value> {
    let mut body = Map::new();
    body.insert("model".to_owned(), json!(request.model));

    // §4.1: only the leading run of system messages becomes `instructions`.
    let leading = request.messages.iter().take_while(|message| message.role == Role::System);
    let instructions = leading
        .map(|message| text_only(message.content.as_ref(), "system"))
        .collect::<ComponentResultV1<Vec<String>>>()?;
    let rest = &request.messages[instructions.len()..];
    if !instructions.is_empty() {
        body.insert("instructions".to_owned(), json!(instructions.join("\n")));
    }
    body.insert("input".to_owned(), Value::Array(input_items(rest)?));

    if !request.tools.is_empty() {
        let strict = request.extensions.get(extension::TOOL_STRICT);
        let tools = request
            .tools
            .iter()
            .map(|tool| {
                let mut out = Map::new();
                out.insert("type".to_owned(), json!("function"));
                out.insert("name".to_owned(), json!(tool.name));
                if let Some(description) = &tool.description {
                    out.insert("description".to_owned(), json!(description));
                }
                out.insert("parameters".to_owned(), tool.parameters.clone());
                if let Some(flag) =
                    strict.and_then(|strict| strict.get(&tool.name)).and_then(Value::as_bool)
                {
                    out.insert("strict".to_owned(), json!(flag));
                }
                Value::Object(out)
            })
            .collect();
        body.insert("tools".to_owned(), Value::Array(tools));
        if let Some(parallel) =
            request.extensions.get(extension::PARALLEL_TOOL_CALLS).and_then(Value::as_bool)
        {
            body.insert("parallel_tool_calls".to_owned(), json!(parallel));
        }
    }
    if let Some(choice) = &request.tool_choice {
        body.insert("tool_choice".to_owned(), tool_choice(choice)?);
    }
    if let Some(format) = &request.response_format {
        body.insert("text".to_owned(), json!({"format": text_format(format)?}));
    }
    if let Some(effort) =
        request.extensions.get(extension::REASONING_EFFORT).and_then(Value::as_str)
        && reasoning_effort_allowed(request, config)
    {
        body.insert("reasoning".to_owned(), json!({"effort": effort}));
    }
    if let Some(temperature) = request.sampling.temperature {
        body.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(top_p) = request.sampling.top_p {
        body.insert("top_p".to_owned(), json!(top_p));
    }
    if let Some(cap) = request.sampling.max_output_tokens {
        body.insert("max_output_tokens".to_owned(), json!(cap));
    }
    // `sampling.stop` is dropped: the wire has no stop field (record §4.1).
    if request.stream {
        body.insert("stream".to_owned(), json!(true));
    }
    // D4 / R-Q4: always stateless. Nothing in the IR can turn this off.
    body.insert("store".to_owned(), json!(false));
    Ok(Value::Object(body))
}

// -- usage -------------------------------------------------------------------

fn required_count(usage: &Value, key: &str, missing: &'static str) -> ComponentResultV1<u64> {
    usage.get(key).and_then(Value::as_u64).ok_or_else(|| protocol(missing))
}

fn optional_count(details: Option<&Value>, key: &str) -> ComponentResultV1<u64> {
    match details.and_then(|details| details.get(key)) {
        None | Some(Value::Null) => Ok(0),
        Some(value) => {
            value.as_u64().ok_or_else(|| protocol("the upstream usage has an invalid token count"))
        }
    }
}

fn details<'a>(usage: &'a Value, key: &str) -> ComponentResultV1<Option<&'a Value>> {
    match usage.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(details @ Value::Object(_)) => Ok(Some(details)),
        Some(_) => {
            Err(protocol("the upstream usage has a *_tokens_details value that is not an object"))
        }
    }
}

/// §7.1: the terminal `response.usage`, strictly. A missing report is an error, never a zero.
fn usage_of(usage: Option<&Value>) -> ComponentResultV1<Usage> {
    let Some(usage) = usage.filter(|usage| usage.is_object()) else {
        return Err(protocol("the upstream response carries no usage object"));
    };
    let input = required_count(
        usage,
        "input_tokens",
        "the upstream usage lacks a valid input_tokens count",
    )?;
    let output = required_count(
        usage,
        "output_tokens",
        "the upstream usage lacks a valid output_tokens count",
    )?;
    let total = required_count(
        usage,
        "total_tokens",
        "the upstream usage lacks a valid total_tokens count",
    )?;
    if input.checked_add(output) != Some(total) {
        return Err(protocol(
            "the upstream total_tokens does not equal input_tokens plus output_tokens",
        ));
    }
    let input_details = details(usage, "input_tokens_details")?;
    let output_details = details(usage, "output_tokens_details")?;
    let cached = optional_count(input_details, "cached_tokens")?;
    let cache_write = optional_count(input_details, "cache_write_tokens")?;
    if cached.checked_add(cache_write).is_none_or(|cache| cache > input) {
        return Err(protocol("the upstream cached and cache-write counts exceed input_tokens"));
    }
    let reasoning = optional_count(output_details, "reasoning_tokens")?;
    if reasoning > output {
        return Err(protocol("the upstream reasoning_tokens exceeds output_tokens"));
    }
    Ok(Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cached,
        cache_write_tokens: cache_write,
        reasoning_tokens: reasoning,
        ..Usage::default()
    })
}

/// §7.3, R-Q3: `tool_usage` is absent, null, or zero in every counter. The IR has no meter for a
/// hosted tool, so a non-zero counter — `image_gen` included — is refused, never folded in.
fn check_tool_usage(value: Option<&Value>) -> ComponentResultV1<()> {
    fn zero_only(value: &Value) -> bool {
        match value {
            Value::Null => true,
            Value::Number(number) => number.as_u64() == Some(0),
            Value::Object(map) => map.values().all(zero_only),
            Value::Array(items) => items.iter().all(zero_only),
            Value::Bool(_) | Value::String(_) => false,
        }
    }
    match value {
        None | Some(Value::Null) => Ok(()),
        Some(usage @ Value::Object(_)) if zero_only(usage) => Ok(()),
        Some(Value::Object(_)) => Err(protocol(
            "the upstream reported hosted-tool usage, which this dialect has no meter for",
        )),
        Some(_) => Err(protocol("the upstream tool_usage is neither null nor an object")),
    }
}

// -- response ----------------------------------------------------------------

fn required_str<'a>(
    value: &'a Value,
    key: &str,
    missing: &'static str,
) -> ComponentResultV1<&'a str> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| protocol(missing))
}

fn typed_parts<'a>(
    value: &'a Value,
    key: &str,
    kind: &str,
    required: bool,
) -> ComponentResultV1<Vec<&'a str>> {
    let parts = match value.get(key) {
        None if !required => return Ok(Vec::new()),
        Some(Value::Array(parts)) => parts,
        _ => {
            return Err(protocol(
                "the upstream reasoning item has a summary or content that is not an array",
            ));
        }
    };
    parts
        .iter()
        .map(|entry| {
            if entry.get("type").and_then(Value::as_str) != Some(kind) {
                return Err(protocol(
                    "the upstream reasoning item carries a part this dialect cannot represent",
                ));
            }
            required_str(entry, "text", "the upstream reasoning part has no text")
        })
        .collect()
}

/// One output item, read and validated (§5, §6.2 rule 6).
enum OutputItem<'a> {
    /// The concatenated `output_text` and `refusal` text of a message (R-Q10).
    Message(String),
    FunctionCall {
        call_id: &'a str,
        name: &'a str,
        arguments: Option<&'a str>,
    },
    /// Summary parts, then content parts.
    Reasoning(Vec<&'a str>),
}

fn output_item(value: &Value) -> ComponentResultV1<OutputItem<'_>> {
    match value.get("type").and_then(Value::as_str) {
        Some(item::MESSAGE) => {
            let Some(parts) = value.get("content").and_then(Value::as_array) else {
                return Err(protocol("the upstream message item has no content array"));
            };
            let mut text = String::new();
            for entry in parts {
                match entry.get("type").and_then(Value::as_str) {
                    Some(part::OUTPUT_TEXT) => text.push_str(required_str(
                        entry,
                        "text",
                        "the upstream output_text part has no text",
                    )?),
                    Some(part::REFUSAL) => text.push_str(required_str(
                        entry,
                        "refusal",
                        "the upstream refusal part has no refusal text",
                    )?),
                    _ => {
                        return Err(protocol(
                            "the upstream message carries a content part this dialect cannot \
                             represent",
                        ));
                    }
                }
            }
            Ok(OutputItem::Message(text))
        }
        Some(item::FUNCTION_CALL) => Ok(OutputItem::FunctionCall {
            call_id: required_str(value, "call_id", "the upstream function call has no call_id")?,
            name: required_str(value, "name", "the upstream function call has no name")?,
            arguments: match value.get("arguments") {
                None => None,
                Some(Value::String(arguments)) => Some(arguments),
                Some(_) => {
                    return Err(protocol("the upstream function call arguments are not a string"));
                }
            },
        }),
        Some(item::REASONING) => {
            let mut texts = typed_parts(value, "summary", part::SUMMARY_TEXT, true)?;
            texts.extend(typed_parts(value, "content", part::REASONING_TEXT, false)?);
            Ok(OutputItem::Reasoning(texts))
        }
        Some(_) => Err(protocol(
            "the upstream returned an output item this dialect cannot represent; dropping it \
             would hide content or cost",
        )),
        None => Err(protocol("the upstream output item has no type")),
    }
}

/// The finish reason a terminal `response` object states, or why it is not a success (§5, §6.3).
fn terminal_status(raw: &Value) -> ComponentResultV1<FinishReason> {
    match raw.get("status").and_then(Value::as_str) {
        Some("completed") => Ok(FinishReason::Stop),
        Some("incomplete") => {
            match raw.pointer("/incomplete_details/reason").and_then(Value::as_str) {
                Some(incomplete_reason::MAX_OUTPUT_TOKENS) => Ok(FinishReason::Length),
                Some(incomplete_reason::CONTENT_FILTER) => Ok(FinishReason::ContentFilter),
                _ => Err(protocol(
                    "the upstream response is incomplete for a reason that does not settle",
                )),
            }
        }
        _ => Err(protocol("the upstream response did not complete")),
    }
}

/// A terminal `response` object as an IR response (§5).
fn response_of(raw: &Value) -> ComponentResultV1<ChatResponse> {
    if raw.get("object").and_then(Value::as_str) != Some("response") {
        return Err(protocol("the upstream 2xx body is not a response object"));
    }
    if raw.get("error").is_some_and(|error| !error.is_null()) {
        return Err(protocol("the upstream embedded an error in a successful response"));
    }
    let status = terminal_status(raw)?;
    check_tool_usage(raw.get("tool_usage"))?;
    let Some(output) = raw.get("output").and_then(Value::as_array) else {
        return Err(protocol("the upstream response has no output array"));
    };
    let mut thinking = Vec::new();
    let mut text: Option<String> = None;
    let mut tool_calls = Vec::new();
    for entry in output {
        match output_item(entry)? {
            OutputItem::Message(piece) => text.get_or_insert_with(String::new).push_str(&piece),
            OutputItem::FunctionCall { call_id, name, arguments } => {
                let Some(arguments) = arguments else {
                    return Err(protocol("the upstream function call has no arguments"));
                };
                tool_calls.push(ToolCall {
                    id: call_id.to_owned(),
                    name: name.to_owned(),
                    arguments: arguments.to_owned(),
                });
            }
            OutputItem::Reasoning(texts) => {
                thinking.extend(texts.into_iter().map(|text| ContentPart::Thinking {
                    thinking: text.to_owned(),
                    signature: None,
                }));
            }
        }
    }
    let usage = usage_of(raw.get("usage"))?;
    let content = if thinking.is_empty() {
        text.map(Content::Text)
    } else {
        let mut parts = thinking;
        if let Some(text) = text {
            parts.push(ContentPart::Text { text });
        }
        Some(Content::Parts(parts))
    };
    let finish_reason = if tool_calls.is_empty() { status } else { FinishReason::ToolCalls };
    Ok(ChatResponse {
        id: raw.get("id").and_then(Value::as_str).unwrap_or_default().to_owned(),
        model: raw.get("model").and_then(Value::as_str).unwrap_or_default().to_owned(),
        choices: vec![Choice {
            index: 0,
            message: Message {
                role: Role::Assistant,
                content,
                tool_calls,
                tool_call_id: None,
                name: None,
                extensions: Extensions::new(),
            },
            finish_reason: Some(finish_reason),
            stop_sequence: None,
        }],
        usage,
        extensions: Extensions::new(),
    })
}

// -- failures ----------------------------------------------------------------

/// §6.4: an upstream error `code` through a closed table: the two checks `map-provider-error`
/// makes, then the codes `OpenAI` publishes for a response's `error.code`, then `internal`.
#[must_use]
pub fn failure_code(code: &str) -> ErrorCode {
    let code = code.to_ascii_lowercase();
    if code == "content_policy_violation" {
        return ErrorCode::ContentPolicy;
    }
    if code.contains("context_length") || code.contains("maximum_context") {
        return ErrorCode::ContextLength;
    }
    match code.as_str() {
        "rate_limit_exceeded" => ErrorCode::RateLimit,
        "server_error" => ErrorCode::UpstreamUnavailable,
        "invalid_prompt" | "image_content_policy_violation" => ErrorCode::ContentPolicy,
        "insufficient_quota" => ErrorCode::PaymentRequired,
        "vector_store_timeout" => ErrorCode::Timeout,
        "invalid_image"
        | "invalid_image_format"
        | "invalid_base64_image"
        | "invalid_image_url"
        | "image_too_large"
        | "image_too_small"
        | "image_parse_error"
        | "invalid_image_mode"
        | "image_file_too_large"
        | "unsupported_image_media_type"
        | "empty_image_file"
        | "failed_to_download_image"
        | "image_file_not_found" => ErrorCode::InvalidRequest,
        _ => ErrorCode::Internal,
    }
}

fn failure_envelope(error: &Value) -> ErrorEnvelope {
    let code = failure_code(error.get("code").and_then(Value::as_str).unwrap_or_default());
    let mut envelope = ErrorEnvelope::new(code, 502, message_of(code));
    envelope.provider_message = error
        .get("message")
        .and_then(Value::as_str)
        .filter(|message| message.chars().count() <= 256)
        .map(str::to_owned);
    envelope
}

/// The error object of a failure frame, or `None` for a frame that is not one (§6.4).
fn failure_of(frame: &Value) -> Option<&Value> {
    let top = frame.get("error").filter(|error| error.is_object());
    match frame.get("type").and_then(Value::as_str) {
        Some(event::FAILED) => Some(
            frame
                .pointer("/response/error")
                .filter(|error| error.is_object())
                .or(top)
                .unwrap_or(&Value::Null),
        ),
        // OpenAI's `error` event carries `code` and `message` at its top level; a backend that
        // nests them uses a top-level `error` object.
        Some(event::ERROR) => Some(top.unwrap_or(frame)),
        _ => top,
    }
}

// -- stream ------------------------------------------------------------------

#[derive(Debug)]
struct ItemIdentity {
    output_index: u64,
    kind: String,
    done: bool,
}

#[derive(Debug)]
struct PartIdentity {
    kind: String,
    done: bool,
}

#[derive(Debug)]
struct CallState {
    ordinal: u32,
    call_id: String,
    name: String,
    emitted: String,
    done: bool,
}

/// One Responses stream, mid-parse.
#[derive(Debug, Default)]
struct ResponsesStreamParser {
    splitter: SseSplitterV1,
    received: bool,
    ended: bool,
    last_sequence: Option<u64>,
    response_id: Option<String>,
    items: BTreeMap<String, ItemIdentity>,
    parts: BTreeMap<(String, u64, u64), PartIdentity>,
    calls: BTreeMap<String, CallState>,
    blocks: BTreeMap<(String, bool, u64), u32>,
}

fn index_of(frame: &Value, key: &str, invalid: &'static str) -> ComponentResultV1<u64> {
    frame.get(key).and_then(Value::as_u64).ok_or_else(|| protocol(invalid))
}

fn item_id_of(frame: &Value) -> ComponentResultV1<&str> {
    frame
        .get("item_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| protocol("the upstream stream event has no item_id"))
}

fn text_of<'a>(frame: &'a Value, key: &str) -> ComponentResultV1<&'a str> {
    frame
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| protocol("the upstream stream event lacks its text payload"))
}

impl ResponsesStreamParser {
    /// Rule 4: `response.id` is non-empty and the same in every event that carries it.
    fn bind_response(&mut self, response: Option<&Value>) -> ComponentResultV1<()> {
        let Some(response) = response.filter(|response| response.is_object()) else {
            return Err(protocol("the upstream stream event carries no response object"));
        };
        let Some(id) = response.get("id").and_then(Value::as_str).filter(|id| !id.is_empty())
        else {
            return Err(protocol("the upstream response object has no id"));
        };
        match &self.response_id {
            Some(known) if known != id => {
                Err(protocol("the upstream changed response.id within one stream"))
            }
            Some(_) => Ok(()),
            None => {
                self.response_id = Some(id.to_owned());
                Ok(())
            }
        }
    }

    fn block(&mut self, item_id: &str, content: bool, index: u64) -> u32 {
        let next = u32::try_from(self.blocks.len()).unwrap_or(u32::MAX);
        *self.blocks.entry((item_id.to_owned(), content, index)).or_insert(next)
    }

    /// A call's final arguments: what extends the emitted prefix is emitted once; anything that
    /// contradicts it is an error (§6.1).
    fn settle_arguments(
        call: &mut CallState,
        arguments: &str,
    ) -> ComponentResultV1<Option<StreamEvent>> {
        let Some(rest) = arguments.strip_prefix(call.emitted.as_str()) else {
            return Err(protocol(
                "the upstream's final function call arguments contradict what it streamed",
            ));
        };
        let event = (!rest.is_empty()).then(|| StreamEvent::ToolCallDelta {
            index: call.ordinal,
            id: None,
            name: None,
            arguments_delta: rest.to_owned(),
        });
        arguments.clone_into(&mut call.emitted);
        Ok(event)
    }

    fn output_item_event(
        &mut self,
        frame: &Value,
        done: bool,
    ) -> ComponentResultV1<Vec<StreamEvent>> {
        let output_index =
            index_of(frame, "output_index", "the upstream output item has no output_index")?;
        let Some(raw) = frame.get("item") else {
            return Err(protocol("the upstream output item event carries no item"));
        };
        let parsed = output_item(raw)?;
        let id = raw
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| protocol("the upstream output item has no id"))?;
        let kind = raw.get("type").and_then(Value::as_str).unwrap_or_default();
        if let Some(previous) = self.items.get_mut(id) {
            if !done
                || previous.done
                || previous.output_index != output_index
                || previous.kind != kind
            {
                return Err(protocol("the upstream output item's added and done disagree"));
            }
            previous.done = true;
        } else {
            self.items
                .insert(id.to_owned(), ItemIdentity { output_index, kind: kind.to_owned(), done });
        }
        let OutputItem::FunctionCall { call_id, name, arguments } = parsed else {
            return Ok(Vec::new());
        };
        if done && arguments.is_none() {
            return Err(protocol("the upstream function call has no arguments"));
        }
        let arguments = arguments.unwrap_or_default();
        if let Some(call) = self.calls.get_mut(id) {
            if call.call_id != call_id || call.name != name {
                return Err(protocol("the upstream changed a function call's id or name"));
            }
            let event = Self::settle_arguments(call, arguments)?;
            call.done = true;
            return Ok(event.into_iter().collect());
        }
        let ordinal = u32::try_from(self.calls.len())
            .map_err(|_| protocol("the upstream opened too many function calls"))?;
        self.calls.insert(
            id.to_owned(),
            CallState {
                ordinal,
                call_id: call_id.to_owned(),
                name: name.to_owned(),
                emitted: arguments.to_owned(),
                done,
            },
        );
        Ok(vec![StreamEvent::ToolCallDelta {
            index: ordinal,
            id: Some(call_id.to_owned()),
            name: Some(name.to_owned()),
            arguments_delta: arguments.to_owned(),
        }])
    }

    fn content_part_event(&mut self, frame: &Value, done: bool) -> ComponentResultV1<()> {
        let item_id = item_id_of(frame)?;
        let output_index =
            index_of(frame, "output_index", "the upstream content part has no output_index")?;
        let content_index =
            index_of(frame, "content_index", "the upstream content part has no content_index")?;
        let kind = frame
            .pointer("/part/type")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol("the upstream content part has no type"))?;
        if !matches!(kind, part::OUTPUT_TEXT | part::REFUSAL | part::REASONING_TEXT) {
            return Err(protocol(
                "the upstream content part is of a type this dialect cannot represent",
            ));
        }
        let key = (item_id.to_owned(), output_index, content_index);
        if let Some(previous) = self.parts.get_mut(&key) {
            if !done || previous.done || previous.kind != kind {
                return Err(protocol("the upstream content part's added and done disagree"));
            }
            previous.done = true;
        } else {
            self.parts.insert(key, PartIdentity { kind: kind.to_owned(), done });
        }
        Ok(())
    }

    /// `item_id`, `output_index` and `content_index`: where a content event belongs.
    fn content_location(frame: &Value) -> ComponentResultV1<()> {
        item_id_of(frame)?;
        index_of(frame, "output_index", "the upstream content event has no output_index")?;
        index_of(frame, "content_index", "the upstream content event has no content_index")?;
        Ok(())
    }

    fn arguments_event(
        &mut self,
        frame: &Value,
        done: bool,
    ) -> ComponentResultV1<Vec<StreamEvent>> {
        let item_id = item_id_of(frame)?;
        index_of(frame, "output_index", "the upstream arguments event has no output_index")?;
        let payload = text_of(frame, if done { "arguments" } else { "delta" })?;
        let Some(call) = self.calls.get_mut(item_id) else {
            return Err(protocol("the upstream streamed arguments for a call it never opened"));
        };
        if call.done {
            return Err(protocol("the upstream streamed arguments for a call it already finished"));
        }
        if done {
            match frame.get("name") {
                None | Some(Value::Null) => {}
                Some(Value::String(name)) if *name == call.name => {}
                Some(_) => {
                    return Err(protocol("the upstream changed a function call's id or name"));
                }
            }
            return Ok(Self::settle_arguments(call, payload)?.into_iter().collect());
        }
        if payload.is_empty() {
            return Ok(Vec::new());
        }
        call.emitted.push_str(payload);
        Ok(vec![StreamEvent::ToolCallDelta {
            index: call.ordinal,
            id: None,
            name: None,
            arguments_delta: payload.to_owned(),
        }])
    }

    fn reasoning_event(
        &mut self,
        frame: &Value,
        content: bool,
        done: bool,
    ) -> ComponentResultV1<Vec<StreamEvent>> {
        let item_id = item_id_of(frame)?;
        index_of(frame, "output_index", "the upstream reasoning event has no output_index")?;
        let key = if content { "content_index" } else { "summary_index" };
        let index = index_of(frame, key, "the upstream reasoning event has no part index")?;
        let payload = text_of(frame, if done { "text" } else { "delta" })?;
        if done || payload.is_empty() {
            return Ok(Vec::new());
        }
        let block_index = self.block(item_id, content, index);
        Ok(vec![StreamEvent::ThinkingDelta {
            index: 0,
            block_index,
            thinking_delta: payload.to_owned(),
        }])
    }

    fn terminal(&mut self, frame: &Value, status: &str) -> ComponentResultV1<Vec<StreamEvent>> {
        let response = frame.get("response");
        self.bind_response(response)?;
        let response = response.unwrap_or(&Value::Null);
        if response.get("status").and_then(Value::as_str) != Some(status) {
            return Err(protocol("the upstream terminal event disagrees with its response status"));
        }
        let parsed = response_of(response)?;
        self.ended = true;
        let finish_reason = parsed.choices.first().and_then(|choice| choice.finish_reason.clone());
        Ok(vec![
            StreamEvent::Finish { finish_reason, stop_sequence: None },
            StreamEvent::Usage { usage: parsed.usage },
            StreamEvent::Done { finish_reason: None, stop_sequence: None },
        ])
    }

    /// §6.4: a failure frame, recognised before the evidence rules. Rules 1 and 4 still apply.
    fn failure(&mut self, frame: &Value, error: &Value) -> ComponentResultV1<Vec<StreamEvent>> {
        let response = frame.get("response").filter(|response| response.is_object());
        if response.is_some() {
            self.bind_response(response)?;
        }
        self.ended = true;
        let mut events = Vec::new();
        if frame.get("type").and_then(Value::as_str) == Some(event::FAILED)
            && let Ok(usage) = usage_of(response.and_then(|response| response.get("usage")))
        {
            // Exact evidence of what a failed exchange consumed; a present but invalid report is
            // ignored so the failure keeps its code.
            events.push(StreamEvent::Usage { usage });
        }
        events.push(StreamEvent::Error { error: failure_envelope(error) });
        Ok(events)
    }

    #[allow(clippy::too_many_lines, reason = "one arm per event of the closed list (§6.1)")]
    fn frame(&mut self, sse: &SseFrameV1) -> ComponentResultV1<Vec<StreamEvent>> {
        let frame: Value = serde_json::from_str(&sse.data)
            .map_err(|_| protocol("the upstream sent a stream frame with invalid JSON"))?;
        if !frame.is_object() {
            return Err(protocol("the upstream sent a stream frame that is not a JSON object"));
        }
        let kind = frame.get("type").and_then(Value::as_str);
        // Rule 1: exactly one terminal frame.
        if self.ended {
            return Err(
                if matches!(kind, Some(event::COMPLETED | event::INCOMPLETE | event::FAILED)) {
                    protocol("the upstream sent a second terminal frame")
                } else {
                    protocol("the upstream sent a frame after the terminal frame")
                },
            );
        }
        if let Some(kind) = kind
            && sse.event != "message"
            && sse.event != kind
        {
            return Err(protocol("the upstream's SSE event name disagrees with the frame's type"));
        }
        if let Some(error) = failure_of(&frame) {
            return self.failure(&frame, error);
        }
        // Rule 2 (R-Q9): the closed list.
        let Some(kind) = kind else {
            return Err(protocol("the upstream sent a stream frame without a type"));
        };
        if !event::ACCEPTED.contains(&kind) {
            return Err(protocol(
                "the upstream sent a stream event type this dialect does not know",
            ));
        }
        // Rule 3: `sequence_number` strictly increases.
        let sequence = index_of(
            &frame,
            "sequence_number",
            "the upstream stream event has no sequence_number",
        )?;
        if self.last_sequence.is_some_and(|last| sequence <= last) {
            return Err(protocol("the upstream sequence_number did not increase"));
        }
        self.last_sequence = Some(sequence);

        match kind {
            event::CREATED | event::IN_PROGRESS | event::QUEUED => {
                self.bind_response(frame.get("response"))?;
                Ok(Vec::new())
            }
            event::COMPLETED => self.terminal(&frame, "completed"),
            event::INCOMPLETE => self.terminal(&frame, "incomplete"),
            event::OUTPUT_ITEM_ADDED => self.output_item_event(&frame, false),
            event::OUTPUT_ITEM_DONE => self.output_item_event(&frame, true),
            event::CONTENT_PART_ADDED => {
                self.content_part_event(&frame, false).map(|()| Vec::new())
            }
            event::CONTENT_PART_DONE => self.content_part_event(&frame, true).map(|()| Vec::new()),
            event::OUTPUT_TEXT_DELTA | event::REFUSAL_DELTA => {
                Self::content_location(&frame)?;
                let delta = text_of(&frame, "delta")?;
                Ok(if delta.is_empty() {
                    Vec::new()
                } else {
                    vec![StreamEvent::Delta { index: 0, content: delta.to_owned() }]
                })
            }
            event::OUTPUT_TEXT_DONE | event::REFUSAL_DONE => {
                Self::content_location(&frame)?;
                text_of(&frame, if kind == event::OUTPUT_TEXT_DONE { "text" } else { "refusal" })?;
                Ok(Vec::new())
            }
            event::OUTPUT_TEXT_ANNOTATION_ADDED => {
                Self::content_location(&frame)?;
                index_of(&frame, "annotation_index", "the upstream annotation has no index")?;
                if !frame.get("annotation").is_some_and(Value::is_object) {
                    return Err(protocol("the upstream annotation is not an object"));
                }
                Ok(Vec::new())
            }
            event::FUNCTION_CALL_ARGUMENTS_DELTA => self.arguments_event(&frame, false),
            event::FUNCTION_CALL_ARGUMENTS_DONE => self.arguments_event(&frame, true),
            event::REASONING_SUMMARY_PART_ADDED | event::REASONING_SUMMARY_PART_DONE => {
                item_id_of(&frame)?;
                index_of(
                    &frame,
                    "output_index",
                    "the upstream reasoning event has no output_index",
                )?;
                index_of(
                    &frame,
                    "summary_index",
                    "the upstream reasoning event has no part index",
                )?;
                if frame.pointer("/part/type").and_then(Value::as_str) != Some(part::SUMMARY_TEXT) {
                    return Err(protocol(
                        "the upstream reasoning summary part is not summary_text",
                    ));
                }
                Ok(Vec::new())
            }
            event::REASONING_SUMMARY_TEXT_DELTA => self.reasoning_event(&frame, false, false),
            event::REASONING_SUMMARY_TEXT_DONE => self.reasoning_event(&frame, false, true),
            event::REASONING_TEXT_DELTA => self.reasoning_event(&frame, true, false),
            event::REASONING_TEXT_DONE => self.reasoning_event(&frame, true, true),
            // `response.failed` is always a failure frame, recognised above.
            _ => Err(protocol("the upstream sent a stream event type this dialect does not know")),
        }
    }

    fn split_error(error: SseSplitErrorV1) -> ErrorEnvelope {
        match error {
            SseSplitErrorV1::NotUtf8 => {
                protocol("the upstream sent a stream line that is not UTF-8")
            }
            SseSplitErrorV1::TooLarge => {
                protocol("the upstream sent a stream frame over the size bound")
            }
        }
    }
}

impl StreamParserV1 for ResponsesStreamParser {
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>> {
        if chunk.is_empty() {
            // The runtime spells a clean EOF as an empty fragment. Before any byte it is tolerated
            // (gate ② feeds one at split 0); after the stream began, a stream that never reached
            // a terminal frame is truncated (§6.2 rule 8), and no `Done` is emitted.
            if !self.received {
                return Ok(Vec::new());
            }
            let mut events = Vec::new();
            if let Some(frame) = self.splitter.finish().map_err(Self::split_error)? {
                events.extend(self.frame(&frame)?);
            }
            if self.ended {
                return Ok(events);
            }
            return Err(ErrorEnvelope::new(
                ErrorCode::TransportTruncated,
                502,
                message_of(ErrorCode::TransportTruncated),
            ));
        }
        self.received = true;
        let frames = self.splitter.push(chunk).map_err(Self::split_error)?;
        let mut events = Vec::new();
        for frame in &frames {
            events.extend(self.frame(frame)?);
        }
        Ok(events)
    }
}

impl ProviderComponentV1 for OpenAiResponsesReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: NAME.to_owned(),
            version: VERSION.to_owned(),
            api_version: PROVIDER_WORLD.to_owned(),
        }
    }

    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<token_station_protocol::ModelCapability>> {
        // No network, so the upstream's own catalog is unreachable. What its operator declared is
        // all there is.
        Ok(config.models.clone())
    }

    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        if config.provider != FAMILY {
            return Err(capability(format!("unsupported provider dialect `{}`", config.provider)));
        }
        // §4.5: the Claude family's replay carrier means nothing to this upstream.
        if request.messages.iter().any(|message| {
            message.extensions.get("reasoning_replay_protocol_family").and_then(Value::as_str)
                == Some("claude-signed-thinking")
        }) {
            return Err(capability(
                "the OpenAI Responses dialect cannot consume Claude reasoning replay",
            ));
        }
        let body = body_of(request, config)?;
        let mut descriptor = HttpRequestDescriptor::new(
            HttpMethod::Post,
            config.base_url.resolve(ProviderApi::Responses),
        );
        descriptor.headers =
            SafeHeaders::try_new([("content-type", "application/json")]).map_err(internal)?;
        descriptor.body = Some(body);
        descriptor.auth = config.auth.clone().map(Auth::bearer);
        Ok(descriptor)
    }

    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        let raw: Value = serde_json::from_str(&parts.body)
            .map_err(|_| protocol("the upstream returned invalid JSON in a 2xx response"))?;
        response_of(&raw)
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        // §5: the OpenAI-compatible reference's table, unchanged.
        OpenAiCompatibleReferenceV1.map_provider_error(parts)
    }

    fn stream_parser(&self) -> Box<dyn StreamParserV1> {
        Box::new(ResponsesStreamParser::default())
    }
}
