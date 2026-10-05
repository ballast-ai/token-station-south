//! The test-only AWS eventstream encoder that builds the suite's upstream bodies.
//!
//! Production South only decodes eventstream, so the encoder lives with the fixtures. Its CRC32 is
//! a bitwise implementation, independent of the table the production decoder uses, and the unit
//! test below pins one frame as hex computed with Python's `zlib.crc32`.

/// Bitwise CRC32 (IEEE 802.3, reflected polynomial `0xEDB88320`).
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

/// One header of the string type (7). The suite's names and values are short constants, so the
/// saturating length conversions never apply.
fn string_header(name: &str, value: &str) -> Vec<u8> {
    let mut bytes = vec![u8::try_from(name.len()).unwrap_or(u8::MAX)];
    bytes.extend_from_slice(name.as_bytes());
    bytes.push(7);
    bytes.extend_from_slice(&u16::try_from(value.len()).unwrap_or(u16::MAX).to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
    bytes
}

/// One frame: prelude (total length, header length, prelude CRC), headers, payload, message CRC.
fn frame(headers: &[(&str, &str)], payload: &[u8]) -> Vec<u8> {
    let block: Vec<u8> =
        headers.iter().flat_map(|(name, value)| string_header(name, value)).collect();
    let total = u32::try_from(16 + block.len() + payload.len()).unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(16 + block.len() + payload.len());
    bytes.extend_from_slice(&total.to_be_bytes());
    bytes.extend_from_slice(&u32::try_from(block.len()).unwrap_or(u32::MAX).to_be_bytes());
    bytes.extend_from_slice(&crc32(&bytes).to_be_bytes());
    bytes.extend_from_slice(&block);
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(&crc32(&bytes).to_be_bytes());
    bytes
}

fn event(event_type: &str, payload: &str) -> Vec<u8> {
    frame(
        &[
            (":message-type", "event"),
            (":event-type", event_type),
            (":content-type", "application/json"),
        ],
        payload.as_bytes(),
    )
}

fn exception(exception_type: &str, payload: &str) -> Vec<u8> {
    frame(
        &[
            (":message-type", "exception"),
            (":exception-type", exception_type),
            (":content-type", "application/json"),
        ],
        payload.as_bytes(),
    )
}

fn error(code: &str, message: &str) -> Vec<u8> {
    frame(&[(":message-type", "error"), (":error-code", code), (":error-message", message)], b"")
}

fn message_start() -> Vec<u8> {
    event("messageStart", r#"{"role":"assistant"}"#)
}

pub(super) fn converse_stream() -> Vec<u8> {
    [
        event("messageStart", "{\r\n  \"role\" : \"assistant\"\r\n}"),
        event(
            "contentBlockDelta",
            "{\n\t\"delta\": {\"text\": \"a b\\n\\\"c\\\"\"},\n\t\"contentBlockIndex\": 0\n}",
        ),
        event("contentBlockStop", r#"{"contentBlockIndex":0}"#),
        event("messageStop", r#"{ "stopReason" : "end_turn" }"#),
        event(
            "metadata",
            r#"{"usage":{"inputTokens":12,"outputTokens":3,"totalTokens":15},"metrics":{"latencyMs":1.5e2}}"#,
        ),
    ]
    .concat()
}

pub(super) fn exception_and_error() -> Vec<u8> {
    [
        message_start(),
        exception("throttlingException", r#"{"message":"Rate exceeded"}"#),
        error("InternalFailure", r#"bad "thing""#),
    ]
    .concat()
}

pub(super) fn checksum_mismatch_mid_stream() -> Vec<u8> {
    let mut corrupt = event("contentBlockStop", r#"{"contentBlockIndex":0}"#);
    if let Some(last) = corrupt.last_mut() {
        *last ^= 0x01;
    }
    [message_start(), corrupt, event("messageStop", r#"{"stopReason":"end_turn"}"#)].concat()
}

pub(super) fn truncated_tail() -> Vec<u8> {
    let stop = event("messageStop", r#"{"stopReason":"end_turn"}"#);
    let partial = stop.get(..20).unwrap_or_default();
    [message_start().as_slice(), partial].concat()
}

pub(super) fn non_json_payload() -> Vec<u8> {
    [message_start(), event("contentBlockDelta", "not json")].concat()
}

#[cfg(test)]
mod tests {
    use super::{crc32, exception};

    /// Computed with Python's `zlib.crc32` and `struct.pack`, independently of this encoder.
    const EXCEPTION_FRAME_HEX: &str = concat!(
        "0000008c000000612b81d3340d3a6d6573736167652d74797065070009657863",
        "657074696f6e0f3a657863657074696f6e2d747970650700137468726f74746c",
        "696e67457863657074696f6e0d3a636f6e74656e742d74797065070010617070",
        "6c69636174696f6e2f6a736f6e7b226d657373616765223a2252617465206578",
        "636565646564227d203e265c",
    );

    fn hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        bytes.iter().fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
    }

    #[test]
    fn crc32_matches_the_ieee_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn exception_frame_is_pinned() {
        let frame = exception("throttlingException", r#"{"message":"Rate exceeded"}"#);
        assert_eq!(frame.len(), 140);
        assert_eq!(hex(&frame), EXCEPTION_FRAME_HEX);
    }
}
