//! The AWS eventstream deframer and its canonical SSE re-encoding.
//!
//! A package that declares `stream_framing: aws-eventstream` has its upstream body deframed here
//! and each message handed to `parse-stream-chunk` as one canonical SSE frame (design record
//! `2026-09-30-host-zero-vendor-boundary.md`, §5.2). Both hosts call these functions, so the split
//! and the bytes the component sees are identical everywhere; the golden vectors in
//! `tests/eventstream_contract_v1.rs` pin them (§5.4).
//!
//! The deframer is pure and bounded: no I/O, no clock, at most one frame of
//! [`MAX_EVENTSTREAM_FRAME_BYTES`] held back, and every length is validated against the prelude
//! CRC before it is trusted. It knows nothing about any dialect: it does not interpret event
//! names, unwrap envelopes or decode base64.

use std::{
    collections::{BTreeMap, btree_map::Entry},
    fmt,
};

use serde::de::IgnoredAny;
use thiserror::Error;

/// The largest accepted eventstream frame, prelude and trailing CRC included.
///
/// The AWS SDK decoders and the adopting host use the same 16 MiB bound. It is checked from the
/// 12-byte prelude alone, so a corrupt or hostile length never causes more than one prelude to be
/// buffered.
pub const MAX_EVENTSTREAM_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// The smallest well-formed frame: a 12-byte prelude and the 4-byte message CRC, with no headers
/// and no payload.
pub const MIN_EVENTSTREAM_FRAME_BYTES: usize = PRELUDE_BYTES + CRC_BYTES;

/// The largest accepted header block, matching the AWS SDK decoders' 128 KiB bound.
///
/// Without it a single 16 MiB frame of three-byte headers would expand into millions of owned
/// header entries.
pub const MAX_EVENTSTREAM_HEADERS_BYTES: usize = 128 * 1024;

const PRELUDE_BYTES: usize = 12;
const CRC_BYTES: usize = 4;

const MESSAGE_TYPE: &str = ":message-type";
const EVENT_TYPE: &str = ":event-type";
const EXCEPTION_TYPE: &str = ":exception-type";
const ERROR_CODE: &str = ":error-code";
const ERROR_MESSAGE: &str = ":error-message";

/// A refused eventstream input. Diagnostics name the failed rule, never the rejected bytes.
///
/// The first group is raised by the deframer ([`AwsEventStreamDeframerV1`],
/// [`deframe_aws_eventstream_v1`]); the second by [`reencode_eventstream_v1`].
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum EventStreamErrorV1 {
    /// The prelude CRC does not match the declared lengths.
    #[error("eventstream prelude checksum mismatch")]
    PreludeChecksumMismatch,
    /// The declared frame length is below [`MIN_EVENTSTREAM_FRAME_BYTES`].
    #[error("eventstream frame is shorter than the minimum")]
    FrameTooShort,
    /// The declared frame length exceeds [`MAX_EVENTSTREAM_FRAME_BYTES`].
    #[error("eventstream frame exceeds the size limit")]
    FrameTooLarge,
    /// The declared header block exceeds [`MAX_EVENTSTREAM_HEADERS_BYTES`].
    #[error("eventstream header block exceeds the size limit")]
    HeadersTooLarge,
    /// The declared header block does not fit inside the declared frame.
    #[error("eventstream header block exceeds its frame")]
    HeadersExceedFrame,
    /// The message CRC does not match the frame contents.
    #[error("eventstream message checksum mismatch")]
    MessageChecksumMismatch,
    /// A header ends before its name, type or value is complete.
    #[error("eventstream header is truncated")]
    TruncatedHeader,
    /// A header name is empty.
    #[error("eventstream header name is empty")]
    EmptyHeaderName,
    /// A header name or string value is not UTF-8.
    #[error("eventstream header is not UTF-8")]
    HeaderNotUtf8,
    /// A header value type byte is not one of the ten AWS value types.
    #[error("eventstream header value type is unknown")]
    UnknownHeaderType,
    /// The same header name appears twice in one message.
    #[error("eventstream header is duplicated")]
    DuplicateHeader,
    /// The input ended inside a frame.
    #[error("eventstream ended inside a frame")]
    TruncatedFrame,
    /// [`AwsEventStreamDeframerV1::finish`] found a complete message the caller never pulled.
    #[error("eventstream message was not drained before finish")]
    UndrainedMessage,

    /// `:message-type` is not `event`, `exception` or `error`.
    #[error("eventstream message type is not supported")]
    UnknownMessageType,
    /// A header the message type requires is absent.
    #[error("required eventstream header {header} is missing")]
    MissingHeader {
        /// The missing header name.
        header: &'static str,
    },
    /// A header the re-encoding reads is not of the string type.
    #[error("eventstream header {header} is not a string")]
    HeaderNotString {
        /// The offending header name.
        header: &'static str,
    },
    /// A value bound for the SSE `event:` line is empty or contains CR or LF.
    #[error("eventstream header {header} is not a valid SSE event name")]
    InvalidEventName {
        /// The offending header name.
        header: &'static str,
    },
    /// The payload is not UTF-8.
    #[error("eventstream payload is not UTF-8")]
    PayloadNotUtf8,
    /// The payload is not exactly one JSON value.
    #[error("eventstream payload is not JSON")]
    PayloadNotJson,
}

/// One typed eventstream header value, covering all ten AWS value types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventStreamHeaderValueV1 {
    /// Types 0 (`true`) and 1 (`false`); no value bytes.
    Bool(bool),
    /// Type 2: one signed byte.
    Byte(i8),
    /// Type 3: a big-endian `i16`.
    Int16(i16),
    /// Type 4: a big-endian `i32`.
    Int32(i32),
    /// Type 5: a big-endian `i64`.
    Int64(i64),
    /// Type 6: a `u16`-length-prefixed byte array.
    ByteArray(Vec<u8>),
    /// Type 7: a `u16`-length-prefixed UTF-8 string, the type AWS uses for every `:`-prefixed
    /// header.
    String(String),
    /// Type 8: milliseconds since the Unix epoch as a big-endian `i64`.
    Timestamp(i64),
    /// Type 9: sixteen raw UUID bytes.
    Uuid([u8; 16]),
}

/// One CRC-verified eventstream message: its headers and its raw payload.
///
/// Header names are unique (a duplicate is refused while deframing) and iterate in byte order;
/// the wire order carries no meaning. `Debug` shows header names and the payload size only,
/// because both header values and payloads are upstream response content.
#[derive(Clone, PartialEq, Eq)]
pub struct EventStreamMessageV1 {
    headers: BTreeMap<String, EventStreamHeaderValueV1>,
    payload: Vec<u8>,
}

impl EventStreamMessageV1 {
    /// Returns the value of the named header, if present.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&EventStreamHeaderValueV1> {
        self.headers.get(name)
    }

    /// Iterates over every header in byte order of the names.
    pub fn headers(&self) -> impl Iterator<Item = (&str, &EventStreamHeaderValueV1)> {
        self.headers.iter().map(|(name, value)| (name.as_str(), value))
    }

    /// Returns the payload bytes exactly as framed.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    fn required_string(&self, header: &'static str) -> Result<&str, EventStreamErrorV1> {
        match self.headers.get(header) {
            Some(EventStreamHeaderValueV1::String(value)) => Ok(value),
            Some(_) => Err(EventStreamErrorV1::HeaderNotString { header }),
            None => Err(EventStreamErrorV1::MissingHeader { header }),
        }
    }

    fn event_name(&self, header: &'static str) -> Result<&str, EventStreamErrorV1> {
        let value = self.required_string(header)?;
        if value.is_empty() || value.contains(['\r', '\n']) {
            return Err(EventStreamErrorV1::InvalidEventName { header });
        }
        Ok(value)
    }
}

impl fmt::Debug for EventStreamMessageV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EventStreamMessageV1")
            .field("header_names", &self.headers.keys().collect::<Vec<_>>())
            .field("payload_byte_count", &self.payload.len())
            .finish()
    }
}

/// The incremental AWS eventstream (`application/vnd.amazon.eventstream`) deframer (§5.2).
///
/// Feed upstream chunks split anywhere with [`push`](Self::push), pull complete messages with
/// [`next_message`](Self::next_message) until it returns `Ok(None)`, and call
/// [`finish`](Self::finish) at end of input. However the input is chunked, the sequence of
/// messages and the first error are identical.
///
/// Errors are sticky: after the first one, every later call returns it again and pushed bytes are
/// discarded. A caller that drains after every push holds at most one partial frame, itself
/// bounded by [`MAX_EVENTSTREAM_FRAME_BYTES`], plus the latest chunk.
///
/// ```
/// use south_contracts::{AwsEventStreamDeframerV1, reencode_eventstream_v1};
///
/// # fn run(chunks: &[&[u8]]) -> Result<String, south_contracts::EventStreamErrorV1> {
/// let mut deframer = AwsEventStreamDeframerV1::new();
/// let mut sse = String::new();
/// for chunk in chunks {
///     deframer.push(chunk);
///     while let Some(message) = deframer.next_message()? {
///         sse.push_str(&reencode_eventstream_v1(&message)?);
///     }
/// }
/// deframer.finish()?;
/// # Ok(sse)
/// # }
/// # assert_eq!(run(&[]), Ok(String::new()));
/// ```
#[derive(Default)]
pub struct AwsEventStreamDeframerV1 {
    buffer: Vec<u8>,
    read_pos: usize,
    failure: Option<EventStreamErrorV1>,
}

impl AwsEventStreamDeframerV1 {
    /// Creates a deframer with nothing buffered.
    #[must_use]
    pub const fn new() -> Self {
        Self { buffer: Vec::new(), read_pos: 0, failure: None }
    }

    /// Appends one chunk of upstream bytes; a chunk may end anywhere, even inside the prelude or
    /// a header. Bytes pushed after an error are discarded.
    pub fn push(&mut self, chunk: &[u8]) {
        if self.failure.is_some() {
            return;
        }
        if self.read_pos > 0 {
            self.buffer.drain(..self.read_pos);
            self.read_pos = 0;
        }
        self.buffer.extend_from_slice(chunk);
    }

    /// Returns the next complete message, `Ok(None)` when more bytes are needed, or the first
    /// error.
    ///
    /// A frame's prelude is validated as soon as its 12 bytes are buffered, so a bad prelude CRC
    /// or an out-of-bounds length is reported without waiting for the declared body.
    pub fn next_message(&mut self) -> Result<Option<EventStreamMessageV1>, EventStreamErrorV1> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let pending = self.buffer.get(self.read_pos..).unwrap_or_default();
        match decode_frame(pending) {
            Ok(Some((message, frame_len))) => {
                self.read_pos += frame_len;
                if self.read_pos == self.buffer.len() {
                    self.buffer.clear();
                    self.read_pos = 0;
                }
                Ok(Some(message))
            }
            Ok(None) => Ok(None),
            Err(error) => {
                self.failure = Some(error);
                self.buffer = Vec::new();
                self.read_pos = 0;
                Err(error)
            }
        }
    }

    /// Returns how many pushed bytes are not yet part of a returned message.
    #[must_use]
    pub const fn buffered_len(&self) -> usize {
        self.buffer.len().saturating_sub(self.read_pos)
    }

    /// Ends the input: `Ok(())` only when every pushed byte belonged to a returned message.
    ///
    /// A partial frame left at end of input is [`EventStreamErrorV1::TruncatedFrame`]; a complete
    /// one the caller never pulled is [`EventStreamErrorV1::UndrainedMessage`]; a sticky error is
    /// returned again.
    pub fn finish(mut self) -> Result<(), EventStreamErrorV1> {
        match self.next_message()? {
            Some(_) => Err(EventStreamErrorV1::UndrainedMessage),
            None if self.buffered_len() == 0 => Ok(()),
            None => Err(EventStreamErrorV1::TruncatedFrame),
        }
    }
}

impl fmt::Debug for AwsEventStreamDeframerV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AwsEventStreamDeframerV1")
            .field("buffered_byte_count", &self.buffered_len())
            .field("failure", &self.failure)
            .finish_non_exhaustive()
    }
}

/// Deframes a complete AWS eventstream body (§5.2, the buffered path).
///
/// Equivalent to pushing `body` into an [`AwsEventStreamDeframerV1`], draining it and calling
/// `finish`, without copying the body. An empty body is an empty stream; a body that ends inside
/// a frame is [`EventStreamErrorV1::TruncatedFrame`].
pub fn deframe_aws_eventstream_v1(
    body: &[u8],
) -> Result<Vec<EventStreamMessageV1>, EventStreamErrorV1> {
    let mut rest = body;
    let mut messages = Vec::new();
    while !rest.is_empty() {
        let Some((message, frame_len)) = decode_frame(rest)? else {
            return Err(EventStreamErrorV1::TruncatedFrame);
        };
        messages.push(message);
        rest = rest.get(frame_len..).unwrap_or_default();
    }
    Ok(messages)
}

/// Re-encodes one message as exactly one canonical SSE frame (§5.2).
///
/// | `:message-type` | Frame |
/// |---|---|
/// | `event` | `event: <:event-type>\ndata: <compact payload>\n\n` |
/// | `exception` | `event: exception:<:exception-type>\ndata: <compact payload>\n\n` |
/// | `error` | `event: error:<:error-code>\ndata: {"message":<:error-message>}\n\n` |
///
/// The compact payload is the payload's own JSON text with every insignificant whitespace byte
/// removed. It is validated as exactly one JSON value first, and nothing else changes: member
/// order, number spelling and string escapes are kept byte for byte. The output therefore does not
/// depend on how a consumer's build configures `serde_json` (`preserve_order`,
/// `arbitrary_precision`), which a parse-and-serialize round trip would. JSON forbids raw control
/// characters inside strings, so the `data:` line never contains CR or LF.
///
/// An `error` frame carries its detail in headers: `:error-message` is rendered as a JSON string
/// and the payload is not read. Every header the frame needs must be present and of the string
/// type, and values placed on the `event:` line must be non-empty and free of CR and LF.
pub fn reencode_eventstream_v1(
    message: &EventStreamMessageV1,
) -> Result<String, EventStreamErrorV1> {
    match message.required_string(MESSAGE_TYPE)? {
        "event" => {
            let name = message.event_name(EVENT_TYPE)?;
            let data = compact_json(&message.payload)?;
            Ok(format!("event: {name}\ndata: {data}\n\n"))
        }
        "exception" => {
            let name = message.event_name(EXCEPTION_TYPE)?;
            let data = compact_json(&message.payload)?;
            Ok(format!("event: exception:{name}\ndata: {data}\n\n"))
        }
        "error" => {
            let code = message.event_name(ERROR_CODE)?;
            let detail =
                serde_json::Value::String(message.required_string(ERROR_MESSAGE)?.to_owned());
            Ok(format!("event: error:{code}\ndata: {{\"message\":{detail}}}\n\n"))
        }
        _ => Err(EventStreamErrorV1::UnknownMessageType),
    }
}

/// Decodes the frame at the start of `bytes`: `Ok(None)` when more bytes are needed, otherwise
/// the message and the frame's byte length.
fn decode_frame(bytes: &[u8]) -> Result<Option<(EventStreamMessageV1, usize)>, EventStreamErrorV1> {
    let Some(prelude) = bytes.first_chunk::<PRELUDE_BYTES>() else {
        return Ok(None);
    };
    let [t0, t1, t2, t3, h0, h1, h2, h3, c0, c1, c2, c3] = *prelude;
    if crc32(&prelude[..8]) != u32::from_be_bytes([c0, c1, c2, c3]) {
        return Err(EventStreamErrorV1::PreludeChecksumMismatch);
    }
    let frame_len = usize::try_from(u32::from_be_bytes([t0, t1, t2, t3]))
        .map_err(|_| EventStreamErrorV1::FrameTooLarge)?;
    if frame_len < MIN_EVENTSTREAM_FRAME_BYTES {
        return Err(EventStreamErrorV1::FrameTooShort);
    }
    if frame_len > MAX_EVENTSTREAM_FRAME_BYTES {
        return Err(EventStreamErrorV1::FrameTooLarge);
    }
    let headers_len = usize::try_from(u32::from_be_bytes([h0, h1, h2, h3]))
        .map_err(|_| EventStreamErrorV1::HeadersTooLarge)?;
    if headers_len > MAX_EVENTSTREAM_HEADERS_BYTES {
        return Err(EventStreamErrorV1::HeadersTooLarge);
    }
    if headers_len > frame_len - MIN_EVENTSTREAM_FRAME_BYTES {
        return Err(EventStreamErrorV1::HeadersExceedFrame);
    }
    let Some(frame) = bytes.get(..frame_len) else {
        return Ok(None);
    };
    let Some((covered, &trailer)) = frame.split_last_chunk::<CRC_BYTES>() else {
        return Err(EventStreamErrorV1::FrameTooShort);
    };
    if crc32(covered) != u32::from_be_bytes(trailer) {
        return Err(EventStreamErrorV1::MessageChecksumMismatch);
    }
    let body = covered.get(PRELUDE_BYTES..).unwrap_or_default();
    let Some((header_block, payload)) = body.split_at_checked(headers_len) else {
        return Err(EventStreamErrorV1::HeadersExceedFrame);
    };
    let headers = parse_headers(header_block)?;
    Ok(Some((EventStreamMessageV1 { headers, payload: payload.to_vec() }, frame_len)))
}

fn parse_headers(
    block: &[u8],
) -> Result<BTreeMap<String, EventStreamHeaderValueV1>, EventStreamErrorV1> {
    let mut reader = HeaderReader { rest: block };
    let mut headers = BTreeMap::new();
    while !reader.rest.is_empty() {
        let [name_len] = reader.array::<1>()?;
        if name_len == 0 {
            return Err(EventStreamErrorV1::EmptyHeaderName);
        }
        let name = std::str::from_utf8(reader.bytes(usize::from(name_len))?)
            .map_err(|_| EventStreamErrorV1::HeaderNotUtf8)?;
        let [type_byte] = reader.array::<1>()?;
        let value = match type_byte {
            0 => EventStreamHeaderValueV1::Bool(true),
            1 => EventStreamHeaderValueV1::Bool(false),
            2 => EventStreamHeaderValueV1::Byte(i8::from_be_bytes(reader.array()?)),
            3 => EventStreamHeaderValueV1::Int16(i16::from_be_bytes(reader.array()?)),
            4 => EventStreamHeaderValueV1::Int32(i32::from_be_bytes(reader.array()?)),
            5 => EventStreamHeaderValueV1::Int64(i64::from_be_bytes(reader.array()?)),
            6 => EventStreamHeaderValueV1::ByteArray(reader.length_prefixed()?.to_vec()),
            7 => EventStreamHeaderValueV1::String(
                std::str::from_utf8(reader.length_prefixed()?)
                    .map_err(|_| EventStreamErrorV1::HeaderNotUtf8)?
                    .to_owned(),
            ),
            8 => EventStreamHeaderValueV1::Timestamp(i64::from_be_bytes(reader.array()?)),
            9 => EventStreamHeaderValueV1::Uuid(reader.array()?),
            _ => return Err(EventStreamErrorV1::UnknownHeaderType),
        };
        match headers.entry(name.to_owned()) {
            Entry::Vacant(slot) => {
                slot.insert(value);
            }
            Entry::Occupied(_) => return Err(EventStreamErrorV1::DuplicateHeader),
        }
    }
    Ok(headers)
}

/// A bounds-checked cursor over one header block.
struct HeaderReader<'a> {
    rest: &'a [u8],
}

impl<'a> HeaderReader<'a> {
    fn bytes(&mut self, len: usize) -> Result<&'a [u8], EventStreamErrorV1> {
        let (taken, rest) =
            self.rest.split_at_checked(len).ok_or(EventStreamErrorV1::TruncatedHeader)?;
        self.rest = rest;
        Ok(taken)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], EventStreamErrorV1> {
        let (taken, rest) =
            self.rest.split_first_chunk::<N>().ok_or(EventStreamErrorV1::TruncatedHeader)?;
        self.rest = rest;
        Ok(*taken)
    }

    fn length_prefixed(&mut self) -> Result<&'a [u8], EventStreamErrorV1> {
        let len = u16::from_be_bytes(self.array()?);
        self.bytes(usize::from(len))
    }
}

/// Validates `payload` as one UTF-8 JSON value and strips the whitespace between its tokens.
fn compact_json(payload: &[u8]) -> Result<String, EventStreamErrorV1> {
    let text = std::str::from_utf8(payload).map_err(|_| EventStreamErrorV1::PayloadNotUtf8)?;
    serde_json::from_str::<IgnoredAny>(text).map_err(|_| EventStreamErrorV1::PayloadNotJson)?;

    let mut compact = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    for ch in text.chars() {
        if in_string {
            // Unreachable after validation, kept local: the SSE line must never carry a raw
            // control character, whatever the JSON validator's future behaviour.
            if ch < ' ' {
                return Err(EventStreamErrorV1::PayloadNotJson);
            }
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            compact.push(ch);
        } else if !matches!(ch, ' ' | '\t' | '\n' | '\r') {
            in_string = ch == '"';
            compact.push(ch);
        }
    }
    Ok(compact)
}

/// CRC32 (IEEE 802.3, reflected polynomial `0xEDB88320`), the checksum eventstream uses for both
/// the prelude and the message.
fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(u32::MAX, |crc, &byte| {
        CRC32_TABLE[usize::from(crc.to_le_bytes()[0] ^ byte)] ^ (crc >> 8)
    })
}

const CRC32_TABLE: [u32; 256] = crc32_table();

const fn crc32_table() -> [u32; 256] {
    let mut table = [0_u32; 256];
    let mut index: u32 = 0;
    while index < 256 {
        let mut crc = index;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            bit += 1;
        }
        table[index as usize] = crc;
        index += 1;
    }
    table
}

#[cfg(test)]
mod tests {
    use super::crc32;

    #[test]
    fn crc32_matches_the_ieee_check_value() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"The quick brown fox jumps over the lazy dog"), 0x414F_A339);
    }
}
