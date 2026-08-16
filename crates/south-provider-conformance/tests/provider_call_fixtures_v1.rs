use std::fmt::Display;

use http::StatusCode;
use south_contracts::{
    BufferedHttpResponseV1, ContractErrorV1, CredentialSlotV1, JsonBodyV1,
    MAX_JSON_REQUEST_BODY_BYTES, MAX_RELATIVE_PATH_BYTES, ProviderEndpointV1, RelativePathV1,
    SafeHeaders, TransportErrorV1,
};
use south_provider_conformance::{
    FAKE_BEARER_SECRET_V1, PROVIDER_CALL_CONFORMANCE_DEADLINE_OFFSET_V1,
    PROVIDER_CALL_CONFORMANCE_SUITE_ID, PROVIDER_CALL_CONFORMANCE_SUITE_VERSION,
    ProviderCallCaseIdV1, ProviderCallControlV1, ProviderCallCountV1,
    ProviderCallExpectedOutcomeV1, ProviderCallFailureCodeV1, ProviderCallUpstreamV1,
    provider_call_fixtures_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(south_provider_conformance::ProviderCallFixtureV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderCallInputV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderCallRawResponseV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderCallExpectedV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderCallExpectedEvidenceV1: Display);

const SENTINELS: &[&str] = &[
    "endpoint-debug-sentinel.invalid",
    "bound-slot-debug-sentinel",
    "requested-slot-debug-sentinel",
    "path-debug-sentinel",
    "header-name-debug-sentinel",
    "header-value-debug-sentinel",
    "request-body-debug-sentinel",
    "response-body-debug-sentinel",
    "content-type-debug-sentinel",
    "retry-after-debug-sentinel",
    FAKE_BEARER_SECRET_V1,
];

#[test]
fn suite_identity_deadline_and_canonical_case_order_are_frozen() {
    assert_eq!(PROVIDER_CALL_CONFORMANCE_SUITE_VERSION, 1);
    assert_eq!(PROVIDER_CALL_CONFORMANCE_SUITE_ID, "south.provider-call.v1");
    assert_eq!(PROVIDER_CALL_CONFORMANCE_DEADLINE_OFFSET_V1.as_secs(), 1);

    let case_ids: Vec<_> = provider_call_fixtures_v1()
        .iter()
        .map(south_provider_conformance::ProviderCallFixtureV1::case_id)
        .collect();
    assert_eq!(
        case_ids,
        [
            ProviderCallCaseIdV1::Success,
            ProviderCallCaseIdV1::InvalidRelativePath,
            ProviderCallCaseIdV1::CredentialSlotMismatch,
            ProviderCallCaseIdV1::RedirectDenied,
            ProviderCallCaseIdV1::ResponseBodyTooLarge,
            ProviderCallCaseIdV1::Cancelled,
            ProviderCallCaseIdV1::DeadlineExceeded,
        ]
    );
}

#[test]
fn canonical_table_freezes_controls_upstreams_outcomes_and_evidence() {
    let fixtures = provider_call_fixtures_v1();
    assert_eq!(fixtures.len(), 7);

    assert!(matches!(fixtures[0].control(), ProviderCallControlV1::Complete));
    assert!(matches!(fixtures[0].upstream(), ProviderCallUpstreamV1::Response(_)));
    assert!(matches!(
        fixtures[0].expected().outcome(),
        ProviderCallExpectedOutcomeV1::Response { .. }
    ));

    assert!(matches!(fixtures[1].control(), ProviderCallControlV1::Complete));
    assert!(matches!(fixtures[1].upstream(), ProviderCallUpstreamV1::NotReached));
    assert_failure(&fixtures[1], ProviderCallFailureCodeV1::InvalidRelativePath);

    assert!(matches!(fixtures[2].control(), ProviderCallControlV1::Complete));
    assert!(matches!(fixtures[2].upstream(), ProviderCallUpstreamV1::NotReached));
    assert_failure(&fixtures[2], ProviderCallFailureCodeV1::CredentialBindingMismatch);

    assert!(matches!(fixtures[3].control(), ProviderCallControlV1::Complete));
    assert!(matches!(
        fixtures[3].upstream(),
        ProviderCallUpstreamV1::TransportFailure(TransportErrorV1::RedirectDenied)
    ));
    assert_failure(&fixtures[3], ProviderCallFailureCodeV1::RedirectDenied);

    assert!(matches!(fixtures[4].control(), ProviderCallControlV1::Complete));
    assert!(matches!(
        fixtures[4].upstream(),
        ProviderCallUpstreamV1::TransportFailure(TransportErrorV1::ResponseBodyTooLarge)
    ));
    assert_failure(&fixtures[4], ProviderCallFailureCodeV1::ResponseBodyTooLarge);

    assert!(matches!(fixtures[5].control(), ProviderCallControlV1::CancelWhileResolverPending));
    assert!(matches!(fixtures[5].upstream(), ProviderCallUpstreamV1::NotReached));
    assert_failure(&fixtures[5], ProviderCallFailureCodeV1::Cancelled);

    assert!(matches!(fixtures[6].control(), ProviderCallControlV1::ExpireWhileTransportPending));
    assert!(matches!(fixtures[6].upstream(), ProviderCallUpstreamV1::Pending));
    assert_failure(&fixtures[6], ProviderCallFailureCodeV1::DeadlineExceeded);

    let expected_counts = [
        (ProviderCallCountV1::One, ProviderCallCountV1::One, false, false),
        (ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false, false),
        (ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false, false),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, false, false),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, false, false),
        (ProviderCallCountV1::One, ProviderCallCountV1::Zero, true, false),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, false, true),
    ];
    for (fixture, expected) in fixtures.iter().zip(expected_counts) {
        let evidence = fixture.expected().evidence();
        assert_eq!(evidence.resolver_calls(), expected.0);
        assert_eq!(evidence.transport_calls(), expected.1);
        assert_eq!(evidence.resolver_future_dropped_while_pending(), expected.2);
        assert_eq!(evidence.transport_future_dropped_while_pending(), expected.3);
    }
}

#[test]
fn every_raw_fixture_field_is_checked_through_the_production_contract() {
    for fixture in provider_call_fixtures_v1() {
        let input = fixture.input();
        ProviderEndpointV1::parse(input.endpoint()).expect("canonical endpoint must parse");
        CredentialSlotV1::parse(input.bound_credential_slot())
            .expect("canonical bound slot must parse");
        CredentialSlotV1::parse(input.requested_credential_slot())
            .expect("canonical requested slot must parse");
        JsonBodyV1::parse(input.json_body()).expect("canonical JSON must parse");
        SafeHeaders::try_from_iter(input.headers().iter().copied())
            .expect("canonical headers must parse");

        assert!(input.relative_path().len() <= MAX_RELATIVE_PATH_BYTES);
        let path = RelativePathV1::parse(input.relative_path());
        if fixture.case_id() == ProviderCallCaseIdV1::InvalidRelativePath {
            assert_eq!(
                path.expect_err("invalid fixture path must fail"),
                ContractErrorV1::InvalidRelativePath
            );
        } else {
            path.expect("canonical path must parse");
        }

        if let ProviderCallUpstreamV1::Response(raw) = fixture.upstream() {
            let status = StatusCode::from_u16(raw.status()).expect("canonical status must parse");
            BufferedHttpResponseV1::try_from_parts(
                status,
                raw.body().as_bytes().to_vec(),
                raw.content_type().map(str::to_owned),
                raw.retry_after().map(str::to_owned),
            )
            .expect("canonical raw response must satisfy production bounds");
        }
    }
}

#[test]
fn raw_fixture_bodies_stay_bounded_without_embedding_an_oversized_allocation() {
    for fixture in provider_call_fixtures_v1() {
        assert!(fixture.input().json_body().len() <= MAX_JSON_REQUEST_BODY_BYTES);
        if let ProviderCallUpstreamV1::Response(raw) = fixture.upstream() {
            assert!(raw.body().len() <= south_contracts::MAX_RESPONSE_BODY_BYTES);
        }
    }
    let oversized = &provider_call_fixtures_v1()[4];
    assert!(matches!(
        oversized.upstream(),
        ProviderCallUpstreamV1::TransportFailure(TransportErrorV1::ResponseBodyTooLarge)
    ));
}

#[test]
fn failure_code_enum_is_closed_over_every_frozen_code() {
    let variants = [
        ProviderCallFailureCodeV1::InvalidEndpoint,
        ProviderCallFailureCodeV1::InvalidRelativePath,
        ProviderCallFailureCodeV1::InvalidCredentialSlot,
        ProviderCallFailureCodeV1::InvalidJsonBody,
        ProviderCallFailureCodeV1::RequestBodyTooLarge,
        ProviderCallFailureCodeV1::UrlOutsideBinding,
        ProviderCallFailureCodeV1::CredentialBindingMismatch,
        ProviderCallFailureCodeV1::CredentialResolutionFailed,
        ProviderCallFailureCodeV1::Cancelled,
        ProviderCallFailureCodeV1::DeadlineExceeded,
        ProviderCallFailureCodeV1::ClientBuildFailed,
        ProviderCallFailureCodeV1::TransportTimeout,
        ProviderCallFailureCodeV1::ConnectFailed,
        ProviderCallFailureCodeV1::RequestFailed,
        ProviderCallFailureCodeV1::ResponseReadFailed,
        ProviderCallFailureCodeV1::ResponseBodyTooLarge,
        ProviderCallFailureCodeV1::ResponseBodyNotUtf8,
        ProviderCallFailureCodeV1::ResponseMetadataInvalid,
        ProviderCallFailureCodeV1::RedirectDenied,
    ];
    let codes: Vec<_> = variants.iter().map(|variant| variant.as_str()).collect();
    assert_eq!(
        codes,
        [
            "INVALID_ENDPOINT",
            "INVALID_RELATIVE_PATH",
            "INVALID_CREDENTIAL_SLOT",
            "INVALID_JSON_BODY",
            "REQUEST_BODY_TOO_LARGE",
            "URL_OUTSIDE_BINDING",
            "CREDENTIAL_BINDING_MISMATCH",
            "CREDENTIAL_RESOLUTION_FAILED",
            "CANCELLED",
            "DEADLINE_EXCEEDED",
            "CLIENT_BUILD_FAILED",
            "TRANSPORT_TIMEOUT",
            "CONNECT_FAILED",
            "REQUEST_FAILED",
            "RESPONSE_READ_FAILED",
            "RESPONSE_BODY_TOO_LARGE",
            "RESPONSE_BODY_NOT_UTF8",
            "RESPONSE_METADATA_INVALID",
            "REDIRECT_DENIED",
        ]
    );
}

#[test]
fn call_count_saturates_without_integer_narrowing() {
    assert_eq!(ProviderCallCountV1::from_usize(0), ProviderCallCountV1::Zero);
    assert_eq!(ProviderCallCountV1::from_usize(1), ProviderCallCountV1::One);
    assert_eq!(ProviderCallCountV1::from_usize(2), ProviderCallCountV1::MoreThanOne);
    assert_eq!(ProviderCallCountV1::from_usize(256), ProviderCallCountV1::MoreThanOne);
    assert_eq!(ProviderCallCountV1::from_usize(257), ProviderCallCountV1::MoreThanOne);
    assert_eq!(ProviderCallCountV1::from_usize(usize::MAX), ProviderCallCountV1::MoreThanOne);
}

#[test]
fn debug_output_redacts_all_raw_values_and_the_fake_secret() {
    for fixture in provider_call_fixtures_v1() {
        assert_redacted(&format!("{fixture:?}"));
        assert_redacted(&format!("{:?}", fixture.input()));
        assert_redacted(&format!("{:?}", fixture.upstream()));
        assert_redacted(&format!("{:?}", fixture.expected()));
        assert_redacted(&format!("{:?}", fixture.expected().evidence()));
        if let ProviderCallUpstreamV1::Response(raw) = fixture.upstream() {
            assert_redacted(&format!("{raw:?}"));
        }
    }
}

fn assert_failure(
    fixture: &south_provider_conformance::ProviderCallFixtureV1,
    code: ProviderCallFailureCodeV1,
) {
    assert!(matches!(
        fixture.expected().outcome(),
        ProviderCallExpectedOutcomeV1::Failure { code: actual } if *actual == code
    ));
}

fn assert_redacted(debug: &str) {
    for sentinel in SENTINELS {
        assert!(!debug.contains(sentinel), "debug output leaked sentinel: {debug}");
    }
}
