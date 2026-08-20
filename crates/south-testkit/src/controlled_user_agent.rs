//! Assembled-executor runner and reference executor for the controlled user-agent suite.

use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use http::StatusCode;
use south_contracts::{
    BufferedHttpResponseV1, ControlledUserAgentV1, CredentialSlotV1, StreamChunkV1,
    StreamingResponseHeadV1, TransportErrorV1,
};
use south_core::{
    AsyncHttpTransport, AsyncStreamingTransport, CredentialResolutionFuture, CredentialResolver,
    OpenedByteStreamV1, PreparedHttpRequestV1, SecretValue, StreamByteSourceV1,
    StreamChunkFutureV1, StreamOpenErrorV1, StreamingOpenFutureV1, TransportFuture,
    execute_provider_call_v1, open_streaming_provider_call_v1,
};
use south_provider_conformance::{
    CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_ID, CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_VERSION,
    ControlledUserAgentCaseIdV1, ControlledUserAgentExpectedOutcomeV1,
    ControlledUserAgentFixtureV1, ControlledUserAgentUpstreamV1, FAKE_BEARER_SECRET_V1,
    ProviderCallCountV1, ProviderCallFailureCodeV1, controlled_user_agent_fixtures_v1,
};
use tokio_util::sync::CancellationToken;

use crate::{map_contract_error, map_provider_call_error, parse_reference_input};

/// Five cases multiplied by the ten closed controlled user-agent mismatch categories.
pub const MAX_CONTROLLED_USER_AGENT_MISMATCHES_V1: usize = 50;

/// A boxed, cancellation-safe assembled controlled user-agent executor future.
pub type AssembledControlledUserAgentExecutionFutureV1<'a> =
    Pin<Box<dyn Future<Output = ControlledUserAgentObservationV1> + Send + 'a>>;

/// A host-assembled controlled user-agent call path exercised by the public runner.
pub trait AssembledControlledUserAgentExecutorV1: Send + Sync {
    /// Executes one immutable canonical controlled user-agent fixture.
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ControlledUserAgentFixtureV1,
    ) -> AssembledControlledUserAgentExecutionFutureV1<'a>;
}

/// Adapter-reported resolver, transport, and wire user-agent boundary evidence.
///
/// `wire_user_agent_exact` is measured at the adapter's real transport boundary: whether the
/// request about to be sent carries a user-agent byte for byte equal to the value the request
/// declared. Like every adapter-reported value, a passing report alone is insufficient for host
/// verification; the adoption review must confirm the wiring.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ControlledUserAgentEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    wire_user_agent_exact: bool,
}

impl ControlledUserAgentEvidenceV1 {
    /// Constructs evidence and saturates both raw call counts.
    #[must_use]
    pub const fn new(
        resolver_calls: usize,
        transport_calls: usize,
        wire_user_agent_exact: bool,
    ) -> Self {
        Self {
            resolver_calls: ProviderCallCountV1::from_usize(resolver_calls),
            transport_calls: ProviderCallCountV1::from_usize(transport_calls),
            wire_user_agent_exact,
        }
    }

    /// Returns the saturated resolver call category.
    #[must_use]
    pub const fn resolver_calls(&self) -> ProviderCallCountV1 {
        self.resolver_calls
    }

    /// Returns the saturated transport call category.
    #[must_use]
    pub const fn transport_calls(&self) -> ProviderCallCountV1 {
        self.transport_calls
    }

    /// Returns whether the transport boundary carried the declared user-agent byte for byte.
    ///
    /// A presence claim, so it stays `false` when no transport call ever observed a wire — and
    /// when the wire was observed but nothing was declared.
    #[must_use]
    pub const fn wire_user_agent_exact(&self) -> bool {
        self.wire_user_agent_exact
    }
}

impl fmt::Debug for ControlledUserAgentEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlledUserAgentEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field("wire_user_agent_exact", &self.wire_user_agent_exact)
            .finish()
    }
}

enum ControlledUserAgentObservedOutcomeV1 {
    Response(BufferedHttpResponseV1),
    Opened { head: StreamingResponseHeadV1, chunks: Vec<StreamChunkV1> },
    Failure(ProviderCallFailureCodeV1),
}

impl fmt::Debug for ControlledUserAgentObservedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(response) => formatter.debug_tuple("Response").field(response).finish(),
            Self::Opened { head, chunks } => formatter
                .debug_struct("Opened")
                .field("head", head)
                .field("chunk_count", &chunks.len())
                .finish(),
            Self::Failure(code) => formatter.debug_tuple("Failure").field(code).finish(),
        }
    }
}

/// An observed controlled user-agent terminal shape plus adapter-reported evidence.
pub struct ControlledUserAgentObservationV1 {
    outcome: ControlledUserAgentObservedOutcomeV1,
    evidence: ControlledUserAgentEvidenceV1,
}

impl ControlledUserAgentObservationV1 {
    /// Constructs a buffered-response observation from an already bounded response.
    #[must_use]
    pub const fn response(
        response: BufferedHttpResponseV1,
        evidence: ControlledUserAgentEvidenceV1,
    ) -> Self {
        Self { outcome: ControlledUserAgentObservedOutcomeV1::Response(response), evidence }
    }

    /// Constructs an opened-stream observation from the bounded head and pulled chunks.
    #[must_use]
    pub const fn opened(
        head: StreamingResponseHeadV1,
        chunks: Vec<StreamChunkV1>,
        evidence: ControlledUserAgentEvidenceV1,
    ) -> Self {
        Self { outcome: ControlledUserAgentObservedOutcomeV1::Opened { head, chunks }, evidence }
    }

    /// Constructs a failed observation from a closed known failure code.
    #[must_use]
    pub const fn failure(
        code: ProviderCallFailureCodeV1,
        evidence: ControlledUserAgentEvidenceV1,
    ) -> Self {
        Self { outcome: ControlledUserAgentObservedOutcomeV1::Failure(code), evidence }
    }

    /// Returns the bounded response when this is a buffered-response observation.
    #[must_use]
    pub const fn response_value(&self) -> Option<&BufferedHttpResponseV1> {
        match &self.outcome {
            ControlledUserAgentObservedOutcomeV1::Response(response) => Some(response),
            _ => None,
        }
    }

    /// Returns the observed head when this is an opened-stream observation.
    #[must_use]
    pub const fn opened_head(&self) -> Option<&StreamingResponseHeadV1> {
        match &self.outcome {
            ControlledUserAgentObservedOutcomeV1::Opened { head, .. } => Some(head),
            _ => None,
        }
    }

    /// Returns the observed chunks when this is an opened-stream observation.
    #[must_use]
    pub fn opened_chunks(&self) -> Option<&[StreamChunkV1]> {
        match &self.outcome {
            ControlledUserAgentObservedOutcomeV1::Opened { chunks, .. } => Some(chunks),
            _ => None,
        }
    }

    /// Returns the closed code when this is a failure observation.
    #[must_use]
    pub const fn failure_code(&self) -> Option<ProviderCallFailureCodeV1> {
        match &self.outcome {
            ControlledUserAgentObservedOutcomeV1::Failure(code) => Some(*code),
            _ => None,
        }
    }

    /// Returns adapter-reported boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ControlledUserAgentEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ControlledUserAgentObservationV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlledUserAgentObservationV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// The closed reasons why an observed controlled user-agent case can differ from its fixture.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ControlledUserAgentMismatchCategoryV1 {
    /// Response versus opened stream versus failure differed.
    OutcomeKind,
    /// Stable failure code differed.
    ErrorCode,
    /// Response or head status differed.
    Status,
    /// Buffered response body bytes differed.
    Body,
    /// `content-type` value or presence differed.
    ContentType,
    /// `retry-after` value or presence differed.
    RetryAfter,
    /// Chunk count or chunk bytes differed.
    ChunkBytes,
    /// Resolver call category differed.
    ResolverCallCount,
    /// Transport call category differed.
    TransportCallCount,
    /// Wire user-agent byte-for-byte evidence differed.
    WireUserAgent,
}

fixed_debug!(ControlledUserAgentMismatchCategoryV1 {
    OutcomeKind => "OutcomeKind",
    ErrorCode => "ErrorCode",
    Status => "Status",
    Body => "Body",
    ContentType => "ContentType",
    RetryAfter => "RetryAfter",
    ChunkBytes => "ChunkBytes",
    ResolverCallCount => "ResolverCallCount",
    TransportCallCount => "TransportCallCount",
    WireUserAgent => "WireUserAgent",
});

/// One case/category mismatch without expected or observed payload values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ControlledUserAgentMismatchV1 {
    case_id: ControlledUserAgentCaseIdV1,
    category: ControlledUserAgentMismatchCategoryV1,
}

impl ControlledUserAgentMismatchV1 {
    /// Returns the canonical case that mismatched.
    #[must_use]
    pub const fn case_id(&self) -> ControlledUserAgentCaseIdV1 {
        self.case_id
    }

    /// Returns the closed mismatch category.
    #[must_use]
    pub const fn category(&self) -> ControlledUserAgentMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for ControlledUserAgentMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlledUserAgentMismatchV1")
            .field("case_id", &self.case_id)
            .field("category", &self.category)
            .finish()
    }
}

/// A successful report for the complete canonical controlled user-agent suite.
pub struct ControlledUserAgentConformanceReportV1 {
    passed_case_ids: Vec<ControlledUserAgentCaseIdV1>,
}

impl ControlledUserAgentConformanceReportV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_VERSION
    }

    /// Returns all passed cases in canonical table order.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[ControlledUserAgentCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for ControlledUserAgentConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlledUserAgentConformanceReportV1")
            .field("suite_id", &CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_ID)
            .field("suite_version", &CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_VERSION)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// A complete bounded mismatch report for the evaluated canonical controlled user-agent suite.
pub struct ControlledUserAgentConformanceFailureV1 {
    evaluated_case_count: usize,
    mismatches: Vec<ControlledUserAgentMismatchV1>,
}

impl ControlledUserAgentConformanceFailureV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_VERSION
    }

    /// Returns how many canonical cases completed evaluation.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Returns every case/category mismatch in canonical evaluation order.
    #[must_use]
    pub fn mismatches(&self) -> &[ControlledUserAgentMismatchV1] {
        &self.mismatches
    }
}

impl fmt::Debug for ControlledUserAgentConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlledUserAgentConformanceFailureV1")
            .field("suite_id", &CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_ID)
            .field("suite_version", &CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_VERSION)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs all canonical controlled user-agent cases sequentially without failing fast.
///
/// Every caller must wrap the entire runner in an outer watchdog. This function intentionally has
/// no internal timeout, so a broken assembled executor may remain pending forever. The watchdog
/// must own the complete structured future tree so timeout drops all in-progress executor work
/// without leaving a detached task.
pub async fn run_controlled_user_agent_conformance_v1(
    executor: &dyn AssembledControlledUserAgentExecutorV1,
) -> Result<ControlledUserAgentConformanceReportV1, ControlledUserAgentConformanceFailureV1> {
    let fixtures = controlled_user_agent_fixtures_v1();
    let mut passed_case_ids = Vec::with_capacity(fixtures.len());
    let mut mismatches = Vec::with_capacity(MAX_CONTROLLED_USER_AGENT_MISMATCHES_V1);

    for fixture in fixtures {
        let mismatch_count_before_case = mismatches.len();
        let observation = executor.execute_case(fixture).await;
        compare_controlled_user_agent_outcome(fixture, &observation, &mut mismatches);
        compare_controlled_user_agent_evidence(fixture, &observation, &mut mismatches);
        if mismatches.len() == mismatch_count_before_case {
            passed_case_ids.push(fixture.case_id());
        }
    }

    if mismatches.is_empty() {
        Ok(ControlledUserAgentConformanceReportV1 { passed_case_ids })
    } else {
        debug_assert!(mismatches.len() <= MAX_CONTROLLED_USER_AGENT_MISMATCHES_V1);
        Err(ControlledUserAgentConformanceFailureV1 {
            evaluated_case_count: fixtures.len(),
            mismatches,
        })
    }
}

fn compare_controlled_user_agent_outcome(
    fixture: &ControlledUserAgentFixtureV1,
    observation: &ControlledUserAgentObservationV1,
    mismatches: &mut Vec<ControlledUserAgentMismatchV1>,
) {
    match (fixture.expected().outcome(), &observation.outcome) {
        (
            ControlledUserAgentExpectedOutcomeV1::Response {
                status,
                body,
                content_type,
                retry_after,
            },
            ControlledUserAgentObservedOutcomeV1::Response(response),
        ) => {
            record_if(
                response.status().as_u16() != *status,
                fixture,
                ControlledUserAgentMismatchCategoryV1::Status,
                mismatches,
            );
            record_if(
                response.body().as_bytes() != body.as_bytes(),
                fixture,
                ControlledUserAgentMismatchCategoryV1::Body,
                mismatches,
            );
            record_if(
                response.content_type() != *content_type,
                fixture,
                ControlledUserAgentMismatchCategoryV1::ContentType,
                mismatches,
            );
            record_if(
                response.retry_after() != *retry_after,
                fixture,
                ControlledUserAgentMismatchCategoryV1::RetryAfter,
                mismatches,
            );
        }
        (
            ControlledUserAgentExpectedOutcomeV1::Opened {
                status,
                content_type,
                retry_after,
                chunks,
            },
            ControlledUserAgentObservedOutcomeV1::Opened { head, chunks: observed_chunks },
        ) => {
            record_if(
                head.status().as_u16() != *status,
                fixture,
                ControlledUserAgentMismatchCategoryV1::Status,
                mismatches,
            );
            record_if(
                head.content_type() != *content_type,
                fixture,
                ControlledUserAgentMismatchCategoryV1::ContentType,
                mismatches,
            );
            record_if(
                head.retry_after() != *retry_after,
                fixture,
                ControlledUserAgentMismatchCategoryV1::RetryAfter,
                mismatches,
            );
            let chunks_match = observed_chunks.len() == chunks.len()
                && observed_chunks
                    .iter()
                    .zip(chunks.iter())
                    .all(|(observed, expected)| observed.as_bytes() == *expected);
            record_if(
                !chunks_match,
                fixture,
                ControlledUserAgentMismatchCategoryV1::ChunkBytes,
                mismatches,
            );
        }
        (
            ControlledUserAgentExpectedOutcomeV1::Failure { code: expected },
            ControlledUserAgentObservedOutcomeV1::Failure(observed),
        ) => record_if(
            expected != observed,
            fixture,
            ControlledUserAgentMismatchCategoryV1::ErrorCode,
            mismatches,
        ),
        _ => record(fixture, ControlledUserAgentMismatchCategoryV1::OutcomeKind, mismatches),
    }
}

fn compare_controlled_user_agent_evidence(
    fixture: &ControlledUserAgentFixtureV1,
    observation: &ControlledUserAgentObservationV1,
    mismatches: &mut Vec<ControlledUserAgentMismatchV1>,
) {
    let expected = fixture.expected().evidence();
    let observed = observation.evidence();
    record_if(
        expected.resolver_calls() != observed.resolver_calls(),
        fixture,
        ControlledUserAgentMismatchCategoryV1::ResolverCallCount,
        mismatches,
    );
    record_if(
        expected.transport_calls() != observed.transport_calls(),
        fixture,
        ControlledUserAgentMismatchCategoryV1::TransportCallCount,
        mismatches,
    );
    record_if(
        expected.wire_user_agent_exact() != observed.wire_user_agent_exact(),
        fixture,
        ControlledUserAgentMismatchCategoryV1::WireUserAgent,
        mismatches,
    );
}

fn record_if(
    condition: bool,
    fixture: &ControlledUserAgentFixtureV1,
    category: ControlledUserAgentMismatchCategoryV1,
    mismatches: &mut Vec<ControlledUserAgentMismatchV1>,
) {
    if condition {
        record(fixture, category, mismatches);
    }
}

fn record(
    fixture: &ControlledUserAgentFixtureV1,
    category: ControlledUserAgentMismatchCategoryV1,
    mismatches: &mut Vec<ControlledUserAgentMismatchV1>,
) {
    mismatches.push(ControlledUserAgentMismatchV1 { case_id: fixture.case_id(), category });
}

/// A deterministic assembled controlled user-agent executor built from real `south-core`
/// orchestration and fake ports.
///
/// The buffered and rejection cases run through `execute_provider_call_v1`; the streaming case
/// runs through `open_streaming_provider_call_v1`. `wire_user_agent_exact` is measured on the
/// prepared request at the fake transport boundary, mirroring what a real adapter must measure on
/// its wire.
pub struct ReferenceAssembledControlledUserAgentExecutorV1;

impl ReferenceAssembledControlledUserAgentExecutorV1 {
    /// Creates an independent reference controlled user-agent executor.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for ReferenceAssembledControlledUserAgentExecutorV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl AssembledControlledUserAgentExecutorV1 for ReferenceAssembledControlledUserAgentExecutorV1 {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ControlledUserAgentFixtureV1,
    ) -> AssembledControlledUserAgentExecutionFutureV1<'a> {
        Box::pin(async move { execute_reference_controlled_user_agent_case(fixture).await })
    }
}

/// Turns a fixture's raw declaration into the user-agent the request will carry, or a failure.
///
/// Three outcomes, and keeping them distinct is the whole job — the same discipline the
/// controlled-query reference learned when a host executor routed the empty declaration through
/// the constructor and silently converted the measured-probe case into a second zero-call
/// rejection. `Ok(Some(_))` is a declared user-agent. `Err(_)` is a declaration the contract
/// refuses. `Ok(None)` is *no* declaration, a legitimate request shape and not a failure, and it
/// must never reach `ControlledUserAgentV1::try_from_static`.
fn declare_reference_user_agent(
    fixture: &ControlledUserAgentFixtureV1,
) -> Result<Option<ControlledUserAgentV1>, ProviderCallFailureCodeV1> {
    let Some(declared) = fixture.declared_user_agent() else {
        return Ok(None);
    };
    ControlledUserAgentV1::try_from_static(declared).map(Some).map_err(map_contract_error)
}

async fn execute_reference_controlled_user_agent_case(
    fixture: &ControlledUserAgentFixtureV1,
) -> ControlledUserAgentObservationV1 {
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let transport_calls = Arc::new(AtomicUsize::new(0));
    let wire_user_agent_exact = Arc::new(AtomicBool::new(false));

    let (binding, request) = match parse_reference_input(fixture.input()) {
        Ok(parsed) => parsed,
        Err(code) => {
            return ControlledUserAgentObservationV1::failure(
                code,
                ControlledUserAgentEvidenceV1::new(0, 0, false),
            );
        }
    };
    let declared_user_agent = match declare_reference_user_agent(fixture) {
        Ok(user_agent) => user_agent,
        Err(code) => {
            return ControlledUserAgentObservationV1::failure(
                code,
                ControlledUserAgentEvidenceV1::new(0, 0, false),
            );
        }
    };
    let request = match declared_user_agent {
        Some(user_agent) => request.with_user_agent(user_agent),
        None => request,
    };

    let resolver = BearerSecretResolver { calls: Arc::clone(&resolver_calls) };
    let transport = UserAgentRecordingTransport {
        calls: Arc::clone(&transport_calls),
        upstream: fixture.upstream(),
        declared_user_agent,
        wire_user_agent_exact: Arc::clone(&wire_user_agent_exact),
    };
    let cancellation = CancellationToken::new();
    let evidence = || {
        ControlledUserAgentEvidenceV1::new(
            resolver_calls.load(Ordering::SeqCst),
            transport_calls.load(Ordering::SeqCst),
            wire_user_agent_exact.load(Ordering::SeqCst),
        )
    };

    match fixture.upstream() {
        ControlledUserAgentUpstreamV1::Stream(_) => {
            let opened = open_streaming_provider_call_v1(
                &binding,
                &request,
                &resolver,
                &transport,
                None,
                &cancellation,
            )
            .await;
            let mut call = match opened {
                Ok(call) => call,
                Err(error) => {
                    return ControlledUserAgentObservationV1::failure(
                        map_provider_call_error(&error),
                        evidence(),
                    );
                }
            };
            let head = call.head().clone();
            let mut chunks = Vec::new();
            while let Some(result) = call.next_chunk().await {
                match result {
                    Ok(chunk) => chunks.push(chunk),
                    Err(error) => {
                        return ControlledUserAgentObservationV1::failure(
                            map_stream_read_terminal(error),
                            evidence(),
                        );
                    }
                }
            }
            ControlledUserAgentObservationV1::opened(head, chunks, evidence())
        }
        ControlledUserAgentUpstreamV1::Response(_) | ControlledUserAgentUpstreamV1::NotReached => {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
            let result = execute_provider_call_v1(
                &binding,
                &request,
                &resolver,
                &transport,
                deadline,
                &cancellation,
            )
            .await;
            match result {
                Ok(response) => ControlledUserAgentObservationV1::response(response, evidence()),
                Err(error) => ControlledUserAgentObservationV1::failure(
                    map_provider_call_error(&error),
                    evidence(),
                ),
            }
        }
    }
}

// The canonical table scripts only clean EOF streams, so a mid-stream terminal here means the
// reference wiring itself is broken. Fail closed with the context-free request fallback rather
// than panicking or widening the frozen code set.
const fn map_stream_read_terminal(
    _error: south_contracts::StreamReadErrorV1,
) -> ProviderCallFailureCodeV1 {
    ProviderCallFailureCodeV1::RequestFailed
}

struct BearerSecretResolver {
    calls: Arc<AtomicUsize>,
}

impl CredentialResolver for BearerSecretResolver {
    fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(SecretValue::new(FAKE_BEARER_SECRET_V1.to_owned())) })
    }
}

struct UserAgentRecordingTransport<'fixture> {
    calls: Arc<AtomicUsize>,
    upstream: &'fixture ControlledUserAgentUpstreamV1,
    declared_user_agent: Option<ControlledUserAgentV1>,
    wire_user_agent_exact: Arc<AtomicBool>,
}

impl UserAgentRecordingTransport<'_> {
    /// Measures the presence claim on the request that is about to be sent.
    ///
    /// The claim is "the wire carried the user-agent this request declared", so it needs both
    /// halves: a declaration to compare against, and a prepared value equal to it byte for byte.
    /// A request that declared nothing can never satisfy it, even though it reaches this boundary
    /// — there is no declared user-agent to have observed. That asymmetry is deliberate, and it is
    /// what [`ControlledUserAgentCaseIdV1::UserAgentFreeRequestReachesTheWire`] measures: a probe
    /// must read the prepared request to answer at all, so one that hardcodes `true` is caught
    /// there.
    fn record_wire_user_agent(&self, request: &PreparedHttpRequestV1<'_>) {
        let wire = request.user_agent().map(ControlledUserAgentV1::as_str);
        let declared = self.declared_user_agent.map(ControlledUserAgentV1::as_str);
        debug_assert!(
            declared.is_some() || wire.is_none(),
            "a request declaring no user-agent must carry none"
        );
        // `declared.is_some()` is not redundant with the equality: without it a request that
        // declared nothing and carried nothing would compare `None == None` and claim `true`,
        // which is an absence claim, not the presence claim this evidence field defines.
        let exact = declared.is_some() && wire == declared;
        self.wire_user_agent_exact.store(exact, Ordering::SeqCst);
    }
}

impl AsyncHttpTransport for UserAgentRecordingTransport<'_> {
    fn execute<'a>(
        &'a self,
        request: &'a PreparedHttpRequestV1<'_>,
        _remaining_timeout: Duration,
    ) -> TransportFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.record_wire_user_agent(request);
        match self.upstream {
            ControlledUserAgentUpstreamV1::Response(raw) => {
                let status = StatusCode::from_u16(raw.status());
                let body = raw.body().as_bytes().to_vec();
                let content_type = raw.content_type().map(str::to_owned);
                let retry_after = raw.retry_after().map(str::to_owned);
                Box::pin(async move {
                    let status = status.map_err(|_| TransportErrorV1::ResponseMetadataInvalid)?;
                    BufferedHttpResponseV1::try_from_parts(status, body, content_type, retry_after)
                })
            }
            // A stream fixture must never reach the buffered boundary, and `NotReached` must not
            // reach any boundary. Fail closed with the context-free request code.
            ControlledUserAgentUpstreamV1::Stream(_)
            | ControlledUserAgentUpstreamV1::NotReached => {
                Box::pin(async { Err(TransportErrorV1::RequestFailed) })
            }
        }
    }
}

impl AsyncStreamingTransport for UserAgentRecordingTransport<'_> {
    fn open<'a>(&'a self, request: &'a PreparedHttpRequestV1<'_>) -> StreamingOpenFutureV1<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.record_wire_user_agent(request);
        match self.upstream {
            ControlledUserAgentUpstreamV1::Stream(raw) => {
                let status = StatusCode::from_u16(raw.head().status());
                let content_type = raw.head().content_type().map(str::to_owned);
                let retry_after = raw.head().retry_after().map(str::to_owned);
                let source = ScriptedChunkSource { chunks: raw.chunks(), next_index: 0 };
                Box::pin(async move {
                    let status = status.map_err(|_| TransportErrorV1::RequestFailed)?;
                    let head =
                        StreamingResponseHeadV1::try_from_parts(status, content_type, retry_after)?;
                    OpenedByteStreamV1::try_new(head, Box::new(source))
                        .map_err(StreamOpenErrorV1::Transport)
                })
            }
            // A buffered fixture must never reach the streaming boundary, and `NotReached` must
            // not reach any boundary. Fail closed with the context-free request code.
            ControlledUserAgentUpstreamV1::Response(_)
            | ControlledUserAgentUpstreamV1::NotReached => Box::pin(async {
                Err(StreamOpenErrorV1::Transport(TransportErrorV1::RequestFailed))
            }),
        }
    }
}

struct ScriptedChunkSource {
    chunks: &'static [&'static [u8]],
    next_index: usize,
}

impl StreamByteSourceV1 for ScriptedChunkSource {
    fn next_chunk(&mut self) -> StreamChunkFutureV1<'_> {
        // Script-state advancement stays inside the returned future, mirroring the
        // cancellation-safety contract real transports must honor.
        Box::pin(async move {
            let chunk = self.chunks.get(self.next_index).copied()?;
            self.next_index += 1;
            Some(StreamChunkV1::try_new(Bytes::from_static(chunk)))
        })
    }
}
