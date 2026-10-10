use std::time::Duration;

use south_contracts::{ProviderAuthV1, SecretHeaderV1};
use south_provider_conformance::{
    ProviderGetAuthArmV1, ProviderGetCaseIdV1, ProviderGetExpectedOutcomeV1,
    provider_get_fixtures_v1,
};
use south_testkit::{
    AssembledProviderGetExecutorV1, ProviderGetMismatchCategoryV1, ProviderGetObservationV1,
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

    use http::{Method, StatusCode};
    use south_contracts::{
        BufferedHttpResponseV1, CredentialSlotV1, QueryStringV1, TransportErrorV1,
    };
    use south_core::{
        AsyncHttpTransport, CredentialResolutionFuture, CredentialResolver, PreparedHttpRequestV1,
        SecretValue, TransportFuture, execute_get_call_v1,
    };
    use south_provider_conformance::{
        FAKE_BEARER_SECRET_V1, FAKE_HEADER_SECRET_V1, ProviderCallFailureCodeV1,
        ProviderGetAuthArmV1, ProviderGetFixtureV1, ProviderGetUpstreamV1,
    };
    use south_testkit::{
        AssembledProviderGetExecutionFutureV1, AssembledProviderGetExecutorV1,
        ProviderGetEvidenceV1, ProviderGetObservationV1, parse_reference_get_input,
    };
    use tokio_util::sync::CancellationToken;

    pub struct HostShapedAdapter {
        pub map_arm: fn(ProviderGetAuthArmV1) -> ProviderGetAuthArmV1,
    }

    /// Returns the fake secret for the fixture's declared arm, whatever the adapter maps it to:
    /// the mutation under test keeps the secret and changes only the arm.
    struct DeclaredArmResolver {
        arm: ProviderGetAuthArmV1,
        calls: Arc<AtomicUsize>,
    }

    impl CredentialResolver for DeclaredArmResolver {
        fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let secret = match self.arm {
                ProviderGetAuthArmV1::Bearer => FAKE_BEARER_SECRET_V1,
                ProviderGetAuthArmV1::HeaderSecret(_) => FAKE_HEADER_SECRET_V1,
            };
            Box::pin(async move { Ok(SecretValue::new(secret.to_owned())) })
        }
    }

    struct ProbingTransport<'fixture> {
        fixture: &'fixture ProviderGetFixtureV1,
        declared_query: Option<QueryStringV1>,
        calls: AtomicUsize,
        method_get: AtomicBool,
        body_absent: AtomicBool,
        query_exact: AtomicBool,
        auth_exact: AtomicBool,
    }

    impl AsyncHttpTransport for ProbingTransport<'_> {
        fn execute<'a>(
            &'a self,
            request: &'a PreparedHttpRequestV1<'_>,
            _remaining_timeout: Duration,
        ) -> TransportFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.method_get.store(request.method() == Method::GET, Ordering::SeqCst);
            self.body_absent.store(request.body().is_none(), Ordering::SeqCst);
            let declared = self.declared_query.as_ref().map(QueryStringV1::as_str);
            self.query_exact
                .store(declared.is_some() && request.url().query() == declared, Ordering::SeqCst);
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
            let ProviderGetUpstreamV1::Response(raw) = *self.fixture.upstream() else {
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

    impl AssembledProviderGetExecutorV1 for HostShapedAdapter {
        fn execute_case<'a>(
            &'a self,
            fixture: &'a ProviderGetFixtureV1,
        ) -> AssembledProviderGetExecutionFutureV1<'a> {
            Box::pin(async move {
                let (binding, request) =
                    parse_reference_get_input(fixture.input(), (self.map_arm)(fixture.auth_arm()))
                        .expect("every canonical GET input parses");
                let declared_query = if fixture.declared_query().is_empty() {
                    None
                } else {
                    Some(
                        QueryStringV1::try_from_iter(fixture.declared_query().iter().cloned())
                            .expect("every canonical query declaration is valid"),
                    )
                };
                let request = match declared_query.clone() {
                    Some(query) => request.with_query(query),
                    None => request,
                };
                let resolver_calls = Arc::new(AtomicUsize::new(0));
                let resolver = DeclaredArmResolver {
                    arm: fixture.auth_arm(),
                    calls: Arc::clone(&resolver_calls),
                };
                let transport = ProbingTransport {
                    fixture,
                    declared_query,
                    calls: AtomicUsize::new(0),
                    method_get: AtomicBool::new(false),
                    body_absent: AtomicBool::new(true),
                    query_exact: AtomicBool::new(false),
                    auth_exact: AtomicBool::new(false),
                };
                let cancellation = CancellationToken::new();
                let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
                let result = execute_get_call_v1(
                    &binding,
                    &request,
                    &resolver,
                    &transport,
                    deadline,
                    &cancellation,
                )
                .await;
                let evidence = ProviderGetEvidenceV1::new(
                    resolver_calls.load(Ordering::SeqCst),
                    transport.calls.load(Ordering::SeqCst),
                    transport.method_get.load(Ordering::SeqCst),
                    transport.body_absent.load(Ordering::SeqCst),
                    transport.query_exact.load(Ordering::SeqCst),
                    transport.auth_exact.load(Ordering::SeqCst),
                );
                match result {
                    Ok(response) => ProviderGetObservationV1::response(response, evidence),
                    Err(error) => {
                        // Only the slot-mismatch row fails inside `south-core`.
                        let code = ProviderCallFailureCodeV1::CredentialBindingMismatch;
                        assert_eq!(error.code(), code.as_str());
                        ProviderGetObservationV1::failure(code, evidence)
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

    let report =
        tokio::time::timeout(Duration::from_secs(5), run_provider_get_conformance_v1(&adapter))
            .await
            .expect("conformance watchdog expired")
            .expect("the identity mapping must conform");

    assert_eq!(report.passed_case_ids().len(), 4);
}

/// The gap `wire_auth_exact` closes: an adapter that maps the header-secret arm to Bearer sends
/// the header secret as `authorization: Bearer …` and no sanctioned header. Every outcome, call
/// count and other wire claim still matches, so before the claim existed every row passed; now
/// exactly the header-secret row fails, and only on `WireAuth`.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn an_adapter_that_sends_the_header_secret_as_bearer_fails_only_wire_auth() {
    let adapter =
        host_shaped_adapter::HostShapedAdapter { map_arm: |_| ProviderGetAuthArmV1::Bearer };

    let failure =
        tokio::time::timeout(Duration::from_secs(5), run_provider_get_conformance_v1(&adapter))
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
            ProviderGetCaseIdV1::BufferedGetHeaderSecretSuccess,
            ProviderGetMismatchCategoryV1::WireAuth
        ),]
    );
}
