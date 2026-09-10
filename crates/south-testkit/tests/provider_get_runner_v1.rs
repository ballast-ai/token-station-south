use std::{
    collections::BTreeSet,
    fmt::Display,
    future::pending,
    sync::{Arc, Mutex},
    time::Duration,
};

use http::StatusCode;
use south_contracts::BufferedHttpResponseV1;
use south_provider_conformance::{
    PROVIDER_GET_CONFORMANCE_SUITE_ID, PROVIDER_GET_CONFORMANCE_SUITE_VERSION, ProviderCallCountV1,
    ProviderCallFailureCodeV1, ProviderGetCaseIdV1, ProviderGetExpectedOutcomeV1,
    ProviderGetFixtureV1, provider_get_fixtures_v1,
};
use south_testkit::{
    AssembledProviderGetExecutionFutureV1, AssembledProviderGetExecutorV1,
    MAX_PROVIDER_GET_MISMATCHES_V1, ProviderGetConformanceFailureV1,
    ProviderGetConformanceReportV1, ProviderGetEvidenceV1, ProviderGetMismatchCategoryV1,
    ProviderGetObservationV1, run_provider_get_conformance_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ProviderGetObservationV1: Display);
assert_not_impl_any!(ProviderGetEvidenceV1: Display);
assert_not_impl_any!(ProviderGetConformanceReportV1: Display);
assert_not_impl_any!(ProviderGetConformanceFailureV1: Display);

#[derive(Default)]
struct MatchingExecutor {
    order: Mutex<Vec<ProviderGetCaseIdV1>>,
}

impl AssembledProviderGetExecutorV1 for MatchingExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderGetFixtureV1,
    ) -> AssembledProviderGetExecutionFutureV1<'a> {
        Box::pin(async move {
            self.order.lock().expect("order lock should be available").push(fixture.case_id());
            observation_matching(fixture)
        })
    }
}

#[tokio::test]
async fn object_safe_send_executor_runs_every_case_in_canonical_order() {
    let executor = MatchingExecutor::default();
    let dynamic: &dyn AssembledProviderGetExecutorV1 = &executor;
    let future = dynamic.execute_case(&provider_get_fixtures_v1()[0]);
    assert_send(&future);
    drop(future);

    let report =
        run_provider_get_conformance_v1(dynamic).await.expect("matching observations must pass");
    assert_eq!(report.suite_id(), PROVIDER_GET_CONFORMANCE_SUITE_ID);
    assert_eq!(report.suite_version(), PROVIDER_GET_CONFORMANCE_SUITE_VERSION);
    assert_eq!(
        report.passed_case_ids(),
        &provider_get_fixtures_v1().iter().map(ProviderGetFixtureV1::case_id).collect::<Vec<_>>()
    );
    assert_eq!(
        *executor.order.lock().expect("order lock should be available"),
        provider_get_fixtures_v1().iter().map(ProviderGetFixtureV1::case_id).collect::<Vec<_>>()
    );
}

struct SingleMismatchExecutor {
    case_id: ProviderGetCaseIdV1,
    category: ProviderGetMismatchCategoryV1,
}

impl AssembledProviderGetExecutorV1 for SingleMismatchExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderGetFixtureV1,
    ) -> AssembledProviderGetExecutionFutureV1<'a> {
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
        (ProviderGetCaseIdV1::GetSlotMismatch, ProviderGetMismatchCategoryV1::OutcomeKind),
        (ProviderGetCaseIdV1::GetSlotMismatch, ProviderGetMismatchCategoryV1::ErrorCode),
        (ProviderGetCaseIdV1::BufferedGetBearerSuccess, ProviderGetMismatchCategoryV1::Status),
        (ProviderGetCaseIdV1::BufferedGetBearerSuccess, ProviderGetMismatchCategoryV1::Body),
        (ProviderGetCaseIdV1::BufferedGetBearerSuccess, ProviderGetMismatchCategoryV1::ContentType),
        (ProviderGetCaseIdV1::BufferedGetBearerSuccess, ProviderGetMismatchCategoryV1::RetryAfter),
        (
            ProviderGetCaseIdV1::BufferedGetHeaderSecretSuccess,
            ProviderGetMismatchCategoryV1::ResolverCallCount,
        ),
        (
            ProviderGetCaseIdV1::BufferedGetHeaderSecretSuccess,
            ProviderGetMismatchCategoryV1::TransportCallCount,
        ),
        (ProviderGetCaseIdV1::BufferedGetBearerSuccess, ProviderGetMismatchCategoryV1::WireMethod),
        (
            ProviderGetCaseIdV1::BufferedGetHeaderSecretSuccess,
            ProviderGetMismatchCategoryV1::WireBody,
        ),
        (
            ProviderGetCaseIdV1::BufferedGetTaskIdQuerySuccess,
            ProviderGetMismatchCategoryV1::WireQuery,
        ),
        // The query-free rows must catch a probe that hardcodes `true`.
        (ProviderGetCaseIdV1::BufferedGetBearerSuccess, ProviderGetMismatchCategoryV1::WireQuery),
        // The refused row must catch a probe that hardcodes the method claim.
        (ProviderGetCaseIdV1::GetSlotMismatch, ProviderGetMismatchCategoryV1::WireMethod),
    ];

    for (case_id, category) in isolated_mismatches {
        let failure =
            run_provider_get_conformance_v1(&SingleMismatchExecutor { case_id, category })
                .await
                .expect_err("one deliberate difference must fail conformance");
        assert_eq!(failure.suite_id(), PROVIDER_GET_CONFORMANCE_SUITE_ID);
        assert_eq!(failure.suite_version(), PROVIDER_GET_CONFORMANCE_SUITE_VERSION);
        assert_eq!(failure.evaluated_case_count(), 4);
        assert!(failure.mismatches().len() <= MAX_PROVIDER_GET_MISMATCHES_V1);
        assert_eq!(failure.mismatches().len(), 1, "category {category:?} must isolate");
        let mismatch = &failure.mismatches()[0];
        assert_eq!(mismatch.case_id(), case_id);
        assert_eq!(mismatch.category(), category);
    }
}

#[tokio::test]
async fn a_fully_wrong_executor_reports_every_case_without_failing_fast() {
    struct WrongExecutor;

    impl AssembledProviderGetExecutorV1 for WrongExecutor {
        fn execute_case<'a>(
            &'a self,
            fixture: &'a ProviderGetFixtureV1,
        ) -> AssembledProviderGetExecutionFutureV1<'a> {
            Box::pin(async move {
                // A POST-shaped executor: every claim inverted, counts saturated.
                let evidence = ProviderGetEvidenceV1::new(257, 256, false, false, true);
                match fixture.case_id() {
                    ProviderGetCaseIdV1::GetSlotMismatch => ProviderGetObservationV1::response(
                        response(200, "{}", None, None),
                        evidence,
                    ),
                    _ => ProviderGetObservationV1::failure(
                        ProviderCallFailureCodeV1::RequestFailed,
                        evidence,
                    ),
                }
            })
        }
    }

    let failure = run_provider_get_conformance_v1(&WrongExecutor)
        .await
        .expect_err("deliberate mismatches must fail");
    assert_eq!(failure.evaluated_case_count(), 4);

    let categories: BTreeSet<_> =
        failure.mismatches().iter().map(south_testkit::ProviderGetMismatchV1::category).collect();
    assert!(categories.contains(&ProviderGetMismatchCategoryV1::OutcomeKind));
    assert!(categories.contains(&ProviderGetMismatchCategoryV1::ResolverCallCount));
    assert!(categories.contains(&ProviderGetMismatchCategoryV1::TransportCallCount));
    assert!(categories.contains(&ProviderGetMismatchCategoryV1::WireMethod));
    assert!(categories.contains(&ProviderGetMismatchCategoryV1::WireBody));
    assert!(categories.contains(&ProviderGetMismatchCategoryV1::WireQuery));
    for fixture in provider_get_fixtures_v1() {
        assert!(failure.mismatches().iter().any(|m| m.case_id() == fixture.case_id()));
    }
}

#[test]
fn evidence_construction_saturates_large_boundary_counts() {
    let evidence = ProviderGetEvidenceV1::new(256, 257, true, false, true);

    assert_eq!(evidence.resolver_calls(), ProviderCallCountV1::MoreThanOne);
    assert_eq!(evidence.transport_calls(), ProviderCallCountV1::MoreThanOne);
    assert!(evidence.wire_method_get());
    assert!(!evidence.wire_body_absent());
    assert!(evidence.wire_query_exact());
}

#[test]
fn debug_output_contains_only_safe_structural_evidence() {
    const BODY_SENTINEL: &str = "provider-get-runner-body-debug-sentinel";
    const METADATA_SENTINEL: &str = "provider-get-runner-metadata-debug-sentinel";

    let evidence = ProviderGetEvidenceV1::new(1, 1, true, true, false);
    let observation = ProviderGetObservationV1::response(
        BufferedHttpResponseV1::try_from_parts(
            StatusCode::OK,
            BODY_SENTINEL.as_bytes().to_vec(),
            Some(METADATA_SENTINEL.to_owned()),
            Some(METADATA_SENTINEL.to_owned()),
        )
        .expect("fixture response should be valid"),
        evidence,
    );

    let debug = format!("{observation:?}");
    for sentinel in [BODY_SENTINEL, METADATA_SENTINEL] {
        assert!(!debug.contains(sentinel), "debug output leaked sentinel: {debug}");
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

impl AssembledProviderGetExecutorV1 for PendingExecutor {
    fn execute_case<'a>(
        &'a self,
        _fixture: &'a ProviderGetFixtureV1,
    ) -> AssembledProviderGetExecutionFutureV1<'a> {
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
    let structured_run = async { run_provider_get_conformance_v1(&executor).await };

    let result = tokio::time::timeout(Duration::from_secs(5), structured_run).await;

    assert!(result.is_err());
    assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
}

const fn matching_evidence(fixture: &ProviderGetFixtureV1) -> ProviderGetEvidenceV1 {
    let evidence = fixture.expected().evidence();
    ProviderGetEvidenceV1::new(
        count_value(evidence.resolver_calls()),
        count_value(evidence.transport_calls()),
        evidence.wire_method_get(),
        evidence.wire_body_absent(),
        evidence.wire_query_exact(),
    )
}

fn observation_matching(fixture: &ProviderGetFixtureV1) -> ProviderGetObservationV1 {
    let evidence = matching_evidence(fixture);
    match fixture.expected().outcome() {
        ProviderGetExpectedOutcomeV1::Response { status, body, content_type, retry_after } => {
            ProviderGetObservationV1::response(
                response(*status, body, *content_type, *retry_after),
                evidence,
            )
        }
        ProviderGetExpectedOutcomeV1::Failure { code } => {
            ProviderGetObservationV1::failure(*code, evidence)
        }
    }
}

fn response(
    status: u16,
    body: &str,
    content_type: Option<&str>,
    retry_after: Option<&str>,
) -> BufferedHttpResponseV1 {
    BufferedHttpResponseV1::try_from_parts(
        StatusCode::from_u16(status).expect("expected status should be valid"),
        body.as_bytes().to_vec(),
        content_type.map(str::to_owned),
        retry_after.map(str::to_owned),
    )
    .expect("expected response should be valid")
}

fn observation_with_single_mismatch(
    fixture: &ProviderGetFixtureV1,
    category: ProviderGetMismatchCategoryV1,
) -> ProviderGetObservationV1 {
    let expected_evidence = fixture.expected().evidence();
    let mut resolver_calls = count_value(expected_evidence.resolver_calls());
    let mut transport_calls = count_value(expected_evidence.transport_calls());
    let mut method_get = expected_evidence.wire_method_get();
    let mut body_absent = expected_evidence.wire_body_absent();
    let mut query_exact = expected_evidence.wire_query_exact();
    match category {
        ProviderGetMismatchCategoryV1::ResolverCallCount => {
            resolver_calls = different_count(resolver_calls);
        }
        ProviderGetMismatchCategoryV1::TransportCallCount => {
            transport_calls = different_count(transport_calls);
        }
        ProviderGetMismatchCategoryV1::WireMethod => method_get = !method_get,
        ProviderGetMismatchCategoryV1::WireBody => body_absent = !body_absent,
        ProviderGetMismatchCategoryV1::WireQuery => query_exact = !query_exact,
        _ => {}
    }
    let evidence = ProviderGetEvidenceV1::new(
        resolver_calls,
        transport_calls,
        method_get,
        body_absent,
        query_exact,
    );

    match fixture.expected().outcome() {
        ProviderGetExpectedOutcomeV1::Response { status, body, content_type, retry_after } => {
            let observed_status = observed_status(*status, category);
            let observed_body = if category == ProviderGetMismatchCategoryV1::Body {
                "isolated-wrong-body"
            } else {
                body
            };
            let observed_content_type = if category == ProviderGetMismatchCategoryV1::ContentType {
                None
            } else {
                *content_type
            };
            let observed_retry_after = if category == ProviderGetMismatchCategoryV1::RetryAfter {
                None
            } else {
                *retry_after
            };
            ProviderGetObservationV1::response(
                response(
                    observed_status,
                    observed_body,
                    observed_content_type,
                    observed_retry_after,
                ),
                evidence,
            )
        }
        ProviderGetExpectedOutcomeV1::Failure { code } => {
            if category == ProviderGetMismatchCategoryV1::OutcomeKind {
                ProviderGetObservationV1::response(response(200, "{}", None, None), evidence)
            } else {
                let observed_code = if category == ProviderGetMismatchCategoryV1::ErrorCode {
                    ProviderCallFailureCodeV1::RequestFailed
                } else {
                    *code
                };
                ProviderGetObservationV1::failure(observed_code, evidence)
            }
        }
    }
}

const fn observed_status(expected: u16, category: ProviderGetMismatchCategoryV1) -> u16 {
    if matches!(category, ProviderGetMismatchCategoryV1::Status) { expected + 1 } else { expected }
}

const fn count_value(count: ProviderCallCountV1) -> usize {
    match count {
        ProviderCallCountV1::Zero => 0,
        ProviderCallCountV1::One => 1,
        ProviderCallCountV1::MoreThanOne => 2,
    }
}

const fn different_count(count: usize) -> usize {
    if count == 0 { 1 } else { 0 }
}

const fn assert_send<T: Send>(_: &T) {}
