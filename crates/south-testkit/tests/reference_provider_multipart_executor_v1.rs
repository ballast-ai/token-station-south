use std::time::Duration;

use south_contracts::{ProviderAuthV1, SecretHeaderV1};
use south_provider_conformance::{
    ProviderMultipartAuthArmV1, ProviderMultipartCaseIdV1, ProviderMultipartExpectedOutcomeV1,
    provider_multipart_fixtures_v1,
};
use south_testkit::{
    AssembledProviderMultipartExecutorV1, ProviderMultipartObservationV1,
    ReferenceAssembledProviderMultipartExecutorV1, parse_reference_multipart_input,
    run_provider_multipart_conformance_v1,
};
use static_assertions::assert_impl_all;

assert_impl_all!(ReferenceAssembledProviderMultipartExecutorV1: Send, Sync);

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_executor_uses_real_core_for_every_case() {
    let executor = ReferenceAssembledProviderMultipartExecutorV1::new();
    let dynamic: &dyn AssembledProviderMultipartExecutorV1 = &executor;
    let structured_run = async {
        for fixture in provider_multipart_fixtures_v1() {
            let observation = dynamic.execute_case(fixture).await;
            assert_observation_matches(fixture, &observation);
        }
    };

    tokio::time::timeout(Duration::from_secs(5), structured_run)
        .await
        .expect("reference watchdog expired");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_assembled_multipart_conforms_under_a_structured_watchdog() {
    let executor = ReferenceAssembledProviderMultipartExecutorV1::new();

    let report = tokio::time::timeout(
        Duration::from_secs(5),
        run_provider_multipart_conformance_v1(&executor),
    )
    .await
    .expect("multipart conformance watchdog expired")
    .expect("reference executor must conform");

    assert_eq!(report.passed_case_ids().len(), provider_multipart_fixtures_v1().len());
    assert_eq!(report.passed_case_ids().len(), 5);
}

/// The public parse a host executor may reuse. Three cases build a request; two fail here, and
/// failing *here* rather than at a boundary is the property the suite is about.
#[test]
fn reference_parse_builds_three_requests_and_refuses_the_two_inconsistent_ones() {
    for fixture in provider_multipart_fixtures_v1() {
        let parsed = parse_reference_multipart_input(fixture.input(), fixture.auth_arm());
        match fixture.case_id() {
            ProviderMultipartCaseIdV1::MultipartBodyBoundaryMismatch
            | ProviderMultipartCaseIdV1::MultipartContentTypeSmuggled => {
                assert!(
                    parsed.is_err(),
                    "{:?} must be refused while the request is still a value",
                    fixture.case_id()
                );
            }
            _ => {
                let (_binding, request) =
                    parsed.expect("every consistent canonical input must parse");
                assert_eq!(request.relative_path().as_str(), fixture.input().relative_path());
                assert_eq!(
                    request.auth().credential_slot().as_str(),
                    fixture.input().requested_credential_slot()
                );
                // The rendered media type comes from the boundary, and the ordinary headers
                // never carry one.
                assert_eq!(request.body().content_type(), fixture.input().expected_content_type());
                assert_eq!(request.headers().get("content-type"), None);
                // The bytes survive the parse unmodified — South re-encodes nothing.
                assert_eq!(request.body().as_bytes(), fixture.input().body());
                match fixture.auth_arm() {
                    ProviderMultipartAuthArmV1::Bearer => {
                        assert!(matches!(request.auth(), ProviderAuthV1::Bearer(_)));
                    }
                    ProviderMultipartAuthArmV1::HeaderSecret(expected) => {
                        let ProviderAuthV1::HeaderSecret { header, .. } = request.auth() else {
                            panic!("the header-secret arm did not survive the parse");
                        };
                        assert_eq!(*header, expected);
                        assert_eq!(expected, SecretHeaderV1::ApiKey);
                    }
                }
            }
        }
    }
}

fn assert_observation_matches(
    fixture: &south_provider_conformance::ProviderMultipartFixtureV1,
    observation: &ProviderMultipartObservationV1,
) {
    match fixture.expected().outcome() {
        ProviderMultipartExpectedOutcomeV1::Response {
            status,
            body,
            content_type,
            retry_after,
        } => {
            let response =
                observation.response_value().expect("expected a buffered response observation");
            assert_eq!(response.status().as_u16(), *status);
            assert_eq!(response.body(), *body);
            assert_eq!(response.content_type(), *content_type);
            assert_eq!(response.retry_after(), *retry_after);
        }
        ProviderMultipartExpectedOutcomeV1::Failure { code } => {
            assert_eq!(observation.failure_code(), Some(*code));
        }
    }
    let expected = fixture.expected().evidence();
    let observed = observation.evidence();
    let case = fixture.case_id();
    assert_eq!(observed.resolver_calls(), expected.resolver_calls(), "case {case:?}");
    assert_eq!(observed.transport_calls(), expected.transport_calls(), "case {case:?}");
    assert_eq!(
        observed.wire_content_type_exact(),
        expected.wire_content_type_exact(),
        "case {case:?}"
    );
    assert_eq!(observed.wire_body_bytes_exact(), expected.wire_body_bytes_exact(), "case {case:?}");
}
