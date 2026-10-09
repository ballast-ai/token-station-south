//! The native reference implementation of the Bedrock `InvokeModel` Anthropic provider component
//! (`provider-anthropic-bedrock-invoke`, family `anthropic-bedrock-invoke`).
//!
//! Design record: `docs/design/2026-10-08-bedrock-invoke-anthropic-component.md`.
//!
//! Bedrock serves Claude through `InvokeModel` with the Anthropic Messages body almost unchanged, so
//! this reference is a thin layer over [`crate::reference_anthropic`]'s source:
//!
//! - **Request.** The Messages body the shared builder produces, minus `model` (the model is in the
//!   URL) and `stream` (the operation name says it), plus `anthropic_version:
//!   "bedrock-2023-05-31"`. `POST {base}/model/{model}/invoke`, or `…/invoke-with-response-stream`,
//!   with the model as one encoded path segment and the headers the host's native arm and the
//!   Converse package send. The descriptor carries no auth: this is the `host_signed` arm, and the
//!   host's `aws-sigv4` finalizer signs the finished request, as for Converse.
//! - **Response.** The Anthropic message JSON, parsed by the shared Messages parser unchanged.
//! - **Stream.** The package declares `stream_framing: aws-eventstream`; the host deframes and
//!   feeds each message's canonical re-encoding, so an event arrives as `event: chunk` with
//!   `data: {"bytes": "<base64>"}`. The deframer is dialect-neutral and does not decode `bytes`;
//!   this parser does (standard alphabet, canonical padding, strictly), then hands the decoded
//!   Anthropic event to the shared Messages state machine. Exception and error frames end the
//!   stream as they do for Converse.
//! - **Errors.** The Bedrock exception name first (shared with Converse), then an Anthropic error
//!   `type`, then the status.

use serde_json::{Value, json};
use south_provider_api::{ComponentMetadataV1, PROVIDER_WORLD};
use token_station_protocol::{
    ChatRequest, ChatResponse, ErrorCode, ErrorEnvelope, HttpMethod, HttpRequestDescriptor,
    HttpResponseParts, ProviderConfig, SafeHeaders, StreamEvent,
};

use crate::base64::{self, Alphabet, Padding};
use crate::component::{ComponentResultV1, ProviderComponentV1, StreamParserV1};
use crate::reasoning_replay::validated_layout;
use crate::reference_anthropic::{
    AnthropicReferenceV1, AnthropicSseParser, checked_body_of, error_type_code, frame_fields,
    message_of, provider_protocol_error, sse_frame_boundary, status_code,
};
use crate::reference_bedrock_converse::{exception_code, exception_name};

/// The family this package serves.
pub const FAMILY: &str = "anthropic-bedrock-invoke";

/// The `InvokeModel` dialect's version of the Messages body, a wire-protocol constant of the
/// dialect as `anthropic-version` is for Messages (design record §4.2).
const BEDROCK_ANTHROPIC_VERSION: &str = "bedrock-2023-05-31";

/// The media type `InvokeModel` streams in: binary AWS eventstream, which the host deframes.
const EVENTSTREAM_MEDIA_TYPE: &str = "application/vnd.amazon.eventstream";

/// The reference component. Stateless; each stream gets its own parser.
#[derive(Debug, Default, Clone, Copy)]
pub struct AnthropicBedrockInvokeReferenceV1;

fn internal(detail: impl std::fmt::Display) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string())
}

fn capability(detail: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Capability, 400, detail)
}

/// One `InvokeModel` stream, mid-parse: the eventstream envelope here, the Messages state machine
/// underneath.
#[derive(Debug, Default)]
struct InvokeStreamParser {
    tail: Vec<u8>,
    messages: AnthropicSseParser,
}

impl InvokeStreamParser {
    /// The Anthropic event inside one `chunk` message.
    ///
    /// Every step refuses rather than skips: a chunk is the only carrier of the model's output and
    /// of the usage the host bills, so a chunk this parser cannot read is a protocol error, never
    /// an event that silently did not happen. Members beside `bytes` (Bedrock pads the envelope
    /// with a random-length `p`) are ignored.
    fn chunk_events(&mut self, envelope: &Value) -> ComponentResultV1<Vec<StreamEvent>> {
        let Some(encoded) = envelope.get("bytes").and_then(Value::as_str) else {
            return Err(provider_protocol_error(
                "the upstream sent an InvokeModel chunk without a base64 `bytes` string",
            ));
        };
        let Some(decoded) = base64::decode(encoded, Alphabet::Standard, Padding::Canonical) else {
            return Err(provider_protocol_error(
                "the upstream sent an InvokeModel chunk whose `bytes` are not canonical base64",
            ));
        };
        let event: Value = serde_json::from_slice(&decoded).map_err(|_| {
            provider_protocol_error(
                "the upstream sent an InvokeModel chunk that does not decode to a JSON event",
            )
        })?;
        let Some(kind) = event.get("type").and_then(Value::as_str) else {
            return Err(provider_protocol_error(
                "the upstream sent an InvokeModel chunk whose event has no `type`",
            ));
        };
        self.messages.events_of(kind, &event)
    }
}

impl StreamParserV1 for InvokeStreamParser {
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>> {
        // A clean transport EOF, which the runtime spells as an empty fragment: the shared state
        // machine settles a stream that reached `message_delta` and leaves one that did not
        // without a terminal, which is the contract's truncation signal.
        if chunk.is_empty() {
            return Ok(self.messages.end_of_stream());
        }
        self.tail.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some((payload_end, frame_end)) = sse_frame_boundary(&self.tail) {
            let frame = self.tail.drain(..frame_end).collect::<Vec<u8>>();
            // Nothing follows a failure the upstream reported (`StreamEvent::Error`).
            if self.messages.is_closed() {
                continue;
            }
            let frame = std::str::from_utf8(&frame[..payload_end]).map_err(|_| {
                provider_protocol_error("the upstream sent a stream frame that is not UTF-8")
            })?;
            let (Some(event), Some(data)) = frame_fields(frame) else {
                continue;
            };
            let parsed: Value = serde_json::from_str(data).map_err(|_| {
                provider_protocol_error("the upstream sent a stream frame with invalid JSON")
            })?;
            match event {
                "chunk" => events.extend(self.chunk_events(&parsed)?),
                // The canonical re-encoding's exception and error frames
                // (`south_contracts::reencode_eventstream_v1`), as for Converse.
                exception if exception.starts_with("exception:") => {
                    let name = &exception["exception:".len()..];
                    let code = exception_code(name).unwrap_or(ErrorCode::Internal);
                    events.extend(self.messages.fail(code, parsed["message"].as_str()));
                }
                error if error.starts_with("error:") => {
                    events.extend(
                        self.messages
                            .fail(ErrorCode::UpstreamUnavailable, parsed["message"].as_str()),
                    );
                }
                // A top-level event InvokeModel gains later. Ignored rather than refused, as
                // Converse ignores one: a new event kind must not break an old component (I-Q12).
                _ => {}
            }
        }
        Ok(events)
    }
}

impl ProviderComponentV1 for AnthropicBedrockInvokeReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "provider-anthropic-bedrock-invoke".to_owned(),
            version: "1.0.1".to_owned(),
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
        for message in &request.messages {
            validated_layout(message)
                .map_err(|()| capability("invalid reasoning replay markers or layout"))?;
        }
        if config.provider != FAMILY {
            return Err(capability(format!("unsupported provider dialect `{}`", config.provider)));
        }
        if request.model.is_empty() {
            return Err(capability(
                "InvokeModel addresses the model in the URL path, so a request without one has \
                 no target to send to",
            ));
        }
        // The Messages body, adjusted exactly as the host's native arm adjusts it (design record
        // §4.2): InvokeModel takes the model from the URL and refuses a body that names it too,
        // rejects `stream` as an extra input, and needs its own version in the body.
        let mut body = checked_body_of(request, config)?;
        let Value::Object(fields) = &mut body else {
            return Err(internal("the Messages body is not a JSON object"));
        };
        fields.remove("model");
        fields.remove("stream");
        fields.insert("anthropic_version".to_owned(), json!(BEDROCK_ANTHROPIC_VERSION));

        // Streaming is a different operation, not a body field. The model is operator data and
        // one path segment: an inference-profile ARN's `/` is encoded inside it (SF26).
        let operation = if request.stream { "invoke-with-response-stream" } else { "invoke" };
        let url = format!(
            "{}/model/{}/{operation}",
            config.base_url.as_str().trim_end_matches('/'),
            crate::url_segment::encode(&request.model)
        );
        let mut descriptor = HttpRequestDescriptor::new(HttpMethod::Post, url);
        // The headers the host's native arm sends on InvokeModel and Converse alike (SF14).
        let accept = if request.stream { EVENTSTREAM_MEDIA_TYPE } else { "application/json" };
        descriptor.headers = SafeHeaders::try_new([
            ("content-type", "application/json"),
            ("accept", accept),
            ("x-amzn-bedrock-accept", "application/json"),
        ])
        .map_err(internal)?;
        descriptor.body = Some(body);
        // Deliberately no auth: the `host_signed` arm. A credential value never reaches this
        // component.
        Ok(descriptor)
    }

    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        // The non-streaming InvokeModel answer is the Messages answer.
        AnthropicReferenceV1.parse_response(parts)
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        let raw: Value = serde_json::from_str(&parts.body).unwrap_or(Value::Null);
        let code = exception_code(exception_name(parts, &raw))
            .or_else(|| raw["error"]["type"].as_str().and_then(error_type_code))
            .unwrap_or_else(|| status_code(parts.status));
        let mut envelope = ErrorEnvelope::new(code, parts.status, message_of(code));
        envelope.provider_message = raw["message"]
            .as_str()
            .or_else(|| raw["error"]["message"].as_str())
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
        Box::new(InvokeStreamParser::default())
    }
}
