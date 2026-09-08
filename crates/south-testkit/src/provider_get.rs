//! Assembled-executor runner and reference executor for the buffered-GET suite.

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

use http::{Method, StatusCode};
use south_contracts::{
    BearerAuthV1, BufferedHttpResponseV1, CredentialSlotV1, GetRequestV1, ProviderAuthV1,
    ProviderEndpointV1, QueryStringV1, RelativePathV1, SafeHeaders, TransportErrorV1,
};
use south_core::{
    AsyncHttpTransport, CredentialResolutionFuture, CredentialResolver, PreparedHttpRequestV1,
    ProviderBindingV1, SecretValue, TransportFuture, execute_get_call_v1,
};
use south_provider_conformance::{
    FAKE_BEARER_SECRET_V1, FAKE_HEADER_SECRET_V1, PROVIDER_GET_CONFORMANCE_SUITE_ID,
    PROVIDER_GET_CONFORMANCE_SUITE_VERSION, ProviderCallCountV1, ProviderCallFailureCodeV1,
    ProviderGetAuthArmV1, ProviderGetCaseIdV1, ProviderGetExpectedOutcomeV1, ProviderGetFixtureV1,
    ProviderGetInputV1, ProviderGetUpstreamV1, provider_get_fixtures_v1,
};
use tokio_util::sync::CancellationToken;

use crate::{
    map_canonical_fixture_header_invariant_failure, map_contract_error, map_provider_call_error,
};

/// Four cases multiplied by the eleven closed buffered-GET mismatch categories.
pub const MAX_PROVIDER_GET_MISMATCHES_V1: usize = 44;

/// A boxed, cancellation-safe assembled buffered-GET executor future.
pub type AssembledProviderGetExecutionFutureV1<'a> =
    Pin<Box<dyn Future<Output = ProviderGetObservationV1> + Send + 'a>>;

/// A host-assembled buffered-GET call path exercised by the public buffered-GET runner.
pub trait AssembledProviderGetExecutorV1: Send + Sync {
    /// Executes one immutable canonical buffered-GET fixture.
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderGetFixtureV1,
    ) -> AssembledProviderGetExecutionFutureV1<'a>;
}

/// Adapter-reported resolver, transport, and wire-shape boundary evidence.
///
/// The three wire-shape booleans are measured at the adapter's real transport boundary with the
/// polarities the fixture table defines: `wire_method_get` and `wire_query_exact` are presence
/// claims (`false` until a transport call observes them), `wire_body_absent` is an absence claim
/// (`true` until a transport call observes a body). Like every adapter-reported value, a passing
/// report alone is insufficient for host verification; the adoption review must confirm the
/// wiring.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderGetEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    wire_method_get: bool,
    wire_body_absent: bool,
    wire_query_exact: bool,
}

impl ProviderGetEvidenceV1 {
    /// Constructs evidence and saturates both raw call counts.
    #[must_use]
    pub const fn new(
        resolver_calls: usize,
        transport_calls: usize,
        wire_method_get: bool,
        wire_body_absent: bool,
        wire_query_exact: bool,
    ) -> Self {
        Self {
            resolver_calls: ProviderCallCountV1::from_usize(resolver_calls),
            transport_calls: ProviderCallCountV1::from_usize(transport_calls),
            wire_method_get,
            wire_body_absent,
            wire_query_exact,
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

    /// Returns whether a transport call observed the method `GET`.
    #[must_use]
    pub const fn wire_method_get(&self) -> bool {
        self.wire_method_get
    }

    /// Returns whether no body existed at the transport boundary.
    #[must_use]
    pub const fn wire_body_absent(&self) -> bool {
        self.wire_body_absent
    }

    /// Returns whether the wire carried exactly the query the request declared.
    #[must_use]
    pub const fn wire_query_exact(&self) -> bool {
        self.wire_query_exact
    }
}

impl fmt::Debug for ProviderGetEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderGetEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field("wire_method_get", &self.wire_method_get)
            .field("wire_body_absent", &self.wire_body_absent)
            .field("wire_query_exact", &self.wire_query_exact)
            .finish()
    }
}

enum ProviderGetObservedOutcomeV1 {
    Response(BufferedHttpResponseV1),
    Failure(ProviderCallFailureCodeV1),
}

impl fmt::Debug for ProviderGetObservedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(response) => formatter.debug_tuple("Response").field(response).finish(),
            Self::Failure(code) => formatter.debug_tuple("Failure").field(code).finish(),
        }
    }
}

/// An observed buffered-GET terminal shape plus adapter-reported evidence.
pub struct ProviderGetObservationV1 {
    outcome: ProviderGetObservedOutcomeV1,
    evidence: ProviderGetEvidenceV1,
}

impl ProviderGetObservationV1 {
    /// Constructs a buffered-response observation from an already bounded response.
    #[must_use]
    pub const fn response(
        response: BufferedHttpResponseV1,
        evidence: ProviderGetEvidenceV1,
    ) -> Self {
        Self { outcome: ProviderGetObservedOutcomeV1::Response(response), evidence }
    }

    /// Constructs a failed observation from a closed known failure code.
    #[must_use]
    pub const fn failure(code: ProviderCallFailureCodeV1, evidence: ProviderGetEvidenceV1) -> Self {
        Self { outcome: ProviderGetObservedOutcomeV1::Failure(code), evidence }
    }

    /// Returns the bounded response when this is a buffered-response observation.
    #[must_use]
    pub const fn response_value(&self) -> Option<&BufferedHttpResponseV1> {
        match &self.outcome {
            ProviderGetObservedOutcomeV1::Response(response) => Some(response),
            ProviderGetObservedOutcomeV1::Failure(_) => None,
        }
    }

    /// Returns the closed code when this is a failure observation.
    #[must_use]
    pub const fn failure_code(&self) -> Option<ProviderCallFailureCodeV1> {
        match &self.outcome {
            ProviderGetObservedOutcomeV1::Failure(code) => Some(*code),
            ProviderGetObservedOutcomeV1::Response(_) => None,
        }
    }

    /// Returns adapter-reported boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderGetEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ProviderGetObservationV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderGetObservationV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// The closed reasons why an observed buffered-GET case can differ from its fixture.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderGetMismatchCategoryV1 {
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
    /// Wire-method evidence differed.
    WireMethod,
    /// Wire-body-absence evidence differed.
    WireBody,
    /// Wire-query evidence differed.
    WireQuery,
}

fixed_debug!(ProviderGetMismatchCategoryV1 {
    OutcomeKind => "OutcomeKind",
    ErrorCode => "ErrorCode",
    Status => "Status",
    Body => "Body",
    ContentType => "ContentType",
    RetryAfter => "RetryAfter",
    ResolverCallCount => "ResolverCallCount",
    TransportCallCount => "TransportCallCount",
    WireMethod => "WireMethod",
    WireBody => "WireBody",
    WireQuery => "WireQuery",
});

/// One case/category mismatch without expected or observed payload values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderGetMismatchV1 {
    case_id: ProviderGetCaseIdV1,
    category: ProviderGetMismatchCategoryV1,
}

impl ProviderGetMismatchV1 {
    /// Returns the canonical case that mismatched.
    #[must_use]
    pub const fn case_id(&self) -> ProviderGetCaseIdV1 {
        self.case_id
    }

    /// Returns the closed mismatch category.
    #[must_use]
    pub const fn category(&self) -> ProviderGetMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for ProviderGetMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderGetMismatchV1")
            .field("case_id", &self.case_id)
            .field("category", &self.category)
            .finish()
    }
}

/// A successful report for the complete canonical buffered-GET suite.
pub struct ProviderGetConformanceReportV1 {
    passed_case_ids: Vec<ProviderGetCaseIdV1>,
}

impl ProviderGetConformanceReportV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_GET_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_GET_CONFORMANCE_SUITE_VERSION
    }

    /// Returns all passed cases in canonical table order.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[ProviderGetCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for ProviderGetConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderGetConformanceReportV1")
            .field("suite_id", &PROVIDER_GET_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_GET_CONFORMANCE_SUITE_VERSION)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// A complete bounded mismatch report for the evaluated canonical buffered-GET suite.
pub struct ProviderGetConformanceFailureV1 {
    evaluated_case_count: usize,
    mismatches: Vec<ProviderGetMismatchV1>,
}

impl ProviderGetConformanceFailureV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_GET_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_GET_CONFORMANCE_SUITE_VERSION
    }

    /// Returns how many canonical cases completed evaluation.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Returns every case/category mismatch in canonical evaluation order.
    #[must_use]
    pub fn mismatches(&self) -> &[ProviderGetMismatchV1] {
        &self.mismatches
    }
}

impl fmt::Debug for ProviderGetConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderGetConformanceFailureV1")
            .field("suite_id", &PROVIDER_GET_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_GET_CONFORMANCE_SUITE_VERSION)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs all canonical buffered-GET cases sequentially without failing fast.
///
/// Every caller must wrap the entire runner in an outer watchdog. This function intentionally has
/// no internal timeout, so a broken assembled executor may remain pending forever. The watchdog
/// must own the complete structured future tree so timeout drops all in-progress executor work
/// without leaving a detached task.
pub async fn run_provider_get_conformance_v1(
    executor: &dyn AssembledProviderGetExecutorV1,
) -> Result<ProviderGetConformanceReportV1, ProviderGetConformanceFailureV1> {
    let fixtures = provider_get_fixtures_v1();
    let mut passed_case_ids = Vec::with_capacity(fixtures.len());
    let mut mismatches = Vec::with_capacity(MAX_PROVIDER_GET_MISMATCHES_V1);

    for fixture in fixtures {
        let mismatch_count_before_case = mismatches.len();
        let observation = executor.execute_case(fixture).await;
        compare_provider_get_outcome(fixture, &observation, &mut mismatches);
        compare_provider_get_evidence(fixture, &observation, &mut mismatches);
        if mismatches.len() == mismatch_count_before_case {
            passed_case_ids.push(fixture.case_id());
        }
    }

    if mismatches.is_empty() {
        Ok(ProviderGetConformanceReportV1 { passed_case_ids })
    } else {
        debug_assert!(mismatches.len() <= MAX_PROVIDER_GET_MISMATCHES_V1);
        Err(ProviderGetConformanceFailureV1 { evaluated_case_count: fixtures.len(), mismatches })
    }
}

fn compare_provider_get_outcome(
    fixture: &ProviderGetFixtureV1,
    observation: &ProviderGetObservationV1,
    mismatches: &mut Vec<ProviderGetMismatchV1>,
) {
    match (fixture.expected().outcome(), &observation.outcome) {
        (
            ProviderGetExpectedOutcomeV1::Response { status, body, content_type, retry_after },
            ProviderGetObservedOutcomeV1::Response(response),
        ) => {
            record_if(
                response.status().as_u16() != *status,
                fixture,
                ProviderGetMismatchCategoryV1::Status,
                mismatches,
            );
            record_if(
                response.body().as_bytes() != body.as_bytes(),
                fixture,
                ProviderGetMismatchCategoryV1::Body,
                mismatches,
            );
            record_if(
                response.content_type() != *content_type,
                fixture,
                ProviderGetMismatchCategoryV1::ContentType,
                mismatches,
            );
            record_if(
                response.retry_after() != *retry_after,
                fixture,
                ProviderGetMismatchCategoryV1::RetryAfter,
                mismatches,
            );
        }
        (
            ProviderGetExpectedOutcomeV1::Failure { code: expected },
            ProviderGetObservedOutcomeV1::Failure(observed),
        ) => record_if(
            expected != observed,
            fixture,
            ProviderGetMismatchCategoryV1::ErrorCode,
            mismatches,
        ),
        _ => record(fixture, ProviderGetMismatchCategoryV1::OutcomeKind, mismatches),
    }
}

fn compare_provider_get_evidence(
    fixture: &ProviderGetFixtureV1,
    observation: &ProviderGetObservationV1,
    mismatches: &mut Vec<ProviderGetMismatchV1>,
) {
    let expected = fixture.expected().evidence();
    let observed = observation.evidence();
    record_if(
        expected.resolver_calls() != observed.resolver_calls(),
        fixture,
        ProviderGetMismatchCategoryV1::ResolverCallCount,
        mismatches,
    );
    record_if(
        expected.transport_calls() != observed.transport_calls(),
        fixture,
        ProviderGetMismatchCategoryV1::TransportCallCount,
        mismatches,
    );
    record_if(
        expected.wire_method_get() != observed.wire_method_get(),
        fixture,
        ProviderGetMismatchCategoryV1::WireMethod,
        mismatches,
    );
    record_if(
        expected.wire_body_absent() != observed.wire_body_absent(),
        fixture,
        ProviderGetMismatchCategoryV1::WireBody,
        mismatches,
    );
    record_if(
        expected.wire_query_exact() != observed.wire_query_exact(),
        fixture,
        ProviderGetMismatchCategoryV1::WireQuery,
        mismatches,
    );
}

fn record_if(
    condition: bool,
    fixture: &ProviderGetFixtureV1,
    category: ProviderGetMismatchCategoryV1,
    mismatches: &mut Vec<ProviderGetMismatchV1>,
) {
    if condition {
        record(fixture, category, mismatches);
    }
}

fn record(
    fixture: &ProviderGetFixtureV1,
    category: ProviderGetMismatchCategoryV1,
    mismatches: &mut Vec<ProviderGetMismatchV1>,
) {
    mismatches.push(ProviderGetMismatchV1 { case_id: fixture.case_id(), category });
}

/// A deterministic assembled buffered-GET executor built from real `south-core` orchestration
/// and fake ports.
///
/// Every case runs through `execute_get_call_v1`. The wire-shape booleans are measured on the
/// prepared request at the fake transport boundary, mirroring what a real adapter must measure
/// on its wire: the method, the presence of a body slot, and the URL's query.
pub struct ReferenceAssembledProviderGetExecutorV1;

impl ReferenceAssembledProviderGetExecutorV1 {
    /// Creates an independent reference buffered-GET executor.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for ReferenceAssembledProviderGetExecutorV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl AssembledProviderGetExecutorV1 for ReferenceAssembledProviderGetExecutorV1 {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderGetFixtureV1,
    ) -> AssembledProviderGetExecutionFutureV1<'a> {
        Box::pin(async move { execute_reference_provider_get_case(fixture).await })
    }
}

/// Parses one canonical GET input through the production contract, attaching the fixture's arm.
///
/// Public so a host's own executor can reuse the exact parse the reference executor uses; how a
/// host obtains the raw strings in production is its own business.
///
/// # Errors
///
/// Returns the closed failure code the fixture table expects for a refused input.
pub fn parse_reference_get_input(
    input: &ProviderGetInputV1,
    auth_arm: ProviderGetAuthArmV1,
) -> Result<(ProviderBindingV1, GetRequestV1), ProviderCallFailureCodeV1> {
    let endpoint = ProviderEndpointV1::parse(input.endpoint()).map_err(map_contract_error)?;
    let bound_slot =
        CredentialSlotV1::parse(input.bound_credential_slot()).map_err(map_contract_error)?;
    let requested_slot =
        CredentialSlotV1::parse(input.requested_credential_slot()).map_err(map_contract_error)?;
    let relative_path = RelativePathV1::parse(input.relative_path()).map_err(map_contract_error)?;
    let headers = SafeHeaders::try_from_iter(input.headers().iter().copied())
        .map_err(map_canonical_fixture_header_invariant_failure)?;
    let slot = BearerAuthV1::new(requested_slot);
    let auth = match auth_arm {
        ProviderGetAuthArmV1::Bearer => ProviderAuthV1::Bearer(slot),
        ProviderGetAuthArmV1::HeaderSecret(header) => ProviderAuthV1::HeaderSecret { header, slot },
    };
    let binding = ProviderBindingV1::new(endpoint, bound_slot);
    Ok((binding, GetRequestV1::new(relative_path, headers, auth)))
}

fn declare_reference_query(
    fixture: &ProviderGetFixtureV1,
) -> Result<Option<QueryStringV1>, ProviderCallFailureCodeV1> {
    if fixture.declared_query().is_empty() {
        return Ok(None);
    }
    QueryStringV1::try_from_iter(fixture.declared_query().iter().copied())
        .map(Some)
        .map_err(map_contract_error)
}

async fn execute_reference_provider_get_case(
    fixture: &ProviderGetFixtureV1,
) -> ProviderGetObservationV1 {
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let transport_calls = Arc::new(AtomicUsize::new(0));
    let wire_shape = Arc::new(WireShapeProbe::default());

    let (binding, request) = match parse_reference_get_input(fixture.input(), fixture.auth_arm()) {
        Ok(parsed) => parsed,
        Err(code) => {
            return ProviderGetObservationV1::failure(
                code,
                ProviderGetEvidenceV1::new(0, 0, false, true, false),
            );
        }
    };
    let declared_query = match declare_reference_query(fixture) {
        Ok(query) => query,
        Err(code) => {
            return ProviderGetObservationV1::failure(
                code,
                ProviderGetEvidenceV1::new(0, 0, false, true, false),
            );
        }
    };
    let request = match declared_query.clone() {
        Some(query) => request.with_query(query),
        None => request,
    };

    let resolver =
        ArmSecretResolver { calls: Arc::clone(&resolver_calls), arm: fixture.auth_arm() };
    let transport = WireRecordingTransport {
        calls: Arc::clone(&transport_calls),
        upstream: fixture.upstream(),
        declared_query,
        wire_shape: Arc::clone(&wire_shape),
    };
    let cancellation = CancellationToken::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let result =
        execute_get_call_v1(&binding, &request, &resolver, &transport, deadline, &cancellation)
            .await;
    let evidence = ProviderGetEvidenceV1::new(
        resolver_calls.load(Ordering::SeqCst),
        transport_calls.load(Ordering::SeqCst),
        wire_shape.method_get.load(Ordering::SeqCst),
        wire_shape.body_absent.load(Ordering::SeqCst),
        wire_shape.query_exact.load(Ordering::SeqCst),
    );
    match result {
        Ok(response) => ProviderGetObservationV1::response(response, evidence),
        Err(error) => ProviderGetObservationV1::failure(map_provider_call_error(&error), evidence),
    }
}

/// Observed wire shape at the fake transport boundary, with the table's polarities: the two
/// presence claims start `false`, the absence claim starts `true`.
struct WireShapeProbe {
    method_get: AtomicBool,
    body_absent: AtomicBool,
    query_exact: AtomicBool,
}

impl Default for WireShapeProbe {
    fn default() -> Self {
        Self {
            method_get: AtomicBool::new(false),
            body_absent: AtomicBool::new(true),
            query_exact: AtomicBool::new(false),
        }
    }
}

/// Yields the fake secret the declared arm's suite uses, so a wire assertion in a host's own
/// executor can compare against the same constant the header-auth and provider-call suites do.
struct ArmSecretResolver {
    calls: Arc<AtomicUsize>,
    arm: ProviderGetAuthArmV1,
}

impl CredentialResolver for ArmSecretResolver {
    fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let secret = match self.arm {
            ProviderGetAuthArmV1::Bearer => FAKE_BEARER_SECRET_V1,
            ProviderGetAuthArmV1::HeaderSecret(_) => FAKE_HEADER_SECRET_V1,
        };
        Box::pin(async move { Ok(SecretValue::new(secret.to_owned())) })
    }
}

struct WireRecordingTransport<'fixture> {
    calls: Arc<AtomicUsize>,
    upstream: &'fixture ProviderGetUpstreamV1,
    declared_query: Option<QueryStringV1>,
    wire_shape: Arc<WireShapeProbe>,
}

impl WireRecordingTransport<'_> {
    fn record_wire_shape(&self, request: &PreparedHttpRequestV1<'_>) {
        self.wire_shape.method_get.store(request.method() == Method::GET, Ordering::SeqCst);
        // A body-less request has no body slot, not an empty one: `None`, never `Some("")`.
        self.wire_shape.body_absent.store(request.body().is_none(), Ordering::SeqCst);
        // The controlled-query polarity: a declaration to compare against *and* a wire query
        // equal to it. A request that declared nothing reaches this boundary and answers `false`.
        let wire = request.url().query();
        let declared = self.declared_query.as_ref().map(QueryStringV1::as_str);
        self.wire_shape.query_exact.store(declared.is_some() && wire == declared, Ordering::SeqCst);
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
            ProviderGetUpstreamV1::Response(raw) => {
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
            ProviderGetUpstreamV1::NotReached => {
                Box::pin(async { Err(TransportErrorV1::RequestFailed) })
            }
        }
    }
}
