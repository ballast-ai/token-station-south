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
    PROVIDER_MULTIPART_CONFORMANCE_SUITE_ID, PROVIDER_MULTIPART_CONFORMANCE_SUITE_VERSION,
    ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderMultipartCaseIdV1,
    ProviderMultipartExpectedOutcomeV1, ProviderMultipartFixtureV1, provider_multipart_fixtures_v1,
};
use south_testkit::{
    AssembledProviderMultipartExecutionFutureV1, AssembledProviderMultipartExecutorV1,
    MAX_PROVIDER_MULTIPART_MISMATCHES_V1, ProviderMultipartConformanceFailureV1,
    ProviderMultipartConformanceReportV1, ProviderMultipartEvidenceV1,
    ProviderMultipartMismatchCategoryV1, ProviderMultipartObservationV1,
    run_provider_multipart_conformance_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ProviderMultipartObservationV1: Display);
assert_not_impl_any!(ProviderMultipartEvidenceV1: Display);
assert_not_impl_any!(ProviderMultipartConformanceReportV1: Display);
assert_not_impl_any!(ProviderMultipartConformanceFailureV1: Display);

#[derive(Default)]
struct MatchingExecutor {
    order: Mutex<Vec<ProviderMultipartCaseIdV1>>,
}

impl AssembledProviderMultipartExecutorV1 for MatchingExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderMultipartFixtureV1,
    ) -> AssembledProviderMultipartExecutionFutureV1<'a> {
        Box::pin(async move {
            self.order.lock().expect("order lock should be available").push(fixture.case_id());
            observation_matching(fixture)
        })
    }
}

#[tokio::test]
async fn object_safe_send_executor_runs_every_case_in_canonical_order() {
    let executor = MatchingExecutor::default();
    let dynamic: &dyn AssembledProviderMultipartExecutorV1 = &executor;
    let future = dynamic.execute_case(&provider_multipart_fixtures_v1()[0]);
    assert_send(&future);
    drop(future);

    let report = run_provider_multipart_conformance_v1(dynamic)
        .await
        .expect("matching observations must pass");
    assert_eq!(report.suite_id(), PROVIDER_MULTIPART_CONFORMANCE_SUITE_ID);
    assert_eq!(report.suite_version(), PROVIDER_MULTIPART_CONFORMANCE_SUITE_VERSION);
    let canonical: Vec<_> =
        provider_multipart_fixtures_v1().iter().map(ProviderMultipartFixtureV1::case_id).collect();
    assert_eq!(report.passed_case_ids(), &canonical);
    assert_eq!(*executor.order.lock().expect("order lock should be available"), canonical);
}

struct SingleMismatchExecutor {
    case_id: ProviderMultipartCaseIdV1,
    category: ProviderMultipartMismatchCategoryV1,
}

impl AssembledProviderMultipartExecutorV1 for SingleMismatchExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderMultipartFixtureV1,
    ) -> AssembledProviderMultipartExecutionFutureV1<'a> {
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
    use ProviderMultipartCaseIdV1 as Case;
    use ProviderMultipartMismatchCategoryV1 as Cat;
    let isolated_mismatches = [
        (Case::MultipartSlotMismatch, Cat::OutcomeKind),
        (Case::MultipartSlotMismatch, Cat::ErrorCode),
        (Case::BufferedMultipartBearerSuccess, Cat::Status),
        (Case::BufferedMultipartBearerSuccess, Cat::Body),
        (Case::BufferedMultipartBearerSuccess, Cat::ContentType),
        (Case::BufferedMultipartBearerSuccess, Cat::RetryAfter),
        (Case::BufferedMultipartHeaderSecretSuccess, Cat::ResolverCallCount),
        (Case::BufferedMultipartHeaderSecretSuccess, Cat::TransportCallCount),
        (Case::BufferedMultipartBearerSuccess, Cat::WireContentType),
        (Case::BufferedMultipartBearerSuccess, Cat::WireBodyBytes),
        // The refusal rows expect `false` for both wire claims, so a probe that hardcodes `true`
        // is caught there and not only on the success rows.
        (Case::MultipartBodyBoundaryMismatch, Cat::WireContentType),
        (Case::MultipartContentTypeSmuggled, Cat::WireBodyBytes),
    ];

    for (case_id, category) in isolated_mismatches {
        let failure =
            run_provider_multipart_conformance_v1(&SingleMismatchExecutor { case_id, category })
                .await
                .expect_err("one deliberate difference must fail conformance");
        assert_eq!(failure.suite_id(), PROVIDER_MULTIPART_CONFORMANCE_SUITE_ID);
        assert_eq!(failure.suite_version(), PROVIDER_MULTIPART_CONFORMANCE_SUITE_VERSION);
        assert_eq!(failure.evaluated_case_count(), 5);
        assert!(failure.mismatches().len() <= MAX_PROVIDER_MULTIPART_MISMATCHES_V1);
        assert_eq!(failure.mismatches().len(), 1, "category {category:?} must isolate");
        let mismatch = &failure.mismatches()[0];
        assert_eq!(mismatch.case_id(), case_id);
        assert_eq!(mismatch.category(), category);
    }
}

#[tokio::test]
async fn a_fully_wrong_executor_reports_every_case_without_failing_fast() {
    struct WrongExecutor;

    impl AssembledProviderMultipartExecutorV1 for WrongExecutor {
        fn execute_case<'a>(
            &'a self,
            fixture: &'a ProviderMultipartFixtureV1,
        ) -> AssembledProviderMultipartExecutionFutureV1<'a> {
            Box::pin(async move {
                // Every claim inverted, counts saturated.
                let evidence = ProviderMultipartEvidenceV1::new(257, 256, false, false);
                match fixture.case_id() {
                    ProviderMultipartCaseIdV1::MultipartSlotMismatch => {
                        ProviderMultipartObservationV1::response(
                            response(200, "{}", None, None),
                            evidence,
                        )
                    }
                    _ => ProviderMultipartObservationV1::failure(
                        ProviderCallFailureCodeV1::RequestFailed,
                        evidence,
                    ),
                }
            })
        }
    }

    let failure = run_provider_multipart_conformance_v1(&WrongExecutor)
        .await
        .expect_err("deliberate mismatches must fail");
    assert_eq!(failure.evaluated_case_count(), 5);

    let categories: BTreeSet<_> = failure
        .mismatches()
        .iter()
        .map(south_testkit::ProviderMultipartMismatchV1::category)
        .collect();
    assert!(categories.contains(&ProviderMultipartMismatchCategoryV1::OutcomeKind));
    assert!(categories.contains(&ProviderMultipartMismatchCategoryV1::ResolverCallCount));
    assert!(categories.contains(&ProviderMultipartMismatchCategoryV1::TransportCallCount));
    assert!(categories.contains(&ProviderMultipartMismatchCategoryV1::WireContentType));
    assert!(categories.contains(&ProviderMultipartMismatchCategoryV1::WireBodyBytes));
    for fixture in provider_multipart_fixtures_v1() {
        assert!(failure.mismatches().iter().any(|m| m.case_id() == fixture.case_id()));
    }
}

#[test]
fn evidence_construction_saturates_large_boundary_counts() {
    let evidence = ProviderMultipartEvidenceV1::new(256, 257, true, false);

    assert_eq!(evidence.resolver_calls(), ProviderCallCountV1::MoreThanOne);
    assert_eq!(evidence.transport_calls(), ProviderCallCountV1::MoreThanOne);
    assert!(evidence.wire_content_type_exact());
    assert!(!evidence.wire_body_bytes_exact());
}

#[test]
fn debug_output_contains_only_safe_structural_evidence() {
    const BODY_SENTINEL: &str = "multipart-runner-body-debug-sentinel";
    const METADATA_SENTINEL: &str = "multipart-runner-metadata-debug-sentinel";

    let observation = ProviderMultipartObservationV1::response(
        BufferedHttpResponseV1::try_from_parts(
            StatusCode::OK,
            BODY_SENTINEL.as_bytes().to_vec(),
            Some(METADATA_SENTINEL.to_owned()),
            Some(METADATA_SENTINEL.to_owned()),
        )
        .expect("fixture response should be valid"),
        ProviderMultipartEvidenceV1::new(1, 1, true, true),
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

impl AssembledProviderMultipartExecutorV1 for PendingExecutor {
    fn execute_case<'a>(
        &'a self,
        _fixture: &'a ProviderMultipartFixtureV1,
    ) -> AssembledProviderMultipartExecutionFutureV1<'a> {
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
    let structured_run = async { run_provider_multipart_conformance_v1(&executor).await };

    let result = tokio::time::timeout(Duration::from_secs(5), structured_run).await;

    assert!(result.is_err());
    assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
}

const fn matching_evidence(fixture: &ProviderMultipartFixtureV1) -> ProviderMultipartEvidenceV1 {
    let evidence = fixture.expected().evidence();
    ProviderMultipartEvidenceV1::new(
        count_value(evidence.resolver_calls()),
        count_value(evidence.transport_calls()),
        evidence.wire_content_type_exact(),
        evidence.wire_body_bytes_exact(),
    )
}

fn observation_matching(fixture: &ProviderMultipartFixtureV1) -> ProviderMultipartObservationV1 {
    let evidence = matching_evidence(fixture);
    match fixture.expected().outcome() {
        ProviderMultipartExpectedOutcomeV1::Response {
            status,
            body,
            content_type,
            retry_after,
        } => ProviderMultipartObservationV1::response(
            response(*status, body, *content_type, *retry_after),
            evidence,
        ),
        ProviderMultipartExpectedOutcomeV1::Failure { code } => {
            ProviderMultipartObservationV1::failure(*code, evidence)
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
    fixture: &ProviderMultipartFixtureV1,
    category: ProviderMultipartMismatchCategoryV1,
) -> ProviderMultipartObservationV1 {
    let expected_evidence = fixture.expected().evidence();
    let mut resolver_calls = count_value(expected_evidence.resolver_calls());
    let mut transport_calls = count_value(expected_evidence.transport_calls());
    let mut content_type_exact = expected_evidence.wire_content_type_exact();
    let mut body_bytes_exact = expected_evidence.wire_body_bytes_exact();
    match category {
        ProviderMultipartMismatchCategoryV1::ResolverCallCount => {
            resolver_calls = different_count(resolver_calls);
        }
        ProviderMultipartMismatchCategoryV1::TransportCallCount => {
            transport_calls = different_count(transport_calls);
        }
        ProviderMultipartMismatchCategoryV1::WireContentType => {
            content_type_exact = !content_type_exact;
        }
        ProviderMultipartMismatchCategoryV1::WireBodyBytes => {
            body_bytes_exact = !body_bytes_exact;
        }
        _ => {}
    }
    let evidence = ProviderMultipartEvidenceV1::new(
        resolver_calls,
        transport_calls,
        content_type_exact,
        body_bytes_exact,
    );

    match fixture.expected().outcome() {
        ProviderMultipartExpectedOutcomeV1::Response {
            status,
            body,
            content_type,
            retry_after,
        } => {
            let observed_status = observed_status(*status, category);
            let observed_body = if category == ProviderMultipartMismatchCategoryV1::Body {
                "isolated-wrong-body"
            } else {
                body
            };
            let observed_content_type =
                if category == ProviderMultipartMismatchCategoryV1::ContentType {
                    None
                } else {
                    *content_type
                };
            let observed_retry_after =
                if category == ProviderMultipartMismatchCategoryV1::RetryAfter {
                    Some("isolated-retry-after")
                } else {
                    *retry_after
                };
            ProviderMultipartObservationV1::response(
                response(
                    observed_status,
                    observed_body,
                    observed_content_type,
                    observed_retry_after,
                ),
                evidence,
            )
        }
        ProviderMultipartExpectedOutcomeV1::Failure { code } => {
            if category == ProviderMultipartMismatchCategoryV1::OutcomeKind {
                ProviderMultipartObservationV1::response(response(200, "{}", None, None), evidence)
            } else {
                let observed_code = if category == ProviderMultipartMismatchCategoryV1::ErrorCode {
                    ProviderCallFailureCodeV1::RequestFailed
                } else {
                    *code
                };
                ProviderMultipartObservationV1::failure(observed_code, evidence)
            }
        }
    }
}

const fn observed_status(expected: u16, category: ProviderMultipartMismatchCategoryV1) -> u16 {
    if matches!(category, ProviderMultipartMismatchCategoryV1::Status) {
        expected + 1
    } else {
        expected
    }
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
