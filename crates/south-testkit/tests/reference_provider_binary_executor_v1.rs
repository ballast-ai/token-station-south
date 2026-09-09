use std::time::Duration;

use south_contracts::ProviderAuthV1;
use south_provider_conformance::{
    ProviderBinaryEntryArmV1, ProviderBinaryExpectedOutcomeV1, ProviderBinaryFixtureV1,
    provider_binary_fixtures_v1,
};
use south_testkit::{
    AssembledProviderBinaryExecutorV1, ProviderBinaryObservationV1,
    ReferenceAssembledProviderBinaryExecutorV1, parse_reference_binary_input,
    run_provider_binary_conformance_v1,
};
use static_assertions::assert_impl_all;

assert_impl_all!(ReferenceAssembledProviderBinaryExecutorV1: Send, Sync);

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_executor_uses_real_core_for_every_case() {
    let executor = ReferenceAssembledProviderBinaryExecutorV1::new();
    let dynamic: &dyn AssembledProviderBinaryExecutorV1 = &executor;
    let structured_run = async {
        for fixture in provider_binary_fixtures_v1() {
            let observation = dynamic.execute_case(fixture).await;
            assert_observation_matches(fixture, &observation);
        }
    };

    tokio::time::timeout(Duration::from_secs(5), structured_run)
        .await
        .expect("reference watchdog expired");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_assembled_binary_conforms_under_a_structured_watchdog() {
    let executor = ReferenceAssembledProviderBinaryExecutorV1::new();

    let report =
        tokio::time::timeout(Duration::from_secs(5), run_provider_binary_conformance_v1(&executor))
            .await
            .expect("binary conformance watchdog expired")
            .expect("reference executor must conform");

    assert_eq!(report.passed_case_ids().len(), provider_binary_fixtures_v1().len());
    assert_eq!(report.passed_case_ids().len(), 6);
}

/// The public parse a host executor may reuse. Every canonical input builds: unlike the multipart
/// suite, no row of this table is refused while the request is still a value, because the request
/// side of a binary call is an ordinary JSON POST.
#[test]
fn reference_parse_builds_a_request_for_every_canonical_input() {
    for fixture in provider_binary_fixtures_v1() {
        let (_binding, request) = parse_reference_binary_input(fixture.input())
            .expect("every canonical binary input must parse");
        assert_eq!(request.relative_path().as_str(), fixture.input().relative_path());
        assert_eq!(
            request.auth().credential_slot().as_str(),
            fixture.input().requested_credential_slot()
        );
        assert_eq!(request.body().as_str(), fixture.input().json_body());
        assert!(matches!(request.auth(), ProviderAuthV1::Bearer(_)));
    }
}

/// The heart of the slice, asserted directly rather than only through the runner: the same bytes
/// succeed on the binary arm and are refused on the UTF-8 arm.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_same_payload_succeeds_on_one_arm_and_is_refused_on_the_other() {
    let executor = ReferenceAssembledProviderBinaryExecutorV1::new();
    let fixtures = provider_binary_fixtures_v1();
    let binary_row = &fixtures[0];
    let utf8_row = &fixtures[5];
    assert_eq!(binary_row.entry_arm(), ProviderBinaryEntryArmV1::Binary);
    assert_eq!(utf8_row.entry_arm(), ProviderBinaryEntryArmV1::Utf8);

    let structured_run = async {
        let binary = executor.execute_case(binary_row).await;
        let utf8 = executor.execute_case(utf8_row).await;

        let response = binary.response_value().expect("the binary arm must return the bytes");
        assert!(std::str::from_utf8(response.body()).is_err());
        assert_eq!(response.body_len(), response.body().len());
        assert!(binary.evidence().wire_binary_response_observed());
        assert!(binary.evidence().wire_body_bytes_exact());

        assert!(utf8.failure_code().is_some(), "the UTF-8 arm must still refuse these bytes");
        // The seam claim stays false on a row that did reach a transport, which is the property
        // that makes a hardcoded probe fail this suite.
        assert!(!utf8.evidence().wire_binary_response_observed());
        assert!(!utf8.evidence().wire_body_bytes_exact());
    };

    tokio::time::timeout(Duration::from_secs(5), structured_run).await.expect("watchdog expired");
}

fn assert_observation_matches(
    fixture: &ProviderBinaryFixtureV1,
    observation: &ProviderBinaryObservationV1,
) {
    let case = fixture.case_id();
    match fixture.expected().outcome() {
        ProviderBinaryExpectedOutcomeV1::Response { status, body, content_type, retry_after } => {
            let response =
                observation.response_value().expect("expected a binary response observation");
            assert_eq!(response.status().as_u16(), *status, "case {case:?}");
            assert!(body.matches(response.body()), "case {case:?}");
            assert_eq!(response.content_type(), *content_type, "case {case:?}");
            assert_eq!(response.retry_after(), *retry_after, "case {case:?}");
        }
        ProviderBinaryExpectedOutcomeV1::Failure { code } => {
            assert_eq!(observation.failure_code(), Some(*code), "case {case:?}");
        }
    }
    let expected = fixture.expected().evidence();
    let observed = observation.evidence();
    assert_eq!(observed.resolver_calls(), expected.resolver_calls(), "case {case:?}");
    assert_eq!(observed.transport_calls(), expected.transport_calls(), "case {case:?}");
    assert_eq!(
        observed.wire_binary_response_observed(),
        expected.wire_binary_response_observed(),
        "case {case:?}"
    );
    assert_eq!(observed.wire_body_bytes_exact(), expected.wire_body_bytes_exact(), "case {case:?}");
}
