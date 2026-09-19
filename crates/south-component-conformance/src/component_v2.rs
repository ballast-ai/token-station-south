//! Task-v2 typed translation seam; persistence and business policy stay in hosts.

use crate::ComponentResultV1;
use serde_json::Value;
use south_contracts::{HostMintedValuesV1, TaskLocatorV2, TaskObservationV2, TaskRenderContextV2};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{
    ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts, ProviderConfig,
};

/// A submit request and the non-secret route needed after a host restart.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedTaskV2 {
    /// The request the host authorizes and sends.
    pub descriptor: HttpRequestDescriptor,
    /// The immutable locator persisted before the first network call.
    pub locator: TaskLocatorV2,
}

/// Explicit submit outcomes; acceptance uncertainty is not rejection.
#[derive(Clone, Debug, PartialEq)]
pub enum SubmitOutcomeV2 {
    /// The original upstream handle, without rewriting it.
    Accepted(String),
    /// A synchronous terminal execution observation.
    AcceptedTerminal(TaskObservationV2),
    /// Explicit upstream refusal.
    Rejected(ErrorEnvelope),
    /// Acceptance could not be determined; the host keeps its reservation.
    Unknown,
}

/// Pure task-v2 dialect operations over explicit host inputs.
pub trait TaskComponentV2 {
    /// Reports the identity verified against the component manifest.
    fn metadata(&self) -> ComponentMetadataV1;
    /// Produces a request and its durable non-secret locator.
    fn build_submit_request(
        &self,
        config: &ProviderConfig,
        request: &Value,
        minted: &HostMintedValuesV1,
    ) -> ComponentResultV1<PreparedTaskV2>;
    /// Interprets submit acceptance without host funds policy.
    fn parse_submit_response(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<SubmitOutcomeV2>;
    /// Uses the original model, id and persisted locator to describe a query.
    fn build_observe_request(
        &self,
        config: &ProviderConfig,
        upstream_model: &str,
        upstream_task_id: &str,
        locator: &TaskLocatorV2,
    ) -> ComponentResultV1<HttpRequestDescriptor>;
    /// Preserves execution, artifacts and simultaneous metering facts.
    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV2>;
    /// Describes a second artifact lookup where the dialect requires one.
    fn build_artifact_request(
        &self,
        config: &ProviderConfig,
        locator: &TaskLocatorV2,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>>;
    /// Renders success with explicit host time and public identity.
    fn render_success(
        &self,
        observation: &TaskObservationV2,
        fetched: Option<&HttpResponseParts>,
        context: &TaskRenderContextV2,
    ) -> ComponentResultV1<Value>;
    /// Maps an explicit terminal failure to the stable error vocabulary.
    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<ErrorEnvelope>;
}
