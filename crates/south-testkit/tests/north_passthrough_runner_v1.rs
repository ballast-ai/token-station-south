//! Self-test of `south.north-passthrough-delivery.v1` (design record
//! `docs/design/2026-10-11-responses-reasoning-replay-and-passthrough-suite.md` §3, §7 items
//! 16–17).
//!
//! The suite is only worth something if a correct host passes it and each plausible wrong host
//! fails it where it should. The reference executor passes every case, with and without keep-alive
//! comments between frames (NP-Q9); each deliberate mistake fails exactly the cases that guard it.

use south_provider_conformance::{
    NORTH_PASSTHROUGH_DELIVERY_SUITE_ID, NORTH_PASSTHROUGH_DELIVERY_SUITE_VERSION,
    NorthPassthroughCaseIdV1 as Case, NorthPassthroughFixtureV1, NorthPassthroughHarnessV1,
    north_passthrough_fixtures_v1, run_north_passthrough_conformance_v1,
};
use south_testkit::{NorthPassthroughFaultV1 as Fault, ReferenceNorthPassthroughExecutorV1};

const EVERY_CASE: [Case; 22] = [
    Case::DeliveryPassesThroughWhenEveryConditionHolds,
    Case::LossyToolMapRenders,
    Case::UndeclaredFamilyRenders,
    Case::OtherSurfaceRenders,
    Case::ForwardedUnchanged,
    Case::ForwardAfterReturn,
    Case::TerminalWithheldUntilSettled,
    Case::IncompleteIsTerminal,
    Case::TerminalTypeWithoutDoneParks,
    Case::DoneWithoutTerminalTypeParks,
    Case::LaggingComponentParks,
    Case::UpstreamFailureReplaced,
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

#[test]
fn suite_identity_and_canonical_case_order_are_frozen() {
    assert_eq!(NORTH_PASSTHROUGH_DELIVERY_SUITE_ID, "south.north-passthrough-delivery.v1");
    assert_eq!(NORTH_PASSTHROUGH_DELIVERY_SUITE_VERSION, 1);
    let mut ids: Vec<Case> =
        north_passthrough_fixtures_v1().iter().map(NorthPassthroughFixtureV1::case_id).collect();
    ids.dedup();
    assert_eq!(ids, EVERY_CASE);
}

fn failed(harness: &dyn NorthPassthroughHarnessV1) -> Vec<Case> {
    match run_north_passthrough_conformance_v1(harness) {
        Ok(report) => {
            assert_eq!(report.passed_case_ids(), EVERY_CASE);
            Vec::new()
        }
        Err(failure) => failure.failed_case_ids(),
    }
}

#[test]
fn the_reference_executor_passes_every_case() {
    assert_eq!(failed(&ReferenceNorthPassthroughExecutorV1::new()), []);
}

/// NP-Q9: comment lines a host writes between frames are not a difference.
#[test]
fn keep_alive_comments_between_frames_pass() {
    let executor = ReferenceNorthPassthroughExecutorV1::new().with_keep_alive_at_boundaries();
    assert_eq!(failed(&executor), []);
}

/// Every stream case: the order of forwarding and the terminal rule are checked in each of them.
const EVERY_STREAM_CASE_BUT_THE_FIRST_FAILURE: [Case; 14] = [
    Case::ForwardedUnchanged,
    Case::ForwardAfterReturn,
    Case::TerminalWithheldUntilSettled,
    Case::IncompleteIsTerminal,
    Case::TerminalTypeWithoutDoneParks,
    Case::DoneWithoutTerminalTypeParks,
    Case::LaggingComponentParks,
    Case::UpstreamFailureReplaced,
    Case::ErrorEventReplaced,
    Case::ComponentErrorForwardsNothing,
    Case::ComponentTrapForwardsNothing,
    Case::TruncatedStreamFails,
    Case::SettlementFailureWithholdsTerminal,
    Case::TerminalAtEndOfInputIsRepaired,
];

/// Every case that ends in the host's failure event.
const EVERY_FAILURE_CASE: [Case; 10] = [
    Case::TerminalTypeWithoutDoneParks,
    Case::DoneWithoutTerminalTypeParks,
    Case::LaggingComponentParks,
    Case::UpstreamFailureReplaced,
    Case::ErrorEventReplaced,
    Case::ComponentErrorForwardsNothing,
    Case::ComponentTrapForwardsNothing,
    Case::FailureBeforeFirstFrame,
    Case::TruncatedStreamFails,
    Case::SettlementFailureWithholdsTerminal,
];

#[test]
fn each_wiring_mistake_fails_exactly_the_cases_that_guard_it() {
    let expectations: [(Fault, &[Case]); 18] = [
        // Forwarding before the call returned, coalescing frames into one call, and splitting a
        // frame with a comment break every stream that relays a frame.
        (Fault::ForwardBeforeReturn, &EVERY_STREAM_CASE_BUT_THE_FIRST_FAILURE),
        (Fault::CoalesceFrames, &EVERY_STREAM_CASE_BUT_THE_FIRST_FAILURE),
        (Fault::KeepAliveMidFrame, &EVERY_STREAM_CASE_BUT_THE_FIRST_FAILURE),
        (
            Fault::ReleaseTerminalBeforeSettlement,
            &[
                Case::ForwardedUnchanged,
                Case::ForwardAfterReturn,
                Case::TerminalWithheldUntilSettled,
                Case::IncompleteIsTerminal,
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
                Case::UpstreamFailureReplaced,
                Case::ErrorEventReplaced,
                Case::ComponentErrorForwardsNothing,
                Case::FailureBeforeFirstFrame,
            ],
        ),
        (Fault::FailureEventBeforeFunds, &EVERY_FAILURE_CASE),
        (Fault::WrongFailureSequence, &EVERY_FAILURE_CASE),
        (
            Fault::LeakUpstreamMessage,
            &[
                Case::UpstreamFailureReplaced,
                Case::ErrorEventReplaced,
                Case::FailureBeforeFirstFrame,
            ],
        ),
        (Fault::ForwardTerminalWhenSettlementFails, &[Case::SettlementFailureWithholdsTerminal]),
        (Fault::NoTerminatorRepair, &[Case::TerminalAtEndOfInputIsRepaired]),
        (Fault::TruncationSettles, &[Case::TruncatedStreamFails]),
        (Fault::DeliveryIgnoresLossless, &[Case::LossyToolMapRenders]),
        (Fault::DeliveryIgnoresDeclaration, &[Case::UndeclaredFamilyRenders]),
        (Fault::DeliveryIgnoresSurface, &[Case::OtherSurfaceRenders]),
        (Fault::BufferedForwardBeforeSettlement, &[Case::BufferedForwardedAfterParseAndSettlement]),
        (
            Fault::BufferedRelaysFailure,
            &[Case::BufferedParseFailureNotRelayed, Case::BufferedNon2xxNotRelayed],
        ),
    ];
    for (fault, expected) in expectations {
        let got = failed(&ReferenceNorthPassthroughExecutorV1::with_fault(fault));
        assert_eq!(got, expected, "{fault:?}");
    }
}

/// A failing run's report names cases, variants and categories, never a payload.
#[test]
fn debug_output_carries_no_payload_text() {
    let failure = run_north_passthrough_conformance_v1(
        &ReferenceNorthPassthroughExecutorV1::with_fault(Fault::LeakUpstreamMessage),
    )
    .unwrap_err();
    let rendered = format!("{failure:?}");
    assert!(rendered.contains("Leak"));
    for text in ["The server had an error", "Your input exceeds", "Upstream overloaded"] {
        assert!(!rendered.contains(text), "{text}");
    }
    for fixture in north_passthrough_fixtures_v1() {
        let rendered = format!("{fixture:?}");
        assert!(!rendered.contains("response.created") && !rendered.contains("data:"));
    }
}
