//! The native reference implementation of the `image-gemini` component.
//!
//! Google's Nano Banana image models through the Gemini API's `:generateContent` (image world step
//! I3-2, design record `docs/design/2026-10-13-image-i3-2-gemini.md`; image record §13, Gemini
//! row). The request and response pieces are the crate-private module `gemini_image`, which the
//! Vertex package of step I3-3 shares; this file is the Gemini API half: the path and the auth.
//!
//! Transcribed from token-station-server's native Gemini image arm as the record summarises it
//! (`handler/images/gemini.rs`, `GEMINI_IMAGE_OWNED` in `images/execute.rs`):
//!
//! - **Generation** (`/v1/images/generations`, §2): `POST {base_url}/v1beta/models/{model}:
//!   generateContent`, `{"contents": [{"role": "user", "parts": [<inlineData>?, {"text": prompt}]}],
//!   "generationConfig": {"responseModalities": ["IMAGE"], "imageConfig"?}}`. The client's
//!   `image_url` (a base64 data URL, always elided) becomes one `inlineData` part whose `data` is a
//!   `data_url_payload` reference (GI-Q1); `aspect_ratio` and `resolution` (upper-cased) become
//!   `imageConfig.aspectRatio` / `imageSize`; every other field is dropped and the answer is always
//!   `b64_json`.
//! - **Edit** (`/v1/images/edits`, multipart, §3): the same JSON request, the first `image` file
//!   part as `inlineData` (`base64` of its bytes, labelled with the part's own `image/*` type or
//!   `image/png`, GI-Q6); `mask` and `stream` refused.
//! - **`n`** from 1 to 10 is `repeat` (GI-Q2): one image per upstream call, `requested_outputs` = `n`.
//! - **Facts**: no `size`; `tier.resolution` only when the client sent one and the host prices the
//!   call per image (GI-Q3); `inputs.reference_image` the reference count.
//! - **Response** (§4): every non-`thought` `inlineData` part across candidates is an inline base64
//!   artifact (GI-Q7); a blocked prompt is `rejected` and a policy finish reason without an image
//!   `unknown` (GI-Q5); `tokens` from `usageMetadata` with `total_input` / `total_output` required
//!   and the modality split only when it adds up (GI-Q4); `images_reported` is always `null`.
//! - **Auth** is `header_secret` `x-goog-api-key` on the configured slot.

use serde_json::{Value, json};
use south_contracts::image::{
    ImageCallContextV1, ImageModelCapabilitiesV1, ImageRenderContextV1, PreparedImageCallV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::ProviderConfig;

use crate::gemini_image::{self, GeminiRouteV1};
use crate::{ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1};

/// The package name this reference is published as.
pub const NAME: &str = "image-gemini";
/// The package version.
pub const VERSION: &str = "1.0.8";
/// The world this reference exports.
pub const WORLD: &str = "image-adapter-v1";
/// The family, as a host's image rows name it.
pub const GEMINI_IMAGE: &str = "gemini-image";
/// The header the Gemini API reads the key from.
pub const API_KEY_HEADER: &str = "x-goog-api-key";

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct GeminiImageReferenceV1;

fn route(config: &ProviderConfig, context: &ImageCallContextV1) -> GeminiRouteV1 {
    GeminiRouteV1 {
        path: format!(
            "v1beta/models/{}:generateContent",
            crate::url_segment::encode(&context.upstream_model)
        ),
        auth: config.auth.as_ref().map(|slot| {
            json!({ "arm": "header_secret", "header": API_KEY_HEADER, "slot": slot.as_str() })
        }),
        edit: true,
    }
}

impl ImageComponentV1 for GeminiImageReferenceV1 {
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
        gemini_image::serves(config, GEMINI_IMAGE)?;
        gemini_image::model_capabilities(config, true)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &ImageCallContextV1,
    ) -> ComponentResultV1<PreparedImageCallV1> {
        gemini_image::serves(config, GEMINI_IMAGE)?;
        gemini_image::prepare(request, context, route(config, context))
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
