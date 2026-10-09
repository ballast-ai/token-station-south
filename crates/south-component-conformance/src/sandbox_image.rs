//! Typed image-v1 host seam over the shared bounded component runtime.
//!
//! Each call encodes its inputs as the guest's [`crate::abi_image`] shim decodes them, and decodes
//! the answer with the strict codecs a host uses (`parse_prepared_image_call_v1`,
//! `parse_image_model_capabilities_v1`, [`crate::image_json`]), so a frame the suite accepts from
//! the sandbox is one a host would accept too.

use serde_json::Value;
use south_contracts::image::{
    ImageCallContextV1, ImageModelCapabilitiesV1, ImageRenderContextV1, PreparedImageCallV1,
    parse_image_model_capabilities_v1, parse_prepared_image_call_v1,
};
use south_contracts::media::MediaLimitsV1;
use south_provider_api::ComponentMetadataV1;
use south_provider_runtime::{CallErrorV1, LoadedComponentV1};
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

use crate::{
    ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1, image_json as wire,
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

/// A loaded component whose admitted world is exactly `image-adapter-v1`.
#[derive(Debug)]
pub struct SandboxedImageComponentV1 {
    component: LoadedComponentV1,
}
impl SandboxedImageComponentV1 {
    /// Refuses a different world without consuming the loaded component.
    ///
    /// # Errors
    /// Returns the original component when its world is not `image-adapter-v1`.
    pub fn new(component: LoadedComponentV1) -> Result<Self, Box<LoadedComponentV1>> {
        if component.manifest().api_version == south_provider_api::IMAGE_WORLD {
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
impl ImageComponentV1 for SandboxedImageComponentV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        self.component.metadata()
    }
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
        let raw =
            self.component.call_image_model_capabilities(&encode(config)?).map_err(seam_error)?;
        parse_image_model_capabilities_v1(&raw).map_err(internal)
    }
    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &ImageCallContextV1,
    ) -> ComponentResultV1<PreparedImageCallV1> {
        let raw = self
            .component
            .call_image_prepare(&encode(config)?, &encode(request)?, &encode(context)?)
            .map_err(seam_error)?;
        parse_prepared_image_call_v1(&raw, &MediaLimitsV1::V1).map_err(internal)
    }
    fn parse_response(&self, state: &Value, response: &Value) -> ComponentResultV1<ImageOutcomeV1> {
        let raw = self
            .component
            .call_image_parse_response(&encode(state)?, &encode(response)?)
            .map_err(seam_error)?;
        wire::parse_image_outcome_json(&raw).map_err(internal)
    }
    fn render(
        &self,
        state: &Value,
        outcomes: &[ImageOutcomeV1],
        context: &ImageRenderContextV1,
    ) -> ComponentResultV1<ImageRenderedV1> {
        let outcomes = wire::image_outcomes_json(outcomes).map_err(internal)?;
        let raw = self
            .component
            .call_image_render(&encode(state)?, &outcomes, &encode(context)?)
            .map_err(seam_error)?;
        wire::parse_image_rendered_json(&raw).map_err(internal)
    }
}
