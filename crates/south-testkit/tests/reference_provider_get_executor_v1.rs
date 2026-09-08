use std::time::Duration;

use south_contracts::{ProviderAuthV1, SecretHeaderV1};
use south_provider_conformance::{
    ProviderGetAuthArmV1, ProviderGetExpectedOutcomeV1, provider_get_fixtures_v1,
};
use south_testkit::{
    AssembledProviderGetExecutorV1, ProviderGetObservationV1,
    ReferenceAssembledProviderGetExecutorV1, parse_reference_get_input,
    run_provider_get_conformance_v1,
};
use static_assertions::assert_impl_all;

assert_impl_all!(ReferenceAssembledProviderGetExecutorV1: Send, Sync);

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_executor_uses_real_core_for_every_case() {
    let executor = ReferenceAssembledProviderGetExecutorV1::new();
    let dynamic: &dyn AssembledProviderGetExecutorV1 = &executor;
    let structured_run = async {
        for fixture in provider_get_fixtures_v1() {
            let observation = dynamic.execute_case(fixture).await;
            assert_observation_matches(fixture, &observation);
        }
    };

    tokio::time::timeout(Duration::from_secs(5), structured_run)
        .await
        .expect("reference watchdog expired");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_assembled_provider_get_conforms_under_a_structured_watchdog() {
    let executor = ReferenceAssembledProviderGetExecutorV1::new();

    let report =
        tokio::time::timeout(Duration::from_secs(5), run_provider_get_conformance_v1(&executor))
            .await
            .expect("buffered-GET conformance watchdog expired")
            .expect("reference executor must conform");

    assert_eq!(report.passed_case_ids().len(), provider_get_fixtures_v1().len());
    assert_eq!(report.passed_case_ids().len(), 4);
}

/// The public parse a host executor may reuse: it produces a `GetRequestV1` (no body slot) with
/// the fixture's arm attached, and never a failure for the frozen table's inputs.
#[test]
fn reference_parse_attaches_the_fixture_arm_to_a_body_less_request() {
    for fixture in provider_get_fixtures_v1() {
        let (binding, request) = parse_reference_get_input(fixture.input(), fixture.auth_arm())
            .expect("every canonical GET input parses");
        assert_eq!(request.relative_path().as_str(), fixture.input().relative_path());
        assert_eq!(
            request.auth().credential_slot().as_str(),
            fixture.input().requested_credential_slot()
        );
        match fixture.auth_arm() {
            ProviderGetAuthArmV1::Bearer => {
                assert!(matches!(request.auth(), ProviderAuthV1::Bearer(_)));
            }
            ProviderGetAuthArmV1::HeaderSecret(expected) => {
                let ProviderAuthV1::HeaderSecret { header, .. } = request.auth() else {
                    panic!("the header-secret arm did not survive the parse");
                };
                assert_eq!(*header, expected);
                assert_eq!(expected, SecretHeaderV1::XApiKey);
            }
        }
        assert!(request.query().is_none(), "the parse attaches no query; the executor does");
        let _ = binding;
    }
}

fn assert_observation_matches(
    fixture: &south_provider_conformance::ProviderGetFixtureV1,
    observation: &ProviderGetObservationV1,
) {
    match fixture.expected().outcome() {
        ProviderGetExpectedOutcomeV1::Response { status, body, content_type, retry_after } => {
            let response =
                observation.response_value().expect("expected a buffered response observation");
            assert_eq!(response.status().as_u16(), *status);
            assert_eq!(response.body(), *body);
            assert_eq!(response.content_type(), *content_type);
            assert_eq!(response.retry_after(), *retry_after);
        }
        ProviderGetExpectedOutcomeV1::Failure { code } => {
            assert_eq!(observation.failure_code(), Some(*code));
        }
    }
    let expected = fixture.expected().evidence();
    let observed = observation.evidence();
    let case = fixture.case_id();
    assert_eq!(observed.resolver_calls(), expected.resolver_calls(), "case {case:?}");
    assert_eq!(observed.transport_calls(), expected.transport_calls(), "case {case:?}");
    assert_eq!(observed.wire_method_get(), expected.wire_method_get(), "case {case:?}");
    assert_eq!(observed.wire_body_absent(), expected.wire_body_absent(), "case {case:?}");
    assert_eq!(observed.wire_query_exact(), expected.wire_query_exact(), "case {case:?}");
}
