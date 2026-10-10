use std::time::Duration;

use south_contracts::{ProviderAuthV1, SecretHeaderV1, TextMediaTypeV1};
use south_provider_conformance::{
    ProviderCallFailureCodeV1, ProviderMediaBinaryAuthArmV1, ProviderMediaBinaryCaseIdV1,
    ProviderMediaBinaryExpectedOutcomeV1, ProviderMediaBinaryFixtureV1,
    provider_media_binary_fixtures_v1,
};
use south_testkit::{
    AssembledProviderMediaBinaryExecutorV1, ProviderMediaBinaryMismatchCategoryV1,
    ProviderMediaBinaryObservationV1, ProviderMediaBinaryRequestV1,
    ReferenceAssembledProviderMediaBinaryExecutorV1, parse_reference_media_binary_input,
    run_provider_media_binary_conformance_v1,
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
    assert_eq!(observed.wire_auth_exact(), expected.wire_auth_exact(), "case {case:?}");
}

/// A host-shaped adapter: real `south-core` orchestration, an honest wire probe at its own
/// transport boundary, and an injectable arm mapping — the one place a host adapter translates
/// the fixture's declared arm into a `ProviderAuthV1`.
mod host_shaped_adapter {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use http::StatusCode;
    use south_contracts::{BufferedBinaryResponseV1, CredentialSlotV1, TransportErrorV1};
    use south_core::{
        AsyncBinaryHttpTransport, BinaryTransportFutureV1, CredentialResolutionFuture,
        CredentialResolver, PreparedHttpRequestV1, RequestBodyRefV1, SecretValue,
        execute_multipart_binary_call_v1, execute_text_binary_call_v1,
    };
    use south_provider_conformance::{
        FAKE_BEARER_SECRET_V1, FAKE_HEADER_SECRET_V1, ProviderCallFailureCodeV1,
        ProviderMediaBinaryAuthArmV1, ProviderMediaBinaryFixtureV1, ProviderMediaBinaryUpstreamV1,
    };
    use south_testkit::{
        AssembledProviderMediaBinaryExecutionFutureV1, AssembledProviderMediaBinaryExecutorV1,
        ProviderMediaBinaryEvidenceV1, ProviderMediaBinaryObservationV1,
        ProviderMediaBinaryRequestV1, parse_reference_media_binary_input,
    };
    use tokio_util::sync::CancellationToken;

    pub struct HostShapedAdapter {
        pub map_arm: fn(ProviderMediaBinaryAuthArmV1) -> ProviderMediaBinaryAuthArmV1,
    }

    /// Returns the fake secret for the fixture's declared arm, whatever the adapter maps it to:
    /// the mutation under test keeps the secret and changes only the arm.
    struct DeclaredArmResolver {
        arm: ProviderMediaBinaryAuthArmV1,
        calls: Arc<AtomicUsize>,
    }

    impl CredentialResolver for DeclaredArmResolver {
        fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let secret = match self.arm {
                ProviderMediaBinaryAuthArmV1::Bearer => FAKE_BEARER_SECRET_V1,
                ProviderMediaBinaryAuthArmV1::HeaderSecret(_) => FAKE_HEADER_SECRET_V1,
            };
            Box::pin(async move { Ok(SecretValue::new(secret.to_owned())) })
        }
    }

    struct ProbingTransport<'fixture> {
        fixture: &'fixture ProviderMediaBinaryFixtureV1,
        calls: AtomicUsize,
        content_type_exact: AtomicBool,
        request_body_exact: AtomicBool,
        binary_response_observed: AtomicBool,
        auth_exact: AtomicBool,
    }

    impl AsyncBinaryHttpTransport for ProbingTransport<'_> {
        fn execute_binary<'a>(
            &'a self,
            request: &'a PreparedHttpRequestV1<'_>,
            _remaining_timeout: Duration,
        ) -> BinaryTransportFutureV1<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let input = self.fixture.input();
            self.content_type_exact.store(
                request.content_type() == Some(input.expected_content_type().as_str()),
                Ordering::SeqCst,
            );
            self.request_body_exact.store(
                request.body().map(RequestBodyRefV1::as_bytes) == Some(input.body().bytes()),
                Ordering::SeqCst,
            );
            self.binary_response_observed.store(true, Ordering::SeqCst);
            let (name, value) = self.fixture.auth_arm().expected_wire_auth_header();
            let bound: Vec<(String, Vec<u8>)> = request
                .auth_headers()
                .map(|(name, value)| (name.to_ascii_lowercase(), value.to_vec()))
                .collect();
            self.auth_exact.store(bound == [(name.to_owned(), value)], Ordering::SeqCst);
            let ProviderMediaBinaryUpstreamV1::Response(raw) = *self.fixture.upstream() else {
                return Box::pin(async { Err(TransportErrorV1::RequestFailed) });
            };
            Box::pin(async move {
                let status = StatusCode::from_u16(raw.status())
                    .map_err(|_| TransportErrorV1::ResponseMetadataInvalid)?;
                BufferedBinaryResponseV1::try_from_parts(
                    status,
                    raw.body().to_vec(),
                    raw.content_type().map(str::to_owned),
                    raw.retry_after().map(str::to_owned),
                )
            })
        }
    }

    impl AssembledProviderMediaBinaryExecutorV1 for HostShapedAdapter {
        fn execute_case<'a>(
            &'a self,
            fixture: &'a ProviderMediaBinaryFixtureV1,
        ) -> AssembledProviderMediaBinaryExecutionFutureV1<'a> {
            Box::pin(async move {
                let parsed = parse_reference_media_binary_input(
                    fixture.input(),
                    (self.map_arm)(fixture.auth_arm()),
                );
                let (binding, request) = match parsed {
                    Ok(parsed) => parsed,
                    Err(code) => {
                        return ProviderMediaBinaryObservationV1::failure(
                            code,
                            ProviderMediaBinaryEvidenceV1::new(0, 0, false, false, false, false),
                        );
                    }
                };
                let resolver_calls = Arc::new(AtomicUsize::new(0));
                let resolver = DeclaredArmResolver {
                    arm: fixture.auth_arm(),
                    calls: Arc::clone(&resolver_calls),
                };
                let transport = ProbingTransport {
                    fixture,
                    calls: AtomicUsize::new(0),
                    content_type_exact: AtomicBool::new(false),
                    request_body_exact: AtomicBool::new(false),
                    binary_response_observed: AtomicBool::new(false),
                    auth_exact: AtomicBool::new(false),
                };
                let cancellation = CancellationToken::new();
                let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
                let result = match &request {
                    ProviderMediaBinaryRequestV1::Multipart(request) => {
                        execute_multipart_binary_call_v1(
                            &binding,
                            request,
                            &resolver,
                            &transport,
                            deadline,
                            &cancellation,
                        )
                        .await
                    }
                    ProviderMediaBinaryRequestV1::Text(request) => {
                        execute_text_binary_call_v1(
                            &binding,
                            request,
                            &resolver,
                            &transport,
                            deadline,
                            &cancellation,
                        )
                        .await
                    }
                };
                let evidence = ProviderMediaBinaryEvidenceV1::new(
                    resolver_calls.load(Ordering::SeqCst),
                    transport.calls.load(Ordering::SeqCst),
                    transport.content_type_exact.load(Ordering::SeqCst),
                    transport.request_body_exact.load(Ordering::SeqCst),
                    transport.binary_response_observed.load(Ordering::SeqCst),
                    transport.auth_exact.load(Ordering::SeqCst),
                );
                match result {
                    Ok(response) => ProviderMediaBinaryObservationV1::response(response, evidence),
                    Err(error) => {
                        // Only the slot-mismatch row fails inside `south-core`.
                        let code = ProviderCallFailureCodeV1::CredentialBindingMismatch;
                        assert_eq!(error.code(), code.as_str());
                        ProviderMediaBinaryObservationV1::failure(code, evidence)
                    }
                }
            })
        }
    }
}

/// The control: the host-shaped adapter with the identity mapping conforms, so its probe is sound
/// and the next test's failures come from the mapping alone.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_host_shaped_adapter_that_keeps_the_declared_arm_conforms() {
    let adapter = host_shaped_adapter::HostShapedAdapter { map_arm: |arm| arm };

    let report = tokio::time::timeout(
        Duration::from_secs(5),
        run_provider_media_binary_conformance_v1(&adapter),
    )
    .await
    .expect("media-binary conformance watchdog expired")
    .expect("the identity mapping must conform");

    assert_eq!(report.passed_case_ids().len(), 6);
}

/// The gap this claim closes: an adapter that maps the header-secret arm to Bearer sends the
/// header secret as `authorization: Bearer …` and no sanctioned header. Every outcome, call count
/// and other wire claim still matches, so before `wire_auth_exact` all six rows passed; now
/// exactly the two header-secret rows fail, and only on `WireAuth`.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn an_adapter_that_sends_the_header_secret_as_bearer_fails_only_wire_auth() {
    let adapter = host_shaped_adapter::HostShapedAdapter {
        map_arm: |_| ProviderMediaBinaryAuthArmV1::Bearer,
    };

    let failure = tokio::time::timeout(
        Duration::from_secs(5),
        run_provider_media_binary_conformance_v1(&adapter),
    )
    .await
    .expect("media-binary conformance watchdog expired")
    .expect_err("a header secret sent as Bearer must not conform");

    let observed: Vec<_> = failure
        .mismatches()
        .iter()
        .map(|mismatch| (mismatch.case_id(), mismatch.category()))
        .collect();
    assert_eq!(
        observed,
        [
            (
                ProviderMediaBinaryCaseIdV1::MultipartBinaryRejectionCarriesBody,
                ProviderMediaBinaryMismatchCategoryV1::WireAuth,
            ),
            (
                ProviderMediaBinaryCaseIdV1::TextBinaryHeaderSecretSuccess,
                ProviderMediaBinaryMismatchCategoryV1::WireAuth,
            ),
        ]
    );
}
