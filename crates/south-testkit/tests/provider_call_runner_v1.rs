use std::{
    collections::BTreeSet,
    fmt::Display,
    future::{Future, pending},
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use http::StatusCode;
use south_contracts::BufferedHttpResponseV1;
use south_provider_conformance::{
    PROVIDER_CALL_CONFORMANCE_SUITE_ID, PROVIDER_CALL_CONFORMANCE_SUITE_VERSION,
    ProviderCallCaseIdV1, ProviderCallExpectedOutcomeV1, ProviderCallFailureCodeV1,
    ProviderCallFixtureV1, provider_call_fixtures_v1,
};
use south_testkit::{
    AssembledExecutionFutureV1, AssembledProviderCallExecutorV1, MAX_PROVIDER_CALL_MISMATCHES_V1,
    ProviderCallConformanceFailureV1, ProviderCallConformanceReportV1, ProviderCallEvidenceV1,
    ProviderCallMismatchCategoryV1, ProviderCallObservationV1, run_provider_call_conformance_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ProviderCallObservationV1: Display);
assert_not_impl_any!(ProviderCallEvidenceV1: Display);
assert_not_impl_any!(ProviderCallConformanceReportV1: Display);
assert_not_impl_any!(ProviderCallConformanceFailureV1: Display);

#[derive(Default)]
struct MatchingExecutor {
    order: Mutex<Vec<ProviderCallCaseIdV1>>,
}

impl AssembledProviderCallExecutorV1 for MatchingExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderCallFixtureV1,
    ) -> AssembledExecutionFutureV1<'a> {
        Box::pin(async move {
            self.order.lock().expect("order lock should be available").push(fixture.case_id());
            observation_matching(fixture)
        })
    }
}

#[tokio::test]
async fn object_safe_send_executor_runs_every_case_in_canonical_order() {
    let executor = MatchingExecutor::default();
    let dynamic: &dyn AssembledProviderCallExecutorV1 = &executor;
    let future = dynamic.execute_case(&provider_call_fixtures_v1()[0]);
    assert_send(&future);
    drop(future);

    let report =
        run_provider_call_conformance_v1(dynamic).await.expect("matching observations must pass");
    assert_eq!(report.suite_id(), PROVIDER_CALL_CONFORMANCE_SUITE_ID);
    assert_eq!(report.suite_version(), PROVIDER_CALL_CONFORMANCE_SUITE_VERSION);
    assert_eq!(
        report.passed_case_ids(),
        &provider_call_fixtures_v1().iter().map(ProviderCallFixtureV1::case_id).collect::<Vec<_>>()
    );
    assert_eq!(
        *executor.order.lock().expect("order lock should be available"),
        provider_call_fixtures_v1().iter().map(ProviderCallFixtureV1::case_id).collect::<Vec<_>>()
    );
}

struct MismatchExecutor;

impl AssembledProviderCallExecutorV1 for MismatchExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderCallFixtureV1,
    ) -> AssembledExecutionFutureV1<'a> {
        Box::pin(async move {
            let evidence = ProviderCallEvidenceV1::new(257, 256, true, true);
            match fixture.case_id() {
                ProviderCallCaseIdV1::Success => ProviderCallObservationV1::response(
                    BufferedHttpResponseV1::try_from_parts(
                        StatusCode::OK,
                        b"wrong-body".to_vec(),
                        None,
                        None,
                    )
                    .expect("mismatch response remains within contract"),
                    evidence,
                ),
                ProviderCallCaseIdV1::InvalidRelativePath => {
                    ProviderCallObservationV1::response(valid_response(), evidence)
                }
                _ => ProviderCallObservationV1::failure(
                    ProviderCallFailureCodeV1::RequestFailed,
                    evidence,
                ),
            }
        })
    }
}

#[tokio::test]
async fn runner_collects_every_mismatch_category_without_failing_fast() {
    let failure = run_provider_call_conformance_v1(&MismatchExecutor)
        .await
        .expect_err("deliberate mismatches must fail");
    assert_eq!(failure.suite_id(), PROVIDER_CALL_CONFORMANCE_SUITE_ID);
    assert_eq!(failure.suite_version(), PROVIDER_CALL_CONFORMANCE_SUITE_VERSION);
    assert_eq!(failure.evaluated_case_count(), 7);
    assert!(failure.mismatches().len() <= MAX_PROVIDER_CALL_MISMATCHES_V1);

    let categories: BTreeSet<_> =
        failure.mismatches().iter().map(south_testkit::ProviderCallMismatchV1::category).collect();
    assert_eq!(
        categories,
        BTreeSet::from([
            ProviderCallMismatchCategoryV1::OutcomeKind,
            ProviderCallMismatchCategoryV1::ErrorCode,
            ProviderCallMismatchCategoryV1::Status,
            ProviderCallMismatchCategoryV1::Body,
            ProviderCallMismatchCategoryV1::ContentType,
            ProviderCallMismatchCategoryV1::RetryAfter,
            ProviderCallMismatchCategoryV1::ResolverCallCount,
            ProviderCallMismatchCategoryV1::TransportCallCount,
            ProviderCallMismatchCategoryV1::ResolverPendingDrop,
            ProviderCallMismatchCategoryV1::TransportPendingDrop,
        ])
    );
    for fixture in provider_call_fixtures_v1() {
        assert!(failure.mismatches().iter().any(|m| m.case_id() == fixture.case_id()));
    }
}

#[test]
fn evidence_construction_saturates_large_counts() {
    let evidence = ProviderCallEvidenceV1::new(256, 257, false, true);
    assert_eq!(
        evidence.resolver_calls(),
        south_provider_conformance::ProviderCallCountV1::MoreThanOne
    );
    assert_eq!(
        evidence.transport_calls(),
        south_provider_conformance::ProviderCallCountV1::MoreThanOne
    );
    assert!(!evidence.resolver_future_dropped_while_pending());
    assert!(evidence.transport_future_dropped_while_pending());
}

#[test]
fn debug_output_contains_only_safe_structural_evidence() {
    const RESPONSE_SENTINEL: &str = "runner-response-debug-sentinel";
    const METADATA_SENTINEL: &str = "runner-metadata-debug-sentinel";
    let observation = ProviderCallObservationV1::response(
        BufferedHttpResponseV1::try_from_parts(
            StatusCode::CREATED,
            RESPONSE_SENTINEL.as_bytes().to_vec(),
            Some(METADATA_SENTINEL.to_owned()),
            Some(METADATA_SENTINEL.to_owned()),
        )
        .expect("sentinel response should satisfy bounds"),
        ProviderCallEvidenceV1::new(1, 1, false, false),
    );
    let debug = format!("{observation:?}");
    assert!(!debug.contains(RESPONSE_SENTINEL));
    assert!(!debug.contains(METADATA_SENTINEL));
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

impl AssembledProviderCallExecutorV1 for PendingExecutor {
    fn execute_case<'a>(
        &'a self,
        _fixture: &'a ProviderCallFixtureV1,
    ) -> AssembledExecutionFutureV1<'a> {
        Box::pin(async move {
            let _drop_flag = DropFlag(Arc::clone(&self.dropped));
            pending().await
        })
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn caller_watchdog_drops_a_permanently_pending_runner_without_detached_work() {
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let executor = PendingExecutor { dropped: Arc::clone(&dropped) };
    let structured_run = async { run_provider_call_conformance_v1(&executor).await };

    let result = tokio::time::timeout(Duration::from_secs(5), structured_run).await;

    assert!(result.is_err());
    assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
}

fn observation_matching(fixture: &ProviderCallFixtureV1) -> ProviderCallObservationV1 {
    let expected = fixture.expected();
    let evidence = expected.evidence();
    let observed_evidence = ProviderCallEvidenceV1::new(
        count_value(evidence.resolver_calls()),
        count_value(evidence.transport_calls()),
        evidence.resolver_future_dropped_while_pending(),
        evidence.transport_future_dropped_while_pending(),
    );
    match expected.outcome() {
        ProviderCallExpectedOutcomeV1::Response { status, body, content_type, retry_after } => {
            ProviderCallObservationV1::response(
                BufferedHttpResponseV1::try_from_parts(
                    StatusCode::from_u16(*status).expect("expected status should be valid"),
                    body.as_bytes().to_vec(),
                    content_type.map(str::to_owned),
                    retry_after.map(str::to_owned),
                )
                .expect("expected response should satisfy the production contract"),
                observed_evidence,
            )
        }
        ProviderCallExpectedOutcomeV1::Failure { code } => {
            ProviderCallObservationV1::failure(*code, observed_evidence)
        }
    }
}

const fn count_value(count: south_provider_conformance::ProviderCallCountV1) -> usize {
    match count {
        south_provider_conformance::ProviderCallCountV1::Zero => 0,
        south_provider_conformance::ProviderCallCountV1::One => 1,
        south_provider_conformance::ProviderCallCountV1::MoreThanOne => 2,
    }
}

fn valid_response() -> BufferedHttpResponseV1 {
    BufferedHttpResponseV1::try_from_parts(StatusCode::OK, b"ok".to_vec(), None, None)
        .expect("fixture response should be valid")
}

const fn assert_send<T: Send>(_: &T) {}

fn _assert_future_shape<'a>(
    future: AssembledExecutionFutureV1<'a>,
) -> Pin<Box<dyn Future<Output = ProviderCallObservationV1> + Send + 'a>> {
    future
}
