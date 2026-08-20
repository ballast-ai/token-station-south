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
    CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_ID, CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_VERSION,
    ControlledUserAgentCaseIdV1, ControlledUserAgentExpectedOutcomeV1,
    ControlledUserAgentFixtureV1, ProviderCallCountV1, ProviderCallFailureCodeV1,
    controlled_user_agent_fixtures_v1,
};
use south_testkit::{
    AssembledControlledUserAgentExecutionFutureV1, AssembledControlledUserAgentExecutorV1,
    ControlledUserAgentConformanceFailureV1, ControlledUserAgentConformanceReportV1,
    ControlledUserAgentEvidenceV1, ControlledUserAgentMismatchCategoryV1,
    ControlledUserAgentObservationV1, MAX_CONTROLLED_USER_AGENT_MISMATCHES_V1,
    ReferenceAssembledControlledUserAgentExecutorV1, run_controlled_user_agent_conformance_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ControlledUserAgentObservationV1: Display);
assert_not_impl_any!(ControlledUserAgentEvidenceV1: Display);
assert_not_impl_any!(ControlledUserAgentConformanceReportV1: Display);
assert_not_impl_any!(ControlledUserAgentConformanceFailureV1: Display);

#[tokio::test]
async fn the_reference_executor_passes_every_canonical_case() {
    let executor = ReferenceAssembledControlledUserAgentExecutorV1::new();

    let report = tokio::time::timeout(
        Duration::from_secs(30),
        run_controlled_user_agent_conformance_v1(&executor),
    )
    .await
    .expect("the reference suite must finish inside the caller watchdog")
    .expect("the reference executor must pass every canonical case");

    assert_eq!(report.suite_id(), CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_ID);
    assert_eq!(report.suite_version(), CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_VERSION);
    assert_eq!(
        report.passed_case_ids(),
        &controlled_user_agent_fixtures_v1()
            .iter()
            .map(ControlledUserAgentFixtureV1::case_id)
            .collect::<Vec<_>>()
    );
}

#[derive(Default)]
struct MatchingExecutor {
    order: Mutex<Vec<ControlledUserAgentCaseIdV1>>,
}

impl AssembledControlledUserAgentExecutorV1 for MatchingExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ControlledUserAgentFixtureV1,
    ) -> AssembledControlledUserAgentExecutionFutureV1<'a> {
        Box::pin(async move {
            self.order.lock().expect("order lock should be available").push(fixture.case_id());
            observation_matching(fixture)
        })
    }
}

#[tokio::test]
async fn object_safe_send_executor_runs_every_case_in_canonical_order() {
    let executor = MatchingExecutor::default();
    let dynamic: &dyn AssembledControlledUserAgentExecutorV1 = &executor;
    let future = dynamic.execute_case(&controlled_user_agent_fixtures_v1()[0]);
    assert_send(&future);
    drop(future);

    let report = run_controlled_user_agent_conformance_v1(dynamic)
        .await
        .expect("matching observations must pass");
    assert_eq!(report.suite_id(), CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_ID);
    assert_eq!(report.suite_version(), CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_VERSION);
    assert_eq!(
        *executor.order.lock().expect("order lock should be available"),
        controlled_user_agent_fixtures_v1()
            .iter()
            .map(ControlledUserAgentFixtureV1::case_id)
            .collect::<Vec<_>>()
    );
}

struct SingleMismatchExecutor {
    case_id: ControlledUserAgentCaseIdV1,
    category: ControlledUserAgentMismatchCategoryV1,
}

impl AssembledControlledUserAgentExecutorV1 for SingleMismatchExecutor {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ControlledUserAgentFixtureV1,
    ) -> AssembledControlledUserAgentExecutionFutureV1<'a> {
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
            ControlledUserAgentCaseIdV1::InvalidUserAgentValueRejected,
            ControlledUserAgentMismatchCategoryV1::OutcomeKind,
        ),
        (
            ControlledUserAgentCaseIdV1::InvalidUserAgentValueRejected,
            ControlledUserAgentMismatchCategoryV1::ErrorCode,
        ),
        (
            ControlledUserAgentCaseIdV1::ReservedHeaderDeclarationStillRejected,
            ControlledUserAgentMismatchCategoryV1::ErrorCode,
        ),
        (
            ControlledUserAgentCaseIdV1::BufferedUserAgentSuccess,
            ControlledUserAgentMismatchCategoryV1::Status,
        ),
        (
            ControlledUserAgentCaseIdV1::BufferedUserAgentSuccess,
            ControlledUserAgentMismatchCategoryV1::Body,
        ),
        (
            ControlledUserAgentCaseIdV1::BufferedUserAgentSuccess,
            ControlledUserAgentMismatchCategoryV1::ContentType,
        ),
        (
            ControlledUserAgentCaseIdV1::BufferedUserAgentSuccess,
            ControlledUserAgentMismatchCategoryV1::RetryAfter,
        ),
        (
            ControlledUserAgentCaseIdV1::StreamingUserAgentSuccess,
            ControlledUserAgentMismatchCategoryV1::ChunkBytes,
        ),
        (
            ControlledUserAgentCaseIdV1::BufferedUserAgentSuccess,
            ControlledUserAgentMismatchCategoryV1::ResolverCallCount,
        ),
        (
            ControlledUserAgentCaseIdV1::StreamingUserAgentSuccess,
            ControlledUserAgentMismatchCategoryV1::TransportCallCount,
        ),
        // Every polarity of the wire user-agent claim must isolate: a success case that failed to
        // put the declared value on the wire, a rejected case that reached a wire at all, and a
        // declaration-free case whose probe claimed a declared value it never had. The last is
        // the one that catches a probe hardcoding `true`, so its isolation is worth freezing
        // explicitly.
        (
            ControlledUserAgentCaseIdV1::BufferedUserAgentSuccess,
            ControlledUserAgentMismatchCategoryV1::WireUserAgent,
        ),
        (
            ControlledUserAgentCaseIdV1::InvalidUserAgentValueRejected,
            ControlledUserAgentMismatchCategoryV1::WireUserAgent,
        ),
        (
            ControlledUserAgentCaseIdV1::UserAgentFreeRequestReachesTheWire,
            ControlledUserAgentMismatchCategoryV1::WireUserAgent,
        ),
    ];

    for (case_id, category) in isolated_mismatches {
        let failure =
            run_controlled_user_agent_conformance_v1(&SingleMismatchExecutor { case_id, category })
                .await
                .expect_err("one deliberate difference must fail conformance");
        assert_eq!(failure.suite_id(), CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_ID);
        assert_eq!(failure.suite_version(), CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_VERSION);
        assert_eq!(failure.evaluated_case_count(), controlled_user_agent_fixtures_v1().len());
        assert!(failure.mismatches().len() <= MAX_CONTROLLED_USER_AGENT_MISMATCHES_V1);
        assert_eq!(failure.mismatches().len(), 1, "category {category:?} must isolate");
        let mismatch = &failure.mismatches()[0];
        assert_eq!(mismatch.case_id(), case_id);
        assert_eq!(mismatch.category(), category);
    }
}

/// The measured-probe blind spot, inherited from the controlled-query suite and frozen here from
/// day one.
///
/// This executor is the mutation a real host adapter shipped against the query suite: every
/// outcome is correct, and the wire probe reports `true` unconditionally without ever reading the
/// prepared request. It must fail, and it must fail on exactly the declaration-free case, because
/// that is the only case whose transport runs while the correct answer is `false`.
#[tokio::test]
async fn a_probe_that_hardcodes_the_wire_user_agent_claim_is_caught() {
    struct HardcodedWireUserAgentExecutor;

    impl AssembledControlledUserAgentExecutorV1 for HardcodedWireUserAgentExecutor {
        fn execute_case<'a>(
            &'a self,
            fixture: &'a ControlledUserAgentFixtureV1,
        ) -> AssembledControlledUserAgentExecutionFutureV1<'a> {
            Box::pin(async move {
                let expected = fixture.expected().evidence();
                let transport_calls = count_value(expected.transport_calls());
                // Correct counts, correct outcome. The probe stores `true` unconditionally
                // instead of reading the prepared request — but it is still only *invoked* from
                // the transport boundary, so a case that never reaches a transport keeps the
                // initial `false`.
                let evidence = ControlledUserAgentEvidenceV1::new(
                    count_value(expected.resolver_calls()),
                    transport_calls,
                    transport_calls > 0,
                );
                observation_with_evidence(fixture, evidence)
            })
        }
    }

    let failure = run_controlled_user_agent_conformance_v1(&HardcodedWireUserAgentExecutor)
        .await
        .expect_err("a probe that never reads the prepared request must not pass");

    assert_eq!(failure.mismatches().len(), 1);
    let mismatch = &failure.mismatches()[0];
    assert_eq!(mismatch.case_id(), ControlledUserAgentCaseIdV1::UserAgentFreeRequestReachesTheWire);
    assert_eq!(mismatch.category(), ControlledUserAgentMismatchCategoryV1::WireUserAgent);
}

#[tokio::test]
async fn a_fully_wrong_executor_reports_every_case_without_failing_fast() {
    struct WrongExecutor;

    impl AssembledControlledUserAgentExecutorV1 for WrongExecutor {
        fn execute_case<'a>(
            &'a self,
            fixture: &'a ControlledUserAgentFixtureV1,
        ) -> AssembledControlledUserAgentExecutionFutureV1<'a> {
            Box::pin(async move {
                let evidence = ControlledUserAgentEvidenceV1::new(257, 256, false);
                match fixture.case_id() {
                    ControlledUserAgentCaseIdV1::BufferedUserAgentSuccess => {
                        ControlledUserAgentObservationV1::failure(
                            ProviderCallFailureCodeV1::RequestFailed,
                            evidence,
                        )
                    }
                    _ => ControlledUserAgentObservationV1::opened(
                        head(StatusCode::OK, None, None),
                        Vec::new(),
                        evidence,
                    ),
                }
            })
        }
    }

    let failure = run_controlled_user_agent_conformance_v1(&WrongExecutor)
        .await
        .expect_err("deliberate mismatches must fail");
    assert_eq!(failure.evaluated_case_count(), controlled_user_agent_fixtures_v1().len());

    let categories: BTreeSet<_> = failure
        .mismatches()
        .iter()
        .map(south_testkit::ControlledUserAgentMismatchV1::category)
        .collect();
    assert!(categories.contains(&ControlledUserAgentMismatchCategoryV1::OutcomeKind));
    assert!(categories.contains(&ControlledUserAgentMismatchCategoryV1::ResolverCallCount));
    assert!(categories.contains(&ControlledUserAgentMismatchCategoryV1::TransportCallCount));
    assert!(categories.contains(&ControlledUserAgentMismatchCategoryV1::WireUserAgent));
    for fixture in controlled_user_agent_fixtures_v1() {
        assert!(failure.mismatches().iter().any(|m| m.case_id() == fixture.case_id()));
    }
}

#[test]
fn evidence_construction_saturates_large_boundary_counts() {
    let evidence = ControlledUserAgentEvidenceV1::new(256, 257, true);

    assert_eq!(evidence.resolver_calls(), ProviderCallCountV1::MoreThanOne);
    assert_eq!(evidence.transport_calls(), ProviderCallCountV1::MoreThanOne);
    assert!(evidence.wire_user_agent_exact());
}

#[test]
fn debug_output_contains_only_safe_structural_evidence() {
    const BODY_SENTINEL: &str = "controlled-user-agent-runner-body-debug-sentinel";
    const METADATA_SENTINEL: &str = "controlled-user-agent-runner-metadata-debug-sentinel";
    const CHUNK_SENTINEL: &str = "controlled-user-agent-runner-chunk-debug-sentinel";

    let evidence = ControlledUserAgentEvidenceV1::new(1, 1, true);
    let response = ControlledUserAgentObservationV1::response(
        BufferedHttpResponseV1::try_from_parts(
            StatusCode::OK,
            BODY_SENTINEL.as_bytes().to_vec(),
            Some(METADATA_SENTINEL.to_owned()),
            Some(METADATA_SENTINEL.to_owned()),
        )
        .expect("fixture response should be valid"),
        evidence,
    );
    let opened = ControlledUserAgentObservationV1::opened(
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

impl AssembledControlledUserAgentExecutorV1 for PendingExecutor {
    fn execute_case<'a>(
        &'a self,
        _fixture: &'a ControlledUserAgentFixtureV1,
    ) -> AssembledControlledUserAgentExecutionFutureV1<'a> {
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
    let structured_run = async { run_controlled_user_agent_conformance_v1(&executor).await };

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

const fn matching_evidence(
    fixture: &ControlledUserAgentFixtureV1,
) -> ControlledUserAgentEvidenceV1 {
    let evidence = fixture.expected().evidence();
    ControlledUserAgentEvidenceV1::new(
        count_value(evidence.resolver_calls()),
        count_value(evidence.transport_calls()),
        evidence.wire_user_agent_exact(),
    )
}

fn observation_matching(
    fixture: &ControlledUserAgentFixtureV1,
) -> ControlledUserAgentObservationV1 {
    observation_with_evidence(fixture, matching_evidence(fixture))
}

fn observation_with_evidence(
    fixture: &ControlledUserAgentFixtureV1,
    evidence: ControlledUserAgentEvidenceV1,
) -> ControlledUserAgentObservationV1 {
    match fixture.expected().outcome() {
        ControlledUserAgentExpectedOutcomeV1::Response {
            status,
            body,
            content_type,
            retry_after,
        } => ControlledUserAgentObservationV1::response(
            response(*status, body, *content_type, *retry_after),
            evidence,
        ),
        ControlledUserAgentExpectedOutcomeV1::Opened {
            status,
            content_type,
            retry_after,
            chunks,
        } => ControlledUserAgentObservationV1::opened(
            head(
                StatusCode::from_u16(*status).expect("expected status should be valid"),
                *content_type,
                *retry_after,
            ),
            expected_chunks(chunks),
            evidence,
        ),
        ControlledUserAgentExpectedOutcomeV1::Failure { code } => {
            ControlledUserAgentObservationV1::failure(*code, evidence)
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
    fixture: &ControlledUserAgentFixtureV1,
    category: ControlledUserAgentMismatchCategoryV1,
) -> ControlledUserAgentObservationV1 {
    let expected_evidence = fixture.expected().evidence();
    let mut resolver_calls = count_value(expected_evidence.resolver_calls());
    let mut transport_calls = count_value(expected_evidence.transport_calls());
    let mut wire_user_agent_exact = expected_evidence.wire_user_agent_exact();
    match category {
        ControlledUserAgentMismatchCategoryV1::ResolverCallCount => {
            resolver_calls = different_count(resolver_calls);
        }
        ControlledUserAgentMismatchCategoryV1::TransportCallCount => {
            transport_calls = different_count(transport_calls);
        }
        ControlledUserAgentMismatchCategoryV1::WireUserAgent => {
            wire_user_agent_exact = !wire_user_agent_exact;
        }
        _ => {}
    }
    let evidence =
        ControlledUserAgentEvidenceV1::new(resolver_calls, transport_calls, wire_user_agent_exact);

    match fixture.expected().outcome() {
        ControlledUserAgentExpectedOutcomeV1::Response {
            status,
            body,
            content_type,
            retry_after,
        } => {
            let observed_status = observed_status(*status, category);
            let observed_body = if category == ControlledUserAgentMismatchCategoryV1::Body {
                "isolated-wrong-body"
            } else {
                body
            };
            let observed_content_type =
                if category == ControlledUserAgentMismatchCategoryV1::ContentType {
                    None
                } else {
                    *content_type
                };
            let observed_retry_after =
                if category == ControlledUserAgentMismatchCategoryV1::RetryAfter {
                    None
                } else {
                    *retry_after
                };
            ControlledUserAgentObservationV1::response(
                response(
                    observed_status,
                    observed_body,
                    observed_content_type,
                    observed_retry_after,
                ),
                evidence,
            )
        }
        ControlledUserAgentExpectedOutcomeV1::Opened {
            status,
            content_type,
            retry_after,
            chunks,
        } => {
            let observed_status = observed_status(*status, category);
            let mut observed_chunks = expected_chunks(chunks);
            if category == ControlledUserAgentMismatchCategoryV1::ChunkBytes {
                observed_chunks[0] =
                    StreamChunkV1::try_new(Bytes::from_static(b"isolated-wrong-chunk"))
                        .expect("isolated chunk should satisfy bounds");
            }
            ControlledUserAgentObservationV1::opened(
                head(
                    StatusCode::from_u16(observed_status).expect("shifted status should be valid"),
                    *content_type,
                    *retry_after,
                ),
                observed_chunks,
                evidence,
            )
        }
        ControlledUserAgentExpectedOutcomeV1::Failure { code } => {
            if category == ControlledUserAgentMismatchCategoryV1::OutcomeKind {
                ControlledUserAgentObservationV1::opened(
                    head(StatusCode::OK, None, None),
                    Vec::new(),
                    evidence,
                )
            } else {
                let observed_code = if category == ControlledUserAgentMismatchCategoryV1::ErrorCode
                {
                    // Both negative rows expect a different frozen code, so shifting to the other
                    // one is guaranteed to differ regardless of which row this is.
                    if *code == ProviderCallFailureCodeV1::RequestFailed {
                        ProviderCallFailureCodeV1::InvalidRelativePath
                    } else {
                        ProviderCallFailureCodeV1::RequestFailed
                    }
                } else {
                    *code
                };
                ControlledUserAgentObservationV1::failure(observed_code, evidence)
            }
        }
    }
}

const fn observed_status(expected: u16, category: ControlledUserAgentMismatchCategoryV1) -> u16 {
    if matches!(category, ControlledUserAgentMismatchCategoryV1::Status) {
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
