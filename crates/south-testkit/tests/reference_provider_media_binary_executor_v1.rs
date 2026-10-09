use std::time::Duration;

use south_contracts::{ProviderAuthV1, SecretHeaderV1, TextMediaTypeV1};
use south_provider_conformance::{
    ProviderCallFailureCodeV1, ProviderMediaBinaryAuthArmV1, ProviderMediaBinaryCaseIdV1,
    ProviderMediaBinaryExpectedOutcomeV1, ProviderMediaBinaryFixtureV1,
    provider_media_binary_fixtures_v1,
};
use south_testkit::{
    AssembledProviderMediaBinaryExecutorV1, ProviderMediaBinaryObservationV1,
    ProviderMediaBinaryRequestV1, ReferenceAssembledProviderMediaBinaryExecutorV1,
    parse_reference_media_binary_input, run_provider_media_binary_conformance_v1,
};
use static_assertions::assert_impl_all;

assert_impl_all!(ReferenceAssembledProviderMediaBinaryExecutorV1: Send, Sync);

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_executor_uses_real_core_for_every_case() {
    let executor = ReferenceAssembledProviderMediaBinaryExecutorV1::new();
    let dynamic: &dyn AssembledProviderMediaBinaryExecutorV1 = &executor;
    let structured_run = async {
        for fixture in provider_media_binary_fixtures_v1() {
            let observation = dynamic.execute_case(fixture).await;
            assert_observation_matches(fixture, &observation);
        }
    };

    tokio::time::timeout(Duration::from_secs(5), structured_run)
        .await
        .expect("reference watchdog expired");
}

/// The reference executor passes its own suite, all six rows.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_assembled_media_binary_conforms_under_a_structured_watchdog() {
    let executor = ReferenceAssembledProviderMediaBinaryExecutorV1::new();

    let report = tokio::time::timeout(
        Duration::from_secs(5),
        run_provider_media_binary_conformance_v1(&executor),
    )
    .await
    .expect("media-binary conformance watchdog expired")
    .expect("reference executor must conform");

    assert_eq!(report.passed_case_ids().len(), provider_media_binary_fixtures_v1().len());
    assert_eq!(report.passed_case_ids().len(), 6);
}

/// The public parse a host executor may reuse. Five cases build a request of the shape their
/// body names; the smuggling case fails here, while the request is still a value.
#[test]
fn reference_parse_builds_five_requests_and_refuses_the_smuggled_content_type() {
    for fixture in provider_media_binary_fixtures_v1() {
        let parsed = parse_reference_media_binary_input(fixture.input(), fixture.auth_arm());
        if fixture.case_id() == ProviderMediaBinaryCaseIdV1::TextContentTypeSmuggled {
            assert_eq!(parsed.err(), Some(ProviderCallFailureCodeV1::InvalidRelativePath));
            continue;
        }
        let (_binding, request) = parsed.expect("every consistent canonical input must parse");
        let auth = match &request {
            ProviderMediaBinaryRequestV1::Multipart(request) => {
                assert_eq!(request.body().content_type(), fixture.input().expected_content_type());
                assert_eq!(request.body().as_bytes(), fixture.input().body().bytes());
                assert_eq!(request.headers().get("content-type"), None);
                request.auth()
            }
            ProviderMediaBinaryRequestV1::Text(request) => {
                assert_eq!(request.body().media_type(), TextMediaTypeV1::Ssml);
                assert_eq!(request.body().content_type(), fixture.input().expected_content_type());
                assert_eq!(request.body().as_str().as_bytes(), fixture.input().body().bytes());
                assert_eq!(request.headers().get("content-type"), None);
                request.auth()
            }
        };
        assert_eq!(auth.credential_slot().as_str(), fixture.input().requested_credential_slot());
        match fixture.auth_arm() {
            ProviderMediaBinaryAuthArmV1::Bearer => {
                assert!(matches!(auth, ProviderAuthV1::Bearer(_)));
            }
            ProviderMediaBinaryAuthArmV1::HeaderSecret(expected) => {
                let ProviderAuthV1::HeaderSecret { header, .. } = auth else {
                    panic!("the header-secret arm did not survive the parse");
                };
                assert_eq!(*header, expected);
                assert!(matches!(
                    expected,
                    SecretHeaderV1::ApiKey | SecretHeaderV1::OcpApimSubscriptionKey
                ));
            }
        }
    }
}

fn assert_observation_matches(
    fixture: &ProviderMediaBinaryFixtureV1,
    observation: &ProviderMediaBinaryObservationV1,
) {
    match fixture.expected().outcome() {
        ProviderMediaBinaryExpectedOutcomeV1::Response {
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
        ProviderMediaBinaryExpectedOutcomeV1::Failure { code } => {
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
    assert_eq!(
        observed.wire_request_body_exact(),
        expected.wire_request_body_exact(),
        "case {case:?}"
    );
    assert_eq!(
        observed.wire_binary_response_observed(),
        expected.wire_binary_response_observed(),
        "case {case:?}"
    );
}
