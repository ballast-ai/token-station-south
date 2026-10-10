//! Golden vectors and properties for the SSE decoder `decode_sse_v1` (design record
//! `2026-09-30-host-zero-vendor-boundary.md`, §5.2 and §13.13; rules in the speech world record,
//! §6).
//!
//! The vectors live in `tests/vectors/sse-v1.json` so that a host can run the same table against
//! its adoption, the way the eventstream vectors are used. Every vector is checked whole, byte by
//! byte, and split at every single point, so "chunking never changes the result" is pinned on
//! each named rule and not only on random inputs.

use std::fmt::Write as _;

use proptest::{
    prelude::*,
    test_runner::{Config, RngSeed},
};
use serde_json::Value;
use south_host_grammars::{
    DEFAULT_SSE_EVENT_TYPE, MAX_SSE_EVENT_BYTES, MAX_SSE_EVENTS, MAX_SSE_LINE_BYTES, SseDecoderV1,
    SseErrorV1, SseEventV1, decode_sse_v1,
};

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

/// Feeds `chunks` to an incremental decoder, draining after every push, then finishes it.
/// Returns the events delivered before the outcome, and the outcome.
fn incremental<'a>(
    chunks: impl IntoIterator<Item = &'a [u8]>,
) -> (Vec<SseEventV1>, Result<(), SseErrorV1>) {
    let mut decoder = SseDecoderV1::new();
    let mut events = Vec::new();
    for chunk in chunks {
        decoder.push(chunk);
        loop {
            match decoder.next_event() {
                Ok(Some(event)) => events.push(event),
                Ok(None) => break,
                Err(error) => return (events, Err(error)),
            }
        }
    }
    match decoder.finish() {
        Ok(last) => {
            events.extend(last);
            (events, Ok(()))
        }
        Err(error) => (events, Err(error)),
    }
}

/// The incremental outcome in `decode_sse_v1`'s shape.
fn incremental_result<'a>(
    chunks: impl IntoIterator<Item = &'a [u8]>,
) -> Result<Vec<SseEventV1>, SseErrorV1> {
    let (events, outcome) = incremental(chunks);
    outcome.map(|()| events)
}

/// Every way of feeding `input` that the vectors are checked under: whole, one byte at a time,
/// and split once at each position.
fn assert_chunking_invariant(input: &[u8], expected: &Result<Vec<SseEventV1>, SseErrorV1>) {
    assert_eq!(&incremental_result([input]), expected, "fed whole");
    assert_eq!(&incremental_result(input.chunks(1)), expected, "fed byte by byte");
    for split in 0..=input.len() {
        let (head, tail) = input.split_at(split);
        assert_eq!(
            &incremental_result(<[&[u8]; 2]>::from((head, tail))),
            expected,
            "split at {split}"
        );
    }
}

fn error_from_name(name: &str) -> SseErrorV1 {
    match name {
        "not_utf8" => SseErrorV1::NotUtf8,
        "line_too_long" => SseErrorV1::LineTooLong,
        "event_too_large" => SseErrorV1::EventTooLarge,
        "too_many_events" => SseErrorV1::TooManyEvents,
        "undrained_event" => SseErrorV1::UndrainedEvent,
        other => panic!("unknown error name in the vectors: {other}"),
    }
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    assert!(hex.len().is_multiple_of(2), "odd hex length");
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("valid hex"))
        .collect()
}

/// Encodes events the way a well-behaved upstream would, with one chosen line terminator.
fn encode(events: &[(Option<String>, Vec<String>)], terminator: &str) -> String {
    let mut out = String::new();
    for (event, lines) in events {
        if let Some(event) = event {
            write!(out, "event: {event}{terminator}").expect("writing to a String");
        }
        for line in lines {
            write!(out, "data: {line}{terminator}").expect("writing to a String");
        }
        out.push_str(terminator);
    }
    out
}

fn reproducible_config(seed: u64) -> Config {
    // `cases` comes from `Config::default()`, which honours PROPTEST_CASES (32 for short runs).
    Config { failure_persistence: None, rng_seed: RngSeed::Fixed(seed), ..Config::default() }
}

// ---------------------------------------------------------------------------------------------
// Golden vectors
// ---------------------------------------------------------------------------------------------

#[test]
fn golden_vectors_hold_whole_and_at_every_split() {
    let vectors: Value =
        serde_json::from_str(include_str!("vectors/sse-v1.json")).expect("the vectors parse");
    let cases = vectors["cases"].as_array().expect("cases is an array");
    assert!(cases.len() >= 40, "the vector table lost cases");
    let mut names = std::collections::BTreeSet::new();
    for case in cases {
        let name = case["name"].as_str().expect("a case has a name");
        assert!(names.insert(name), "duplicate case name {name}");
        let input = match (case.get("input"), case.get("input_hex")) {
            (Some(text), None) => text.as_str().expect("input is text").as_bytes().to_vec(),
            (None, Some(hex)) => hex_bytes(hex.as_str().expect("input_hex is text")),
            _ => panic!("{name}: exactly one of input and input_hex"),
        };
        let expected = match (case.get("events"), case.get("error")) {
            (Some(events), None) => Ok(events
                .as_array()
                .expect("events is an array")
                .iter()
                .map(|event| {
                    SseEventV1::new(
                        event["event"].as_str().expect("event is text"),
                        event["data"].as_str().expect("data is text"),
                    )
                })
                .collect::<Vec<_>>()),
            (None, Some(error)) => Err(error_from_name(error.as_str().expect("error is text"))),
            _ => panic!("{name}: exactly one of events and error"),
        };
        assert_eq!(decode_sse_v1(&input), expected, "{name}: decode_sse_v1");
        assert_chunking_invariant(&input, &expected);
    }
}

// ---------------------------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------------------------

#[test]
fn a_line_at_the_limit_is_accepted_and_one_byte_more_is_refused() {
    let value_len = MAX_SSE_LINE_BYTES - "data:".len();
    let mut at_limit = b"data:".to_vec();
    at_limit.resize(MAX_SSE_LINE_BYTES, b'x');
    at_limit.extend_from_slice(b"\n\n");
    let events = decode_sse_v1(&at_limit).expect("a line of exactly the limit is accepted");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data().len(), value_len);

    let mut over = b"data:".to_vec();
    over.resize(MAX_SSE_LINE_BYTES + 1, b'x');
    assert_eq!(decode_sse_v1(&over), Err(SseErrorV1::LineTooLong), "unterminated");
    over.push(b'\n');
    assert_eq!(decode_sse_v1(&over), Err(SseErrorV1::LineTooLong), "terminated");
    assert_eq!(incremental_result(over.chunks(1 << 20)), Err(SseErrorV1::LineTooLong));
}

#[test]
fn an_unterminated_line_is_refused_as_soon_as_it_outgrows_the_limit() {
    // The decoder must not wait for a terminator that never comes before refusing.
    let mut decoder = SseDecoderV1::new();
    decoder.push(b"data: ok\n\n");
    assert_eq!(decoder.next_event(), Ok(Some(SseEventV1::new("message", "ok"))));
    decoder.push(&vec![b'x'; MAX_SSE_LINE_BYTES]);
    assert_eq!(decoder.next_event(), Ok(None));
    assert_eq!(decoder.buffered_len(), MAX_SSE_LINE_BYTES);
    decoder.push(b"x");
    assert_eq!(decoder.next_event(), Err(SseErrorV1::LineTooLong));
    assert_eq!(decoder.buffered_len(), 0, "the refused bytes are released");
}

#[test]
fn an_event_larger_than_the_limit_is_refused_across_many_lines() {
    let line = format!("data: {}\n", "y".repeat(1 << 20));
    let lines = MAX_SSE_EVENT_BYTES / (1 << 20);
    // `lines` lines of 2^20 bytes plus their joins is just over the limit.
    let body = line.repeat(lines);
    assert_eq!(decode_sse_v1(body.as_bytes()), Err(SseErrorV1::EventTooLarge));
    assert_eq!(incremental_result(body.as_bytes().chunks(4096)), Err(SseErrorV1::EventTooLarge));
    // One line fewer fits.
    let fits = line.repeat(lines - 1);
    let events = decode_sse_v1(fits.as_bytes()).expect("one line fewer fits");
    assert_eq!(events.len(), 1);
}

#[test]
fn the_event_type_counts_toward_the_event_limit() {
    let half = MAX_SSE_EVENT_BYTES / 2;
    let body = format!("event: {}\ndata: {}\n\n", "e".repeat(half), "d".repeat(half));
    assert_eq!(decode_sse_v1(body.as_bytes()), Err(SseErrorV1::EventTooLarge));
    let body = format!("data: {}\nevent: {}\n\n", "d".repeat(half), "e".repeat(half));
    assert_eq!(decode_sse_v1(body.as_bytes()), Err(SseErrorV1::EventTooLarge));
}

#[test]
fn the_whole_body_function_bounds_the_event_count() {
    let at_limit = "data\n\n".repeat(MAX_SSE_EVENTS);
    assert_eq!(decode_sse_v1(at_limit.as_bytes()).map(|events| events.len()), Ok(MAX_SSE_EVENTS));
    let over = format!("{at_limit}data\n\n");
    assert_eq!(decode_sse_v1(over.as_bytes()), Err(SseErrorV1::TooManyEvents));
    // The end-of-input dispatch counts too.
    let over_at_end = format!("{at_limit}data");
    assert_eq!(decode_sse_v1(over_at_end.as_bytes()), Err(SseErrorV1::TooManyEvents));
    // The incremental decoder hands events out one at a time and is not bounded by count.
    assert_eq!(
        incremental_result([over.as_bytes()]).map(|events| events.len()),
        Ok(MAX_SSE_EVENTS + 1)
    );
}

// ---------------------------------------------------------------------------------------------
// Incremental decoder behavior
// ---------------------------------------------------------------------------------------------

#[test]
fn errors_are_sticky_and_later_bytes_are_discarded() {
    let mut decoder = SseDecoderV1::new();
    decoder.push(b"data: \xff\n");
    assert_eq!(decoder.next_event(), Err(SseErrorV1::NotUtf8));
    decoder.push(b"data: fine\n\n");
    assert_eq!(decoder.buffered_len(), 0);
    assert_eq!(decoder.next_event(), Err(SseErrorV1::NotUtf8));
    assert_eq!(decoder.finish(), Err(SseErrorV1::NotUtf8));
}

#[test]
fn finish_refuses_an_event_the_caller_never_pulled() {
    let mut decoder = SseDecoderV1::new();
    decoder.push(b"data: a\n\n");
    assert_eq!(decoder.finish(), Err(SseErrorV1::UndrainedEvent));
}

#[test]
fn finish_dispatches_the_pending_event_and_reads_the_last_line() {
    let mut decoder = SseDecoderV1::new();
    decoder.push(b"data: a\ndata: b");
    assert_eq!(decoder.next_event(), Ok(None));
    assert_eq!(decoder.finish(), Ok(Some(SseEventV1::new(DEFAULT_SSE_EVENT_TYPE, "a\nb"))));

    let empty = SseDecoderV1::new();
    assert_eq!(empty.finish(), Ok(None));
}

#[test]
fn a_crlf_split_between_chunks_is_one_line_end() {
    let mut decoder = SseDecoderV1::new();
    decoder.push(b"data: a\r");
    assert_eq!(decoder.next_event(), Ok(None));
    assert_eq!(decoder.buffered_len(), 8, "a final CR waits for the next byte");
    assert_eq!(decoder.position(), 0);
    decoder.push(b"\n");
    assert_eq!(decoder.next_event(), Ok(None), "CR LF is one line end, not a blank line");
    assert_eq!(decoder.buffered_len(), 0);
    assert_eq!(decoder.position(), 9);
    decoder.push(b"\r\n");
    assert_eq!(decoder.next_event(), Ok(Some(SseEventV1::new("message", "a"))));
    assert_eq!(decoder.position(), 11, "the frame ends after the blank line's LF");
    assert_eq!(decoder.finish(), Ok(None));
}

#[test]
fn a_final_cr_is_read_at_finish() {
    let mut decoder = SseDecoderV1::new();
    decoder.push(b"data: a\n\r");
    assert_eq!(decoder.next_event(), Ok(None));
    assert_eq!(decoder.position(), 8);
    assert_eq!(decoder.buffered_len(), 1);
    assert_eq!(decoder.finish(), Ok(Some(SseEventV1::new("message", "a"))));
}

/// Splits `input` into its frames by feeding it in `chunks` and reading
/// [`SseDecoderV1::position`] after each event; the end of input closes the last frame.
fn frames(
    input: &[u8],
    chunk: usize,
) -> Result<Vec<(SseEventV1, std::ops::Range<usize>)>, SseErrorV1> {
    let mut decoder = SseDecoderV1::new();
    let mut frames = Vec::new();
    let mut start = 0;
    for piece in input.chunks(chunk.max(1)) {
        decoder.push(piece);
        while let Some(event) = decoder.next_event()? {
            let end = usize::try_from(decoder.position()).expect("small input");
            frames.push((event, start..end));
            start = end;
        }
    }
    let end = usize::try_from(decoder.position()).expect("small input") + decoder.buffered_len();
    assert_eq!(end, input.len(), "every byte is accounted for");
    if let Some(event) = decoder.finish()? {
        frames.push((event, start..end));
    }
    Ok(frames)
}

#[test]
fn positions_cut_the_stream_into_self_contained_frames() {
    let input =
        b"\xef\xbb\xbf: hi\r\nevent: a\r\ndata: 1\r\n\r\nid: 2\n\ndata: 2\rdata: 3\r\rdata: last";
    let expected = [
        (SseEventV1::new("a", "1"), 0..30),
        (SseEventV1::new("message", "2\n3"), 30..54),
        (SseEventV1::new("message", "last"), 54..input.len()),
    ];
    for chunk in 1..=input.len() {
        let found = frames(input, chunk).expect("valid");
        assert_eq!(found, expected, "chunk size {chunk}");
    }
    for (event, range) in expected {
        assert_eq!(decode_sse_v1(&input[range]), Ok(vec![event]));
    }
}

#[test]
fn a_bom_split_across_chunks_is_still_dropped() {
    assert_eq!(
        incremental_result([&b"\xef"[..], b"\xbb", b"\xbfdata: x\n\n"]),
        Ok(vec![SseEventV1::new("message", "x")])
    );
}

#[test]
fn the_buffer_is_released_once_drained() {
    let mut decoder = SseDecoderV1::new();
    for _ in 0..1000 {
        decoder.push(b"data: 0123456789\n\n");
        assert!(decoder.next_event().expect("valid").is_some());
        assert_eq!(decoder.next_event(), Ok(None));
        assert_eq!(decoder.buffered_len(), 0);
    }
}

#[test]
fn debug_output_never_shows_the_data() {
    let event = SseEventV1::new("response.completed", "secret-upstream-content");
    let shown = format!("{event:?}");
    assert!(shown.contains("response.completed"));
    assert!(shown.contains("data_byte_count: 23"));
    assert!(!shown.contains("secret"));
    let mut decoder = SseDecoderV1::new();
    decoder.push(b"data: secret-upstream-content");
    assert!(!format!("{decoder:?}").contains("secret"), "{decoder:?}");
}

#[test]
fn error_messages_name_the_rule_not_the_bytes() {
    for error in [
        SseErrorV1::NotUtf8,
        SseErrorV1::LineTooLong,
        SseErrorV1::EventTooLarge,
        SseErrorV1::TooManyEvents,
        SseErrorV1::UndrainedEvent,
    ] {
        let message = error.to_string();
        assert!(message.starts_with("SSE "), "{message}");
        let _: &dyn std::error::Error = &error;
    }
}

#[test]
fn into_parts_returns_both_fields() {
    let (event, data) = SseEventV1::new("e", "d").into_parts();
    assert_eq!((event.as_str(), data.as_str()), ("e", "d"));
}

// ---------------------------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------------------------

/// Bytes drawn mostly from the grammar's own tokens, so random inputs reach every rule.
fn sse_like_bytes() -> impl Strategy<Value = Vec<u8>> {
    let token = prop_oneof![
        Just(b"data".to_vec()),
        Just(b"event".to_vec()),
        Just(b"id".to_vec()),
        Just(b"retry".to_vec()),
        Just(b":".to_vec()),
        Just(b" ".to_vec()),
        Just(b"\r".to_vec()),
        Just(b"\n".to_vec()),
        Just(b"\r\n".to_vec()),
        Just(b"\xef\xbb\xbf".to_vec()),
        Just("\u{e9}".as_bytes().to_vec()),
        Just(b"\xff".to_vec()),
        "[a-z{}\"]{1,4}".prop_map(String::into_bytes),
        any::<u8>().prop_map(|byte| vec![byte]),
    ];
    proptest::collection::vec(token, 0..48).prop_map(|tokens| tokens.concat())
}

proptest! {
    #![proptest_config(reproducible_config(0x55E5_0001))]

    /// Chunking never changes the events or the first error.
    #[test]
    fn any_chunking_equals_the_whole_body(
        bytes in sse_like_bytes(),
        points in proptest::collection::vec(any::<usize>(), 0..8),
    ) {
        let whole = decode_sse_v1(&bytes);
        let mut cuts: Vec<usize> = points.iter().map(|point| point % (bytes.len() + 1)).collect();
        cuts.sort_unstable();
        let mut chunks = Vec::new();
        let mut start = 0;
        for cut in cuts {
            chunks.push(&bytes[start..cut]);
            start = cut;
        }
        chunks.push(&bytes[start..]);
        let (events, outcome) = incremental(chunks);
        match whole {
            Ok(expected) => {
                prop_assert_eq!(outcome, Ok(()));
                prop_assert_eq!(events, expected);
            }
            Err(error) => prop_assert_eq!(outcome, Err(error)),
        }
    }

    /// Positions do not depend on chunking, and every frame they cut decodes alone to its event.
    #[test]
    fn positions_cut_self_contained_frames_at_any_chunking(
        bytes in sse_like_bytes(),
        chunk in 1_usize..9,
    ) {
        if let Ok(expected) = decode_sse_v1(&bytes) {
            let whole = frames(&bytes, bytes.len()).expect("the whole body decodes");
            let chunked = frames(&bytes, chunk).expect("the chunked body decodes");
            prop_assert_eq!(&whole, &chunked);
            prop_assert_eq!(
                whole.iter().map(|(event, _)| event.clone()).collect::<Vec<_>>(),
                expected
            );
            for (event, range) in whole {
                prop_assert_eq!(decode_sse_v1(&bytes[range]), Ok(vec![event]));
            }
        }
    }

    /// Well-formed events round-trip under every line terminator.
    #[test]
    fn encoded_events_decode_to_themselves(
        events in proptest::collection::vec(
            (
                proptest::option::of("[A-Za-z][A-Za-z0-9._-]{0,15}"),
                proptest::collection::vec("[^\r\n]{0,12}", 1..4),
            ),
            0..6,
        ),
        terminator in prop_oneof![Just("\n"), Just("\r"), Just("\r\n")],
        bom in any::<bool>(),
    ) {
        let mut body = if bom { "\u{feff}".to_owned() } else { String::new() };
        body.push_str(&encode(&events, terminator));
        let expected: Vec<SseEventV1> = events
            .iter()
            .map(|(event, lines)| {
                // `encode` writes one space after the colon, which the decoder drops; a value
                // that itself starts with a space keeps it.
                SseEventV1::new(
                    event.clone().unwrap_or_else(|| DEFAULT_SSE_EVENT_TYPE.to_owned()),
                    lines.join("\n"),
                )
            })
            .collect();
        prop_assert_eq!(decode_sse_v1(body.as_bytes()), Ok(expected.clone()));
        prop_assert_eq!(incremental_result(body.as_bytes().chunks(3)), Ok(expected));
    }

    /// Arbitrary bytes never panic, and every returned event respects the limits.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        if let Ok(events) = decode_sse_v1(&bytes) {
            for event in events {
                prop_assert!(!event.event().is_empty());
                prop_assert!(event.event().len() + event.data().len() <= MAX_SSE_EVENT_BYTES);
            }
        }
        let _ = incremental(bytes.chunks(7));
    }
}
