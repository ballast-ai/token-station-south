//! Guest-side embeddings-v1 JSON boundary, using the shared validated codecs.
//!
//! A guest crate exports the `embeddings-adapter-v1` world by forwarding each export here with
//! its native component, so the guest and the host-side seam agree byte for byte.

use south_contracts::EmbeddingsRequestV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope};

use crate::{EmbeddingsComponentV1, embeddings_json as wire};

fn fail(error: &ErrorEnvelope) -> String {
    serde_json::to_string(error).unwrap_or_else(|_| {
        r#"{"code":"internal","http_status":500,"message":"unserializable error"}"#.to_owned()
    })
}

fn internal(detail: impl std::fmt::Display) -> String {
    fail(&ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string()))
}

fn parse<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, String> {
    serde_json::from_str(raw).map_err(internal)
}

/// `ProviderConfig`, `EmbeddingsRequestV1` -> `PreparedEmbeddingsV1`.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn build_embeddings_request_json(
    component: &dyn EmbeddingsComponentV1,
    config: &str,
    request: &str,
) -> Result<String, String> {
    let request: EmbeddingsRequestV1 = parse(request)?;
    let prepared = component
        .build_embeddings_request(&parse(config)?, &request)
        .map_err(|error| fail(&error))?;
    Ok(wire::prepared_embeddings_json(&prepared).map_err(internal)?.to_string())
}

/// An erased 2xx and the prepared parse context -> `EmbeddingsParsedV1`.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn parse_embeddings_response_json(
    component: &dyn EmbeddingsComponentV1,
    parts: &str,
    parse_context: &str,
) -> Result<String, String> {
    let context: serde_json::Value = parse(parse_context)?;
    wire::validate_parse_context(&context).map_err(internal)?;
    let parsed = component
        .parse_embeddings_response(&parse(parts)?, &context)
        .map_err(|error| fail(&error))?;
    Ok(wire::embeddings_parsed_json(&parsed).map_err(internal)?.to_string())
}

/// A non-2xx -> `{"outcome": ..., "error": ErrorEnvelope}`.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn map_provider_error_json(
    component: &dyn EmbeddingsComponentV1,
    parts: &str,
) -> Result<String, String> {
    let (outcome, error) =
        component.map_provider_error(&parse(parts)?).map_err(|error| fail(&error))?;
    Ok(wire::provider_error_json(outcome, &error).map_err(internal)?.to_string())
}
