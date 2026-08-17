use std::fmt::Display;

use bytes::Bytes;
use http::StatusCode;
use south_contracts::{
    CredentialSlotV1, JsonBodyV1, MAX_STREAM_CHUNK_BYTES, MAX_STREAM_ERROR_BODY_BYTES,
    ProviderEndpointV1, RelativePathV1, SafeHeaders, StreamChunkV1, StreamReadErrorV1,
    StreamingResponseHeadV1, TransportErrorV1,
};
use south_provider_conformance::{
    PROVIDER_STREAM_CONFORMANCE_DEADLINE_OFFSET_V1, PROVIDER_STREAM_CONFORMANCE_IDLE_TIMEOUT_V1,
    PROVIDER_STREAM_CONFORMANCE_SUITE_ID, PROVIDER_STREAM_CONFORMANCE_SUITE_VERSION,
    ProviderCallCaseIdV1, ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderStreamCaseIdV1,
    ProviderStreamControlV1, ProviderStreamExpectedOutcomeV1, ProviderStreamFixtureV1,
    ProviderStreamTerminalV1, ProviderStreamUpstreamV1, provider_stream_fixtures_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(ProviderStreamFixtureV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderStreamRawStreamV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderStreamRawRejectionV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderStreamExpectedV1: Display);
assert_not_impl_any!(south_provider_conformance::ProviderStreamExpectedEvidenceV1: Display);

const SENTINELS: &[&str] = &[
    "endpoint-debug-sentinel.invalid",
    "bound-slot-debug-sentinel",
    "path-debug-sentinel",
    "header-name-debug-sentinel",
    "header-value-debug-sentinel",
    "request-body-debug-sentinel",
    "stream-chunk-one-debug-sentinel",
    "stream-chunk-two-debug-sentinel",
    "stream-chunk-three-debug-sentinel",
    "stream-rejected-body-debug-sentinel",
    "content-type-debug-sentinel",
    "retry-after-debug-sentinel",
];

#[test]
fn suite_identity_clock_bounds_and_canonical_case_order_are_frozen() {
    assert_eq!(PROVIDER_STREAM_CONFORMANCE_SUITE_VERSION, 1);
    assert_eq!(PROVIDER_STREAM_CONFORMANCE_SUITE_ID, "south.provider-stream.v1");
    assert_eq!(PROVIDER_STREAM_CONFORMANCE_DEADLINE_OFFSET_V1.as_secs(), 1);
    assert_eq!(PROVIDER_STREAM_CONFORMANCE_IDLE_TIMEOUT_V1.as_secs(), 2);

    let case_ids: Vec<_> =
        provider_stream_fixtures_v1().iter().map(ProviderStreamFixtureV1::case_id).collect();
    assert_eq!(
        case_ids,
        [
            ProviderStreamCaseIdV1::StreamSuccess,
            ProviderStreamCaseIdV1::RejectedUpstreamStatus,
            ProviderStreamCaseIdV1::RedirectDenied,
            ProviderStreamCaseIdV1::CancelBetweenChunks,
            ProviderStreamCaseIdV1::IdleTimeoutMidStream,
            ProviderStreamCaseIdV1::DeadlineMidStream,
            ProviderStreamCaseIdV1::UpstreamBreakMidStream,
            ProviderStreamCaseIdV1::InvalidRelativePath,
            ProviderStreamCaseIdV1::ErrorBodyTooLargeIsTruncated,
        ]
    );
}

#[test]
fn canonical_table_freezes_controls_upstreams_outcomes_and_evidence() {
    let fixtures = provider_stream_fixtures_v1();
    assert_eq!(fixtures.len(), 9);

    // 1. StreamSuccess: headers-ready, three byte-identical chunks, clean EOF.
    assert!(matches!(fixtures[0].control(), ProviderStreamControlV1::Complete));
    let ProviderStreamUpstreamV1::Stream(raw) = fixtures[0].upstream() else {
        panic!("the success case must stream");
    };
    assert_eq!(raw.chunks().len(), 3);
    assert!(matches!(raw.terminal(), ProviderStreamTerminalV1::CleanEof));
    let ProviderStreamExpectedOutcomeV1::Opened { status, chunks, .. } =
        fixtures[0].expected().outcome()
    else {
        panic!("the success case must expect an opened stream");
    };
    assert_eq!(*status, 200);
    assert_eq!(*chunks, raw.chunks());

    // 2. RejectedUpstreamStatus: bounded error body, no stream object.
    assert!(matches!(fixtures[1].control(), ProviderStreamControlV1::Complete));
    let ProviderStreamUpstreamV1::Rejected(rejection) = fixtures[1].upstream() else {
        panic!("the rejected case must reject");
    };
    assert!(rejection.body().len() <= MAX_STREAM_ERROR_BODY_BYTES);
    assert!(matches!(
        fixtures[1].expected().outcome(),
        ProviderStreamExpectedOutcomeV1::Rejected { status: 429, .. }
    ));

    // 3. RedirectDenied: refused before any body pull.
    assert!(matches!(
        fixtures[2].upstream(),
        ProviderStreamUpstreamV1::TransportFailure(TransportErrorV1::RedirectDenied)
    ));
    assert!(matches!(
        fixtures[2].expected().outcome(),
        ProviderStreamExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::RedirectDenied
        }
    ));

    // 4. CancelBetweenChunks: one chunk, then a pending pull dropped by cancellation.
    assert!(matches!(fixtures[3].control(), ProviderStreamControlV1::CancelWhileChunkPending));
    let ProviderStreamUpstreamV1::Stream(raw) = fixtures[3].upstream() else {
        panic!("the cancel case must stream");
    };
    assert_eq!(raw.chunks().len(), 1);
    assert!(matches!(raw.terminal(), ProviderStreamTerminalV1::PendingForever));

    // 5. IdleTimeoutMidStream: silent upstream after chunk 1 at the virtual idle bound.
    assert!(matches!(fixtures[4].control(), ProviderStreamControlV1::AdvanceIdleWhileChunkPending));
    let ProviderStreamUpstreamV1::Stream(raw) = fixtures[4].upstream() else {
        panic!("the idle case must stream");
    };
    assert!(matches!(raw.terminal(), ProviderStreamTerminalV1::IdleStall));

    // 6. DeadlineMidStream: absolute deadline fires while a pull is pending.
    assert!(matches!(fixtures[5].control(), ProviderStreamControlV1::ExpireWhileChunkPending));
    let ProviderStreamUpstreamV1::Stream(raw) = fixtures[5].upstream() else {
        panic!("the deadline case must stream");
    };
    assert!(matches!(raw.terminal(), ProviderStreamTerminalV1::PendingForever));

    // 7. UpstreamBreakMidStream: transport error after chunk 1.
    assert!(matches!(fixtures[6].control(), ProviderStreamControlV1::Complete));
    let ProviderStreamUpstreamV1::Stream(raw) = fixtures[6].upstream() else {
        panic!("the break case must stream");
    };
    assert!(matches!(raw.terminal(), ProviderStreamTerminalV1::BreakWithReadFailure));

    // 8. InvalidRelativePath: contract parse fails before resolver and transport.
    assert!(matches!(fixtures[7].control(), ProviderStreamControlV1::Complete));
    assert!(matches!(fixtures[7].upstream(), ProviderStreamUpstreamV1::NotReached));
    assert!(matches!(
        fixtures[7].expected().outcome(),
        ProviderStreamExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::InvalidRelativePath
        }
    ));

    // 9. ErrorBodyTooLargeIsTruncated: truncated at the bound, not failed.
    assert!(matches!(fixtures[8].control(), ProviderStreamControlV1::Complete));
    let ProviderStreamUpstreamV1::Rejected(rejection) = fixtures[8].upstream() else {
        panic!("the truncation case must reject");
    };
    assert_eq!(rejection.body().len(), MAX_STREAM_ERROR_BODY_BYTES + 1);
    let ProviderStreamExpectedOutcomeV1::Rejected { body, .. } = fixtures[8].expected().outcome()
    else {
        panic!("the truncation case must expect a rejection");
    };
    assert_eq!(body.len(), MAX_STREAM_ERROR_BODY_BYTES);
    assert_eq!(*body, &rejection.body()[..MAX_STREAM_ERROR_BODY_BYTES]);
}

#[test]
fn canonical_table_freezes_the_expected_boundary_evidence() {
    let fixtures = provider_stream_fixtures_v1();

    let expected_evidence = [
        (ProviderCallCountV1::One, ProviderCallCountV1::One, false, false, 3, None),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, false, false, 0, None),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, false, false, 0, None),
        (
            ProviderCallCountV1::One,
            ProviderCallCountV1::One,
            false,
            true,
            1,
            Some(StreamReadErrorV1::StreamCancelled),
        ),
        (
            ProviderCallCountV1::One,
            ProviderCallCountV1::One,
            false,
            false,
            1,
            Some(StreamReadErrorV1::StreamIdleTimeout),
        ),
        (
            ProviderCallCountV1::One,
            ProviderCallCountV1::One,
            false,
            true,
            1,
            Some(StreamReadErrorV1::StreamDeadlineExceeded),
        ),
        (
            ProviderCallCountV1::One,
            ProviderCallCountV1::One,
            false,
            false,
            1,
            Some(StreamReadErrorV1::StreamReadFailed),
        ),
        (ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false, false, 0, None),
        (ProviderCallCountV1::One, ProviderCallCountV1::One, false, false, 0, None),
    ];
    for (fixture, expected) in fixtures.iter().zip(expected_evidence) {
        let evidence = fixture.expected().evidence();
        assert_eq!(evidence.resolver_calls(), expected.0);
        assert_eq!(evidence.transport_calls(), expected.1);
        assert_eq!(evidence.resolver_future_dropped_while_pending(), expected.2);
        assert_eq!(evidence.transport_future_dropped_while_pending(), expected.3);
        assert_eq!(evidence.chunks_pulled(), expected.4);
        assert_eq!(evidence.poststream_error_code(), expected.5);
    }
}

#[test]
fn every_raw_fixture_field_is_checked_through_the_production_contract() {
    for fixture in provider_stream_fixtures_v1() {
        let input = fixture.input();
        ProviderEndpointV1::parse(input.endpoint()).expect("canonical endpoint must parse");
        CredentialSlotV1::parse(input.bound_credential_slot())
            .expect("canonical bound slot must parse");
        CredentialSlotV1::parse(input.requested_credential_slot())
            .expect("canonical requested slot must parse");
        JsonBodyV1::parse(input.json_body()).expect("canonical JSON must parse");
        SafeHeaders::try_from_iter(input.headers().iter().copied())
            .expect("canonical headers must parse");

        let path = RelativePathV1::parse(input.relative_path());
        if fixture.case_id() == ProviderStreamCaseIdV1::InvalidRelativePath {
            path.expect_err("the invalid fixture path must fail");
        } else {
            path.expect("canonical path must parse");
        }

        match fixture.upstream() {
            ProviderStreamUpstreamV1::Stream(raw) => {
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
            ProviderStreamUpstreamV1::Rejected(rejection) => {
                let status = StatusCode::from_u16(rejection.head().status())
                    .expect("canonical status must parse");
                assert!(!status.is_success(), "a rejected fixture head must be non-2xx");
                StreamingResponseHeadV1::try_from_parts(
                    status,
                    rejection.head().content_type().map(str::to_owned),
                    rejection.head().retry_after().map(str::to_owned),
                )
                .expect("canonical rejection head must satisfy production bounds");
            }
            ProviderStreamUpstreamV1::TransportFailure(_)
            | ProviderStreamUpstreamV1::NotReached => {}
        }
    }
}

#[test]
fn stream_and_call_suites_share_one_frozen_input_shape() {
    let stream_inputs: Vec<_> =
        provider_stream_fixtures_v1().iter().map(|fixture| fixture.input().endpoint()).collect();
    let call_inputs: Vec<_> = south_provider_conformance::provider_call_fixtures_v1()
        .iter()
        .filter(|fixture| fixture.case_id() == ProviderCallCaseIdV1::Success)
        .map(|fixture| fixture.input().endpoint())
        .collect();

    assert!(stream_inputs.iter().all(|endpoint| *endpoint == call_inputs[0]));
}

#[test]
fn debug_output_redacts_all_raw_values() {
    for fixture in provider_stream_fixtures_v1() {
        assert_redacted(&format!("{fixture:?}"));
        assert_redacted(&format!("{:?}", fixture.input()));
        assert_redacted(&format!("{:?}", fixture.control()));
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
