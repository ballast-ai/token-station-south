//! Self-test of the `messages` cases of `south.north-passthrough-delivery.v2` (design record
//! `docs/design/2026-10-17-messages-passthrough-and-count-tokens.md` §3.4, corrected in §3.5): the reference executor
//! passes every case, with and without keep-alive comments between frames, and each deliberate
//! mistake fails exactly the cases that guard it.

use south_provider_conformance::{
    NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_ID, NorthPassthroughCaseIdV1 as Case,
    NorthPassthroughFixtureV1, NorthPassthroughProtocolV2 as Protocol,
    north_passthrough_fixtures_v2, run_north_passthrough_conformance_v2,
};
use south_testkit::{NorthPassthroughFaultV1 as Fault, ReferenceNorthPassthroughExecutorV1};

const EVERY_MESSAGES_CASE: [Case; 20] = [
    Case::DeliveryPassesThroughWhenEveryConditionHolds,
    Case::UndeclaredFamilyRenders,
    Case::OtherSurfaceRenders,
    Case::ForwardedUnchanged,
    Case::ForwardAfterReturn,
    Case::TerminalWithheldUntilSettled,
    Case::UnknownContentForwardedUnchanged,
    Case::TerminalTypeWithoutDoneParks,
    Case::DoneWithoutTerminalTypeParks,
    Case::LaggingComponentParks,
    Case::ErrorEventReplaced,
    Case::ComponentErrorForwardsNothing,
    Case::ComponentTrapForwardsNothing,
    Case::FailureBeforeFirstFrame,
    Case::TruncatedStreamFails,
    Case::SettlementFailureWithholdsTerminal,
    Case::TerminalAtEndOfInputIsRepaired,
    Case::BufferedForwardedAfterParseAndSettlement,
    Case::BufferedParseFailureNotRelayed,
    Case::BufferedNon2xxNotRelayed,
];

/// Every stream case that relays at least one frame.
const EVERY_RELAYING_STREAM_CASE: [Case; 13] = [
    Case::ForwardedUnchanged,
    Case::ForwardAfterReturn,
    Case::TerminalWithheldUntilSettled,
    Case::UnknownContentForwardedUnchanged,
    Case::TerminalTypeWithoutDoneParks,
    Case::DoneWithoutTerminalTypeParks,
    Case::LaggingComponentParks,
    Case::ErrorEventReplaced,
    Case::ComponentErrorForwardsNothing,
    Case::ComponentTrapForwardsNothing,
    Case::TruncatedStreamFails,
    Case::SettlementFailureWithholdsTerminal,
    Case::TerminalAtEndOfInputIsRepaired,
];

/// Every case that ends in a failure, and so in the host's own `error` event.
const EVERY_FAILURE_CASE: [Case; 9] = [
    Case::TerminalTypeWithoutDoneParks,
    Case::DoneWithoutTerminalTypeParks,
    Case::LaggingComponentParks,
    Case::ErrorEventReplaced,
    Case::ComponentErrorForwardsNothing,
    Case::ComponentTrapForwardsNothing,
    Case::FailureBeforeFirstFrame,
    Case::TruncatedStreamFails,
    Case::SettlementFailureWithholdsTerminal,
];

fn case_ids(fixtures: &[NorthPassthroughFixtureV1]) -> Vec<Case> {
    let mut ids: Vec<Case> = fixtures.iter().map(NorthPassthroughFixtureV1::case_id).collect();
    ids.dedup();
    ids
}

fn failed(executor: ReferenceNorthPassthroughExecutorV1) -> Vec<Case> {
    match run_north_passthrough_conformance_v2(&executor, Protocol::Messages) {
        Ok(report) => {
            assert_eq!(report.suite_id(), NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_ID);
            assert_eq!(report.protocol(), Protocol::Messages);
            assert_eq!(report.passed_case_ids(), EVERY_MESSAGES_CASE);
            Vec::new()
        }
        Err(failure) => {
            assert_eq!(failure.protocol(), Protocol::Messages);
            failure.failed_case_ids()
        }
    }
}

#[test]
fn the_messages_case_order_is_frozen() {
    assert_eq!(case_ids(&north_passthrough_fixtures_v2(Protocol::Messages)), EVERY_MESSAGES_CASE);
}

#[test]
fn the_reference_executor_passes_every_messages_passthrough_case() {
    assert_eq!(failed(ReferenceNorthPassthroughExecutorV1::new()), []);
    let executor = ReferenceNorthPassthroughExecutorV1::new().with_keep_alive_at_boundaries();
    assert_eq!(failed(executor), []);
}

#[test]
fn each_messages_passthrough_mistake_fails_exactly_the_cases_that_guard_it() {
    let expectations: [(Fault, &[Case]); 24] = [
        (Fault::ForwardBeforeReturn, &EVERY_RELAYING_STREAM_CASE),
        (Fault::CoalesceFrames, &EVERY_RELAYING_STREAM_CASE),
        (Fault::KeepAliveMidFrame, &EVERY_RELAYING_STREAM_CASE),
        (
            Fault::ReleaseTerminalBeforeSettlement,
            &[
                Case::ForwardedUnchanged,
                Case::ForwardAfterReturn,
                Case::TerminalWithheldUntilSettled,
                Case::UnknownContentForwardedUnchanged,
                Case::SettlementFailureWithholdsTerminal,
                Case::TerminalAtEndOfInputIsRepaired,
            ],
        ),
        (Fault::TerminalOnDoneAlone, &[Case::DoneWithoutTerminalTypeParks]),
        (
            Fault::TerminalOnTypeAlone,
            &[Case::TerminalTypeWithoutDoneParks, Case::LaggingComponentParks],
        ),
        (
            Fault::RelayFailureFrame,
            &[
                Case::ErrorEventReplaced,
                Case::ComponentErrorForwardsNothing,
                Case::FailureBeforeFirstFrame,
            ],
        ),
        (Fault::FailureEventBeforeFunds, &EVERY_FAILURE_CASE),
        // Messages events carry no sequence number.
        (Fault::WrongFailureSequence, &[]),
        (Fault::LeakUpstreamMessage, &[Case::ErrorEventReplaced, Case::FailureBeforeFirstFrame]),
        (Fault::ForwardTerminalWhenSettlementFails, &[Case::SettlementFailureWithholdsTerminal]),
        (Fault::NoTerminatorRepair, &[Case::TerminalAtEndOfInputIsRepaired]),
        (Fault::TruncationSettles, &[Case::TruncatedStreamFails]),
        // The request is passed through too: the client's tools decide nothing.
        (Fault::DeliveryIgnoresLossless, &[]),
        (Fault::DeliveryIgnoresDeclaration, &[Case::UndeclaredFamilyRenders]),
        (Fault::DeliveryIgnoresSurface, &[Case::OtherSurfaceRenders]),
        (Fault::BufferedForwardBeforeSettlement, &[Case::BufferedForwardedAfterParseAndSettlement]),
        (
            Fault::BufferedRelaysFailure,
            &[Case::BufferedParseFailureNotRelayed, Case::BufferedNon2xxNotRelayed],
        ),
        (Fault::GenerateContentFailureFrame, &[]),
        (Fault::GenerateContentNon2xxReplaced, &[]),
        (Fault::GenerateContentNon2xxStatusRewritten, &[]),
        (Fault::TerminalOnFirstCandidateOnly, &[]),
        (Fault::MessagesNoFailureEvent, &EVERY_FAILURE_CASE),
        // Every case that reaches a `Done` on `message_delta` (record §3.5).
        (
            Fault::MessagesForwardDoneFrame,
            &[
                Case::ForwardedUnchanged,
                Case::ForwardAfterReturn,
                Case::TerminalWithheldUntilSettled,
                Case::UnknownContentForwardedUnchanged,
                Case::TruncatedStreamFails,
                Case::SettlementFailureWithholdsTerminal,
                Case::TerminalAtEndOfInputIsRepaired,
            ],
        ),
    ];
    for (fault, expected) in expectations {
        let got = failed(ReferenceNorthPassthroughExecutorV1::with_fault(fault));
        assert_eq!(got, expected, "{fault:?}");
    }
}

/// A failing run's report names cases, variants and categories, never a payload.
#[test]
fn messages_passthrough_debug_output_carries_no_payload_text() {
    let failure = run_north_passthrough_conformance_v2(
        &ReferenceNorthPassthroughExecutorV1::with_fault(Fault::LeakUpstreamMessage),
        Protocol::Messages,
    )
    .unwrap_err();
    let rendered = format!("{failure:?}");
    assert!(rendered.contains("Leak") && rendered.contains("Messages"));
    for text in ["prompt is too long", "Overloaded", "message_start"] {
        assert!(!rendered.contains(text), "{text}");
    }
}
