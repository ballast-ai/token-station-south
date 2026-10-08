//! The embeddings-v1 typed translation seam (`docs/design/2026-09-30-embeddings-contract.md`).
//!
//! Mirrors the `embeddings-adapter-v1` world exactly as [`crate::TaskComponentV2`] mirrors
//! `task-adapter-v2`, with the `json` payloads already parsed. The IR-bearing
//! [`PreparedEmbeddingsV1`] lives here rather than in `south-contracts`, the same division as
//! task-v2 (record D1). `healthcheck` is absent for the reason [`crate::ProviderComponentV1`]
//! gives: it carries no fixture, and a component exporting the world at all is a load-time fact.
//!
//! Every method is pure. The host owns credentials, HTTP, bytes, vector extraction, pricing and
//! settlement; the component owns where the request goes, what its body is, where the vectors
//! sit and what the erased response skeleton says about usage.

use serde_json::Value;
use south_contracts::{
    EmbeddingsEstimateV1, EmbeddingsFailureOutcomeV1, EmbeddingsParsedV1, EmbeddingsRequestV1,
    VectorLocatorV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{
    ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts, ProviderConfig,
};

use crate::ComponentResultV1;

/// What `build-embeddings-request` returns (record §3).
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedEmbeddingsV1 {
    /// The request the host authorizes, seals and sends.
    pub descriptor: HttpRequestDescriptor,
    /// The fallback estimate and the component's tightening of the host bound (§7).
    pub estimate: EmbeddingsEstimateV1,
    /// Where the vectors sit in a 2xx body of this request (§5).
    pub vectors: VectorLocatorV1,
    /// Request-body paths the host must not rewrite, with task contract 6's syntax and meaning:
    /// `None` = the component makes no statement, so the host must not add body fields at all;
    /// `Some(vec![])` = none are reserved.
    pub immutable_body_paths: Option<Vec<String>>,
    /// Handed back unchanged by the host to `parse-embeddings-response`, never interpreted;
    /// at most `MAX_EMBEDDINGS_PARSE_CONTEXT_BYTES` serialized.
    pub parse_context: Value,
}

/// Pure embeddings dialect operations over explicit host inputs.
pub trait EmbeddingsComponentV1 {
    /// Reports the identity verified against the component manifest.
    fn metadata(&self) -> ComponentMetadataV1;
    /// Builds the upstream request. A request the model or dialect cannot serve is a capability
    /// error here, before the host admits anything.
    fn build_embeddings_request(
        &self,
        config: &ProviderConfig,
        request: &EmbeddingsRequestV1,
    ) -> ComponentResultV1<PreparedEmbeddingsV1>;
    /// Reads a 2xx whose vectors the host has replaced with `null` per the prepared locator. A
    /// body that cannot yield the facts is an error, never a zero.
    fn parse_embeddings_response(
        &self,
        parts: &HttpResponseParts,
        parse_context: &Value,
    ) -> ComponentResultV1<EmbeddingsParsedV1>;
    /// Classifies a non-2xx: `Rejected` only when the response proves the upstream produced
    /// nothing, with the envelope the caller is answered with.
    fn map_provider_error(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<(EmbeddingsFailureOutcomeV1, ErrorEnvelope)>;
}
