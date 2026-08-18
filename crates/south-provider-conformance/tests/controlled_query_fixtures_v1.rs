use std::fmt::Display;

use bytes::Bytes;
use http::StatusCode;
use south_contracts::{
    BufferedHttpResponseV1, CredentialSlotV1, JsonBodyV1, MAX_STREAM_CHUNK_BYTES,
    ProviderEndpointV1, QueryParameterV1, QueryStringV1, RelativePathV1, SafeHeaders,
    StreamChunkV1, StreamingResponseHeadV1,
};
use south_provider_conformance::{
    CONTROLLED_QUERY_CONFORMANCE_SUITE_ID, CONTROLLED_QUERY_CONFORMANCE_SUITE_VERSION,
    ControlledQueryCaseIdV1, ControlledQueryExpectedOutcomeV1, ControlledQueryFixtureV1,
    ControlledQueryUpstreamV1, ProviderCallCaseIdV1, ProviderCallCountV1,
    ProviderCallFailureCodeV1, ProviderStreamTerminalV1, controlled_query_fixtures_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ControlledQueryFixtureV1: Display);
assert_not_impl_any!(south_provider_conformance::ControlledQueryExpectedV1: Display);
assert_not_impl_any!(south_provider_conformance::ControlledQueryExpectedEvidenceV1: Display);

const SENTINELS: &[&str] = &[
    "endpoint-debug-sentinel.invalid",
    "bound-slot-debug-sentinel",
    "path-debug-sentinel",
    "header-name-debug-sentinel",
    "header-value-debug-sentinel",
    "request-body-debug-sentinel",
    "response-body-debug-sentinel",
    "controlled-query-chunk-one-debug-sentinel",
    "controlled-query-chunk-two-debug-sentinel",
    "content-type-debug-sentinel",
    "retry-after-debug-sentinel",
    "invalid value-debug-sentinel",
];

#[test]
fn suite_identity_and_canonical_case_order_are_frozen() {
    assert_eq!(CONTROLLED_QUERY_CONFORMANCE_SUITE_VERSION, 1);
    assert_eq!(CONTROLLED_QUERY_CONFORMANCE_SUITE_ID, "south.controlled-query.v1");

    let case_ids: Vec<_> =
        controlled_query_fixtures_v1().iter().map(ControlledQueryFixtureV1::case_id).collect();
    assert_eq!(
        case_ids,
        [
            ControlledQueryCaseIdV1::BufferedQuerySuccess,
            ControlledQueryCaseIdV1::StreamingQuerySuccess,
            ControlledQueryCaseIdV1::InvalidQueryValueRejected,
            ControlledQueryCaseIdV1::ReversedDeclarationOrderIsCanonicalized,
            ControlledQueryCaseIdV1::QueryFreeRequestReachesTheWire,
        ]
    );
}

#[test]
fn canonical_table_freezes_queries_upstreams_outcomes_and_evidence() {
    let fixtures = controlled_query_fixtures_v1();
    assert_eq!(fixtures.len(), 5);

    // 1. BufferedQuerySuccess: one buffered exchange carrying a sanctioned `api-version`.
    let declared = fixtures[0].declared_query();
    assert_eq!(declared.len(), 1);
    assert_eq!(declared[0].0, QueryParameterV1::ApiVersion);
    let ControlledQueryUpstreamV1::Response(raw) = fixtures[0].upstream() else {
        panic!("the buffered case must respond");
    };
    assert_eq!(raw.status(), 201);
    let ControlledQueryExpectedOutcomeV1::Response { status, body, .. } =
        fixtures[0].expected().outcome()
    else {
        panic!("the buffered case must expect a response");
    };
    assert_eq!(*status, 201);
    assert_eq!(*body, raw.body());

    // 2. StreamingQuerySuccess: `alt=sse`, byte-identical chunks, clean EOF.
    let declared = fixtures[1].declared_query();
    assert_eq!(declared.len(), 1);
    assert_eq!(declared[0], (QueryParameterV1::Alt, "sse"));
    let ControlledQueryUpstreamV1::Stream(raw) = fixtures[1].upstream() else {
        panic!("the streaming case must stream");
    };
    assert_eq!(raw.chunks().len(), 2);
    assert!(matches!(raw.terminal(), ProviderStreamTerminalV1::CleanEof));
    let ControlledQueryExpectedOutcomeV1::Opened { status, chunks, .. } =
        fixtures[1].expected().outcome()
    else {
        panic!("the streaming case must expect an opened stream");
    };
    assert_eq!(*status, 200);
    assert_eq!(*chunks, raw.chunks());

    // 3. InvalidQueryValueRejected: refused before resolver and transport, and its slot is bound
    //    so that the rejection can only come from the query, never from a binding mismatch.
    assert_eq!(
        fixtures[2].input().requested_credential_slot(),
        fixtures[2].input().bound_credential_slot()
    );
    assert!(matches!(fixtures[2].upstream(), ControlledQueryUpstreamV1::NotReached));
    assert!(matches!(
        fixtures[2].expected().outcome(),
        ControlledQueryExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::InvalidRelativePath
        }
    ));

    // 4. ReversedDeclarationOrderIsCanonicalized: the only case declaring two parameters, and the
    //    only one where serialization order is observable at all. It declares them in reverse
    //    canonical order so an adapter emitting host-declaration order fails the suite.
    let declared = fixtures[3].declared_query();
    assert_eq!(declared.len(), 2);
    assert_eq!(declared[0].0, QueryParameterV1::Alt);
    assert_eq!(declared[1].0, QueryParameterV1::ApiVersion);
    let canonical = QueryStringV1::try_from_iter(declared.iter().copied())
        .expect("the reversed-order case must construct");
    assert!(
        canonical.as_str().starts_with(QueryParameterV1::ApiVersion.wire_name()),
        "canonical order must lead with api-version regardless of declaration order"
    );

    // 5. QueryFreeRequestReachesTheWire: the only case that declares nothing, and the only one
    //    that reaches the transport while expecting `wire_query_exact: false`. Both halves are
    //    load-bearing, so freeze both: an empty declaration, and a reached upstream.
    assert!(fixtures[4].declared_query().is_empty());
    assert!(matches!(fixtures[4].upstream(), ControlledQueryUpstreamV1::Response(_)));
    assert!(
        !fixtures[4].expected().evidence().wire_query_exact(),
        "the query-free case is the table's only measured `false`"
    );
    assert_eq!(fixtures[4].expected().evidence().transport_calls(), ProviderCallCountV1::One);
}

/// The table must keep exactly one case that both reaches the transport and expects `false`.
///
/// Without such a case an adapter probe that hardcodes `true` and never reads the prepared URL
/// passes the whole suite — the `true` expectations by construction, the zero-call `false` by
/// never being invoked. This test freezes the property rather than the case, so removing
/// `QueryFreeRequestReachesTheWire` or flipping its expectation reopens the blind spot loudly.
#[test]
fn some_case_reaches_the_transport_and_still_expects_no_wire_query() {
    let measured_false = controlled_query_fixtures_v1().iter().filter(|fixture| {
        let evidence = fixture.expected().evidence();
        !evidence.wire_query_exact() && evidence.transport_calls() != ProviderCallCountV1::Zero
    });

    assert_eq!(measured_false.count(), 1);
}

#[test]
fn canonical_table_freezes_the_expected_wire_query_evidence() {
    let fixtures = controlled_query_fixtures_v1();

    let expected_evidence = [
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        (ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, false),
    ];
    // `zip` truncates silently, so a fixture added without extending the table above would go
    // unchecked rather than failing here.
    assert_eq!(fixtures.len(), expected_evidence.len());
    for (fixture, expected) in fixtures.iter().zip(expected_evidence) {
        let evidence = fixture.expected().evidence();
        assert_eq!(evidence.resolver_calls(), expected.0);
        assert_eq!(evidence.transport_calls(), expected.1);
        assert_eq!(evidence.wire_query_exact(), expected.2);
    }
}

#[test]
fn every_raw_fixture_field_is_checked_through_the_production_contract() {
    for fixture in controlled_query_fixtures_v1() {
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

        // A path never carries a query: the grammar is unchanged by this slice.
        assert!(!input.relative_path().contains('?'));

        match fixture.upstream() {
            ControlledQueryUpstreamV1::Response(raw) => {
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
            ControlledQueryUpstreamV1::Stream(raw) => {
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
            ControlledQueryUpstreamV1::NotReached => {}
        }
    }
}

#[test]
fn success_queries_construct_and_the_negative_query_is_refused_by_the_contract() {
    for fixture in controlled_query_fixtures_v1() {
        // An empty declaration is the absence of a query, not a query the contract refuses. The
        // constructor has no empty representation, so a correct executor never reaches it for
        // this case — and neither does this test.
        if fixture.declared_query().is_empty() {
            assert_eq!(fixture.case_id(), ControlledQueryCaseIdV1::QueryFreeRequestReachesTheWire);
            continue;
        }
        let constructed = QueryStringV1::try_from_iter(fixture.declared_query().iter().copied());
        if fixture.case_id() == ControlledQueryCaseIdV1::InvalidQueryValueRejected {
            constructed.expect_err("the negative case value must violate its grammar");
        } else {
            let query = constructed.expect("a success case query must construct");
            // Canonical order is the `QueryParameterV1::ALL` order, not the declaration order —
            // which is exactly what the reversed-order case exists to prove, and what the
            // wire-query evidence compares against.
            let mut canonical: Vec<(QueryParameterV1, &str)> = fixture.declared_query().to_vec();
            canonical.sort_by_key(|(parameter, _)| {
                QueryParameterV1::ALL
                    .iter()
                    .position(|candidate| candidate == parameter)
                    .expect("every declared parameter must be in the sanctioned set")
            });
            let expected = canonical
                .iter()
                .map(|(parameter, value)| format!("{}={value}", parameter.wire_name()))
                .collect::<Vec<_>>()
                .join("&");
            assert_eq!(query.as_str(), expected);
        }
    }
}

#[test]
fn controlled_query_and_call_suites_share_one_frozen_input_shape() {
    let controlled_query_inputs: Vec<_> =
        controlled_query_fixtures_v1().iter().map(|fixture| fixture.input().endpoint()).collect();
    let call_inputs: Vec<_> = south_provider_conformance::provider_call_fixtures_v1()
        .iter()
        .filter(|fixture| fixture.case_id() == ProviderCallCaseIdV1::Success)
        .map(|fixture| fixture.input().endpoint())
        .collect();

    assert!(controlled_query_inputs.iter().all(|endpoint| *endpoint == call_inputs[0]));
}

#[test]
fn debug_output_redacts_all_raw_values() {
    for fixture in controlled_query_fixtures_v1() {
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
