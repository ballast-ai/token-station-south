//! The `generate_content` bodies and frame rules of `south.north-passthrough-delivery.v2` (design
//! record `docs/design/2026-10-14-generate-content-request-passthrough.md` §6). The suite's
//! self-test against the reference executor and its mutants is in `south-testkit`
//! (`tests/north_passthrough_runner_v2.rs`).

use std::path::Path;

use serde_json::json;
use south_provider_conformance::{
    NORTH_PASSTHROUGH_MESSAGES_DONE_TYPE, NORTH_PASSTHROUGH_MESSAGES_PING_TYPE,
    NorthPassthroughBodyV1 as Body, NorthPassthroughFrameV1, NorthPassthroughPathV1,
    NorthPassthroughProtocolV2 as Protocol, north_passthrough_buffered_upstream_text_v1,
    north_passthrough_fixtures_v2, north_passthrough_frames_v1,
    north_passthrough_is_terminal_data_v2,
};

fn terminal(body: Body) -> Vec<bool> {
    north_passthrough_frames_v1(&body.bytes())
        .unwrap()
        .iter()
        .map(|frame| frame.is_terminal_frame(Protocol::GenerateContent))
        .collect()
}

#[test]
fn each_generate_content_passthrough_body_cuts_into_the_frames_its_cases_count_on() {
    assert_eq!(terminal(Body::GeminiMixedFraming), [false, false, true]);
    assert_eq!(terminal(Body::GeminiText), [false, false, true]);
    assert_eq!(terminal(Body::GeminiMaxTokens), [false, true]);
    assert_eq!(terminal(Body::GeminiSecondCandidateFinishes), [false, true]);
    assert_eq!(terminal(Body::GeminiErrorFirst), [false]);
    assert_eq!(terminal(Body::GeminiErrorAfterFrames), [false, false, false]);
    // The trailing comment of a truncated stream belongs to no frame.
    assert_eq!(terminal(Body::GeminiTruncated), [false, false]);
    for body in [Body::GeminiTerminalWithoutBlankLine, Body::GeminiTerminalWithoutLineEnd] {
        let frames = north_passthrough_frames_v1(&body.bytes()).unwrap();
        assert_eq!(frames.len(), 3, "{body:?}");
        assert!(frames[2].at_end_of_input() && frames[2].has_finish_reason(), "{body:?}");
    }
    let frames = north_passthrough_frames_v1(&Body::GeminiMixedFraming.bytes()).unwrap();
    assert!(frames.iter().all(|frame| !frame.at_end_of_input()));
    // A Gemini frame has no Responses type: it is never terminal under the Responses rule.
    assert!(frames.iter().all(|frame| frame.kind().is_none() && !frame.is_terminal_type()));
    assert!(frames.iter().all(|frame| !frame.is_terminal_frame(Protocol::Responses)));
    // The frames cover the body: nothing is lost or added between them.
    assert_eq!(
        frames.iter().flat_map(|frame| frame.bytes().to_vec()).collect::<Vec<_>>(),
        Body::GeminiMixedFraming.bytes()
    );
    // The split `data` field joins into one JSON value with its text.
    assert!(frames[1].bytes().windows(b"and sunny.".len()).any(|window| window == b"and sunny."));
}

#[test]
fn the_generate_content_passthrough_terminal_rule() {
    let terminal = |data| north_passthrough_is_terminal_data_v2(Protocol::GenerateContent, &data);
    assert!(terminal(json!({"candidates": [{"finishReason": "STOP"}]})));
    assert!(terminal(json!({"candidates": [{"finishReason": "SAFETY"}]})));
    assert!(terminal(json!({"candidates": [{"index": 0}, {"finishReason": "STOP", "index": 1}]})));
    assert!(!terminal(json!({"candidates": [{"finishReason": null}]})));
    assert!(!terminal(json!({"candidates": [{"content": {"parts": [{"text": "hi"}]}}]})));
    assert!(!terminal(json!({"candidates": []})));
    assert!(!terminal(json!({"usageMetadata": {"totalTokenCount": 3}})));
    assert!(!terminal(json!({"type": "response.completed"})));
    assert!(!terminal(json!({"error": {"code": 503}})));
    let responses = |data| north_passthrough_is_terminal_data_v2(Protocol::Responses, &data);
    assert!(responses(json!({"type": "response.completed"})));
    assert!(responses(json!({"type": "response.incomplete"})));
    assert!(!responses(json!({"type": "response.failed"})));
    assert!(!responses(json!({"candidates": [{"finishReason": "STOP"}]})));
}

/// Two suite bodies are the Gemini fixture pack's own stream rows.
#[test]
fn the_copied_generate_content_passthrough_bodies_equal_the_gemini_pack_rows() {
    let pack = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../south-component-conformance/fixtures-gemini");
    for (body, row) in [
        (Body::GeminiText, "text-and-terminal"),
        (Body::GeminiMaxTokens, "eof-terminates-without-a-usage-frame"),
    ] {
        let input: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(pack.join(format!("provider.stream.{row}.input.json")))
                .unwrap(),
        )
        .unwrap();
        let joined: String = input["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|chunk| chunk.as_str().unwrap())
            .collect();
        assert_eq!(body.bytes(), joined.as_bytes(), "{row}");
    }
}

/// Each leak marker is in its body, so the leak checks can bite; a body delivered as sent has none.
#[test]
fn every_generate_content_passthrough_upstream_failure_text_is_in_its_body() {
    for body in [Body::GeminiErrorFirst, Body::GeminiErrorAfterFrames] {
        let text = body.upstream_failure_text().unwrap();
        let bytes = body.bytes();
        assert!(bytes.windows(text.len()).any(|window| window == text.as_bytes()), "{body:?}");
    }
    let mut buffered_with_text = 0;
    let mut delivered_non_2xx = 0;
    for fixture in north_passthrough_fixtures_v2(Protocol::GenerateContent) {
        if let NorthPassthroughPathV1::Buffered { status, body, forwarded, .. } = fixture.path() {
            assert!(serde_json::from_str::<serde_json::Value>(body).is_ok());
            match north_passthrough_buffered_upstream_text_v1(body) {
                Some(text) => {
                    assert!(body.contains(text));
                    assert!(!forwarded);
                    buffered_with_text += 1;
                }
                None => assert!(forwarded),
            }
            if !(200..300).contains(status) {
                assert!(forwarded, "a generate_content non-2xx answer is delivered as sent");
                delivered_non_2xx += 1;
            }
        }
    }
    assert_eq!((buffered_with_text, delivered_non_2xx), (1, 1));
}

/// Every Gemini frame's data is one JSON value, so the terminal rule reads what a host reads.
#[test]
fn every_generate_content_passthrough_frame_is_json() {
    for body in [
        Body::GeminiMixedFraming,
        Body::GeminiText,
        Body::GeminiMaxTokens,
        Body::GeminiSecondCandidateFinishes,
        Body::GeminiErrorFirst,
        Body::GeminiErrorAfterFrames,
        Body::GeminiTerminalWithoutLineEnd,
    ] {
        let frames: Vec<NorthPassthroughFrameV1> =
            north_passthrough_frames_v1(&body.bytes()).unwrap();
        for frame in frames {
            let events = south_host_grammars::decode_sse_v1(frame.bytes()).unwrap();
            assert_eq!(events.len(), 1, "{body:?}");
            assert!(
                serde_json::from_str::<serde_json::Value>(events[0].data()).is_ok(),
                "{body:?}"
            );
        }
    }
}

fn messages_terminal(body: Body) -> Vec<bool> {
    north_passthrough_frames_v1(&body.bytes())
        .unwrap()
        .iter()
        .map(|frame| frame.is_terminal_frame(Protocol::Messages))
        .collect()
}

/// The `messages` bodies (record `docs/design/2026-10-17-messages-passthrough-and-count-tokens.md`
/// §3.4) cut into the frames their cases count on.
#[test]
fn each_messages_passthrough_body_cuts_into_the_frames_its_cases_count_on() {
    assert_eq!(messages_terminal(Body::MessagesMixedFraming), [false, false, false, true]);
    assert_eq!(
        messages_terminal(Body::MessagesText),
        [false, false, false, false, false, false, true]
    );
    assert_eq!(
        messages_terminal(Body::MessagesCitationsAndUnknownBlock),
        [false, false, false, false, false, false, false, false, true]
    );
    assert_eq!(messages_terminal(Body::MessagesErrorFirst), [false]);
    assert_eq!(messages_terminal(Body::MessagesErrorAfterFrames), [false; 5]);
    assert_eq!(messages_terminal(Body::MessagesTruncated), [false, false, false]);
    for body in [Body::MessagesTerminalWithoutBlankLine, Body::MessagesTerminalWithoutLineEnd] {
        let frames = north_passthrough_frames_v1(&body.bytes()).unwrap();
        assert_eq!(frames.len(), 4, "{body:?}");
        assert!(frames[3].at_end_of_input(), "{body:?}");
        assert!(frames[3].is_terminal_frame(Protocol::Messages), "{body:?}");
    }
    let frames = north_passthrough_frames_v1(&Body::MessagesMixedFraming.bytes()).unwrap();
    assert_eq!(
        frames.iter().flat_map(|frame| frame.bytes().to_vec()).collect::<Vec<_>>(),
        Body::MessagesMixedFraming.bytes()
    );
    assert!(frames.iter().all(|frame| !frame.is_terminal_frame(Protocol::Responses)));
    assert!(frames.iter().all(|frame| !frame.is_terminal_frame(Protocol::GenerateContent)));
    let cited = Body::MessagesCitationsAndUnknownBlock.bytes();
    for marker in [&b"citations_delta"[..], &b"server_tool_use_v9"[..]] {
        assert!(cited.windows(marker.len()).any(|window| window == marker));
    }
}

/// The first frame each `messages` body withholds is its `message_delta`, on which the component
/// answers `Done` (record §3.5); the citations body has a `ping` between it and `message_stop`.
#[test]
fn each_messages_passthrough_body_withholds_from_its_done_frame() {
    let first = |body: Body| {
        let frames = north_passthrough_frames_v1(&body.bytes()).unwrap();
        let index = NorthPassthroughFrameV1::first_withheld(&frames, Protocol::Messages);
        (index, frames[index].messages_type().map(str::to_owned))
    };
    let delta = Some(NORTH_PASSTHROUGH_MESSAGES_DONE_TYPE.to_owned());
    assert_eq!(first(Body::MessagesMixedFraming), (2, delta.clone()));
    assert_eq!(first(Body::MessagesText), (5, delta.clone()));
    assert_eq!(first(Body::MessagesCitationsAndUnknownBlock), (6, delta.clone()));
    assert_eq!(first(Body::MessagesTruncated), (2, delta));
    let cited = north_passthrough_frames_v1(&Body::MessagesCitationsAndUnknownBlock.bytes()).unwrap();
    assert_eq!(cited[7].messages_type(), Some(NORTH_PASSTHROUGH_MESSAGES_PING_TYPE));
    // The other protocols withhold only their last frame.
    let frames = north_passthrough_frames_v1(&Body::MixedFraming.bytes()).unwrap();
    assert_eq!(
        NorthPassthroughFrameV1::first_withheld(&frames, Protocol::Responses),
        frames.len() - 1
    );
}

#[test]
fn the_messages_passthrough_terminal_rule() {
    let terminal = |data| north_passthrough_is_terminal_data_v2(Protocol::Messages, &data);
    assert!(terminal(json!({"type": "message_stop"})));
    assert!(!terminal(json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}})));
    assert!(!terminal(json!({"type": "error", "error": {"type": "overloaded_error"}})));
    assert!(!terminal(json!({"type": "response.completed"})));
    assert!(!terminal(json!({"candidates": [{"finishReason": "STOP"}]})));
}

/// `MessagesText` is the Anthropic fixture pack's own stream row.
#[test]
fn the_copied_messages_passthrough_body_equals_the_anthropic_pack_row() {
    let pack = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../south-component-conformance/fixtures-anthropic");
    let input: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(pack.join("provider.stream.text-and-terminal.input.json"))
            .unwrap(),
    )
    .unwrap();
    let joined: String =
        input["chunks"].as_array().unwrap().iter().map(|chunk| chunk.as_str().unwrap()).collect();
    assert_eq!(Body::MessagesText.bytes(), joined.as_bytes());
}

/// Each leak marker is in its body; every `messages` buffered body but the success is never
/// forwarded, a non-2xx one included.
#[test]
fn every_messages_passthrough_upstream_failure_text_is_in_its_body() {
    for body in [Body::MessagesErrorFirst, Body::MessagesErrorAfterFrames] {
        let text = body.upstream_failure_text().unwrap();
        let bytes = body.bytes();
        assert!(bytes.windows(text.len()).any(|window| window == text.as_bytes()), "{body:?}");
    }
    let mut withheld = 0;
    for fixture in north_passthrough_fixtures_v2(Protocol::Messages) {
        if let NorthPassthroughPathV1::Buffered { status, body, forwarded, .. } = fixture.path() {
            assert!(serde_json::from_str::<serde_json::Value>(body).is_ok());
            match north_passthrough_buffered_upstream_text_v1(body) {
                Some(text) => {
                    assert!(body.contains(text));
                    assert!(!forwarded);
                    withheld += 1;
                }
                None => assert!(*forwarded && (200..300).contains(status)),
            }
        }
    }
    assert_eq!(withheld, 2);
}
