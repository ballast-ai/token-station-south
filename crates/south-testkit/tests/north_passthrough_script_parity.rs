//! The delivery suite's component double must answer as the real parser does.
//!
//! `south.north-passthrough-delivery.v2` replaces the component with a scripted double. Every case
//! that stands for a correct component must script what the protocol's reference parser answers,
//! frame by frame, on the case's own body. Otherwise the suite certifies a host against a component
//! that does not exist: release 0.73.0 scripted `Done` on `message_stop`, while `provider-anthropic`
//! answers `Done` on `message_delta` (design record
//! `docs/design/2026-10-17-messages-passthrough-and-count-tokens.md` §3.5).
//!
//! The cases left out script a component fault on purpose (a lagging, erring or trapping
//! component, `Done` on the wrong frame), so no correct parser answers them.
//!
//! Only `messages` is checked. The Responses (`south.north-passthrough-delivery.v1`, frozen) and
//! `generate_content` bodies are synthetic framing probes that their reference parsers refuse
//! (record §3.5); making them parser-faithful would change a frozen suite, so it is left to a
//! record of its own.

use south_component_conformance::ProviderComponentV1;
use south_component_conformance::reference_anthropic::AnthropicReferenceV1;
use south_provider_conformance::{
    NorthPassthroughCaseIdV1 as Case, NorthPassthroughFrameVerdictV1 as Verdict,
    NorthPassthroughPathV1, NorthPassthroughProtocolV2 as Protocol, north_passthrough_fixtures_v2,
    north_passthrough_frames_v1,
};
use token_station_protocol::StreamEvent;

/// The stream cases whose double stands for a correct component.
const FAITHFUL_CASES: [Case; 9] = [
    Case::ForwardedUnchanged,
    Case::ForwardAfterReturn,
    Case::TerminalWithheldUntilSettled,
    Case::UnknownContentForwardedUnchanged,
    Case::ErrorEventReplaced,
    Case::FailureBeforeFirstFrame,
    Case::TruncatedStreamFails,
    Case::SettlementFailureWithholdsTerminal,
    Case::TerminalAtEndOfInputIsRepaired,
];

/// What `provider-anthropic`'s reference parser answers for each frame of `bytes`, fed one whole
/// frame per call.
fn parsed_verdicts(bytes: &[u8]) -> Vec<Verdict> {
    let frames = north_passthrough_frames_v1(bytes).expect("the body cuts into frames");
    let mut parser = AnthropicReferenceV1.stream_parser();
    let mut verdicts = Vec::new();
    for frame in &frames {
        let verdict = match parser.parse_chunk(frame.bytes()) {
            Err(_) => Verdict::Trap,
            Ok(events) if events.iter().any(|event| matches!(event, StreamEvent::Error { .. })) => {
                Verdict::Error
            }
            Ok(events) if events.iter().any(|event| matches!(event, StreamEvent::Done { .. })) => {
                Verdict::Done
            }
            Ok(_) => Verdict::Events,
        };
        verdicts.push(verdict);
        if verdict == Verdict::Error {
            break;
        }
    }
    verdicts
}

#[test]
fn every_faithful_messages_script_answers_as_the_reference_parser_does() {
    let mut checked = 0usize;
    let mut disagreements = Vec::new();
    for fixture in north_passthrough_fixtures_v2(Protocol::Messages) {
        if !FAITHFUL_CASES.contains(&fixture.case_id()) {
            continue;
        }
        let NorthPassthroughPathV1::Stream { body, script, .. } = fixture.path() else {
            continue;
        };
        let parsed = parsed_verdicts(&body.bytes());
        let scripted: Vec<Verdict> = (0..parsed.len()).map(|index| script.frame(index)).collect();
        if parsed != scripted {
            disagreements.push(format!(
                "{:?} on {body:?}: scripted {scripted:?}, the parser answers {parsed:?}",
                fixture.case_id()
            ));
        }
        checked += 1;
    }
    // Every faithful case, the end-of-input repair twice (two bodies).
    assert_eq!(checked, FAITHFUL_CASES.len() + 1);
    assert!(disagreements.is_empty(), "{}", disagreements.join("\n"));
}
