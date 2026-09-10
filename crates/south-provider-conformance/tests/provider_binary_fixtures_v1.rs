use std::fmt::Display;

use http::StatusCode;
use south_contracts::{
    BufferedBinaryResponseV1, CredentialSlotV1, JsonBodyV1, MAX_BINARY_RESPONSE_BODY_BYTES,
    MAX_RESPONSE_BODY_BYTES, ProviderEndpointV1, RelativePathV1, SafeHeaders,
};
use south_provider_conformance::{
    PROVIDER_BINARY_CONFORMANCE_SUITE_ID, PROVIDER_BINARY_CONFORMANCE_SUITE_VERSION,
    ProviderBinaryBodyV1, ProviderBinaryCaseIdV1, ProviderBinaryEntryArmV1,
    ProviderBinaryExpectedOutcomeV1, ProviderBinaryFixtureV1, ProviderBinaryUpstreamV1,
    ProviderCallCountV1, ProviderCallFailureCodeV1, provider_binary_fixtures_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ProviderBinaryFixtureV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderBinaryInputV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderBinaryExpectedV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderBinaryExpectedEvidenceV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderBinaryBodyV1: Display);

const SENTINELS: &[&str] = &[
    "endpoint-debug-sentinel.invalid",
    "bound-slot-debug-sentinel",
    "requested-slot-debug-sentinel",
    "path-debug-sentinel",
    "header-name-debug-sentinel",
    "header-value-debug-sentinel",
    "request-body-debug-sentinel",
    "rejection-body-debug-sentinel",
    "content-type-debug-sentinel",
    "json-content-type-debug-sentinel",
    "retry-after-debug-sentinel",
];

#[test]
fn suite_identity_and_canonical_case_order_are_frozen() {
    assert_eq!(PROVIDER_BINARY_CONFORMANCE_SUITE_VERSION, 1);
    assert_eq!(PROVIDER_BINARY_CONFORMANCE_SUITE_ID, "south.provider-binary.v1");

    let case_ids: Vec<_> =
        provider_binary_fixtures_v1().iter().map(ProviderBinaryFixtureV1::case_id).collect();
    assert_eq!(
        case_ids,
        [
            ProviderBinaryCaseIdV1::BinarySuccessNonUtf8Body,
            ProviderBinaryCaseIdV1::BinaryRejectionCarriesJsonBody,
            ProviderBinaryCaseIdV1::BinaryBodyAboveTextCapSucceeds,
            ProviderBinaryCaseIdV1::BinaryBodyAboveBinaryCapRefused,
            ProviderBinaryCaseIdV1::BinarySlotMismatch,
            ProviderBinaryCaseIdV1::TextArmStillRefusesNonUtf8,
        ]
    );
}

/// The premise of the whole suite: the payload really cannot be read as text. If this ever became
/// valid UTF-8, cases one and six would both pass for the wrong reason.
#[test]
fn the_canonical_payload_is_genuinely_not_utf8() {
    let fixtures = provider_binary_fixtures_v1();
    let ProviderBinaryUpstreamV1::Response(raw) = fixtures[0].upstream() else {
        panic!("the success case must respond");
    };
    let ProviderBinaryBodyV1::Literal(bytes) = raw.body() else {
        panic!("the success case's body is retained literally");
    };
    assert!(std::str::from_utf8(bytes).is_err(), "the canonical audio payload must not be UTF-8");

    // The filler byte for both synthesized bodies is equally untextual, so neither cap row can
    // accidentally pass through a UTF-8 path.
    let ProviderBinaryUpstreamV1::Response(above_text_cap) = fixtures[2].upstream() else {
        panic!("the above-text-cap case must respond");
    };
    let ProviderBinaryBodyV1::Synthesized { fill, .. } = above_text_cap.body() else {
        panic!("the cap rows are synthesized");
    };
    assert!(std::str::from_utf8(&[fill]).is_err(), "the synthesized fill must not be UTF-8");
}

#[test]
fn canonical_table_freezes_entry_arms_upstreams_and_outcomes() {
    let fixtures = provider_binary_fixtures_v1();
    assert_eq!(fixtures.len(), 6);

    // 1. A non-UTF-8 2xx comes back byte for byte.
    assert_eq!(fixtures[0].entry_arm(), ProviderBinaryEntryArmV1::Binary);
    let ProviderBinaryExpectedOutcomeV1::Response { status, body, .. } =
        fixtures[0].expected().outcome()
    else {
        panic!("the success case must expect a response");
    };
    assert_eq!(*status, 200);
    let ProviderBinaryUpstreamV1::Response(raw) = fixtures[0].upstream() else {
        panic!("the success case must respond");
    };
    assert_eq!(*body, raw.body());

    // 2. A rejection is a response at the South boundary, not a transport error, and it still
    // carries the metadata contracts.
    let ProviderBinaryExpectedOutcomeV1::Response { status, retry_after, .. } =
        fixtures[1].expected().outcome()
    else {
        panic!("a rejection must still be observed as a response");
    };
    assert_eq!(*status, 429);
    assert!(retry_after.is_some(), "the rejection row must exercise retry-after");

    // 3–4. The two cap rows straddle the binary cap, and the accepted one is above the text cap —
    // which is the only thing that makes it a test of D4 rather than of nothing.
    let above_text_cap = fixtures[2].expected().outcome();
    let ProviderBinaryExpectedOutcomeV1::Response { body, .. } = above_text_cap else {
        panic!("the above-text-cap row must succeed");
    };
    assert!(body.len() > MAX_RESPONSE_BODY_BYTES);
    assert!(body.len() <= MAX_BINARY_RESPONSE_BODY_BYTES);

    let ProviderBinaryUpstreamV1::Response(over_cap) = fixtures[3].upstream() else {
        panic!("the over-cap row must reach the transport");
    };
    assert!(over_cap.body().len() > MAX_BINARY_RESPONSE_BODY_BYTES);
    assert!(matches!(
        fixtures[3].expected().outcome(),
        ProviderBinaryExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::ResponseBodyTooLarge
        }
    ));

    // 5. Slot mismatch: refused before resolver and transport.
    assert_ne!(
        fixtures[4].input().requested_credential_slot(),
        fixtures[4].input().bound_credential_slot()
    );
    assert!(matches!(fixtures[4].upstream(), ProviderBinaryUpstreamV1::NotReached));
    assert!(matches!(
        fixtures[4].expected().outcome(),
        ProviderBinaryExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::CredentialBindingMismatch
        }
    ));

    // 6. The regression row: the same bytes as case one, the other entry arm, still refused. The
    // pair is only a controlled experiment while the payloads are identical.
    assert_eq!(fixtures[5].entry_arm(), ProviderBinaryEntryArmV1::Utf8);
    let ProviderBinaryUpstreamV1::Response(text_arm_raw) = fixtures[5].upstream() else {
        panic!("the regression row must reach a transport");
    };
    assert_eq!(text_arm_raw.body(), raw.body());
    assert!(matches!(
        fixtures[5].expected().outcome(),
        ProviderBinaryExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::ResponseBodyNotUtf8
        }
    ));

    // Exactly one row drives the frozen UTF-8 entry point.
    assert_eq!(
        fixtures.iter().filter(|f| f.entry_arm() == ProviderBinaryEntryArmV1::Utf8).count(),
        1
    );
}

/// Both wire booleans are presence claims. Two rows reach a transport and still expect `false` for
/// at least one, which is what an adapter whose probe hardcodes `true` cannot survive.
#[test]
fn canonical_table_freezes_the_expected_wire_shape_evidence() {
    let fixtures = provider_binary_fixtures_v1();
    let expected_evidence = [
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true, true),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true, true),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true, true),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true, false),
        (ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false, false),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, false, false),
    ];
    assert_eq!(fixtures.len(), expected_evidence.len());
    for (fixture, expected) in fixtures.iter().zip(expected_evidence) {
        let evidence = fixture.expected().evidence();
        let case = fixture.case_id();
        assert_eq!(evidence.resolver_calls(), expected.0, "{case:?}");
        assert_eq!(evidence.transport_calls(), expected.1, "{case:?}");
        assert_eq!(evidence.wire_binary_response_observed(), expected.2, "{case:?}");
        assert_eq!(evidence.wire_body_bytes_exact(), expected.3, "{case:?}");
    }

    // The binary-seam claim is true exactly on the rows driven through the binary entry point that
    // reach a transport: no row asserts a seam it could not have used.
    for fixture in fixtures {
        let binary_arm = fixture.entry_arm() == ProviderBinaryEntryArmV1::Binary;
        let reached = matches!(fixture.upstream(), ProviderBinaryUpstreamV1::Response(_));
        assert_eq!(
            fixture.expected().evidence().wire_binary_response_observed(),
            binary_arm && reached,
            "{:?}",
            fixture.case_id()
        );
    }

    // The bytes claim is true exactly where a response is expected: nothing came back to compare
    // on any refusal row, whichever boundary refused it.
    for fixture in fixtures {
        let returns_body = matches!(
            fixture.expected().outcome(),
            ProviderBinaryExpectedOutcomeV1::Response { .. }
        );
        assert_eq!(
            fixture.expected().evidence().wire_body_bytes_exact(),
            returns_body,
            "{:?}",
            fixture.case_id()
        );
    }
}

#[test]
fn every_raw_fixture_field_is_checked_through_the_production_contract() {
    for fixture in provider_binary_fixtures_v1() {
        let input = fixture.input();
        ProviderEndpointV1::parse(input.endpoint()).expect("canonical endpoint must parse");
        CredentialSlotV1::parse(input.bound_credential_slot())
            .expect("canonical bound slot must parse");
        CredentialSlotV1::parse(input.requested_credential_slot())
            .expect("canonical requested slot must parse");
        RelativePathV1::parse(input.relative_path()).expect("canonical path must parse");
        JsonBodyV1::parse(input.json_body()).expect("canonical request body must parse");
        SafeHeaders::try_from_iter(input.headers().iter().copied())
            .expect("canonical headers must parse");

        // Literal upstream bodies are built through the real response contract. The two
        // synthesized rows are deliberately not materialised here — the cap bound is asserted
        // above from the declared length, and the runner exercises the real construction on both.
        if let ProviderBinaryUpstreamV1::Response(raw) = fixture.upstream() {
            let status = StatusCode::from_u16(raw.status()).expect("canonical status must parse");
            if let ProviderBinaryBodyV1::Literal(bytes) = raw.body() {
                BufferedBinaryResponseV1::try_from_parts(
                    status,
                    bytes.to_vec(),
                    raw.content_type().map(str::to_owned),
                    raw.retry_after().map(str::to_owned),
                )
                .expect("canonical response must satisfy production bounds");
            }
        }
    }
}

/// `matches` is what the runner compares with, so its two arms are worth pinning directly.
#[test]
fn body_matching_is_exact_in_both_forms() {
    let literal = ProviderBinaryBodyV1::Literal(&[0xFF, 0x00]);
    assert!(literal.matches(&[0xFF, 0x00]));
    assert!(!literal.matches(&[0xFF, 0x01]));
    assert!(!literal.matches(&[0xFF]));
    assert_eq!(literal.len(), 2);
    assert!(!literal.is_empty());
    assert_eq!(literal.materialize(), vec![0xFF, 0x00]);

    let synthesized = ProviderBinaryBodyV1::Synthesized { fill: 0x80, length: 3 };
    assert!(synthesized.matches(&[0x80, 0x80, 0x80]));
    assert!(!synthesized.matches(&[0x80, 0x80]));
    assert!(!synthesized.matches(&[0x80, 0x80, 0x81]));
    assert_eq!(synthesized.len(), 3);
    assert_eq!(synthesized.materialize(), vec![0x80; 3]);
}

#[test]
fn debug_output_redacts_all_raw_values() {
    for fixture in provider_binary_fixtures_v1() {
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
