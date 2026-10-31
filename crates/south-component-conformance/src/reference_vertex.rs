//! The native reference implementation of the official `provider-vertex` component: Google's
//! Gemini models on Vertex AI for text.
//!
//! Design record: `docs/design/2026-10-15-provider-vertex.md` (accepted 2026-10-15; VX-Q1 (a):
//! `:generateContent` serves every surface; VX-Q2: the family is `vertex-ai`).
//!
//! The request body, the response reader, the stream parser (with SF27's terminal-chunk usage rule,
//! which was found on Vertex) and the refusals are the crate-private module `gemini_text`, which
//! `provider-gemini` also uses; the location rules are the crate-private module `vertex_location`,
//! which the embeddings, image and speech Vertex references also use. This file is the Vertex half:
//! the family, the route and the auth (record §3).
//!
//! Transcribed from token-station-server's native Vertex text arm (`engine/vertex.rs`
//! `build_vertex_url`, the `(VertexAi, GeminiGenerate)` arm of `engine/text_admission.rs`):
//!
//! - **Route**: `POST {base_url}/v1/projects/{project}/locations/{region}/publishers/google/models/
//!   {model}:generateContent`, or `:streamGenerateContent?alt=sse` when streaming, each of
//!   `project`, `region` and `model` one percent-encoded path segment.
//! - **Location**: `region` from the `region` config key, the project from the `project` config key
//!   when set, else the `project_id` the credential exports; a Vertex AI base URL must be the
//!   region's origin, any other base is used as given. A missing value is a 500 naming the key,
//!   reported before any refusal of the request itself.
//! - **Auth** is `bearer` on the configured slot, which the package's `vertex_sa` credential recipe
//!   mints from the service account (the host executes it). No other header.

use south_provider_api::{ComponentMetadataV1, PROVIDER_WORLD};
use token_station_protocol::{
    Auth, ChatRequest, ChatResponse, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts,
    ProviderConfig,
};

use crate::component::{ComponentResultV1, ProviderComponentV1, StreamParserV1};
use crate::gemini_text;

/// The package name this reference is published as.
pub const NAME: &str = "provider-vertex";
/// The package version.
pub const VERSION: &str = "1.4.5";
/// The family, as a host's provider rows name it (VX-Q2).
pub const VERTEX_AI: &str = "vertex-ai";

/// The reference component. Stateless; each stream gets its own parser.
#[derive(Debug, Default, Clone, Copy)]
pub struct VertexReferenceV1;

impl ProviderComponentV1 for VertexReferenceV1 {
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
        Ok(config.models.clone())
    }

    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        gemini_text::serves(config, VERTEX_AI)?;
        let (method, query) = gemini_text::operation(request.stream);
        // The location first: a configuration error is reported before any refusal of the request
        // itself (record §4 P3, as `image-vertex`). The base URL is checked against the region.
        let (base, path) =
            crate::vertex_location::publisher_model_path(config, &request.model, method)?;
        gemini_text::refuse_foreign_reasoning_replay(request)?;
        gemini_text::require_model(request)?;
        // A model that refuses a forced tool choice is refused here, as on every provider family
        // (the family model parameters record §5, DH-Q4 revisited).
        crate::anthropic_dialect::refuse_declared_forced_tool(request, config)?;
        let mut descriptor = gemini_text::descriptor(request, format!("{base}/{path}{query}"))?;
        descriptor.auth = config.auth.clone().map(Auth::bearer);
        Ok(descriptor)
    }

    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        gemini_text::parse_response(parts)
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        Ok(gemini_text::map_provider_error(parts))
    }

    fn stream_parser(&self) -> Box<dyn StreamParserV1> {
        gemini_text::stream_parser()
    }
}
