use std::{
    collections::BTreeSet,
    fmt::Display,
    future::pending,
    sync::{Arc, Mutex},
    time::Duration,
};

use bytes::Bytes;
use http::StatusCode;
use south_contracts::{
    StreamChunkV1, StreamReadErrorV1, StreamRejectedV1, StreamingResponseHeadV1,
};
use south_provider_conformance::{
    PROVIDER_STREAM_CONFORMANCE_SUITE_ID, PROVIDER_STREAM_CONFORMANCE_SUITE_VERSION,
    ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderStreamCaseIdV1,
    ProviderStreamExpectedOutcomeV1, ProviderStreamFixtureV1, provider_stream_fixtures_v1,
};
use south_testkit::{
    AssembledProviderStreamExecutorV1, AssembledStreamExecutionFutureV1,
    MAX_PROVIDER_STREAM_MISMATCHES_V1, ProviderStreamConformanceFailureV1,
    ProviderStreamConformanceReportV1, ProviderStreamEvidenceV1, ProviderStreamMismatchCategoryV1,
    ProviderStreamObservationV1, run_provider_stream_conformance_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ProviderStreamObservationV1: Display);
assert_not_impl_any!(ProviderStreamEvidenceV1: Display);
assert_not_impl_any!(ProviderStreamConformanceReportV1: Display);
assert_not_impl_any!(ProviderStreamConformanceFailureV1: Display);

#[derive(Default)]
struct MatchingExecutor {
    order: Mutex<Vec<ProviderStreamCaseIdV1>>,
}

impl AssembledProviderStreamExecutorV1 for MatchingExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderStreamFixtureV1,
    ) -> AssembledStreamExecutionFutureV1<'a> {
        Box::pin(async move {
            self.order.lock().expect("order lock should be available").push(fixture.case_id());
            observation_matching(fixture)
        })
    }
}

#[tokio::test]
async fn object_safe_send_executor_runs_every_case_in_canonical_order() {
    let executor = MatchingExecutor::default();
    let dynamic: &dyn AssembledProviderStreamExecutorV1 = &executor;
    let future = dynamic.execute_case(&provider_stream_fixtures_v1()[0]);
    assert_send(&future);
    drop(future);

    let report =
        run_provider_stream_conformance_v1(dynamic).await.expect("matching observations must pass");
    assert_eq!(report.suite_id(), PROVIDER_STREAM_CONFORMANCE_SUITE_ID);
    assert_eq!(report.suite_version(), PROVIDER_STREAM_CONFORMANCE_SUITE_VERSION);
    assert_eq!(
        report.passed_case_ids(),
        &provider_stream_fixtures_v1()
            .iter()
            .map(ProviderStreamFixtureV1::case_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        *executor.order.lock().expect("order lock should be available"),
        provider_stream_fixtures_v1()
            .iter()
            .map(ProviderStreamFixtureV1::case_id)
            .collect::<Vec<_>>()
    );
}

struct SingleMismatchExecutor {
    case_id: ProviderStreamCaseIdV1,
    category: ProviderStreamMismatchCategoryV1,
}

impl AssembledProviderStreamExecutorV1 for SingleMismatchExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderStreamFixtureV1,
    ) -> AssembledStreamExecutionFutureV1<'a> {
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
        (
            ProviderStreamCaseIdV1::InvalidRelativePath,
            ProviderStreamMismatchCategoryV1::OutcomeKind,
        ),
        (ProviderStreamCaseIdV1::RedirectDenied, ProviderStreamMismatchCategoryV1::ErrorCode),
        (ProviderStreamCaseIdV1::StreamSuccess, ProviderStreamMismatchCategoryV1::Status),
        (ProviderStreamCaseIdV1::StreamSuccess, ProviderStreamMismatchCategoryV1::ContentType),
        (
            ProviderStreamCaseIdV1::RejectedUpstreamStatus,
            ProviderStreamMismatchCategoryV1::RetryAfter,
        ),
        (
            ProviderStreamCaseIdV1::RejectedUpstreamStatus,
            ProviderStreamMismatchCategoryV1::RejectedBody,
        ),
        (ProviderStreamCaseIdV1::StreamSuccess, ProviderStreamMismatchCategoryV1::ChunkBytes),
        (
            ProviderStreamCaseIdV1::RedirectDenied,
            ProviderStreamMismatchCategoryV1::ResolverCallCount,
        ),
        (
            ProviderStreamCaseIdV1::RejectedUpstreamStatus,
            ProviderStreamMismatchCategoryV1::TransportCallCount,
        ),
        (
            ProviderStreamCaseIdV1::StreamSuccess,
            ProviderStreamMismatchCategoryV1::ResolverPendingDrop,
        ),
        (
            ProviderStreamCaseIdV1::CancelBetweenChunks,
            ProviderStreamMismatchCategoryV1::TransportPendingDrop,
        ),
        (ProviderStreamCaseIdV1::StreamSuccess, ProviderStreamMismatchCategoryV1::ChunksPulled),
        (
            ProviderStreamCaseIdV1::UpstreamBreakMidStream,
            ProviderStreamMismatchCategoryV1::PoststreamErrorCode,
        ),
    ];

    for (case_id, category) in isolated_mismatches {
        let failure =
            run_provider_stream_conformance_v1(&SingleMismatchExecutor { case_id, category })
                .await
                .expect_err("one deliberate difference must fail conformance");
        assert_eq!(failure.suite_id(), PROVIDER_STREAM_CONFORMANCE_SUITE_ID);
        assert_eq!(failure.suite_version(), PROVIDER_STREAM_CONFORMANCE_SUITE_VERSION);
        assert_eq!(failure.evaluated_case_count(), 9);
        assert!(failure.mismatches().len() <= MAX_PROVIDER_STREAM_MISMATCHES_V1);
        assert_eq!(failure.mismatches().len(), 1, "category {category:?} must isolate");
        let mismatch = &failure.mismatches()[0];
        assert_eq!(mismatch.case_id(), case_id);
        assert_eq!(mismatch.category(), category);
    }
}

#[tokio::test]
async fn a_fully_wrong_executor_reports_every_category_without_failing_fast() {
    struct WrongExecutor;

    impl AssembledProviderStreamExecutorV1 for WrongExecutor {
        fn execute_case<'a>(
            &'a self,
            fixture: &'a ProviderStreamFixtureV1,
        ) -> AssembledStreamExecutionFutureV1<'a> {
            Box::pin(async move {
                let evidence = ProviderStreamEvidenceV1::new(
                    257,
                    256,
                    true,
                    true,
                    99,
                    Some(StreamReadErrorV1::ChunkNotDeliverable),
                );
                match fixture.case_id() {
                    ProviderStreamCaseIdV1::StreamSuccess => ProviderStreamObservationV1::failure(
                        ProviderCallFailureCodeV1::RequestFailed,
                        evidence,
                    ),
                    _ => ProviderStreamObservationV1::opened(
                        head(StatusCode::OK, None, None),
                        Vec::new(),
                        evidence,
                    ),
                }
            })
        }
    }

    let failure = run_provider_stream_conformance_v1(&WrongExecutor)
        .await
        .expect_err("deliberate mismatches must fail");
    assert_eq!(failure.evaluated_case_count(), 9);

    let categories: BTreeSet<_> = failure
        .mismatches()
        .iter()
        .map(south_testkit::ProviderStreamMismatchV1::category)
        .collect();
    assert!(categories.contains(&ProviderStreamMismatchCategoryV1::OutcomeKind));
    assert!(categories.contains(&ProviderStreamMismatchCategoryV1::ResolverCallCount));
    assert!(categories.contains(&ProviderStreamMismatchCategoryV1::TransportCallCount));
    assert!(categories.contains(&ProviderStreamMismatchCategoryV1::ChunksPulled));
    assert!(categories.contains(&ProviderStreamMismatchCategoryV1::PoststreamErrorCode));
    for fixture in provider_stream_fixtures_v1() {
        assert!(failure.mismatches().iter().any(|m| m.case_id() == fixture.case_id()));
    }
}

#[test]
fn evidence_construction_saturates_large_boundary_counts_but_keeps_chunks_exact() {
    let evidence = ProviderStreamEvidenceV1::new(
        256,
        257,
        false,
        true,
        1234,
        Some(StreamReadErrorV1::StreamIdleTimeout),
    );

    assert_eq!(evidence.resolver_calls(), ProviderCallCountV1::MoreThanOne);
    assert_eq!(evidence.transport_calls(), ProviderCallCountV1::MoreThanOne);
    assert!(!evidence.resolver_future_dropped_while_pending());
    assert!(evidence.transport_future_dropped_while_pending());
    assert_eq!(evidence.chunks_pulled(), 1234);
    assert_eq!(evidence.poststream_error_code(), Some(StreamReadErrorV1::StreamIdleTimeout));
}

#[test]
fn debug_output_contains_only_safe_structural_evidence() {
    const CHUNK_SENTINEL: &str = "stream-runner-chunk-debug-sentinel";
    const METADATA_SENTINEL: &str = "stream-runner-metadata-debug-sentinel";
    const REJECTED_SENTINEL: &str = "stream-runner-rejected-debug-sentinel";

    let evidence = ProviderStreamEvidenceV1::new(1, 1, false, false, 1, None);
    let opened = ProviderStreamObservationV1::opened(
        head(StatusCode::OK, Some(METADATA_SENTINEL), Some(METADATA_SENTINEL)),
        vec![
            StreamChunkV1::try_new(Bytes::from_static(CHUNK_SENTINEL.as_bytes()))
                .expect("sentinel chunk should satisfy bounds"),
        ],
        evidence,
    );
    let rejected = ProviderStreamObservationV1::rejected(
        StreamRejectedV1::new(
            head(StatusCode::BAD_GATEWAY, Some(METADATA_SENTINEL), None),
            REJECTED_SENTINEL.as_bytes().to_vec(),
        ),
        evidence,
    );

    let debug = format!("{opened:?} {rejected:?}");
    for sentinel in [CHUNK_SENTINEL, METADATA_SENTINEL, REJECTED_SENTINEL] {
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

impl AssembledProviderStreamExecutorV1 for PendingExecutor {
    fn execute_case<'a>(
        &'a self,
        _fixture: &'a ProviderStreamFixtureV1,
    ) -> AssembledStreamExecutionFutureV1<'a> {
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
    let structured_run = async { run_provider_stream_conformance_v1(&executor).await };

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

fn observation_matching(fixture: &ProviderStreamFixtureV1) -> ProviderStreamObservationV1 {
    let expected = fixture.expected();
    let evidence = expected.evidence();
    let observed_evidence = ProviderStreamEvidenceV1::new(
        count_value(evidence.resolver_calls()),
        count_value(evidence.transport_calls()),
        evidence.resolver_future_dropped_while_pending(),
        evidence.transport_future_dropped_while_pending(),
        evidence.chunks_pulled(),
        evidence.poststream_error_code(),
    );
    match expected.outcome() {
        ProviderStreamExpectedOutcomeV1::Opened { status, content_type, retry_after, chunks } => {
            ProviderStreamObservationV1::opened(
                head(
                    StatusCode::from_u16(*status).expect("expected status should be valid"),
                    *content_type,
                    *retry_after,
                ),
                expected_chunks(chunks),
                observed_evidence,
            )
        }
        ProviderStreamExpectedOutcomeV1::Rejected { status, content_type, retry_after, body } => {
            ProviderStreamObservationV1::rejected(
                StreamRejectedV1::new(
                    head(
                        StatusCode::from_u16(*status).expect("expected status should be valid"),
                        *content_type,
                        *retry_after,
                    ),
                    body.to_vec(),
                ),
                observed_evidence,
            )
        }
        ProviderStreamExpectedOutcomeV1::Failure { code } => {
            ProviderStreamObservationV1::failure(*code, observed_evidence)
        }
    }
}

fn observation_with_single_mismatch(
    fixture: &ProviderStreamFixtureV1,
    category: ProviderStreamMismatchCategoryV1,
) -> ProviderStreamObservationV1 {
    let expected = fixture.expected();
    let expected_evidence = expected.evidence();
    let mut resolver_calls = count_value(expected_evidence.resolver_calls());
    let mut transport_calls = count_value(expected_evidence.transport_calls());
    let mut resolver_drop = expected_evidence.resolver_future_dropped_while_pending();
    let mut transport_drop = expected_evidence.transport_future_dropped_while_pending();
    let mut chunks_pulled = expected_evidence.chunks_pulled();
    let mut poststream = expected_evidence.poststream_error_code();
    match category {
        ProviderStreamMismatchCategoryV1::ResolverCallCount => {
            resolver_calls = different_count(resolver_calls);
        }
        ProviderStreamMismatchCategoryV1::TransportCallCount => {
            transport_calls = different_count(transport_calls);
        }
        ProviderStreamMismatchCategoryV1::ResolverPendingDrop => resolver_drop = !resolver_drop,
        ProviderStreamMismatchCategoryV1::TransportPendingDrop => transport_drop = !transport_drop,
        ProviderStreamMismatchCategoryV1::ChunksPulled => chunks_pulled += 1,
        ProviderStreamMismatchCategoryV1::PoststreamErrorCode => {
            poststream = match poststream {
                Some(_) => None,
                None => Some(StreamReadErrorV1::StreamReadFailed),
            };
        }
        _ => {}
    }
    let evidence = ProviderStreamEvidenceV1::new(
        resolver_calls,
        transport_calls,
        resolver_drop,
        transport_drop,
        chunks_pulled,
        poststream,
    );

    match expected.outcome() {
        ProviderStreamExpectedOutcomeV1::Opened { status, content_type, retry_after, chunks } => {
            let observed_status = observed_status(*status, category);
            let observed_content_type = if category == ProviderStreamMismatchCategoryV1::ContentType
            {
                None
            } else {
                *content_type
            };
            let observed_retry_after = if category == ProviderStreamMismatchCategoryV1::RetryAfter {
                None
            } else {
                *retry_after
            };
            let mut observed_chunks = expected_chunks(chunks);
            if category == ProviderStreamMismatchCategoryV1::ChunkBytes {
                observed_chunks[0] =
                    StreamChunkV1::try_new(Bytes::from_static(b"isolated-wrong-chunk"))
                        .expect("isolated chunk should satisfy bounds");
            }
            ProviderStreamObservationV1::opened(
                head(observed_status, observed_content_type, observed_retry_after),
                observed_chunks,
                evidence,
            )
        }
        ProviderStreamExpectedOutcomeV1::Rejected { status, content_type, retry_after, body } => {
            let observed_status = observed_status(*status, category);
            let observed_retry_after = if category == ProviderStreamMismatchCategoryV1::RetryAfter {
                None
            } else {
                *retry_after
            };
            let observed_body = if category == ProviderStreamMismatchCategoryV1::RejectedBody {
                b"isolated-wrong-body".to_vec()
            } else {
                body.to_vec()
            };
            ProviderStreamObservationV1::rejected(
                StreamRejectedV1::new(
                    head(observed_status, *content_type, observed_retry_after),
                    observed_body,
                ),
                evidence,
            )
        }
        ProviderStreamExpectedOutcomeV1::Failure { code } => {
            if category == ProviderStreamMismatchCategoryV1::OutcomeKind {
                ProviderStreamObservationV1::opened(
                    head(StatusCode::OK, None, None),
                    Vec::new(),
                    evidence,
                )
            } else {
                let observed_code = if category == ProviderStreamMismatchCategoryV1::ErrorCode {
                    ProviderCallFailureCodeV1::RequestFailed
                } else {
                    *code
                };
                ProviderStreamObservationV1::failure(observed_code, evidence)
            }
        }
    }
}

fn observed_status(expected: u16, category: ProviderStreamMismatchCategoryV1) -> StatusCode {
    if category == ProviderStreamMismatchCategoryV1::Status {
        StatusCode::from_u16(expected + 1).expect("shifted status should be valid")
    } else {
        StatusCode::from_u16(expected).expect("expected status should be valid")
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
