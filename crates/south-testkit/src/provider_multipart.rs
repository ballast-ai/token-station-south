//! Assembled-executor runner and reference executor for the multipart suite.

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
    BearerAuthV1, BufferedHttpResponseV1, CredentialSlotV1, MultipartBodyV1, MultipartBoundaryV1,
    MultipartPostRequestV1, ProviderAuthV1, ProviderEndpointV1, RelativePathV1, SafeHeaders,
    TransportErrorV1,
};
use south_core::{
    AsyncHttpTransport, CredentialResolutionFuture, CredentialResolver, PreparedHttpRequestV1,
    ProviderBindingV1, SecretValue, TransportFuture, execute_multipart_call_v1,
};
use south_provider_conformance::{
    FAKE_BEARER_SECRET_V1, FAKE_HEADER_SECRET_V1, PROVIDER_MULTIPART_CONFORMANCE_SUITE_ID,
    PROVIDER_MULTIPART_CONFORMANCE_SUITE_VERSION, ProviderCallCountV1, ProviderCallFailureCodeV1,
    ProviderMultipartAuthArmV1, ProviderMultipartCaseIdV1, ProviderMultipartExpectedOutcomeV1,
    ProviderMultipartFixtureV1, ProviderMultipartInputV1, ProviderMultipartUpstreamV1,
    provider_multipart_fixtures_v1,
};
use tokio_util::sync::CancellationToken;

use crate::{
    map_canonical_fixture_header_invariant_failure, map_contract_error, map_provider_call_error,
};

/// Five cases multiplied by the ten closed multipart mismatch categories.
pub const MAX_PROVIDER_MULTIPART_MISMATCHES_V1: usize = 50;

/// A boxed, cancellation-safe assembled multipart executor future.
pub type AssembledProviderMultipartExecutionFutureV1<'a> =
    Pin<Box<dyn Future<Output = ProviderMultipartObservationV1> + Send + 'a>>;

/// A host-assembled multipart call path exercised by the public multipart runner.
pub trait AssembledProviderMultipartExecutorV1: Send + Sync {
    /// Executes one immutable canonical multipart fixture.
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderMultipartFixtureV1,
    ) -> AssembledProviderMultipartExecutionFutureV1<'a>;
}

/// Adapter-reported resolver, transport, and wire-shape boundary evidence.
///
/// Both wire-shape booleans are presence claims measured at the adapter's real transport
/// boundary: whether the rendered media type reached it byte for byte, and whether the declared
/// body bytes reached it unmodified. Like every adapter-reported value, a passing report alone is
/// insufficient for host verification; the adoption review must confirm the wiring.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderMultipartEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    wire_content_type_exact: bool,
    wire_body_bytes_exact: bool,
}

impl ProviderMultipartEvidenceV1 {
    /// Constructs evidence and saturates both raw call counts.
    #[must_use]
    pub const fn new(
        resolver_calls: usize,
        transport_calls: usize,
        wire_content_type_exact: bool,
        wire_body_bytes_exact: bool,
    ) -> Self {
        Self {
            resolver_calls: ProviderCallCountV1::from_usize(resolver_calls),
            transport_calls: ProviderCallCountV1::from_usize(transport_calls),
            wire_content_type_exact,
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

    /// Returns whether the rendered media type reached the transport byte for byte.
    #[must_use]
    pub const fn wire_content_type_exact(&self) -> bool {
        self.wire_content_type_exact
    }

    /// Returns whether the declared body bytes reached the transport unmodified.
    #[must_use]
    pub const fn wire_body_bytes_exact(&self) -> bool {
        self.wire_body_bytes_exact
    }
}

impl fmt::Debug for ProviderMultipartEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMultipartEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field("wire_content_type_exact", &self.wire_content_type_exact)
            .field("wire_body_bytes_exact", &self.wire_body_bytes_exact)
            .finish()
    }
}

enum ProviderMultipartObservedOutcomeV1 {
    Response(BufferedHttpResponseV1),
    Failure(ProviderCallFailureCodeV1),
}

impl fmt::Debug for ProviderMultipartObservedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(response) => formatter.debug_tuple("Response").field(response).finish(),
            Self::Failure(code) => formatter.debug_tuple("Failure").field(code).finish(),
        }
    }
}

/// An observed multipart terminal shape plus adapter-reported evidence.
pub struct ProviderMultipartObservationV1 {
    outcome: ProviderMultipartObservedOutcomeV1,
    evidence: ProviderMultipartEvidenceV1,
}

impl ProviderMultipartObservationV1 {
    /// Constructs a buffered-response observation from an already bounded response.
    #[must_use]
    pub const fn response(
        response: BufferedHttpResponseV1,
        evidence: ProviderMultipartEvidenceV1,
    ) -> Self {
        Self { outcome: ProviderMultipartObservedOutcomeV1::Response(response), evidence }
    }

    /// Constructs a failed observation from a closed known failure code.
    #[must_use]
    pub const fn failure(
        code: ProviderCallFailureCodeV1,
        evidence: ProviderMultipartEvidenceV1,
    ) -> Self {
        Self { outcome: ProviderMultipartObservedOutcomeV1::Failure(code), evidence }
    }

    /// Returns the bounded response when this is a buffered-response observation.
    #[must_use]
    pub const fn response_value(&self) -> Option<&BufferedHttpResponseV1> {
        match &self.outcome {
            ProviderMultipartObservedOutcomeV1::Response(response) => Some(response),
            ProviderMultipartObservedOutcomeV1::Failure(_) => None,
        }
    }

    /// Returns the closed code when this is a failure observation.
    #[must_use]
    pub const fn failure_code(&self) -> Option<ProviderCallFailureCodeV1> {
        match &self.outcome {
            ProviderMultipartObservedOutcomeV1::Failure(code) => Some(*code),
            ProviderMultipartObservedOutcomeV1::Response(_) => None,
        }
    }

    /// Returns adapter-reported boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderMultipartEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ProviderMultipartObservationV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMultipartObservationV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// The closed reasons why an observed multipart case can differ from its fixture.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderMultipartMismatchCategoryV1 {
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
    /// Rendered-media-type wire evidence differed.
    WireContentType,
    /// Body-bytes wire evidence differed.
    WireBodyBytes,
}

fixed_debug!(ProviderMultipartMismatchCategoryV1 {
    OutcomeKind => "OutcomeKind",
    ErrorCode => "ErrorCode",
    Status => "Status",
    Body => "Body",
    ContentType => "ContentType",
    RetryAfter => "RetryAfter",
    ResolverCallCount => "ResolverCallCount",
    TransportCallCount => "TransportCallCount",
    WireContentType => "WireContentType",
    WireBodyBytes => "WireBodyBytes",
});

/// One case/category mismatch without expected or observed payload values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderMultipartMismatchV1 {
    case_id: ProviderMultipartCaseIdV1,
    category: ProviderMultipartMismatchCategoryV1,
}

impl ProviderMultipartMismatchV1 {
    /// Returns the canonical case that mismatched.
    #[must_use]
    pub const fn case_id(&self) -> ProviderMultipartCaseIdV1 {
        self.case_id
    }

    /// Returns the closed mismatch category.
    #[must_use]
    pub const fn category(&self) -> ProviderMultipartMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for ProviderMultipartMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMultipartMismatchV1")
            .field("case_id", &self.case_id)
            .field("category", &self.category)
            .finish()
    }
}

/// A successful report for the complete canonical multipart suite.
pub struct ProviderMultipartConformanceReportV1 {
    passed_case_ids: Vec<ProviderMultipartCaseIdV1>,
}

impl ProviderMultipartConformanceReportV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_MULTIPART_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_MULTIPART_CONFORMANCE_SUITE_VERSION
    }

    /// Returns all passed cases in canonical table order.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[ProviderMultipartCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for ProviderMultipartConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMultipartConformanceReportV1")
            .field("suite_id", &PROVIDER_MULTIPART_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_MULTIPART_CONFORMANCE_SUITE_VERSION)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// A complete bounded mismatch report for the evaluated canonical multipart suite.
pub struct ProviderMultipartConformanceFailureV1 {
    evaluated_case_count: usize,
    mismatches: Vec<ProviderMultipartMismatchV1>,
}

impl ProviderMultipartConformanceFailureV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_MULTIPART_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_MULTIPART_CONFORMANCE_SUITE_VERSION
    }

    /// Returns how many canonical cases completed evaluation.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Returns every case/category mismatch in canonical evaluation order.
    #[must_use]
    pub fn mismatches(&self) -> &[ProviderMultipartMismatchV1] {
        &self.mismatches
    }
}

impl fmt::Debug for ProviderMultipartConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMultipartConformanceFailureV1")
            .field("suite_id", &PROVIDER_MULTIPART_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_MULTIPART_CONFORMANCE_SUITE_VERSION)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs all canonical multipart cases sequentially without failing fast.
///
/// Every caller must wrap the entire runner in an outer watchdog. This function intentionally has
/// no internal timeout, so a broken assembled executor may remain pending forever. The watchdog
/// must own the complete structured future tree so timeout drops all in-progress executor work
/// without leaving a detached task.
pub async fn run_provider_multipart_conformance_v1(
    executor: &dyn AssembledProviderMultipartExecutorV1,
) -> Result<ProviderMultipartConformanceReportV1, ProviderMultipartConformanceFailureV1> {
    let fixtures = provider_multipart_fixtures_v1();
    let mut passed_case_ids = Vec::with_capacity(fixtures.len());
    let mut mismatches = Vec::with_capacity(MAX_PROVIDER_MULTIPART_MISMATCHES_V1);

    for fixture in fixtures {
        let mismatch_count_before_case = mismatches.len();
        let observation = executor.execute_case(fixture).await;
        compare_multipart_outcome(fixture, &observation, &mut mismatches);
        compare_multipart_evidence(fixture, &observation, &mut mismatches);
        if mismatches.len() == mismatch_count_before_case {
            passed_case_ids.push(fixture.case_id());
        }
    }

    if mismatches.is_empty() {
        Ok(ProviderMultipartConformanceReportV1 { passed_case_ids })
    } else {
        debug_assert!(mismatches.len() <= MAX_PROVIDER_MULTIPART_MISMATCHES_V1);
        Err(ProviderMultipartConformanceFailureV1 {
            evaluated_case_count: fixtures.len(),
            mismatches,
        })
    }
}

fn compare_multipart_outcome(
    fixture: &ProviderMultipartFixtureV1,
    observation: &ProviderMultipartObservationV1,
    mismatches: &mut Vec<ProviderMultipartMismatchV1>,
) {
    match (fixture.expected().outcome(), &observation.outcome) {
        (
            ProviderMultipartExpectedOutcomeV1::Response {
                status,
                body,
                content_type,
                retry_after,
            },
            ProviderMultipartObservedOutcomeV1::Response(response),
        ) => {
            record_if(
                response.status().as_u16() != *status,
                fixture,
                ProviderMultipartMismatchCategoryV1::Status,
                mismatches,
            );
            record_if(
                response.body().as_bytes() != body.as_bytes(),
                fixture,
                ProviderMultipartMismatchCategoryV1::Body,
                mismatches,
            );
            record_if(
                response.content_type() != *content_type,
                fixture,
                ProviderMultipartMismatchCategoryV1::ContentType,
                mismatches,
            );
            record_if(
                response.retry_after() != *retry_after,
                fixture,
                ProviderMultipartMismatchCategoryV1::RetryAfter,
                mismatches,
            );
        }
        (
            ProviderMultipartExpectedOutcomeV1::Failure { code: expected },
            ProviderMultipartObservedOutcomeV1::Failure(observed),
        ) => record_if(
            expected != observed,
            fixture,
            ProviderMultipartMismatchCategoryV1::ErrorCode,
            mismatches,
        ),
        _ => record(fixture, ProviderMultipartMismatchCategoryV1::OutcomeKind, mismatches),
    }
}

fn compare_multipart_evidence(
    fixture: &ProviderMultipartFixtureV1,
    observation: &ProviderMultipartObservationV1,
    mismatches: &mut Vec<ProviderMultipartMismatchV1>,
) {
    let expected = fixture.expected().evidence();
    let observed = observation.evidence();
    record_if(
        expected.resolver_calls() != observed.resolver_calls(),
        fixture,
        ProviderMultipartMismatchCategoryV1::ResolverCallCount,
        mismatches,
    );
    record_if(
        expected.transport_calls() != observed.transport_calls(),
        fixture,
        ProviderMultipartMismatchCategoryV1::TransportCallCount,
        mismatches,
    );
    record_if(
        expected.wire_content_type_exact() != observed.wire_content_type_exact(),
        fixture,
        ProviderMultipartMismatchCategoryV1::WireContentType,
        mismatches,
    );
    record_if(
        expected.wire_body_bytes_exact() != observed.wire_body_bytes_exact(),
        fixture,
        ProviderMultipartMismatchCategoryV1::WireBodyBytes,
        mismatches,
    );
}

fn record_if(
    condition: bool,
    fixture: &ProviderMultipartFixtureV1,
    category: ProviderMultipartMismatchCategoryV1,
    mismatches: &mut Vec<ProviderMultipartMismatchV1>,
) {
    if condition {
        record(fixture, category, mismatches);
    }
}

fn record(
    fixture: &ProviderMultipartFixtureV1,
    category: ProviderMultipartMismatchCategoryV1,
    mismatches: &mut Vec<ProviderMultipartMismatchV1>,
) {
    mismatches.push(ProviderMultipartMismatchV1 { case_id: fixture.case_id(), category });
}

/// Parses one canonical multipart input through the production contract, attaching the fixture's
/// arm.
///
/// Public so a host's own executor can reuse the exact parse the reference executor uses. Two of
/// the five canonical cases fail here rather than at a boundary, which is the point: a body that
/// does not match its declared boundary, and a `content-type` smuggled through the ordinary
/// header channel, are both refused while the request is still a value.
///
/// # Errors
///
/// Returns the closed failure code the fixture table expects for a refused input.
pub fn parse_reference_multipart_input(
    input: &ProviderMultipartInputV1,
    auth_arm: ProviderMultipartAuthArmV1,
) -> Result<(ProviderBindingV1, MultipartPostRequestV1), ProviderCallFailureCodeV1> {
    let endpoint = ProviderEndpointV1::parse(input.endpoint()).map_err(map_contract_error)?;
    let bound_slot =
        CredentialSlotV1::parse(input.bound_credential_slot()).map_err(map_contract_error)?;
    let requested_slot =
        CredentialSlotV1::parse(input.requested_credential_slot()).map_err(map_contract_error)?;
    let relative_path = RelativePathV1::parse(input.relative_path()).map_err(map_contract_error)?;
    let headers = SafeHeaders::try_from_iter(input.headers().iter().copied())
        .map_err(map_canonical_fixture_header_invariant_failure)?;
    let boundary = MultipartBoundaryV1::parse(input.boundary()).map_err(map_contract_error)?;
    let body =
        MultipartBodyV1::parse(input.body().to_vec(), boundary).map_err(map_contract_error)?;
    let slot = BearerAuthV1::new(requested_slot);
    let auth = match auth_arm {
        ProviderMultipartAuthArmV1::Bearer => ProviderAuthV1::Bearer(slot),
        ProviderMultipartAuthArmV1::HeaderSecret(header) => {
            ProviderAuthV1::HeaderSecret { header, slot }
        }
    };
    let binding = ProviderBindingV1::new(endpoint, bound_slot);
    let request = MultipartPostRequestV1::try_new(relative_path, headers, body, auth)
        .map_err(map_contract_error)?;
    Ok((binding, request))
}

/// A deterministic assembled multipart executor built from real `south-core` orchestration and
/// fake ports.
///
/// Every case runs through `execute_multipart_call_v1`. Both wire-shape booleans are measured on
/// the prepared request at the fake transport boundary, mirroring what a real adapter must
/// measure on its wire: the rendered media type and the body bytes.
pub struct ReferenceAssembledProviderMultipartExecutorV1;

impl ReferenceAssembledProviderMultipartExecutorV1 {
    /// Creates an independent reference multipart executor.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for ReferenceAssembledProviderMultipartExecutorV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl AssembledProviderMultipartExecutorV1 for ReferenceAssembledProviderMultipartExecutorV1 {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderMultipartFixtureV1,
    ) -> AssembledProviderMultipartExecutionFutureV1<'a> {
        Box::pin(async move { execute_reference_multipart_case(fixture).await })
    }
}

async fn execute_reference_multipart_case(
    fixture: &ProviderMultipartFixtureV1,
) -> ProviderMultipartObservationV1 {
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let transport_calls = Arc::new(AtomicUsize::new(0));
    let probe = Arc::new(WireShapeProbe::default());

    let (binding, request) =
        match parse_reference_multipart_input(fixture.input(), fixture.auth_arm()) {
            Ok(parsed) => parsed,
            Err(code) => {
                return ProviderMultipartObservationV1::failure(
                    code,
                    ProviderMultipartEvidenceV1::new(0, 0, false, false),
                );
            }
        };

    let resolver =
        ArmSecretResolver { calls: Arc::clone(&resolver_calls), arm: fixture.auth_arm() };
    let transport = WireRecordingTransport {
        calls: Arc::clone(&transport_calls),
        upstream: fixture.upstream(),
        expected_content_type: fixture.input().expected_content_type(),
        expected_body: fixture.input().body(),
        probe: Arc::clone(&probe),
    };
    let cancellation = CancellationToken::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let result = execute_multipart_call_v1(
        &binding,
        &request,
        &resolver,
        &transport,
        deadline,
        &cancellation,
    )
    .await;
    let evidence = ProviderMultipartEvidenceV1::new(
        resolver_calls.load(Ordering::SeqCst),
        transport_calls.load(Ordering::SeqCst),
        probe.content_type_exact.load(Ordering::SeqCst),
        probe.body_bytes_exact.load(Ordering::SeqCst),
    );
    match result {
        Ok(response) => ProviderMultipartObservationV1::response(response, evidence),
        Err(error) => {
            ProviderMultipartObservationV1::failure(map_provider_call_error(&error), evidence)
        }
    }
}

/// Observed wire shape at the fake transport boundary. Both are presence claims, so both start
/// `false` and only a transport call can raise them.
#[derive(Default)]
struct WireShapeProbe {
    content_type_exact: AtomicBool,
    body_bytes_exact: AtomicBool,
}

struct ArmSecretResolver {
    calls: Arc<AtomicUsize>,
    arm: ProviderMultipartAuthArmV1,
}

impl CredentialResolver for ArmSecretResolver {
    fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let secret = match self.arm {
            ProviderMultipartAuthArmV1::Bearer => FAKE_BEARER_SECRET_V1,
            ProviderMultipartAuthArmV1::HeaderSecret(_) => FAKE_HEADER_SECRET_V1,
        };
        Box::pin(async move { Ok(SecretValue::new(secret.to_owned())) })
    }
}

struct WireRecordingTransport<'fixture> {
    calls: Arc<AtomicUsize>,
    upstream: &'fixture ProviderMultipartUpstreamV1,
    expected_content_type: String,
    expected_body: &'static [u8],
    probe: Arc<WireShapeProbe>,
}

impl WireRecordingTransport<'_> {
    fn record_wire_shape(&self, request: &PreparedHttpRequestV1<'_>) {
        // The media type the transport is about to emit, compared against the value rebuilt from
        // the fixture's own boundary — not against South's answer, so this measures agreement
        // rather than self-consistency.
        self.probe.content_type_exact.store(
            request.content_type() == Some(self.expected_content_type.as_str()),
            Ordering::SeqCst,
        );
        // The bytes the transport is about to send, compared against the fixture's declared
        // bytes: a multipart body must reach the wire unmodified, which is the whole reason the
        // host encodes it rather than South.
        self.probe.body_bytes_exact.store(
            request.body().map(south_core::RequestBodyRefV1::as_bytes) == Some(self.expected_body),
            Ordering::SeqCst,
        );
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
            ProviderMultipartUpstreamV1::Response(raw) => {
                let status = StatusCode::from_u16(raw.status());
                let body = raw.body().as_bytes().to_vec();
                let content_type = raw.content_type().map(str::to_owned);
                let retry_after = raw.retry_after().map(str::to_owned);
                Box::pin(async move {
                    let status = status.map_err(|_| TransportErrorV1::ResponseMetadataInvalid)?;
                    BufferedHttpResponseV1::try_from_parts(status, body, content_type, retry_after)
                })
            }
            // `NotReached` must not reach any boundary. Fail closed with the context-free request
            // code rather than panicking.
            ProviderMultipartUpstreamV1::NotReached => {
                Box::pin(async { Err(TransportErrorV1::RequestFailed) })
            }
        }
    }
}
