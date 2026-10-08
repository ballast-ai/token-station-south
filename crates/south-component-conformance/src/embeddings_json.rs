//! The unique strict embeddings-v1 JSON codec shared by fixtures, ABI and sandbox.
//!
//! Three frames cross the component boundary in IR-bearing or composite form: the prepared
//! request, the parsed facts and the provider-error result. Each decoder refuses unknown fields,
//! bounds the frame before parsing, and re-encodes what it accepted, so a frame the encoder could
//! not produce is never admitted.

use crate::PreparedEmbeddingsV1;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use south_contracts::{
    EmbeddingsEstimateV1, EmbeddingsFailureOutcomeV1, EmbeddingsParsedV1,
    MAX_EMBEDDINGS_PARSE_CONTEXT_BYTES, MAX_JSON_REQUEST_BODY_BYTES, VectorLocatorV1,
};
use token_station_protocol::{ErrorEnvelope, HttpRequestDescriptor};

/// Maximum encoded parsed-facts or provider-error frame: 2048 per-input counts and a bounded
/// error envelope fit with room to spare.
pub const MAX_EMBEDDINGS_FACT_JSON_BYTES: usize = 256 * 1024;

fn parse<T: DeserializeOwned>(input: &str, limit: usize) -> Result<T, String> {
    if input.len() > limit {
        return Err("embeddings JSON exceeds the boundary limit".into());
    }
    serde_json::from_str(input).map_err(|_| "invalid embeddings JSON".into())
}
fn bounded(value: Value, limit: usize) -> Result<Value, String> {
    if value.to_string().len() > limit {
        return Err("embeddings JSON exceeds the boundary limit".into());
    }
    Ok(value)
}
fn explicit_optional<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// Checks the parse context against its bound; it is otherwise opaque.
///
/// # Errors
/// Returns why the context is refused.
pub fn validate_parse_context(context: &Value) -> Result<(), String> {
    if context.to_string().len() > MAX_EMBEDDINGS_PARSE_CONTEXT_BYTES {
        return Err("embeddings parse context exceeds its bound".into());
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedWire {
    descriptor: HttpRequestDescriptor,
    estimate: EmbeddingsEstimateV1,
    vectors: VectorLocatorV1,
    /// Required key; `null` = the component makes no statement.
    #[serde(deserialize_with = "explicit_optional")]
    immutable_body_paths: Option<Vec<String>>,
    /// Required key; `null` is a valid context.
    parse_context: Value,
}

/// Decodes a prepared request; network authorization remains host-owned.
///
/// # Errors
/// Returns why the frame is refused: too large, not the strict shape, invalid immutable paths or
/// an oversized parse context.
pub fn parse_prepared_embeddings_json(input: &str) -> Result<PreparedEmbeddingsV1, String> {
    let wire = parse::<PreparedWire>(input, MAX_JSON_REQUEST_BODY_BYTES)?;
    let prepared = PreparedEmbeddingsV1 {
        descriptor: wire.descriptor,
        estimate: wire.estimate,
        vectors: wire.vectors,
        immutable_body_paths: wire.immutable_body_paths,
        parse_context: wire.parse_context,
    };
    // Canonical JSON may expand numeric spellings or add descriptor defaults.
    prepared_embeddings_json(&prepared)?;
    Ok(prepared)
}

/// Encodes a prepared request after validating what the type cannot hold by construction.
///
/// # Errors
/// Returns why the value cannot cross the boundary.
pub fn prepared_embeddings_json(value: &PreparedEmbeddingsV1) -> Result<Value, String> {
    if let Some(paths) = &value.immutable_body_paths {
        south_contracts::validate_immutable_body_paths(paths).map_err(|error| error.to_string())?;
    }
    validate_parse_context(&value.parse_context)?;
    bounded(
        json!({"descriptor":value.descriptor,"estimate":value.estimate,"vectors":value.vectors,
            "immutable_body_paths":value.immutable_body_paths,"parse_context":value.parse_context}),
        MAX_JSON_REQUEST_BODY_BYTES,
    )
}

/// Decodes the facts of an erased 2xx; consistency with the request is the host's
/// `check_embeddings_response_v1`.
///
/// # Errors
/// Returns why the frame is refused.
pub fn parse_embeddings_parsed_json(input: &str) -> Result<EmbeddingsParsedV1, String> {
    let parsed = parse::<EmbeddingsParsedV1>(input, MAX_EMBEDDINGS_FACT_JSON_BYTES)?;
    embeddings_parsed_json(&parsed)?;
    Ok(parsed)
}

/// Encodes parsed facts.
///
/// # Errors
/// Returns why the value cannot cross the boundary.
pub fn embeddings_parsed_json(value: &EmbeddingsParsedV1) -> Result<Value, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    bounded(value, MAX_EMBEDDINGS_FACT_JSON_BYTES)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderErrorWire {
    outcome: EmbeddingsFailureOutcomeV1,
    error: ErrorEnvelope,
}

/// Decodes `map-provider-error`'s result: `{"outcome":"rejected"|"unknown","error":{...}}`.
///
/// # Errors
/// Returns why the frame is refused.
pub fn parse_provider_error_json(
    input: &str,
) -> Result<(EmbeddingsFailureOutcomeV1, ErrorEnvelope), String> {
    let wire = parse::<ProviderErrorWire>(input, MAX_EMBEDDINGS_FACT_JSON_BYTES)?;
    provider_error_json(wire.outcome, &wire.error)?;
    Ok((wire.outcome, wire.error))
}

/// Encodes `map-provider-error`'s result.
///
/// # Errors
/// Returns why the value cannot cross the boundary.
pub fn provider_error_json(
    outcome: EmbeddingsFailureOutcomeV1,
    error: &ErrorEnvelope,
) -> Result<Value, String> {
    bounded(json!({"outcome":outcome,"error":error}), MAX_EMBEDDINGS_FACT_JSON_BYTES)
}
