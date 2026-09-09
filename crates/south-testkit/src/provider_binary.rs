//! Assembled-executor runner and reference executor for the buffered-binary suite.

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

use http::StatusCode;
use south_contracts::{
    BearerAuthV1, BufferedBinaryResponseV1, BufferedHttpResponseV1, CredentialSlotV1, JsonBodyV1,
    JsonPostRequestV1, ProviderAuthV1, ProviderEndpointV1, RelativePathV1, SafeHeaders,
    TransportErrorV1,
};
use south_core::{
    AsyncBinaryHttpTransport, AsyncHttpTransport, BinaryTransportFutureV1,
    CredentialResolutionFuture, CredentialResolver, PreparedHttpRequestV1, ProviderBindingV1,
    SecretValue, TransportFuture, execute_binary_call_v1, execute_provider_call_v1,
};
use south_provider_conformance::{
    FAKE_BEARER_SECRET_V1, PROVIDER_BINARY_CONFORMANCE_SUITE_ID,
    PROVIDER_BINARY_CONFORMANCE_SUITE_VERSION, ProviderBinaryBodyV1, ProviderBinaryCaseIdV1,
    ProviderBinaryEntryArmV1, ProviderBinaryExpectedOutcomeV1, ProviderBinaryFixtureV1,
    ProviderBinaryInputV1, ProviderBinaryUpstreamV1, ProviderCallCountV1,
    ProviderCallFailureCodeV1, provider_binary_fixtures_v1,
};
use tokio_util::sync::CancellationToken;

use crate::{
    map_canonical_fixture_header_invariant_failure, map_contract_error, map_provider_call_error,
};

/// Six cases multiplied by the ten closed buffered-binary mismatch categories.
pub const MAX_PROVIDER_BINARY_MISMATCHES_V1: usize = 60;

/// A boxed, cancellation-safe assembled buffered-binary executor future.
pub type AssembledProviderBinaryExecutionFutureV1<'a> =
    Pin<Box<dyn Future<Output = ProviderBinaryObservationV1> + Send + 'a>>;

/// A host-assembled buffered-binary call path exercised by the public buffered-binary runner.
pub trait AssembledProviderBinaryExecutorV1: Send + Sync {
    /// Executes one immutable canonical buffered-binary fixture.
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderBinaryFixtureV1,
    ) -> AssembledProviderBinaryExecutionFutureV1<'a>;
}

/// Adapter-reported resolver, transport, and wire-shape boundary evidence.
///
/// Both wire-shape booleans are presence claims measured at the adapter's real boundary: whether
/// the binary transport seam is what produced the response, and whether the bytes that came back
/// are exactly the bytes the upstream sent. Like every adapter-reported value, a passing report
/// alone is insufficient for host verification; the adoption review must confirm the wiring.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderBinaryEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    wire_binary_response_observed: bool,
    wire_body_bytes_exact: bool,
}

impl ProviderBinaryEvidenceV1 {
    /// Constructs evidence and saturates both raw call counts.
    #[must_use]
    pub const fn new(
        resolver_calls: usize,
        transport_calls: usize,
        wire_binary_response_observed: bool,
        wire_body_bytes_exact: bool,
    ) -> Self {
        Self {
            resolver_calls: ProviderCallCountV1::from_usize(resolver_calls),
            transport_calls: ProviderCallCountV1::from_usize(transport_calls),
            wire_binary_response_observed,
            wire_body_bytes_exact,
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

    /// Returns whether the binary transport seam produced the response.
    #[must_use]
    pub const fn wire_binary_response_observed(&self) -> bool {
        self.wire_binary_response_observed
    }

    /// Returns whether the returned body is exactly the bytes the upstream produced.
    #[must_use]
    pub const fn wire_body_bytes_exact(&self) -> bool {
        self.wire_body_bytes_exact
    }
}

impl fmt::Debug for ProviderBinaryEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderBinaryEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field("wire_binary_response_observed", &self.wire_binary_response_observed)
            .field("wire_body_bytes_exact", &self.wire_body_bytes_exact)
            .finish()
    }
}

enum ProviderBinaryObservedOutcomeV1 {
    Response(BufferedBinaryResponseV1),
    Failure(ProviderCallFailureCodeV1),
}

impl fmt::Debug for ProviderBinaryObservedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(response) => formatter.debug_tuple("Response").field(response).finish(),
            Self::Failure(code) => formatter.debug_tuple("Failure").field(code).finish(),
        }
    }
}

/// An observed buffered-binary terminal shape plus adapter-reported evidence.
pub struct ProviderBinaryObservationV1 {
    outcome: ProviderBinaryObservedOutcomeV1,
    evidence: ProviderBinaryEvidenceV1,
}

impl ProviderBinaryObservationV1 {
    /// Constructs a binary-response observation from an already bounded response.
    ///
    /// The UTF-8 row reports through here too, on the one path that matters: an executor whose
    /// text arm *succeeded* on non-UTF-8 bytes reports the response it got, and the table's frozen
    /// `Failure` expectation turns that into an `OutcomeKind` mismatch. That is the regression
    /// signal, so the observation type deliberately has no third "the text arm answered" variant
    /// to hide it in.
    #[must_use]
    pub const fn response(
        response: BufferedBinaryResponseV1,
        evidence: ProviderBinaryEvidenceV1,
    ) -> Self {
        Self { outcome: ProviderBinaryObservedOutcomeV1::Response(response), evidence }
    }

    /// Constructs a failed observation from a closed known failure code.
    #[must_use]
    pub const fn failure(
        code: ProviderCallFailureCodeV1,
        evidence: ProviderBinaryEvidenceV1,
    ) -> Self {
        Self { outcome: ProviderBinaryObservedOutcomeV1::Failure(code), evidence }
    }

    /// Returns the bounded response when this is a binary-response observation.
    #[must_use]
    pub const fn response_value(&self) -> Option<&BufferedBinaryResponseV1> {
        match &self.outcome {
            ProviderBinaryObservedOutcomeV1::Response(response) => Some(response),
            ProviderBinaryObservedOutcomeV1::Failure(_) => None,
        }
    }

    /// Returns the closed code when this is a failure observation.
    #[must_use]
    pub const fn failure_code(&self) -> Option<ProviderCallFailureCodeV1> {
        match &self.outcome {
            ProviderBinaryObservedOutcomeV1::Failure(code) => Some(*code),
            ProviderBinaryObservedOutcomeV1::Response(_) => None,
        }
    }

    /// Returns adapter-reported boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderBinaryEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ProviderBinaryObservationV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderBinaryObservationV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// The closed reasons why an observed buffered-binary case can differ from its fixture.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderBinaryMismatchCategoryV1 {
    /// Response versus failure differed.
    OutcomeKind,
    /// Stable failure code differed.
    ErrorCode,
    /// Response status differed.
    Status,
    /// Response body bytes differed.
    Body,
    /// `content-type` value or presence differed.
    ContentType,
    /// `retry-after` value or presence differed.
    RetryAfter,
    /// Resolver call category differed.
    ResolverCallCount,
    /// Transport call category differed.
    TransportCallCount,
    /// Binary-seam wire evidence differed.
    WireBinaryResponse,
    /// Body-bytes wire evidence differed.
    WireBodyBytes,
}

fixed_debug!(ProviderBinaryMismatchCategoryV1 {
    OutcomeKind => "OutcomeKind",
    ErrorCode => "ErrorCode",
    Status => "Status",
    Body => "Body",
    ContentType => "ContentType",
    RetryAfter => "RetryAfter",
    ResolverCallCount => "ResolverCallCount",
    TransportCallCount => "TransportCallCount",
    WireBinaryResponse => "WireBinaryResponse",
    WireBodyBytes => "WireBodyBytes",
});

/// One case/category mismatch without expected or observed payload values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderBinaryMismatchV1 {
    case_id: ProviderBinaryCaseIdV1,
    category: ProviderBinaryMismatchCategoryV1,
}

impl ProviderBinaryMismatchV1 {
    /// Returns the canonical case that mismatched.
    #[must_use]
    pub const fn case_id(&self) -> ProviderBinaryCaseIdV1 {
        self.case_id
    }

    /// Returns the closed mismatch category.
    #[must_use]
    pub const fn category(&self) -> ProviderBinaryMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for ProviderBinaryMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderBinaryMismatchV1")
            .field("case_id", &self.case_id)
            .field("category", &self.category)
            .finish()
    }
}

/// A successful report for the complete canonical buffered-binary suite.
pub struct ProviderBinaryConformanceReportV1 {
    passed_case_ids: Vec<ProviderBinaryCaseIdV1>,
}

impl ProviderBinaryConformanceReportV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_BINARY_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_BINARY_CONFORMANCE_SUITE_VERSION
    }

    /// Returns all passed cases in canonical table order.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[ProviderBinaryCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for ProviderBinaryConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderBinaryConformanceReportV1")
            .field("suite_id", &PROVIDER_BINARY_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_BINARY_CONFORMANCE_SUITE_VERSION)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// A complete bounded mismatch report for the evaluated canonical buffered-binary suite.
pub struct ProviderBinaryConformanceFailureV1 {
    evaluated_case_count: usize,
    mismatches: Vec<ProviderBinaryMismatchV1>,
}

impl ProviderBinaryConformanceFailureV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_BINARY_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_BINARY_CONFORMANCE_SUITE_VERSION
    }

    /// Returns how many canonical cases completed evaluation.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Returns every case/category mismatch in canonical evaluation order.
    #[must_use]
    pub fn mismatches(&self) -> &[ProviderBinaryMismatchV1] {
        &self.mismatches
    }
}

impl fmt::Debug for ProviderBinaryConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderBinaryConformanceFailureV1")
            .field("suite_id", &PROVIDER_BINARY_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_BINARY_CONFORMANCE_SUITE_VERSION)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs all canonical buffered-binary cases sequentially without failing fast.
///
/// Every caller must wrap the entire runner in an outer watchdog. This function intentionally has
/// no internal timeout, so a broken assembled executor may remain pending forever. The watchdog
/// must own the complete structured future tree so timeout drops all in-progress executor work
/// without leaving a detached task.
pub async fn run_provider_binary_conformance_v1(
    executor: &dyn AssembledProviderBinaryExecutorV1,
) -> Result<ProviderBinaryConformanceReportV1, ProviderBinaryConformanceFailureV1> {
    let fixtures = provider_binary_fixtures_v1();
    let mut passed_case_ids = Vec::with_capacity(fixtures.len());
    let mut mismatches = Vec::with_capacity(MAX_PROVIDER_BINARY_MISMATCHES_V1);

    for fixture in fixtures {
        let mismatch_count_before_case = mismatches.len();
        let observation = executor.execute_case(fixture).await;
        compare_binary_outcome(fixture, &observation, &mut mismatches);
        compare_binary_evidence(fixture, &observation, &mut mismatches);
        if mismatches.len() == mismatch_count_before_case {
            passed_case_ids.push(fixture.case_id());
        }
    }

    if mismatches.is_empty() {
        Ok(ProviderBinaryConformanceReportV1 { passed_case_ids })
    } else {
        debug_assert!(mismatches.len() <= MAX_PROVIDER_BINARY_MISMATCHES_V1);
        Err(ProviderBinaryConformanceFailureV1 { evaluated_case_count: fixtures.len(), mismatches })
    }
}

fn compare_binary_outcome(
    fixture: &ProviderBinaryFixtureV1,
    observation: &ProviderBinaryObservationV1,
    mismatches: &mut Vec<ProviderBinaryMismatchV1>,
) {
    match (fixture.expected().outcome(), &observation.outcome) {
        (
            ProviderBinaryExpectedOutcomeV1::Response { status, body, content_type, retry_after },
            ProviderBinaryObservedOutcomeV1::Response(response),
        ) => {
            record_if(
                response.status().as_u16() != *status,
                fixture,
                ProviderBinaryMismatchCategoryV1::Status,
                mismatches,
            );
            // Compared without materialising the expected body: the cap rows would otherwise
            // build a second multi-mebibyte buffer just to prove a length.
            record_if(
                !body.matches(response.body()),
                fixture,
                ProviderBinaryMismatchCategoryV1::Body,
                mismatches,
            );
            record_if(
                response.content_type() != *content_type,
                fixture,
                ProviderBinaryMismatchCategoryV1::ContentType,
                mismatches,
            );
            record_if(
                response.retry_after() != *retry_after,
                fixture,
                ProviderBinaryMismatchCategoryV1::RetryAfter,
                mismatches,
            );
        }
        (
            ProviderBinaryExpectedOutcomeV1::Failure { code: expected },
            ProviderBinaryObservedOutcomeV1::Failure(observed),
        ) => record_if(
            expected != observed,
            fixture,
            ProviderBinaryMismatchCategoryV1::ErrorCode,
            mismatches,
        ),
        _ => record(fixture, ProviderBinaryMismatchCategoryV1::OutcomeKind, mismatches),
    }
}

fn compare_binary_evidence(
    fixture: &ProviderBinaryFixtureV1,
    observation: &ProviderBinaryObservationV1,
    mismatches: &mut Vec<ProviderBinaryMismatchV1>,
) {
    let expected = fixture.expected().evidence();
    let observed = observation.evidence();
    record_if(
        expected.resolver_calls() != observed.resolver_calls(),
        fixture,
        ProviderBinaryMismatchCategoryV1::ResolverCallCount,
        mismatches,
    );
    record_if(
        expected.transport_calls() != observed.transport_calls(),
        fixture,
        ProviderBinaryMismatchCategoryV1::TransportCallCount,
        mismatches,
    );
    record_if(
        expected.wire_binary_response_observed() != observed.wire_binary_response_observed(),
        fixture,
        ProviderBinaryMismatchCategoryV1::WireBinaryResponse,
        mismatches,
    );
    record_if(
        expected.wire_body_bytes_exact() != observed.wire_body_bytes_exact(),
        fixture,
        ProviderBinaryMismatchCategoryV1::WireBodyBytes,
        mismatches,
    );
}

fn record_if(
    condition: bool,
    fixture: &ProviderBinaryFixtureV1,
    category: ProviderBinaryMismatchCategoryV1,
    mismatches: &mut Vec<ProviderBinaryMismatchV1>,
) {
    if condition {
        record(fixture, category, mismatches);
    }
}

fn record(
    fixture: &ProviderBinaryFixtureV1,
    category: ProviderBinaryMismatchCategoryV1,
    mismatches: &mut Vec<ProviderBinaryMismatchV1>,
) {
    mismatches.push(ProviderBinaryMismatchV1 { case_id: fixture.case_id(), category });
}

/// Parses one canonical buffered-binary input through the production contract.
///
/// Public so a host's own executor can reuse the exact parse the reference executor uses; how a
/// host obtains the raw strings in production is its own business. Every canonical input parses:
/// this suite's refusals all happen at or after a boundary, never while the request is still a
/// value, because the request side of a binary call is an ordinary JSON POST.
///
/// # Errors
///
/// Returns the closed failure code the fixture table expects for a refused input.
pub fn parse_reference_binary_input(
    input: &ProviderBinaryInputV1,
) -> Result<(ProviderBindingV1, JsonPostRequestV1), ProviderCallFailureCodeV1> {
    let endpoint = ProviderEndpointV1::parse(input.endpoint()).map_err(map_contract_error)?;
    let bound_slot =
        CredentialSlotV1::parse(input.bound_credential_slot()).map_err(map_contract_error)?;
    let requested_slot =
        CredentialSlotV1::parse(input.requested_credential_slot()).map_err(map_contract_error)?;
    let relative_path = RelativePathV1::parse(input.relative_path()).map_err(map_contract_error)?;
    let body = JsonBodyV1::parse(input.json_body()).map_err(map_contract_error)?;
    let headers = SafeHeaders::try_from_iter(input.headers().iter().copied())
        .map_err(map_canonical_fixture_header_invariant_failure)?;
    let binding = ProviderBindingV1::new(endpoint, bound_slot);
    let auth = ProviderAuthV1::Bearer(BearerAuthV1::new(requested_slot));
    let request = JsonPostRequestV1::new(relative_path, headers, body, auth);
    Ok((binding, request))
}

/// A deterministic assembled buffered-binary executor built from real `south-core` orchestration
/// and fake ports.
///
/// Five cases run through `execute_binary_call_v1`; the sixth runs through the frozen
/// `execute_provider_call_v1`, because the fixture says so. Both wire-shape booleans are measured
/// at the fake transport boundary and on the returned value, mirroring what a real adapter must
/// measure: which seam carried the exchange, and whether the bytes survived it.
pub struct ReferenceAssembledProviderBinaryExecutorV1;

impl ReferenceAssembledProviderBinaryExecutorV1 {
    /// Creates an independent reference buffered-binary executor.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for ReferenceAssembledProviderBinaryExecutorV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl AssembledProviderBinaryExecutorV1 for ReferenceAssembledProviderBinaryExecutorV1 {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderBinaryFixtureV1,
    ) -> AssembledProviderBinaryExecutionFutureV1<'a> {
        Box::pin(async move { execute_reference_binary_case(fixture).await })
    }
}

async fn execute_reference_binary_case(
    fixture: &ProviderBinaryFixtureV1,
) -> ProviderBinaryObservationV1 {
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let transport_calls = Arc::new(AtomicUsize::new(0));
    let probe = Arc::new(BinarySeamProbe::default());

    let (binding, request) = match parse_reference_binary_input(fixture.input()) {
        Ok(parsed) => parsed,
        Err(code) => {
            return ProviderBinaryObservationV1::failure(
                code,
                ProviderBinaryEvidenceV1::new(0, 0, false, false),
            );
        }
    };

    let resolver = BearerSecretResolver { calls: Arc::clone(&resolver_calls) };
    let cancellation = CancellationToken::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let expected_body = expected_upstream_body(fixture);

    let outcome = match fixture.entry_arm() {
        ProviderBinaryEntryArmV1::Binary => {
            let transport = BinaryRecordingTransport {
                calls: Arc::clone(&transport_calls),
                upstream: fixture.upstream(),
                probe: Arc::clone(&probe),
            };
            let result = execute_binary_call_v1(
                &binding,
                &request,
                &resolver,
                &transport,
                deadline,
                &cancellation,
            )
            .await;
            match result {
                Ok(response) => {
                    // The bytes claim is measured on what came back, not on what was sent: a
                    // truncating or re-encoding implementation reaches the seam and still fails.
                    probe
                        .body_bytes_exact
                        .store(expected_body.as_deref() == Some(response.body()), Ordering::SeqCst);
                    Ok(response)
                }
                Err(error) => Err(map_provider_call_error(&error)),
            }
        }
        // The regression row. It drives the frozen UTF-8 entry point over the frozen UTF-8
        // transport trait, so the binary seam is never touched and its presence claim must stay
        // `false` even though a transport call happened.
        ProviderBinaryEntryArmV1::Utf8 => {
            let transport = Utf8RecordingTransport {
                calls: Arc::clone(&transport_calls),
                upstream: fixture.upstream(),
            };
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
                // Reached only if the UTF-8 path stopped refusing these bytes, which is the
                // regression this row exists to catch. Report it as a response so the frozen
                // `Failure` expectation fires an `OutcomeKind` mismatch.
                Ok(response) => BufferedBinaryResponseV1::try_from_parts(
                    response.status(),
                    response.body().as_bytes().to_vec(),
                    response.content_type().map(str::to_owned),
                    response.retry_after().map(str::to_owned),
                )
                .map_err(|_| ProviderCallFailureCodeV1::RequestFailed),
                Err(error) => Err(map_provider_call_error(&error)),
            }
        }
    };

    let evidence = ProviderBinaryEvidenceV1::new(
        resolver_calls.load(Ordering::SeqCst),
        transport_calls.load(Ordering::SeqCst),
        probe.binary_response_observed.load(Ordering::SeqCst),
        probe.body_bytes_exact.load(Ordering::SeqCst),
    );
    match outcome {
        Ok(response) => ProviderBinaryObservationV1::response(response, evidence),
        Err(code) => ProviderBinaryObservationV1::failure(code, evidence),
    }
}

/// Materialises the bytes the canonical upstream produces, once per case.
fn expected_upstream_body(fixture: &ProviderBinaryFixtureV1) -> Option<Vec<u8>> {
    match fixture.upstream() {
        ProviderBinaryUpstreamV1::Response(raw) => Some(raw.body().materialize()),
        ProviderBinaryUpstreamV1::NotReached => None,
    }
}

/// Observed wire shape. Both are presence claims, so both start `false` and only a real exchange
/// can raise them.
#[derive(Default)]
struct BinarySeamProbe {
    binary_response_observed: AtomicBool,
    body_bytes_exact: AtomicBool,
}

struct BearerSecretResolver {
    calls: Arc<AtomicUsize>,
}

impl CredentialResolver for BearerSecretResolver {
    fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(SecretValue::new(FAKE_BEARER_SECRET_V1.to_owned())) })
    }
}

/// Builds the raw parts one canonical response is made of.
fn raw_parts(
    body: ProviderBinaryBodyV1,
    status: u16,
    content_type: Option<&'static str>,
    retry_after: Option<&'static str>,
) -> (Result<StatusCode, TransportErrorV1>, Vec<u8>, Option<String>, Option<String>) {
    (
        StatusCode::from_u16(status).map_err(|_| TransportErrorV1::ResponseMetadataInvalid),
        body.materialize(),
        content_type.map(str::to_owned),
        retry_after.map(str::to_owned),
    )
}

struct BinaryRecordingTransport<'fixture> {
    calls: Arc<AtomicUsize>,
    upstream: &'fixture ProviderBinaryUpstreamV1,
    probe: Arc<BinarySeamProbe>,
}

impl AsyncBinaryHttpTransport for BinaryRecordingTransport<'_> {
    fn execute_binary<'a>(
        &'a self,
        _request: &'a PreparedHttpRequestV1<'_>,
        _remaining_timeout: Duration,
    ) -> BinaryTransportFutureV1<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        // Raised at the seam itself: this claim is about *which* transport trait carried the
        // exchange, which is the thing the third-trait decision is judged on.
        self.probe.binary_response_observed.store(true, Ordering::SeqCst);
        match self.upstream {
            ProviderBinaryUpstreamV1::Response(raw) => {
                let (status, body, content_type, retry_after) =
                    raw_parts(raw.body(), raw.status(), raw.content_type(), raw.retry_after());
                Box::pin(async move {
                    BufferedBinaryResponseV1::try_from_parts(
                        status?,
                        body,
                        content_type,
                        retry_after,
                    )
                })
            }
            // `NotReached` must not reach any boundary. Fail closed with the context-free request
            // code rather than panicking.
            ProviderBinaryUpstreamV1::NotReached => {
                Box::pin(async { Err(TransportErrorV1::RequestFailed) })
            }
        }
    }
}

/// The frozen UTF-8 transport, used only by the regression row.
struct Utf8RecordingTransport<'fixture> {
    calls: Arc<AtomicUsize>,
    upstream: &'fixture ProviderBinaryUpstreamV1,
}

impl AsyncHttpTransport for Utf8RecordingTransport<'_> {
    fn execute<'a>(
        &'a self,
        _request: &'a PreparedHttpRequestV1<'_>,
        _remaining_timeout: Duration,
    ) -> TransportFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.upstream {
            ProviderBinaryUpstreamV1::Response(raw) => {
                let (status, body, content_type, retry_after) =
                    raw_parts(raw.body(), raw.status(), raw.content_type(), raw.retry_after());
                Box::pin(async move {
                    BufferedHttpResponseV1::try_from_parts(status?, body, content_type, retry_after)
                })
            }
            ProviderBinaryUpstreamV1::NotReached => {
                Box::pin(async { Err(TransportErrorV1::RequestFailed) })
            }
        }
    }
}
