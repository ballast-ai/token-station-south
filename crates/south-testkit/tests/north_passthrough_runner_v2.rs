//! Self-test of `south.north-passthrough-delivery.v2` (design record
//! `docs/design/2026-10-14-generate-content-request-passthrough.md` §6).
//!
//! Version two runs one protocol at a time. For Responses it must judge every host exactly as
//! version one does; for `generate_content` the reference executor passes every case, with and
//! without keep-alive comments between frames, and each deliberate mistake fails exactly the cases
//! that guard it.

use south_provider_conformance::{
    NORTH_PASSTHROUGH_DELIVERY_SUITE_ID, NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_ID,
    NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_VERSION, NORTH_PASSTHROUGH_DELIVERY_SUITE_VERSION,
    NorthPassthroughCaseIdV1 as Case, NorthPassthroughFixtureV1, NorthPassthroughHarnessV2,
    NorthPassthroughProtocolV2 as Protocol, north_passthrough_fixtures_v1,
    north_passthrough_fixtures_v2, run_north_passthrough_conformance_v1,
    run_north_passthrough_conformance_v2,
};
use south_provider_api::NorthProtocolV1;
use south_testkit::{NorthPassthroughFaultV1 as Fault, ReferenceNorthPassthroughExecutorV1};

const EVERY_GENERATE_CONTENT_CASE: [Case; 21] = [
    Case::DeliveryPassesThroughWhenEveryConditionHolds,
    Case::UndeclaredFamilyRenders,
    Case::OtherSurfaceRenders,
    Case::ForwardedUnchanged,
    Case::ForwardAfterReturn,
    Case::TerminalWithheldUntilSettled,
    Case::OtherFinishReasonIsTerminal,
    Case::FinishReasonOnAnyCandidateIsTerminal,
    Case::TerminalTypeWithoutDoneParks,
    Case::DoneWithoutTerminalTypeParks,
    Case::LaggingComponentParks,
    Case::UpstreamErrorFrameNotRelayed,
    Case::ComponentErrorForwardsNothing,
    Case::ComponentTrapForwardsNothing,
    Case::FailureBeforeFirstFrame,
    Case::TruncatedStreamFails,
    Case::SettlementFailureWithholdsTerminal,
    Case::TerminalAtEndOfInputIsRepaired,
    Case::BufferedForwardedAfterParseAndSettlement,
    Case::BufferedParseFailureNotRelayed,
    Case::BufferedNon2xxDeliveredAsSent,
];

/// Every fault the reference executor knows, version one's and version two's.
const EVERY_FAULT: [Fault; 22] = [
    Fault::ForwardBeforeReturn,
    Fault::CoalesceFrames,
    Fault::ReleaseTerminalBeforeSettlement,
    Fault::TerminalOnDoneAlone,
    Fault::TerminalOnTypeAlone,
    Fault::RelayFailureFrame,
    Fault::FailureEventBeforeFunds,
    Fault::WrongFailureSequence,
    Fault::LeakUpstreamMessage,
    Fault::ForwardTerminalWhenSettlementFails,
    Fault::NoTerminatorRepair,
    Fault::KeepAliveMidFrame,
    Fault::TruncationSettles,
    Fault::DeliveryIgnoresLossless,
    Fault::DeliveryIgnoresDeclaration,
    Fault::DeliveryIgnoresSurface,
    Fault::BufferedForwardBeforeSettlement,
    Fault::BufferedRelaysFailure,
    Fault::GenerateContentFailureFrame,
    Fault::GenerateContentNon2xxReplaced,
    Fault::GenerateContentNon2xxStatusRewritten,
    Fault::TerminalOnFirstCandidateOnly,
];

fn case_ids(fixtures: &[NorthPassthroughFixtureV1]) -> Vec<Case> {
    let mut ids: Vec<Case> = fixtures.iter().map(NorthPassthroughFixtureV1::case_id).collect();
    ids.dedup();
    ids
}

#[test]
fn north_passthrough_v2_identity_and_canonical_case_order_are_frozen() {
    assert_eq!(NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_ID, "south.north-passthrough-delivery.v2");
    assert_eq!(NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_VERSION, 2);
    assert_eq!(
        case_ids(&north_passthrough_fixtures_v2(Protocol::GenerateContent)),
        EVERY_GENERATE_CONTENT_CASE
    );
    // Version two's Responses cases are version one's, unchanged.
    assert_eq!(north_passthrough_fixtures_v2(Protocol::Responses), north_passthrough_fixtures_v1());
    assert_eq!(Protocol::ALL, [Protocol::Responses, Protocol::GenerateContent]);
    for protocol in Protocol::ALL {
        assert_eq!(Protocol::from_north_protocol(protocol.north_protocol()), Some(protocol));
    }
    for protocol in [NorthProtocolV1::ChatCompletions, NorthProtocolV1::Messages] {
        assert_eq!(Protocol::from_north_protocol(protocol), None, "{protocol:?}");
    }
}

fn failed(harness: &dyn NorthPassthroughHarnessV2, protocol: Protocol) -> Vec<Case> {
    match run_north_passthrough_conformance_v2(harness, protocol) {
        Ok(report) => {
            assert_eq!(report.suite_id(), NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_ID);
            assert_eq!(report.suite_version(), NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_VERSION);
            assert_eq!(report.protocol(), protocol);
            assert_eq!(report.passed_case_ids(), case_ids(&north_passthrough_fixtures_v2(protocol)));
            Vec::new()
        }
        Err(failure) => {
            assert_eq!(failure.suite_id(), NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_ID);
            assert_eq!(failure.protocol(), protocol);
            failure.failed_case_ids()
        }
    }
}

#[test]
fn the_reference_executor_passes_every_generate_content_passthrough_case() {
    let reference = ReferenceNorthPassthroughExecutorV1::new();
    assert_eq!(failed(&reference, Protocol::GenerateContent), []);
    assert_eq!(failed(&reference, Protocol::Responses), []);
}

/// NP-Q9 holds for `generate_content` too: comment lines between frames are not a difference,
/// and comment lines are not a frame of the host's own.
#[test]
fn generate_content_passthrough_keep_alive_comments_between_frames_pass() {
    let executor = ReferenceNorthPassthroughExecutorV1::new().with_keep_alive_at_boundaries();
    assert_eq!(failed(&executor, Protocol::GenerateContent), []);
}

/// Version one reports itself as version one for Responses, whatever version two adds.
#[test]
fn north_passthrough_v1_report_keeps_its_identity() {
    let report =
        run_north_passthrough_conformance_v1(&ReferenceNorthPassthroughExecutorV1::new()).unwrap();
    assert_eq!(report.suite_id(), NORTH_PASSTHROUGH_DELIVERY_SUITE_ID);
    assert_eq!(report.suite_version(), NORTH_PASSTHROUGH_DELIVERY_SUITE_VERSION);
    assert_eq!(report.protocol(), Protocol::Responses);
    assert_eq!(report.passed_case_ids().len(), 22);
}

/// For Responses, version two fails exactly what version one fails, for every mistake; the
/// `generate_content` mistakes fail nothing there.
#[test]
fn north_passthrough_v2_judges_responses_exactly_as_v1() {
    for fault in EVERY_FAULT {
        let executor = ReferenceNorthPassthroughExecutorV1::with_fault(fault);
        let v1 = match run_north_passthrough_conformance_v1(&executor) {
            Ok(_) => Vec::new(),
            Err(failure) => failure.failed_case_ids(),
        };
        assert_eq!(failed(&executor, Protocol::Responses), v1, "{fault:?}");
    }
    for fault in [
        Fault::GenerateContentFailureFrame,
        Fault::GenerateContentNon2xxReplaced,
        Fault::GenerateContentNon2xxStatusRewritten,
        Fault::TerminalOnFirstCandidateOnly,
    ] {
        let executor = ReferenceNorthPassthroughExecutorV1::with_fault(fault);
        assert_eq!(failed(&executor, Protocol::Responses), [], "{fault:?}");
    }
}

/// Every stream case that relays at least one frame.
const EVERY_RELAYING_STREAM_CASE: [Case; 14] = [
    Case::ForwardedUnchanged,
    Case::ForwardAfterReturn,
    Case::TerminalWithheldUntilSettled,
    Case::OtherFinishReasonIsTerminal,
    Case::FinishReasonOnAnyCandidateIsTerminal,
    Case::TerminalTypeWithoutDoneParks,
    Case::DoneWithoutTerminalTypeParks,
    Case::LaggingComponentParks,
    Case::UpstreamErrorFrameNotRelayed,
    Case::ComponentErrorForwardsNothing,
    Case::ComponentTrapForwardsNothing,
    Case::TruncatedStreamFails,
    Case::SettlementFailureWithholdsTerminal,
    Case::TerminalAtEndOfInputIsRepaired,
];

/// Every case that ends in a failure: for `generate_content`, with no frame of the host's own.
const EVERY_FAILURE_CASE: [Case; 9] = [
    Case::TerminalTypeWithoutDoneParks,
    Case::DoneWithoutTerminalTypeParks,
    Case::LaggingComponentParks,
    Case::UpstreamErrorFrameNotRelayed,
    Case::ComponentErrorForwardsNothing,
    Case::ComponentTrapForwardsNothing,
    Case::FailureBeforeFirstFrame,
    Case::TruncatedStreamFails,
    Case::SettlementFailureWithholdsTerminal,
];

#[test]
fn each_generate_content_passthrough_mistake_fails_exactly_the_cases_that_guard_it() {
    let expectations: [(Fault, &[Case]); 22] = [
        (Fault::ForwardBeforeReturn, &EVERY_RELAYING_STREAM_CASE),
        (Fault::CoalesceFrames, &EVERY_RELAYING_STREAM_CASE),
        (Fault::KeepAliveMidFrame, &EVERY_RELAYING_STREAM_CASE),
        (
            Fault::ReleaseTerminalBeforeSettlement,
            &[
                Case::ForwardedUnchanged,
                Case::ForwardAfterReturn,
                Case::TerminalWithheldUntilSettled,
                Case::OtherFinishReasonIsTerminal,
                Case::FinishReasonOnAnyCandidateIsTerminal,
                Case::SettlementFailureWithholdsTerminal,
                Case::TerminalAtEndOfInputIsRepaired,
            ],
        ),
        (Fault::TerminalOnDoneAlone, &[Case::DoneWithoutTerminalTypeParks]),
        (
            Fault::TerminalOnTypeAlone,
            &[Case::TerminalTypeWithoutDoneParks, Case::LaggingComponentParks],
        ),
        // Relaying the frame whose call returned an error writes it after the relayed frames.
        (
            Fault::RelayFailureFrame,
            &[
                Case::UpstreamErrorFrameNotRelayed,
                Case::ComponentErrorForwardsNothing,
                Case::FailureBeforeFirstFrame,
            ],
        ),
        // The Responses failure event's mistakes have nothing to act on: there is no event.
        (Fault::FailureEventBeforeFunds, &[]),
        (Fault::WrongFailureSequence, &[]),
        (Fault::LeakUpstreamMessage, &[]),
        (Fault::ForwardTerminalWhenSettlementFails, &[Case::SettlementFailureWithholdsTerminal]),
        (Fault::NoTerminatorRepair, &[Case::TerminalAtEndOfInputIsRepaired]),
        (Fault::TruncationSettles, &[Case::TruncatedStreamFails]),
        // The request is passed through too: the client's tools decide nothing.
        (Fault::DeliveryIgnoresLossless, &[]),
        (Fault::DeliveryIgnoresDeclaration, &[Case::UndeclaredFamilyRenders]),
        (Fault::DeliveryIgnoresSurface, &[Case::OtherSurfaceRenders]),
        (Fault::BufferedForwardBeforeSettlement, &[Case::BufferedForwardedAfterParseAndSettlement]),
        // A non-2xx body is delivered anyway; only the refused 2xx body must not be.
        (Fault::BufferedRelaysFailure, &[Case::BufferedParseFailureNotRelayed]),
        (Fault::GenerateContentFailureFrame, &EVERY_FAILURE_CASE),
        (Fault::GenerateContentNon2xxReplaced, &[Case::BufferedNon2xxDeliveredAsSent]),
        (Fault::GenerateContentNon2xxStatusRewritten, &[Case::BufferedNon2xxDeliveredAsSent]),
        (Fault::TerminalOnFirstCandidateOnly, &[Case::FinishReasonOnAnyCandidateIsTerminal]),
    ];
    assert_eq!(expectations.map(|(fault, _)| fault), EVERY_FAULT);
    for (fault, expected) in expectations {
        let got =
            failed(&ReferenceNorthPassthroughExecutorV1::with_fault(fault), Protocol::GenerateContent);
        assert_eq!(got, expected, "{fault:?}");
    }
}

/// A failing run's report names cases, variants and categories, never a payload.
#[test]
fn generate_content_passthrough_debug_output_carries_no_payload_text() {
    let failure = run_north_passthrough_conformance_v2(
        &ReferenceNorthPassthroughExecutorV1::with_fault(Fault::RelayFailureFrame),
        Protocol::GenerateContent,
    )
    .unwrap_err();
    let rendered = format!("{failure:?}");
    assert!(rendered.contains("Leak") && rendered.contains("GenerateContent"));
    for text in ["The model is overloaded", "The input token count", "candidates"] {
        assert!(!rendered.contains(text), "{text}");
    }
    for fixture in north_passthrough_fixtures_v2(Protocol::GenerateContent) {
        let rendered = format!("{fixture:?}");
        assert!(!rendered.contains("candidates") && !rendered.contains("data:"));
    }
}
