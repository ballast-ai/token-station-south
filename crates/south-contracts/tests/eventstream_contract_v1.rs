//! Golden vectors and properties for the AWS eventstream deframer and its canonical SSE
//! re-encoding (design record `2026-09-30-host-zero-vendor-boundary.md`, §5.2 and §5.4).
//!
//! Frames are built by a test-only encoder whose CRC32 is a bitwise implementation independent of
//! the production table, and one frame is pinned as hex computed outside Rust (Python `zlib.crc32`)
//! so the encoder itself cannot drift unnoticed. The frame layout mirrors the production host's
//! own Bedrock test helpers (`frame`, `converse_frame`, `exception_frame`), so the vectors show that
//! South accepts exactly what that host decodes today.

use std::fmt::Write as _;

use proptest::{
    prelude::*,
    test_runner::{Config, RngSeed},
};
use south_contracts::{
    AwsEventStreamDeframerV1, EventStreamErrorV1, EventStreamHeaderValueV1, EventStreamMessageV1,
    MAX_EVENTSTREAM_FRAME_BYTES, MAX_EVENTSTREAM_HEADERS_BYTES, MIN_EVENTSTREAM_FRAME_BYTES,
    deframe_aws_eventstream_v1, reencode_eventstream_v1,
};

// ---------------------------------------------------------------------------------------------
// Test-only encoder
// ---------------------------------------------------------------------------------------------

/// Bitwise CRC32 (IEEE 802.3, reflected polynomial `0xEDB88320`), independent of the table the
/// production decoder uses.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn raw_header(name: &[u8], type_byte: u8, value: &[u8]) -> Vec<u8> {
    let mut bytes = vec![u8::try_from(name.len()).expect("test header name fits u8")];
    bytes.extend_from_slice(name);
    bytes.push(type_byte);
    bytes.extend_from_slice(value);
    bytes
}

fn string_header(name: &str, value: &[u8]) -> Vec<u8> {
    let mut encoded =
        u16::try_from(value.len()).expect("test header value fits u16").to_be_bytes().to_vec();
    encoded.extend_from_slice(value);
    raw_header(name.as_bytes(), 7, &encoded)
}

/// Assembles one frame from an already encoded header block, exactly like the host's `frame`.
fn frame_from_parts(header_block: &[u8], payload: &[u8]) -> Vec<u8> {
    let total = u32::try_from(16 + header_block.len() + payload.len()).expect("test frame fits");
    let mut frame = Vec::new();
    frame.extend_from_slice(&total.to_be_bytes());
    frame.extend_from_slice(
        &u32::try_from(header_block.len()).expect("test headers fit").to_be_bytes(),
    );
    frame.extend_from_slice(&crc32(&frame).to_be_bytes());
    frame.extend_from_slice(header_block);
    frame.extend_from_slice(payload);
    frame.extend_from_slice(&crc32(&frame).to_be_bytes());
    frame
}

fn frame(headers: &[(&str, &str)], payload: &[u8]) -> Vec<u8> {
    let block: Vec<u8> =
        headers.iter().flat_map(|(name, value)| string_header(name, value.as_bytes())).collect();
    frame_from_parts(&block, payload)
}

/// The host's `converse_frame` shape.
fn converse_frame(event_type: &str, payload: &str) -> Vec<u8> {
    frame(
        &[
            (":message-type", "event"),
            (":event-type", event_type),
            (":content-type", "application/json"),
        ],
        payload.as_bytes(),
    )
}

/// The host's `exception_frame` shape.
fn exception_frame(kind: &str, payload: &str) -> Vec<u8> {
    frame(
        &[
            (":message-type", "exception"),
            (":exception-type", kind),
            (":content-type", "application/json"),
        ],
        payload.as_bytes(),
    )
}

fn error_frame(code: &str, message: &str) -> Vec<u8> {
    frame(&[(":message-type", "error"), (":error-code", code), (":error-message", message)], b"")
}

/// A prelude with a valid CRC that declares the given lengths; no body follows.
fn prelude(total: u32, headers: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&total.to_be_bytes());
    bytes.extend_from_slice(&headers.to_be_bytes());
    bytes.extend_from_slice(&crc32(&bytes).to_be_bytes());
    bytes
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).expect("valid hex"))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Drivers
// ---------------------------------------------------------------------------------------------

/// What a caller observes: every message in order, then the terminal outcome (the first error,
/// or the result of `finish`).
type Observed = (Vec<EventStreamMessageV1>, Result<(), EventStreamErrorV1>);

fn observe_chunked(chunks: &[&[u8]]) -> Observed {
    let mut deframer = AwsEventStreamDeframerV1::new();
    let mut messages = Vec::new();
    for chunk in chunks {
        deframer.push(chunk);
        loop {
            match deframer.next_message() {
                Ok(Some(message)) => messages.push(message),
                Ok(None) => break,
                Err(error) => return (messages, Err(error)),
            }
        }
    }
    (messages, deframer.finish())
}

fn observe_whole(bytes: &[u8]) -> Observed {
    observe_chunked(&[bytes])
}

fn split_at_points(bytes: &[u8], points: &[usize]) -> Vec<Vec<u8>> {
    let mut points: Vec<usize> = points.iter().map(|point| point % (bytes.len() + 1)).collect();
    points.sort_unstable();
    let mut chunks = Vec::new();
    let mut start = 0;
    for point in points {
        chunks.push(bytes[start..point].to_vec());
        start = point;
    }
    chunks.push(bytes[start..].to_vec());
    chunks
}

/// Deframes a whole body and concatenates the canonical re-encoding of every message.
fn reencode_body(bytes: &[u8]) -> Result<String, EventStreamErrorV1> {
    deframe_aws_eventstream_v1(bytes)?.iter().map(reencode_eventstream_v1).collect()
}

fn first_error(bytes: &[u8]) -> EventStreamErrorV1 {
    match deframe_aws_eventstream_v1(bytes) {
        Err(error) => error,
        Ok(messages) => panic!("expected a deframing error, got {} messages", messages.len()),
    }
}

fn single_message(bytes: &[u8]) -> EventStreamMessageV1 {
    let mut messages = deframe_aws_eventstream_v1(bytes).expect("valid frame");
    assert_eq!(messages.len(), 1);
    messages.remove(0)
}

fn reencode_error(bytes: &[u8]) -> EventStreamErrorV1 {
    match reencode_eventstream_v1(&single_message(bytes)) {
        Err(error) => error,
        Ok(frame) => panic!("expected a re-encoding error, got {frame:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Golden vectors
// ---------------------------------------------------------------------------------------------

/// A Bedrock Converse `contentBlockDelta` frame in the host's `converse_frame` layout. The hex was
/// computed with Python's `zlib.crc32`, independently of both the test encoder and production.
const GOLDEN_CONVERSE_DELTA_HEX: &str = concat!(
    "000000a2000000579acad7c80d3a6d6573736167652d74797065070005657665",
    "6e740b3a6576656e742d74797065070011636f6e74656e74426c6f636b44656c",
    "74610d3a636f6e74656e742d747970650700106170706c69636174696f6e2f6a",
    "736f6e7b22636f6e74656e74426c6f636b496e646578223a302c2264656c7461",
    "223a7b2274657874223a2248656c6c6f227d2c2270223a2261626364227d9b1c",
    "fb23",
);
const GOLDEN_CONVERSE_DELTA_PAYLOAD: &str =
    r#"{"contentBlockIndex":0,"delta":{"text":"Hello"},"p":"abcd"}"#;

#[test]
fn test_encoder_crc_matches_the_ieee_check_value() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

#[test]
fn golden_converse_frame_is_pinned_and_reencodes_byte_exactly() {
    let golden = hex(GOLDEN_CONVERSE_DELTA_HEX);
    assert_eq!(golden.len(), 162);
    assert_eq!(converse_frame("contentBlockDelta", GOLDEN_CONVERSE_DELTA_PAYLOAD), golden);

    let message = single_message(&golden);
    assert_eq!(
        message.header(":event-type"),
        Some(&EventStreamHeaderValueV1::String("contentBlockDelta".to_owned()))
    );
    assert_eq!(
        message.header(":content-type"),
        Some(&EventStreamHeaderValueV1::String("application/json".to_owned()))
    );
    assert_eq!(message.payload(), GOLDEN_CONVERSE_DELTA_PAYLOAD.as_bytes());
    assert_eq!(
        reencode_eventstream_v1(&message).expect("valid event"),
        concat!(
            "event: contentBlockDelta\n",
            r#"data: {"contentBlockIndex":0,"delta":{"text":"Hello"},"p":"abcd"}"#,
            "\n\n"
        )
    );
}

/// A complete Converse stream: pretty-printed payloads (CR, LF, tabs, spaces between tokens)
/// become compact, member order and token spelling stay exactly as the upstream wrote them.
fn golden_converse_frames() -> [Vec<u8>; 5] {
    [
        converse_frame("messageStart", "{\r\n  \"role\" : \"assistant\"\r\n}"),
        converse_frame(
            "contentBlockDelta",
            "{\n\t\"delta\": {\"text\": \"a b\\n\\\"c\\\"\"},\n\t\"contentBlockIndex\": 0\n}",
        ),
        converse_frame("contentBlockStop", r#"{"contentBlockIndex":0}"#),
        converse_frame("messageStop", r#"{ "stopReason" : "end_turn" }"#),
        converse_frame(
            "metadata",
            r#"{"usage":{"inputTokens":12,"outputTokens":3,"totalTokens":15},"metrics":{"latencyMs":1.5e2}}"#,
        ),
    ]
}

fn golden_converse_stream() -> Vec<u8> {
    golden_converse_frames().concat()
}

const GOLDEN_CONVERSE_STREAM_SSE: &str = concat!(
    "event: messageStart\n",
    "data: {\"role\":\"assistant\"}\n\n",
    "event: contentBlockDelta\n",
    "data: {\"delta\":{\"text\":\"a b\\n\\\"c\\\"\"},\"contentBlockIndex\":0}\n\n",
    "event: contentBlockStop\n",
    "data: {\"contentBlockIndex\":0}\n\n",
    "event: messageStop\n",
    "data: {\"stopReason\":\"end_turn\"}\n\n",
    "event: metadata\n",
    "data: {\"usage\":{\"inputTokens\":12,\"outputTokens\":3,\"totalTokens\":15},",
    "\"metrics\":{\"latencyMs\":1.5e2}}\n\n",
);

#[test]
fn golden_converse_stream_reencodes_byte_exactly() {
    assert_eq!(reencode_body(&golden_converse_stream()).as_deref(), Ok(GOLDEN_CONVERSE_STREAM_SSE));
}

#[test]
fn golden_stream_split_at_every_byte_boundary_is_identical() {
    let stream = golden_converse_stream();
    let whole = observe_whole(&stream);
    assert_eq!(whole.0.len(), 5);
    assert_eq!(whole.1, Ok(()));
    for point in 0..=stream.len() {
        let halves = [&stream[..point], &stream[point..]];
        assert_eq!(observe_chunked(&halves), whole, "split at byte {point}");
    }
    let bytes: Vec<&[u8]> = stream.chunks(1).collect();
    assert_eq!(observe_chunked(&bytes), whole, "one byte per chunk");
}

#[test]
fn golden_exception_frame_reencodes_byte_exactly() {
    let bytes = exception_frame("throttlingException", "{ \"message\" : \"Rate exceeded\" }");
    assert_eq!(
        reencode_body(&bytes).as_deref(),
        Ok("event: exception:throttlingException\ndata: {\"message\":\"Rate exceeded\"}\n\n")
    );
}

#[test]
fn golden_error_frame_reencodes_byte_exactly_and_escapes_the_message() {
    let bytes = error_frame("InternalFailure", "bad \"thing\"\r\nhappened \\ here");
    assert_eq!(
        reencode_body(&bytes).as_deref(),
        Ok(concat!(
            "event: error:InternalFailure\n",
            r#"data: {"message":"bad \"thing\"\r\nhappened \\ here"}"#,
            "\n\n"
        ))
    );
}

#[test]
fn error_frame_payload_is_not_part_of_the_reencoding() {
    let bytes = frame(
        &[(":message-type", "error"), (":error-code", "E"), (":error-message", "m")],
        b"not json at all",
    );
    assert_eq!(
        reencode_body(&bytes).as_deref(),
        Ok("event: error:E\ndata: {\"message\":\"m\"}\n\n")
    );
}

#[test]
fn invoke_chunk_frames_pass_through_without_base64_decoding() {
    // The InvokeModel shape: the deframer is dialect-neutral, so the `bytes` envelope reaches the
    // component untouched (only whitespace would be removed).
    let bytes = frame(
        &[
            (":message-type", "event"),
            (":event-type", "chunk"),
            (":content-type", "application/json"),
        ],
        br#"{"bytes":"eyJ0eXBlIjoibWVzc2FnZV9zdG9wIn0="}"#,
    );
    assert_eq!(
        reencode_body(&bytes).as_deref(),
        Ok("event: chunk\ndata: {\"bytes\":\"eyJ0eXBlIjoibWVzc2FnZV9zdG9wIn0=\"}\n\n")
    );
}

#[test]
fn messages_before_a_corrupt_frame_are_delivered_in_order() {
    let mut corrupt = converse_frame("messageStop", "{}");
    let last = corrupt.len() - 1;
    corrupt[last] ^= 0x01;
    let stream = [converse_frame("messageStart", "{}"), corrupt].concat();
    let (messages, outcome) = observe_whole(&stream);
    assert_eq!(messages.len(), 1);
    assert_eq!(outcome, Err(EventStreamErrorV1::MessageChecksumMismatch));
}

// ---------------------------------------------------------------------------------------------
// Framing errors
// ---------------------------------------------------------------------------------------------

#[test]
fn prelude_crc_mismatch_is_rejected() {
    let mut bytes = converse_frame("messageStop", "{}");
    bytes[8] ^= 0xFF;
    assert_eq!(first_error(&bytes), EventStreamErrorV1::PreludeChecksumMismatch);

    // A corrupted length is caught by the prelude CRC before the length is trusted.
    let mut bytes = converse_frame("messageStop", "{}");
    bytes[3] ^= 0x01;
    assert_eq!(first_error(&bytes), EventStreamErrorV1::PreludeChecksumMismatch);
}

#[test]
fn prelude_errors_surface_after_twelve_bytes_without_waiting_for_the_frame() {
    let mut bytes = converse_frame("messageStop", "{}");
    bytes[8] ^= 0xFF;
    let mut deframer = AwsEventStreamDeframerV1::new();
    deframer.push(&bytes[..11]);
    assert_eq!(deframer.next_message(), Ok(None));
    deframer.push(&bytes[11..12]);
    assert_eq!(deframer.next_message(), Err(EventStreamErrorV1::PreludeChecksumMismatch));
}

#[test]
fn message_crc_mismatch_is_rejected() {
    let mut bytes = converse_frame("messageStop", r#"{"stopReason":"end_turn"}"#);
    let payload_byte = bytes.len() - 6;
    bytes[payload_byte] ^= 0x20;
    assert_eq!(first_error(&bytes), EventStreamErrorV1::MessageChecksumMismatch);

    let mut bytes = converse_frame("messageStop", "{}");
    let crc_byte = bytes.len() - 1;
    bytes[crc_byte] ^= 0x80;
    assert_eq!(first_error(&bytes), EventStreamErrorV1::MessageChecksumMismatch);
}

#[test]
fn oversized_frames_are_rejected_from_the_prelude_alone() {
    for total in [u32::try_from(MAX_EVENTSTREAM_FRAME_BYTES + 1).expect("fits"), u32::MAX] {
        let mut deframer = AwsEventStreamDeframerV1::new();
        deframer.push(&prelude(total, 0));
        assert_eq!(deframer.next_message(), Err(EventStreamErrorV1::FrameTooLarge));
    }
    // Exactly the bound is accepted and simply awaits its body.
    let mut deframer = AwsEventStreamDeframerV1::new();
    deframer.push(&prelude(u32::try_from(MAX_EVENTSTREAM_FRAME_BYTES).expect("fits"), 0));
    assert_eq!(deframer.next_message(), Ok(None));
    assert_eq!(deframer.finish(), Err(EventStreamErrorV1::TruncatedFrame));
}

#[test]
fn undersized_frames_are_rejected() {
    assert_eq!(MIN_EVENTSTREAM_FRAME_BYTES, 16);
    for total in [0, 12, 15] {
        let mut bytes = prelude(total, 0);
        bytes.extend_from_slice(&[0; 4]);
        assert_eq!(first_error(&bytes), EventStreamErrorV1::FrameTooShort, "total {total}");
    }
    // The smallest frame — no headers, no payload — is well formed.
    let empty = frame_from_parts(&[], &[]);
    assert_eq!(empty.len(), 16);
    let message = single_message(&empty);
    assert_eq!(message.headers().count(), 0);
    assert!(message.payload().is_empty());
}

#[test]
fn header_lengths_beyond_the_frame_or_the_bound_are_rejected() {
    let mut bytes = prelude(20, 5);
    bytes.extend_from_slice(&[0; 8]);
    assert_eq!(first_error(&bytes), EventStreamErrorV1::HeadersExceedFrame);

    let too_many = u32::try_from(MAX_EVENTSTREAM_HEADERS_BYTES + 1).expect("fits");
    let mut deframer = AwsEventStreamDeframerV1::new();
    deframer.push(&prelude(too_many + 16, too_many));
    assert_eq!(deframer.next_message(), Err(EventStreamErrorV1::HeadersTooLarge));
}

#[test]
fn truncated_frame_at_eof_is_reported_by_finish() {
    let bytes = converse_frame("messageStop", "{}");
    for cut in 1..bytes.len() {
        let mut deframer = AwsEventStreamDeframerV1::new();
        deframer.push(&bytes[..cut]);
        assert_eq!(deframer.next_message(), Ok(None), "cut at {cut}");
        assert_eq!(deframer.finish(), Err(EventStreamErrorV1::TruncatedFrame), "cut at {cut}");
        assert_eq!(first_error(&bytes[..cut]), EventStreamErrorV1::TruncatedFrame);
    }
    // No bytes at all is a clean end of stream.
    assert_eq!(AwsEventStreamDeframerV1::new().finish(), Ok(()));
    assert_eq!(deframe_aws_eventstream_v1(&[]), Ok(Vec::new()));
}

#[test]
fn finish_reports_a_complete_message_the_caller_never_drained() {
    let mut deframer = AwsEventStreamDeframerV1::new();
    deframer.push(&converse_frame("messageStop", "{}"));
    assert_eq!(deframer.finish(), Err(EventStreamErrorV1::UndrainedMessage));
}

#[test]
fn the_first_error_is_sticky() {
    let mut bytes = converse_frame("messageStop", "{}");
    bytes[8] ^= 0xFF;
    let mut deframer = AwsEventStreamDeframerV1::new();
    deframer.push(&bytes);
    assert_eq!(deframer.next_message(), Err(EventStreamErrorV1::PreludeChecksumMismatch));
    deframer.push(&converse_frame("messageStop", "{}"));
    assert_eq!(deframer.next_message(), Err(EventStreamErrorV1::PreludeChecksumMismatch));
    assert_eq!(deframer.buffered_len(), 0);
    assert_eq!(deframer.finish(), Err(EventStreamErrorV1::PreludeChecksumMismatch));
}

#[test]
fn buffered_bytes_never_exceed_one_partial_frame_after_draining() {
    let largest_frame = golden_converse_frames().iter().map(Vec::len).max().expect("frames");
    let stream = golden_converse_stream();
    let mut deframer = AwsEventStreamDeframerV1::new();
    for chunk in stream.chunks(7) {
        deframer.push(chunk);
        while deframer.next_message().expect("valid stream").is_some() {}
        assert!(deframer.buffered_len() < largest_frame);
    }
    assert_eq!(deframer.buffered_len(), 0);
    assert_eq!(deframer.finish(), Ok(()));
}

// ---------------------------------------------------------------------------------------------
// Header encoding
// ---------------------------------------------------------------------------------------------

#[test]
fn every_aws_header_value_type_is_decoded() {
    let uuid: [u8; 16] = core::array::from_fn(|index| u8::try_from(index).expect("small"));
    let block = [
        raw_header(b"t", 0, &[]),
        raw_header(b"f", 1, &[]),
        raw_header(b"byte", 2, &[0xFF]),
        raw_header(b"short", 3, &(-2_i16).to_be_bytes()),
        raw_header(b"int", 4, &70_000_i32.to_be_bytes()),
        raw_header(b"long", 5, &(-5_000_000_000_i64).to_be_bytes()),
        raw_header(b"bytes", 6, &[0, 3, 0xDE, 0xAD, 0x00]),
        string_header("str", "caf\u{e9}".as_bytes()),
        raw_header(b"ts", 8, &1_700_000_000_000_i64.to_be_bytes()),
        raw_header(b"id", 9, &uuid),
    ]
    .concat();
    let message = single_message(&frame_from_parts(&block, b"{}"));
    let expected = [
        ("byte", EventStreamHeaderValueV1::Byte(-1)),
        ("bytes", EventStreamHeaderValueV1::ByteArray(vec![0xDE, 0xAD, 0x00])),
        ("f", EventStreamHeaderValueV1::Bool(false)),
        ("id", EventStreamHeaderValueV1::Uuid(uuid)),
        ("int", EventStreamHeaderValueV1::Int32(70_000)),
        ("long", EventStreamHeaderValueV1::Int64(-5_000_000_000)),
        ("short", EventStreamHeaderValueV1::Int16(-2)),
        ("str", EventStreamHeaderValueV1::String("caf\u{e9}".to_owned())),
        ("t", EventStreamHeaderValueV1::Bool(true)),
        ("ts", EventStreamHeaderValueV1::Timestamp(1_700_000_000_000)),
    ];
    let decoded: Vec<_> = message.headers().map(|(name, value)| (name, value.clone())).collect();
    assert_eq!(decoded, expected);
}

#[test]
fn unknown_header_value_types_are_rejected() {
    for type_byte in [10, 0x7F, 0xFF] {
        let block = raw_header(b"x", type_byte, &[0; 16]);
        assert_eq!(
            first_error(&frame_from_parts(&block, b"{}")),
            EventStreamErrorV1::UnknownHeaderType,
            "type {type_byte}"
        );
    }
}

#[test]
fn malformed_header_blocks_are_rejected() {
    let cases: [(Vec<u8>, EventStreamErrorV1); 9] = [
        (raw_header(b"", 7, &[0, 0]), EventStreamErrorV1::EmptyHeaderName),
        (vec![5, b'a', b'b'], EventStreamErrorV1::TruncatedHeader),
        (vec![1, b'a'], EventStreamErrorV1::TruncatedHeader),
        (raw_header(b"a", 4, &[0, 0]), EventStreamErrorV1::TruncatedHeader),
        (raw_header(b"a", 7, &[0]), EventStreamErrorV1::TruncatedHeader),
        (raw_header(b"a", 7, &[0, 9, b'x']), EventStreamErrorV1::TruncatedHeader),
        (raw_header(&[0xFF], 7, &[0, 0]), EventStreamErrorV1::HeaderNotUtf8),
        (string_header("a", &[0xC3]), EventStreamErrorV1::HeaderNotUtf8),
        (
            [string_header(":event-type", b"a"), string_header(":event-type", b"b")].concat(),
            EventStreamErrorV1::DuplicateHeader,
        ),
    ];
    for (block, expected) in cases {
        assert_eq!(first_error(&frame_from_parts(&block, b"{}")), expected, "block {block:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// Re-encoding errors
// ---------------------------------------------------------------------------------------------

#[test]
fn non_json_payloads_are_rejected_never_passed_through() {
    for payload in [
        &b""[..],
        b"not json",
        b"{\"a\":1} trailing",
        b"{\"a\":1}{\"b\":2}",
        b"{\"a\":\"line\nbreak\"}",
        b"{\"a\":\"carriage\rreturn\"}",
        b"{'single':1}",
    ] {
        let bytes = frame(&[(":message-type", "event"), (":event-type", "x")], payload);
        assert_eq!(reencode_error(&bytes), EventStreamErrorV1::PayloadNotJson, "{payload:?}");
    }
    for payload in [&b"{\"a\":\"\xFF\"}"[..], b"\xEF\xBB"] {
        let bytes = frame(&[(":message-type", "event"), (":event-type", "x")], payload);
        assert_eq!(reencode_error(&bytes), EventStreamErrorV1::PayloadNotUtf8, "{payload:?}");
    }
    let bytes = frame(&[(":message-type", "exception"), (":exception-type", "x")], b"<html>");
    assert_eq!(reencode_error(&bytes), EventStreamErrorV1::PayloadNotJson);
}

#[test]
fn json_scalars_and_arrays_are_accepted_payloads() {
    for (payload, compact) in
        [("  1 ", "1"), ("[ 1 , \"a b\" ]", "[1,\"a b\"]"), ("null", "null"), ("\"s\"", "\"s\"")]
    {
        let bytes = frame(&[(":message-type", "event"), (":event-type", "x")], payload.as_bytes());
        assert_eq!(reencode_body(&bytes), Ok(format!("event: x\ndata: {compact}\n\n")));
    }
}

#[test]
fn unknown_message_types_are_rejected() {
    for message_type in ["Event", "exceptions", "", "ping"] {
        let bytes = frame(&[(":message-type", message_type), (":event-type", "x")], b"{}");
        assert_eq!(reencode_error(&bytes), EventStreamErrorV1::UnknownMessageType);
    }
}

#[test]
fn line_breaks_or_emptiness_in_event_line_headers_are_rejected() {
    for value in ["a\nb", "a\rb", "\n", "trailing\r\n", ""] {
        let event = frame(&[(":message-type", "event"), (":event-type", value)], b"{}");
        assert_eq!(
            reencode_error(&event),
            EventStreamErrorV1::InvalidEventName { header: ":event-type" }
        );
        let exception = frame(&[(":message-type", "exception"), (":exception-type", value)], b"{}");
        assert_eq!(
            reencode_error(&exception),
            EventStreamErrorV1::InvalidEventName { header: ":exception-type" }
        );
        let error = error_frame(value, "m");
        assert_eq!(
            reencode_error(&error),
            EventStreamErrorV1::InvalidEventName { header: ":error-code" }
        );
    }
}

#[test]
fn missing_required_headers_are_rejected() {
    let cases: [(Vec<u8>, &str); 5] = [
        (frame(&[(":event-type", "x")], b"{}"), ":message-type"),
        (frame(&[(":message-type", "event")], b"{}"), ":event-type"),
        (frame(&[(":message-type", "exception")], b"{}"), ":exception-type"),
        (frame(&[(":message-type", "error"), (":error-message", "m")], b""), ":error-code"),
        (frame(&[(":message-type", "error"), (":error-code", "E")], b""), ":error-message"),
    ];
    for (bytes, header) in cases {
        assert_eq!(reencode_error(&bytes), EventStreamErrorV1::MissingHeader { header });
    }
}

#[test]
fn required_headers_must_be_strings() {
    let block = [raw_header(b":message-type", 6, &[0, 5, b'e', b'v', b'e', b'n', b't'])].concat();
    let bytes = frame_from_parts(&[block, string_header(":event-type", b"x")].concat(), b"{}");
    assert_eq!(
        reencode_error(&bytes),
        EventStreamErrorV1::HeaderNotString { header: ":message-type" }
    );
    let block = [string_header(":message-type", b"event"), raw_header(b":event-type", 0, &[])];
    assert_eq!(
        reencode_error(&frame_from_parts(&block.concat(), b"{}")),
        EventStreamErrorV1::HeaderNotString { header: ":event-type" }
    );
}

#[test]
fn errors_never_echo_input() {
    let rendered = [
        EventStreamErrorV1::PayloadNotJson,
        EventStreamErrorV1::MissingHeader { header: ":event-type" },
        EventStreamErrorV1::InvalidEventName { header: ":error-code" },
    ]
    .map(|error| error.to_string());
    assert!(rendered.iter().all(|text| !text.contains("secret")));
    let message = single_message(&error_frame("Code", "secret upstream detail"));
    assert!(!format!("{message:?}").contains("secret"));
}

// ---------------------------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------------------------

fn reproducible_config(seed: u64) -> Config {
    Config {
        cases: 256,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(seed),
        ..Config::default()
    }
}

fn event_names() -> impl Strategy<Value = String> {
    "[A-Za-z][A-Za-z0-9:_-]{0,23}"
}

fn json_payloads() -> impl Strategy<Value = serde_json::Value> {
    let leaf = prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::from),
        any::<i64>().prop_map(serde_json::Value::from),
        "[ -~\\n\\r\\t\u{e9}\u{4e2d}]{0,12}".prop_map(serde_json::Value::from),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..4).prop_map(serde_json::Value::from),
            proptest::collection::btree_map("[a-z]{1,6}", inner, 0..4)
                .prop_map(|map| serde_json::Value::Object(map.into_iter().collect())),
        ]
    })
}

proptest! {
    #![proptest_config(reproducible_config(0xE5E5_0001))]

    /// Valid streams: any chunking gives the same messages, and the re-encoding is the compact
    /// form of each payload regardless of the upstream's whitespace.
    #[test]
    fn valid_streams_are_split_invariant_and_compact(
        events in proptest::collection::vec((event_names(), json_payloads(), any::<bool>()), 0..6),
        points in proptest::collection::vec(any::<usize>(), 0..8),
    ) {
        let mut stream = Vec::new();
        let mut expected = String::new();
        for (name, payload, pretty) in &events {
            let upstream = if *pretty {
                serde_json::to_string_pretty(payload).expect("serializable")
            } else {
                serde_json::to_string(payload).expect("serializable")
            };
            stream.extend(converse_frame(name, &upstream));
            let compact = serde_json::to_string(payload).expect("serializable");
            write!(expected, "event: {name}\ndata: {compact}\n\n").expect("string write");
        }
        let whole = observe_whole(&stream);
        prop_assert_eq!(whole.0.len(), events.len());
        prop_assert_eq!(&whole.1, &Ok(()));
        let chunks = split_at_points(&stream, &points);
        let chunk_refs: Vec<&[u8]> = chunks.iter().map(Vec::as_slice).collect();
        prop_assert_eq!(&observe_chunked(&chunk_refs), &whole);

        let mut reencoded = String::new();
        for message in &whole.0 {
            let frame = reencode_eventstream_v1(message).expect("valid event");
            prop_assert_eq!(frame.matches('\n').count(), 3);
            prop_assert!(!frame.contains('\r'));
            prop_assert!(frame.ends_with("\n\n"));
            reencoded.push_str(&frame);
        }
        prop_assert_eq!(reencoded, expected);
    }

    /// Arbitrary bytes never panic, and the observable outcome does not depend on chunking.
    #[test]
    fn arbitrary_bytes_are_split_invariant(
        bytes in proptest::collection::vec(any::<u8>(), 0..256),
        points in proptest::collection::vec(any::<usize>(), 0..8),
    ) {
        let whole = observe_whole(&bytes);
        let chunks = split_at_points(&bytes, &points);
        let chunk_refs: Vec<&[u8]> = chunks.iter().map(Vec::as_slice).collect();
        prop_assert_eq!(&observe_chunked(&chunk_refs), &whole);
        let body = deframe_aws_eventstream_v1(&bytes);
        match &whole.1 {
            Ok(()) => prop_assert_eq!(body, Ok(whole.0)),
            Err(error) => prop_assert_eq!(body, Err(*error)),
        }
    }

    /// A single flipped bit in a valid stream is either caught or still splits identically, and
    /// any accepted message still re-encodes to a frame with exactly its three structural breaks.
    #[test]
    fn bit_flips_are_split_invariant(
        events in proptest::collection::vec((event_names(), json_payloads()), 1..4),
        flip in any::<usize>(),
        bit in 0_u8..8,
        points in proptest::collection::vec(any::<usize>(), 0..8),
    ) {
        let mut stream: Vec<u8> = events
            .iter()
            .flat_map(|(name, payload)| converse_frame(name, &payload.to_string()))
            .collect();
        let index = flip % stream.len();
        stream[index] ^= 1 << bit;
        let whole = observe_whole(&stream);
        let chunks = split_at_points(&stream, &points);
        let chunk_refs: Vec<&[u8]> = chunks.iter().map(Vec::as_slice).collect();
        prop_assert_eq!(&observe_chunked(&chunk_refs), &whole);
        for message in &whole.0 {
            if let Ok(frame) = reencode_eventstream_v1(message) {
                prop_assert_eq!(frame.matches('\n').count(), 3);
                prop_assert!(!frame.contains('\r'));
            }
        }
    }

    /// Arbitrary header text on the event line is either refused or carried verbatim on one line.
    #[test]
    fn event_line_headers_never_break_the_frame(
        name in "[ -~\\r\\n\u{e9}]{0,16}",
        message_type in prop_oneof![Just("event"), Just("exception"), Just("error")],
        detail in "[ -~\\r\\n]{0,16}",
    ) {
        let bytes = match message_type {
            "event" => frame(&[(":message-type", "event"), (":event-type", &name)], b"{}"),
            "exception" => frame(
                &[(":message-type", "exception"), (":exception-type", &name)],
                b"{}",
            ),
            _ => error_frame(&name, &detail),
        };
        match reencode_eventstream_v1(&single_message(&bytes)) {
            Ok(frame) => {
                prop_assert!(!name.is_empty() && !name.contains(['\r', '\n']));
                prop_assert_eq!(frame.matches('\n').count(), 3);
                prop_assert!(!frame.contains('\r'));
                let first_line = frame.lines().next().unwrap_or_default();
                prop_assert!(first_line.ends_with(name.as_str()));
            }
            Err(error) => {
                prop_assert!(name.is_empty() || name.contains(['\r', '\n']));
                let invalid_name = matches!(error, EventStreamErrorV1::InvalidEventName { .. });
                prop_assert!(invalid_name);
            }
        }
    }
}
