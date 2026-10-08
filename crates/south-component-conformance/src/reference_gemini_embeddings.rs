//! The native reference implementation of the `embeddings-gemini` component.
//!
//! Gemini's native `embedContent` / `batchEmbedContents` dialect under embeddings contract 1
//! (`docs/design/2026-09-30-embeddings-contract.md` §6, §7.2).
//!
//! Transcribed from token-station-server's native Gemini embeddings arm so a dual run agrees on
//! the wire: `proxy_gemini_embeddings` in `gateway/src/modules/inference/handler/embeddings.rs`
//! (the URL, the single / batch choice, the estimate) and `openai_embeddings_to_gemini` in
//! `crates/gateway-provider-protocol/src/translate_gemini.rs` (the body).
//!
//! - A single input (`input_shape` `single`) goes to `…/v1beta/models/{model}:embedContent` with
//!   `{"model": "models/{model}", "content": {"parts": [{"text": …}]}}`; an array goes to
//!   `:batchEmbedContents` with one such object per input under `requests`. `dimensions` becomes
//!   `outputDimensionality` on the single body or on every batch item.
//! - Token-id inputs cannot be expressed in `content.parts` and are a capability error; the
//!   unmodelled northbound fields, `user` and `encoding_format` are ignored (the upstream
//!   returns floats; the host renders the requested encoding).
//! - Gemini reports no token counts (DE3 unmeasured), so usage is `NotReported` and the request
//!   carries the native arm's estimate as the fallback: the sum over inputs of
//!   `(utf8_len + 3) / 4`. `max_input_tokens` is the same value, so the reservation stays the
//!   estimate (§7.3).
//!
//! Differences from the native arm, all deliberate: a batch response lacking `embeddings` or
//! carrying an empty vector is a protocol error (the host's extraction refuses it) where the
//! native arm returned an empty vector (§11); the model is percent-encoded as one path segment in
//! the URL, as the provider world's Gemini reference does, where the native arm interpolates it
//! raw (identical for every model name of `[A-Za-z0-9._-]`).

use serde_json::{Value, json};
use south_contracts::{
    EmbeddingInputV1, EmbeddingsEstimateV1, EmbeddingsFailureOutcomeV1, EmbeddingsParsedV1,
    EmbeddingsRequestV1, EmbeddingsUsageFactsV1, InputShapeV1, JsonPointerV1, VectorLocatorV1,
};
use south_provider_api::{ComponentMetadataV1, EMBEDDINGS_WORLD};
use token_station_protocol::{
    Auth, ErrorCode, ErrorEnvelope, HttpMethod, HttpRequestDescriptor, HttpResponseParts,
    ProviderConfig, SafeHeaders,
};

use crate::reference_openai_compatible_embeddings::{envelope, outcome_of};
use crate::{ComponentResultV1, EmbeddingsComponentV1, PreparedEmbeddingsV1};

/// The package name and version this reference is published as.
pub const NAME: &str = "embeddings-gemini";
pub const VERSION: &str = "1.0.0";

/// The family, as the provider world names it.
const GEMINI: &str = "gemini";
/// The API version this component speaks. A wire constant of the dialect.
const API_VERSION: &str = "v1beta";

/// Request-body fields the host must not rewrite: the single and batch inputs and the model,
/// which must agree with the URL (the native arm's `GEMINI_EMBEDDINGS_OWNED_FIELDS`).
const IMMUTABLE_BODY_PATHS: [&str; 3] = ["content", "requests", "model"];

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct GeminiEmbeddingsReferenceV1;

fn internal(detail: impl std::fmt::Display) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string())
}

fn capability(detail: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Capability, 400, detail)
}

fn provider_protocol_error(message: &'static str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message)
}

fn pointer(text: &str) -> ComponentResultV1<JsonPointerV1> {
    JsonPointerV1::parse(text).map_err(internal)
}

/// The texts of the request; a token-id input has no faithful `content.parts` form.
fn texts(request: &EmbeddingsRequestV1) -> ComponentResultV1<Vec<&str>> {
    request
        .inputs()
        .iter()
        .map(|input| match input {
            EmbeddingInputV1::Text(text) => Ok(text.as_str()),
            EmbeddingInputV1::TokenIds(_) => Err(capability(
                "Gemini embeddings take text only; token-id inputs are not supported",
            )),
        })
        .collect()
}

/// One `embedContent` request object: the single body, or one batch item.
fn content_request(model_path: &str, text: &str, dimensions: Option<u32>) -> Value {
    let mut item = json!({"model": model_path, "content": {"parts": [{"text": text}]}});
    if let Some(dimensions) = dimensions {
        item["outputDimensionality"] = json!(dimensions);
    }
    item
}

/// The native arm's estimate: UTF-8 bytes, one token per four, rounded up, per input.
fn estimate_of(texts: &[&str]) -> u64 {
    texts
        .iter()
        .map(|text| u64::try_from(text.len()).unwrap_or(u64::MAX).saturating_add(3) / 4)
        .fold(0, u64::saturating_add)
}

impl EmbeddingsComponentV1 for GeminiEmbeddingsReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: NAME.to_owned(),
            version: VERSION.to_owned(),
            api_version: EMBEDDINGS_WORLD.to_owned(),
        }
    }

    fn build_embeddings_request(
        &self,
        config: &ProviderConfig,
        request: &EmbeddingsRequestV1,
    ) -> ComponentResultV1<PreparedEmbeddingsV1> {
        if config.provider != GEMINI {
            return Err(capability(format!("unsupported provider family `{}`", config.provider)));
        }
        let texts = texts(request)?;
        let model = request.model();
        let model_path = format!("models/{model}");
        let dimensions = request.dimensions();
        let batch = request.input_shape() == InputShapeV1::Array;
        let (method, body, vectors) = if batch {
            let requests: Vec<Value> =
                texts.iter().map(|text| content_request(&model_path, text, dimensions)).collect();
            let locator = VectorLocatorV1::Array {
                array: pointer("/embeddings")?,
                vector: pointer("/values")?,
                index: None,
            };
            ("batchEmbedContents", json!({"requests": requests}), locator)
        } else {
            let [text] = texts.as_slice() else {
                return Err(internal("a single-shaped request has one input"));
            };
            let locator = VectorLocatorV1::Single { vector: pointer("/embedding/values")? };
            ("embedContent", content_request(&model_path, text, dimensions), locator)
        };
        let url = format!(
            "{}/{API_VERSION}/models/{}:{method}",
            config.base_url.as_str().trim_end_matches('/'),
            crate::url_segment::encode(model)
        );
        let mut descriptor = HttpRequestDescriptor::new(HttpMethod::Post, url);
        descriptor.headers =
            SafeHeaders::try_new([("content-type", "application/json")]).map_err(internal)?;
        descriptor.body = Some(body);
        descriptor.auth = match config.auth.clone() {
            Some(secret) => Some(Auth::header("x-goog-api-key", secret).map_err(internal)?),
            None => None,
        };
        let estimate = estimate_of(&texts);
        Ok(PreparedEmbeddingsV1 {
            descriptor,
            estimate: EmbeddingsEstimateV1::new(Some(estimate), Some(estimate)),
            vectors,
            immutable_body_paths: Some(IMMUTABLE_BODY_PATHS.map(str::to_owned).to_vec()),
            parse_context: Value::Null,
        })
    }

    fn parse_embeddings_response(
        &self,
        parts: &HttpResponseParts,
        _parse_context: &Value,
    ) -> ComponentResultV1<EmbeddingsParsedV1> {
        let raw: Value = serde_json::from_str(&parts.body).map_err(|_| {
            provider_protocol_error("the upstream returned invalid JSON in a 2xx response")
        })?;
        if raw.get("error").is_some_and(|error| !error.is_null()) {
            return Err(provider_protocol_error(
                "the upstream embedded an error in a successful response",
            ));
        }
        let count = match (raw.get("embeddings"), raw.get("embedding")) {
            (Some(Value::Array(items)), _) => u32::try_from(items.len()).ok(),
            (None, Some(Value::Object(_))) => Some(1),
            _ => None,
        }
        .ok_or_else(|| provider_protocol_error("the upstream 2xx response has no embeddings"))?;
        Ok(EmbeddingsParsedV1::new(EmbeddingsUsageFactsV1::not_reported(), count, None))
    }

    fn map_provider_error(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<(EmbeddingsFailureOutcomeV1, ErrorEnvelope)> {
        let raw: Value = serde_json::from_str(&parts.body).unwrap_or(Value::Null);
        let code = match raw["error"]["status"].as_str().unwrap_or_default() {
            "RESOURCE_EXHAUSTED" => ErrorCode::RateLimit,
            "UNAUTHENTICATED" | "PERMISSION_DENIED" => ErrorCode::Auth,
            "INVALID_ARGUMENT" | "NOT_FOUND" | "FAILED_PRECONDITION" => ErrorCode::InvalidRequest,
            "UNAVAILABLE" => ErrorCode::UpstreamUnavailable,
            "DEADLINE_EXCEEDED" => ErrorCode::Timeout,
            _ => match parts.status {
                400 | 404 | 422 => ErrorCode::InvalidRequest,
                401 | 403 => ErrorCode::Auth,
                402 => ErrorCode::PaymentRequired,
                408 => ErrorCode::Timeout,
                429 => ErrorCode::RateLimit,
                529 => ErrorCode::Capacity,
                500 | 502 | 503 | 504 => ErrorCode::UpstreamUnavailable,
                _ => ErrorCode::Internal,
            },
        };
        Ok((outcome_of(parts.status), envelope(code, parts, &raw)))
    }
}
