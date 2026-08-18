use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use http::StatusCode;
use south_contracts::{
    BufferedHttpResponseV1, CredentialSlotV1, ProviderQuotaMetadataFieldV1,
    ProviderQuotaMetadataV1, TransportErrorV1,
};
use south_core::{
    AsyncHttpTransport, CredentialResolutionFuture, CredentialResolver, PreparedHttpRequestV1,
    SecretValue, TransportFuture, execute_provider_call_v1,
};
use south_provider_conformance::{
    FAKE_BEARER_SECRET_V1, PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_ID,
    PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_VERSION, ProviderCallCountV1,
    ProviderCallFailureCodeV1, ProviderQuotaMetadataCaseIdV1,
    ProviderQuotaMetadataExpectedOutcomeV1, ProviderQuotaMetadataFixtureV1,
    ProviderQuotaMetadataRawV1, ProviderQuotaMetadataUpstreamV1,
    provider_quota_metadata_fixtures_v1,
};
use tokio_util::sync::CancellationToken;

/// Three canonical cases multiplied by thirteen closed mismatch categories.
pub const MAX_PROVIDER_QUOTA_METADATA_MISMATCHES_V1: usize = 39;

/// A boxed, cancellation-safe assembled quota metadata executor future.
pub type AssembledProviderQuotaMetadataExecutionFutureV1<'a> =
    Pin<Box<dyn Future<Output = ProviderQuotaMetadataObservationV1> + Send + 'a>>;

/// A host-assembled provider-call path exercised for quota metadata propagation.
pub trait AssembledProviderQuotaMetadataExecutorV1: Send + Sync {
    /// Executes one immutable canonical fixture through the host adapter.
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderQuotaMetadataFixtureV1,
    ) -> AssembledProviderQuotaMetadataExecutionFutureV1<'a>;
}

/// Adapter-reported resolver and transport counts for one metadata case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderQuotaMetadataEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
}

impl ProviderQuotaMetadataEvidenceV1 {
    /// Constructs evidence and saturates both raw call counts.
    #[must_use]
    pub const fn new(resolver_calls: usize, transport_calls: usize) -> Self {
        Self {
            resolver_calls: ProviderCallCountV1::from_usize(resolver_calls),
            transport_calls: ProviderCallCountV1::from_usize(transport_calls),
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
}

impl fmt::Debug for ProviderQuotaMetadataEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderQuotaMetadataEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .finish()
    }
}

enum ProviderQuotaMetadataObservedOutcomeV1 {
    Response(ProviderQuotaMetadataV1),
    Failure(ProviderCallFailureCodeV1),
}

impl fmt::Debug for ProviderQuotaMetadataObservedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(metadata) => formatter.debug_tuple("Response").field(metadata).finish(),
            Self::Failure(code) => formatter.debug_tuple("Failure").field(code).finish(),
        }
    }
}

/// Bounded quota metadata or one closed call failure plus adapter-reported evidence.
pub struct ProviderQuotaMetadataObservationV1 {
    outcome: ProviderQuotaMetadataObservedOutcomeV1,
    evidence: ProviderQuotaMetadataEvidenceV1,
}

impl ProviderQuotaMetadataObservationV1 {
    /// Constructs a successful observation from bounded metadata.
    #[must_use]
    pub const fn response(
        metadata: ProviderQuotaMetadataV1,
        evidence: ProviderQuotaMetadataEvidenceV1,
    ) -> Self {
        Self { outcome: ProviderQuotaMetadataObservedOutcomeV1::Response(metadata), evidence }
    }

    /// Constructs a failed observation from a closed provider-call code.
    #[must_use]
    pub const fn failure(
        code: ProviderCallFailureCodeV1,
        evidence: ProviderQuotaMetadataEvidenceV1,
    ) -> Self {
        Self { outcome: ProviderQuotaMetadataObservedOutcomeV1::Failure(code), evidence }
    }

    /// Returns bounded metadata for a response observation.
    #[must_use]
    pub const fn response_metadata(&self) -> Option<&ProviderQuotaMetadataV1> {
        match &self.outcome {
            ProviderQuotaMetadataObservedOutcomeV1::Response(metadata) => Some(metadata),
            ProviderQuotaMetadataObservedOutcomeV1::Failure(_) => None,
        }
    }

    /// Returns the closed failure code for a failed observation.
    #[must_use]
    pub const fn failure_code(&self) -> Option<ProviderCallFailureCodeV1> {
        match self.outcome {
            ProviderQuotaMetadataObservedOutcomeV1::Response(_) => None,
            ProviderQuotaMetadataObservedOutcomeV1::Failure(code) => Some(code),
        }
    }

    /// Returns adapter-reported resolver and transport evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderQuotaMetadataEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ProviderQuotaMetadataObservationV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderQuotaMetadataObservationV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// The closed reasons an observed metadata case can differ from its fixture.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderQuotaMetadataMismatchCategoryV1 {
    /// Response versus failure differed.
    OutcomeKind,
    /// Both sides failed, but the closed failure code differed.
    FailureCode,
    /// `x-ratelimit-limit-tokens` differed.
    XRateLimitLimitTokens,
    /// `x-ratelimit-remaining-tokens` differed.
    XRateLimitRemainingTokens,
    /// `x-ratelimit-reset-tokens` differed.
    XRateLimitResetTokens,
    /// `anthropic-ratelimit-tokens-limit` differed.
    AnthropicRateLimitTokensLimit,
    /// `anthropic-ratelimit-tokens-remaining` differed.
    AnthropicRateLimitTokensRemaining,
    /// `anthropic-ratelimit-tokens-reset` differed.
    AnthropicRateLimitTokensReset,
    /// `anthropic-ratelimit-unified-limit` differed.
    AnthropicRateLimitUnifiedLimit,
    /// `anthropic-ratelimit-unified-remaining` differed.
    AnthropicRateLimitUnifiedRemaining,
    /// `anthropic-ratelimit-unified-reset` differed.
    AnthropicRateLimitUnifiedReset,
    /// Resolver call count differed.
    ResolverCallCount,
    /// Transport call count differed.
    TransportCallCount,
}

impl fmt::Debug for ProviderQuotaMetadataMismatchCategoryV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::OutcomeKind => "OutcomeKind",
            Self::FailureCode => "FailureCode",
            Self::XRateLimitLimitTokens => "XRateLimitLimitTokens",
            Self::XRateLimitRemainingTokens => "XRateLimitRemainingTokens",
            Self::XRateLimitResetTokens => "XRateLimitResetTokens",
            Self::AnthropicRateLimitTokensLimit => "AnthropicRateLimitTokensLimit",
            Self::AnthropicRateLimitTokensRemaining => "AnthropicRateLimitTokensRemaining",
            Self::AnthropicRateLimitTokensReset => "AnthropicRateLimitTokensReset",
            Self::AnthropicRateLimitUnifiedLimit => "AnthropicRateLimitUnifiedLimit",
            Self::AnthropicRateLimitUnifiedRemaining => "AnthropicRateLimitUnifiedRemaining",
            Self::AnthropicRateLimitUnifiedReset => "AnthropicRateLimitUnifiedReset",
            Self::ResolverCallCount => "ResolverCallCount",
            Self::TransportCallCount => "TransportCallCount",
        })
    }
}

/// One case/category mismatch without raw expected or observed values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderQuotaMetadataMismatchV1 {
    case_id: ProviderQuotaMetadataCaseIdV1,
    category: ProviderQuotaMetadataMismatchCategoryV1,
}

impl ProviderQuotaMetadataMismatchV1 {
    /// Returns the canonical case that mismatched.
    #[must_use]
    pub const fn case_id(&self) -> ProviderQuotaMetadataCaseIdV1 {
        self.case_id
    }

    /// Returns the closed mismatch category.
    #[must_use]
    pub const fn category(&self) -> ProviderQuotaMetadataMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for ProviderQuotaMetadataMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderQuotaMetadataMismatchV1")
            .field("case_id", &self.case_id)
            .field("category", &self.category)
            .finish()
    }
}

/// A successful report for the complete metadata extension suite.
pub struct ProviderQuotaMetadataConformanceReportV1 {
    passed_case_ids: Vec<ProviderQuotaMetadataCaseIdV1>,
}

impl ProviderQuotaMetadataConformanceReportV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_VERSION
    }

    /// Returns all passed cases in canonical order.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[ProviderQuotaMetadataCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for ProviderQuotaMetadataConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderQuotaMetadataConformanceReportV1")
            .field("suite_id", &PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_VERSION)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// A complete bounded mismatch report for the metadata extension suite.
pub struct ProviderQuotaMetadataConformanceFailureV1 {
    evaluated_case_count: usize,
    mismatches: Vec<ProviderQuotaMetadataMismatchV1>,
}

impl ProviderQuotaMetadataConformanceFailureV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_VERSION
    }

    /// Returns how many canonical cases completed evaluation.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Returns every case/category mismatch in canonical order.
    #[must_use]
    pub fn mismatches(&self) -> &[ProviderQuotaMetadataMismatchV1] {
        &self.mismatches
    }
}

impl fmt::Debug for ProviderQuotaMetadataConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderQuotaMetadataConformanceFailureV1")
            .field("suite_id", &PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_VERSION)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs both metadata cases sequentially without failing fast.
///
/// Every caller must wrap the complete runner in an outer structured watchdog. The runner has no
/// internal timer, and dropping it must also drop any in-progress executor future.
pub async fn run_provider_quota_metadata_conformance_v1(
    executor: &dyn AssembledProviderQuotaMetadataExecutorV1,
) -> Result<ProviderQuotaMetadataConformanceReportV1, ProviderQuotaMetadataConformanceFailureV1> {
    let fixtures = provider_quota_metadata_fixtures_v1();
    let mut passed_case_ids = Vec::with_capacity(fixtures.len());
    let mut mismatches = Vec::with_capacity(MAX_PROVIDER_QUOTA_METADATA_MISMATCHES_V1);

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
        Ok(ProviderQuotaMetadataConformanceReportV1 { passed_case_ids })
    } else {
        debug_assert!(mismatches.len() <= MAX_PROVIDER_QUOTA_METADATA_MISMATCHES_V1);
        Err(ProviderQuotaMetadataConformanceFailureV1 {
            evaluated_case_count: fixtures.len(),
            mismatches,
        })
    }
}

/// A deterministic metadata extension executor using real `south-core` orchestration.
pub struct ReferenceAssembledProviderQuotaMetadataExecutorV1;

impl ReferenceAssembledProviderQuotaMetadataExecutorV1 {
    /// Creates an independent reference executor.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for ReferenceAssembledProviderQuotaMetadataExecutorV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl AssembledProviderQuotaMetadataExecutorV1
    for ReferenceAssembledProviderQuotaMetadataExecutorV1
{
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderQuotaMetadataFixtureV1,
    ) -> AssembledProviderQuotaMetadataExecutionFutureV1<'a> {
        Box::pin(async move { execute_reference_case(fixture).await })
    }
}

async fn execute_reference_case(
    fixture: &ProviderQuotaMetadataFixtureV1,
) -> ProviderQuotaMetadataObservationV1 {
    let (binding, request) = match super::parse_reference_input(fixture.input()) {
        Ok(parsed) => parsed,
        Err(code) => {
            return ProviderQuotaMetadataObservationV1::failure(
                code,
                ProviderQuotaMetadataEvidenceV1::new(0, 0),
            );
        }
    };
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let transport_calls = Arc::new(AtomicUsize::new(0));
    let resolver = QuotaMetadataResolver { calls: Arc::clone(&resolver_calls) };
    let transport = QuotaMetadataTransport {
        calls: Arc::clone(&transport_calls),
        upstream: *fixture.upstream(),
    };
    let result = execute_provider_call_v1(
        &binding,
        &request,
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await;
    let evidence = ProviderQuotaMetadataEvidenceV1::new(
        resolver_calls.load(Ordering::SeqCst),
        transport_calls.load(Ordering::SeqCst),
    );

    match result {
        Ok(response) => ProviderQuotaMetadataObservationV1::response(
            response.provider_quota_metadata().clone(),
            evidence,
        ),
        Err(error) => ProviderQuotaMetadataObservationV1::failure(
            super::map_provider_call_error(&error),
            evidence,
        ),
    }
}

struct QuotaMetadataResolver {
    calls: Arc<AtomicUsize>,
}

impl CredentialResolver for QuotaMetadataResolver {
    fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(SecretValue::new(FAKE_BEARER_SECRET_V1.to_owned())) })
    }
}

struct QuotaMetadataTransport {
    calls: Arc<AtomicUsize>,
    upstream: ProviderQuotaMetadataUpstreamV1,
}

impl AsyncHttpTransport for QuotaMetadataTransport {
    fn execute<'a>(
        &'a self,
        _request: &'a PreparedHttpRequestV1<'_>,
        _remaining_timeout: Duration,
    ) -> TransportFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let upstream = self.upstream;
        Box::pin(async move {
            let metadata = match upstream {
                ProviderQuotaMetadataUpstreamV1::Metadata(raw) => raw,
                // Reaching a `NotReached` transport is itself the failure the case exists to
                // detect. Surfacing it as a transport error keeps the counter increment above
                // visible to the evidence comparison rather than silently succeeding.
                ProviderQuotaMetadataUpstreamV1::NotReached => {
                    return Err(TransportErrorV1::RequestFailed);
                }
            };
            BufferedHttpResponseV1::try_from_parts_with_provider_quota_metadata(
                StatusCode::OK,
                b"{}".to_vec(),
                None,
                None,
                bounded_metadata(&metadata)?,
            )
        })
    }
}

fn bounded_metadata(
    raw: &ProviderQuotaMetadataRawV1,
) -> Result<ProviderQuotaMetadataV1, TransportErrorV1> {
    ProviderQuotaMetadataV1::try_from_iter(
        FIELD_CATEGORIES
            .into_iter()
            .filter_map(|(field, _)| raw.value(field).map(|value| (field, value.to_owned()))),
    )
}

const FIELD_CATEGORIES: [(ProviderQuotaMetadataFieldV1, ProviderQuotaMetadataMismatchCategoryV1);
    9] = [
    (
        ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens,
        ProviderQuotaMetadataMismatchCategoryV1::XRateLimitLimitTokens,
    ),
    (
        ProviderQuotaMetadataFieldV1::XRateLimitRemainingTokens,
        ProviderQuotaMetadataMismatchCategoryV1::XRateLimitRemainingTokens,
    ),
    (
        ProviderQuotaMetadataFieldV1::XRateLimitResetTokens,
        ProviderQuotaMetadataMismatchCategoryV1::XRateLimitResetTokens,
    ),
    (
        ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensLimit,
        ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitTokensLimit,
    ),
    (
        ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensRemaining,
        ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitTokensRemaining,
    ),
    (
        ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensReset,
        ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitTokensReset,
    ),
    (
        ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedLimit,
        ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitUnifiedLimit,
    ),
    (
        ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedRemaining,
        ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitUnifiedRemaining,
    ),
    (
        ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedReset,
        ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitUnifiedReset,
    ),
];

fn compare_outcome(
    fixture: &ProviderQuotaMetadataFixtureV1,
    observation: &ProviderQuotaMetadataObservationV1,
    mismatches: &mut Vec<ProviderQuotaMetadataMismatchV1>,
) {
    match (fixture.expected_outcome(), &observation.outcome) {
        (
            ProviderQuotaMetadataExpectedOutcomeV1::Metadata(expected),
            ProviderQuotaMetadataObservedOutcomeV1::Response(actual),
        ) => {
            for (field, category) in FIELD_CATEGORIES {
                record_if(
                    actual.value(field) != expected.value(field),
                    fixture,
                    category,
                    mismatches,
                );
            }
        }
        (
            ProviderQuotaMetadataExpectedOutcomeV1::Failure { code },
            ProviderQuotaMetadataObservedOutcomeV1::Failure(actual),
        ) => {
            record_if(
                actual != code,
                fixture,
                ProviderQuotaMetadataMismatchCategoryV1::FailureCode,
                mismatches,
            );
        }
        (
            ProviderQuotaMetadataExpectedOutcomeV1::Metadata(_),
            ProviderQuotaMetadataObservedOutcomeV1::Failure(_),
        )
        | (
            ProviderQuotaMetadataExpectedOutcomeV1::Failure { .. },
            ProviderQuotaMetadataObservedOutcomeV1::Response(_),
        ) => {
            record(fixture, ProviderQuotaMetadataMismatchCategoryV1::OutcomeKind, mismatches);
        }
    }
}

fn compare_evidence(
    fixture: &ProviderQuotaMetadataFixtureV1,
    observation: &ProviderQuotaMetadataObservationV1,
    mismatches: &mut Vec<ProviderQuotaMetadataMismatchV1>,
) {
    record_if(
        observation.evidence.resolver_calls != fixture.expected_evidence().resolver_calls(),
        fixture,
        ProviderQuotaMetadataMismatchCategoryV1::ResolverCallCount,
        mismatches,
    );
    record_if(
        observation.evidence.transport_calls != fixture.expected_evidence().transport_calls(),
        fixture,
        ProviderQuotaMetadataMismatchCategoryV1::TransportCallCount,
        mismatches,
    );
}

fn record_if(
    condition: bool,
    fixture: &ProviderQuotaMetadataFixtureV1,
    category: ProviderQuotaMetadataMismatchCategoryV1,
    mismatches: &mut Vec<ProviderQuotaMetadataMismatchV1>,
) {
    if condition {
        record(fixture, category, mismatches);
    }
}

fn record(
    fixture: &ProviderQuotaMetadataFixtureV1,
    category: ProviderQuotaMetadataMismatchCategoryV1,
    mismatches: &mut Vec<ProviderQuotaMetadataMismatchV1>,
) {
    mismatches.push(ProviderQuotaMetadataMismatchV1 { case_id: fixture.case_id(), category });
}
