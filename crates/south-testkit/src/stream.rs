//! Assembled-executor runner and reference executor for the provider-stream suite.

use std::{
    fmt,
    future::{Future, pending},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use bytes::Bytes;
use http::StatusCode;
use south_contracts::{
    StreamChunkV1, StreamReadErrorV1, StreamRejectedV1, StreamingResponseHeadV1, TransportErrorV1,
};
use south_core::{
    AsyncStreamingTransport, OpenedByteStreamV1, PreparedHttpRequestV1, ProviderCallErrorV1,
    StreamByteSourceV1, StreamChunkFutureV1, StreamOpenErrorV1, StreamingCallV1,
    StreamingOpenFutureV1, open_streaming_provider_call_v1,
};
use south_provider_conformance::{
    PROVIDER_STREAM_CONFORMANCE_DEADLINE_OFFSET_V1, PROVIDER_STREAM_CONFORMANCE_IDLE_TIMEOUT_V1,
    PROVIDER_STREAM_CONFORMANCE_SUITE_ID, PROVIDER_STREAM_CONFORMANCE_SUITE_VERSION,
    ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderStreamCaseIdV1,
    ProviderStreamControlV1, ProviderStreamExpectedOutcomeV1, ProviderStreamFixtureV1,
    ProviderStreamRawHeadV1, ProviderStreamTerminalV1, ProviderStreamUpstreamV1,
    provider_stream_fixtures_v1,
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::{PendingDropFlag, ReferenceResolver, map_provider_call_error, parse_reference_input};

/// Nine cases multiplied by the thirteen closed stream mismatch categories.
pub const MAX_PROVIDER_STREAM_MISMATCHES_V1: usize = 117;

/// A boxed, cancellation-safe assembled stream-executor future.
pub type AssembledStreamExecutionFutureV1<'a> =
    Pin<Box<dyn Future<Output = ProviderStreamObservationV1> + Send + 'a>>;

/// A host-assembled streaming call path exercised by the public stream conformance runner.
pub trait AssembledProviderStreamExecutorV1: Send + Sync {
    /// Executes one immutable canonical streaming fixture.
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderStreamFixtureV1,
    ) -> AssembledStreamExecutionFutureV1<'a>;
}

/// Adapter-reported resolver, transport, and stream-phase boundary evidence.
///
/// The transport pending-drop flag covers every pending transport-owned future: the open future
/// and any in-flight chunk pull dropped by cancellation or the caller deadline.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderStreamEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    resolver_future_dropped_while_pending: bool,
    transport_future_dropped_while_pending: bool,
    chunks_pulled: usize,
    poststream_error_code: Option<StreamReadErrorV1>,
}

impl ProviderStreamEvidenceV1 {
    /// Constructs evidence, saturating both boundary call counts and keeping chunk pulls exact.
    #[must_use]
    pub const fn new(
        resolver_calls: usize,
        transport_calls: usize,
        resolver_future_dropped_while_pending: bool,
        transport_future_dropped_while_pending: bool,
        chunks_pulled: usize,
        poststream_error_code: Option<StreamReadErrorV1>,
    ) -> Self {
        Self {
            resolver_calls: ProviderCallCountV1::from_usize(resolver_calls),
            transport_calls: ProviderCallCountV1::from_usize(transport_calls),
            resolver_future_dropped_while_pending,
            transport_future_dropped_while_pending,
            chunks_pulled,
            poststream_error_code,
        }
    }

    /// Returns the saturated resolver call category.
    #[must_use]
    pub const fn resolver_calls(&self) -> ProviderCallCountV1 {
        self.resolver_calls
    }

    /// Returns the saturated transport open call category.
    #[must_use]
    pub const fn transport_calls(&self) -> ProviderCallCountV1 {
        self.transport_calls
    }

    /// Returns whether a pending resolver future was dropped.
    #[must_use]
    pub const fn resolver_future_dropped_while_pending(&self) -> bool {
        self.resolver_future_dropped_while_pending
    }

    /// Returns whether a pending transport-owned future was dropped.
    #[must_use]
    pub const fn transport_future_dropped_while_pending(&self) -> bool {
        self.transport_future_dropped_while_pending
    }

    /// Returns the exact number of successful chunk pulls.
    #[must_use]
    pub const fn chunks_pulled(&self) -> usize {
        self.chunks_pulled
    }

    /// Returns the observed terminal stream error, or `None` for a clean EOF or no stream.
    #[must_use]
    pub const fn poststream_error_code(&self) -> Option<StreamReadErrorV1> {
        self.poststream_error_code
    }
}

impl fmt::Debug for ProviderStreamEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderStreamEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field(
                "resolver_future_dropped_while_pending",
                &self.resolver_future_dropped_while_pending,
            )
            .field(
                "transport_future_dropped_while_pending",
                &self.transport_future_dropped_while_pending,
            )
            .field("chunks_pulled", &self.chunks_pulled)
            .field("poststream_error_code", &self.poststream_error_code)
            .finish()
    }
}

enum ProviderStreamObservedOutcomeV1 {
    Opened { head: StreamingResponseHeadV1, chunks: Vec<StreamChunkV1> },
    Rejected(StreamRejectedV1),
    Failure(ProviderCallFailureCodeV1),
}

impl fmt::Debug for ProviderStreamObservedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Opened { head, chunks } => formatter
                .debug_struct("Opened")
                .field("head", head)
                .field("chunk_count", &chunks.len())
                .finish(),
            Self::Rejected(rejected) => formatter.debug_tuple("Rejected").field(rejected).finish(),
            Self::Failure(code) => formatter.debug_tuple("Failure").field(code).finish(),
        }
    }
}

/// An observed streaming terminal shape plus adapter-reported evidence.
pub struct ProviderStreamObservationV1 {
    outcome: ProviderStreamObservedOutcomeV1,
    evidence: ProviderStreamEvidenceV1,
}

impl ProviderStreamObservationV1 {
    /// Constructs an opened-stream observation from the bounded head and pulled chunks.
    #[must_use]
    pub const fn opened(
        head: StreamingResponseHeadV1,
        chunks: Vec<StreamChunkV1>,
        evidence: ProviderStreamEvidenceV1,
    ) -> Self {
        Self { outcome: ProviderStreamObservedOutcomeV1::Opened { head, chunks }, evidence }
    }

    /// Constructs a rejected-exchange observation from the bounded contract rejection.
    #[must_use]
    pub const fn rejected(rejected: StreamRejectedV1, evidence: ProviderStreamEvidenceV1) -> Self {
        Self { outcome: ProviderStreamObservedOutcomeV1::Rejected(rejected), evidence }
    }

    /// Constructs a failed observation from a closed known failure code.
    #[must_use]
    pub const fn failure(
        code: ProviderCallFailureCodeV1,
        evidence: ProviderStreamEvidenceV1,
    ) -> Self {
        Self { outcome: ProviderStreamObservedOutcomeV1::Failure(code), evidence }
    }

    /// Returns the observed head when this is an opened-stream observation.
    #[must_use]
    pub const fn opened_head(&self) -> Option<&StreamingResponseHeadV1> {
        match &self.outcome {
            ProviderStreamObservedOutcomeV1::Opened { head, .. } => Some(head),
            _ => None,
        }
    }

    /// Returns the observed chunks when this is an opened-stream observation.
    #[must_use]
    pub fn opened_chunks(&self) -> Option<&[StreamChunkV1]> {
        match &self.outcome {
            ProviderStreamObservedOutcomeV1::Opened { chunks, .. } => Some(chunks),
            _ => None,
        }
    }

    /// Returns the bounded rejection when this is a rejected observation.
    #[must_use]
    pub const fn rejected_value(&self) -> Option<&StreamRejectedV1> {
        match &self.outcome {
            ProviderStreamObservedOutcomeV1::Rejected(rejected) => Some(rejected),
            _ => None,
        }
    }

    /// Returns the closed code when this is a failure observation.
    #[must_use]
    pub const fn failure_code(&self) -> Option<ProviderCallFailureCodeV1> {
        match &self.outcome {
            ProviderStreamObservedOutcomeV1::Failure(code) => Some(*code),
            _ => None,
        }
    }

    /// Returns adapter-reported boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderStreamEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ProviderStreamObservationV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderStreamObservationV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// The closed reasons why an observed streaming case can differ from its fixture.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderStreamMismatchCategoryV1 {
    /// Opened versus rejected versus failure differed.
    OutcomeKind,
    /// Stable failure code differed.
    ErrorCode,
    /// Head status differed.
    Status,
    /// `content-type` value or presence differed.
    ContentType,
    /// `retry-after` value or presence differed.
    RetryAfter,
    /// Rejected error body bytes differed.
    RejectedBody,
    /// Chunk count or chunk bytes differed.
    ChunkBytes,
    /// Resolver call category differed.
    ResolverCallCount,
    /// Transport open call category differed.
    TransportCallCount,
    /// Pending resolver drop evidence differed.
    ResolverPendingDrop,
    /// Pending transport-owned future drop evidence differed.
    TransportPendingDrop,
    /// Exact successful pull count differed.
    ChunksPulled,
    /// Terminal stream error evidence differed.
    PoststreamErrorCode,
}

fixed_debug!(ProviderStreamMismatchCategoryV1 {
    OutcomeKind => "OutcomeKind",
    ErrorCode => "ErrorCode",
    Status => "Status",
    ContentType => "ContentType",
    RetryAfter => "RetryAfter",
    RejectedBody => "RejectedBody",
    ChunkBytes => "ChunkBytes",
    ResolverCallCount => "ResolverCallCount",
    TransportCallCount => "TransportCallCount",
    ResolverPendingDrop => "ResolverPendingDrop",
    TransportPendingDrop => "TransportPendingDrop",
    ChunksPulled => "ChunksPulled",
    PoststreamErrorCode => "PoststreamErrorCode",
});

/// One case/category mismatch without expected or observed payload values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderStreamMismatchV1 {
    case_id: ProviderStreamCaseIdV1,
    category: ProviderStreamMismatchCategoryV1,
}

impl ProviderStreamMismatchV1 {
    /// Returns the canonical case that mismatched.
    #[must_use]
    pub const fn case_id(&self) -> ProviderStreamCaseIdV1 {
        self.case_id
    }

    /// Returns the closed mismatch category.
    #[must_use]
    pub const fn category(&self) -> ProviderStreamMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for ProviderStreamMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderStreamMismatchV1")
            .field("case_id", &self.case_id)
            .field("category", &self.category)
            .finish()
    }
}

/// A successful report for the complete canonical streaming suite.
pub struct ProviderStreamConformanceReportV1 {
    passed_case_ids: Vec<ProviderStreamCaseIdV1>,
}

impl ProviderStreamConformanceReportV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_STREAM_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_STREAM_CONFORMANCE_SUITE_VERSION
    }

    /// Returns all passed cases in canonical table order.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[ProviderStreamCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for ProviderStreamConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderStreamConformanceReportV1")
            .field("suite_id", &PROVIDER_STREAM_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_STREAM_CONFORMANCE_SUITE_VERSION)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// A complete bounded mismatch report for the evaluated canonical streaming suite.
pub struct ProviderStreamConformanceFailureV1 {
    evaluated_case_count: usize,
    mismatches: Vec<ProviderStreamMismatchV1>,
}

impl ProviderStreamConformanceFailureV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_STREAM_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_STREAM_CONFORMANCE_SUITE_VERSION
    }

    /// Returns how many canonical cases completed evaluation.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Returns every case/category mismatch in canonical evaluation order.
    #[must_use]
    pub fn mismatches(&self) -> &[ProviderStreamMismatchV1] {
        &self.mismatches
    }
}

impl fmt::Debug for ProviderStreamConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderStreamConformanceFailureV1")
            .field("suite_id", &PROVIDER_STREAM_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_STREAM_CONFORMANCE_SUITE_VERSION)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs all canonical provider-stream cases sequentially without failing fast.
///
/// Every caller must wrap the entire runner and any clock driver in an outer watchdog. This
/// function intentionally has no internal timeout, so a broken assembled executor may remain
/// pending forever. The watchdog must own the complete structured future tree so timeout drops
/// all in-progress executor work without leaving a detached task.
pub async fn run_provider_stream_conformance_v1(
    executor: &dyn AssembledProviderStreamExecutorV1,
) -> Result<ProviderStreamConformanceReportV1, ProviderStreamConformanceFailureV1> {
    let fixtures = provider_stream_fixtures_v1();
    let mut passed_case_ids = Vec::with_capacity(fixtures.len());
    let mut mismatches = Vec::with_capacity(MAX_PROVIDER_STREAM_MISMATCHES_V1);

    for fixture in fixtures {
        let mismatch_count_before_case = mismatches.len();
        let observation = executor.execute_case(fixture).await;
        compare_stream_outcome(fixture, &observation, &mut mismatches);
        compare_stream_evidence(fixture, &observation, &mut mismatches);
        if mismatches.len() == mismatch_count_before_case {
            passed_case_ids.push(fixture.case_id());
        }
    }

    if mismatches.is_empty() {
        Ok(ProviderStreamConformanceReportV1 { passed_case_ids })
    } else {
        debug_assert!(mismatches.len() <= MAX_PROVIDER_STREAM_MISMATCHES_V1);
        Err(ProviderStreamConformanceFailureV1 { evaluated_case_count: fixtures.len(), mismatches })
    }
}

fn compare_stream_outcome(
    fixture: &ProviderStreamFixtureV1,
    observation: &ProviderStreamObservationV1,
    mismatches: &mut Vec<ProviderStreamMismatchV1>,
) {
    match (fixture.expected().outcome(), &observation.outcome) {
        (
            ProviderStreamExpectedOutcomeV1::Opened { status, content_type, retry_after, chunks },
            ProviderStreamObservedOutcomeV1::Opened { head, chunks: observed_chunks },
        ) => {
            compare_head(fixture, head, *status, *content_type, *retry_after, mismatches);
            let chunks_match = observed_chunks.len() == chunks.len()
                && observed_chunks
                    .iter()
                    .zip(chunks.iter())
                    .all(|(observed, expected)| observed.as_bytes() == *expected);
            record_if(
                !chunks_match,
                fixture,
                ProviderStreamMismatchCategoryV1::ChunkBytes,
                mismatches,
            );
        }
        (
            ProviderStreamExpectedOutcomeV1::Rejected { status, content_type, retry_after, body },
            ProviderStreamObservedOutcomeV1::Rejected(rejected),
        ) => {
            compare_head(
                fixture,
                rejected.head(),
                *status,
                *content_type,
                *retry_after,
                mismatches,
            );
            record_if(
                rejected.body() != *body,
                fixture,
                ProviderStreamMismatchCategoryV1::RejectedBody,
                mismatches,
            );
        }
        (
            ProviderStreamExpectedOutcomeV1::Failure { code: expected },
            ProviderStreamObservedOutcomeV1::Failure(observed),
        ) => record_if(
            expected != observed,
            fixture,
            ProviderStreamMismatchCategoryV1::ErrorCode,
            mismatches,
        ),
        _ => record(fixture, ProviderStreamMismatchCategoryV1::OutcomeKind, mismatches),
    }
}

fn compare_head(
    fixture: &ProviderStreamFixtureV1,
    head: &StreamingResponseHeadV1,
    status: u16,
    content_type: Option<&'static str>,
    retry_after: Option<&'static str>,
    mismatches: &mut Vec<ProviderStreamMismatchV1>,
) {
    record_if(
        head.status().as_u16() != status,
        fixture,
        ProviderStreamMismatchCategoryV1::Status,
        mismatches,
    );
    record_if(
        head.content_type() != content_type,
        fixture,
        ProviderStreamMismatchCategoryV1::ContentType,
        mismatches,
    );
    record_if(
        head.retry_after() != retry_after,
        fixture,
        ProviderStreamMismatchCategoryV1::RetryAfter,
        mismatches,
    );
}

fn compare_stream_evidence(
    fixture: &ProviderStreamFixtureV1,
    observation: &ProviderStreamObservationV1,
    mismatches: &mut Vec<ProviderStreamMismatchV1>,
) {
    let expected = fixture.expected().evidence();
    let observed = observation.evidence();
    record_if(
        expected.resolver_calls() != observed.resolver_calls(),
        fixture,
        ProviderStreamMismatchCategoryV1::ResolverCallCount,
        mismatches,
    );
    record_if(
        expected.transport_calls() != observed.transport_calls(),
        fixture,
        ProviderStreamMismatchCategoryV1::TransportCallCount,
        mismatches,
    );
    record_if(
        expected.resolver_future_dropped_while_pending()
            != observed.resolver_future_dropped_while_pending(),
        fixture,
        ProviderStreamMismatchCategoryV1::ResolverPendingDrop,
        mismatches,
    );
    record_if(
        expected.transport_future_dropped_while_pending()
            != observed.transport_future_dropped_while_pending(),
        fixture,
        ProviderStreamMismatchCategoryV1::TransportPendingDrop,
        mismatches,
    );
    record_if(
        expected.chunks_pulled() != observed.chunks_pulled(),
        fixture,
        ProviderStreamMismatchCategoryV1::ChunksPulled,
        mismatches,
    );
    record_if(
        expected.poststream_error_code() != observed.poststream_error_code(),
        fixture,
        ProviderStreamMismatchCategoryV1::PoststreamErrorCode,
        mismatches,
    );
}

fn record_if(
    condition: bool,
    fixture: &ProviderStreamFixtureV1,
    category: ProviderStreamMismatchCategoryV1,
    mismatches: &mut Vec<ProviderStreamMismatchV1>,
) {
    if condition {
        record(fixture, category, mismatches);
    }
}

fn record(
    fixture: &ProviderStreamFixtureV1,
    category: ProviderStreamMismatchCategoryV1,
    mismatches: &mut Vec<ProviderStreamMismatchV1>,
) {
    mismatches.push(ProviderStreamMismatchV1 { case_id: fixture.case_id(), category });
}

/// A deterministic assembled stream executor built from real `south-core` orchestration and fake
/// ports.
///
/// Cases must execute sequentially. The idle-stall and deadline chunk starts each grant one
/// consumable notification permit for the caller-owned virtual-clock driver. If a caller aborts
/// after one of those pulls starts but before consuming its notification, it must discard this
/// executor and construct a new one; reusing an executor with an abandoned permit could associate
/// that stale permit with a later clock-driven case.
pub struct ReferenceAssembledProviderStreamExecutorV1 {
    idle_stall_started: Arc<Notify>,
    deadline_chunk_started: Arc<Notify>,
}

impl ReferenceAssembledProviderStreamExecutorV1 {
    /// Creates an independent reference stream executor.
    #[must_use]
    pub fn new() -> Self {
        Self {
            idle_stall_started: Arc::new(Notify::new()),
            deadline_chunk_started: Arc::new(Notify::new()),
        }
    }

    /// Consumes one notification that the idle-stall fixture's silent pull has started.
    pub async fn idle_stall_started(&self) {
        self.idle_stall_started.notified().await;
    }

    /// Consumes one notification that the deadline fixture's pending pull has started.
    pub async fn deadline_chunk_started(&self) {
        self.deadline_chunk_started.notified().await;
    }
}

impl Default for ReferenceAssembledProviderStreamExecutorV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl AssembledProviderStreamExecutorV1 for ReferenceAssembledProviderStreamExecutorV1 {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderStreamFixtureV1,
    ) -> AssembledStreamExecutionFutureV1<'a> {
        Box::pin(async move { self.execute_reference_stream_case(fixture).await })
    }
}

impl ReferenceAssembledProviderStreamExecutorV1 {
    async fn execute_reference_stream_case(
        &self,
        fixture: &ProviderStreamFixtureV1,
    ) -> ProviderStreamObservationV1 {
        let resolver_calls = Arc::new(AtomicUsize::new(0));
        let transport_calls = Arc::new(AtomicUsize::new(0));
        let resolver_dropped = Arc::new(AtomicBool::new(false));
        let transport_dropped = Arc::new(AtomicBool::new(false));

        let parsed = parse_reference_input(fixture.input());
        let (binding, request) = match parsed {
            Ok(parsed) => parsed,
            Err(code) => {
                return ProviderStreamObservationV1::failure(
                    code,
                    ProviderStreamEvidenceV1::new(0, 0, false, false, 0, None),
                );
            }
        };

        let cancellation = CancellationToken::new();
        let cancel_signal = Arc::new(Notify::new());
        let stall_started = match fixture.control() {
            ProviderStreamControlV1::CancelWhileChunkPending => Arc::clone(&cancel_signal),
            ProviderStreamControlV1::AdvanceIdleWhileChunkPending => {
                Arc::clone(&self.idle_stall_started)
            }
            ProviderStreamControlV1::ExpireWhileChunkPending => {
                Arc::clone(&self.deadline_chunk_started)
            }
            ProviderStreamControlV1::Complete => Arc::new(Notify::new()),
        };
        let resolver = ReferenceResolver {
            calls: Arc::clone(&resolver_calls),
            pending: false,
            started: Mutex::new(None),
            dropped: Arc::clone(&resolver_dropped),
        };
        let transport = ReferenceStreamTransport {
            calls: Arc::clone(&transport_calls),
            upstream: fixture.upstream(),
            stall_started,
            pending_dropped: Arc::clone(&transport_dropped),
        };
        let deadline =
            if matches!(fixture.control(), ProviderStreamControlV1::ExpireWhileChunkPending) {
                Some(tokio::time::Instant::now() + PROVIDER_STREAM_CONFORMANCE_DEADLINE_OFFSET_V1)
            } else {
                None
            };

        let evidence = |chunks_pulled: usize, poststream: Option<StreamReadErrorV1>| {
            ProviderStreamEvidenceV1::new(
                resolver_calls.load(Ordering::SeqCst),
                transport_calls.load(Ordering::SeqCst),
                resolver_dropped.load(Ordering::SeqCst),
                transport_dropped.load(Ordering::SeqCst),
                chunks_pulled,
                poststream,
            )
        };

        let opened = open_streaming_provider_call_v1(
            &binding,
            &request,
            &resolver,
            &transport,
            deadline,
            &cancellation,
        )
        .await;
        let mut call = match opened {
            Ok(call) => call,
            Err(ProviderCallErrorV1::Rejected(rejected)) => {
                return ProviderStreamObservationV1::rejected(rejected, evidence(0, None));
            }
            Err(error) => {
                return ProviderStreamObservationV1::failure(
                    map_provider_call_error(&error),
                    evidence(0, None),
                );
            }
        };

        let head = call.head().clone();
        let mut chunks = Vec::new();
        let mut poststream = None;
        let pull_all = pull_until_terminal(&mut call, &mut chunks, &mut poststream);
        if matches!(fixture.control(), ProviderStreamControlV1::CancelWhileChunkPending) {
            let cancel_driver = async {
                cancel_signal.notified().await;
                cancellation.cancel();
            };
            let ((), ()) = tokio::join!(pull_all, cancel_driver);
        } else {
            pull_all.await;
        }

        let observed_evidence = evidence(chunks.len(), poststream);
        ProviderStreamObservationV1::opened(head, chunks, observed_evidence)
    }
}

async fn pull_until_terminal(
    call: &mut StreamingCallV1,
    chunks: &mut Vec<StreamChunkV1>,
    poststream: &mut Option<StreamReadErrorV1>,
) {
    loop {
        match call.next_chunk().await {
            Some(Ok(chunk)) => chunks.push(chunk),
            Some(Err(error)) => {
                *poststream = Some(error);
                // Terminal errors must stick: one extra pull proves later pulls yield `None`;
                // any extra delivery or error would corrupt the reported evidence.
                if let Some(after_terminal) = call.next_chunk().await {
                    match after_terminal {
                        Ok(chunk) => chunks.push(chunk),
                        Err(error) => *poststream = Some(error),
                    }
                }
                break;
            }
            None => break,
        }
    }
}

struct ReferenceStreamTransport<'fixture> {
    calls: Arc<AtomicUsize>,
    upstream: &'fixture ProviderStreamUpstreamV1,
    stall_started: Arc<Notify>,
    pending_dropped: Arc<AtomicBool>,
}

impl AsyncStreamingTransport for ReferenceStreamTransport<'_> {
    fn open<'a>(&'a self, _request: &'a PreparedHttpRequestV1<'_>) -> StreamingOpenFutureV1<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.upstream {
            ProviderStreamUpstreamV1::Stream(raw) => {
                let head = raw_head(raw.head());
                let source = ReferenceStreamSource {
                    chunks: raw.chunks(),
                    next_index: 0,
                    terminal: raw.terminal(),
                    stall_started: Arc::clone(&self.stall_started),
                    pending_dropped: Arc::clone(&self.pending_dropped),
                };
                Box::pin(async move {
                    OpenedByteStreamV1::try_new(head?, Box::new(source))
                        .map_err(StreamOpenErrorV1::Transport)
                })
            }
            ProviderStreamUpstreamV1::Rejected(raw) => {
                let head = raw_head(raw.head());
                let body = raw.body().to_vec();
                Box::pin(async move {
                    Err(StreamOpenErrorV1::Rejected(StreamRejectedV1::new(head?, body)))
                })
            }
            ProviderStreamUpstreamV1::TransportFailure(error) => {
                let error = *error;
                Box::pin(async move { Err(StreamOpenErrorV1::Transport(error)) })
            }
            ProviderStreamUpstreamV1::NotReached => Box::pin(async {
                Err(StreamOpenErrorV1::Transport(TransportErrorV1::RequestFailed))
            }),
        }
    }
}

// Canonical fixtures are immutable and their raw head values are production-parsed in the
// conformance package's public table tests, so this conversion cannot fail for the shipped
// table. Fail closed with the context-free request fallback rather than panicking.
fn raw_head(raw: &ProviderStreamRawHeadV1) -> Result<StreamingResponseHeadV1, TransportErrorV1> {
    let status = StatusCode::from_u16(raw.status()).map_err(|_| TransportErrorV1::RequestFailed)?;
    StreamingResponseHeadV1::try_from_parts(
        status,
        raw.content_type().map(str::to_owned),
        raw.retry_after().map(str::to_owned),
    )
}

struct ReferenceStreamSource {
    chunks: &'static [&'static [u8]],
    next_index: usize,
    terminal: ProviderStreamTerminalV1,
    stall_started: Arc<Notify>,
    pending_dropped: Arc<AtomicBool>,
}

impl StreamByteSourceV1 for ReferenceStreamSource {
    fn next_chunk(&mut self) -> StreamChunkFutureV1<'_> {
        // All script-state advancement stays inside the returned future: a pull future that is
        // dropped before completing must not consume its scripted chunk, mirroring the
        // cancellation-safety contract real transports must honor.
        Box::pin(async move {
            if let Some(chunk) = self.chunks.get(self.next_index).copied() {
                self.next_index += 1;
                return Some(StreamChunkV1::try_new(Bytes::from_static(chunk)));
            }
            match self.terminal {
                ProviderStreamTerminalV1::CleanEof => None,
                ProviderStreamTerminalV1::BreakWithReadFailure => {
                    Some(Err(StreamReadErrorV1::StreamReadFailed))
                }
                ProviderStreamTerminalV1::IdleStall => {
                    self.stall_started.notify_one();
                    tokio::time::sleep(PROVIDER_STREAM_CONFORMANCE_IDLE_TIMEOUT_V1).await;
                    Some(Err(StreamReadErrorV1::StreamIdleTimeout))
                }
                ProviderStreamTerminalV1::PendingForever => {
                    let _drop_flag = PendingDropFlag(Arc::clone(&self.pending_dropped));
                    self.stall_started.notify_one();
                    pending().await
                }
            }
        })
    }
}
