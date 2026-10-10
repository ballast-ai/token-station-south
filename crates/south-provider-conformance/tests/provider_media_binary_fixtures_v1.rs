use std::fmt::Display;

use http::StatusCode;
use south_contracts::{
    BearerAuthV1, BufferedBinaryResponseV1, ContractErrorV1, CredentialSlotV1, MultipartBodyV1,
    MultipartBoundaryV1, ProviderEndpointV1, RelativePathV1, SafeHeaders, SecretHeaderV1,
    TextBodyV1, TextMediaTypeV1, TextPostRequestV1,
};
use south_provider_conformance::{
    FAKE_BEARER_SECRET_V1, FAKE_HEADER_SECRET_V1, PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_ID,
    PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_VERSION, ProviderCallCountV1,
    ProviderCallFailureCodeV1, ProviderMediaBinaryAuthArmV1, ProviderMediaBinaryCaseIdV1,
    ProviderMediaBinaryExpectedOutcomeV1, ProviderMediaBinaryFixtureV1,
    ProviderMediaBinaryRequestBodyV1, ProviderMediaBinaryUpstreamV1, provider_binary_fixtures_v1,
    provider_media_binary_fixtures_v1, provider_multipart_fixtures_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ProviderMediaBinaryFixtureV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderMediaBinaryInputV1: Display);
assert_not_impl_any!(ProviderMediaBinaryRequestBodyV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderMediaBinaryExpectedV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderMediaBinaryExpectedEvidenceV1: Display);

const SENTINELS: &[&str] = &[
    "endpoint-debug-sentinel.invalid",
    "bound-slot-debug-sentinel",
    "requested-slot-debug-sentinel",
    "path-debug-sentinel",
    "header-name-debug-sentinel",
    "header-value-debug-sentinel",
    "content-type-debug-sentinel",
    "retry-after-debug-sentinel",
    "rejection-body-debug-sentinel",
    "boundary-debug-sentinel",
    "body-value-debug-sentinel",
    "ssml-text-debug-sentinel",
    "voice-debug-sentinel",
    "application/ssml+xml",
];

#[test]
fn suite_identity_and_canonical_case_order_are_frozen() {
    assert_eq!(PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_VERSION, 1);
    assert_eq!(PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_ID, "south.provider-media-binary.v1");

    let case_ids: Vec<_> = provider_media_binary_fixtures_v1()
        .iter()
        .map(ProviderMediaBinaryFixtureV1::case_id)
        .collect();
    assert_eq!(
        case_ids,
        [
            ProviderMediaBinaryCaseIdV1::MultipartBinarySuccess,
            ProviderMediaBinaryCaseIdV1::MultipartBinaryRejectionCarriesBody,
            ProviderMediaBinaryCaseIdV1::TextBinaryBearerSuccess,
            ProviderMediaBinaryCaseIdV1::TextBinaryHeaderSecretSuccess,
            ProviderMediaBinaryCaseIdV1::TextBinarySlotMismatch,
            ProviderMediaBinaryCaseIdV1::TextContentTypeSmuggled,
        ]
    );
}

/// The new shapes got their own table so the two suites hosts have already verified against keep
/// the case counts their evidence names.
#[test]
fn the_existing_multipart_and_binary_tables_are_untouched() {
    assert_eq!(provider_multipart_fixtures_v1().len(), 5);
    assert_eq!(provider_binary_fixtures_v1().len(), 6);
}

#[test]
fn canonical_table_freezes_bodies_arms_upstreams_and_outcomes() {
    let fixtures = provider_media_binary_fixtures_v1();
    assert_eq!(fixtures.len(), 6);

    // 1–2. Multipart rows: the same body, a success that is not UTF-8 and a rejection with
    // metadata, so the pair separates "bytes on success" from "bytes on every status".
    for fixture in &fixtures[..2] {
        assert!(matches!(
            fixture.input().body(),
            ProviderMediaBinaryRequestBodyV1::Multipart { .. }
        ));
    }
    let ProviderMediaBinaryUpstreamV1::Response(success) = fixtures[0].upstream() else {
        panic!("the multipart success row must respond");
    };
    assert_eq!(success.status(), 200);
    assert!(std::str::from_utf8(success.body()).is_err(), "the success body must not be UTF-8");
    let ProviderMediaBinaryUpstreamV1::Response(rejection) = fixtures[1].upstream() else {
        panic!("the multipart rejection row must respond");
    };
    assert!(!(200..300).contains(&rejection.status()));
    assert!(rejection.retry_after().is_some());
    assert_eq!(
        fixtures[1].auth_arm(),
        ProviderMediaBinaryAuthArmV1::HeaderSecret(SecretHeaderV1::ApiKey)
    );

    // 3–4. Text rows differ only in the credential arm.
    assert_eq!(fixtures[2].input(), fixtures[3].input());
    assert_eq!(fixtures[2].upstream(), fixtures[3].upstream());
    assert_eq!(fixtures[2].auth_arm(), ProviderMediaBinaryAuthArmV1::Bearer);
    assert_eq!(
        fixtures[3].auth_arm(),
        ProviderMediaBinaryAuthArmV1::HeaderSecret(SecretHeaderV1::OcpApimSubscriptionKey)
    );

    // Every reached row expects exactly what the upstream sent.
    for fixture in &fixtures[..4] {
        let ProviderMediaBinaryUpstreamV1::Response(raw) = fixture.upstream() else {
            panic!("a reached row must respond");
        };
        let ProviderMediaBinaryExpectedOutcomeV1::Response {
            status,
            body,
            content_type,
            retry_after,
        } = fixture.expected().outcome()
        else {
            panic!("a reached row must expect a response");
        };
        assert_eq!(*status, raw.status());
        assert_eq!(*body, raw.body());
        assert_eq!(*content_type, raw.content_type());
        assert_eq!(*retry_after, raw.retry_after());
    }

    // 5. Slot mismatch on the text shape.
    assert_ne!(
        fixtures[4].input().requested_credential_slot(),
        fixtures[4].input().bound_credential_slot()
    );
    assert!(matches!(
        fixtures[4].expected().outcome(),
        ProviderMediaBinaryExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::CredentialBindingMismatch
        }
    ));

    // 6. The smuggled `content-type` is present and carries the *correct* value.
    let smuggled = fixtures[5]
        .input()
        .headers()
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .expect("the smuggling case must carry a content-type");
    assert_eq!(smuggled.1, fixtures[5].input().expected_content_type());
    assert!(matches!(
        fixtures[5].expected().outcome(),
        ProviderMediaBinaryExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::InvalidRelativePath
        }
    ));
    for fixture in &fixtures[2..] {
        assert!(matches!(fixture.input().body(), ProviderMediaBinaryRequestBodyV1::Text { .. }));
    }
    for fixture in &fixtures[4..] {
        assert!(matches!(fixture.upstream(), ProviderMediaBinaryUpstreamV1::NotReached));
    }
}

/// Every wire claim is a presence claim: true exactly where the transport is reached, so no row
/// asserts something it could not have observed.
#[test]
fn canonical_table_freezes_the_expected_wire_shape_evidence() {
    for fixture in provider_media_binary_fixtures_v1() {
        let reached = matches!(fixture.upstream(), ProviderMediaBinaryUpstreamV1::Response(_));
        let evidence = fixture.expected().evidence();
        let case = fixture.case_id();
        let calls = if reached { ProviderCallCountV1::One } else { ProviderCallCountV1::Zero };
        assert_eq!(evidence.resolver_calls(), calls, "{case:?}");
        assert_eq!(evidence.transport_calls(), calls, "{case:?}");
        assert_eq!(evidence.wire_content_type_exact(), reached, "{case:?}");
        assert_eq!(evidence.wire_request_body_exact(), reached, "{case:?}");
        assert_eq!(evidence.wire_binary_response_observed(), reached, "{case:?}");
        assert_eq!(evidence.wire_auth_exact(), reached, "{case:?}");
    }
}

/// The expected auth header is built from the declared arm alone: Bearer binds `authorization`
/// with the `Bearer ` prefix, a header secret binds its sanctioned lowercase name with the secret
/// verbatim and never `authorization`. The two header-secret rows therefore expect a different
/// wire from the Bearer rows even though the request is otherwise identical.
#[test]
fn expected_wire_auth_header_follows_the_declared_arm_only() {
    let bearer = ProviderMediaBinaryAuthArmV1::Bearer.expected_wire_auth_header();
    assert_eq!(bearer.0, "authorization");
    assert_eq!(bearer.1, [b"Bearer ".as_slice(), FAKE_BEARER_SECRET_V1.as_bytes()].concat());
    for header in SecretHeaderV1::ALL {
        let (name, value) =
            ProviderMediaBinaryAuthArmV1::HeaderSecret(header).expected_wire_auth_header();
        assert_eq!(name, header.header_name());
        assert_eq!(name, name.to_ascii_lowercase());
        assert_ne!(name, "authorization");
        assert_eq!(value, FAKE_HEADER_SECRET_V1.as_bytes());
    }
    let header_secret_rows: Vec<_> = provider_media_binary_fixtures_v1()
        .iter()
        .filter(|fixture| {
            matches!(fixture.auth_arm(), ProviderMediaBinaryAuthArmV1::HeaderSecret(_))
                && matches!(fixture.upstream(), ProviderMediaBinaryUpstreamV1::Response(_))
        })
        .map(ProviderMediaBinaryFixtureV1::case_id)
        .collect();
    assert_eq!(
        header_secret_rows,
        [
            ProviderMediaBinaryCaseIdV1::MultipartBinaryRejectionCarriesBody,
            ProviderMediaBinaryCaseIdV1::TextBinaryHeaderSecretSuccess,
        ]
    );
}

#[test]
fn every_raw_fixture_field_is_checked_through_the_production_contract() {
    for fixture in provider_media_binary_fixtures_v1() {
        let input = fixture.input();
        ProviderEndpointV1::parse(input.endpoint()).expect("canonical endpoint must parse");
        CredentialSlotV1::parse(input.bound_credential_slot())
            .expect("canonical bound slot must parse");
        let slot = CredentialSlotV1::parse(input.requested_credential_slot())
            .expect("canonical requested slot must parse");
        let path = RelativePathV1::parse(input.relative_path()).expect("canonical path must parse");
        let headers = SafeHeaders::try_from_iter(input.headers().iter().copied())
            .expect("canonical headers must parse");

        match *input.body() {
            ProviderMediaBinaryRequestBodyV1::Multipart { bytes, boundary } => {
                let boundary =
                    MultipartBoundaryV1::parse(boundary).expect("canonical boundary must parse");
                let body = MultipartBodyV1::parse(bytes.to_vec(), boundary)
                    .expect("canonical body must be delimited by its declared boundary");
                assert_eq!(body.content_type(), input.expected_content_type());
                assert_eq!(body.as_bytes(), input.body().bytes());
            }
            ProviderMediaBinaryRequestBodyV1::Text { text, media_type } => {
                let media_type =
                    TextMediaTypeV1::parse(media_type).expect("canonical media type must parse");
                assert_eq!(media_type, TextMediaTypeV1::Ssml);
                let body = TextBodyV1::try_new(text.to_owned(), media_type)
                    .expect("canonical text must be in bounds");
                assert_eq!(body.content_type(), input.expected_content_type());
                assert_eq!(body.as_str().as_bytes(), input.body().bytes());
                let built =
                    TextPostRequestV1::try_new(path, headers, body, BearerAuthV1::new(slot));
                if fixture.case_id() == ProviderMediaBinaryCaseIdV1::TextContentTypeSmuggled {
                    assert_eq!(built.err(), Some(ContractErrorV1::ContentTypeHeaderNotPermitted));
                } else {
                    built.expect("every other canonical text request must build");
                }
            }
        }

        if let ProviderMediaBinaryAuthArmV1::HeaderSecret(header) = fixture.auth_arm() {
            SafeHeaders::try_from_iter([(header.header_name(), "value")])
                .expect_err("the sanctioned header must stay reserved");
        }

        if let ProviderMediaBinaryUpstreamV1::Response(raw) = fixture.upstream() {
            let status = StatusCode::from_u16(raw.status()).expect("canonical status must parse");
            BufferedBinaryResponseV1::try_from_parts(
                status,
                raw.body().to_vec(),
                raw.content_type().map(str::to_owned),
                raw.retry_after().map(str::to_owned),
            )
            .expect("canonical response must satisfy production bounds");
        }
    }
}

#[test]
fn debug_output_redacts_all_raw_values() {
    for fixture in provider_media_binary_fixtures_v1() {
        assert_redacted(&format!("{fixture:?}"));
        assert_redacted(&format!("{:?}", fixture.input()));
        assert_redacted(&format!("{:?}", fixture.input().body()));
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
