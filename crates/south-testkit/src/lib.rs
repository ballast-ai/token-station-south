#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Reusable public contract tests for south consumers.

use std::{
    fmt,
    future::{Future, pending},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use http::StatusCode;
use south_contracts::{
    BearerAuthV1, BufferedHttpResponseV1, ContractErrorV1, CredentialSlotV1, HeaderPolicyError,
    JsonBodyV1, JsonPostRequestV1, PreparationErrorV1, ProviderEndpointV1, RelativePathV1,
    SafeHeaders, TransportErrorV1,
};
use south_core::{
    AsyncHttpTransport, CredentialResolutionFuture, CredentialResolver, PreparedHttpRequestV1,
    ProviderBindingV1, ProviderCallErrorV1, SecretValue, TransportFuture, execute_provider_call_v1,
};
use south_provider_conformance::{
    FAKE_BEARER_SECRET_V1, PROVIDER_CALL_CONFORMANCE_DEADLINE_OFFSET_V1,
    PROVIDER_CALL_CONFORMANCE_SUITE_ID, PROVIDER_CALL_CONFORMANCE_SUITE_VERSION,
    ProviderCallCaseIdV1, ProviderCallControlV1, ProviderCallCountV1,
    ProviderCallExpectedOutcomeV1, ProviderCallFailureCodeV1, ProviderCallFixtureV1,
    ProviderCallUpstreamV1, provider_call_fixtures_v1,
};
use tokio::sync::{Notify, oneshot};
use tokio_util::sync::CancellationToken;

macro_rules! fixed_debug {
    ($type:ty { $($variant:ident => $name:literal),+ $(,)? }) => {
        impl fmt::Debug for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    $(Self::$variant => formatter.write_str($name),)+
                }
            }
        }
    };
}

mod controlled_query;
mod controlled_user_agent;
mod header_auth;
mod quota;
mod raw;
mod stream;

pub use raw::RawProviderCallBuilderV1;

pub use controlled_query::{
    AssembledControlledQueryExecutionFutureV1, AssembledControlledQueryExecutorV1,
    ControlledQueryConformanceFailureV1, ControlledQueryConformanceReportV1,
    ControlledQueryEvidenceV1, ControlledQueryMismatchCategoryV1, ControlledQueryMismatchV1,
    ControlledQueryObservationV1, MAX_CONTROLLED_QUERY_MISMATCHES_V1,
    ReferenceAssembledControlledQueryExecutorV1, run_controlled_query_conformance_v1,
};
pub use controlled_user_agent::{
    AssembledControlledUserAgentExecutionFutureV1, AssembledControlledUserAgentExecutorV1,
    ControlledUserAgentConformanceFailureV1, ControlledUserAgentConformanceReportV1,
    ControlledUserAgentEvidenceV1, ControlledUserAgentMismatchCategoryV1,
    ControlledUserAgentMismatchV1, ControlledUserAgentObservationV1,
    MAX_CONTROLLED_USER_AGENT_MISMATCHES_V1, ReferenceAssembledControlledUserAgentExecutorV1,
    run_controlled_user_agent_conformance_v1,
};
pub use header_auth::{
    AssembledHeaderAuthExecutionFutureV1, AssembledHeaderAuthExecutorV1,
    HeaderAuthConformanceFailureV1, HeaderAuthConformanceReportV1, HeaderAuthEvidenceV1,
    HeaderAuthMismatchCategoryV1, HeaderAuthMismatchV1, HeaderAuthObservationV1,
    MAX_HEADER_AUTH_MISMATCHES_V1, ReferenceAssembledHeaderAuthExecutorV1,
    run_header_auth_conformance_v1,
};
pub use quota::{
    AssembledProviderQuotaMetadataExecutionFutureV1, AssembledProviderQuotaMetadataExecutorV1,
    MAX_PROVIDER_QUOTA_METADATA_MISMATCHES_V1, ProviderQuotaMetadataConformanceFailureV1,
    ProviderQuotaMetadataConformanceReportV1, ProviderQuotaMetadataEvidenceV1,
    ProviderQuotaMetadataMismatchCategoryV1, ProviderQuotaMetadataMismatchV1,
    ProviderQuotaMetadataObservationV1, ReferenceAssembledProviderQuotaMetadataExecutorV1,
    run_provider_quota_metadata_conformance_v1,
};

pub use stream::{
    AssembledProviderStreamExecutorV1, AssembledStreamExecutionFutureV1,
    MAX_PROVIDER_STREAM_MISMATCHES_V1, ProviderStreamConformanceFailureV1,
    ProviderStreamConformanceReportV1, ProviderStreamEvidenceV1, ProviderStreamMismatchCategoryV1,
    ProviderStreamMismatchV1, ProviderStreamObservationV1,
    ReferenceAssembledProviderStreamExecutorV1, run_provider_stream_conformance_v1,
};

/// Seven cases multiplied by the ten closed mismatch categories.
pub const MAX_PROVIDER_CALL_MISMATCHES_V1: usize = 70;

/// A boxed, cancellation-safe assembled-executor future.
pub type AssembledExecutionFutureV1<'a> =
    Pin<Box<dyn Future<Output = ProviderCallObservationV1> + Send + 'a>>;

/// A host-assembled provider-call path exercised by the public conformance runner.
pub trait AssembledProviderCallExecutorV1: Send + Sync {
    /// Executes one immutable canonical fixture.
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderCallFixtureV1,
    ) -> AssembledExecutionFutureV1<'a>;
}

/// Adapter-reported resolver and transport boundary evidence.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderCallEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    resolver_future_dropped_while_pending: bool,
    transport_future_dropped_while_pending: bool,
}

impl ProviderCallEvidenceV1 {
    /// Constructs evidence and saturates both raw call counts.
    #[must_use]
    pub const fn new(
        resolver_calls: usize,
        transport_calls: usize,
        resolver_future_dropped_while_pending: bool,
        transport_future_dropped_while_pending: bool,
    ) -> Self {
        Self {
            resolver_calls: ProviderCallCountV1::from_usize(resolver_calls),
            transport_calls: ProviderCallCountV1::from_usize(transport_calls),
            resolver_future_dropped_while_pending,
            transport_future_dropped_while_pending,
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

    /// Returns whether a pending resolver future was dropped.
    #[must_use]
    pub const fn resolver_future_dropped_while_pending(&self) -> bool {
        self.resolver_future_dropped_while_pending
    }

    /// Returns whether a pending transport future was dropped.
    #[must_use]
    pub const fn transport_future_dropped_while_pending(&self) -> bool {
        self.transport_future_dropped_while_pending
    }
}

impl fmt::Debug for ProviderCallEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderCallEvidenceV1")
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
            .finish()
    }
}

enum ProviderCallObservedOutcomeV1 {
    Response(BufferedHttpResponseV1),
    Failure(ProviderCallFailureCodeV1),
}

impl fmt::Debug for ProviderCallObservedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(response) => formatter.debug_tuple("Response").field(response).finish(),
            Self::Failure(code) => formatter.debug_tuple("Failure").field(code).finish(),
        }
    }
}

/// A bounded response or closed failure plus adapter-reported evidence.
pub struct ProviderCallObservationV1 {
    outcome: ProviderCallObservedOutcomeV1,
    evidence: ProviderCallEvidenceV1,
}

impl ProviderCallObservationV1 {
    /// Constructs a successful observation from an already bounded response.
    #[must_use]
    pub const fn response(
        response: BufferedHttpResponseV1,
        evidence: ProviderCallEvidenceV1,
    ) -> Self {
        Self { outcome: ProviderCallObservedOutcomeV1::Response(response), evidence }
    }

    /// Constructs a failed observation from a closed known failure code.
    #[must_use]
    pub const fn failure(
        code: ProviderCallFailureCodeV1,
        evidence: ProviderCallEvidenceV1,
    ) -> Self {
        Self { outcome: ProviderCallObservedOutcomeV1::Failure(code), evidence }
    }

    /// Returns the bounded response when this is a response observation.
    #[must_use]
    pub const fn response_value(&self) -> Option<&BufferedHttpResponseV1> {
        match &self.outcome {
            ProviderCallObservedOutcomeV1::Response(response) => Some(response),
            ProviderCallObservedOutcomeV1::Failure(_) => None,
        }
    }

    /// Returns the closed code when this is a failure observation.
    #[must_use]
    pub const fn failure_code(&self) -> Option<ProviderCallFailureCodeV1> {
        match &self.outcome {
            ProviderCallObservedOutcomeV1::Response(_) => None,
            ProviderCallObservedOutcomeV1::Failure(code) => Some(*code),
        }
    }

    /// Returns adapter-reported resolver and transport evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderCallEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ProviderCallObservationV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderCallObservationV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// The closed reasons why an observed case can differ from its fixture.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderCallMismatchCategoryV1 {
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
    /// Pending resolver drop evidence differed.
    ResolverPendingDrop,
    /// Pending transport drop evidence differed.
    TransportPendingDrop,
}

fixed_debug!(ProviderCallMismatchCategoryV1 {
    OutcomeKind => "OutcomeKind",
    ErrorCode => "ErrorCode",
    Status => "Status",
    Body => "Body",
    ContentType => "ContentType",
    RetryAfter => "RetryAfter",
    ResolverCallCount => "ResolverCallCount",
    TransportCallCount => "TransportCallCount",
    ResolverPendingDrop => "ResolverPendingDrop",
    TransportPendingDrop => "TransportPendingDrop",
});

/// One case/category mismatch without expected or observed payload values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderCallMismatchV1 {
    case_id: ProviderCallCaseIdV1,
    category: ProviderCallMismatchCategoryV1,
}

impl ProviderCallMismatchV1 {
    /// Returns the canonical case that mismatched.
    #[must_use]
    pub const fn case_id(&self) -> ProviderCallCaseIdV1 {
        self.case_id
    }

    /// Returns the closed mismatch category.
    #[must_use]
    pub const fn category(&self) -> ProviderCallMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for ProviderCallMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderCallMismatchV1")
            .field("case_id", &self.case_id)
            .field("category", &self.category)
            .finish()
    }
}

/// A successful report for the complete canonical suite.
pub struct ProviderCallConformanceReportV1 {
    passed_case_ids: Vec<ProviderCallCaseIdV1>,
}

impl ProviderCallConformanceReportV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_CALL_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_CALL_CONFORMANCE_SUITE_VERSION
    }

    /// Returns all passed cases in canonical table order.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[ProviderCallCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for ProviderCallConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderCallConformanceReportV1")
            .field("suite_id", &PROVIDER_CALL_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_CALL_CONFORMANCE_SUITE_VERSION)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// A complete bounded mismatch report for the evaluated canonical suite.
pub struct ProviderCallConformanceFailureV1 {
    evaluated_case_count: usize,
    mismatches: Vec<ProviderCallMismatchV1>,
}

impl ProviderCallConformanceFailureV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_CALL_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_CALL_CONFORMANCE_SUITE_VERSION
    }

    /// Returns how many canonical cases completed evaluation.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Returns every case/category mismatch in canonical evaluation order.
    #[must_use]
    pub fn mismatches(&self) -> &[ProviderCallMismatchV1] {
        &self.mismatches
    }
}

impl fmt::Debug for ProviderCallConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderCallConformanceFailureV1")
            .field("suite_id", &PROVIDER_CALL_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_CALL_CONFORMANCE_SUITE_VERSION)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs all canonical provider-call cases sequentially without failing fast.
///
/// Every caller must wrap the entire runner and any deadline driver in an outer watchdog. This
/// function intentionally has no internal timeout, so a broken assembled executor may remain
/// pending forever. The watchdog must own the complete structured future tree so timeout drops all
/// in-progress executor work without leaving a detached task.
pub async fn run_provider_call_conformance_v1(
    executor: &dyn AssembledProviderCallExecutorV1,
) -> Result<ProviderCallConformanceReportV1, ProviderCallConformanceFailureV1> {
    let fixtures = provider_call_fixtures_v1();
    let mut passed_case_ids = Vec::with_capacity(fixtures.len());
    let mut mismatches = Vec::with_capacity(MAX_PROVIDER_CALL_MISMATCHES_V1);

    for fixture in fixtures {
        let mismatch_count_before_case = mismatches.len();
        let observation = executor.execute_case(fixture).await;
        compare_outcome(fixture, &observation, &mut mismatches);
        compare_evidence(fixture, &observation, &mut mismatches);
        if mismatches.len() == mismatch_count_before_case {
            passed_case_ids.push(fixture.case_id());
        }
    }

    if mismatches.is_empty() {
        Ok(ProviderCallConformanceReportV1 { passed_case_ids })
    } else {
        debug_assert!(mismatches.len() <= MAX_PROVIDER_CALL_MISMATCHES_V1);
        Err(ProviderCallConformanceFailureV1 { evaluated_case_count: fixtures.len(), mismatches })
    }
}

/// A deterministic assembled executor built from real `south-core` orchestration and fake ports.
///
/// Cases must execute sequentially. A deadline transport start grants one consumable notification
/// permit. If a caller aborts after that transport starts but before consuming the corresponding
/// notification, it must discard this executor and construct a new one; reusing an executor with
/// an abandoned permit could associate that stale permit with a later deadline case.
pub struct ReferenceAssembledProviderCallExecutorV1 {
    deadline_transport_started: Arc<Notify>,
}

impl ReferenceAssembledProviderCallExecutorV1 {
    /// Creates an independent reference executor.
    #[must_use]
    pub fn new() -> Self {
        Self { deadline_transport_started: Arc::new(Notify::new()) }
    }

    /// Consumes one notification that the deadline fixture's pending transport has started.
    ///
    /// [`Notify`] retains at most one permit when no waiter exists, so callers may create or poll
    /// this future before or after the transport reaches its pending boundary.
    pub async fn deadline_transport_started(&self) {
        self.deadline_transport_started.notified().await;
    }
}

impl Default for ReferenceAssembledProviderCallExecutorV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl AssembledProviderCallExecutorV1 for ReferenceAssembledProviderCallExecutorV1 {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderCallFixtureV1,
    ) -> AssembledExecutionFutureV1<'a> {
        Box::pin(async move { self.execute_reference_case(fixture).await })
    }
}

impl ReferenceAssembledProviderCallExecutorV1 {
    async fn execute_reference_case(
        &self,
        fixture: &ProviderCallFixtureV1,
    ) -> ProviderCallObservationV1 {
        let resolver_calls = Arc::new(AtomicUsize::new(0));
        let transport_calls = Arc::new(AtomicUsize::new(0));
        let resolver_dropped = Arc::new(AtomicBool::new(false));
        let transport_dropped = Arc::new(AtomicBool::new(false));

        let parsed = parse_reference_input(fixture.input());
        let (binding, request) = match parsed {
            Ok(parsed) => parsed,
            Err(code) => {
                return ProviderCallObservationV1::failure(
                    code,
                    ProviderCallEvidenceV1::new(0, 0, false, false),
                );
            }
        };

        let cancellation = CancellationToken::new();
        let (resolver_started_sender, resolver_started_receiver) = oneshot::channel();
        let resolver = ReferenceResolver {
            calls: Arc::clone(&resolver_calls),
            pending: matches!(fixture.control(), ProviderCallControlV1::CancelWhileResolverPending),
            started: Mutex::new(Some(resolver_started_sender)),
            dropped: Arc::clone(&resolver_dropped),
        };
        let transport = ReferenceTransport {
            calls: Arc::clone(&transport_calls),
            upstream: fixture.upstream(),
            deadline_started: self.deadline_transport_started.clone(),
            dropped: Arc::clone(&transport_dropped),
        };
        let deadline = tokio::time::Instant::now()
            + if matches!(fixture.control(), ProviderCallControlV1::ExpireWhileTransportPending) {
                PROVIDER_CALL_CONFORMANCE_DEADLINE_OFFSET_V1
            } else {
                Duration::from_secs(30)
            };

        let result =
            if matches!(fixture.control(), ProviderCallControlV1::CancelWhileResolverPending) {
                let call = execute_provider_call_v1(
                    &binding,
                    &request,
                    &resolver,
                    &transport,
                    deadline,
                    &cancellation,
                );
                let cancel = async {
                    let _ = resolver_started_receiver.await;
                    cancellation.cancel();
                };
                let (result, ()) = tokio::join!(call, cancel);
                result
            } else {
                execute_provider_call_v1(
                    &binding,
                    &request,
                    &resolver,
                    &transport,
                    deadline,
                    &cancellation,
                )
                .await
            };

        let evidence = ProviderCallEvidenceV1::new(
            resolver_calls.load(Ordering::SeqCst),
            transport_calls.load(Ordering::SeqCst),
            resolver_dropped.load(Ordering::SeqCst),
            transport_dropped.load(Ordering::SeqCst),
        );
        match result {
            Ok(response) => ProviderCallObservationV1::response(response, evidence),
            Err(error) => {
                ProviderCallObservationV1::failure(map_provider_call_error(&error), evidence)
            }
        }
    }
}

fn parse_reference_input(
    input: &south_provider_conformance::ProviderCallInputV1,
) -> Result<(ProviderBindingV1, JsonPostRequestV1), ProviderCallFailureCodeV1> {
    parse_reference_input_with_auth(input, |slot| BearerAuthV1::new(slot).into())
}

fn parse_reference_input_with_auth(
    input: &south_provider_conformance::ProviderCallInputV1,
    auth: impl FnOnce(CredentialSlotV1) -> south_contracts::ProviderAuthV1,
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
    let request = JsonPostRequestV1::new(relative_path, headers, body, auth(requested_slot));
    Ok((binding, request))
}

struct ReferenceResolver {
    calls: Arc<AtomicUsize>,
    pending: bool,
    started: Mutex<Option<oneshot::Sender<()>>>,
    dropped: Arc<AtomicBool>,
}

impl CredentialResolver for ReferenceResolver {
    fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.pending {
            let started = {
                let mut sender = match self.started.lock() {
                    Ok(sender) => sender,
                    Err(poisoned) => poisoned.into_inner(),
                };
                sender.take()
            };
            let dropped = Arc::clone(&self.dropped);
            Box::pin(async move {
                let _drop_flag = PendingDropFlag(dropped);
                if let Some(started) = started {
                    let _ = started.send(());
                }
                pending().await
            })
        } else {
            Box::pin(async { Ok(SecretValue::new(FAKE_BEARER_SECRET_V1.to_owned())) })
        }
    }
}

struct ReferenceTransport<'fixture> {
    calls: Arc<AtomicUsize>,
    upstream: &'fixture ProviderCallUpstreamV1,
    deadline_started: Arc<Notify>,
    dropped: Arc<AtomicBool>,
}

impl AsyncHttpTransport for ReferenceTransport<'_> {
    fn execute<'a>(
        &'a self,
        _request: &'a PreparedHttpRequestV1<'_>,
        _remaining_timeout: Duration,
    ) -> TransportFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.upstream {
            ProviderCallUpstreamV1::Response(raw) => {
                let status = StatusCode::from_u16(raw.status());
                let body = raw.body().as_bytes().to_vec();
                let content_type = raw.content_type().map(str::to_owned);
                let retry_after = raw.retry_after().map(str::to_owned);
                Box::pin(async move {
                    let status = status.map_err(|_| TransportErrorV1::ResponseMetadataInvalid)?;
                    BufferedHttpResponseV1::try_from_parts(status, body, content_type, retry_after)
                })
            }
            ProviderCallUpstreamV1::TransportFailure(error) => {
                let error = *error;
                Box::pin(async move { Err(error) })
            }
            ProviderCallUpstreamV1::Pending => {
                let started = Arc::clone(&self.deadline_started);
                let dropped = Arc::clone(&self.dropped);
                Box::pin(async move {
                    let _drop_flag = PendingDropFlag(dropped);
                    started.notify_one();
                    pending().await
                })
            }
            ProviderCallUpstreamV1::NotReached => {
                Box::pin(async { Err(TransportErrorV1::RequestFailed) })
            }
        }
    }
}

struct PendingDropFlag(Arc<AtomicBool>);

impl Drop for PendingDropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

// The query arms deliberately share `InvalidRelativePath`'s body rather than merging into its
// arm: keeping them separate is what lets the comment below record *why* they fold into that
// code, and it makes a future query-specific code a one-line change at the right place.
#[allow(clippy::match_same_arms)]
const fn map_contract_error(error: ContractErrorV1) -> ProviderCallFailureCodeV1 {
    match error {
        ContractErrorV1::InvalidEndpoint => ProviderCallFailureCodeV1::InvalidEndpoint,
        ContractErrorV1::InvalidRelativePath => ProviderCallFailureCodeV1::InvalidRelativePath,
        ContractErrorV1::InvalidCredentialSlot => ProviderCallFailureCodeV1::InvalidCredentialSlot,
        ContractErrorV1::InvalidJsonBody => ProviderCallFailureCodeV1::InvalidJsonBody,
        ContractErrorV1::RequestBodyTooLarge => ProviderCallFailureCodeV1::RequestBodyTooLarge,
        // The frozen 19-code set predates controlled query and deliberately stays frozen: both
        // hosts' verified status and evidence records are burned to those ids. A query is part of
        // the provider-selected destination, and every query contract error is a preparation-time
        // failure with zero resolver and transport calls — exactly the shape of
        // `InvalidRelativePath` — so query errors fold into that code rather than widening the
        // contract. The distinct `ContractErrorV1` variants stay available to hosts that want the
        // finer reason in their own logs.
        ContractErrorV1::InvalidQueryValue
        | ContractErrorV1::DuplicateQueryParameter
        | ContractErrorV1::EmptyQuery
        | ContractErrorV1::QueryTooLarge => ProviderCallFailureCodeV1::InvalidRelativePath,
        // The controlled user-agent declaration error is the same preparation-time, zero-call
        // shape, and the frozen set has exactly one preparation-time provider-declaration code —
        // so both sanctioned channels fold their declaration errors into it.
        ContractErrorV1::InvalidUserAgentValue => ProviderCallFailureCodeV1::InvalidRelativePath,
    }
}

// The frozen 19-code provider-call set intentionally has no header-policy code, so header
// validation failures surface through the context-free request fallback rather than panicking or
// expanding the stable code contract. For the provider-call, stream, quota, and query suites this
// path is an internal fixture invariant — their canonical headers always parse. The controlled
// user-agent suite's reserved-header case exercises it deliberately: its fixture smuggles a plain
// `user-agent` pair through the ordinary channel and expects exactly this fold at zero calls.
const fn map_canonical_fixture_header_invariant_failure(
    _error: HeaderPolicyError,
) -> ProviderCallFailureCodeV1 {
    ProviderCallFailureCodeV1::RequestFailed
}

const fn map_provider_call_error(error: &ProviderCallErrorV1) -> ProviderCallFailureCodeV1 {
    match error {
        ProviderCallErrorV1::Preparation(error) => match error {
            PreparationErrorV1::UrlOutsideBinding => ProviderCallFailureCodeV1::UrlOutsideBinding,
            PreparationErrorV1::CredentialBindingMismatch => {
                ProviderCallFailureCodeV1::CredentialBindingMismatch
            }
            PreparationErrorV1::CredentialResolutionFailed => {
                ProviderCallFailureCodeV1::CredentialResolutionFailed
            }
            PreparationErrorV1::Cancelled => ProviderCallFailureCodeV1::Cancelled,
            PreparationErrorV1::DeadlineExceeded => ProviderCallFailureCodeV1::DeadlineExceeded,
            // `PreparationErrorV1` is `#[non_exhaustive]` since 0.7.0 (host-prelude D2/D4). No
            // frozen fixture can produce a newer preparation variant, so one reaching this
            // mapping is an executor wiring error: use the context-free request fallback rather
            // than expanding the frozen buffered code set (same ruling as the rejection arm
            // below).
            _ => ProviderCallFailureCodeV1::RequestFailed,
        },
        ProviderCallErrorV1::Transport(error) => match error {
            TransportErrorV1::ClientBuildFailed => ProviderCallFailureCodeV1::ClientBuildFailed,
            TransportErrorV1::TransportTimeout => ProviderCallFailureCodeV1::TransportTimeout,
            TransportErrorV1::ConnectFailed => ProviderCallFailureCodeV1::ConnectFailed,
            TransportErrorV1::RequestFailed => ProviderCallFailureCodeV1::RequestFailed,
            TransportErrorV1::ResponseReadFailed => ProviderCallFailureCodeV1::ResponseReadFailed,
            TransportErrorV1::ResponseBodyTooLarge => {
                ProviderCallFailureCodeV1::ResponseBodyTooLarge
            }
            TransportErrorV1::ResponseBodyNotUtf8 => ProviderCallFailureCodeV1::ResponseBodyNotUtf8,
            TransportErrorV1::ResponseMetadataInvalid => {
                ProviderCallFailureCodeV1::ResponseMetadataInvalid
            }
            TransportErrorV1::RedirectDenied => ProviderCallFailureCodeV1::RedirectDenied,
        },
        // The buffered entry point can never produce the streaming-only rejection variant, so a
        // rejection reaching this buffered mapping is an executor wiring error. Use the
        // context-free request fallback rather than expanding the frozen buffered code set.
        ProviderCallErrorV1::Rejected(_) => ProviderCallFailureCodeV1::RequestFailed,
    }
}

fn compare_outcome(
    fixture: &ProviderCallFixtureV1,
    observation: &ProviderCallObservationV1,
    mismatches: &mut Vec<ProviderCallMismatchV1>,
) {
    match (fixture.expected().outcome(), &observation.outcome) {
        (
            ProviderCallExpectedOutcomeV1::Response { status, body, content_type, retry_after },
            ProviderCallObservedOutcomeV1::Response(response),
        ) => {
            record_if(
                response.status().as_u16() != *status,
                fixture,
                ProviderCallMismatchCategoryV1::Status,
                mismatches,
            );
            record_if(
                response.body().as_bytes() != body.as_bytes(),
                fixture,
                ProviderCallMismatchCategoryV1::Body,
                mismatches,
            );
            record_if(
                response.content_type() != *content_type,
                fixture,
                ProviderCallMismatchCategoryV1::ContentType,
                mismatches,
            );
            record_if(
                response.retry_after() != *retry_after,
                fixture,
                ProviderCallMismatchCategoryV1::RetryAfter,
                mismatches,
            );
        }
        (
            ProviderCallExpectedOutcomeV1::Failure { code: expected },
            ProviderCallObservedOutcomeV1::Failure(observed),
        ) => record_if(
            expected != observed,
            fixture,
            ProviderCallMismatchCategoryV1::ErrorCode,
            mismatches,
        ),
        _ => record(fixture, ProviderCallMismatchCategoryV1::OutcomeKind, mismatches),
    }
}

fn compare_evidence(
    fixture: &ProviderCallFixtureV1,
    observation: &ProviderCallObservationV1,
    mismatches: &mut Vec<ProviderCallMismatchV1>,
) {
    let expected = fixture.expected().evidence();
    let observed = observation.evidence();
    record_if(
        expected.resolver_calls() != observed.resolver_calls(),
        fixture,
        ProviderCallMismatchCategoryV1::ResolverCallCount,
        mismatches,
    );
    record_if(
        expected.transport_calls() != observed.transport_calls(),
        fixture,
        ProviderCallMismatchCategoryV1::TransportCallCount,
        mismatches,
    );
    record_if(
        expected.resolver_future_dropped_while_pending()
            != observed.resolver_future_dropped_while_pending(),
        fixture,
        ProviderCallMismatchCategoryV1::ResolverPendingDrop,
        mismatches,
    );
    record_if(
        expected.transport_future_dropped_while_pending()
            != observed.transport_future_dropped_while_pending(),
        fixture,
        ProviderCallMismatchCategoryV1::TransportPendingDrop,
        mismatches,
    );
}

fn record_if(
    condition: bool,
    fixture: &ProviderCallFixtureV1,
    category: ProviderCallMismatchCategoryV1,
    mismatches: &mut Vec<ProviderCallMismatchV1>,
) {
    if condition {
        record(fixture, category, mismatches);
    }
}

fn record(
    fixture: &ProviderCallFixtureV1,
    category: ProviderCallMismatchCategoryV1,
    mismatches: &mut Vec<ProviderCallMismatchV1>,
) {
    mismatches.push(ProviderCallMismatchV1 { case_id: fixture.case_id(), category });
}

#[cfg(test)]
mod tests {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{Arc, Mutex, atomic::AtomicBool, atomic::AtomicUsize},
        time::Duration,
    };

    use south_contracts::CredentialSlotV1;
    use south_core::CredentialResolver;
    use tokio::sync::oneshot;

    use super::ReferenceResolver;

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn pending_resolver_recovers_its_start_sender_from_a_poisoned_mutex() {
        let (started_sender, started_receiver) = oneshot::channel();
        let started = Mutex::new(Some(started_sender));
        let poisoned = catch_unwind(AssertUnwindSafe(|| {
            let _guard = started.lock().expect("test mutex should initially be available");
            panic!("poison the test mutex");
        }));
        assert!(poisoned.is_err());
        let resolver = ReferenceResolver {
            calls: Arc::new(AtomicUsize::new(0)),
            pending: true,
            started,
            dropped: Arc::new(AtomicBool::new(false)),
        };
        let slot = CredentialSlotV1::parse("test-slot").expect("test slot should be valid");
        let mut resolution = resolver.resolve(&slot);
        let wait_for_start = async {
            tokio::select! {
                result = started_receiver => {
                    result.expect("poison recovery must retain the start sender");
                }
                _ = &mut resolution => panic!("pending resolution unexpectedly completed"),
            }
        };

        tokio::time::timeout(Duration::from_secs(1), wait_for_start)
            .await
            .expect("poison recovery start signal timed out");
        drop(resolution);
    }
}
