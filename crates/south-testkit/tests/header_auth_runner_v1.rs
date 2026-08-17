use std::{
    collections::BTreeSet,
    fmt::Display,
    future::pending,
    sync::{Arc, Mutex},
    time::Duration,
};

use bytes::Bytes;
use http::StatusCode;
use south_contracts::{BufferedHttpResponseV1, StreamChunkV1, StreamingResponseHeadV1};
use south_provider_conformance::{
    HEADER_AUTH_CONFORMANCE_SUITE_ID, HEADER_AUTH_CONFORMANCE_SUITE_VERSION, HeaderAuthCaseIdV1,
    HeaderAuthExpectedOutcomeV1, HeaderAuthFixtureV1, ProviderCallCountV1,
    ProviderCallFailureCodeV1, header_auth_fixtures_v1,
};
use south_testkit::{
    AssembledHeaderAuthExecutionFutureV1, AssembledHeaderAuthExecutorV1,
    HeaderAuthConformanceFailureV1, HeaderAuthConformanceReportV1, HeaderAuthEvidenceV1,
    HeaderAuthMismatchCategoryV1, HeaderAuthObservationV1, MAX_HEADER_AUTH_MISMATCHES_V1,
    run_header_auth_conformance_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(HeaderAuthObservationV1: Display);
assert_not_impl_any!(HeaderAuthEvidenceV1: Display);
assert_not_impl_any!(HeaderAuthConformanceReportV1: Display);
assert_not_impl_any!(HeaderAuthConformanceFailureV1: Display);

#[derive(Default)]
struct MatchingExecutor {
    order: Mutex<Vec<HeaderAuthCaseIdV1>>,
}

impl AssembledHeaderAuthExecutorV1 for MatchingExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a HeaderAuthFixtureV1,
    ) -> AssembledHeaderAuthExecutionFutureV1<'a> {
        Box::pin(async move {
            self.order.lock().expect("order lock should be available").push(fixture.case_id());
            observation_matching(fixture)
        })
    }
}

#[tokio::test]
async fn object_safe_send_executor_runs_every_case_in_canonical_order() {
    let executor = MatchingExecutor::default();
    let dynamic: &dyn AssembledHeaderAuthExecutorV1 = &executor;
    let future = dynamic.execute_case(&header_auth_fixtures_v1()[0]);
    assert_send(&future);
    drop(future);

    let report =
        run_header_auth_conformance_v1(dynamic).await.expect("matching observations must pass");
    assert_eq!(report.suite_id(), HEADER_AUTH_CONFORMANCE_SUITE_ID);
    assert_eq!(report.suite_version(), HEADER_AUTH_CONFORMANCE_SUITE_VERSION);
    assert_eq!(
        report.passed_case_ids(),
        &header_auth_fixtures_v1().iter().map(HeaderAuthFixtureV1::case_id).collect::<Vec<_>>()
    );
    assert_eq!(
        *executor.order.lock().expect("order lock should be available"),
        header_auth_fixtures_v1().iter().map(HeaderAuthFixtureV1::case_id).collect::<Vec<_>>()
    );
}

struct SingleMismatchExecutor {
    case_id: HeaderAuthCaseIdV1,
    category: HeaderAuthMismatchCategoryV1,
}

impl AssembledHeaderAuthExecutorV1 for SingleMismatchExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a HeaderAuthFixtureV1,
    ) -> AssembledHeaderAuthExecutionFutureV1<'a> {
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
        (HeaderAuthCaseIdV1::HeaderSecretSlotMismatch, HeaderAuthMismatchCategoryV1::OutcomeKind),
        (HeaderAuthCaseIdV1::HeaderSecretSlotMismatch, HeaderAuthMismatchCategoryV1::ErrorCode),
        (HeaderAuthCaseIdV1::BufferedHeaderSecretSuccess, HeaderAuthMismatchCategoryV1::Status),
        (HeaderAuthCaseIdV1::BufferedHeaderSecretSuccess, HeaderAuthMismatchCategoryV1::Body),
        (
            HeaderAuthCaseIdV1::BufferedHeaderSecretSuccess,
            HeaderAuthMismatchCategoryV1::ContentType,
        ),
        (HeaderAuthCaseIdV1::BufferedHeaderSecretSuccess, HeaderAuthMismatchCategoryV1::RetryAfter),
        (
            HeaderAuthCaseIdV1::StreamingHeaderSecretSuccess,
            HeaderAuthMismatchCategoryV1::ChunkBytes,
        ),
        (
            HeaderAuthCaseIdV1::BufferedHeaderSecretSuccess,
            HeaderAuthMismatchCategoryV1::ResolverCallCount,
        ),
        (
            HeaderAuthCaseIdV1::StreamingHeaderSecretSuccess,
            HeaderAuthMismatchCategoryV1::TransportCallCount,
        ),
        (
            HeaderAuthCaseIdV1::BufferedHeaderSecretSuccess,
            HeaderAuthMismatchCategoryV1::SanctionedHeader,
        ),
        (
            HeaderAuthCaseIdV1::StreamingHeaderSecretSuccess,
            HeaderAuthMismatchCategoryV1::AuthorizationPresence,
        ),
    ];

    for (case_id, category) in isolated_mismatches {
        let failure = run_header_auth_conformance_v1(&SingleMismatchExecutor { case_id, category })
            .await
            .expect_err("one deliberate difference must fail conformance");
        assert_eq!(failure.suite_id(), HEADER_AUTH_CONFORMANCE_SUITE_ID);
        assert_eq!(failure.suite_version(), HEADER_AUTH_CONFORMANCE_SUITE_VERSION);
        assert_eq!(failure.evaluated_case_count(), 3);
        assert!(failure.mismatches().len() <= MAX_HEADER_AUTH_MISMATCHES_V1);
        assert_eq!(failure.mismatches().len(), 1, "category {category:?} must isolate");
        let mismatch = &failure.mismatches()[0];
        assert_eq!(mismatch.case_id(), case_id);
        assert_eq!(mismatch.category(), category);
    }
}

#[tokio::test]
async fn a_fully_wrong_executor_reports_every_case_without_failing_fast() {
    struct WrongExecutor;

    impl AssembledHeaderAuthExecutorV1 for WrongExecutor {
        fn execute_case<'a>(
            &'a self,
            fixture: &'a HeaderAuthFixtureV1,
        ) -> AssembledHeaderAuthExecutionFutureV1<'a> {
            Box::pin(async move {
                let evidence = HeaderAuthEvidenceV1::new(257, 256, false, false);
                match fixture.case_id() {
                    HeaderAuthCaseIdV1::BufferedHeaderSecretSuccess => {
                        HeaderAuthObservationV1::failure(
                            ProviderCallFailureCodeV1::RequestFailed,
                            evidence,
                        )
                    }
                    _ => HeaderAuthObservationV1::opened(
                        head(StatusCode::OK, None, None),
                        Vec::new(),
                        evidence,
                    ),
                }
            })
        }
    }

    let failure = run_header_auth_conformance_v1(&WrongExecutor)
        .await
        .expect_err("deliberate mismatches must fail");
    assert_eq!(failure.evaluated_case_count(), 3);

    let categories: BTreeSet<_> =
        failure.mismatches().iter().map(south_testkit::HeaderAuthMismatchV1::category).collect();
    assert!(categories.contains(&HeaderAuthMismatchCategoryV1::OutcomeKind));
    assert!(categories.contains(&HeaderAuthMismatchCategoryV1::ResolverCallCount));
    assert!(categories.contains(&HeaderAuthMismatchCategoryV1::TransportCallCount));
    assert!(categories.contains(&HeaderAuthMismatchCategoryV1::SanctionedHeader));
    assert!(categories.contains(&HeaderAuthMismatchCategoryV1::AuthorizationPresence));
    for fixture in header_auth_fixtures_v1() {
        assert!(failure.mismatches().iter().any(|m| m.case_id() == fixture.case_id()));
    }
}

#[test]
fn evidence_construction_saturates_large_boundary_counts() {
    let evidence = HeaderAuthEvidenceV1::new(256, 257, true, false);

    assert_eq!(evidence.resolver_calls(), ProviderCallCountV1::MoreThanOne);
    assert_eq!(evidence.transport_calls(), ProviderCallCountV1::MoreThanOne);
    assert!(evidence.sanctioned_header_exact());
    assert!(!evidence.authorization_header_absent());
}

#[test]
fn debug_output_contains_only_safe_structural_evidence() {
    const BODY_SENTINEL: &str = "header-auth-runner-body-debug-sentinel";
    const METADATA_SENTINEL: &str = "header-auth-runner-metadata-debug-sentinel";
    const CHUNK_SENTINEL: &str = "header-auth-runner-chunk-debug-sentinel";

    let evidence = HeaderAuthEvidenceV1::new(1, 1, true, true);
    let response = HeaderAuthObservationV1::response(
        BufferedHttpResponseV1::try_from_parts(
            StatusCode::OK,
            BODY_SENTINEL.as_bytes().to_vec(),
            Some(METADATA_SENTINEL.to_owned()),
            Some(METADATA_SENTINEL.to_owned()),
        )
        .expect("fixture response should be valid"),
        evidence,
    );
    let opened = HeaderAuthObservationV1::opened(
        head(StatusCode::OK, Some(METADATA_SENTINEL), None),
        vec![
            StreamChunkV1::try_new(Bytes::from_static(CHUNK_SENTINEL.as_bytes()))
                .expect("sentinel chunk should satisfy bounds"),
        ],
        evidence,
    );

    let debug = format!("{response:?} {opened:?}");
    for sentinel in [BODY_SENTINEL, METADATA_SENTINEL, CHUNK_SENTINEL] {
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

impl AssembledHeaderAuthExecutorV1 for PendingExecutor {
    fn execute_case<'a>(
        &'a self,
        _fixture: &'a HeaderAuthFixtureV1,
    ) -> AssembledHeaderAuthExecutionFutureV1<'a> {
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
    let structured_run = async { run_header_auth_conformance_v1(&executor).await };

    let result = tokio::time::timeout(Duration::from_secs(5), structured_run).await;

    assert!(result.is_err());
    assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
}

fn head(
    status: StatusCode,
    content_type: Option<&str>,
    retry_after: Option<&str>,
) -> StreamingResponseHeadV1 {
    StreamingResponseHeadV1::try_from_parts(
        status,
        content_type.map(str::to_owned),
        retry_after.map(str::to_owned),
    )
    .expect("fixture head should be valid")
}

fn expected_chunks(chunks: &'static [&'static [u8]]) -> Vec<StreamChunkV1> {
    chunks
        .iter()
        .map(|chunk| {
            StreamChunkV1::try_new(Bytes::from_static(chunk))
                .expect("expected chunk should satisfy bounds")
        })
        .collect()
}

const fn matching_evidence(fixture: &HeaderAuthFixtureV1) -> HeaderAuthEvidenceV1 {
    let evidence = fixture.expected().evidence();
    HeaderAuthEvidenceV1::new(
        count_value(evidence.resolver_calls()),
        count_value(evidence.transport_calls()),
        evidence.sanctioned_header_exact(),
        evidence.authorization_header_absent(),
    )
}

fn observation_matching(fixture: &HeaderAuthFixtureV1) -> HeaderAuthObservationV1 {
    let evidence = matching_evidence(fixture);
    match fixture.expected().outcome() {
        HeaderAuthExpectedOutcomeV1::Response { status, body, content_type, retry_after } => {
            HeaderAuthObservationV1::response(
                response(*status, body, *content_type, *retry_after),
                evidence,
            )
        }
        HeaderAuthExpectedOutcomeV1::Opened { status, content_type, retry_after, chunks } => {
            HeaderAuthObservationV1::opened(
                head(
                    StatusCode::from_u16(*status).expect("expected status should be valid"),
                    *content_type,
                    *retry_after,
                ),
                expected_chunks(chunks),
                evidence,
            )
        }
        HeaderAuthExpectedOutcomeV1::Failure { code } => {
            HeaderAuthObservationV1::failure(*code, evidence)
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
    fixture: &HeaderAuthFixtureV1,
    category: HeaderAuthMismatchCategoryV1,
) -> HeaderAuthObservationV1 {
    let expected_evidence = fixture.expected().evidence();
    let mut resolver_calls = count_value(expected_evidence.resolver_calls());
    let mut transport_calls = count_value(expected_evidence.transport_calls());
    let mut sanctioned = expected_evidence.sanctioned_header_exact();
    let mut authorization_absent = expected_evidence.authorization_header_absent();
    match category {
        HeaderAuthMismatchCategoryV1::ResolverCallCount => {
            resolver_calls = different_count(resolver_calls);
        }
        HeaderAuthMismatchCategoryV1::TransportCallCount => {
            transport_calls = different_count(transport_calls);
        }
        HeaderAuthMismatchCategoryV1::SanctionedHeader => sanctioned = !sanctioned,
        HeaderAuthMismatchCategoryV1::AuthorizationPresence => {
            authorization_absent = !authorization_absent;
        }
        _ => {}
    }
    let evidence = HeaderAuthEvidenceV1::new(
        resolver_calls,
        transport_calls,
        sanctioned,
        authorization_absent,
    );

    match fixture.expected().outcome() {
        HeaderAuthExpectedOutcomeV1::Response { status, body, content_type, retry_after } => {
            let observed_status = observed_status(*status, category);
            let observed_body = if category == HeaderAuthMismatchCategoryV1::Body {
                "isolated-wrong-body"
            } else {
                body
            };
            let observed_content_type = if category == HeaderAuthMismatchCategoryV1::ContentType {
                None
            } else {
                *content_type
            };
            let observed_retry_after = if category == HeaderAuthMismatchCategoryV1::RetryAfter {
                None
            } else {
                *retry_after
            };
            HeaderAuthObservationV1::response(
                response(
                    observed_status,
                    observed_body,
                    observed_content_type,
                    observed_retry_after,
                ),
                evidence,
            )
        }
        HeaderAuthExpectedOutcomeV1::Opened { status, content_type, retry_after, chunks } => {
            let observed_status = observed_status(*status, category);
            let mut observed_chunks = expected_chunks(chunks);
            if category == HeaderAuthMismatchCategoryV1::ChunkBytes {
                observed_chunks[0] =
                    StreamChunkV1::try_new(Bytes::from_static(b"isolated-wrong-chunk"))
                        .expect("isolated chunk should satisfy bounds");
            }
            HeaderAuthObservationV1::opened(
                head(
                    StatusCode::from_u16(observed_status).expect("shifted status should be valid"),
                    *content_type,
                    *retry_after,
                ),
                observed_chunks,
                evidence,
            )
        }
        HeaderAuthExpectedOutcomeV1::Failure { code } => {
            if category == HeaderAuthMismatchCategoryV1::OutcomeKind {
                HeaderAuthObservationV1::opened(
                    head(StatusCode::OK, None, None),
                    Vec::new(),
                    evidence,
                )
            } else {
                let observed_code = if category == HeaderAuthMismatchCategoryV1::ErrorCode {
                    ProviderCallFailureCodeV1::RequestFailed
                } else {
                    *code
                };
                HeaderAuthObservationV1::failure(observed_code, evidence)
            }
        }
    }
}

const fn observed_status(expected: u16, category: HeaderAuthMismatchCategoryV1) -> u16 {
    if matches!(category, HeaderAuthMismatchCategoryV1::Status) { expected + 1 } else { expected }
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
