//! Typed task-v2 host seam over the shared bounded component runtime.

use south_contracts::{HostMintedValuesV1, TaskLocatorV2, TaskObservationV2, TaskRenderContextV2};
use south_provider_api::ComponentMetadataV1;
use south_provider_runtime::{CallErrorV1, LoadedComponentV1};
use token_station_protocol::{
    ErrorCode, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts, ProviderConfig,
};

use crate::{
    ComponentResultV1, PreparedTaskV2, SubmitOutcomeV2, TaskComponentV2, task_v2_json as wire,
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
fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> ComponentResultV1<T> {
    serde_json::from_str(raw).map_err(internal)
}

/// A loaded component whose admitted world is exactly task-v2.
#[derive(Debug)]
pub struct SandboxedTaskComponentV2 {
    component: LoadedComponentV1,
}
impl SandboxedTaskComponentV2 {
    /// Refuses a different world without consuming the loaded component.
    ///
    /// # Errors
    /// Returns the original component when its world is not task-v2.
    pub fn new(component: LoadedComponentV1) -> Result<Self, Box<LoadedComponentV1>> {
        if component.manifest().api_version == south_provider_api::TASK_WORLD_V2 {
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
impl TaskComponentV2 for SandboxedTaskComponentV2 {
    fn metadata(&self) -> ComponentMetadataV1 {
        self.component.metadata()
    }
    fn build_submit_request(
        &self,
        config: &ProviderConfig,
        request: &serde_json::Value,
        minted: &HostMintedValuesV1,
    ) -> ComponentResultV1<PreparedTaskV2> {
        let minted =
            serde_json::json!({"task_id":minted.task_id(), "callback_url":minted.callback_url()});
        let raw = self
            .component
            .call_build_submit_request_v2(&encode(config)?, &encode(request)?, &minted.to_string())
            .map_err(seam_error)?;
        wire::parse_prepared_task_json(&raw).map_err(internal)
    }
    fn parse_submit_response(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<SubmitOutcomeV2> {
        let raw =
            self.component.call_parse_submit_response_v2(&encode(parts)?).map_err(seam_error)?;
        wire::parse_submit_outcome_json(&raw).map_err(internal)
    }
    fn build_observe_request(
        &self,
        config: &ProviderConfig,
        upstream_model: &str,
        upstream_task_id: &str,
        locator: &TaskLocatorV2,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        let raw = self
            .component
            .call_build_observe_request_v2(
                &encode(config)?,
                upstream_model,
                upstream_task_id,
                &wire::locator_json(locator).to_string(),
            )
            .map_err(seam_error)?;
        decode(&raw)
    }
    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV2> {
        let raw = self.component.call_parse_observation_v2(&encode(parts)?).map_err(seam_error)?;
        wire::parse_observation_json(&raw).map_err(internal)
    }
    fn build_artifact_request(
        &self,
        config: &ProviderConfig,
        locator: &TaskLocatorV2,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        let raw = self
            .component
            .call_build_artifact_request_v2(
                &encode(config)?,
                &wire::locator_json(locator).to_string(),
                &wire::observation_json(observation).map_err(internal)?.to_string(),
            )
            .map_err(seam_error)?;
        decode(&raw)
    }
    fn render_success(
        &self,
        observation: &TaskObservationV2,
        fetched: Option<&HttpResponseParts>,
        context: &TaskRenderContextV2,
    ) -> ComponentResultV1<serde_json::Value> {
        let fetched = fetched.map(encode).transpose()?;
        let raw = self
            .component
            .call_render_success_v2(
                &wire::observation_json(observation).map_err(internal)?.to_string(),
                fetched.as_deref(),
                &wire::render_context_json(context).to_string(),
            )
            .map_err(seam_error)?;
        decode(&raw)
    }
    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<ErrorEnvelope> {
        let raw = self
            .component
            .call_map_terminal_failure_v2(
                &wire::observation_json(observation).map_err(internal)?.to_string(),
            )
            .map_err(seam_error)?;
        decode(&raw)
    }
}
