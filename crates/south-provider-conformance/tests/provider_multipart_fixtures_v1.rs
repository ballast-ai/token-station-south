use std::fmt::Display;

use http::StatusCode;
use south_contracts::{
    BufferedHttpResponseV1, ContractErrorV1, CredentialSlotV1, MultipartBodyV1,
    MultipartBoundaryV1, MultipartPostRequestV1, ProviderEndpointV1, RelativePathV1, SafeHeaders,
    SecretHeaderV1,
};
use south_provider_conformance::{
    PROVIDER_MULTIPART_CONFORMANCE_SUITE_ID, PROVIDER_MULTIPART_CONFORMANCE_SUITE_VERSION,
    ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderMultipartAuthArmV1,
    ProviderMultipartCaseIdV1, ProviderMultipartExpectedOutcomeV1, ProviderMultipartFixtureV1,
    ProviderMultipartUpstreamV1, provider_multipart_fixtures_v1,
    provider_multipart_mismatched_boundary_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ProviderMultipartFixtureV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderMultipartInputV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderMultipartExpectedV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderMultipartExpectedEvidenceV1: Display);

const SENTINELS: &[&str] = &[
    "endpoint-debug-sentinel.invalid",
    "bound-slot-debug-sentinel",
    "requested-slot-debug-sentinel",
    "path-debug-sentinel",
    "header-name-debug-sentinel",
    "header-value-debug-sentinel",
    "response-body-debug-sentinel",
    "content-type-debug-sentinel",
    "boundary-debug-sentinel",
    "body-value-debug-sentinel",
];

#[test]
fn suite_identity_and_canonical_case_order_are_frozen() {
    assert_eq!(PROVIDER_MULTIPART_CONFORMANCE_SUITE_VERSION, 1);
    assert_eq!(PROVIDER_MULTIPART_CONFORMANCE_SUITE_ID, "south.provider-multipart.v1");

    let case_ids: Vec<_> =
        provider_multipart_fixtures_v1().iter().map(ProviderMultipartFixtureV1::case_id).collect();
    assert_eq!(
        case_ids,
        [
            ProviderMultipartCaseIdV1::BufferedMultipartBearerSuccess,
            ProviderMultipartCaseIdV1::BufferedMultipartHeaderSecretSuccess,
            ProviderMultipartCaseIdV1::MultipartSlotMismatch,
            ProviderMultipartCaseIdV1::MultipartBodyBoundaryMismatch,
            ProviderMultipartCaseIdV1::MultipartContentTypeSmuggled,
        ]
    );
}

#[test]
fn canonical_table_freezes_arms_upstreams_and_outcomes() {
    let fixtures = provider_multipart_fixtures_v1();
    assert_eq!(fixtures.len(), 5);

    // 1–2. The two success rows differ only in the credential arm, which is what makes them a
    // pair: everything else being identical is what proves the arm is the variable under test.
    assert_eq!(fixtures[0].auth_arm(), ProviderMultipartAuthArmV1::Bearer);
    assert_eq!(
        fixtures[1].auth_arm(),
        ProviderMultipartAuthArmV1::HeaderSecret(SecretHeaderV1::ApiKey)
    );
    assert_eq!(fixtures[0].input().body(), fixtures[1].input().body());
    assert_eq!(fixtures[0].input().boundary(), fixtures[1].input().boundary());
    for fixture in &fixtures[..2] {
        let ProviderMultipartUpstreamV1::Response(raw) = fixture.upstream() else {
            panic!("a success case must respond");
        };
        let ProviderMultipartExpectedOutcomeV1::Response { status, body, .. } =
            fixture.expected().outcome()
        else {
            panic!("a success case must expect a response");
        };
        assert_eq!(*status, 200);
        assert_eq!(*body, raw.body());
    }

    // 3. Slot mismatch: refused before resolver and transport.
    assert_ne!(
        fixtures[2].input().requested_credential_slot(),
        fixtures[2].input().bound_credential_slot()
    );
    assert!(matches!(fixtures[2].upstream(), ProviderMultipartUpstreamV1::NotReached));

    // 4. The body is delimited by a boundary the request does not declare — the case is only
    // meaningful while those two differ, so assert it rather than trusting the constants.
    assert_ne!(fixtures[3].input().boundary(), provider_multipart_mismatched_boundary_v1());
    assert!(
        fixtures[3]
            .input()
            .body()
            .starts_with(format!("--{}", provider_multipart_mismatched_boundary_v1()).as_bytes())
    );
    assert!(matches!(fixtures[3].upstream(), ProviderMultipartUpstreamV1::NotReached));

    // 5. A `content-type` really is present in the ordinary headers, and its value is the
    // *correct* one — so the case cannot pass by the value happening to be wrong.
    let smuggled = fixtures[4]
        .input()
        .headers()
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .expect("the smuggling case must carry a content-type");
    assert_eq!(smuggled.1, fixtures[4].input().expected_content_type());
    assert!(matches!(fixtures[4].upstream(), ProviderMultipartUpstreamV1::NotReached));

    // Both refusal rows fold into the same frozen code, and deliberately not into a JSON one.
    for fixture in &fixtures[3..] {
        assert!(matches!(
            fixture.expected().outcome(),
            ProviderMultipartExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::InvalidRelativePath
            }
        ));
    }
}

/// Both wire booleans are presence claims, so the three rows that never reach the transport
/// expect `false` for both. An adapter whose probe answers without reading the prepared request
/// fails those three.
#[test]
fn canonical_table_freezes_the_expected_wire_shape_evidence() {
    let fixtures = provider_multipart_fixtures_v1();
    let expected_evidence = [
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true, true),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true, true),
        (ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false, false),
        (ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false, false),
        (ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false, false),
    ];
    assert_eq!(fixtures.len(), expected_evidence.len());
    for (fixture, expected) in fixtures.iter().zip(expected_evidence) {
        let evidence = fixture.expected().evidence();
        let case = fixture.case_id();
        assert_eq!(evidence.resolver_calls(), expected.0, "{case:?}");
        assert_eq!(evidence.transport_calls(), expected.1, "{case:?}");
        assert_eq!(evidence.wire_content_type_exact(), expected.2, "{case:?}");
        assert_eq!(evidence.wire_body_bytes_exact(), expected.3, "{case:?}");
    }
    // A wire claim is true exactly where the transport is reached: no row asserts something it
    // could not have observed.
    for fixture in fixtures {
        let reached = matches!(fixture.upstream(), ProviderMultipartUpstreamV1::Response(_));
        assert_eq!(fixture.expected().evidence().wire_content_type_exact(), reached);
        assert_eq!(fixture.expected().evidence().wire_body_bytes_exact(), reached);
    }
}

#[test]
fn every_raw_fixture_field_is_checked_through_the_production_contract() {
    for fixture in provider_multipart_fixtures_v1() {
        let input = fixture.input();
        ProviderEndpointV1::parse(input.endpoint()).expect("canonical endpoint must parse");
        CredentialSlotV1::parse(input.bound_credential_slot())
            .expect("canonical bound slot must parse");
        CredentialSlotV1::parse(input.requested_credential_slot())
            .expect("canonical requested slot must parse");
        RelativePathV1::parse(input.relative_path()).expect("canonical path must parse");
        let headers = SafeHeaders::try_from_iter(input.headers().iter().copied())
            .expect("canonical headers must parse");
        let boundary =
            MultipartBoundaryV1::parse(input.boundary()).expect("canonical boundary must parse");
        assert_eq!(
            input.expected_content_type(),
            format!("multipart/form-data; boundary={}", boundary.as_str())
        );

        // The two refusal rows must fail exactly where the table says they do, and the three
        // others must build cleanly — this is the fixture table proving its own premises rather
        // than asserting them in prose.
        let body = MultipartBodyV1::parse(input.body().to_vec(), boundary);
        match fixture.case_id() {
            ProviderMultipartCaseIdV1::MultipartBodyBoundaryMismatch => {
                assert_eq!(body.err(), Some(ContractErrorV1::InvalidMultipartBody));
            }
            ProviderMultipartCaseIdV1::MultipartContentTypeSmuggled => {
                let body = body.expect("this case's body is well formed; its headers are not");
                let slot = CredentialSlotV1::parse(input.requested_credential_slot()).unwrap();
                assert_eq!(
                    MultipartPostRequestV1::try_new(
                        RelativePathV1::parse(input.relative_path()).unwrap(),
                        headers,
                        body,
                        south_contracts::BearerAuthV1::new(slot),
                    )
                    .err(),
                    Some(ContractErrorV1::ContentTypeHeaderNotPermitted)
                );
            }
            _ => {
                body.expect("canonical body must be delimited by its declared boundary");
            }
        }

        if let ProviderMultipartAuthArmV1::HeaderSecret(header) = fixture.auth_arm() {
            SafeHeaders::try_from_iter([(header.header_name(), "value")])
                .expect_err("the sanctioned header must stay reserved");
        }

        match fixture.upstream() {
            ProviderMultipartUpstreamV1::Response(raw) => {
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
            ProviderMultipartUpstreamV1::NotReached => {}
        }
    }
}

#[test]
fn debug_output_redacts_all_raw_values() {
    for fixture in provider_multipart_fixtures_v1() {
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
