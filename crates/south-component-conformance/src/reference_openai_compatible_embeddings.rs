//! The native reference implementation of the `embeddings-openai-compatible` component.
//!
//! It serves the `openai-compatible` and `azure-openai-v1` families of embeddings contract 1
//! (`docs/design/2026-09-30-embeddings-contract.md` §6).
//!
//! Transcribed from token-station-server's native OpenAI-compatible embeddings arm
//! (`gateway/src/modules/inference/handler/embeddings.rs`), which forwards the client body with
//! only `model` replaced and returns the upstream bytes unchanged. So the body reproduces the
//! client's: `input` is rebuilt from the inputs and their shape, `dimensions`, `encoding_format`
//! and `user` appear only when the client sent them, and every unmodelled field is forwarded
//! unchanged. The vectors are where a northbound response has them (`NorthIdentical`), usage is
//! the upstream's `usage.prompt_tokens`, and there is no fallback estimate: a 2xx without usage
//! cannot be settled.
//!
//! The URL is the endpoint's API root plus `/embeddings`, with the root rule the provider
//! world's reference applies to Chat Completions (`ProviderEndpoint::resolve`): the endpoint's
//! path, or `/v1` for an origin-only endpoint. `azure-openai-v1` is the GA v1 surface — the
//! endpoint is `…/openai/v1`, there is no `api-version` query, the deployment rides in `model` —
//! and presents its key in `api-key`.
//!
//! Differences from the native arm, all deliberate: a request mixing text and token-id inputs
//! (which no northbound body parses to) is refused here; an explicit `null` for `dimensions`,
//! `encoding_format` or `user` is not forwarded (the contract carries it as absent); more than
//! 2048 inputs never reach here (the contract refuses them before admission, record §11).

use serde_json::{Map, Value, json};
use south_contracts::{
    EmbeddingInputV1, EmbeddingsEstimateV1, EmbeddingsFailureOutcomeV1, EmbeddingsParsedV1,
    EmbeddingsRequestV1, EmbeddingsUsageFactsV1, EncodingV1, InputShapeV1, VectorLocatorV1,
};
use south_provider_api::{ComponentMetadataV1, EMBEDDINGS_WORLD};
use token_station_protocol::{
    Auth, ErrorCode, ErrorEnvelope, HttpMethod, HttpRequestDescriptor, HttpResponseParts,
    ProviderApi, ProviderConfig, SafeHeaders,
};

use crate::{ComponentResultV1, EmbeddingsComponentV1, PreparedEmbeddingsV1};

/// The package name and version this reference is published as.
pub const NAME: &str = "embeddings-openai-compatible";
pub const VERSION: &str = "1.0.1";

/// The two families, as the provider world names them.
const OPENAI_COMPATIBLE: &str = "openai-compatible";
const AZURE_OPENAI_V1: &str = "azure-openai-v1";

/// Request-body fields the host must not rewrite: the billed input and the upstream identity
/// (the native arm's `OPENAI_EMBEDDINGS_OWNED_FIELDS`).
const IMMUTABLE_BODY_PATHS: [&str; 2] = ["input", "model"];

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAiCompatibleEmbeddingsReferenceV1;

fn internal(detail: impl std::fmt::Display) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string())
}

fn capability(detail: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Capability, 400, detail)
}

fn provider_protocol_error(message: &'static str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message)
}

/// The URL: the API root `resolve` would use, plus `embeddings`. `ProviderApi` has no embeddings
/// entry, so the root is taken from the `models` resolution, whose last segment is fixed.
fn embeddings_url(config: &ProviderConfig) -> ComponentResultV1<String> {
    let models = config.base_url.resolve(ProviderApi::Models);
    let root = models.strip_suffix("models").ok_or_else(|| internal("unexpected API root"))?;
    Ok(format!("{root}embeddings"))
}

/// The northbound `input`, rebuilt from the inputs and their shape (record §3 table).
fn input_of(request: &EmbeddingsRequestV1) -> ComponentResultV1<Value> {
    let inputs = request.inputs();
    let all_text = inputs.iter().all(|input| matches!(input, EmbeddingInputV1::Text(_)));
    let all_ids = inputs.iter().all(|input| matches!(input, EmbeddingInputV1::TokenIds(_)));
    if !all_text && !all_ids {
        return Err(capability(
            "the OpenAI embeddings dialect cannot mix text and token-id inputs in one request",
        ));
    }
    let item = |input: &EmbeddingInputV1| match input {
        EmbeddingInputV1::Text(text) => json!(text),
        EmbeddingInputV1::TokenIds(ids) => json!(ids),
    };
    Ok(match (request.input_shape(), inputs) {
        (InputShapeV1::Single, [only]) => item(only),
        (InputShapeV1::Single, _) => return Err(internal("a single-shaped request has one input")),
        (InputShapeV1::Array, _) => Value::Array(inputs.iter().map(item).collect()),
    })
}

fn body_of(request: &EmbeddingsRequestV1) -> ComponentResultV1<Value> {
    let mut body = Map::new();
    body.insert("model".to_owned(), json!(request.model()));
    body.insert("input".to_owned(), input_of(request)?);
    if let Some(dimensions) = request.dimensions() {
        body.insert("dimensions".to_owned(), json!(dimensions));
    }
    if let Some(encoding) = request.encoding_format() {
        let word = match encoding {
            EncodingV1::Float => "float",
            EncodingV1::Base64 => "base64",
        };
        body.insert("encoding_format".to_owned(), json!(word));
    }
    if let Some(user) = request.user() {
        body.insert("user".to_owned(), json!(user));
    }
    for (key, value) in request.extra() {
        body.insert(key.clone(), value.clone());
    }
    Ok(Value::Object(body))
}

/// `rejected` for a 4xx other than 408: the upstream answered that it would not do the work.
/// A timeout, a 5xx and anything else do not prove nothing was produced.
pub(crate) const fn outcome_of(status: u16) -> EmbeddingsFailureOutcomeV1 {
    if status >= 400 && status < 500 && status != 408 {
        EmbeddingsFailureOutcomeV1::Rejected
    } else {
        EmbeddingsFailureOutcomeV1::Unknown
    }
}

impl EmbeddingsComponentV1 for OpenAiCompatibleEmbeddingsReferenceV1 {
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
        let auth = match (config.provider.as_str(), config.auth.clone()) {
            (OPENAI_COMPATIBLE, secret) => secret.map(Auth::bearer),
            (AZURE_OPENAI_V1, Some(secret)) => {
                Some(Auth::header("api-key", secret).map_err(internal)?)
            }
            (AZURE_OPENAI_V1, None) => None,
            (family, _) => {
                return Err(capability(format!("unsupported provider family `{family}`")));
            }
        };
        let mut descriptor = HttpRequestDescriptor::new(HttpMethod::Post, embeddings_url(config)?);
        descriptor.headers =
            SafeHeaders::try_new([("content-type", "application/json")]).map_err(internal)?;
        descriptor.body = Some(body_of(request)?);
        descriptor.auth = auth;
        Ok(PreparedEmbeddingsV1 {
            descriptor,
            estimate: EmbeddingsEstimateV1::new(None, None),
            vectors: VectorLocatorV1::NorthIdentical,
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
        let count = raw["data"]
            .as_array()
            .and_then(|data| u32::try_from(data.len()).ok())
            .ok_or_else(|| provider_protocol_error("the upstream 2xx response has no data"))?;
        // Usage is funds evidence: a missing or malformed count is an error, never a zero.
        let tokens = raw["usage"]["prompt_tokens"].as_u64().ok_or_else(|| {
            provider_protocol_error("the upstream 2xx response carries no usage.prompt_tokens")
        })?;
        Ok(EmbeddingsParsedV1::new(
            EmbeddingsUsageFactsV1::reported(tokens),
            count,
            raw["model"].as_str().map(str::to_owned),
        ))
    }

    fn map_provider_error(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<(EmbeddingsFailureOutcomeV1, ErrorEnvelope)> {
        let raw: Value = serde_json::from_str(&parts.body).unwrap_or(Value::Null);
        let provider_code = raw["error"]["code"].as_str().unwrap_or_default().to_ascii_lowercase();
        let code = if provider_code == "content_policy_violation" {
            ErrorCode::ContentPolicy
        } else if provider_code.contains("context_length")
            || provider_code.contains("maximum_context")
        {
            ErrorCode::ContextLength
        } else {
            match parts.status {
                400 | 404 | 422 => ErrorCode::InvalidRequest,
                401 | 403 => ErrorCode::Auth,
                402 => ErrorCode::PaymentRequired,
                408 => ErrorCode::Timeout,
                429 => ErrorCode::RateLimit,
                529 => ErrorCode::Capacity,
                500 | 502 | 503 | 504 => ErrorCode::UpstreamUnavailable,
                _ => ErrorCode::Internal,
            }
        };
        Ok((outcome_of(parts.status), envelope(code, parts, &raw)))
    }
}

/// The envelope the caller is answered with, in the provider references' wording.
pub(crate) fn envelope(code: ErrorCode, parts: &HttpResponseParts, raw: &Value) -> ErrorEnvelope {
    let message = match code {
        ErrorCode::InvalidRequest => "the upstream refused the request as malformed",
        ErrorCode::Auth => "the upstream rejected the credential",
        ErrorCode::PaymentRequired => {
            "the upstream requires payment or the account is out of funds"
        }
        ErrorCode::RateLimit => "the upstream rate limited this request",
        ErrorCode::ContentPolicy => "the upstream refused on content-policy grounds",
        ErrorCode::ContextLength => "the request exceeds the model's context window",
        ErrorCode::Timeout => "the upstream did not answer in time",
        ErrorCode::UpstreamUnavailable => "the upstream is unavailable",
        ErrorCode::TransportTruncated => "the upstream connection dropped mid-response",
        ErrorCode::ProviderProtocolError => "the upstream answered with an invalid body",
        ErrorCode::Capacity | ErrorCode::Capability | ErrorCode::Internal => "the upstream failed",
    };
    let mut envelope = ErrorEnvelope::new(code, parts.status, message);
    envelope.provider_message = raw["error"]["message"]
        .as_str()
        .filter(|message| message.chars().count() <= 256)
        .map(str::to_owned);
    envelope.retry_after_ms = parts
        .headers
        .get("retry-after")
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| seconds.saturating_mul(1000));
    envelope
}
