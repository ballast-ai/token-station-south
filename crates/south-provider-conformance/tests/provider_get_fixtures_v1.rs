use std::fmt::Display;

use http::StatusCode;
use south_contracts::{
    BufferedHttpResponseV1, CredentialSlotV1, ProviderEndpointV1, QueryParameterV1, QueryStringV1,
    RelativePathV1, SafeHeaders, SecretHeaderV1,
};
use south_provider_conformance::{
    PROVIDER_GET_CONFORMANCE_SUITE_ID, PROVIDER_GET_CONFORMANCE_SUITE_VERSION,
    ProviderCallCaseIdV1, ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderGetAuthArmV1,
    ProviderGetCaseIdV1, ProviderGetExpectedOutcomeV1, ProviderGetFixtureV1, ProviderGetUpstreamV1,
    provider_get_fixtures_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ProviderGetFixtureV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderGetInputV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderGetExpectedV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderGetExpectedEvidenceV1: Display);

const SENTINELS: &[&str] = &[
    "endpoint-debug-sentinel.invalid",
    "bound-slot-debug-sentinel",
    "requested-slot-debug-sentinel",
    "path-debug-sentinel",
    "header-name-debug-sentinel",
    "header-value-debug-sentinel",
    "response-body-debug-sentinel",
    "content-type-debug-sentinel",
    "retry-after-debug-sentinel",
    "276843862449040",
];

#[test]
fn suite_identity_and_canonical_case_order_are_frozen() {
    assert_eq!(PROVIDER_GET_CONFORMANCE_SUITE_VERSION, 1);
    assert_eq!(PROVIDER_GET_CONFORMANCE_SUITE_ID, "south.provider-get.v1");

    let case_ids: Vec<_> =
        provider_get_fixtures_v1().iter().map(ProviderGetFixtureV1::case_id).collect();
    assert_eq!(
        case_ids,
        [
            ProviderGetCaseIdV1::BufferedGetBearerSuccess,
            ProviderGetCaseIdV1::BufferedGetHeaderSecretSuccess,
            ProviderGetCaseIdV1::GetSlotMismatch,
            ProviderGetCaseIdV1::BufferedGetTaskIdQuerySuccess,
        ]
    );
}

#[test]
fn canonical_table_freezes_arms_queries_upstreams_and_outcomes() {
    let fixtures = provider_get_fixtures_v1();
    assert_eq!(fixtures.len(), 4);

    // 1. BufferedGetBearerSuccess: the Bearer arm, no query, a full response.
    assert_eq!(fixtures[0].auth_arm(), ProviderGetAuthArmV1::Bearer);
    assert!(fixtures[0].declared_query().is_empty());
    let ProviderGetUpstreamV1::Response(raw) = fixtures[0].upstream() else {
        panic!("the Bearer case must respond");
    };
    assert_eq!(raw.status(), 200);
    assert!(raw.retry_after().is_some());
    let ProviderGetExpectedOutcomeV1::Response { status, body, retry_after, .. } =
        fixtures[0].expected().outcome()
    else {
        panic!("the Bearer case must expect a response");
    };
    assert_eq!(*status, 200);
    assert_eq!(*body, raw.body());
    assert_eq!(*retry_after, raw.retry_after());

    // 2. BufferedGetHeaderSecretSuccess: a sanctioned header, no query.
    assert_eq!(fixtures[1].auth_arm(), ProviderGetAuthArmV1::HeaderSecret(SecretHeaderV1::XApiKey));
    assert!(fixtures[1].declared_query().is_empty());
    assert!(matches!(fixtures[1].upstream(), ProviderGetUpstreamV1::Response(_)));
    assert!(matches!(
        fixtures[1].expected().outcome(),
        ProviderGetExpectedOutcomeV1::Response { status: 200, .. }
    ));

    // 3. GetSlotMismatch: refused before resolver and transport.
    assert_ne!(
        fixtures[2].input().requested_credential_slot(),
        fixtures[2].input().bound_credential_slot()
    );
    assert!(matches!(fixtures[2].upstream(), ProviderGetUpstreamV1::NotReached));
    assert!(matches!(
        fixtures[2].expected().outcome(),
        ProviderGetExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::CredentialBindingMismatch
        }
    ));

    // 4. BufferedGetTaskIdQuerySuccess: the one polling family that carries the id as a query.
    assert_eq!(fixtures[3].auth_arm(), ProviderGetAuthArmV1::Bearer);
    assert_eq!(fixtures[3].declared_query(), [(QueryParameterV1::TaskId, "276843862449040")]);
    assert!(matches!(fixtures[3].upstream(), ProviderGetUpstreamV1::Response(_)));
    assert!(matches!(
        fixtures[3].expected().outcome(),
        ProviderGetExpectedOutcomeV1::Response { status: 200, .. }
    ));
}

/// The three wire-shape booleans have fixed polarities: two rows reach the transport and still
/// expect `wire_query_exact == false`, and the refused row expects `wire_method_get == false`
/// with `wire_body_absent` vacuously `true`. A probe that hardcodes any of them fails a row.
#[test]
fn canonical_table_freezes_the_expected_wire_shape_evidence() {
    let fixtures = provider_get_fixtures_v1();

    let expected_evidence = [
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true, true, false),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true, true, false),
        (ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false, true, false),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true, true, true),
    ];
    assert_eq!(fixtures.len(), expected_evidence.len());
    for (fixture, expected) in fixtures.iter().zip(expected_evidence) {
        let evidence = fixture.expected().evidence();
        assert_eq!(evidence.resolver_calls(), expected.0, "{:?}", fixture.case_id());
        assert_eq!(evidence.transport_calls(), expected.1, "{:?}", fixture.case_id());
        assert_eq!(evidence.wire_method_get(), expected.2, "{:?}", fixture.case_id());
        assert_eq!(evidence.wire_body_absent(), expected.3, "{:?}", fixture.case_id());
        assert_eq!(evidence.wire_query_exact(), expected.4, "{:?}", fixture.case_id());
    }
    // Every row expects the body absent: a GET has no body slot, reached or not.
    assert!(fixtures.iter().all(|fixture| fixture.expected().evidence().wire_body_absent()));
    // The query claim is `true` exactly where something was declared.
    for fixture in fixtures {
        assert_eq!(
            fixture.expected().evidence().wire_query_exact(),
            !fixture.declared_query().is_empty(),
            "{:?}",
            fixture.case_id()
        );
    }
}

#[test]
fn every_raw_fixture_field_is_checked_through_the_production_contract() {
    for fixture in provider_get_fixtures_v1() {
        let input = fixture.input();
        ProviderEndpointV1::parse(input.endpoint()).expect("canonical endpoint must parse");
        CredentialSlotV1::parse(input.bound_credential_slot())
            .expect("canonical bound slot must parse");
        CredentialSlotV1::parse(input.requested_credential_slot())
            .expect("canonical requested slot must parse");
        RelativePathV1::parse(input.relative_path()).expect("canonical path must parse");
        SafeHeaders::try_from_iter(input.headers().iter().copied())
            .expect("canonical headers must parse");
        if !fixture.declared_query().is_empty() {
            QueryStringV1::try_from_iter(fixture.declared_query().iter().copied())
                .expect("canonical query must satisfy its grammar");
        }
        if let ProviderGetAuthArmV1::HeaderSecret(header) = fixture.auth_arm() {
            // The sanctioned header itself must never be constructible as a plain header.
            SafeHeaders::try_from_iter([(header.header_name(), "value")])
                .expect_err("the sanctioned header must stay reserved");
        }

        match fixture.upstream() {
            ProviderGetUpstreamV1::Response(raw) => {
                let status =
                    StatusCode::from_u16(raw.status()).expect("canonical status must parse");
                BufferedHttpResponseV1::try_from_parts(
                    status,
                    raw.body().as_bytes().to_vec(),
                    raw.content_type().map(str::to_owned),
                    raw.retry_after().map(str::to_owned),
                )
                .expect("canonical response must satisfy production bounds");
            }
            ProviderGetUpstreamV1::NotReached => {}
        }
    }
}

/// The GET input is the provider-call input minus its body, at the same endpoint and slot, so
/// a host adapter binds the same fake identity across suites — and cannot carry a body.
#[test]
fn get_and_call_suites_share_endpoint_slot_and_headers_but_the_get_input_has_no_body() {
    let call_success = south_provider_conformance::provider_call_fixtures_v1()
        .iter()
        .find(|fixture| fixture.case_id() == ProviderCallCaseIdV1::Success)
        .expect("the provider-call table has a success case");
    for fixture in provider_get_fixtures_v1() {
        assert_eq!(fixture.input().endpoint(), call_success.input().endpoint());
        assert_eq!(
            fixture.input().bound_credential_slot(),
            call_success.input().bound_credential_slot()
        );
        assert_eq!(fixture.input().headers(), call_success.input().headers());
    }
    let rendered = format!("{:?}", provider_get_fixtures_v1()[0].input());
    assert!(!rendered.contains("body"), "a GET input must not even name a body: {rendered}");
}

#[test]
fn debug_output_redacts_all_raw_values() {
    for fixture in provider_get_fixtures_v1() {
        assert_redacted(&format!("{fixture:?}"));
        assert_redacted(&format!("{:?}", fixture.input()));
        assert_redacted(&format!("{:?}", fixture.upstream()));
        assert_redacted(&format!("{:?}", fixture.expected()));
        assert_redacted(&format!("{:?}", fixture.expected().evidence()));
    }
}

fn assert_redacted(debug: &str) {
    for sentinel in SENTINELS {
        assert!(!debug.contains(sentinel), "debug output leaked sentinel: {debug}");
    }
}
