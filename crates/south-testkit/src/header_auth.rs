//! Assembled-executor runner and reference executor for the header-auth suite.

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
    BearerAuthV1, BufferedHttpResponseV1, CredentialSlotV1, ProviderAuthV1, SecretHeaderV1,
    StreamChunkV1, StreamingResponseHeadV1, TransportErrorV1,
};
use south_core::{
    AsyncHttpTransport, AsyncStreamingTransport, CredentialResolutionFuture, CredentialResolver,
    OpenedByteStreamV1, PreparedHttpRequestV1, SecretValue, StreamByteSourceV1,
    StreamChunkFutureV1, StreamOpenErrorV1, StreamingOpenFutureV1, TransportFuture,
    execute_provider_call_v1, open_streaming_provider_call_v1,
};
use south_provider_conformance::{
    FAKE_HEADER_SECRET_V1, HEADER_AUTH_CONFORMANCE_SUITE_ID, HEADER_AUTH_CONFORMANCE_SUITE_VERSION,
    HeaderAuthCaseIdV1, HeaderAuthExpectedOutcomeV1, HeaderAuthFixtureV1, HeaderAuthUpstreamV1,
    ProviderCallCountV1, ProviderCallFailureCodeV1, header_auth_fixtures_v1,
};
use tokio_util::sync::CancellationToken;

use crate::{map_provider_call_error, parse_reference_input_with_auth};

/// Three cases multiplied by the eleven closed header-auth mismatch categories.
pub const MAX_HEADER_AUTH_MISMATCHES_V1: usize = 33;

/// A boxed, cancellation-safe assembled header-auth executor future.
pub type AssembledHeaderAuthExecutionFutureV1<'a> =
    Pin<Box<dyn Future<Output = HeaderAuthObservationV1> + Send + 'a>>;

/// A host-assembled header-secret call path exercised by the public header-auth runner.
pub trait AssembledHeaderAuthExecutorV1: Send + Sync {
    /// Executes one immutable canonical header-auth fixture.
    fn execute_case<'a>(
        &'a self,
        fixture: &'a HeaderAuthFixtureV1,
    ) -> AssembledHeaderAuthExecutionFutureV1<'a>;
}

/// Adapter-reported resolver, transport, and wire-shape boundary evidence.
///
/// The two wire-shape booleans are measured at the adapter's real transport boundary: whether the
/// declared sanctioned header carried the resolved secret byte for byte, and whether no
/// `authorization` header existed on the wire. Like every adapter-reported value, a passing
/// report alone is insufficient for host verification; the adoption review must confirm the
/// wiring.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HeaderAuthEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    sanctioned_header_exact: bool,
    authorization_header_absent: bool,
}

impl HeaderAuthEvidenceV1 {
    /// Constructs evidence and saturates both raw call counts.
    #[must_use]
    pub const fn new(
        resolver_calls: usize,
        transport_calls: usize,
        sanctioned_header_exact: bool,
        authorization_header_absent: bool,
    ) -> Self {
        Self {
            resolver_calls: ProviderCallCountV1::from_usize(resolver_calls),
            transport_calls: ProviderCallCountV1::from_usize(transport_calls),
            sanctioned_header_exact,
            authorization_header_absent,
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

    /// Returns whether the sanctioned header carried the resolved secret byte for byte.
    #[must_use]
    pub const fn sanctioned_header_exact(&self) -> bool {
        self.sanctioned_header_exact
    }

    /// Returns whether no `authorization` header existed at the transport boundary.
    #[must_use]
    pub const fn authorization_header_absent(&self) -> bool {
        self.authorization_header_absent
    }
}

impl fmt::Debug for HeaderAuthEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HeaderAuthEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field("sanctioned_header_exact", &self.sanctioned_header_exact)
            .field("authorization_header_absent", &self.authorization_header_absent)
            .finish()
    }
}

enum HeaderAuthObservedOutcomeV1 {
    Response(BufferedHttpResponseV1),
    Opened { head: StreamingResponseHeadV1, chunks: Vec<StreamChunkV1> },
    Failure(ProviderCallFailureCodeV1),
}

impl fmt::Debug for HeaderAuthObservedOutcomeV1 {
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

/// An observed header-auth terminal shape plus adapter-reported evidence.
pub struct HeaderAuthObservationV1 {
    outcome: HeaderAuthObservedOutcomeV1,
    evidence: HeaderAuthEvidenceV1,
}

impl HeaderAuthObservationV1 {
    /// Constructs a buffered-response observation from an already bounded response.
    #[must_use]
    pub const fn response(
        response: BufferedHttpResponseV1,
        evidence: HeaderAuthEvidenceV1,
    ) -> Self {
        Self { outcome: HeaderAuthObservedOutcomeV1::Response(response), evidence }
    }

    /// Constructs an opened-stream observation from the bounded head and pulled chunks.
    #[must_use]
    pub const fn opened(
        head: StreamingResponseHeadV1,
        chunks: Vec<StreamChunkV1>,
        evidence: HeaderAuthEvidenceV1,
    ) -> Self {
        Self { outcome: HeaderAuthObservedOutcomeV1::Opened { head, chunks }, evidence }
    }

    /// Constructs a failed observation from a closed known failure code.
    #[must_use]
    pub const fn failure(code: ProviderCallFailureCodeV1, evidence: HeaderAuthEvidenceV1) -> Self {
        Self { outcome: HeaderAuthObservedOutcomeV1::Failure(code), evidence }
    }

    /// Returns the bounded response when this is a buffered-response observation.
    #[must_use]
    pub const fn response_value(&self) -> Option<&BufferedHttpResponseV1> {
        match &self.outcome {
            HeaderAuthObservedOutcomeV1::Response(response) => Some(response),
            _ => None,
        }
    }

    /// Returns the observed head when this is an opened-stream observation.
    #[must_use]
    pub const fn opened_head(&self) -> Option<&StreamingResponseHeadV1> {
        match &self.outcome {
            HeaderAuthObservedOutcomeV1::Opened { head, .. } => Some(head),
            _ => None,
        }
    }

    /// Returns the observed chunks when this is an opened-stream observation.
    #[must_use]
    pub fn opened_chunks(&self) -> Option<&[StreamChunkV1]> {
        match &self.outcome {
            HeaderAuthObservedOutcomeV1::Opened { chunks, .. } => Some(chunks),
            _ => None,
        }
    }

    /// Returns the closed code when this is a failure observation.
    #[must_use]
    pub const fn failure_code(&self) -> Option<ProviderCallFailureCodeV1> {
        match &self.outcome {
            HeaderAuthObservedOutcomeV1::Failure(code) => Some(*code),
            _ => None,
        }
    }

    /// Returns adapter-reported boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &HeaderAuthEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for HeaderAuthObservationV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HeaderAuthObservationV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// The closed reasons why an observed header-auth case can differ from its fixture.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HeaderAuthMismatchCategoryV1 {
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
    /// Sanctioned-header wire-shape evidence differed.
    SanctionedHeader,
    /// Authorization-absence wire-shape evidence differed.
    AuthorizationPresence,
}

fixed_debug!(HeaderAuthMismatchCategoryV1 {
    OutcomeKind => "OutcomeKind",
    ErrorCode => "ErrorCode",
    Status => "Status",
    Body => "Body",
    ContentType => "ContentType",
    RetryAfter => "RetryAfter",
    ChunkBytes => "ChunkBytes",
    ResolverCallCount => "ResolverCallCount",
    TransportCallCount => "TransportCallCount",
    SanctionedHeader => "SanctionedHeader",
    AuthorizationPresence => "AuthorizationPresence",
});

/// One case/category mismatch without expected or observed payload values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HeaderAuthMismatchV1 {
    case_id: HeaderAuthCaseIdV1,
    category: HeaderAuthMismatchCategoryV1,
}

impl HeaderAuthMismatchV1 {
    /// Returns the canonical case that mismatched.
    #[must_use]
    pub const fn case_id(&self) -> HeaderAuthCaseIdV1 {
        self.case_id
    }

    /// Returns the closed mismatch category.
    #[must_use]
    pub const fn category(&self) -> HeaderAuthMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for HeaderAuthMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HeaderAuthMismatchV1")
            .field("case_id", &self.case_id)
            .field("category", &self.category)
            .finish()
    }
}

/// A successful report for the complete canonical header-auth suite.
pub struct HeaderAuthConformanceReportV1 {
    passed_case_ids: Vec<HeaderAuthCaseIdV1>,
}

impl HeaderAuthConformanceReportV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        HEADER_AUTH_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        HEADER_AUTH_CONFORMANCE_SUITE_VERSION
    }

    /// Returns all passed cases in canonical table order.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[HeaderAuthCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for HeaderAuthConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HeaderAuthConformanceReportV1")
            .field("suite_id", &HEADER_AUTH_CONFORMANCE_SUITE_ID)
            .field("suite_version", &HEADER_AUTH_CONFORMANCE_SUITE_VERSION)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// A complete bounded mismatch report for the evaluated canonical header-auth suite.
pub struct HeaderAuthConformanceFailureV1 {
    evaluated_case_count: usize,
    mismatches: Vec<HeaderAuthMismatchV1>,
}

impl HeaderAuthConformanceFailureV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        HEADER_AUTH_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        HEADER_AUTH_CONFORMANCE_SUITE_VERSION
    }

    /// Returns how many canonical cases completed evaluation.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Returns every case/category mismatch in canonical evaluation order.
    #[must_use]
    pub fn mismatches(&self) -> &[HeaderAuthMismatchV1] {
        &self.mismatches
    }
}

impl fmt::Debug for HeaderAuthConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HeaderAuthConformanceFailureV1")
            .field("suite_id", &HEADER_AUTH_CONFORMANCE_SUITE_ID)
            .field("suite_version", &HEADER_AUTH_CONFORMANCE_SUITE_VERSION)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs all canonical header-auth cases sequentially without failing fast.
///
/// Every caller must wrap the entire runner in an outer watchdog. This function intentionally has
/// no internal timeout, so a broken assembled executor may remain pending forever. The watchdog
/// must own the complete structured future tree so timeout drops all in-progress executor work
/// without leaving a detached task.
pub async fn run_header_auth_conformance_v1(
    executor: &dyn AssembledHeaderAuthExecutorV1,
) -> Result<HeaderAuthConformanceReportV1, HeaderAuthConformanceFailureV1> {
    let fixtures = header_auth_fixtures_v1();
    let mut passed_case_ids = Vec::with_capacity(fixtures.len());
    let mut mismatches = Vec::with_capacity(MAX_HEADER_AUTH_MISMATCHES_V1);

    for fixture in fixtures {
        let mismatch_count_before_case = mismatches.len();
        let observation = executor.execute_case(fixture).await;
        compare_header_auth_outcome(fixture, &observation, &mut mismatches);
        compare_header_auth_evidence(fixture, &observation, &mut mismatches);
        if mismatches.len() == mismatch_count_before_case {
            passed_case_ids.push(fixture.case_id());
        }
    }

    if mismatches.is_empty() {
        Ok(HeaderAuthConformanceReportV1 { passed_case_ids })
    } else {
        debug_assert!(mismatches.len() <= MAX_HEADER_AUTH_MISMATCHES_V1);
        Err(HeaderAuthConformanceFailureV1 { evaluated_case_count: fixtures.len(), mismatches })
    }
}

fn compare_header_auth_outcome(
    fixture: &HeaderAuthFixtureV1,
    observation: &HeaderAuthObservationV1,
    mismatches: &mut Vec<HeaderAuthMismatchV1>,
) {
    match (fixture.expected().outcome(), &observation.outcome) {
        (
            HeaderAuthExpectedOutcomeV1::Response { status, body, content_type, retry_after },
            HeaderAuthObservedOutcomeV1::Response(response),
        ) => {
            record_if(
                response.status().as_u16() != *status,
                fixture,
                HeaderAuthMismatchCategoryV1::Status,
                mismatches,
            );
            record_if(
                response.body().as_bytes() != body.as_bytes(),
                fixture,
                HeaderAuthMismatchCategoryV1::Body,
                mismatches,
            );
            record_if(
                response.content_type() != *content_type,
                fixture,
                HeaderAuthMismatchCategoryV1::ContentType,
                mismatches,
            );
            record_if(
                response.retry_after() != *retry_after,
                fixture,
                HeaderAuthMismatchCategoryV1::RetryAfter,
                mismatches,
            );
        }
        (
            HeaderAuthExpectedOutcomeV1::Opened { status, content_type, retry_after, chunks },
            HeaderAuthObservedOutcomeV1::Opened { head, chunks: observed_chunks },
        ) => {
            record_if(
                head.status().as_u16() != *status,
                fixture,
                HeaderAuthMismatchCategoryV1::Status,
                mismatches,
            );
            record_if(
                head.content_type() != *content_type,
                fixture,
                HeaderAuthMismatchCategoryV1::ContentType,
                mismatches,
            );
            record_if(
                head.retry_after() != *retry_after,
                fixture,
                HeaderAuthMismatchCategoryV1::RetryAfter,
                mismatches,
            );
            let chunks_match = observed_chunks.len() == chunks.len()
                && observed_chunks
                    .iter()
                    .zip(chunks.iter())
                    .all(|(observed, expected)| observed.as_bytes() == *expected);
            record_if(!chunks_match, fixture, HeaderAuthMismatchCategoryV1::ChunkBytes, mismatches);
        }
        (
            HeaderAuthExpectedOutcomeV1::Failure { code: expected },
            HeaderAuthObservedOutcomeV1::Failure(observed),
        ) => record_if(
            expected != observed,
            fixture,
            HeaderAuthMismatchCategoryV1::ErrorCode,
            mismatches,
        ),
        _ => record(fixture, HeaderAuthMismatchCategoryV1::OutcomeKind, mismatches),
    }
}

fn compare_header_auth_evidence(
    fixture: &HeaderAuthFixtureV1,
    observation: &HeaderAuthObservationV1,
    mismatches: &mut Vec<HeaderAuthMismatchV1>,
) {
    let expected = fixture.expected().evidence();
    let observed = observation.evidence();
    record_if(
        expected.resolver_calls() != observed.resolver_calls(),
        fixture,
        HeaderAuthMismatchCategoryV1::ResolverCallCount,
        mismatches,
    );
    record_if(
        expected.transport_calls() != observed.transport_calls(),
        fixture,
        HeaderAuthMismatchCategoryV1::TransportCallCount,
        mismatches,
    );
    record_if(
        expected.sanctioned_header_exact() != observed.sanctioned_header_exact(),
        fixture,
        HeaderAuthMismatchCategoryV1::SanctionedHeader,
        mismatches,
    );
    record_if(
        expected.authorization_header_absent() != observed.authorization_header_absent(),
        fixture,
        HeaderAuthMismatchCategoryV1::AuthorizationPresence,
        mismatches,
    );
}

fn record_if(
    condition: bool,
    fixture: &HeaderAuthFixtureV1,
    category: HeaderAuthMismatchCategoryV1,
    mismatches: &mut Vec<HeaderAuthMismatchV1>,
) {
    if condition {
        record(fixture, category, mismatches);
    }
}

fn record(
    fixture: &HeaderAuthFixtureV1,
    category: HeaderAuthMismatchCategoryV1,
    mismatches: &mut Vec<HeaderAuthMismatchV1>,
) {
    mismatches.push(HeaderAuthMismatchV1 { case_id: fixture.case_id(), category });
}

/// A deterministic assembled header-auth executor built from real `south-core` orchestration and
/// fake ports.
///
/// The buffered and slot-mismatch cases run through `execute_provider_call_v1`; the streaming
/// case runs through `open_streaming_provider_call_v1`. The wire-shape booleans are measured on
/// the prepared request at the fake transport boundary, mirroring what a real adapter must
/// measure on its wire.
pub struct ReferenceAssembledHeaderAuthExecutorV1;

impl ReferenceAssembledHeaderAuthExecutorV1 {
    /// Creates an independent reference header-auth executor.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for ReferenceAssembledHeaderAuthExecutorV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl AssembledHeaderAuthExecutorV1 for ReferenceAssembledHeaderAuthExecutorV1 {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a HeaderAuthFixtureV1,
    ) -> AssembledHeaderAuthExecutionFutureV1<'a> {
        Box::pin(async move { execute_reference_header_auth_case(fixture).await })
    }
}

async fn execute_reference_header_auth_case(
    fixture: &HeaderAuthFixtureV1,
) -> HeaderAuthObservationV1 {
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let transport_calls = Arc::new(AtomicUsize::new(0));
    let wire_shape = Arc::new(WireShapeProbe::default());

    let secret_header = fixture.secret_header();
    let parsed = parse_reference_input_with_auth(fixture.input(), |slot| {
        ProviderAuthV1::HeaderSecret { header: secret_header, slot: BearerAuthV1::new(slot) }
    });
    let (binding, request) = match parsed {
        Ok(parsed) => parsed,
        Err(code) => {
            return HeaderAuthObservationV1::failure(
                code,
                HeaderAuthEvidenceV1::new(0, 0, false, true),
            );
        }
    };

    let resolver = HeaderSecretResolver { calls: Arc::clone(&resolver_calls) };
    let transport = WireRecordingTransport {
        calls: Arc::clone(&transport_calls),
        upstream: fixture.upstream(),
        expected_header: secret_header,
        wire_shape: Arc::clone(&wire_shape),
    };
    let cancellation = CancellationToken::new();
    let evidence = || {
        HeaderAuthEvidenceV1::new(
            resolver_calls.load(Ordering::SeqCst),
            transport_calls.load(Ordering::SeqCst),
            wire_shape.sanctioned_header_exact.load(Ordering::SeqCst),
            wire_shape.authorization_header_absent.load(Ordering::SeqCst),
        )
    };

    match fixture.upstream() {
        HeaderAuthUpstreamV1::Stream(_) => {
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
                    return HeaderAuthObservationV1::failure(
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
                        return HeaderAuthObservationV1::failure(
                            map_stream_read_terminal(error),
                            evidence(),
                        );
                    }
                }
            }
            HeaderAuthObservationV1::opened(head, chunks, evidence())
        }
        HeaderAuthUpstreamV1::Response(_) | HeaderAuthUpstreamV1::NotReached => {
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
                Ok(response) => HeaderAuthObservationV1::response(response, evidence()),
                Err(error) => {
                    HeaderAuthObservationV1::failure(map_provider_call_error(&error), evidence())
                }
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

/// Observed wire shape at the fake transport boundary.
///
/// `authorization_header_absent` starts `true` because the absence claim is vacuously true until
/// a transport call observes the wire; `sanctioned_header_exact` starts `false` because the
/// presence claim requires an observation.
struct WireShapeProbe {
    sanctioned_header_exact: AtomicBool,
    authorization_header_absent: AtomicBool,
}

impl Default for WireShapeProbe {
    fn default() -> Self {
        Self {
            sanctioned_header_exact: AtomicBool::new(false),
            authorization_header_absent: AtomicBool::new(true),
        }
    }
}

struct HeaderSecretResolver {
    calls: Arc<AtomicUsize>,
}

impl CredentialResolver for HeaderSecretResolver {
    fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(SecretValue::new(FAKE_HEADER_SECRET_V1.to_owned())) })
    }
}

struct WireRecordingTransport<'fixture> {
    calls: Arc<AtomicUsize>,
    upstream: &'fixture HeaderAuthUpstreamV1,
    expected_header: SecretHeaderV1,
    wire_shape: Arc<WireShapeProbe>,
}

impl WireRecordingTransport<'_> {
    fn record_wire_shape(&self, request: &PreparedHttpRequestV1<'_>) {
        let (auth_name, auth_value) = request.auth_header();
        let sanctioned_header_exact = auth_name == self.expected_header.header_name()
            && auth_value == FAKE_HEADER_SECRET_V1.as_bytes();
        let authorization_header_absent =
            auth_name != "authorization" && request.headers().get("authorization").is_none();
        self.wire_shape.sanctioned_header_exact.store(sanctioned_header_exact, Ordering::SeqCst);
        self.wire_shape
            .authorization_header_absent
            .store(authorization_header_absent, Ordering::SeqCst);
    }
}

impl AsyncHttpTransport for WireRecordingTransport<'_> {
    fn execute<'a>(
        &'a self,
        request: &'a PreparedHttpRequestV1<'_>,
        _remaining_timeout: Duration,
    ) -> TransportFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.record_wire_shape(request);
        match self.upstream {
            HeaderAuthUpstreamV1::Response(raw) => {
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
            HeaderAuthUpstreamV1::Stream(_) | HeaderAuthUpstreamV1::NotReached => {
                Box::pin(async { Err(TransportErrorV1::RequestFailed) })
            }
        }
    }
}

impl AsyncStreamingTransport for WireRecordingTransport<'_> {
    fn open<'a>(&'a self, request: &'a PreparedHttpRequestV1<'_>) -> StreamingOpenFutureV1<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.record_wire_shape(request);
        match self.upstream {
            HeaderAuthUpstreamV1::Stream(raw) => {
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
            HeaderAuthUpstreamV1::Response(_) | HeaderAuthUpstreamV1::NotReached => {
                Box::pin(async {
                    Err(StreamOpenErrorV1::Transport(TransportErrorV1::RequestFailed))
                })
            }
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
