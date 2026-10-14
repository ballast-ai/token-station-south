//! The speech-v1 typed translation seam (`docs/design/2026-09-30-speech-world.md` §7, §9).
//!
//! Mirrors the `speech-adapter-v1` world as [`crate::ImageComponentV1`] mirrors
//! `image-adapter-v1`, with the `json` payloads already parsed. [`SpeechOutcomeV1`] carries an
//! [`ErrorEnvelope`], so it lives here rather than in `south-contracts`, as the image world's
//! outcome does (speech record, S-S-1 implementation, interpretation 1). `healthcheck` is absent
//! for the reason [`crate::ProviderComponentV1`] gives.
//!
//! Every method is pure. The host owns bytes, credentials, HTTP, the clock, pricing, persistence
//! and delivery; the component owns the upstream request, where the audio sits or what the client
//! receives, and what the upstream reported.

use serde_json::Value;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechArtifactV1, SpeechCallContextV1, SpeechMeteringV1,
    SpeechModelCapabilitiesV1, TranscriptDeliveryV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorEnvelope, ProviderConfig};

use crate::ComponentResultV1;

/// What a `succeeded` call delivers: synthesized audio, or a recognized transcript (§6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpeechDeliveryV1 {
    /// Where the audio is; the host reads, decodes, wraps and verifies it (§6, §10).
    Audio(SpeechArtifactV1),
    /// The document the client receives, rendered by the component (§6 ASR delivery).
    Transcript(TranscriptDeliveryV1),
}

/// The outcome of one upstream round (§7: the image record §9.2's four outcomes).
#[derive(Clone, Debug, PartialEq)]
pub enum SpeechOutcomeV1 {
    /// A deliverable answer; `metering` carries every required fact of every declared form
    /// (`check_required_metering_v1`; `seconds` may be `null`, §9).
    Succeeded {
        /// The audio or the transcript.
        delivery: SpeechDeliveryV1,
        /// What the upstream reported, or the request count where the upstream bills by it.
        metering: SpeechMeteringV1,
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
        metering: SpeechMeteringV1,
    },
    /// Cannot be decided: a 5xx, an unparsable body, a missing required fact, no audio.
    Unknown {
        /// Why, for the operator; never upstream content.
        reason: String,
        /// The envelope to answer with, when the component has one.
        error: Option<ErrorEnvelope>,
    },
}

/// Pure speech dialect operations over explicit host inputs.
pub trait SpeechComponentV1 {
    /// Reports the identity verified against the component manifest.
    fn metadata(&self) -> ComponentMetadataV1;
    /// The declarations of every model the configuration lists: its operations (the dialect's,
    /// intersected with the dialect words on its row, §4) and the metering forms it can report.
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<SpeechModelCapabilitiesV1>>;
    /// Builds the upstream request from the request view. An `Err` is a pre-dispatch refusal:
    /// nothing was sent and nothing may be reserved (an unsupported format, voice or operation).
    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1>;
    /// Judges one upstream round from its response view.
    fn parse_response(&self, state: &Value, response: &Value)
    -> ComponentResultV1<SpeechOutcomeV1>;
}
