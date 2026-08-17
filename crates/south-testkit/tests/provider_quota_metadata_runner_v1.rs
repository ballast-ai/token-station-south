use std::{future::pending, sync::Arc};

use south_contracts::{ProviderQuotaMetadataFieldV1, ProviderQuotaMetadataV1};
use south_provider_conformance::{
    PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_ID,
    PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_VERSION, ProviderCallCountV1,
    ProviderCallFailureCodeV1, ProviderQuotaMetadataFixtureV1, provider_quota_metadata_fixtures_v1,
};
use south_testkit::{
    AssembledProviderQuotaMetadataExecutionFutureV1, AssembledProviderQuotaMetadataExecutorV1,
    MAX_PROVIDER_QUOTA_METADATA_MISMATCHES_V1, ProviderQuotaMetadataEvidenceV1,
    ProviderQuotaMetadataMismatchCategoryV1, ProviderQuotaMetadataObservationV1,
    run_provider_quota_metadata_conformance_v1,
};

const FIELDS: [ProviderQuotaMetadataFieldV1; 9] = [
    ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens,
    ProviderQuotaMetadataFieldV1::XRateLimitRemainingTokens,
    ProviderQuotaMetadataFieldV1::XRateLimitResetTokens,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensLimit,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensRemaining,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensReset,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedLimit,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedRemaining,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedReset,
];

const CATEGORIES: [ProviderQuotaMetadataMismatchCategoryV1; 12] = [
    ProviderQuotaMetadataMismatchCategoryV1::OutcomeKind,
    ProviderQuotaMetadataMismatchCategoryV1::XRateLimitLimitTokens,
    ProviderQuotaMetadataMismatchCategoryV1::XRateLimitRemainingTokens,
    ProviderQuotaMetadataMismatchCategoryV1::XRateLimitResetTokens,
    ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitTokensLimit,
    ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitTokensRemaining,
    ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitTokensReset,
    ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitUnifiedLimit,
    ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitUnifiedRemaining,
    ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitUnifiedReset,
    ProviderQuotaMetadataMismatchCategoryV1::ResolverCallCount,
    ProviderQuotaMetadataMismatchCategoryV1::TransportCallCount,
];

struct MatchingExecutor;

impl AssembledProviderQuotaMetadataExecutorV1 for MatchingExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderQuotaMetadataFixtureV1,
    ) -> AssembledProviderQuotaMetadataExecutionFutureV1<'a> {
        Box::pin(async move {
            ProviderQuotaMetadataObservationV1::response(
                metadata_from_fixture(fixture, None),
                ProviderQuotaMetadataEvidenceV1::new(1, 1),
            )
        })
    }
}

#[tokio::test]
async fn object_safe_runner_reports_both_cases_in_canonical_order() {
    let dynamic: &dyn AssembledProviderQuotaMetadataExecutorV1 = &MatchingExecutor;
    let report = run_provider_quota_metadata_conformance_v1(dynamic)
        .await
        .expect("matching assembled executor should pass");

    assert_eq!(report.suite_id(), PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_ID);
    assert_eq!(report.suite_version(), PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_VERSION);
    assert_eq!(report.passed_case_ids().len(), 2);
    assert_eq!(
        report.passed_case_ids(),
        &provider_quota_metadata_fixtures_v1()
            .iter()
            .map(ProviderQuotaMetadataFixtureV1::case_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(MAX_PROVIDER_QUOTA_METADATA_MISMATCHES_V1, 24);
}

struct SingleMismatchExecutor {
    category: ProviderQuotaMetadataMismatchCategoryV1,
}

impl AssembledProviderQuotaMetadataExecutorV1 for SingleMismatchExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderQuotaMetadataFixtureV1,
    ) -> AssembledProviderQuotaMetadataExecutionFutureV1<'a> {
        Box::pin(async move {
            if fixture.case_id() == provider_quota_metadata_fixtures_v1()[0].case_id() {
                mismatched_observation(fixture, self.category)
            } else {
                ProviderQuotaMetadataObservationV1::response(
                    metadata_from_fixture(fixture, None),
                    ProviderQuotaMetadataEvidenceV1::new(1, 1),
                )
            }
        })
    }
}

#[tokio::test]
async fn every_dimension_reports_exactly_one_isolated_mismatch() {
    for category in CATEGORIES {
        let failure =
            run_provider_quota_metadata_conformance_v1(&SingleMismatchExecutor { category })
                .await
                .expect_err("one isolated difference must fail the suite");
        assert_eq!(failure.evaluated_case_count(), 2);
        assert_eq!(failure.mismatches().len(), 1);
        assert_eq!(
            failure.mismatches()[0].case_id(),
            provider_quota_metadata_fixtures_v1()[0].case_id()
        );
        assert_eq!(failure.mismatches()[0].category(), category);
    }
}

struct PendingExecutor {
    dropped: Arc<std::sync::atomic::AtomicBool>,
}

struct DropFlag(Arc<std::sync::atomic::AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl AssembledProviderQuotaMetadataExecutorV1 for PendingExecutor {
    fn execute_case<'a>(
        &'a self,
        _fixture: &'a ProviderQuotaMetadataFixtureV1,
    ) -> AssembledProviderQuotaMetadataExecutionFutureV1<'a> {
        Box::pin(async move {
            let _drop_flag = DropFlag(Arc::clone(&self.dropped));
            pending().await
        })
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn caller_watchdog_drops_a_permanently_pending_runner() {
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let executor = PendingExecutor { dropped: Arc::clone(&dropped) };

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        run_provider_quota_metadata_conformance_v1(&executor),
    )
    .await;

    assert!(result.is_err());
    assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn observations_and_reports_redact_all_fixture_values() {
    let fixture = &provider_quota_metadata_fixtures_v1()[0];
    let observation = ProviderQuotaMetadataObservationV1::response(
        metadata_from_fixture(fixture, None),
        ProviderQuotaMetadataEvidenceV1::new(1, 1),
    );
    let rendered = format!("{observation:?}");
    for field in FIELDS {
        let value = fixture
            .upstream_metadata()
            .value(field)
            .expect("all-fields fixture must contain every value");
        assert!(!rendered.contains(value));
    }
}

#[test]
fn evidence_saturates_large_counts_without_narrowing() {
    let evidence = ProviderQuotaMetadataEvidenceV1::new(256, 257);

    assert_eq!(evidence.resolver_calls(), ProviderCallCountV1::MoreThanOne);
    assert_eq!(evidence.transport_calls(), ProviderCallCountV1::MoreThanOne);
}

fn mismatched_observation(
    fixture: &ProviderQuotaMetadataFixtureV1,
    category: ProviderQuotaMetadataMismatchCategoryV1,
) -> ProviderQuotaMetadataObservationV1 {
    if category == ProviderQuotaMetadataMismatchCategoryV1::OutcomeKind {
        return ProviderQuotaMetadataObservationV1::failure(
            ProviderCallFailureCodeV1::RequestFailed,
            ProviderQuotaMetadataEvidenceV1::new(1, 1),
        );
    }
    let evidence = ProviderQuotaMetadataEvidenceV1::new(
        usize::from(category != ProviderQuotaMetadataMismatchCategoryV1::ResolverCallCount),
        usize::from(category != ProviderQuotaMetadataMismatchCategoryV1::TransportCallCount),
    );
    ProviderQuotaMetadataObservationV1::response(
        metadata_from_fixture(fixture, category_field(category)),
        evidence,
    )
}

const fn category_field(
    category: ProviderQuotaMetadataMismatchCategoryV1,
) -> Option<ProviderQuotaMetadataFieldV1> {
    match category {
        ProviderQuotaMetadataMismatchCategoryV1::OutcomeKind
        | ProviderQuotaMetadataMismatchCategoryV1::ResolverCallCount
        | ProviderQuotaMetadataMismatchCategoryV1::TransportCallCount => None,
        ProviderQuotaMetadataMismatchCategoryV1::XRateLimitLimitTokens => {
            Some(ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens)
        }
        ProviderQuotaMetadataMismatchCategoryV1::XRateLimitRemainingTokens => {
            Some(ProviderQuotaMetadataFieldV1::XRateLimitRemainingTokens)
        }
        ProviderQuotaMetadataMismatchCategoryV1::XRateLimitResetTokens => {
            Some(ProviderQuotaMetadataFieldV1::XRateLimitResetTokens)
        }
        ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitTokensLimit => {
            Some(ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensLimit)
        }
        ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitTokensRemaining => {
            Some(ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensRemaining)
        }
        ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitTokensReset => {
            Some(ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensReset)
        }
        ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitUnifiedLimit => {
            Some(ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedLimit)
        }
        ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitUnifiedRemaining => {
            Some(ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedRemaining)
        }
        ProviderQuotaMetadataMismatchCategoryV1::AnthropicRateLimitUnifiedReset => {
            Some(ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedReset)
        }
    }
}

fn metadata_from_fixture(
    fixture: &ProviderQuotaMetadataFixtureV1,
    omitted: Option<ProviderQuotaMetadataFieldV1>,
) -> ProviderQuotaMetadataV1 {
    ProviderQuotaMetadataV1::try_from_iter(FIELDS.into_iter().filter_map(|field| {
        (Some(field) != omitted)
            .then(|| fixture.expected_metadata().value(field))
            .flatten()
            .map(|value| (field, value.to_owned()))
    }))
    .expect("canonical fixture metadata must satisfy the production contract")
}
