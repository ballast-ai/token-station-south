//! The native reference implementation of the `image-vertex` component.
//!
//! Google's Nano Banana image models through Vertex AI's `:generateContent` (image world step
//! I3-3, design record `docs/design/2026-10-13-image-i3-3-vertex.md`; image record §13, Vertex
//! row). The request and response pieces are the crate-private module `gemini_image`, which
//! `image-gemini` also uses; the location rules are the crate-private module `vertex_location`,
//! which `embeddings-vertex` also uses. This file is the Vertex half: the route and the auth.
//!
//! Transcribed from token-station-server's native arm, which is one implementation for both
//! Google families (`handler/images/gemini.rs`; `build_vertex_media_url` in `engine/vertex.rs`):
//!
//! - **Generation** (`/v1/images/generations`, §3): `POST {base_url}/v1/projects/{project}/
//!   locations/{region}/publishers/google/models/{model}:generateContent`, each of `project`,
//!   `region` and `model` one percent-encoded path segment; the body, the refusals, `n` as
//!   `repeat`, the facts and the immutable paths are `image-gemini`'s (I3-2 §2).
//! - **Location** (§4): `region` from the `region` config key, the project from the `project`
//!   config key when set, else the `project_id` the credential exports; a Vertex AI base URL must
//!   be the region's origin, any other base is used as given. A missing value is a 500 naming the
//!   key, reported before any request refusal (the route is built before the shared `prepare`).
//! - **Edit** is not served (§3): `model-capabilities` lists `generate` and `image_url` only, and a
//!   multipart request is refused with 400.
//! - **Response, metering and render** (§5): `image-gemini`'s, unchanged.
//! - **Auth** is `bearer` on the configured slot, which the package's `vertex_sa` credential recipe
//!   mints from the service account (the host executes it). No other header.

use serde_json::{Value, json};
use south_contracts::image::{
    ImageCallContextV1, ImageModelCapabilitiesV1, ImageRenderContextV1, PreparedImageCallV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::ProviderConfig;

use crate::gemini_image::{self, GeminiRouteV1};
use crate::{ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1};

/// The package name this reference is published as.
pub const NAME: &str = "image-vertex";
/// The package version.
pub const VERSION: &str = "1.1.1";
/// The world this reference exports.
pub const WORLD: &str = "image-adapter-v1";
/// The family, as a host's image rows name it.
pub const VERTEX_AI_IMAGE: &str = "vertex-ai-image";

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct VertexImageReferenceV1;

fn route(
    config: &ProviderConfig,
    context: &ImageCallContextV1,
) -> ComponentResultV1<GeminiRouteV1> {
    // The base URL is checked against the region; the descriptor's path is relative to it.
    let (_base, path) = crate::vertex_location::publisher_model_path(
        config,
        &context.upstream_model,
        "generateContent",
    )?;
    Ok(GeminiRouteV1 {
        path,
        auth: config.auth.as_ref().map(|slot| json!({ "arm": "bearer", "slot": slot.as_str() })),
        edit: false,
    })
}

impl ImageComponentV1 for VertexImageReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: NAME.to_owned(),
            version: VERSION.to_owned(),
            api_version: WORLD.to_owned(),
        }
    }

    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
        gemini_image::serves(config, VERTEX_AI_IMAGE)?;
        gemini_image::model_capabilities(config, false)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &ImageCallContextV1,
    ) -> ComponentResultV1<PreparedImageCallV1> {
        gemini_image::serves(config, VERTEX_AI_IMAGE)?;
        let route = route(config, context)?;
        gemini_image::prepare(request, context, route)
    }

    fn parse_response(&self, state: &Value, response: &Value) -> ComponentResultV1<ImageOutcomeV1> {
        gemini_image::parse_response(state, response)
    }

    fn render(
        &self,
        _state: &Value,
        outcomes: &[ImageOutcomeV1],
        context: &ImageRenderContextV1,
    ) -> ComponentResultV1<ImageRenderedV1> {
        gemini_image::render(outcomes, context)
    }
}
