use std::time::Duration;

use south_provider_conformance::{HeaderAuthExpectedOutcomeV1, header_auth_fixtures_v1};
use south_testkit::{
    AssembledHeaderAuthExecutorV1, HeaderAuthObservationV1, ReferenceAssembledHeaderAuthExecutorV1,
    run_header_auth_conformance_v1,
};
use static_assertions::assert_impl_all;

assert_impl_all!(ReferenceAssembledHeaderAuthExecutorV1: Send, Sync);

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_executor_uses_real_core_for_every_case() {
    let executor = ReferenceAssembledHeaderAuthExecutorV1::new();
    let dynamic: &dyn AssembledHeaderAuthExecutorV1 = &executor;
    let structured_run = async {
        for fixture in header_auth_fixtures_v1() {
            let observation = dynamic.execute_case(fixture).await;
            assert_observation_matches(fixture, &observation);
        }
    };

    tokio::time::timeout(Duration::from_secs(5), structured_run)
        .await
        .expect("reference watchdog expired");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_assembled_header_auth_conforms_under_a_structured_watchdog() {
    let executor = ReferenceAssembledHeaderAuthExecutorV1::new();

    let report =
        tokio::time::timeout(Duration::from_secs(5), run_header_auth_conformance_v1(&executor))
            .await
            .expect("header-auth conformance watchdog expired")
            .expect("reference executor must conform");

    assert_eq!(report.passed_case_ids().len(), 3);
}

fn assert_observation_matches(
    fixture: &south_provider_conformance::HeaderAuthFixtureV1,
    observation: &HeaderAuthObservationV1,
) {
    match fixture.expected().outcome() {
        HeaderAuthExpectedOutcomeV1::Response { status, body, content_type, retry_after } => {
            let response =
                observation.response_value().expect("expected a buffered response observation");
            assert_eq!(response.status().as_u16(), *status);
            assert_eq!(response.body(), *body);
            assert_eq!(response.content_type(), *content_type);
            assert_eq!(response.retry_after(), *retry_after);
        }
        HeaderAuthExpectedOutcomeV1::Opened { status, content_type, retry_after, chunks } => {
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
        HeaderAuthExpectedOutcomeV1::Failure { code } => {
            assert_eq!(observation.failure_code(), Some(*code));
        }
    }
    let expected = fixture.expected().evidence();
    let observed = observation.evidence();
    assert_eq!(observed.resolver_calls(), expected.resolver_calls());
    assert_eq!(observed.transport_calls(), expected.transport_calls());
    assert_eq!(
        observed.sanctioned_header_exact(),
        expected.sanctioned_header_exact(),
        "case {:?}",
        fixture.case_id()
    );
    assert_eq!(
        observed.authorization_header_absent(),
        expected.authorization_header_absent(),
        "case {:?}",
        fixture.case_id()
    );
}
