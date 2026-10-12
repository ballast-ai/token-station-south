//! The bodies and frame rules of `south.north-passthrough-delivery.v1` (design record
//! `docs/design/2026-10-11-responses-reasoning-replay-and-passthrough-suite.md` §3.4, §3.6, §7
//! item 18). The suite's self-test against the reference executor and its mutants is in
//! `south-testkit` (`tests/north_passthrough_runner_v1.rs`).

use std::path::Path;

use south_host_grammars::SseDecoderV1;
use south_provider_conformance::{
    NorthPassthroughBodyV1 as Body, NorthPassthroughFrameV1, NorthPassthroughPathV1,
    north_passthrough_buffered_upstream_text_v1, north_passthrough_fixtures_v1,
    north_passthrough_frames_v1, north_passthrough_terminator_v1,
};

fn kinds(body: Body) -> Vec<Option<String>> {
    north_passthrough_frames_v1(&body.bytes())
        .unwrap()
        .iter()
        .map(|frame| frame.kind().map(str::to_owned))
        .collect()
}

#[test]
fn each_body_cuts_into_the_frames_its_cases_count_on() {
    let mixed = kinds(Body::MixedFraming);
    assert_eq!(
        mixed,
        [
            Some("response.created".to_owned()),
            Some("response.output_text.delta".to_owned()),
            Some("response.completed".to_owned()),
        ]
    );
    let text = kinds(Body::OpenAiText);
    assert_eq!(text.len(), 13);
    assert_eq!(text.last().unwrap().as_deref(), Some("response.completed"));
    let incomplete = kinds(Body::OpenAiIncomplete);
    assert_eq!(incomplete.len(), 9);
    assert_eq!(incomplete.last().unwrap().as_deref(), Some("response.incomplete"));
    assert_eq!(kinds(Body::OpenAiFailed).len(), 3);
    assert_eq!(kinds(Body::OpenAiErrorFirst), [Some("error".to_owned())]);
    assert_eq!(kinds(Body::ErrorEventAfterFrames).len(), 3);
    assert_eq!(kinds(Body::ErrorEventAfterFrames)[2].as_deref(), Some("error"));
    // The trailing comment of a truncated stream belongs to no frame.
    assert_eq!(kinds(Body::Truncated).len(), 2);
    for body in [Body::TerminalWithoutBlankLine, Body::TerminalWithoutLineEnd] {
        let frames = north_passthrough_frames_v1(&body.bytes()).unwrap();
        assert_eq!(frames.len(), 3, "{body:?}");
        assert!(frames[2].at_end_of_input() && frames[2].is_terminal_type(), "{body:?}");
    }
    let frames = north_passthrough_frames_v1(&Body::MixedFraming.bytes()).unwrap();
    assert!(frames.iter().all(|frame| !frame.at_end_of_input()));
    assert_eq!(
        frames.iter().map(NorthPassthroughFrameV1::sequence_number).collect::<Vec<_>>(),
        [Some(0), Some(1), Some(2)]
    );
    // The frames cover the body: nothing is lost or added between them.
    assert_eq!(
        frames.iter().flat_map(|frame| frame.bytes().to_vec()).collect::<Vec<_>>(),
        Body::MixedFraming.bytes()
    );
}

/// A host cuts incrementally; the frames do not depend on how the bytes arrive.
#[test]
fn incremental_cutting_at_every_split_gives_the_same_frames() {
    for body in [Body::MixedFraming, Body::TerminalWithoutLineEnd, Body::Truncated] {
        let bytes = body.bytes();
        let whole: Vec<Vec<u8>> = north_passthrough_frames_v1(&bytes)
            .unwrap()
            .iter()
            .map(|frame| frame.bytes().to_vec())
            .collect();
        for point in 0..=bytes.len() {
            let chunks: [&[u8]; 2] = [&bytes[..point], &bytes[point..]];
            let mut decoder = SseDecoderV1::new();
            let mut buffer = Vec::new();
            let mut start = 0usize;
            let mut frames = Vec::new();
            for chunk in chunks {
                decoder.push(chunk);
                buffer.extend_from_slice(chunk);
                while decoder.next_event().unwrap().is_some() {
                    let end = usize::try_from(decoder.position()).unwrap();
                    frames.push(buffer[start..end].to_vec());
                    start = end;
                }
            }
            if decoder.finish().unwrap().is_some() {
                frames.push(buffer[start..].to_vec());
            }
            assert_eq!(frames, whole, "{body:?} split at {point}");
        }
    }
}

#[test]
fn the_terminator_rule_of_np_q10() {
    for (frame, terminator) in [
        (&b"data: x\n\n"[..], &b""[..]),
        (b"data: x\r\n\r\n", b""),
        (b"data: x\r\r", b""),
        (b"data: x\n\r", b""),
        (b"data: x\n", b"\n"),
        (b"data: x\r\n", b"\n"),
        (b"data: x\r", b"\r"),
        (b"data: x", b"\n\n"),
    ] {
        assert_eq!(
            north_passthrough_terminator_v1(frame),
            terminator,
            "{}",
            String::from_utf8_lossy(frame)
        );
        // Repaired, the frame dispatches exactly one event without relying on end of input.
        let repaired = [frame, terminator].concat();
        let mut decoder = SseDecoderV1::new();
        decoder.push(&repaired);
        if repaired.ends_with(b"\r") {
            // A final CR may be half of a CRLF; any next byte that is not LF settles it.
            decoder.push(b":");
        }
        assert!(decoder.next_event().unwrap().is_some(), "{}", String::from_utf8_lossy(frame));
    }
}

/// Four suite bodies are the `OpenAI` Responses fixture pack's own stream rows (§3.4).
#[test]
fn the_copied_bodies_equal_the_responses_pack_rows() {
    let pack = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../south-component-conformance/fixtures-openai-responses");
    for (body, row) in [
        (Body::OpenAiText, "text"),
        (Body::OpenAiIncomplete, "incomplete"),
        (Body::OpenAiFailed, "failed"),
        (Body::OpenAiErrorFirst, "error-event-without-sequence"),
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

/// Each leak marker is in its body, so the leak checks can bite.
#[test]
fn every_upstream_failure_text_is_in_its_body() {
    for body in [Body::OpenAiFailed, Body::OpenAiErrorFirst, Body::ErrorEventAfterFrames] {
        let text = body.upstream_failure_text().unwrap();
        let bytes = body.bytes();
        assert!(bytes.windows(text.len()).any(|window| window == text.as_bytes()), "{body:?}");
    }
    let mut buffered_with_text = 0;
    for fixture in north_passthrough_fixtures_v1() {
        if let NorthPassthroughPathV1::Buffered { body, forwarded, .. } = fixture.path() {
            match north_passthrough_buffered_upstream_text_v1(body) {
                Some(text) => {
                    assert!(body.contains(text));
                    assert!(!forwarded);
                    buffered_with_text += 1;
                }
                None => assert!(forwarded),
            }
        }
    }
    assert_eq!(buffered_with_text, 2);
}
