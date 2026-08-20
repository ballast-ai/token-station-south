//! Guest-side JSON shims: one implementation of the `json in → json out`
//! boundary over any [`ProviderComponentV1`].
//!
//! The official component's wit-bindgen shell calls these, so the guest's
//! boundary behavior (error-envelope encoding, serialization failure
//! handling) is written once here and judged by gate ② rather than
//! re-implemented per component. The host-side inverse lives in the
//! [`sandbox`](crate::sandbox) module.

use token_station_protocol::{
    ChatRequest, ErrorCode, ErrorEnvelope, HttpResponseParts, ProviderConfig,
};

use crate::component::{ProviderComponentV1, StreamParserV1};

/// Serializes an envelope for the error channel; a serialization failure
/// falls back to a literal internal envelope rather than panicking.
fn fail(envelope: &ErrorEnvelope) -> String {
    serde_json::to_string(envelope).unwrap_or_else(|_| {
        r#"{"code":"internal","http_status":500,"message":"unserializable error"}"#.to_owned()
    })
}

fn internal(detail: impl std::fmt::Display) -> String {
    fail(&ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string()))
}

fn parse_input<T: for<'de> serde::Deserialize<'de>>(input: &str) -> Result<T, String> {
    serde_json::from_str(input).map_err(internal)
}

fn to_output<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(internal)
}

/// `provider-config` JSON → `list<ModelCapability>` JSON.
///
/// # Errors
///
/// An `ErrorEnvelope` as JSON, exactly as the WIT error channel carries it.
pub fn model_capabilities_json(
    component: &dyn ProviderComponentV1,
    config_json: &str,
) -> Result<String, String> {
    let config: ProviderConfig = parse_input(config_json)?;
    to_output(&component.model_capabilities(&config).map_err(|error| fail(&error))?)
}

/// (`ChatRequest`, `ProviderConfig`) JSON → `HttpRequestDescriptor` JSON.
///
/// # Errors
///
/// An `ErrorEnvelope` as JSON.
pub fn build_http_request_json(
    component: &dyn ProviderComponentV1,
    request_json: &str,
    config_json: &str,
) -> Result<String, String> {
    let request: ChatRequest = parse_input(request_json)?;
    let config: ProviderConfig = parse_input(config_json)?;
    to_output(&component.build_http_request(&request, &config).map_err(|error| fail(&error))?)
}

/// `HttpResponseParts` JSON → `ChatResponse` JSON.
///
/// # Errors
///
/// An `ErrorEnvelope` as JSON.
pub fn parse_response_json(
    component: &dyn ProviderComponentV1,
    parts_json: &str,
) -> Result<String, String> {
    let parts: HttpResponseParts = parse_input(parts_json)?;
    to_output(&component.parse_response(&parts).map_err(|error| fail(&error))?)
}

/// `HttpResponseParts` JSON → `ErrorEnvelope` JSON.
///
/// # Errors
///
/// An `ErrorEnvelope` as JSON — the mapping itself broke, as opposed to the
/// mapped envelope in the success position.
pub fn map_provider_error_json(
    component: &dyn ProviderComponentV1,
    parts_json: &str,
) -> Result<String, String> {
    let parts: HttpResponseParts = parse_input(parts_json)?;
    to_output(&component.map_provider_error(&parts).map_err(|error| fail(&error))?)
}

/// One stream's guest-side shim: raw chunk bytes in, `list<StreamEvent>`
/// JSON out.
pub struct StreamAbiV1 {
    parser: Box<dyn StreamParserV1>,
}

impl StreamAbiV1 {
    #[must_use]
    pub fn new(component: &dyn ProviderComponentV1) -> Self {
        Self { parser: component.stream_parser() }
    }

    /// # Errors
    ///
    /// An `ErrorEnvelope` as JSON.
    pub fn parse_chunk_json(&mut self, chunk: &[u8]) -> Result<String, String> {
        to_output(&self.parser.parse_chunk(chunk).map_err(|error| fail(&error))?)
    }
}

impl std::fmt::Debug for StreamAbiV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamAbiV1").finish_non_exhaustive()
    }
}

/// Kept typed so the shims and the fixtures agree on what an opaque error
/// payload minimally is; used by the sandbox seam's fallback parser.
#[cfg_attr(not(feature = "sandbox"), allow(dead_code))]
pub(crate) fn parse_error_envelope(error_json: &str) -> ErrorEnvelope {
    let parsed: Result<ErrorEnvelope, _> = serde_json::from_str(error_json);
    parsed.unwrap_or_else(|_| {
        ErrorEnvelope::new(
            ErrorCode::Internal,
            500,
            "component returned a malformed error envelope",
        )
    })
}
