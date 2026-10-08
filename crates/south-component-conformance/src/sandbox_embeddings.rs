//! Typed embeddings-v1 host seam over the shared bounded component runtime.

use serde_json::Value;
use south_contracts::{EmbeddingsFailureOutcomeV1, EmbeddingsParsedV1, EmbeddingsRequestV1};
use south_provider_api::ComponentMetadataV1;
use south_provider_runtime::{CallErrorV1, LoadedComponentV1};
use token_station_protocol::{ErrorCode, ErrorEnvelope, HttpResponseParts, ProviderConfig};

use crate::{
    ComponentResultV1, EmbeddingsComponentV1, PreparedEmbeddingsV1, embeddings_json as wire,
};

fn internal(detail: impl std::fmt::Display) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string())
}
fn seam_error(error: CallErrorV1) -> ErrorEnvelope {
    match error {
        CallErrorV1::Component(raw) => crate::abi::parse_error_envelope(&raw),
        other => internal(other),
    }
}
fn encode<T: serde::Serialize>(value: &T) -> ComponentResultV1<String> {
    serde_json::to_string(value).map_err(internal)
}

/// A loaded component whose admitted world is exactly `embeddings-adapter-v1`.
#[derive(Debug)]
pub struct SandboxedEmbeddingsComponentV1 {
    component: LoadedComponentV1,
}
impl SandboxedEmbeddingsComponentV1 {
    /// Refuses a different world without consuming the loaded component.
    ///
    /// # Errors
    /// Returns the original component when its world is not `embeddings-adapter-v1`.
    pub fn new(component: LoadedComponentV1) -> Result<Self, Box<LoadedComponentV1>> {
        if component.manifest().api_version == south_provider_api::EMBEDDINGS_WORLD {
            Ok(Self { component })
        } else {
            Err(Box::new(component))
        }
    }
    /// Access to the runtime JSON face for bounded ABI calls.
    #[must_use]
    pub const fn inner(&self) -> &LoadedComponentV1 {
        &self.component
    }
}
impl EmbeddingsComponentV1 for SandboxedEmbeddingsComponentV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        self.component.metadata()
    }
    fn build_embeddings_request(
        &self,
        config: &ProviderConfig,
        request: &EmbeddingsRequestV1,
    ) -> ComponentResultV1<PreparedEmbeddingsV1> {
        let raw = self
            .component
            .call_build_embeddings_request(&encode(config)?, &encode(request)?)
            .map_err(seam_error)?;
        wire::parse_prepared_embeddings_json(&raw).map_err(internal)
    }
    fn parse_embeddings_response(
        &self,
        parts: &HttpResponseParts,
        parse_context: &Value,
    ) -> ComponentResultV1<EmbeddingsParsedV1> {
        wire::validate_parse_context(parse_context).map_err(internal)?;
        let raw = self
            .component
            .call_parse_embeddings_response(&encode(parts)?, &encode(parse_context)?)
            .map_err(seam_error)?;
        wire::parse_embeddings_parsed_json(&raw).map_err(internal)
    }
    fn map_provider_error(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<(EmbeddingsFailureOutcomeV1, ErrorEnvelope)> {
        let raw = self
            .component
            .call_map_embeddings_provider_error(&encode(parts)?)
            .map_err(seam_error)?;
        wire::parse_provider_error_json(&raw).map_err(internal)
    }
}
