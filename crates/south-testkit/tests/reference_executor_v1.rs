use std::time::Duration;

use south_provider_conformance::{
    PROVIDER_CALL_CONFORMANCE_DEADLINE_OFFSET_V1, ProviderCallCaseIdV1,
    ProviderCallExpectedOutcomeV1, provider_call_fixtures_v1,
};
use south_testkit::{
    AssembledProviderCallExecutorV1, ProviderCallObservationV1,
    ReferenceAssembledProviderCallExecutorV1, run_provider_call_conformance_v1,
};
use static_assertions::assert_impl_all;

assert_impl_all!(ReferenceAssembledProviderCallExecutorV1: Send, Sync);

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_executor_uses_real_core_for_every_non_deadline_case() {
    let executor = ReferenceAssembledProviderCallExecutorV1::new();
    let dynamic: &dyn AssembledProviderCallExecutorV1 = &executor;
    let structured_run = async {
        for fixture in &provider_call_fixtures_v1()[..6] {
            let observation = dynamic.execute_case(fixture).await;
            assert_observation_matches(fixture, &observation);
        }
    };

    tokio::time::timeout(Duration::from_secs(5), structured_run)
        .await
        .expect("non-deadline reference watchdog expired");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn deadline_waits_for_transport_start_then_drops_the_pending_future() {
    let executor = ReferenceAssembledProviderCallExecutorV1::new();
    let fixture = provider_call_fixtures_v1()
        .iter()
        .find(|fixture| fixture.case_id() == ProviderCallCaseIdV1::DeadlineExceeded)
        .expect("deadline fixture must exist");

    let structured_run = async {
        let execute = executor.execute_case(fixture);
        let deadline_driver = async {
            executor.deadline_transport_started().await;
            tokio::time::advance(PROVIDER_CALL_CONFORMANCE_DEADLINE_OFFSET_V1).await;
        };
        let (observation, ()) = tokio::join!(execute, deadline_driver);
        observation
    };
    let observation = tokio::time::timeout(Duration::from_secs(5), structured_run)
        .await
        .expect("deadline reference watchdog expired");

    assert_observation_matches(fixture, &observation);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn one_executor_consumes_a_fresh_transport_start_for_each_sequential_deadline_case() {
    let executor = ReferenceAssembledProviderCallExecutorV1::new();
    let fixture = provider_call_fixtures_v1()
        .iter()
        .find(|fixture| fixture.case_id() == ProviderCallCaseIdV1::DeadlineExceeded)
        .expect("deadline fixture must exist");

    run_deadline_case_with_watchdog(&executor, fixture, false).await;
    assert!(
        tokio::time::timeout(Duration::ZERO, executor.deadline_transport_started()).await.is_err(),
        "the first transport-start notification must be consumed exactly once"
    );
    run_deadline_case_with_watchdog(&executor, fixture, true).await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_assembled_provider_call_conforms_under_a_structured_watchdog() {
    let executor = ReferenceAssembledProviderCallExecutorV1::new();
    let deadline_driver = async {
        executor.deadline_transport_started().await;
        tokio::time::advance(PROVIDER_CALL_CONFORMANCE_DEADLINE_OFFSET_V1).await;
    };
    let structured_run = async {
        let (result, ()) =
            tokio::join!(run_provider_call_conformance_v1(&executor), deadline_driver,);
        result
    };

    let report = tokio::time::timeout(Duration::from_secs(5), structured_run)
        .await
        .expect("conformance watchdog expired")
        .expect("reference executor must conform");

    assert_eq!(report.passed_case_ids().len(), 7);
}

fn assert_observation_matches(
    fixture: &south_provider_conformance::ProviderCallFixtureV1,
    observation: &ProviderCallObservationV1,
) {
    match fixture.expected().outcome() {
        ProviderCallExpectedOutcomeV1::Response { status, body, content_type, retry_after } => {
            let response = observation.response_value().expect("expected response observation");
            assert_eq!(response.status().as_u16(), *status);
            assert_eq!(response.body(), *body);
            assert_eq!(response.content_type(), *content_type);
            assert_eq!(response.retry_after(), *retry_after);
            assert_eq!(observation.failure_code(), None);
        }
        ProviderCallExpectedOutcomeV1::Failure { code } => {
            assert_eq!(observation.failure_code(), Some(*code));
            assert!(observation.response_value().is_none());
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
        expected.transport_future_dropped_while_pending()
    );
}

async fn run_deadline_case_with_watchdog(
    executor: &ReferenceAssembledProviderCallExecutorV1,
    fixture: &south_provider_conformance::ProviderCallFixtureV1,
    poll_driver_first: bool,
) {
    let structured_run = async {
        let execute = executor.execute_case(fixture);
        let deadline_driver = async {
            executor.deadline_transport_started().await;
            tokio::time::advance(PROVIDER_CALL_CONFORMANCE_DEADLINE_OFFSET_V1).await;
        };
        if poll_driver_first {
            let ((), observation) = tokio::join!(deadline_driver, execute);
            observation
        } else {
            let (observation, ()) = tokio::join!(execute, deadline_driver);
            observation
        }
    };
    let observation = tokio::time::timeout(Duration::from_secs(5), structured_run)
        .await
        .expect("sequential deadline watchdog expired");
    assert_observation_matches(fixture, &observation);
}
