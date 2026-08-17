use std::time::Duration;

use south_provider_conformance::{
    PROVIDER_STREAM_CONFORMANCE_DEADLINE_OFFSET_V1, PROVIDER_STREAM_CONFORMANCE_IDLE_TIMEOUT_V1,
    ProviderStreamControlV1, ProviderStreamExpectedOutcomeV1, provider_stream_fixtures_v1,
};
use south_testkit::{
    AssembledProviderStreamExecutorV1, ProviderStreamObservationV1,
    ReferenceAssembledProviderStreamExecutorV1, run_provider_stream_conformance_v1,
};
use static_assertions::assert_impl_all;

assert_impl_all!(ReferenceAssembledProviderStreamExecutorV1: Send, Sync);

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_executor_uses_real_core_for_every_non_clock_case() {
    let executor = ReferenceAssembledProviderStreamExecutorV1::new();
    let dynamic: &dyn AssembledProviderStreamExecutorV1 = &executor;
    let structured_run = async {
        for fixture in provider_stream_fixtures_v1() {
            if matches!(
                fixture.control(),
                ProviderStreamControlV1::AdvanceIdleWhileChunkPending
                    | ProviderStreamControlV1::ExpireWhileChunkPending
            ) {
                continue;
            }
            let observation = dynamic.execute_case(fixture).await;
            assert_observation_matches(fixture, &observation);
        }
    };

    tokio::time::timeout(Duration::from_secs(5), structured_run)
        .await
        .expect("non-clock reference watchdog expired");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_assembled_provider_stream_conforms_under_a_structured_watchdog() {
    let executor = ReferenceAssembledProviderStreamExecutorV1::new();
    let clock_driver = async {
        executor.idle_stall_started().await;
        tokio::time::advance(PROVIDER_STREAM_CONFORMANCE_IDLE_TIMEOUT_V1).await;
        executor.deadline_chunk_started().await;
        tokio::time::advance(PROVIDER_STREAM_CONFORMANCE_DEADLINE_OFFSET_V1).await;
    };
    let structured_run = async {
        let (result, ()) =
            tokio::join!(run_provider_stream_conformance_v1(&executor), clock_driver);
        result
    };

    let report = tokio::time::timeout(Duration::from_secs(5), structured_run)
        .await
        .expect("stream conformance watchdog expired")
        .expect("reference executor must conform");

    assert_eq!(report.passed_case_ids().len(), 9);
}

fn assert_observation_matches(
    fixture: &south_provider_conformance::ProviderStreamFixtureV1,
    observation: &ProviderStreamObservationV1,
) {
    match fixture.expected().outcome() {
        ProviderStreamExpectedOutcomeV1::Opened { status, content_type, retry_after, chunks } => {
            let head = observation.opened_head().expect("expected an opened stream observation");
            assert_eq!(head.status().as_u16(), *status);
            assert_eq!(head.content_type(), *content_type);
            assert_eq!(head.retry_after(), *retry_after);
            let observed_chunks =
                observation.opened_chunks().expect("expected opened stream chunks");
            assert_eq!(observed_chunks.len(), chunks.len());
            for (observed, expected) in observed_chunks.iter().zip(*chunks) {
                assert_eq!(observed.as_bytes(), *expected);
            }
        }
        ProviderStreamExpectedOutcomeV1::Rejected { status, content_type, retry_after, body } => {
            let rejected = observation.rejected_value().expect("expected a rejected observation");
            assert_eq!(rejected.head().status().as_u16(), *status);
            assert_eq!(rejected.head().content_type(), *content_type);
            assert_eq!(rejected.head().retry_after(), *retry_after);
            assert_eq!(rejected.body(), *body);
        }
        ProviderStreamExpectedOutcomeV1::Failure { code } => {
            assert_eq!(observation.failure_code(), Some(*code));
        }
    }
    let expected = fixture.expected().evidence();
    let observed = observation.evidence();
    assert_eq!(observed.resolver_calls(), expected.resolver_calls());
    assert_eq!(observed.transport_calls(), expected.transport_calls());
    assert_eq!(
        observed.resolver_future_dropped_while_pending(),
        expected.resolver_future_dropped_while_pending()
    );
    assert_eq!(
        observed.transport_future_dropped_while_pending(),
        expected.transport_future_dropped_while_pending(),
        "case {:?}",
        fixture.case_id()
    );
    assert_eq!(observed.chunks_pulled(), expected.chunks_pulled());
    assert_eq!(observed.poststream_error_code(), expected.poststream_error_code());
}
