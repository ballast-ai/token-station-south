//! The seam between the conformance suite and a provider component.
//!
//! The suite is written against these traits rather than against a WASM
//! runtime, exactly as the donor gates were: it lets gates ① and ② exist and
//! be proven to bite before `south-provider-runtime` can instantiate a
//! component, and it lets a component author run the same suite against a
//! native build in their own CI without a WASM toolchain. What the runtime
//! does with the boundary — serialize, call, deserialize, and turn a trap
//! into an [`ErrorEnvelope`] — is what makes it an implementation of these
//! traits (S3).
//!
//! Each method mirrors one function of the `provider-adapter-v2` world, with
//! the `json` payloads already parsed into the Canonical IR. `healthcheck` is
//! absent: it carries no fixture — a component exporting the world at all is
//! a load-time fact the runtime checks, not something a fixture can express.

use serde_json::Value;
use south_contracts::{HostMintedValuesV1, TaskObservationV1};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{
    ChatRequest, ChatResponse, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts,
    ModelCapability, ProviderConfig, StreamEvent,
};

/// What a component returns. The error is the component's own
/// [`ErrorEnvelope`], which is also what the runtime reports when a component
/// traps.
pub type ComponentResultV1<T> = Result<T, ErrorEnvelope>;

/// One provider stream, mid-parse.
///
/// Streaming is the only stateful part of the ABI. A chunk off the socket is
/// not a whole frame — SSE or binary eventstream alike — so a component must
/// hold the tail until the rest arrives. The v2 world expresses that as
/// instance state behind a plain `parse-stream-chunk` function, which means
/// the host instantiates a component per stream; this trait hands out a fresh
/// parser per stream rather than pretending the call is pure.
///
/// Chunks are raw bounded bytes (S0 ruling D2). A split may land inside a
/// UTF-8 sequence; a parser buffers bytes and decodes only complete frames.
///
/// `Send` because host streams cross worker threads; a wasm guest is
/// single-threaded, so the bound costs it nothing.
pub trait StreamParserV1: Send {
    /// Consumes one fragment and emits whatever complete events it completed.
    ///
    /// Zero events is a normal answer: the fragment ended mid-frame.
    ///
    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with.
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>>;

    /// Flushes a clean transport EOF. The v2 world has no separate finish
    /// export, so the runtime represents EOF as an empty fragment, which a
    /// successful network read can never produce.
    ///
    /// # Errors
    ///
    /// Returns a typed protocol failure when buffered state cannot finish.
    fn finish(&mut self) -> ComponentResultV1<Vec<StreamEvent>> {
        self.parse_chunk(&[])
    }
}

/// Southbound: the Canonical IR, in and out of one provider's HTTP dialect.
pub trait ProviderComponentV1 {
    fn metadata(&self) -> ComponentMetadataV1;

    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with.
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ModelCapability>>;

    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with.
    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor>;

    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with.
    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse>;

    /// Maps a failed upstream response onto the stable error catalog.
    ///
    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with. A component
    /// that cannot classify a failure returns
    /// `Ok(ErrorEnvelope { code: Internal, .. })` rather than `Err`; `Err`
    /// here means the mapping itself broke.
    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope>;

    /// A parser for one stream. Called once per exchange.
    fn stream_parser(&self) -> Box<dyn StreamParserV1>;
}

/// Southbound, task shape: one media task's lifecycle, as pure translation.
///
/// Mirrors the `task-adapter-v1` world exactly as [`ProviderComponentV1`]
/// mirrors `provider-adapter-v2`, with the `json` payloads already parsed.
/// `healthcheck` is absent for the same reason.
///
/// Every method is pure. A component has no network, no clock and no memory
/// across calls, so it can neither send its own request nor decide that time
/// has run out — the host does both. What the component owns is the dialect:
/// where each stage's request goes, and what each response means.
///
/// The two surveys behind these signatures (`2026-09-19-task-world-fit-survey`
/// and `2026-09-19-task-vocabulary-fit-survey`) measured them against six real
/// provider families; every parameter here is one a family was shown to need.
pub trait TaskComponentV1 {
    fn metadata(&self) -> ComponentMetadataV1;

    /// Stage one: where to submit, and what to send.
    ///
    /// `minted` carries the values only the host can produce and only the
    /// dialect knows where to put. **The component places them; it never
    /// invents them** — a component generating its own task id destroys the
    /// host's idempotency anchor, and the reservation pays for both copies of
    /// the work.
    ///
    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with.
    fn build_submit_request(
        &self,
        config: &ProviderConfig,
        request: &Value,
        minted: &HostMintedValuesV1,
    ) -> ComponentResultV1<HttpRequestDescriptor>;

    /// Stage one, second half: what the creation response means.
    ///
    /// Four outcomes, not "an id or an error": see [`SubmitOutcomeV1`].
    ///
    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with. Note that a
    /// business-layer refusal is `Ok(Rejected)`, not `Err` — `Err` means the
    /// component could not process the response at all.
    fn parse_submit_response(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<SubmitOutcomeV1>;

    /// Stage two: where to look for this task's state.
    ///
    /// `upstream_model` is a parameter because one provider can serve several
    /// task shapes on several paths, and which one a task used is per-task
    /// state rather than provider configuration.
    ///
    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with.
    fn build_observe_request(
        &self,
        config: &ProviderConfig,
        upstream_model: &str,
        upstream_task_id: &str,
    ) -> ComponentResultV1<HttpRequestDescriptor>;

    /// Stage two, second half: what one query response says about the task.
    ///
    /// The rules are frozen (2026-08-27 vocabulary record, D3): an
    /// unrecognised status word is `Unknown`, never a synthesised failure; a
    /// non-2xx query is `Unknown` because a failed *query* is not a failed
    /// *task*; and `provider-expired` requires an explicit upstream statement.
    ///
    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with.
    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV1>;

    /// Stage three: whether the host must fetch something before rendering.
    ///
    /// `None` means "the observation already has everything", not "not
    /// supported". Most families are `None`; one answers with an id whose
    /// download URL takes another call.
    ///
    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with.
    fn build_artifact_request(
        &self,
        config: &ProviderConfig,
        observation: &TaskObservationV1,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>>;

    /// Stage three, second half: the client-facing success body.
    ///
    /// `fetched` is the artifact response when the previous method asked for
    /// one. `minted` reaches here because artifacts are addressed by the
    /// host's own task id, which the component places and never invents.
    ///
    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with.
    fn render_success(
        &self,
        observation: &TaskObservationV1,
        fetched: Option<&HttpResponseParts>,
        minted: &HostMintedValuesV1,
    ) -> ComponentResultV1<Value>;

    /// A terminal failure, mapped onto the stable error catalog.
    ///
    /// Whether to try another upstream is the host's decision and is never
    /// expressed here.
    ///
    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with. As with
    /// `map_provider_error`, an unclassifiable failure is `Ok(Internal)`
    /// rather than `Err`.
    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV1,
    ) -> ComponentResultV1<ErrorEnvelope>;
}

/// How a submit attempt ended, as the dialect reads it off the wire.
///
/// Deliberately not `Result<String, _>`: two of these four have no
/// representation in "an id or an error", and one of the two is
/// funds-critical (2026-09-19 world fit survey, D1).
#[derive(Clone, Debug, PartialEq)]
pub enum SubmitOutcomeV1 {
    /// The upstream accepted and issued a task id.
    Accepted(String),
    /// Submit *was* the terminal answer — some dialects answer a creation
    /// synchronously and issue no pollable id at all.
    AcceptedTerminal(Value),
    /// HTTP 2xx carrying a business-layer refusal. Nothing is running, so the
    /// host may release.
    Rejected(ErrorEnvelope),
    /// The response shape is unrecognised. **The upstream may have accepted
    /// the work and may be billing for it**, so the host keeps the
    /// reservation and lets reconciliation settle it.
    ///
    /// This variant is why the outcome is not a `Result`. "We cannot tell" is
    /// not "it failed".
    Unknown,
}
