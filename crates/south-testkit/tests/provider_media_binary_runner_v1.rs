use std::{
    collections::BTreeSet,
    fmt::Display,
    future::pending,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use http::StatusCode;
use south_contracts::BufferedBinaryResponseV1;
use south_provider_conformance::{
    PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_ID, PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_VERSION,
    ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderMediaBinaryCaseIdV1,
    ProviderMediaBinaryExpectedOutcomeV1, ProviderMediaBinaryFixtureV1,
    ProviderMediaBinaryUpstreamV1, provider_media_binary_fixtures_v1,
};
use south_testkit::{
    AssembledProviderMediaBinaryExecutionFutureV1, AssembledProviderMediaBinaryExecutorV1,
    MAX_PROVIDER_MEDIA_BINARY_MISMATCHES_V1, ProviderMediaBinaryConformanceFailureV1,
    ProviderMediaBinaryConformanceReportV1, ProviderMediaBinaryEvidenceV1,
    ProviderMediaBinaryMismatchCategoryV1, ProviderMediaBinaryMismatchV1,
    ProviderMediaBinaryObservationV1, run_provider_media_binary_conformance_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ProviderMediaBinaryObservationV1: Display);
assert_not_impl_any!(ProviderMediaBinaryEvidenceV1: Display);
assert_not_impl_any!(ProviderMediaBinaryConformanceReportV1: Display);
assert_not_impl_any!(ProviderMediaBinaryConformanceFailureV1: Display);

#[derive(Default)]
struct MatchingExecutor {
    order: Mutex<Vec<ProviderMediaBinaryCaseIdV1>>,
}

impl AssembledProviderMediaBinaryExecutorV1 for MatchingExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderMediaBinaryFixtureV1,
    ) -> AssembledProviderMediaBinaryExecutionFutureV1<'a> {
        Box::pin(async move {
            self.order.lock().expect("order lock should be available").push(fixture.case_id());
            observation_matching(fixture)
        })
    }
}

#[tokio::test]
async fn object_safe_send_executor_runs_every_case_in_canonical_order() {
    let executor = MatchingExecutor::default();
    let dynamic: &dyn AssembledProviderMediaBinaryExecutorV1 = &executor;
    let future = dynamic.execute_case(&provider_media_binary_fixtures_v1()[0]);
    assert_send(&future);
    drop(future);

    let report = run_provider_media_binary_conformance_v1(dynamic)
        .await
        .expect("matching observations must pass");
    assert_eq!(report.suite_id(), PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_ID);
    assert_eq!(report.suite_version(), PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_VERSION);
    let canonical: Vec<_> = provider_media_binary_fixtures_v1()
        .iter()
        .map(ProviderMediaBinaryFixtureV1::case_id)
        .collect();
    assert_eq!(report.passed_case_ids(), &canonical);
    assert_eq!(*executor.order.lock().expect("order lock should be available"), canonical);
}

struct SingleMismatchExecutor {
    case_id: ProviderMediaBinaryCaseIdV1,
    category: ProviderMediaBinaryMismatchCategoryV1,
}

impl AssembledProviderMediaBinaryExecutorV1 for SingleMismatchExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderMediaBinaryFixtureV1,
    ) -> AssembledProviderMediaBinaryExecutionFutureV1<'a> {
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
    use ProviderMediaBinaryCaseIdV1 as Case;
    use ProviderMediaBinaryMismatchCategoryV1 as Cat;
    let isolated_mismatches = [
        (Case::TextBinarySlotMismatch, Cat::OutcomeKind),
        (Case::TextContentTypeSmuggled, Cat::ErrorCode),
        (Case::MultipartBinarySuccess, Cat::Status),
        (Case::MultipartBinarySuccess, Cat::Body),
        (Case::MultipartBinaryRejectionCarriesBody, Cat::ContentType),
        (Case::MultipartBinaryRejectionCarriesBody, Cat::RetryAfter),
        (Case::TextBinaryBearerSuccess, Cat::ResolverCallCount),
        (Case::TextBinaryHeaderSecretSuccess, Cat::TransportCallCount),
        (Case::TextBinaryBearerSuccess, Cat::WireContentType),
        (Case::MultipartBinarySuccess, Cat::WireRequestBody),
        (Case::MultipartBinarySuccess, Cat::WireBinaryResponse),
        // The refused rows expect `false` for every wire claim, so a probe that hardcodes `true`
        // is caught there and not only on the reached rows.
        (Case::TextContentTypeSmuggled, Cat::WireContentType),
        (Case::TextBinarySlotMismatch, Cat::WireBinaryResponse),
    ];

    for (case_id, category) in isolated_mismatches {
        let failure =
            run_provider_media_binary_conformance_v1(&SingleMismatchExecutor { case_id, category })
                .await
                .expect_err("one deliberate difference must fail conformance");
        assert_eq!(failure.suite_id(), PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_ID);
        assert_eq!(failure.suite_version(), PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_VERSION);
        assert_eq!(failure.evaluated_case_count(), 6);
        assert!(failure.mismatches().len() <= MAX_PROVIDER_MEDIA_BINARY_MISMATCHES_V1);
        assert_eq!(failure.mismatches().len(), 1, "category {category:?} must isolate");
        let mismatch = &failure.mismatches()[0];
        assert_eq!(mismatch.case_id(), case_id);
        assert_eq!(mismatch.category(), category);
    }
}

/// An adapter that answers every outcome correctly but over a UTF-8 seam — the shortcut of
/// routing the multipart rows through the old multipart entry point and re-encoding — fails on
/// exactly the seam claim of every reached row and nowhere else.
#[tokio::test]
async fn an_executor_that_skips_the_bytes_seam_fails_only_the_seam_claim() {
    struct Utf8SeamExecutor;

    impl AssembledProviderMediaBinaryExecutorV1 for Utf8SeamExecutor {
        fn execute_case<'a>(
            &'a self,
            fixture: &'a ProviderMediaBinaryFixtureV1,
        ) -> AssembledProviderMediaBinaryExecutionFutureV1<'a> {
            Box::pin(async move {
                if matches!(fixture.upstream(), ProviderMediaBinaryUpstreamV1::Response(_)) {
                    observation_with_single_mismatch(
                        fixture,
                        ProviderMediaBinaryMismatchCategoryV1::WireBinaryResponse,
                    )
                } else {
                    // Refused rows reach no seam at all, so the shortcut cannot show there.
                    observation_matching(fixture)
                }
            })
        }
    }

    let failure = run_provider_media_binary_conformance_v1(&Utf8SeamExecutor)
        .await
        .expect_err("a UTF-8 seam must not pass");
    assert!(
        failure
            .mismatches()
            .iter()
            .all(|m| m.category() == ProviderMediaBinaryMismatchCategoryV1::WireBinaryResponse)
    );
    let seam: Vec<_> =
        failure.mismatches().iter().map(ProviderMediaBinaryMismatchV1::case_id).collect();
    assert_eq!(
        seam,
        [
            ProviderMediaBinaryCaseIdV1::MultipartBinarySuccess,
            ProviderMediaBinaryCaseIdV1::MultipartBinaryRejectionCarriesBody,
            ProviderMediaBinaryCaseIdV1::TextBinaryBearerSuccess,
            ProviderMediaBinaryCaseIdV1::TextBinaryHeaderSecretSuccess,
        ]
    );
}

#[tokio::test]
async fn a_fully_wrong_executor_reports_every_case_without_failing_fast() {
    struct WrongExecutor;

    impl AssembledProviderMediaBinaryExecutorV1 for WrongExecutor {
        fn execute_case<'a>(
            &'a self,
            fixture: &'a ProviderMediaBinaryFixtureV1,
        ) -> AssembledProviderMediaBinaryExecutionFutureV1<'a> {
            Box::pin(async move {
                let evidence = ProviderMediaBinaryEvidenceV1::new(257, 256, false, false, false);
                match fixture.upstream() {
                    ProviderMediaBinaryUpstreamV1::NotReached => {
                        ProviderMediaBinaryObservationV1::response(
                            response(200, b"{}", None, None),
                            evidence,
                        )
                    }
                    ProviderMediaBinaryUpstreamV1::Response(_) => {
                        ProviderMediaBinaryObservationV1::failure(
                            ProviderCallFailureCodeV1::RequestFailed,
                            evidence,
                        )
                    }
                }
            })
        }
    }

    let failure = run_provider_media_binary_conformance_v1(&WrongExecutor)
        .await
        .expect_err("deliberate mismatches must fail");
    assert_eq!(failure.evaluated_case_count(), 6);
    let categories: BTreeSet<_> =
        failure.mismatches().iter().map(ProviderMediaBinaryMismatchV1::category).collect();
    for category in [
        ProviderMediaBinaryMismatchCategoryV1::OutcomeKind,
        ProviderMediaBinaryMismatchCategoryV1::ResolverCallCount,
        ProviderMediaBinaryMismatchCategoryV1::TransportCallCount,
        ProviderMediaBinaryMismatchCategoryV1::WireContentType,
        ProviderMediaBinaryMismatchCategoryV1::WireRequestBody,
        ProviderMediaBinaryMismatchCategoryV1::WireBinaryResponse,
    ] {
        assert!(categories.contains(&category), "{category:?}");
    }
    for fixture in provider_media_binary_fixtures_v1() {
        assert!(failure.mismatches().iter().any(|m| m.case_id() == fixture.case_id()));
    }
}

#[test]
fn evidence_construction_saturates_large_boundary_counts() {
    let evidence = ProviderMediaBinaryEvidenceV1::new(256, 257, true, false, true);

    assert_eq!(evidence.resolver_calls(), ProviderCallCountV1::MoreThanOne);
    assert_eq!(evidence.transport_calls(), ProviderCallCountV1::MoreThanOne);
    assert!(evidence.wire_content_type_exact());
    assert!(!evidence.wire_request_body_exact());
    assert!(evidence.wire_binary_response_observed());
}

#[test]
fn debug_output_contains_only_safe_structural_evidence() {
    const BODY_SENTINEL: &[u8] = b"media-runner-body-debug-sentinel";
    const METADATA_SENTINEL: &str = "media-runner-metadata-debug-sentinel";

    let observation = ProviderMediaBinaryObservationV1::response(
        response(200, BODY_SENTINEL, Some(METADATA_SENTINEL), Some(METADATA_SENTINEL)),
        ProviderMediaBinaryEvidenceV1::new(1, 1, true, true, true),
    );

    let debug = format!("{observation:?}");
    for sentinel in ["media-runner-body-debug-sentinel", METADATA_SENTINEL] {
        assert!(!debug.contains(sentinel), "debug output leaked sentinel: {debug}");
    }
}

struct PendingExecutor {
    dropped: Arc<AtomicBool>,
}

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl AssembledProviderMediaBinaryExecutorV1 for PendingExecutor {
    fn execute_case<'a>(
        &'a self,
        _fixture: &'a ProviderMediaBinaryFixtureV1,
    ) -> AssembledProviderMediaBinaryExecutionFutureV1<'a> {
        Box::pin(async move {
            let _drop_flag = DropFlag(Arc::clone(&self.dropped));
            pending().await
        })
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn caller_watchdog_drops_a_permanently_pending_runner_without_detached_work() {
    let dropped = Arc::new(AtomicBool::new(false));
    let executor = PendingExecutor { dropped: Arc::clone(&dropped) };
    let structured_run = async { run_provider_media_binary_conformance_v1(&executor).await };

    let result = tokio::time::timeout(Duration::from_secs(5), structured_run).await;

    assert!(result.is_err());
    assert!(dropped.load(Ordering::SeqCst));
}

fn observation_matching(
    fixture: &ProviderMediaBinaryFixtureV1,
) -> ProviderMediaBinaryObservationV1 {
    observation_with_single_mismatch_inner(fixture, None)
}

fn observation_with_single_mismatch(
    fixture: &ProviderMediaBinaryFixtureV1,
    category: ProviderMediaBinaryMismatchCategoryV1,
) -> ProviderMediaBinaryObservationV1 {
    observation_with_single_mismatch_inner(fixture, Some(category))
}

fn observation_with_single_mismatch_inner(
    fixture: &ProviderMediaBinaryFixtureV1,
    category: Option<ProviderMediaBinaryMismatchCategoryV1>,
) -> ProviderMediaBinaryObservationV1 {
    use ProviderMediaBinaryMismatchCategoryV1 as Cat;
    let is = |wanted: Cat| category == Some(wanted);
    let expected = fixture.expected().evidence();
    let mut resolver_calls = count_value(expected.resolver_calls());
    let mut transport_calls = count_value(expected.transport_calls());
    if is(Cat::ResolverCallCount) {
        resolver_calls = different_count(resolver_calls);
    }
    if is(Cat::TransportCallCount) {
        transport_calls = different_count(transport_calls);
    }
    let evidence = ProviderMediaBinaryEvidenceV1::new(
        resolver_calls,
        transport_calls,
        expected.wire_content_type_exact() ^ is(Cat::WireContentType),
        expected.wire_request_body_exact() ^ is(Cat::WireRequestBody),
        expected.wire_binary_response_observed() ^ is(Cat::WireBinaryResponse),
    );

    match fixture.expected().outcome() {
        ProviderMediaBinaryExpectedOutcomeV1::Response {
            status,
            body,
            content_type,
            retry_after,
        } => ProviderMediaBinaryObservationV1::response(
            response(
                if is(Cat::Status) { status + 1 } else { *status },
                if is(Cat::Body) { b"isolated-wrong-body" } else { body },
                if is(Cat::ContentType) { None } else { *content_type },
                if is(Cat::RetryAfter) { Some("isolated-retry-after") } else { *retry_after },
            ),
            evidence,
        ),
        ProviderMediaBinaryExpectedOutcomeV1::Failure { code } => {
            if is(Cat::OutcomeKind) {
                ProviderMediaBinaryObservationV1::response(
                    response(200, b"{}", None, None),
                    evidence,
                )
            } else if is(Cat::ErrorCode) {
                ProviderMediaBinaryObservationV1::failure(
                    ProviderCallFailureCodeV1::RequestFailed,
                    evidence,
                )
            } else {
                ProviderMediaBinaryObservationV1::failure(*code, evidence)
            }
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
