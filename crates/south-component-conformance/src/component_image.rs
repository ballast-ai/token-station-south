//! The image-v1 typed translation seam (`docs/design/2026-09-30-image-world.md` §7, §9.2).
//!
//! Mirrors the `image-adapter-v1` world as [`crate::EmbeddingsComponentV1`] mirrors
//! `embeddings-adapter-v1`, with the `json` payloads already parsed. [`ImageOutcomeV1`] carries an
//! [`ErrorEnvelope`], so it lives here rather than in `south-contracts` (record §15, the task v2
//! division). `healthcheck` is absent for the reason [`crate::ProviderComponentV1`] gives.
//!
//! Every method is pure. The host owns bytes, credentials, HTTP, the clock, pricing, persistence
//! and delivery; the component owns the upstream request, where the artifacts sit, what the
//! upstream reported, and the client body's shape.

use serde_json::Value;
use south_contracts::image::{
    ImageArtifactV1, ImageCallContextV1, ImageMeteringV1, ImageModelCapabilitiesV1,
    ImageRenderContextV1, PreparedImageCallV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorEnvelope, ProviderConfig};

use crate::ComponentResultV1;

/// The outcome of one upstream round (§9.2).
#[derive(Clone, Debug, PartialEq)]
pub enum ImageOutcomeV1 {
    /// Deliverable artifacts; `metering` carries every required fact of every form the call
    /// requires.
    Succeeded {
        /// The artifacts, in the order the client body lists them.
        artifacts: Vec<ImageArtifactV1>,
        /// What the upstream reported.
        metering: ImageMeteringV1,
        /// Round facts `render` needs (bounded like `state`); `null` when none.
        extras: Value,
    },
    /// The upstream explicitly refused: no output, no charge.
    Rejected {
        /// What the caller is answered with.
        error: ErrorEnvelope,
    },
    /// The upstream charged but produced nothing deliverable.
    ChargedFailure {
        /// What the caller is answered with.
        error: ErrorEnvelope,
        /// What the upstream reported it charged.
        metering: ImageMeteringV1,
    },
    /// Cannot be decided: a 5xx, an unparsable body, a missing required fact.
    Unknown {
        /// Why, for the operator; never upstream content.
        reason: String,
        /// The envelope to answer with, when the component has one.
        error: Option<ErrorEnvelope>,
    },
}

/// What `render` returns: the client body as a template whose artifact positions are
/// `{"$south.artifact": {"index", "as"}}` (§7), as compact JSON text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageRenderedV1 {
    /// The template.
    pub template: String,
}

/// Pure image dialect operations over explicit host inputs.
pub trait ImageComponentV1 {
    /// Reports the identity verified against the component manifest.
    fn metadata(&self) -> ComponentMetadataV1;
    /// The dialect-bound declarations of every model the configuration lists (§7, R-1).
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>>;
    /// Builds the upstream request from the request view. An `Err` is a pre-dispatch refusal:
    /// nothing was sent and nothing may be reserved.
    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &ImageCallContextV1,
    ) -> ComponentResultV1<PreparedImageCallV1>;
    /// Judges one upstream round from its response view.
    fn parse_response(&self, state: &Value, response: &Value) -> ComponentResultV1<ImageOutcomeV1>;
    /// Renders the client body from the succeeded rounds' outcomes. A failure here, after the
    /// upstream succeeded, is an `unknown` call, never a pre-dispatch refusal.
    fn render(
        &self,
        state: &Value,
        outcomes: &[ImageOutcomeV1],
        context: &ImageRenderContextV1,
    ) -> ComponentResultV1<ImageRenderedV1>;
}
