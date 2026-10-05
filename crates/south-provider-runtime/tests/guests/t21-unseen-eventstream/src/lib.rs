//! A provider component for a wire no host knows, carried in AWS eventstream:
//! family `t21-unseen-eventstream`.
//!
//! It is T21 item 5 of `docs/design/2026-09-30-host-zero-vendor-boundary.md`
//! (§12). An adopting host builds it from a south checkout, synthesizes its
//! manifest, and proves three generic executors against it, none of which may
//! know this provider:
//!
//! - the `aws-eventstream` stream framing (§5.2): the host deframes the
//!   upstream body with south's deframer and feeds each message's canonical
//!   re-encoding to `parse-stream-chunk`;
//! - the buffered path (§5.2): for a non-streaming caller of a family that
//!   declares `request_facts.stream: "none"`, the host buffers the whole 2xx
//!   body, deframes it and hands `parse-response` the concatenated re-encoding;
//! - declaration-selected signing (§5.3): the host signs with SigV4 because
//!   the manifest says `signing.scheme: aws-sigv4`, for a service name and an
//!   origin it has never seen.
//!
//! # The manifest a host synthesizes
//!
//! The host owns the compatibility tuple, the service name and the endpoint;
//! the values below are the ones south's own test uses
//! (`crates/south-provider-runtime/tests/t21_unseen_eventstream_v1.rs`). What
//! this guest's wire fixes is the identity, the family, the stream framing and
//! the request facts:
//!
//! ```json
//! "name": "t21-unseen-eventstream", "version": "1.0.0",
//! "api_version": "provider-adapter-v2",
//! "providers": ["t21-unseen-eventstream"], "capabilities": ["chat", "stream"],
//! "auth_arms": ["host_signed"],
//! "emits": ["authorization", "x-amz-date", "x-amz-content-sha256", "x-amz-security-token"],
//! "stream_framing": "aws-eventstream",
//! "signing": { "scheme": "aws-sigv4", "service": "t21svc",
//!              "region": { "template_param": "region" },
//!              "credentials": { "access_key_id": "access_key_id",
//!                               "secret_access_key": "secret_access_key",
//!                               "session_token": "session_token" } },
//! "credentials": { "schema": "south.credential-recipe.v1",
//!                  "fields": { "access_key_id": { "secret": true, "required": true },
//!                              "secret_access_key": { "secret": true, "required": true },
//!                              "session_token": { "secret": true } } },
//! "request_facts": { "t21-unseen-eventstream": {
//!     "output_cap": ["/t21_limits/max_out"],
//!     "model": { "url": "/t21/models/{model}/invoke" },
//!     "stream": "none" } },
//! "endpoint": { "t21-unseen-eventstream": "https://api.{region}.p21-unseen.test" },
//! "config_schema": { "t21-unseen-eventstream": {
//!     "region": { "syntax": "aws_region", "required": true } } },
//! "permissions": { "network": false, "filesystem": false, "secrets": [] }
//! ```
//!
//! The same guest also serves a bearer-variant manifest (`auth_arms:
//! ["bearer"]`, no `emits` or `signing`, the slot under `permissions.secrets`):
//! see "Auth" below.
//!
//! # The request
//!
//! `POST {base_url}/t21/models/{model}/invoke`, the routed model percent-encoded
//! as one path segment (RFC 3986 `pchar`, the encoding gate ② checks), with
//! `content-type: application/json` and `accept: application/vnd.amazon.eventstream`.
//! The body is `{"t21_turns": [{"speaker", "words"}...], "t21_limits":
//! {"max_out": N}}`: no `model`, no `messages`, no `stream`, no top-level cap.
//! There is no stream switch: a streaming and a non-streaming caller get the
//! same URL and the same body, which is what `stream: "none"` declares. The cap
//! is `sampling.max_output_tokens`; a request without one is refused, because
//! this wire has no unbounded form.
//!
//! # Auth
//!
//! A `host_signed` package's descriptor carries no `auth`: the host passes no
//! slot (`provider_config.auth` absent) and signs the finished request itself.
//! When the host does pass a slot, the guest presents it as bearer, so the same
//! component can serve a bearer-variant manifest. A host_signed manifest must
//! therefore never be given a slot; `rogue-signed-auth` below is the guest
//! carrying one anyway.
//!
//! # The upstream wire
//!
//! Three event types, as `:event-type` names, each with a JSON payload:
//!
//! - `t21Say` `{"text": "..."}` → a `delta` event;
//! - `t21Meter` `{"in": N, "out": M}` → a `usage` event. Exactly one per
//!   exchange, both counts exact: usage is funds evidence, so a missing count
//!   is a protocol error, never a zero;
//! - `t21End` `{"reason": "complete" | "cap", "ticket": "...", "served_model":
//!   "..."}` → a `done` event (`stop` or `length`). It ends the exchange and
//!   must follow the meter. `ticket` and `served_model` become the buffered
//!   response's `id` and `model`.
//!
//! What this guest reads is south's canonical re-encoding
//! (`south_contracts::reencode_eventstream_v1`), exactly one SSE frame per
//! message: `event: <:event-type>\ndata: <compact JSON>\n\n`; an exception
//! message is `event: exception:<:exception-type>` and an error message is
//! `event: error:<:error-code>` with `data: {"message": "..."}`. Both of those
//! end the exchange with the IR's stream `error` event (a provider error on the
//! buffered path), and nothing is emitted after them. An exception named
//! `t21Throttled` maps to `rate_limit`; every other exception and every error
//! message to `upstream_unavailable`.
//!
//! Anything else is refused rather than skipped: an unknown event type, or a
//! frame not in canonical form (CR line ends, a missing or extra line, a
//! `data:` value that is not compact JSON). A host that relayed the raw eventstream
//! bytes, or invented its own SSE framing, is therefore observable. A chunk may
//! end anywhere, so the unparsed tail is held in instance state (the host
//! promises one instance per stream); a tail that can no longer become a
//! canonical frame (a CR, or a head other than `event: `) is refused at once
//! rather than held forever. An empty chunk carries nothing.
//!
//! # The buffered path
//!
//! `parse-response` reads a body that is the concatenated canonical
//! re-encoding of the whole eventstream — not JSON. A body that is plain JSON,
//! ends inside a frame, has a frame after `t21End` or never reaches `t21End` is
//! a protocol error. This is what makes "the host took the buffered path"
//! observable: a host that handed over the raw upstream body, or a JSON body
//! from some other path, is refused.
//!
//! # Rogue modes, keyed by the routed model name
//!
//! As in T03, the negative evidence is keyed by the routed model name, so a
//! host seeds one catalog row per sentinel and asserts on its own side. Each
//! mode changes exactly one field of the descriptor the honest wire builds, so
//! the host's refusal points at that field. The host must refuse each with
//! zero upstream calls:
//!
//! - `rogue-signed-auth` — the descriptor carries a bearer `auth` slot
//!   (`provider_api_key`) although the package is `host_signed`. Proves the
//!   host's descriptor-auth admission (§4) refuses an arm the manifest does not
//!   declare instead of signing around it.
//! - `rogue-origin` — the URL is on `https://rogue.p21-unseen.test`, not on
//!   `base_url`. Proves endpoint confinement applies to the filled template's
//!   origin, even one under the same parent domain.
//! - `rogue-cap` — `t21_limits` is dropped and the cap is written only at
//!   top-level `max_tokens`, a location this family does not declare. Proves
//!   the seal reads the declared `output_cap` pointer, not the legacy
//!   top-level fields.
//! - `rogue-model-url` — the URL names `t21-decoy` instead of the routed
//!   model. Proves the seal checks the `{model}` segment of the declared URL
//!   template, not just the origin and a path prefix.
//!
//! Any other model name is the well-behaved wire above.

use std::sync::Mutex;

// The path names the single file, not the `wit/` directory: that directory
// resolves more than one package, and a directory path would make the
// generated module layout depend on which packages sit beside this one.
wit_bindgen::generate!({
    path: "../../../../south-provider-api/wit/provider-adapter.wit",
    world: "provider-adapter-v2",
});

use exports::token_station::adapter::provider_adapter::{AdapterHealth, AdapterMetadata, Guest};
use serde_json::{json, Value};
use token_station::adapter::common::HealthStatus;

/// The origin `rogue-origin` sends to. Not `.invalid`, which some hosts
/// special-case as a placeholder.
const ROGUE_ORIGIN: &str = "https://rogue.p21-unseen.test";

/// The model `rogue-model-url` names in the URL instead of the routed one.
const DECOY_MODEL: &str = "t21-decoy";

/// The slot `rogue-signed-auth` presents.
const ROGUE_SLOT: &str = "provider_api_key";

/// The longest upstream message carried into an error envelope.
const MAX_PROVIDER_MESSAGE_CHARS: usize = 256;

struct T21UnseenEventstream;

fn error_envelope(code: &str, http_status: u16, message: &str) -> String {
    json!({ "code": code, "http_status": http_status, "message": message }).to_string()
}

fn protocol_error(message: &str) -> String {
    error_envelope("provider_protocol_error", 502, message)
}

fn parse(input: &str) -> Result<Value, String> {
    serde_json::from_str(input)
        .map_err(|error| error_envelope("internal", 500, &format!("input is not JSON: {error}")))
}

/// Percent-encodes `value` as one URL path segment (RFC 3986 `pchar`), the
/// encoding the host's seal compares the `{model}` placeholder against.
fn encode_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

/// The text of one turn, whatever shape the IR used for it: a bare string for
/// plain text, an array of parts otherwise.
fn turn_text(message: &Value) -> String {
    match &message["content"] {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts.iter().filter_map(|part| part["text"].as_str()).collect(),
        _ => String::new(),
    }
}

// -- frames -------------------------------------------------------------------

/// One frame of the canonical re-encoding.
#[derive(Debug, PartialEq)]
enum Frame {
    /// `event: <name>` — an upstream event.
    Event { name: String, data: Value },
    /// `event: exception:<name>` — an upstream exception message.
    Exception { name: String, data: Value },
    /// `event: error:<code>` — an upstream error message.
    Error { message: String },
}

/// The offset of the next frame terminator, if a whole frame is buffered.
/// Compact JSON never holds a raw LF, so the first blank line ends the frame.
fn frame_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(2).position(|window| window == b"\n\n")
}

/// Whether `json` has no whitespace outside its strings, as the re-encoding
/// leaves it.
fn is_compact(json: &str) -> bool {
    let (mut in_string, mut escaped) = (false, false);
    for c in json.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
        } else if matches!(c, ' ' | '\t' | '\n' | '\r') {
            return false;
        }
    }
    true
}

/// Parses one frame, given without its terminating blank line. Only the
/// canonical form is accepted: exactly `event: <name>` then `data: <JSON>`,
/// LF line ends, no other line.
fn parse_frame(frame: &[u8]) -> Result<Frame, &'static str> {
    const NOT_CANONICAL: &str = "stream frame is not in the canonical eventstream re-encoding";
    let text = std::str::from_utf8(frame).map_err(|_| NOT_CANONICAL)?;
    if text.contains('\r') {
        return Err(NOT_CANONICAL);
    }
    let (event_line, data_line) = text.split_once('\n').ok_or(NOT_CANONICAL)?;
    if data_line.contains('\n') {
        return Err(NOT_CANONICAL);
    }
    let name = event_line
        .strip_prefix("event: ")
        .filter(|name| !name.is_empty() && !name.starts_with(' '))
        .ok_or(NOT_CANONICAL)?;
    let data = data_line
        .strip_prefix("data: ")
        .filter(|data| !data.is_empty() && is_compact(data))
        .ok_or(NOT_CANONICAL)?;
    let data: Value = serde_json::from_str(data).map_err(|_| NOT_CANONICAL)?;

    if let Some(exception) = name.strip_prefix("exception:") {
        if exception.is_empty() {
            return Err(NOT_CANONICAL);
        }
        return Ok(Frame::Exception { name: exception.to_owned(), data });
    }
    if let Some(code) = name.strip_prefix("error:") {
        // The re-encoding renders an error message's detail as exactly
        // `{"message": "..."}`; anything else did not come from it.
        let message = data
            .as_object()
            .filter(|object| object.len() == 1 && !code.is_empty())
            .and_then(|object| object.get("message"))
            .and_then(Value::as_str)
            .ok_or(NOT_CANONICAL)?;
        return Ok(Frame::Error { message: message.to_owned() });
    }
    Ok(Frame::Event { name: name.to_owned(), data })
}

// -- the exchange -------------------------------------------------------------

/// What one frame means for the exchange.
#[derive(Debug, PartialEq)]
enum Step {
    Say(String),
    Meter {
        input: u64,
        output: u64,
    },
    End {
        finish: &'static str,
        ticket: String,
        served_model: String,
    },
    /// The upstream reported a failure; the payload is the error envelope.
    Failed(Value),
}

/// The ordering rules both paths share: one meter, and the end after it.
#[derive(Debug, Default)]
struct Exchange {
    metered: bool,
}

fn failure(code: &str, message: &str, provider_message: Option<&str>) -> Value {
    let mut envelope = json!({ "code": code, "http_status": 502, "message": message });
    if let Some(detail) =
        provider_message.filter(|detail| detail.chars().count() <= MAX_PROVIDER_MESSAGE_CHARS)
    {
        envelope["provider_message"] = json!(detail);
    }
    envelope
}

impl Exchange {
    fn step(&mut self, frame: Frame) -> Result<Step, String> {
        match frame {
            Frame::Exception { name, data } => {
                let detail = data["message"].as_str();
                Ok(Step::Failed(if name == "t21Throttled" {
                    failure("rate_limit", "the upstream rate limited this request", detail)
                } else {
                    failure("upstream_unavailable", "the upstream is unavailable", detail)
                }))
            }
            Frame::Error { message } => Ok(Step::Failed(failure(
                "upstream_unavailable",
                "the upstream is unavailable",
                Some(&message),
            ))),
            Frame::Event { name, data } => match name.as_str() {
                "t21Say" => data["text"]
                    .as_str()
                    .map(|text| Step::Say(text.to_owned()))
                    .ok_or_else(|| protocol_error("t21Say carries no text")),
                "t21Meter" => {
                    let (Some(input), Some(output)) = (data["in"].as_u64(), data["out"].as_u64())
                    else {
                        return Err(protocol_error("t21Meter lacks exact in/out counts"));
                    };
                    if self.metered {
                        return Err(protocol_error("the exchange was metered twice"));
                    }
                    self.metered = true;
                    Ok(Step::Meter { input, output })
                }
                "t21End" => {
                    if !self.metered {
                        return Err(protocol_error("t21End arrived before t21Meter"));
                    }
                    let finish = match data["reason"].as_str() {
                        Some("complete") => "stop",
                        Some("cap") => "length",
                        _ => return Err(protocol_error("t21End has an unknown reason")),
                    };
                    let (Some(ticket), Some(served_model)) =
                        (data["ticket"].as_str(), data["served_model"].as_str())
                    else {
                        return Err(protocol_error("t21End lacks its ticket or served_model"));
                    };
                    Ok(Step::End {
                        finish,
                        ticket: ticket.to_owned(),
                        served_model: served_model.to_owned(),
                    })
                }
                _ => Err(protocol_error("unknown t21 event type")),
            },
        }
    }
}

// -- the stream ---------------------------------------------------------------

/// One stream's state. Instance state on purpose: the host promises one
/// component instance per stream.
struct StreamState {
    /// The unparsed tail.
    tail: Vec<u8>,
    exchange: Exchange,
    /// Whether `t21End` or a failure ended the stream; nothing is emitted
    /// after that.
    closed: bool,
}

static STREAM: Mutex<StreamState> = Mutex::new(StreamState {
    tail: Vec::new(),
    exchange: Exchange { metered: false },
    closed: false,
});

impl StreamState {
    fn parse_chunk(&mut self, chunk: &[u8]) -> Result<Vec<Value>, String> {
        if self.closed {
            return Ok(Vec::new());
        }
        self.tail.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(end) = frame_end(&self.tail) {
            let frame: Vec<u8> = self.tail.drain(..end + 2).collect();
            let frame = parse_frame(&frame[..end]).map_err(protocol_error)?;
            match self.exchange.step(frame)? {
                Step::Say(text) => {
                    events.push(json!({ "type": "delta", "index": 0, "content": text }));
                }
                Step::Meter { input, output } => events.push(json!({
                    "type": "usage",
                    "usage": { "input_tokens": input, "output_tokens": output },
                })),
                Step::End { finish, .. } => {
                    events.push(json!({ "type": "done", "finish_reason": finish }));
                    self.closed = true;
                }
                Step::Failed(error) => {
                    events.push(json!({ "type": "error", "error": error }));
                    self.closed = true;
                }
            }
            if self.closed {
                self.tail.clear();
                return Ok(events);
            }
        }
        // A partial frame that can no longer become canonical is refused now,
        // not held: a CRLF stream never shows the LF-LF terminator, so waiting
        // would silently swallow it.
        if self.tail.contains(&b'\r') || !is_frame_head(&self.tail) {
            return Err(protocol_error(
                "stream frame is not in the canonical eventstream re-encoding",
            ));
        }
        Ok(events)
    }
}

/// Whether `partial` can still grow into a canonical frame's `event: ` line.
fn is_frame_head(partial: &[u8]) -> bool {
    const HEAD: &[u8] = b"event: ";
    let shared = partial.len().min(HEAD.len());
    partial[..shared] == HEAD[..shared]
}

// -- the buffered path --------------------------------------------------------

/// Builds the response from the concatenated re-encoding of a whole
/// eventstream body.
fn parse_buffered(body: &str) -> Result<Value, String> {
    let mut rest = body.as_bytes();
    let mut exchange = Exchange::default();
    let mut text = String::new();
    let mut usage = None;
    let mut end = None;
    while !rest.is_empty() {
        let Some(at) = frame_end(rest) else {
            return Err(protocol_error(
                "the buffered body is not the canonical eventstream re-encoding",
            ));
        };
        let frame = parse_frame(&rest[..at]).map_err(protocol_error)?;
        rest = &rest[at + 2..];
        if end.is_some() {
            return Err(protocol_error("a frame follows t21End"));
        }
        match exchange.step(frame)? {
            Step::Say(piece) => text.push_str(&piece),
            Step::Meter { input, output } => usage = Some((input, output)),
            Step::End { finish, ticket, served_model } => {
                end = Some((finish, ticket, served_model))
            }
            Step::Failed(error) => return Err(error.to_string()),
        }
    }
    let (Some((finish, ticket, served_model)), Some((input, output))) = (end, usage) else {
        return Err(protocol_error("the buffered body never reached t21End"));
    };
    Ok(json!({
        "id": ticket,
        "model": served_model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": text },
            "finish_reason": finish,
        }],
        "usage": { "input_tokens": input, "output_tokens": output },
    }))
}

impl Guest for T21UnseenEventstream {
    fn metadata() -> AdapterMetadata {
        // Must equal the synthesized manifest, or gate ① refuses the
        // component: a package whose report disagrees with its declaration has
        // been repackaged around its vetting.
        AdapterMetadata {
            name: "t21-unseen-eventstream".to_owned(),
            version: "1.0.0".to_owned(),
            api_version: "provider-adapter-v2".to_owned(),
        }
    }

    fn healthcheck() -> AdapterHealth {
        AdapterHealth { status: HealthStatus::Ready, detail: None }
    }

    fn model_capabilities(provider_config: String) -> Result<String, String> {
        let config = parse(&provider_config)?;
        // A component has no network, so the operator's declaration is all
        // there is to report.
        Ok(config.get("models").cloned().unwrap_or_else(|| json!([])).to_string())
    }

    fn build_http_request(chat_request: String, provider_config: String) -> Result<String, String> {
        let request = parse(&chat_request)?;
        let config = parse(&provider_config)?;

        let base_url = config
            .get("base_url")
            .and_then(Value::as_str)
            .ok_or_else(|| error_envelope("internal", 500, "config has no base_url"))?
            .trim_end_matches('/');
        let model = request["model"]
            .as_str()
            .ok_or_else(|| error_envelope("internal", 500, "the request names no model"))?;
        // This wire has no unbounded form: refusing here is louder than
        // sending a request the seal will reject.
        let cap = request["sampling"]["max_output_tokens"].as_u64().ok_or_else(|| {
            error_envelope(
                "capability",
                400,
                "t21-unseen-eventstream requires sampling.max_output_tokens",
            )
        })?;

        let turns: Vec<Value> = request["messages"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|message| json!({ "speaker": message["role"], "words": turn_text(message) }))
            .collect();

        // No `stream` member and no stream-dependent path: the upstream always
        // answers in eventstream, so both callers send the same request.
        let mut descriptor = json!({
            "method": "POST",
            "url": format!("{base_url}/t21/models/{}/invoke", encode_segment(model)),
            "headers": {
                "content-type": "application/json",
                "accept": "application/vnd.amazon.eventstream",
            },
            "body": {
                "t21_turns": turns,
                "t21_limits": { "max_out": cap },
            },
        });

        // A host_signed manifest gets no slot; a bearer-variant one does.
        if let Some(slot) = config.get("auth").and_then(Value::as_str) {
            descriptor["auth"] = json!({ "scheme": "bearer", "secret": slot });
        }

        // Rogue modes; see the module header. Each one changes exactly one
        // field, so a host refusal points at that field and nothing else.
        match model {
            "rogue-signed-auth" => {
                descriptor["auth"] = json!({ "scheme": "bearer", "secret": ROGUE_SLOT });
            }
            "rogue-origin" => {
                descriptor["url"] =
                    json!(format!("{ROGUE_ORIGIN}/t21/models/{}/invoke", encode_segment(model)));
            }
            "rogue-cap" => {
                if let Some(body) = descriptor["body"].as_object_mut() {
                    body.remove("t21_limits");
                    body.insert("max_tokens".to_owned(), json!(cap));
                }
            }
            "rogue-model-url" => {
                descriptor["url"] = json!(format!("{base_url}/t21/models/{DECOY_MODEL}/invoke"));
            }
            _ => {}
        }

        Ok(descriptor.to_string())
    }

    fn parse_response(response_parts: String) -> Result<String, String> {
        let parts = parse(&response_parts)?;
        let body = parts["body"].as_str().unwrap_or_default();
        parse_buffered(body).map(|response| response.to_string())
    }

    fn parse_stream_chunk(chunk: Vec<u8>) -> Result<String, String> {
        let mut state = STREAM.lock().expect("single-threaded guest");
        let events = state.parse_chunk(&chunk)?;
        Ok(Value::Array(events).to_string())
    }

    fn map_provider_error(response_parts: String) -> Result<String, String> {
        let parts = parse(&response_parts)?;
        let status = parts["status"].as_u64().unwrap_or(500);
        let (code, message) = match status {
            401 | 403 => ("auth", "the upstream rejected the credential"),
            429 => ("rate_limit", "the upstream rate limited this request"),
            400..=499 => ("capability", "the upstream refused the request"),
            _ => ("upstream_unavailable", "the upstream failed"),
        };
        Ok(error_envelope(code, u16::try_from(status).unwrap_or(500), message))
    }
}

export!(T21UnseenEventstream);

#[cfg(test)]
mod tests {
    use super::{encode_segment, parse_buffered, parse_frame, Frame, StreamState};
    use serde_json::{json, Value};

    const STREAM: &str = concat!(
        "event: t21Say\ndata: {\"text\":\"Hi\"}\n\n",
        "event: t21Meter\ndata: {\"in\":5,\"out\":1}\n\n",
        "event: t21End\ndata: {\"reason\":\"cap\",\"ticket\":\"k\",\"served_model\":\"m\"}\n\n",
    );

    fn new_stream() -> StreamState {
        StreamState { tail: Vec::new(), exchange: super::Exchange::default(), closed: false }
    }

    fn code_of(error: &str) -> String {
        let envelope: Value = serde_json::from_str(error).expect("an envelope");
        envelope["code"].as_str().expect("a code").to_owned()
    }

    #[test]
    fn the_three_canonical_forms_parse() {
        assert_eq!(
            parse_frame(b"event: t21Say\ndata: {\"text\":\"hi\"}"),
            Ok(Frame::Event { name: "t21Say".to_owned(), data: json!({ "text": "hi" }) })
        );
        assert_eq!(
            parse_frame(b"event: exception:t21Throttled\ndata: {\"message\":\"m\"}"),
            Ok(Frame::Exception {
                name: "t21Throttled".to_owned(),
                data: json!({ "message": "m" })
            })
        );
        assert_eq!(
            parse_frame(b"event: error:T21Internal\ndata: {\"message\":\"m\"}"),
            Ok(Frame::Error { message: "m".to_owned() })
        );
    }

    #[test]
    fn frames_not_in_canonical_form_are_refused() {
        for frame in [
            &b"data: {}"[..],
            b"event: t21Say",
            b"event: t21Say\r\ndata: {}",
            b"event: t21Say\ndata: {}\nid: 7",
            b"event:t21Say\ndata: {}",
            b"event: \ndata: {}",
            b"event: t21Say\ndata:{}",
            b"event: t21Say\ndata: { }",
            b"event: t21Say\ndata: {\"text\": \"hi\"}",
            b"event: t21Say\ndata: not json",
            b"event: exception:\ndata: {}",
            b"event: error:Code\ndata: {\"message\":\"m\",\"extra\":1}",
            b"event: error:\ndata: {\"message\":\"m\"}",
            b"event: t21Say\ndata: \xff",
        ] {
            assert!(parse_frame(frame).is_err(), "{}", String::from_utf8_lossy(frame));
        }
    }

    #[test]
    fn a_stream_split_anywhere_yields_the_same_events() {
        let whole = new_stream().parse_chunk(STREAM.as_bytes()).expect("parses");
        assert_eq!(whole.len(), 3);
        assert_eq!(whole[2], json!({ "type": "done", "finish_reason": "length" }));
        for split in 0..=STREAM.len() {
            let mut stream = new_stream();
            let mut events = stream.parse_chunk(&STREAM.as_bytes()[..split]).expect("parses");
            events.extend(stream.parse_chunk(&STREAM.as_bytes()[split..]).expect("parses"));
            assert_eq!(events, whole, "split at {split}");
        }
    }

    #[test]
    fn nothing_is_emitted_after_a_failure() {
        let mut stream = new_stream();
        let events = stream
            .parse_chunk(
                b"event: error:X\ndata: {\"message\":\"m\"}\n\n\
                  event: t21Say\ndata: {\"text\":\"a\"}\n\n",
            )
            .expect("parses");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], json!("error"));
        assert_eq!(stream.parse_chunk(b"event: bogus\ndata: {}\n\n"), Ok(Vec::new()));
    }

    #[test]
    fn a_partial_frame_that_cannot_become_canonical_is_refused_at_once() {
        for partial in [&b"event: t21Say\r\n"[..], b"data: {", b"{\"text\":"] {
            let refused = new_stream().parse_chunk(partial).expect_err("refused");
            assert_eq!(code_of(&refused), "provider_protocol_error");
        }
        for partial in [&b"ev"[..], b"event: t21Say\ndata: {\"te"] {
            assert_eq!(new_stream().parse_chunk(partial), Ok(Vec::new()), "held");
        }
    }

    #[test]
    fn usage_is_never_defaulted() {
        let mut stream = new_stream();
        let refused =
            stream.parse_chunk(b"event: t21Meter\ndata: {\"in\":5}\n\n").expect_err("no out");
        assert_eq!(code_of(&refused), "provider_protocol_error");
        let early = "event: t21End\n\
                     data: {\"reason\":\"complete\",\"ticket\":\"k\",\"served_model\":\"m\"}\n\n";
        let refused = new_stream().parse_chunk(early.as_bytes()).expect_err("no meter");
        assert_eq!(code_of(&refused), "provider_protocol_error");
    }

    #[test]
    fn the_buffered_body_must_be_the_re_encoding() {
        let response = parse_buffered(STREAM).expect("parses");
        assert_eq!(response["usage"], json!({ "input_tokens": 5, "output_tokens": 1 }));
        assert_eq!(response["choices"][0]["finish_reason"], json!("length"));
        for body in [
            "{\"text\":\"Hi\"}",
            "",
            &STREAM[..STREAM.len() - 1],
            format!("{STREAM}{STREAM}").as_str(),
        ] {
            let refused = parse_buffered(body).expect_err("refused");
            assert_eq!(code_of(&refused), "provider_protocol_error", "{body}");
        }
    }

    #[test]
    fn the_model_is_one_segment() {
        assert_eq!(encode_segment("a/b c:d"), "a%2Fb%20c:d");
    }

    #[test]
    fn whitespace_inside_strings_is_still_compact() {
        assert!(super::is_compact("{\"text\":\"a b\\\" c\"}"));
        assert!(!super::is_compact("{\"text\":\"a\\\\\" ,\"b\":1}"));
    }
}
