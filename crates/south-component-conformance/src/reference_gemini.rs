//! The native reference implementation of the official Gemini provider
//! component.
//!
//! Design record: `docs/design/2026-08-22-gemini-provider-component.md`.
//!
//! Same shape as the other two references: gate ② is frozen against this
//! implementation, a `wasm32-wasip2` build of the same logic is what ships,
//! and the sandbox parity test proves the two agree.
//!
//! The request body, the response reader, the stream parser and the refusals
//! are the crate-private module `gemini_text`, which `provider-vertex` also
//! uses (design record `docs/design/2026-10-15-provider-vertex.md` §3). This
//! file is the Gemini API half: the family, the route and the auth.

use south_provider_api::{ComponentMetadataV1, PROVIDER_WORLD};
use token_station_protocol::{
    Auth, ChatRequest, ChatResponse, ErrorCode, ErrorEnvelope, HttpRequestDescriptor,
    HttpResponseParts, ProviderConfig,
};

use crate::component::{ComponentResultV1, ProviderComponentV1, StreamParserV1};
use crate::gemini_text;

/// The API version this component speaks. A wire constant of the dialect.
const API_VERSION: &str = "v1beta";
/// The family, as a host's provider rows name it.
const GEMINI: &str = "gemini";
/// The header the Gemini API reads the key from.
const API_KEY_HEADER: &str = "x-goog-api-key";

/// The reference component. Stateless; each stream gets its own parser.
#[derive(Debug, Default, Clone, Copy)]
pub struct GeminiReferenceV1;

fn internal(detail: impl std::fmt::Display) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string())
}

impl ProviderComponentV1 for GeminiReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "provider-gemini".to_owned(),
            version: "1.3.9".to_owned(),
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
        gemini_text::refuse_foreign_reasoning_replay(request)?;
        gemini_text::serves(config, GEMINI)?;
        gemini_text::require_model(request)?;
        // `ProviderApi::resolve` covers four canonical shapes and none of them
        // is this one, so the URL is built from the endpoint's own text — which
        // is what `permits` authorizes against.
        let (method, query) = gemini_text::operation(request.stream);
        let url = format!(
            "{}/{API_VERSION}/models/{}:{method}{query}",
            config.base_url.as_str().trim_end_matches('/'),
            crate::url_segment::encode(&request.model)
        );
        let mut descriptor = gemini_text::descriptor(request, url)?;
        descriptor.auth = match config.auth.clone() {
            Some(secret) => Some(Auth::header(API_KEY_HEADER, secret).map_err(internal)?),
            None => None,
        };
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
