use std::fmt::Display;

use bytes::Bytes;
use http::StatusCode;
use south_contracts::{
    BufferedHttpResponseV1, ControlledUserAgentV1, CredentialSlotV1, JsonBodyV1,
    MAX_STREAM_CHUNK_BYTES, ProviderEndpointV1, RelativePathV1, SafeHeaders, StreamChunkV1,
    StreamingResponseHeadV1,
};
use south_provider_conformance::{
    CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_ID, CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_VERSION,
    ControlledUserAgentCaseIdV1, ControlledUserAgentExpectedOutcomeV1,
    ControlledUserAgentFixtureV1, ControlledUserAgentUpstreamV1, ProviderCallCaseIdV1,
    ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderStreamTerminalV1,
    controlled_user_agent_fixtures_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ControlledUserAgentFixtureV1: Display);
assert_not_impl_any!(south_provider_conformance::ControlledUserAgentExpectedV1: Display);
assert_not_impl_any!(south_provider_conformance::ControlledUserAgentExpectedEvidenceV1: Display);

const SENTINELS: &[&str] = &[
    "endpoint-debug-sentinel.invalid",
    "bound-slot-debug-sentinel",
    "path-debug-sentinel",
    "header-name-debug-sentinel",
    "header-value-debug-sentinel",
    "request-body-debug-sentinel",
    "response-body-debug-sentinel",
    "controlled-user-agent-chunk-one-debug-sentinel",
    "controlled-user-agent-chunk-two-debug-sentinel",
    "content-type-debug-sentinel",
    "retry-after-debug-sentinel",
    "user-agent-value-debug-sentinel",
    "user-agent-invalid-debug-sentinel",
    "user-agent-plain-debug-sentinel",
];

#[test]
fn suite_identity_and_canonical_case_order_are_frozen() {
    assert_eq!(CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_VERSION, 1);
    assert_eq!(CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_ID, "south.controlled-user-agent.v1");

    let case_ids: Vec<_> = controlled_user_agent_fixtures_v1()
        .iter()
        .map(ControlledUserAgentFixtureV1::case_id)
        .collect();
    assert_eq!(
        case_ids,
        [
            ControlledUserAgentCaseIdV1::BufferedUserAgentSuccess,
            ControlledUserAgentCaseIdV1::StreamingUserAgentSuccess,
            ControlledUserAgentCaseIdV1::InvalidUserAgentValueRejected,
            ControlledUserAgentCaseIdV1::UserAgentFreeRequestReachesTheWire,
            ControlledUserAgentCaseIdV1::ReservedHeaderDeclarationStillRejected,
        ]
    );
}

#[test]
fn canonical_table_freezes_declarations_upstreams_outcomes_and_evidence() {
    let fixtures = controlled_user_agent_fixtures_v1();
    assert_eq!(fixtures.len(), 5);

    // 1. BufferedUserAgentSuccess: one buffered exchange declaring a sanctioned user-agent.
    assert!(fixtures[0].declared_user_agent().is_some());
    let ControlledUserAgentUpstreamV1::Response(raw) = fixtures[0].upstream() else {
        panic!("the buffered case must respond");
    };
    assert_eq!(raw.status(), 201);
    let ControlledUserAgentExpectedOutcomeV1::Response { status, body, .. } =
        fixtures[0].expected().outcome()
    else {
        panic!("the buffered case must expect a response");
    };
    assert_eq!(*status, 201);
    assert_eq!(*body, raw.body());

    // 2. StreamingUserAgentSuccess: the same declaration on the streaming path, byte-identical
    //    chunks, clean EOF.
    assert_eq!(fixtures[1].declared_user_agent(), fixtures[0].declared_user_agent());
    let ControlledUserAgentUpstreamV1::Stream(raw) = fixtures[1].upstream() else {
        panic!("the streaming case must stream");
    };
    assert_eq!(raw.chunks().len(), 2);
    assert!(matches!(raw.terminal(), ProviderStreamTerminalV1::CleanEof));
    let ControlledUserAgentExpectedOutcomeV1::Opened { status, chunks, .. } =
        fixtures[1].expected().outcome()
    else {
        panic!("the streaming case must expect an opened stream");
    };
    assert_eq!(*status, 200);
    assert_eq!(*chunks, raw.chunks());

    // 3. InvalidUserAgentValueRejected: refused before resolver and transport, and its slot is
    //    bound so that the rejection can only come from the declaration, never from a binding
    //    mismatch.
    assert_eq!(
        fixtures[2].input().requested_credential_slot(),
        fixtures[2].input().bound_credential_slot()
    );
    assert!(matches!(fixtures[2].upstream(), ControlledUserAgentUpstreamV1::NotReached));
    assert!(matches!(
        fixtures[2].expected().outcome(),
        ControlledUserAgentExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::InvalidRelativePath
        }
    ));

    // 4. UserAgentFreeRequestReachesTheWire: the only case that declares nothing and reaches the
    //    transport while expecting `wire_user_agent_exact: false`. Both halves are load-bearing.
    assert!(fixtures[3].declared_user_agent().is_none());
    assert!(matches!(fixtures[3].upstream(), ControlledUserAgentUpstreamV1::Response(_)));
    assert!(
        !fixtures[3].expected().evidence().wire_user_agent_exact(),
        "the declaration-free case is the table's only measured `false`"
    );
    assert_eq!(fixtures[3].expected().evidence().transport_calls(), ProviderCallCountV1::One);

    // 5. ReservedHeaderDeclarationStillRejected: no typed declaration, a plain `user-agent` pair
    //    in the ordinary headers, refused with zero calls. Its slot is also bound so the refusal
    //    can only come from header validation.
    assert!(fixtures[4].declared_user_agent().is_none());
    assert!(
        fixtures[4].input().headers().iter().any(|(name, _)| *name == "user-agent"),
        "the reserved-header case must smuggle the name through the ordinary channel"
    );
    assert_eq!(
        fixtures[4].input().requested_credential_slot(),
        fixtures[4].input().bound_credential_slot()
    );
    assert!(matches!(fixtures[4].upstream(), ControlledUserAgentUpstreamV1::NotReached));
    assert!(matches!(
        fixtures[4].expected().outcome(),
        ControlledUserAgentExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::RequestFailed
        }
    ));
}

/// The table must keep exactly one case that both reaches the transport and expects `false`.
///
/// This freezes the measured-probe property the controlled-query suite had to learn from a real
/// adapter that hardcoded its probe: removing `UserAgentFreeRequestReachesTheWire` or flipping its
/// expectation reopens the blind spot loudly.
#[test]
fn some_case_reaches_the_transport_and_still_expects_no_wire_user_agent() {
    let measured_false = controlled_user_agent_fixtures_v1().iter().filter(|fixture| {
        let evidence = fixture.expected().evidence();
        !evidence.wire_user_agent_exact() && evidence.transport_calls() != ProviderCallCountV1::Zero
    });

    assert_eq!(measured_false.count(), 1);
}

#[test]
fn canonical_table_freezes_the_expected_wire_user_agent_evidence() {
    let fixtures = controlled_user_agent_fixtures_v1();

    let expected_evidence = [
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        (ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, false),
        (ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false),
    ];
    // `zip` truncates silently, so a fixture added without extending the table above would go
    // unchecked rather than failing here.
    assert_eq!(fixtures.len(), expected_evidence.len());
    for (fixture, expected) in fixtures.iter().zip(expected_evidence) {
        let evidence = fixture.expected().evidence();
        assert_eq!(evidence.resolver_calls(), expected.0);
        assert_eq!(evidence.transport_calls(), expected.1);
        assert_eq!(evidence.wire_user_agent_exact(), expected.2);
    }
}

#[test]
fn every_raw_fixture_field_is_checked_through_the_production_contract() {
    for fixture in controlled_user_agent_fixtures_v1() {
        let input = fixture.input();
        ProviderEndpointV1::parse(input.endpoint()).expect("canonical endpoint must parse");
        CredentialSlotV1::parse(input.bound_credential_slot())
            .expect("canonical bound slot must parse");
        CredentialSlotV1::parse(input.requested_credential_slot())
            .expect("canonical requested slot must parse");
        RelativePathV1::parse(input.relative_path()).expect("canonical path must parse");
        JsonBodyV1::parse(input.json_body()).expect("canonical JSON must parse");

        // The reserved-header case is the one fixture whose ordinary headers deliberately violate
        // the header policy; every other fixture's headers must satisfy it, and must in particular
        // not carry `user-agent` — the typed declaration has to be the only possible source.
        let headers = SafeHeaders::try_from_iter(input.headers().iter().copied());
        if fixture.case_id() == ControlledUserAgentCaseIdV1::ReservedHeaderDeclarationStillRejected
        {
            let error =
                headers.expect_err("the reserved-header case must violate the header policy");
            assert_eq!(error.code(), "RESERVED_HEADER_FORBIDDEN");
        } else {
            headers.expect("canonical headers must parse");
            assert!(!input.headers().iter().any(|(name, _)| *name == "user-agent"));
        }

        match fixture.upstream() {
            ControlledUserAgentUpstreamV1::Response(raw) => {
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
            ControlledUserAgentUpstreamV1::Stream(raw) => {
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
            ControlledUserAgentUpstreamV1::NotReached => {}
        }
    }
}

#[test]
fn success_declarations_construct_and_the_negative_declaration_is_refused() {
    for fixture in controlled_user_agent_fixtures_v1() {
        let Some(declared) = fixture.declared_user_agent() else {
            // An absent declaration is the absence of a user-agent, not a value the contract
            // refuses, so it must never be handed to the constructor — by an executor or by this
            // test.
            assert!(matches!(
                fixture.case_id(),
                ControlledUserAgentCaseIdV1::UserAgentFreeRequestReachesTheWire
                    | ControlledUserAgentCaseIdV1::ReservedHeaderDeclarationStillRejected
            ));
            continue;
        };
        let constructed = ControlledUserAgentV1::try_from_static(declared);
        if fixture.case_id() == ControlledUserAgentCaseIdV1::InvalidUserAgentValueRejected {
            constructed.expect_err("the negative case value must violate the grammar");
        } else {
            let user_agent = constructed.expect("a success case declaration must construct");
            assert_eq!(user_agent.as_str(), declared);
        }
    }
}

#[test]
fn controlled_user_agent_and_call_suites_share_one_frozen_input_shape() {
    let user_agent_inputs: Vec<_> = controlled_user_agent_fixtures_v1()
        .iter()
        .map(|fixture| fixture.input().endpoint())
        .collect();
    let call_inputs: Vec<_> = south_provider_conformance::provider_call_fixtures_v1()
        .iter()
        .filter(|fixture| fixture.case_id() == ProviderCallCaseIdV1::Success)
        .map(|fixture| fixture.input().endpoint())
        .collect();

    assert!(user_agent_inputs.iter().all(|endpoint| *endpoint == call_inputs[0]));
}

#[test]
fn debug_output_redacts_all_raw_values() {
    for fixture in controlled_user_agent_fixtures_v1() {
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
