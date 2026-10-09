//! The native reference implementation of the `embeddings-vertex` component.
//!
//! Vertex AI's `:predict` embeddings dialect under embeddings contract 1
//! (`docs/design/2026-09-30-embeddings-contract.md` §6, §16).
//!
//! Transcribed from token-station-server's native Vertex embeddings arm so a dual run agrees on the
//! wire: `vertex_embeddings_request`, `vertex_predict_to_openai` and `proxy_vertex_embeddings` in
//! `gateway/src/modules/inference/handler/embeddings.rs`, and `build_vertex_media_url` /
//! `vertex_host_for_region` in `gateway/src/modules/inference/engine/vertex.rs`.
//!
//! - One text input per request: the body is `{"instances": [{"content": text}]}`, with
//!   `parameters.outputDimensionality` when `dimensions` is set. More than one input, and token-id
//!   inputs, are capability errors before admission; a text of whitespace only is refused as the
//!   native arm refuses it. The unmodelled northbound fields, `user` and `encoding_format` are
//!   ignored (the upstream returns floats; the host renders the requested encoding).
//! - The URL is `{base_url}/v1/projects/{project}/locations/{region}/publishers/google/models/
//!   {model}:predict`. `region` and the project come from `ProviderConfig.declared` (§16): the
//!   `region` config key, and the `project` config key when the operator set that override, else
//!   the `project_id` the credential exports. `base_url` is the location's API origin, so the URL
//!   stays below it (`EndpointConfinement`). When `base_url` is a Vertex AI API origin it must be
//!   the one the region selects: `https://{region}-aiplatform.googleapis.com`, except
//!   `https://aiplatform.googleapis.com` for `global`, whose prefixed host answers 404. Any other
//!   base URL (a proxy, a test server) is used as given.
//! - Auth is `bearer` on the configured slot, which the package's credential recipe mints from the
//!   service account (the host executes it).
//! - Usage is `Reported`: each prediction's `embeddings.statistics.token_count` (the native arm
//!   also reads `tokenCount`), a float on the wire, rounded half away from zero, summed, with
//!   `per_input_tokens`. A missing, negative or non-numeric count is a protocol error, never zero.
//!   There is no fallback estimate; the host's bound (text bytes + 64) is the native arm's.
//! - Errors use the google.rpc status envelope Gemini uses, mapped as the Gemini reference does.
//!
//! Differences from the native arm, all deliberate: the model, region and project are each
//! percent-encoded as one path segment (identical for every value of `[A-Za-z0-9._-]`); a
//! response with more predictions than inputs fails the host's consistency checks, where the
//! native arm billed and returned only the first; a single token-id sequence is a token-id
//! refusal, where the native arm counted its integers as inputs (both answer 400 before
//! admission); missing configuration is a 500 naming the key, where the native arm answered 400.

use serde_json::{Value, json};
use south_contracts::{
    EmbeddingInputV1, EmbeddingsEstimateV1, EmbeddingsFailureOutcomeV1, EmbeddingsParsedV1,
    EmbeddingsRequestV1, EmbeddingsUsageFactsV1, JsonPointerV1, UsageSourceV1, VectorLocatorV1,
};
use south_provider_api::{ComponentMetadataV1, EMBEDDINGS_WORLD};
use token_station_protocol::{
    Auth, ErrorCode, ErrorEnvelope, HttpMethod, HttpRequestDescriptor, HttpResponseParts,
    ProviderConfig, SafeHeaders,
};

use crate::reference_gemini_embeddings::GeminiEmbeddingsReferenceV1;
use crate::{ComponentResultV1, EmbeddingsComponentV1, PreparedEmbeddingsV1};

/// The package name and version this reference is published as.
pub const NAME: &str = "embeddings-vertex";
pub const VERSION: &str = "1.0.2";

/// The family, as the host's task bindings name it.
const VERTEX_AI: &str = "vertex-ai";

/// The `declared` keys this component reads: the `region` and `project` config keys and the
/// `project_id` the credential exports.
const REGION: &str = "region";
const PROJECT_OVERRIDE: &str = "project";
const PROJECT_ID: &str = "project_id";

/// The location whose API host carries no region prefix.
const GLOBAL: &str = "global";
/// The suffix every Vertex AI API host shares.
const API_DOMAIN: &str = "aiplatform.googleapis.com";

/// Request-body fields the host must not rewrite: the billed input (the native arm's
/// `VERTEX_EMBEDDINGS_OWNED_FIELDS`; `parameters.*` is not priced, so request extras may set it).
const IMMUTABLE_BODY_PATHS: [&str; 1] = ["instances"];

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct VertexEmbeddingsReferenceV1;

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

/// The one text input `:predict` takes (the native arm's `vertex_embeddings_request`).
fn single_text(request: &EmbeddingsRequestV1) -> ComponentResultV1<&str> {
    if request.carries_media() {
        return Err(crate::reference_openai_compatible_embeddings::media_not_accepted());
    }
    let text = match request.inputs() {
        [EmbeddingInputV1::Text(text)] => text,
        [EmbeddingInputV1::Media { .. }] => {
            return Err(internal("a media input was checked above"));
        }
        [EmbeddingInputV1::TokenIds(_)] => {
            return Err(capability(
                "Vertex embeddings take text input; token-id arrays are not supported",
            ));
        }
        inputs => {
            return Err(capability(format!(
                "Vertex embeddings accept exactly one input per request (got {}); send one \
                 request per text",
                inputs.len()
            )));
        }
    };
    if text.trim().is_empty() {
        return Err(ErrorEnvelope::new(
            ErrorCode::InvalidRequest,
            400,
            "'input' must be a non-empty string",
        ));
    }
    Ok(text)
}

/// The API origin a location selects.
fn origin_of(region: &str) -> String {
    if region == GLOBAL {
        format!("https://{API_DOMAIN}")
    } else {
        format!("https://{region}-{API_DOMAIN}")
    }
}

/// The base URL the request goes under: `base_url`, which must be the region's origin when it is a
/// Vertex AI API origin at all.
fn base_of(config: &ProviderConfig, region: &str) -> ComponentResultV1<String> {
    let base = config.base_url.as_str();
    let base = base.trim_end_matches('/');
    let authority = base.split_once("://").map_or(base, |(_, rest)| rest);
    let host = authority.split(['/', ':']).next().unwrap_or_default();
    let is_vertex_host = host == API_DOMAIN || host.ends_with(&format!("-{API_DOMAIN}"));
    let expected = origin_of(region);
    if is_vertex_host && base != expected {
        return Err(internal(format!(
            "the base URL {base} is not the Vertex AI endpoint of location `{region}`, which is \
             {expected}"
        )));
    }
    Ok(base.to_owned())
}

/// A `declared` value the host must pass.
fn declared<'c>(config: &'c ProviderConfig, key: &str) -> ComponentResultV1<&'c str> {
    config.declared.get(key).ok_or_else(|| {
        internal(format!("the provider configuration carries no `{key}` for Vertex AI"))
    })
}

/// The largest count taken: below 2^53 every integral `f64` is exact.
const MAX_TOKEN_COUNT: f64 = 9_007_199_254_740_992.0;

/// One prediction's `token_count`, rounded half away from zero (`f64::round`, as the native arm
/// rounds), or `None` when it is absent, not a number, negative or out of range.
fn token_count(prediction: &Value) -> Option<u64> {
    let statistics = &prediction["embeddings"]["statistics"];
    let count = statistics.get("token_count").or_else(|| statistics.get("tokenCount"))?.as_f64()?;
    // NaN and the infinities lie outside the range too.
    if !(0.0..MAX_TOKEN_COUNT).contains(&count) {
        return None;
    }
    // `abs` turns a `-0.0` count, which the native arm reads as zero, into `0`.
    format!("{:.0}", count.round().abs()).parse().ok()
}

impl EmbeddingsComponentV1 for VertexEmbeddingsReferenceV1 {
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
        if config.provider != VERTEX_AI {
            return Err(capability(format!("unsupported provider family `{}`", config.provider)));
        }
        let text = single_text(request)?;
        let region = declared(config, REGION)?;
        let project = match config.declared.get(PROJECT_OVERRIDE) {
            Some(project) => project,
            None => declared(config, PROJECT_ID)?,
        };
        let url = format!(
            "{}/v1/projects/{}/locations/{}/publishers/google/models/{}:predict",
            base_of(config, region)?,
            crate::url_segment::encode(project),
            crate::url_segment::encode(region),
            crate::url_segment::encode(request.model())
        );
        let mut body = json!({"instances": [{"content": text}]});
        if let Some(dimensions) = request.dimensions() {
            body["parameters"] = json!({"outputDimensionality": dimensions});
        }
        let mut descriptor = HttpRequestDescriptor::new(HttpMethod::Post, url);
        descriptor.headers =
            SafeHeaders::try_new([("content-type", "application/json")]).map_err(internal)?;
        descriptor.body = Some(body);
        descriptor.auth = config.auth.clone().map(Auth::bearer);
        Ok(PreparedEmbeddingsV1 {
            descriptor,
            estimate: EmbeddingsEstimateV1::new(None, None),
            vectors: VectorLocatorV1::Array {
                array: pointer("/predictions")?,
                vector: pointer("/embeddings/values")?,
                index: None,
            },
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
        let predictions = raw["predictions"]
            .as_array()
            .filter(|predictions| !predictions.is_empty())
            .ok_or_else(|| {
                provider_protocol_error("the upstream 2xx response has no predictions")
            })?;
        let count = u32::try_from(predictions.len())
            .map_err(|_| provider_protocol_error("the upstream 2xx response has no predictions"))?;
        // Usage is funds evidence: a missing or malformed count is an error, never a zero.
        let per_input =
            predictions.iter().map(token_count).collect::<Option<Vec<u64>>>().ok_or_else(|| {
                provider_protocol_error(
                    "the upstream 2xx response carries no embeddings.statistics.token_count",
                )
            })?;
        let total = per_input
            .iter()
            .try_fold(0_u64, |sum, count| sum.checked_add(*count))
            .ok_or_else(|| provider_protocol_error("the upstream token counts overflow"))?;
        let usage =
            EmbeddingsUsageFactsV1::new(UsageSourceV1::Reported, Some(total), Some(per_input))
                .map_err(internal)?;
        Ok(EmbeddingsParsedV1::new(usage, count, None))
    }

    /// Vertex AI answers with the google.rpc status envelope Gemini uses.
    fn map_provider_error(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<(EmbeddingsFailureOutcomeV1, ErrorEnvelope)> {
        GeminiEmbeddingsReferenceV1.map_provider_error(parts)
    }
}
