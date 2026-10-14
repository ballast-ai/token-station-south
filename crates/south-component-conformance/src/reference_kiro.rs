//! The native reference implementation of the Kiro provider component (`provider-kiro`, family
//! `kiro`).
//!
//! Design record: `docs/design/2026-09-30-kiro-provider-component.md` (accepted 2026-10-13).
//!
//! The upstream is the `generateAssistantResponse` action of the `CodeWhisperer` streaming service,
//! reached with a Kiro sign-in token. The host's native arm translates in three hops (IR →
//! Anthropic Messages → `conversationState`, and back); this component does one hop each way and
//! reproduces the native body (record D2, D11):
//!
//! - **Request.** `POST {base_url}/generateAssistantResponse`, `Auth::Bearer` on the minted slot,
//!   the client-identification headers the native arm sends (`x-amz-target`, `amz-sdk-request`,
//!   `x-amzn-kiro-agent-mode`, `accept`; the `user-agent` value is the manifest's declaration),
//!   and no `content-type` (K-Q10). The body is one `history` entry per IR message but the last,
//!   which becomes `currentMessage`; system text is prepended to the first user entry;
//!   `conversationId` is the host's attempt id (`host_values.attempt_id`, boundary §13.8);
//!   `profileArn` is the credential's exported attribute when it has one.
//! - **Stream.** The package declares `stream_framing: aws-eventstream`, so each message arrives as
//!   the canonical re-encoding (`event: <name>` / `data: <json>`). Text and tool-call fragments
//!   become IR events; `meteringEvent` and `contextUsageEvent` carry nothing the IR has a field
//!   for. The wire has no terminal event: the end of the body completes the answer, a tool call
//!   without its `stop` included, as natively, unless no frame was parsed (record §5.3, §18).
//! - **Response.** The family declares `request_facts.stream: "none"`, so a non-streaming answer
//!   reaches `parse-response` as the same canonical re-encoding, whole (the buffered path,
//!   boundary §5.2), and is folded by the same parser.
//! - **Usage.** `usage_evidence: absent`: the upstream reports no tokens, the component never emits
//!   `Usage`, and its response usage is all zero (record §6).
//! - **Errors.** The AWS exception name when one is present, then the status (record §9).

use serde_json::{Map, Value, json};
use south_provider_api::{ComponentMetadataV1, HOST_VALUE_ATTEMPT_ID, PROVIDER_WORLD};
use token_station_protocol::{
    Auth, ChatRequest, ChatResponse, Choice, Content, ContentPart, ErrorCode, ErrorEnvelope,
    FinishReason, HttpMethod, HttpRequestDescriptor, HttpResponseParts, Message, ProviderConfig,
    Role, SafeHeaders, StreamEvent, ToolCall, ToolChoice, Usage,
};

use crate::component::{ComponentResultV1, ProviderComponentV1, StreamParserV1};
use crate::reference_anthropic::{
    frame_fields, message_of, provider_protocol_error, sse_frame_boundary,
};
use crate::reference_bedrock_converse::exception_name;

/// The package this reference implements.
pub const NAME: &str = "provider-kiro";

/// The package version.
pub const VERSION: &str = "1.0.4";

/// The family this package serves.
pub const FAMILY: &str = "kiro";

/// The credential attribute the recipes export; the component writes it to `profileArn`.
pub const PROFILE_ARN_ATTRIBUTE: &str = "profile_arn";

/// The path of the streaming action, off the provider row's `base_url`.
const ACTION_PATH: &str = "/generateAssistantResponse";

/// `x-amz-target` of the streaming action.
const AMZ_TARGET: &str = "AmazonCodeWhispererStreamingService.GenerateAssistantResponse";

/// `amz-sdk-request`, as the native arm sends it.
const AMZ_SDK_REQUEST: &str = "attempt=1; max=1";

/// `x-amzn-kiro-agent-mode`, as the native arm sends it.
const AGENT_MODE: &str = "vibe";

/// The media type the upstream answers in, streaming or not.
const EVENTSTREAM_MEDIA_TYPE: &str = "application/vnd.amazon.eventstream";

/// The `origin` of every user entry: the editor surface.
const ORIGIN: &str = "AI_EDITOR";

/// The `chatTriggerType` of every request.
const CHAT_TRIGGER_TYPE: &str = "MANUAL";

/// The reference component. Stateless; each stream gets its own parser.
#[derive(Debug, Default, Clone, Copy)]
pub struct KiroReferenceV1;

fn internal(detail: impl std::fmt::Display) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string())
}

fn capability(detail: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Capability, 400, detail)
}

fn invalid_request(detail: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, detail)
}

// -- request -----------------------------------------------------------------

/// Whether `value` is a UUID in the host-value form: lowercase hexadecimal, hyphenated 8-4-4-4-12.
fn is_attempt_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(byte),
        })
}

/// The host's attempt id, which becomes `conversationId` (record §4.5, K-Q11 provisional). The
/// host fills it on every call to a package that declares it; its absence is a host error, never
/// a request sent without one.
fn attempt_id(request: &ChatRequest) -> ComponentResultV1<&str> {
    match request.host_values.get(HOST_VALUE_ATTEMPT_ID) {
        Some(value) if is_attempt_id(value) => Ok(value),
        Some(_) => Err(internal(
            "host value attempt_id is not a lowercase hyphenated UUID; the host must mint one per \
             upstream attempt",
        )),
        None => Err(internal(
            "host value attempt_id is absent; this package declares it and the host must fill it \
             on every call",
        )),
    }
}

/// The refusal of an image, wherever the native arm refuses one (KR1).
fn image_refused() -> ErrorEnvelope {
    capability("this upstream takes text and tools only; image input is refused")
}

/// What the native arm reads from a user or assistant message.
///
/// The native first hop (the Anthropic reference) turns each part into a Messages block and sends a
/// part this crate does not model verbatim; the second hop (server `translate_kiro.rs`) then reads
/// the blocks by their `type`: `text` blocks are the text, concatenated with no separator; `tool_use`
/// blocks are an assistant entry's `toolUses`; `tool_result` blocks are a user entry's
/// `toolResults`; an `image` block refuses the request; any other block is dropped. Reasoning
/// parts have no slot on this wire and are dropped (KR12). A part this crate does not model is read
/// the same way, so the component sends what the native arm sends (KR2, aligned 2026-10-14).
#[derive(Default)]
struct Read {
    text: String,
    tool_uses: Vec<Value>,
    tool_results: Vec<Value>,
}

fn read_parts(content: Option<&Content>) -> ComponentResultV1<Read> {
    let mut read = Read::default();
    match content {
        None => {}
        Some(Content::Text(text)) => read.text.push_str(text),
        Some(Content::Parts(parts)) => {
            for part in parts {
                match part {
                    ContentPart::Text { text, .. } => read.text.push_str(text),
                    ContentPart::Thinking { .. } | ContentPart::RedactedThinking { .. } => {}
                    ContentPart::ImageUrl { .. } => return Err(image_refused()),
                    ContentPart::Unknown(block) => read_unmodelled(block, &mut read)?,
                }
            }
        }
    }
    Ok(read)
}

/// A part this crate does not model, read as the native second hop reads the block the first hop
/// sent verbatim: by its `type`, each field defaulted as the native arm defaults it.
fn read_unmodelled(block: &Value, read: &mut Read) -> ComponentResultV1<()> {
    let text_of = |key: &str| block.get(key).and_then(Value::as_str);
    match text_of("type") {
        Some("text") => read.text.push_str(text_of("text").unwrap_or_default()),
        Some("image") => return Err(image_refused()),
        Some("tool_use") => read.tool_uses.push(json!({
            "toolUseId": text_of("id").unwrap_or_default(),
            "name": text_of("name").unwrap_or_default(),
            "input": block.get("input").cloned().unwrap_or_else(|| json!({})),
        })),
        Some("tool_result") => {
            let status = if block.get("is_error").and_then(Value::as_bool) == Some(true) {
                "error"
            } else {
                "success"
            };
            read.tool_results.push(json!({
                "toolUseId": text_of("tool_use_id").unwrap_or_default(),
                "content": [{"text": unmodelled_result_text(block.get("content"))}],
                "status": status,
            }));
        }
        _ => {}
    }
    Ok(())
}

/// The text of an unmodelled `tool_result` block's `content`: a string as it is, or the `text` of
/// every element of an array, concatenated, as the native second hop reads it.
fn unmodelled_result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .concat(),
        _ => String::new(),
    }
}

/// The text of a tool message, as the native first hop's `tool_result` block carries it: its text
/// parts concatenated. Every other part is dropped there, a part this crate does not model
/// included; an image is refused (KR1).
fn tool_text(content: Option<&Content>) -> ComponentResultV1<String> {
    match content {
        None => Ok(String::new()),
        Some(Content::Text(text)) => Ok(text.clone()),
        Some(Content::Parts(parts)) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    ContentPart::Text { text: part, .. } => text.push_str(part),
                    ContentPart::ImageUrl { .. } => return Err(image_refused()),
                    ContentPart::Thinking { .. }
                    | ContentPart::RedactedThinking { .. }
                    | ContentPart::Unknown(_) => {}
                }
            }
            Ok(text)
        }
    }
}

/// The system text: the non-empty text of every system message, joined by `\n`, in order. `None`
/// when there is none. Only text survives, as in the native first hop; an image is refused (KR1).
fn system_text(request: &ChatRequest) -> ComponentResultV1<Option<String>> {
    let mut pieces: Vec<String> = Vec::new();
    for message in request.messages.iter().filter(|message| message.role == Role::System) {
        match message.content.as_ref() {
            Some(Content::Parts(parts)) => {
                if parts.iter().any(|part| matches!(part, ContentPart::ImageUrl { .. })) {
                    return Err(image_refused());
                }
                pieces.extend(parts.iter().filter_map(|part| match part {
                    ContentPart::Text { text, .. } if !text.is_empty() => Some(text.clone()),
                    _ => None,
                }));
            }
            Some(Content::Text(text)) if !text.is_empty() => pieces.push(text.clone()),
            _ => {}
        }
    }
    Ok((!pieces.is_empty()).then(|| pieces.join("\n")))
}

/// One user-side entry: a user turn or a tool result.
fn user_input(content: &str, tool_results: Vec<Value>) -> Map<String, Value> {
    let mut entry = Map::new();
    entry.insert("content".to_owned(), json!(content));
    entry.insert("origin".to_owned(), json!(ORIGIN));
    if !tool_results.is_empty() {
        entry.insert("userInputMessageContext".to_owned(), json!({"toolResults": tool_results}));
    }
    entry
}

/// The `toolResults` entry of a tool message. `status` is always `success`: the IR has no field
/// for a failed tool result (K-Q14, KR11).
fn tool_result(message: &Message) -> ComponentResultV1<Value> {
    let Some(id) = message.tool_call_id.as_deref().filter(|id| !id.is_empty()) else {
        return Err(capability(
            "a tool result has no tool_call_id; this upstream needs one to pair it with the call",
        ));
    };
    Ok(json!({
        "toolUseId": id,
        "content": [{"text": tool_text(message.content.as_ref())?}],
        "status": "success",
    }))
}

/// The `toolUses` entry of one assistant tool call.
fn tool_use(call: &ToolCall) -> ComponentResultV1<Value> {
    if call.id.is_empty() {
        return Err(capability(
            "an assistant tool call has no id; this upstream needs one to pair the result with \
             the call",
        ));
    }
    if call.name.is_empty() {
        return Err(capability(
            "an assistant tool call has no name; this upstream needs one to name the tool",
        ));
    }
    Ok(json!({"toolUseId": call.id, "name": call.name, "input": tool_input(&call.arguments)}))
}

/// The `input` of a tool use, exactly as the native arm sends it (the Anthropic first hop): the
/// arguments parsed as JSON, any JSON value kept as it is, and text that does not parse, empty or
/// whitespace-only text included, sent as that text in a JSON string.
fn tool_input(arguments: &str) -> Value {
    serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!(arguments))
}

/// One wire entry for a non-system message, before the system text is placed.
enum Entry {
    User(Map<String, Value>),
    Assistant(Value),
}

fn entry_of(message: &Message) -> ComponentResultV1<Entry> {
    match message.role {
        Role::User => {
            let read = read_parts(message.content.as_ref())?;
            Ok(Entry::User(user_input(&read.text, read.tool_results)))
        }
        Role::Tool => Ok(Entry::User(user_input("", vec![tool_result(message)?]))),
        Role::Assistant => {
            let read = read_parts(message.content.as_ref())?;
            let mut entry = Map::new();
            entry.insert("content".to_owned(), json!(read.text));
            // Unmodelled `tool_use` blocks sit among the parts, before the calls the first hop
            // appends, so their uses come first.
            let mut uses = read.tool_uses;
            for call in &message.tool_calls {
                uses.push(tool_use(call)?);
            }
            if !uses.is_empty() {
                entry.insert("toolUses".to_owned(), Value::Array(uses));
            }
            Ok(Entry::Assistant(Value::Object(entry)))
        }
        Role::System => Err(internal("a system message reached the entry mapping")),
    }
}

/// The current message: the last non-system message, whatever its role, read as the native second
/// hop reads it — its text and its tool results; an assistant turn's tool calls are checked as the
/// first hop checks them and then have no slot (KR5, aligned 2026-10-14: the native arm sends a
/// final assistant turn as the current user message).
fn current_of(message: &Message) -> ComponentResultV1<Map<String, Value>> {
    match message.role {
        Role::User | Role::Tool => match entry_of(message)? {
            Entry::User(entry) => Ok(entry),
            Entry::Assistant(_) => Err(internal("a user-side message became an assistant entry")),
        },
        Role::Assistant => {
            let read = read_parts(message.content.as_ref())?;
            for call in &message.tool_calls {
                tool_use(call)?;
            }
            Ok(user_input(&read.text, read.tool_results))
        }
        Role::System => Err(internal("a system message reached the entry mapping")),
    }
}

/// Prepends the system text to `content`, or replaces it when it is empty (the native rule).
fn with_system(system: &str, content: &Value) -> Value {
    match content.as_str() {
        Some(content) if !content.is_empty() => json!(format!("{system}\n\n{content}")),
        _ => json!(system),
    }
}

/// The tool declarations the current message carries, unless the choice withholds them.
///
/// `none` withholds them, as the native first hop does. `required` and a named tool have no slot on
/// this wire: the native first hop writes them into its intermediate body and the second hop never
/// reads them, so the declarations are sent as for `auto` (KR4, aligned 2026-10-14). A model row
/// declaring `anthropic.tool_choice.auto_only` is refused by the host before any component, for
/// either arm.
fn tools_of(request: &ChatRequest) -> Option<Value> {
    if request.tool_choice == Some(ToolChoice::None) || request.tools.is_empty() {
        return None;
    }
    Some(Value::Array(
        request
            .tools
            .iter()
            .map(|tool| {
                let mut spec = Map::new();
                spec.insert("name".to_owned(), json!(tool.name));
                if let Some(description) = &tool.description {
                    spec.insert("description".to_owned(), json!(description));
                }
                spec.insert("inputSchema".to_owned(), json!({"json": tool.parameters}));
                json!({"toolSpecification": spec})
            })
            .collect(),
    ))
}

/// The `generateAssistantResponse` body. `response_format` has no slot on this wire and is not
/// read: the native first hop never writes it, so a JSON format reaches the upstream as nothing
/// (KR3, aligned 2026-10-14).
fn body_of(request: &ChatRequest, config: &ProviderConfig) -> ComponentResultV1<Value> {
    let tools = tools_of(request);
    let system = system_text(request)?;
    let conversation: Vec<&Message> =
        request.messages.iter().filter(|message| message.role != Role::System).collect();
    let Some((last, earlier)) = conversation.split_last() else {
        return Err(invalid_request("the request has no message to send"));
    };
    let conversation_id = attempt_id(request)?;

    let mut history = Vec::with_capacity(earlier.len());
    for message in earlier {
        history.push(entry_of(message)?);
    }
    let mut current = current_of(last)?;

    if let Some(system) = system {
        let first_user = history.iter_mut().find_map(|entry| match entry {
            Entry::User(entry) => Some(entry),
            Entry::Assistant(_) => None,
        });
        let target = first_user.unwrap_or(&mut current);
        let merged = with_system(&system, target.get("content").unwrap_or(&Value::Null));
        target.insert("content".to_owned(), merged);
    }

    current.insert("modelId".to_owned(), json!(request.model));
    if let Some(tools) = tools {
        let context = current
            .entry("userInputMessageContext".to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(context) = context {
            context.insert("tools".to_owned(), tools);
        }
    }

    let mut state = Map::new();
    state.insert("chatTriggerType".to_owned(), json!(CHAT_TRIGGER_TYPE));
    state.insert("conversationId".to_owned(), json!(conversation_id));
    state.insert("currentMessage".to_owned(), json!({"userInputMessage": current}));
    if !history.is_empty() {
        let entries: Vec<Value> = history
            .into_iter()
            .map(|entry| match entry {
                Entry::User(entry) => json!({"userInputMessage": entry}),
                Entry::Assistant(entry) => json!({"assistantResponseMessage": entry}),
            })
            .collect();
        state.insert("history".to_owned(), Value::Array(entries));
    }

    let mut body = Map::new();
    body.insert("conversationState".to_owned(), Value::Object(state));
    if let Some(arn) = config.declared.get(PROFILE_ARN_ATTRIBUTE) {
        body.insert("profileArn".to_owned(), json!(arn));
    }
    Ok(Value::Object(body))
}

// -- stream ------------------------------------------------------------------

/// The stream parser: the canonical re-encoding of the upstream's eventstream messages in,
/// IR events out.
#[derive(Debug, Default)]
struct KiroStreamParser {
    tail: Vec<u8>,
    /// Whether any frame has been parsed: an empty chunk before that is not the end of a body.
    parsed: bool,
    /// Whether `Done` or an in-band failure has ended the stream.
    closed: bool,
    /// The `toolUseId` of every tool call so far; its position is its IR index.
    calls: Vec<String>,
}

impl KiroStreamParser {
    /// Ends the stream with the failure the upstream reported in band.
    fn fail(&mut self, code: ErrorCode, provider_message: Option<&str>) -> Vec<StreamEvent> {
        self.closed = true;
        let mut error = ErrorEnvelope::new(code, 502, message_of(code));
        error.provider_message =
            provider_message.filter(|message| message.chars().count() <= 256).map(str::to_owned);
        vec![StreamEvent::Error { error }]
    }

    /// One `toolUseEvent`, read as the native arm reads it (server `translate_kiro.rs`): a missing
    /// `toolUseId` is the empty id, an `input` that is not a string is no fragment, and `stop` ends
    /// nothing the IR has to close. Calls are keyed by `toolUseId`, as the native non-streaming fold
    /// keys them, so a fragment after its call's `stop` extends that call (KD13).
    fn tool_events(&mut self, data: &Value) -> ComponentResultV1<Vec<StreamEvent>> {
        let id = data.get("toolUseId").and_then(Value::as_str).unwrap_or_default();
        if data.get("stop").and_then(Value::as_bool) == Some(true) {
            return Ok(Vec::new());
        }
        let fragment = data.get("input").and_then(Value::as_str).unwrap_or_default();
        let index = |position: usize| {
            u32::try_from(position)
                .map_err(|_| provider_protocol_error("the upstream opened too many tool calls"))
        };
        match self.calls.iter().position(|call| call == id) {
            // The first event of a call opens it and names it, whether or not it carries input.
            None => {
                let index = index(self.calls.len())?;
                self.calls.push(id.to_owned());
                Ok(vec![StreamEvent::ToolCallDelta {
                    index,
                    id: Some(id.to_owned()),
                    name: Some(
                        data.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    ),
                    arguments_delta: fragment.to_owned(),
                }])
            }
            Some(_) if fragment.is_empty() => Ok(Vec::new()),
            Some(position) => Ok(vec![StreamEvent::ToolCallDelta {
                index: index(position)?,
                id: None,
                name: None,
                arguments_delta: fragment.to_owned(),
            }]),
        }
    }

    fn events_of(&mut self, event: &str, data: &Value) -> ComponentResultV1<Vec<StreamEvent>> {
        if self.closed {
            return Ok(Vec::new());
        }
        self.parsed = true;
        match event {
            "assistantResponseEvent" => Ok(match data.get("content").and_then(Value::as_str) {
                Some(content) if !content.is_empty() => {
                    vec![StreamEvent::Delta { index: 0, content: content.to_owned() }]
                }
                _ => Vec::new(),
            }),
            "toolUseEvent" => self.tool_events(data),
            // The canonical re-encoding's exception and error frames: a failure the upstream
            // reported in band. Nothing follows it.
            exception if exception.starts_with("exception:") => {
                let code =
                    exception_code(&exception["exception:".len()..]).unwrap_or(ErrorCode::Internal);
                Ok(self.fail(code, data.get("message").and_then(Value::as_str)))
            }
            error if error.starts_with("error:") => Ok(self
                .fail(ErrorCode::UpstreamUnavailable, data.get("message").and_then(Value::as_str))),
            // `meteringEvent` carries credits, which are not tokens and never become `Usage`
            // (record §6.1); `contextUsageEvent` and any event the dialect gains later carry
            // nothing the IR has a field for.
            _ => Ok(Vec::new()),
        }
    }

    /// The end of the body. The wire has no terminal event, so the end completes the answer, a tool
    /// call without its `stop` included, as natively (record §5.3, as revised 2026-10-14). Before
    /// any frame an empty chunk is not the end of a body (gate ②'s incrementality check feeds one at
    /// split 0), so a stream that carried no frame emits no `Done` (KR20).
    fn end_of_body(&mut self) -> Vec<StreamEvent> {
        if self.closed || !self.parsed {
            return Vec::new();
        }
        self.closed = true;
        let finish_reason =
            if self.calls.is_empty() { FinishReason::Stop } else { FinishReason::ToolCalls };
        vec![
            StreamEvent::Finish { finish_reason: Some(finish_reason.clone()), stop_sequence: None },
            StreamEvent::Done { finish_reason: Some(finish_reason), stop_sequence: None },
        ]
    }
}

impl StreamParserV1 for KiroStreamParser {
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>> {
        if chunk.is_empty() {
            return Ok(self.end_of_body());
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

// -- errors ------------------------------------------------------------------

/// An AWS exception name this upstream may send, as a response header, a body `__type` or an
/// in-band `:exception-type` names it (record §9; which names it really sends is K-Q6, provisional).
/// The first letter is compared without case, since streams spell names in lower camel case.
fn exception_code(exception: &str) -> Option<ErrorCode> {
    let mut chars = exception.chars();
    let first = chars.next()?.to_ascii_uppercase();
    let name = format!("{first}{}", chars.as_str());
    Some(match name.as_str() {
        "ValidationException" | "ResourceNotFoundException" => ErrorCode::InvalidRequest,
        "AccessDeniedException" | "UnrecognizedClientException" => ErrorCode::Auth,
        "ModelTimeoutException" => ErrorCode::Timeout,
        "ThrottlingException" => ErrorCode::RateLimit,
        "InternalServerException" | "ServiceUnavailableException" => ErrorCode::UpstreamUnavailable,
        _ => return None,
    })
}

/// The code a status means when no exception name maps (record §9).
const fn status_code(status: u16) -> ErrorCode {
    match status {
        400 | 404 | 422 => ErrorCode::InvalidRequest,
        401 | 403 => ErrorCode::Auth,
        402 => ErrorCode::PaymentRequired,
        408 => ErrorCode::Timeout,
        429 => ErrorCode::RateLimit,
        500 | 502 | 503 | 504 => ErrorCode::UpstreamUnavailable,
        _ => ErrorCode::Internal,
    }
}

impl ProviderComponentV1 for KiroReferenceV1 {
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
        // all there is (K-Q13).
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
        let body = body_of(request, config)?;
        // The wire has no stream switch (`request_facts.stream: "none"`): the request is the same
        // for both modes, and the upstream always answers with an event stream.
        let url = format!("{}{ACTION_PATH}", config.base_url.as_str().trim_end_matches('/'));
        let mut descriptor = HttpRequestDescriptor::new(HttpMethod::Post, url);
        // The headers the native arm sends, but `user-agent`, which the manifest declares
        // (`user_agent`), and `authorization`, which the host presents for the bearer slot. No
        // `content-type`, as natively (K-Q10, provisional).
        descriptor.headers = SafeHeaders::try_new([
            ("accept", EVENTSTREAM_MEDIA_TYPE),
            ("amz-sdk-request", AMZ_SDK_REQUEST),
            ("x-amz-target", AMZ_TARGET),
            ("x-amzn-kiro-agent-mode", AGENT_MODE),
        ])
        .map_err(internal)?;
        descriptor.body = Some(body);
        descriptor.auth = config.auth.clone().map(Auth::bearer);
        Ok(descriptor)
    }

    /// The buffered path (boundary §5.2): the whole 2xx body, deframed by the host and handed over
    /// as the canonical re-encoding, folded by the stream parser.
    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        let mut parser = KiroStreamParser::default();
        let mut events = parser.parse_chunk(parts.body.as_bytes())?;
        events.extend(parser.parse_chunk(&[])?);

        let mut text = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut finish_reason = None;
        for event in events {
            match event {
                StreamEvent::Delta { content, .. } => text.push_str(&content),
                StreamEvent::ToolCallDelta { index, id, name, arguments_delta } => {
                    let index = usize::try_from(index).map_err(internal)?;
                    if index == tool_calls.len() {
                        tool_calls.push(ToolCall {
                            id: id.unwrap_or_default(),
                            name: name.unwrap_or_default(),
                            arguments: String::new(),
                        });
                    }
                    let Some(call) = tool_calls.get_mut(index) else {
                        return Err(internal("a tool call fragment named an index never opened"));
                    };
                    call.arguments.push_str(&arguments_delta);
                }
                StreamEvent::Error { error } => return Err(error),
                StreamEvent::Done { finish_reason: reason, .. } => finish_reason = reason,
                _ => {}
            }
        }
        // Without `Done` and without an error the body carried no frame at all: the native arm
        // answers that with an empty success, and so does the buffered path (record §5.4).
        let finish_reason = finish_reason.unwrap_or(FinishReason::Stop);

        Ok(ChatResponse {
            // The wire carries neither a response id nor the model; the host fills both.
            id: String::new(),
            model: String::new(),
            choices: vec![Choice {
                index: 0,
                message: Message {
                    role: Role::Assistant,
                    content: (!text.is_empty()).then_some(Content::Text(text)),
                    tool_calls,
                    tool_call_id: None,
                    name: None,
                    extensions: token_station_protocol::Extensions::new(),
                },
                finish_reason: Some(finish_reason),
                stop_sequence: None,
            }],
            // `usage_evidence: absent`: all zero, and the host never reads it.
            usage: Usage::default(),
            extensions: token_station_protocol::Extensions::new(),
        })
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        let raw: Value = serde_json::from_str(&parts.body).unwrap_or(Value::Null);
        let code = exception_code(exception_name(parts, &raw))
            .unwrap_or_else(|| status_code(parts.status));
        let mut envelope = ErrorEnvelope::new(code, parts.status, message_of(code));
        envelope.provider_message = raw
            .get("message")
            .and_then(Value::as_str)
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
        Box::new(KiroStreamParser::default())
    }
}
