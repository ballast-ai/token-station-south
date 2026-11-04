//! `SseSplitterV1::push` returns the frames it completed before a refusal (stream parser
//! first-error record 2026-11-04, F2), so a parser judges them before it reports the refusal.

use south_component_conformance::sse_split::{
    MAX_SSE_SPLIT_BYTES, SseFrameV1, SsePushV1, SseSplitErrorV1, SseSplitterV1,
};

fn frame(data: &str) -> SseFrameV1 {
    SseFrameV1 { event: "message".to_owned(), data: data.to_owned() }
}

#[test]
fn a_clean_push_returns_its_frames_and_no_error() {
    let mut splitter = SseSplitterV1::new();
    assert_eq!(
        splitter.push(b"data: a\n\ndata: b\n\n"),
        SsePushV1 { frames: vec![frame("a"), frame("b")], error: None }
    );
}

#[test]
fn frames_before_a_line_that_is_not_utf8_are_returned_with_the_refusal() {
    let mut splitter = SseSplitterV1::new();
    assert_eq!(
        splitter.push(b"data: a\n\ndata: \xff\n\ndata: b\n\n"),
        SsePushV1 { frames: vec![frame("a")], error: Some(SseSplitErrorV1::NotUtf8) }
    );
}

#[test]
fn frames_before_an_oversized_tail_are_returned_with_the_refusal() {
    let mut splitter = SseSplitterV1::new();
    let mut chunk = b"data: a\n\ndata: ".to_vec();
    chunk.resize(chunk.len() + MAX_SSE_SPLIT_BYTES + 1, b'x');
    assert_eq!(
        splitter.push(&chunk),
        SsePushV1 { frames: vec![frame("a")], error: Some(SseSplitErrorV1::TooLarge) }
    );
}

#[test]
fn the_refusal_does_not_depend_on_chunking() {
    let body: &[u8] = b"data: a\n\ndata: b\n\ndata: \xff\n\n";
    let mut whole = SseSplitterV1::new();
    let whole = whole.push(body);
    let mut split = SseSplitterV1::new();
    let mut frames = Vec::new();
    let mut error = None;
    for byte in body.chunks(1) {
        let push = split.push(byte);
        frames.extend(push.frames);
        if push.error.is_some() {
            error = push.error;
            break;
        }
    }
    assert_eq!(whole, SsePushV1 { frames, error });
}
