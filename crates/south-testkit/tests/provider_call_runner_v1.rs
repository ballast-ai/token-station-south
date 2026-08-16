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

struct SingleMismatchExecutor {
    case_id: ProviderCallCaseIdV1,
    category: ProviderCallMismatchCategoryV1,
}

impl AssembledProviderCallExecutorV1 for SingleMismatchExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderCallFixtureV1,
    ) -> AssembledExecutionFutureV1<'a> {
        Box::pin(async move {
            if fixture.case_id() == self.case_id {
                observation_with_single_mismatch(fixture, self.category)
            } else {
                observation_matching(fixture)
            }
        })
    }
}

#[tokio::test]
async fn each_single_difference_reports_exactly_its_one_case_and_category() {
    let isolated_mismatches = [
        (ProviderCallCaseIdV1::Success, ProviderCallMismatchCategoryV1::Status),
        (ProviderCallCaseIdV1::Success, ProviderCallMismatchCategoryV1::Body),
        (ProviderCallCaseIdV1::Success, ProviderCallMismatchCategoryV1::ContentType),
        (ProviderCallCaseIdV1::Success, ProviderCallMismatchCategoryV1::RetryAfter),
        (ProviderCallCaseIdV1::InvalidRelativePath, ProviderCallMismatchCategoryV1::ErrorCode),
        (ProviderCallCaseIdV1::CredentialSlotMismatch, ProviderCallMismatchCategoryV1::OutcomeKind),
        (ProviderCallCaseIdV1::RedirectDenied, ProviderCallMismatchCategoryV1::ResolverCallCount),
        (
            ProviderCallCaseIdV1::ResponseBodyTooLarge,
            ProviderCallMismatchCategoryV1::TransportCallCount,
        ),
        (ProviderCallCaseIdV1::Cancelled, ProviderCallMismatchCategoryV1::ResolverPendingDrop),
        (
            ProviderCallCaseIdV1::DeadlineExceeded,
            ProviderCallMismatchCategoryV1::TransportPendingDrop,
        ),
    ];

    for (case_id, category) in isolated_mismatches {
        let failure =
            run_provider_call_conformance_v1(&SingleMismatchExecutor { case_id, category })
                .await
                .expect_err("one deliberate difference must fail conformance");
        assert_eq!(failure.mismatches().len(), 1);
        let mismatch = &failure.mismatches()[0];
        assert_eq!(mismatch.case_id(), case_id);
        assert_eq!(mismatch.category(), category);
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

fn observation_with_single_mismatch(
    fixture: &ProviderCallFixtureV1,
    category: ProviderCallMismatchCategoryV1,
) -> ProviderCallObservationV1 {
    let expected = fixture.expected();
    let expected_evidence = expected.evidence();
    let mut resolver_calls = count_value(expected_evidence.resolver_calls());
    let mut transport_calls = count_value(expected_evidence.transport_calls());
    let mut resolver_drop = expected_evidence.resolver_future_dropped_while_pending();
    let mut transport_drop = expected_evidence.transport_future_dropped_while_pending();
    match category {
        ProviderCallMismatchCategoryV1::ResolverCallCount => {
            resolver_calls = different_count(resolver_calls);
        }
        ProviderCallMismatchCategoryV1::TransportCallCount => {
            transport_calls = different_count(transport_calls);
        }
        ProviderCallMismatchCategoryV1::ResolverPendingDrop => resolver_drop = !resolver_drop,
        ProviderCallMismatchCategoryV1::TransportPendingDrop => transport_drop = !transport_drop,
        _ => {}
    }
    let evidence =
        ProviderCallEvidenceV1::new(resolver_calls, transport_calls, resolver_drop, transport_drop);

    match expected.outcome() {
        ProviderCallExpectedOutcomeV1::Response { status, body, content_type, retry_after } => {
            let observed_status = if category == ProviderCallMismatchCategoryV1::Status {
                StatusCode::OK
            } else {
                StatusCode::from_u16(*status).expect("expected status should be valid")
            };
            let observed_body = if category == ProviderCallMismatchCategoryV1::Body {
                "isolated-wrong-body"
            } else {
                body
            };
            let observed_content_type = if category == ProviderCallMismatchCategoryV1::ContentType {
                None
            } else {
                *content_type
            };
            let observed_retry_after = if category == ProviderCallMismatchCategoryV1::RetryAfter {
                None
            } else {
                *retry_after
            };
            ProviderCallObservationV1::response(
                BufferedHttpResponseV1::try_from_parts(
                    observed_status,
                    observed_body.as_bytes().to_vec(),
                    observed_content_type.map(str::to_owned),
                    observed_retry_after.map(str::to_owned),
                )
                .expect("isolated response must satisfy contract bounds"),
                evidence,
            )
        }
        ProviderCallExpectedOutcomeV1::Failure { code } => {
            if category == ProviderCallMismatchCategoryV1::OutcomeKind {
                ProviderCallObservationV1::response(valid_response(), evidence)
            } else {
                let observed_code = if category == ProviderCallMismatchCategoryV1::ErrorCode {
                    ProviderCallFailureCodeV1::RequestFailed
                } else {
                    *code
                };
                ProviderCallObservationV1::failure(observed_code, evidence)
            }
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

const fn different_count(count: usize) -> usize {
    if count == 0 { 1 } else { 0 }
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
