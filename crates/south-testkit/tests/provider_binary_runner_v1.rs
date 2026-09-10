use std::{
    collections::BTreeSet,
    fmt::Display,
    future::pending,
    sync::{Arc, Mutex},
    time::Duration,
};

use http::StatusCode;
use south_contracts::BufferedBinaryResponseV1;
use south_provider_conformance::{
    PROVIDER_BINARY_CONFORMANCE_SUITE_ID, PROVIDER_BINARY_CONFORMANCE_SUITE_VERSION,
    ProviderBinaryCaseIdV1, ProviderBinaryExpectedOutcomeV1, ProviderBinaryFixtureV1,
    ProviderCallCountV1, ProviderCallFailureCodeV1, provider_binary_fixtures_v1,
};
use south_testkit::{
    AssembledProviderBinaryExecutionFutureV1, AssembledProviderBinaryExecutorV1,
    MAX_PROVIDER_BINARY_MISMATCHES_V1, ProviderBinaryConformanceFailureV1,
    ProviderBinaryConformanceReportV1, ProviderBinaryEvidenceV1, ProviderBinaryMismatchCategoryV1,
    ProviderBinaryObservationV1, run_provider_binary_conformance_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ProviderBinaryObservationV1: Display);
assert_not_impl_any!(ProviderBinaryEvidenceV1: Display);
assert_not_impl_any!(ProviderBinaryConformanceReportV1: Display);
assert_not_impl_any!(ProviderBinaryConformanceFailureV1: Display);

#[derive(Default)]
struct MatchingExecutor {
    order: Mutex<Vec<ProviderBinaryCaseIdV1>>,
}

impl AssembledProviderBinaryExecutorV1 for MatchingExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderBinaryFixtureV1,
    ) -> AssembledProviderBinaryExecutionFutureV1<'a> {
        Box::pin(async move {
            self.order.lock().expect("order lock should be available").push(fixture.case_id());
            observation_matching(fixture)
        })
    }
}

#[tokio::test]
async fn object_safe_send_executor_runs_every_case_in_canonical_order() {
    let executor = MatchingExecutor::default();
    let dynamic: &dyn AssembledProviderBinaryExecutorV1 = &executor;
    let future = dynamic.execute_case(&provider_binary_fixtures_v1()[0]);
    assert_send(&future);
    drop(future);

    let report =
        run_provider_binary_conformance_v1(dynamic).await.expect("matching observations must pass");
    assert_eq!(report.suite_id(), PROVIDER_BINARY_CONFORMANCE_SUITE_ID);
    assert_eq!(report.suite_version(), PROVIDER_BINARY_CONFORMANCE_SUITE_VERSION);
    let canonical: Vec<_> =
        provider_binary_fixtures_v1().iter().map(ProviderBinaryFixtureV1::case_id).collect();
    assert_eq!(report.passed_case_ids(), &canonical);
    assert_eq!(*executor.order.lock().expect("order lock should be available"), canonical);
}

struct SingleMismatchExecutor {
    case_id: ProviderBinaryCaseIdV1,
    category: ProviderBinaryMismatchCategoryV1,
}

impl AssembledProviderBinaryExecutorV1 for SingleMismatchExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderBinaryFixtureV1,
    ) -> AssembledProviderBinaryExecutionFutureV1<'a> {
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
    use ProviderBinaryCaseIdV1 as Case;
    use ProviderBinaryMismatchCategoryV1 as Cat;
    let isolated_mismatches = [
        (Case::BinarySlotMismatch, Cat::OutcomeKind),
        (Case::BinarySlotMismatch, Cat::ErrorCode),
        (Case::BinarySuccessNonUtf8Body, Cat::Status),
        (Case::BinarySuccessNonUtf8Body, Cat::Body),
        (Case::BinarySuccessNonUtf8Body, Cat::ContentType),
        (Case::BinaryRejectionCarriesJsonBody, Cat::RetryAfter),
        (Case::BinaryRejectionCarriesJsonBody, Cat::ResolverCallCount),
        (Case::BinarySuccessNonUtf8Body, Cat::TransportCallCount),
        (Case::BinarySuccessNonUtf8Body, Cat::WireBinaryResponse),
        (Case::BinarySuccessNonUtf8Body, Cat::WireBodyBytes),
        // The rows that expect `false` catch the inverse error — a probe reporting a seam or a
        // byte comparison it never performed. Without these three, an adapter hardcoding `true`
        // would pass everything above.
        (Case::BinarySlotMismatch, Cat::WireBinaryResponse),
        (Case::TextArmStillRefusesNonUtf8, Cat::WireBinaryResponse),
        (Case::BinaryBodyAboveBinaryCapRefused, Cat::WireBodyBytes),
    ];

    for (case_id, category) in isolated_mismatches {
        let failure =
            run_provider_binary_conformance_v1(&SingleMismatchExecutor { case_id, category })
                .await
                .expect_err("one deliberate difference must fail conformance");
        assert_eq!(failure.suite_id(), PROVIDER_BINARY_CONFORMANCE_SUITE_ID);
        assert_eq!(failure.suite_version(), PROVIDER_BINARY_CONFORMANCE_SUITE_VERSION);
        assert_eq!(failure.evaluated_case_count(), 6);
        assert!(failure.mismatches().len() <= MAX_PROVIDER_BINARY_MISMATCHES_V1);
        assert_eq!(failure.mismatches().len(), 1, "category {category:?} must isolate");
        let mismatch = &failure.mismatches()[0];
        assert_eq!(mismatch.case_id(), case_id);
        assert_eq!(mismatch.category(), category);
    }
}

#[tokio::test]
async fn a_fully_wrong_executor_reports_every_case_without_failing_fast() {
    struct WrongExecutor;

    impl AssembledProviderBinaryExecutorV1 for WrongExecutor {
        fn execute_case<'a>(
            &'a self,
            fixture: &'a ProviderBinaryFixtureV1,
        ) -> AssembledProviderBinaryExecutionFutureV1<'a> {
            Box::pin(async move {
                // Every claim inverted, counts saturated.
                let evidence = ProviderBinaryEvidenceV1::new(257, 256, false, false);
                match fixture.case_id() {
                    ProviderBinaryCaseIdV1::BinarySlotMismatch => {
                        ProviderBinaryObservationV1::response(
                            response(200, b"{}", None, None),
                            evidence,
                        )
                    }
                    _ => ProviderBinaryObservationV1::failure(
                        ProviderCallFailureCodeV1::RequestFailed,
                        evidence,
                    ),
                }
            })
        }
    }

    let failure = run_provider_binary_conformance_v1(&WrongExecutor)
        .await
        .expect_err("deliberate mismatches must fail");
    assert_eq!(failure.evaluated_case_count(), 6);

    let categories: BTreeSet<_> = failure
        .mismatches()
        .iter()
        .map(south_testkit::ProviderBinaryMismatchV1::category)
        .collect();
    assert!(categories.contains(&ProviderBinaryMismatchCategoryV1::OutcomeKind));
    assert!(categories.contains(&ProviderBinaryMismatchCategoryV1::ResolverCallCount));
    assert!(categories.contains(&ProviderBinaryMismatchCategoryV1::TransportCallCount));
    assert!(categories.contains(&ProviderBinaryMismatchCategoryV1::WireBinaryResponse));
    assert!(categories.contains(&ProviderBinaryMismatchCategoryV1::WireBodyBytes));
    for fixture in provider_binary_fixtures_v1() {
        assert!(failure.mismatches().iter().any(|m| m.case_id() == fixture.case_id()));
    }
}

#[test]
fn evidence_construction_saturates_large_boundary_counts() {
    let evidence = ProviderBinaryEvidenceV1::new(256, 257, true, false);

    assert_eq!(evidence.resolver_calls(), ProviderCallCountV1::MoreThanOne);
    assert_eq!(evidence.transport_calls(), ProviderCallCountV1::MoreThanOne);
    assert!(evidence.wire_binary_response_observed());
    assert!(!evidence.wire_body_bytes_exact());
}

#[test]
fn debug_output_contains_only_safe_structural_evidence() {
    const BODY_SENTINEL: &str = "binary-runner-body-debug-sentinel";
    const METADATA_SENTINEL: &str = "binary-runner-metadata-debug-sentinel";

    let observation = ProviderBinaryObservationV1::response(
        BufferedBinaryResponseV1::try_from_parts(
            StatusCode::OK,
            BODY_SENTINEL.as_bytes().to_vec(),
            Some(METADATA_SENTINEL.to_owned()),
            Some(METADATA_SENTINEL.to_owned()),
        )
        .expect("fixture response should be valid"),
        ProviderBinaryEvidenceV1::new(1, 1, true, true),
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

impl AssembledProviderBinaryExecutorV1 for PendingExecutor {
    fn execute_case<'a>(
        &'a self,
        _fixture: &'a ProviderBinaryFixtureV1,
    ) -> AssembledProviderBinaryExecutionFutureV1<'a> {
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
    let structured_run = async { run_provider_binary_conformance_v1(&executor).await };

    let result = tokio::time::timeout(Duration::from_secs(5), structured_run).await;

    assert!(result.is_err());
    assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
}

const fn matching_evidence(fixture: &ProviderBinaryFixtureV1) -> ProviderBinaryEvidenceV1 {
    let evidence = fixture.expected().evidence();
    ProviderBinaryEvidenceV1::new(
        count_value(evidence.resolver_calls()),
        count_value(evidence.transport_calls()),
        evidence.wire_binary_response_observed(),
        evidence.wire_body_bytes_exact(),
    )
}

fn observation_matching(fixture: &ProviderBinaryFixtureV1) -> ProviderBinaryObservationV1 {
    let evidence = matching_evidence(fixture);
    match fixture.expected().outcome() {
        ProviderBinaryExpectedOutcomeV1::Response { status, body, content_type, retry_after } => {
            ProviderBinaryObservationV1::response(
                response(*status, &body.materialize(), *content_type, *retry_after),
                evidence,
            )
        }
        ProviderBinaryExpectedOutcomeV1::Failure { code } => {
            ProviderBinaryObservationV1::failure(*code, evidence)
        }
    }
}

fn response(
    status: u16,
    body: &[u8],
    content_type: Option<&str>,
    retry_after: Option<&str>,
) -> BufferedBinaryResponseV1 {
    BufferedBinaryResponseV1::try_from_parts(
        StatusCode::from_u16(status).expect("expected status should be valid"),
        body.to_vec(),
        content_type.map(str::to_owned),
        retry_after.map(str::to_owned),
    )
    .expect("expected response should be valid")
}

fn observation_with_single_mismatch(
    fixture: &ProviderBinaryFixtureV1,
    category: ProviderBinaryMismatchCategoryV1,
) -> ProviderBinaryObservationV1 {
    let expected_evidence = fixture.expected().evidence();
    let mut resolver_calls = count_value(expected_evidence.resolver_calls());
    let mut transport_calls = count_value(expected_evidence.transport_calls());
    let mut binary_response_observed = expected_evidence.wire_binary_response_observed();
    let mut body_bytes_exact = expected_evidence.wire_body_bytes_exact();
    match category {
        ProviderBinaryMismatchCategoryV1::ResolverCallCount => {
            resolver_calls = different_count(resolver_calls);
        }
        ProviderBinaryMismatchCategoryV1::TransportCallCount => {
            transport_calls = different_count(transport_calls);
        }
        ProviderBinaryMismatchCategoryV1::WireBinaryResponse => {
            binary_response_observed = !binary_response_observed;
        }
        ProviderBinaryMismatchCategoryV1::WireBodyBytes => {
            body_bytes_exact = !body_bytes_exact;
        }
        _ => {}
    }
    let evidence = ProviderBinaryEvidenceV1::new(
        resolver_calls,
        transport_calls,
        binary_response_observed,
        body_bytes_exact,
    );

    match fixture.expected().outcome() {
        ProviderBinaryExpectedOutcomeV1::Response { status, body, content_type, retry_after } => {
            let observed_status = observed_status(*status, category);
            let observed_body = if category == ProviderBinaryMismatchCategoryV1::Body {
                b"isolated-wrong-body".to_vec()
            } else {
                body.materialize()
            };
            let observed_content_type = if category == ProviderBinaryMismatchCategoryV1::ContentType
            {
                None
            } else {
                *content_type
            };
            let observed_retry_after = if category == ProviderBinaryMismatchCategoryV1::RetryAfter {
                Some("isolated-retry-after")
            } else {
                *retry_after
            };
            ProviderBinaryObservationV1::response(
                response(
                    observed_status,
                    &observed_body,
                    observed_content_type,
                    observed_retry_after,
                ),
                evidence,
            )
        }
        ProviderBinaryExpectedOutcomeV1::Failure { code } => {
            if category == ProviderBinaryMismatchCategoryV1::OutcomeKind {
                ProviderBinaryObservationV1::response(response(200, b"{}", None, None), evidence)
            } else {
                let observed_code = if category == ProviderBinaryMismatchCategoryV1::ErrorCode {
                    ProviderCallFailureCodeV1::RequestFailed
                } else {
                    *code
                };
                ProviderBinaryObservationV1::failure(observed_code, evidence)
            }
        }
    }
}

const fn observed_status(expected: u16, category: ProviderBinaryMismatchCategoryV1) -> u16 {
    if matches!(category, ProviderBinaryMismatchCategoryV1::Status) {
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
