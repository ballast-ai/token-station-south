//! The Converse package declares `stream_framing: aws-eventstream` (B2,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §5.2): a host deframes the upstream's binary
//! frames with `south_contracts`, re-encodes each message canonically, and feeds the component. This
//! drives that whole path from real-shaped frames, including the exception and error frames a host
//! used to swallow in its own validator.

use std::path::Path;

use serde_json::json;
use south_component_conformance::ProviderComponentV1;
use south_component_conformance::reference_bedrock_converse::BedrockConverseReferenceV1;
use south_contracts::{AwsEventStreamDeframerV1, reencode_eventstream_v1};
use south_provider_api::{ComponentManifestV1, StreamFramingV1};
use token_station_protocol::{ErrorCode, StreamEvent};

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

/// One eventstream message with string headers, laid out as AWS sends it.
fn frame(headers: &[(&str, &str)], payload: &[u8]) -> Vec<u8> {
    let mut header_block = Vec::new();
    for (name, value) in headers {
        header_block.push(u8::try_from(name.len()).unwrap());
        header_block.extend_from_slice(name.as_bytes());
        header_block.push(7);
        header_block.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
        header_block.extend_from_slice(value.as_bytes());
    }
    let total = u32::try_from(12 + header_block.len() + payload.len() + 4).unwrap();
    let mut out = Vec::new();
    out.extend_from_slice(&total.to_be_bytes());
    out.extend_from_slice(&u32::try_from(header_block.len()).unwrap().to_be_bytes());
    out.extend_from_slice(&crc32(&out).to_be_bytes());
    out.extend_from_slice(&header_block);
    out.extend_from_slice(payload);
    out.extend_from_slice(&crc32(&out).to_be_bytes());
    out
}

fn event(kind: &str, payload: &serde_json::Value) -> Vec<u8> {
    frame(
        &[(":message-type", "event"), (":event-type", kind), (":content-type", "application/json")],
        serde_json::to_string_pretty(payload).unwrap().as_bytes(),
    )
}

/// What the component emits for `body`, fed as a host with the declared framing feeds it: split
/// at an awkward byte, deframed, re-encoded, one SSE frame at a time.
fn through_the_declared_framing(body: &[u8]) -> Vec<StreamEvent> {
    let mut deframer = AwsEventStreamDeframerV1::new();
    let mut parser = BedrockConverseReferenceV1.stream_parser();
    let mut events = Vec::new();
    for chunk in body.chunks(7) {
        deframer.push(chunk);
        while let Some(message) = deframer.next_message().unwrap() {
            let sse = reencode_eventstream_v1(&message).unwrap();
            events.extend(parser.parse_chunk(sse.as_bytes()).unwrap());
        }
    }
    deframer.finish().unwrap();
    events.extend(parser.finish().unwrap());
    events
}

#[test]
fn the_shipped_manifest_declares_eventstream_framing() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/provider-bedrock-converse/manifest.json");
    let manifest: ComponentManifestV1 =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(manifest.stream_framing, StreamFramingV1::AwsEventstream);
}

#[test]
fn a_whole_stream_reaches_the_component_through_the_deframer() {
    let mut body = Vec::new();
    body.extend(event("messageStart", &json!({"role": "assistant"})));
    body.extend(event(
        "contentBlockDelta",
        &json!({"contentBlockIndex": 0, "delta": {"text": "Mild."}}),
    ));
    body.extend(event("contentBlockStop", &json!({"contentBlockIndex": 0})));
    body.extend(event("messageStop", &json!({"stopReason": "end_turn"})));
    body.extend(event(
        "metadata",
        &json!({"usage": {"inputTokens": 10, "outputTokens": 2, "totalTokens": 12}}),
    ));
    let events = through_the_declared_framing(&body);
    assert!(
        matches!(events.first(), Some(StreamEvent::Delta { content, .. }) if content == "Mild.")
    );
    assert!(matches!(events.last(), Some(StreamEvent::Done { .. })), "{events:?}");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::Usage { usage } if usage.output_tokens == 2))
    );
}

#[test]
fn a_mid_stream_exception_ends_the_stream_with_its_error() {
    let mut body = Vec::new();
    body.extend(event("messageStart", &json!({"role": "assistant"})));
    body.extend(event(
        "contentBlockDelta",
        &json!({"contentBlockIndex": 0, "delta": {"text": "Mi"}}),
    ));
    body.extend(frame(
        &[(":message-type", "exception"), (":exception-type", "throttlingException")],
        br#"{"message": "Too many requests, please wait."}"#,
    ));
    // Anything after the failure is not the component's to report.
    body.extend(event(
        "contentBlockDelta",
        &json!({"contentBlockIndex": 0, "delta": {"text": "ld"}}),
    ));
    let events = through_the_declared_framing(&body);
    let Some(StreamEvent::Error { error }) = events.last() else {
        panic!("the stream must end with its error: {events:?}");
    };
    assert_eq!(error.code, ErrorCode::RateLimit);
    assert_eq!(error.provider_message.as_deref(), Some("Too many requests, please wait."));
    assert_eq!(events.iter().filter(|event| matches!(event, StreamEvent::Delta { .. })).count(), 1);
}

#[test]
fn an_error_frame_ends_the_stream_too() {
    let mut body = Vec::new();
    body.extend(event("messageStart", &json!({"role": "assistant"})));
    body.extend(frame(
        &[
            (":message-type", "error"),
            (":error-code", "InternalFailure"),
            (":error-message", "We encountered an internal error."),
        ],
        b"",
    ));
    let events = through_the_declared_framing(&body);
    let Some(StreamEvent::Error { error }) = events.last() else {
        panic!("the stream must end with its error: {events:?}");
    };
    assert_eq!(error.code, ErrorCode::UpstreamUnavailable);
    assert_eq!(error.provider_message.as_deref(), Some("We encountered an internal error."));
}
