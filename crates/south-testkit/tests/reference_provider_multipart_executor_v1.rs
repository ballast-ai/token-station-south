use std::time::Duration;

use south_contracts::{ProviderAuthV1, SecretHeaderV1};
use south_provider_conformance::{
    ProviderMultipartAuthArmV1, ProviderMultipartCaseIdV1, ProviderMultipartExpectedOutcomeV1,
    provider_multipart_fixtures_v1,
};
use south_testkit::{
    AssembledProviderMultipartExecutorV1, ProviderMultipartMismatchCategoryV1,
    ProviderMultipartObservationV1, ReferenceAssembledProviderMultipartExecutorV1,
    parse_reference_multipart_input, run_provider_multipart_conformance_v1,
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
    use south_contracts::{BufferedHttpResponseV1, CredentialSlotV1, TransportErrorV1};
    use south_core::{
        AsyncHttpTransport, CredentialResolutionFuture, CredentialResolver, PreparedHttpRequestV1,
        RequestBodyRefV1, SecretValue, TransportFuture, execute_multipart_call_v1,
    };
    use south_provider_conformance::{
        FAKE_BEARER_SECRET_V1, FAKE_HEADER_SECRET_V1, ProviderCallFailureCodeV1,
        ProviderMultipartAuthArmV1, ProviderMultipartFixtureV1, ProviderMultipartUpstreamV1,
    };
    use south_testkit::{
        AssembledProviderMultipartExecutionFutureV1, AssembledProviderMultipartExecutorV1,
        ProviderMultipartEvidenceV1, ProviderMultipartObservationV1,
        parse_reference_multipart_input,
    };
    use tokio_util::sync::CancellationToken;

    pub struct HostShapedAdapter {
        pub map_arm: fn(ProviderMultipartAuthArmV1) -> ProviderMultipartAuthArmV1,
    }

    /// Returns the fake secret for the fixture's declared arm, whatever the adapter maps it to:
    /// the mutation under test keeps the secret and changes only the arm.
    struct DeclaredArmResolver {
        arm: ProviderMultipartAuthArmV1,
        calls: Arc<AtomicUsize>,
    }

    impl CredentialResolver for DeclaredArmResolver {
        fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let secret = match self.arm {
                ProviderMultipartAuthArmV1::Bearer => FAKE_BEARER_SECRET_V1,
                ProviderMultipartAuthArmV1::HeaderSecret(_) => FAKE_HEADER_SECRET_V1,
            };
            Box::pin(async move { Ok(SecretValue::new(secret.to_owned())) })
        }
    }

    struct ProbingTransport<'fixture> {
        fixture: &'fixture ProviderMultipartFixtureV1,
        calls: AtomicUsize,
        content_type_exact: AtomicBool,
        body_bytes_exact: AtomicBool,
        auth_exact: AtomicBool,
    }

    impl AsyncHttpTransport for ProbingTransport<'_> {
        fn execute<'a>(
            &'a self,
            request: &'a PreparedHttpRequestV1<'_>,
            _remaining_timeout: Duration,
        ) -> TransportFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let input = self.fixture.input();
            self.content_type_exact.store(
                request.content_type() == Some(input.expected_content_type().as_str()),
                Ordering::SeqCst,
            );
            self.body_bytes_exact.store(
                request.body().map(RequestBodyRefV1::as_bytes) == Some(input.body()),
                Ordering::SeqCst,
            );
            let (name, value) = self.fixture.auth_arm().expected_wire_auth_header();
            let bound: Vec<(String, Vec<u8>)> = request
                .auth_headers()
                .map(|(name, value)| (name.to_ascii_lowercase(), value.to_vec()))
                .collect();
            self.auth_exact.store(
                bound == [(name.to_owned(), value)]
                    && request.headers().get("authorization").is_none(),
                Ordering::SeqCst,
            );
            let ProviderMultipartUpstreamV1::Response(raw) = *self.fixture.upstream() else {
                return Box::pin(async { Err(TransportErrorV1::RequestFailed) });
            };
            Box::pin(async move {
                let status = StatusCode::from_u16(raw.status())
                    .map_err(|_| TransportErrorV1::ResponseMetadataInvalid)?;
                BufferedHttpResponseV1::try_from_parts(
                    status,
                    raw.body().as_bytes().to_vec(),
                    raw.content_type().map(str::to_owned),
                    raw.retry_after().map(str::to_owned),
                )
            })
        }
    }

    impl AssembledProviderMultipartExecutorV1 for HostShapedAdapter {
        fn execute_case<'a>(
            &'a self,
            fixture: &'a ProviderMultipartFixtureV1,
        ) -> AssembledProviderMultipartExecutionFutureV1<'a> {
            Box::pin(async move {
                let parsed = parse_reference_multipart_input(
                    fixture.input(),
                    (self.map_arm)(fixture.auth_arm()),
                );
                let (binding, request) = match parsed {
                    Ok(parsed) => parsed,
                    Err(code) => {
                        return ProviderMultipartObservationV1::failure(
                            code,
                            ProviderMultipartEvidenceV1::new(0, 0, false, false, false),
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
                    body_bytes_exact: AtomicBool::new(false),
                    auth_exact: AtomicBool::new(false),
                };
                let cancellation = CancellationToken::new();
                let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
                let result = execute_multipart_call_v1(
                    &binding,
                    &request,
                    &resolver,
                    &transport,
                    deadline,
                    &cancellation,
                )
                .await;
                let evidence = ProviderMultipartEvidenceV1::new(
                    resolver_calls.load(Ordering::SeqCst),
                    transport.calls.load(Ordering::SeqCst),
                    transport.content_type_exact.load(Ordering::SeqCst),
                    transport.body_bytes_exact.load(Ordering::SeqCst),
                    transport.auth_exact.load(Ordering::SeqCst),
                );
                match result {
                    Ok(response) => ProviderMultipartObservationV1::response(response, evidence),
                    Err(error) => {
                        // Only the slot-mismatch row fails inside `south-core`.
                        let code = ProviderCallFailureCodeV1::CredentialBindingMismatch;
                        assert_eq!(error.code(), code.as_str());
                        ProviderMultipartObservationV1::failure(code, evidence)
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
        run_provider_multipart_conformance_v1(&adapter),
    )
    .await
    .expect("conformance watchdog expired")
    .expect("the identity mapping must conform");

    assert_eq!(report.passed_case_ids().len(), 5);
}

/// The gap `wire_auth_exact` closes: an adapter that maps the header-secret arm to Bearer sends
/// the header secret as `authorization: Bearer …` and no sanctioned header. Every outcome, call
/// count and other wire claim still matches, so before the claim existed every row passed; now
/// exactly the header-secret row fails, and only on `WireAuth`.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn an_adapter_that_sends_the_header_secret_as_bearer_fails_only_wire_auth() {
    let adapter =
        host_shaped_adapter::HostShapedAdapter { map_arm: |_| ProviderMultipartAuthArmV1::Bearer };

    let failure = tokio::time::timeout(
        Duration::from_secs(5),
        run_provider_multipart_conformance_v1(&adapter),
    )
    .await
    .expect("conformance watchdog expired")
    .expect_err("a header secret sent as Bearer must not conform");

    let observed: Vec<_> = failure
        .mismatches()
        .iter()
        .map(|mismatch| (mismatch.case_id(), mismatch.category()))
        .collect();
    assert_eq!(
        observed,
        [(
            ProviderMultipartCaseIdV1::BufferedMultipartHeaderSecretSuccess,
            ProviderMultipartMismatchCategoryV1::WireAuth
        ),]
    );
}
