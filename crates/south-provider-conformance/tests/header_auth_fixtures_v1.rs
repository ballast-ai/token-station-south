use std::fmt::Display;

use bytes::Bytes;
use http::StatusCode;
use south_contracts::{
    BufferedHttpResponseV1, CredentialSlotV1, JsonBodyV1, MAX_STREAM_CHUNK_BYTES,
    ProviderEndpointV1, RelativePathV1, SafeHeaders, SecretHeaderV1, StreamChunkV1,
    StreamingResponseHeadV1,
};
use south_provider_conformance::{
    FAKE_HEADER_SECRET_V1, HEADER_AUTH_CONFORMANCE_SUITE_ID, HEADER_AUTH_CONFORMANCE_SUITE_VERSION,
    HeaderAuthCaseIdV1, HeaderAuthExpectedOutcomeV1, HeaderAuthFixtureV1, HeaderAuthUpstreamV1,
    ProviderCallCaseIdV1, ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderStreamTerminalV1,
    header_auth_fixtures_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(HeaderAuthFixtureV1: Display);
assert_not_impl_any!(south_provider_conformance::HeaderAuthExpectedV1: Display);
assert_not_impl_any!(south_provider_conformance::HeaderAuthExpectedEvidenceV1: Display);

const SENTINELS: &[&str] = &[
    "endpoint-debug-sentinel.invalid",
    "bound-slot-debug-sentinel",
    "requested-slot-debug-sentinel",
    "path-debug-sentinel",
    "header-name-debug-sentinel",
    "header-value-debug-sentinel",
    "request-body-debug-sentinel",
    "response-body-debug-sentinel",
    "header-auth-chunk-one-debug-sentinel",
    "header-auth-chunk-two-debug-sentinel",
    "content-type-debug-sentinel",
    "retry-after-debug-sentinel",
];

#[test]
fn suite_identity_and_canonical_case_order_are_frozen() {
    assert_eq!(HEADER_AUTH_CONFORMANCE_SUITE_VERSION, 1);
    assert_eq!(HEADER_AUTH_CONFORMANCE_SUITE_ID, "south.header-auth.v1");

    let case_ids: Vec<_> =
        header_auth_fixtures_v1().iter().map(HeaderAuthFixtureV1::case_id).collect();
    assert_eq!(
        case_ids,
        [
            HeaderAuthCaseIdV1::BufferedHeaderSecretSuccess,
            HeaderAuthCaseIdV1::StreamingHeaderSecretSuccess,
            HeaderAuthCaseIdV1::HeaderSecretSlotMismatch,
        ]
    );
}

#[test]
fn canonical_table_freezes_headers_upstreams_outcomes_and_evidence() {
    let fixtures = header_auth_fixtures_v1();
    assert_eq!(fixtures.len(), 3);

    // 1. BufferedHeaderSecretSuccess: one buffered exchange under the sanctioned header.
    assert_eq!(fixtures[0].secret_header(), SecretHeaderV1::XApiKey);
    let HeaderAuthUpstreamV1::Response(raw) = fixtures[0].upstream() else {
        panic!("the buffered case must respond");
    };
    assert_eq!(raw.status(), 201);
    let HeaderAuthExpectedOutcomeV1::Response { status, body, .. } =
        fixtures[0].expected().outcome()
    else {
        panic!("the buffered case must expect a response");
    };
    assert_eq!(*status, 201);
    assert_eq!(*body, raw.body());

    // 2. StreamingHeaderSecretSuccess: headers-ready, byte-identical chunks, clean EOF.
    assert_eq!(fixtures[1].secret_header(), SecretHeaderV1::XGoogApiKey);
    let HeaderAuthUpstreamV1::Stream(raw) = fixtures[1].upstream() else {
        panic!("the streaming case must stream");
    };
    assert_eq!(raw.chunks().len(), 2);
    assert!(matches!(raw.terminal(), ProviderStreamTerminalV1::CleanEof));
    let HeaderAuthExpectedOutcomeV1::Opened { status, chunks, .. } =
        fixtures[1].expected().outcome()
    else {
        panic!("the streaming case must expect an opened stream");
    };
    assert_eq!(*status, 200);
    assert_eq!(*chunks, raw.chunks());

    // 3. HeaderSecretSlotMismatch: refused before resolver and transport under the header arm.
    assert_eq!(fixtures[2].secret_header(), SecretHeaderV1::ApiKey);
    assert_ne!(
        fixtures[2].input().requested_credential_slot(),
        fixtures[2].input().bound_credential_slot()
    );
    assert!(matches!(fixtures[2].upstream(), HeaderAuthUpstreamV1::NotReached));
    assert!(matches!(
        fixtures[2].expected().outcome(),
        HeaderAuthExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::CredentialBindingMismatch
        }
    ));
}

#[test]
fn canonical_table_freezes_the_expected_wire_shape_evidence() {
    let fixtures = header_auth_fixtures_v1();

    let expected_evidence = [
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true, true),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true, true),
        (ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false, true),
    ];
    for (fixture, expected) in fixtures.iter().zip(expected_evidence) {
        let evidence = fixture.expected().evidence();
        assert_eq!(evidence.resolver_calls(), expected.0);
        assert_eq!(evidence.transport_calls(), expected.1);
        assert_eq!(evidence.sanctioned_header_exact(), expected.2);
        assert_eq!(evidence.authorization_header_absent(), expected.3);
    }
}

#[test]
fn every_raw_fixture_field_is_checked_through_the_production_contract() {
    for fixture in header_auth_fixtures_v1() {
        let input = fixture.input();
        ProviderEndpointV1::parse(input.endpoint()).expect("canonical endpoint must parse");
        CredentialSlotV1::parse(input.bound_credential_slot())
            .expect("canonical bound slot must parse");
        CredentialSlotV1::parse(input.requested_credential_slot())
            .expect("canonical requested slot must parse");
        RelativePathV1::parse(input.relative_path()).expect("canonical path must parse");
        JsonBodyV1::parse(input.json_body()).expect("canonical JSON must parse");
        SafeHeaders::try_from_iter(input.headers().iter().copied())
            .expect("canonical headers must parse");

        // The sanctioned header itself must never be constructible as a plain header.
        SafeHeaders::try_from_iter([(fixture.secret_header().header_name(), "value")])
            .expect_err("the sanctioned header must stay reserved");

        match fixture.upstream() {
            HeaderAuthUpstreamV1::Response(raw) => {
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
            HeaderAuthUpstreamV1::Stream(raw) => {
                let status =
                    StatusCode::from_u16(raw.head().status()).expect("canonical status must parse");
                assert!(status.is_success(), "a streamed fixture head must be 2xx");
                StreamingResponseHeadV1::try_from_parts(
                    status,
                    raw.head().content_type().map(str::to_owned),
                    raw.head().retry_after().map(str::to_owned),
                )
                .expect("canonical stream head must satisfy production bounds");
                for chunk in raw.chunks() {
                    assert!(chunk.len() <= MAX_STREAM_CHUNK_BYTES);
                    StreamChunkV1::try_new(Bytes::from_static(chunk))
                        .expect("canonical chunk must satisfy the delivery bound");
                }
            }
            HeaderAuthUpstreamV1::NotReached => {}
        }
    }
}

#[test]
fn header_auth_and_call_suites_share_one_frozen_input_shape() {
    let header_auth_inputs: Vec<_> =
        header_auth_fixtures_v1().iter().map(|fixture| fixture.input().endpoint()).collect();
    let call_inputs: Vec<_> = south_provider_conformance::provider_call_fixtures_v1()
        .iter()
        .filter(|fixture| fixture.case_id() == ProviderCallCaseIdV1::Success)
        .map(|fixture| fixture.input().endpoint())
        .collect();

    assert!(header_auth_inputs.iter().all(|endpoint| *endpoint == call_inputs[0]));
}

#[test]
fn fake_header_secret_is_synthetic_and_distinct_from_the_bearer_fixture() {
    assert_eq!(FAKE_HEADER_SECRET_V1, "south-test-only-fake-header-secret-v1");
    assert_ne!(FAKE_HEADER_SECRET_V1, south_provider_conformance::FAKE_BEARER_SECRET_V1);
}

#[test]
fn debug_output_redacts_all_raw_values() {
    for fixture in header_auth_fixtures_v1() {
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
