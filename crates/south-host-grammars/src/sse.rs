//! The server-sent events decoder (`decode_sse_v1`).
//!
//! The rules are the WHATWG HTML event-stream interpretation (section 9.2.6, "Interpreting an
//! event stream"):
//!
//! - LF, CR and CRLF each end a line, and may be mixed in one stream;
//! - one leading byte order mark (U+FEFF) is dropped;
//! - a line starting with `:` is a comment and is dropped;
//! - a line is split at its first `:` into field name and value, one space after the colon is
//!   dropped, and a line with no colon is a field name with an empty value;
//! - `data` appends its value and an LF to the data buffer, `event` sets the event type, `id`,
//!   `retry` and every other field name are ignored (field names are case-sensitive);
//! - a blank line dispatches: an empty data buffer dispatches nothing, otherwise the final LF is
//!   removed and the event is named by the event type, or [`DEFAULT_SSE_EVENT_TYPE`] when that is
//!   empty; both buffers are then reset.
//!
//! Three departures, each deliberate:
//!
//! - **The end of input dispatches.** The standard discards an event that no blank line has
//!   ended, because a live connection might still deliver one. Both uses here end at a definite
//!   point (a buffered body, or a stream the upstream closed), and discarding would lose a
//!   terminal usage event from an upstream that omits the final blank line (speech record §6).
//!   An unterminated last line is therefore also read as a line.
//! - **Invalid UTF-8 is an error**, not a replacement character. A replaced byte silently changes
//!   the content a host forwards, bills or hands to a component; [`SseErrorV1::NotUtf8`] says so
//!   instead.
//! - **Memory is bounded**: [`MAX_SSE_LINE_BYTES`], [`MAX_SSE_EVENT_BYTES`] and, for the whole-body
//!   function, [`MAX_SSE_EVENTS`].
//!
//! `id` and `retry` are reconnection state for a browser's `EventSource`; neither use here
//! reconnects, so neither is kept (speech record §6, Q13).

use std::{error::Error, fmt, mem};

/// The longest accepted line, in bytes, without its line terminator.
///
/// Checked as soon as an unterminated line grows past it, so a stream that never ends a line
/// holds at most this much (plus the latest chunk) before it is refused.
pub const MAX_SSE_LINE_BYTES: usize = 16 * 1024 * 1024;

/// The largest accepted event: its event type plus its data, joined with LF, in bytes.
///
/// Many `data:` lines each under [`MAX_SSE_LINE_BYTES`] cannot build an event larger than this.
pub const MAX_SSE_EVENT_BYTES: usize = 16 * 1024 * 1024;

/// The most events [`decode_sse_v1`] returns for one body.
///
/// A two-byte event (`data\n\n` is seven bytes, `:` lines dispatch nothing) still costs an owned
/// event, so without this bound a 32 MiB body could become millions of allocations. The
/// incremental [`SseDecoderV1`] hands events out one at a time and has no such bound.
pub const MAX_SSE_EVENTS: usize = 262_144;

/// The event type of an event whose stream named none (or named the empty string).
pub const DEFAULT_SSE_EVENT_TYPE: &str = "message";

const CR: u8 = b'\r';
const LF: u8 = b'\n';
const BOM: &[u8] = "\u{feff}".as_bytes();

/// A refused SSE input. Diagnostics name the failed rule, never the rejected bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SseErrorV1 {
    /// A line is not valid UTF-8.
    NotUtf8,
    /// A line exceeds [`MAX_SSE_LINE_BYTES`].
    LineTooLong,
    /// An event's type and data exceed [`MAX_SSE_EVENT_BYTES`].
    EventTooLarge,
    /// [`decode_sse_v1`] would return more than [`MAX_SSE_EVENTS`] events.
    TooManyEvents,
    /// [`SseDecoderV1::finish`] found a complete event the caller never pulled.
    UndrainedEvent,
}

impl fmt::Display for SseErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotUtf8 => "SSE line is not UTF-8",
            Self::LineTooLong => "SSE line exceeds the size limit",
            Self::EventTooLarge => "SSE event exceeds the size limit",
            Self::TooManyEvents => "SSE body exceeds the event count limit",
            Self::UndrainedEvent => "SSE event was not drained before finish",
        })
    }
}

impl Error for SseErrorV1 {}

/// One dispatched event: its type and its data.
///
/// `Debug` shows the event type and the data size only, because the data is upstream response
/// content.
#[derive(Clone, PartialEq, Eq)]
pub struct SseEventV1 {
    event: String,
    data: String,
}

impl SseEventV1 {
    /// Builds an event, for instance an expected one in a host's test.
    #[must_use]
    pub fn new(event: impl Into<String>, data: impl Into<String>) -> Self {
        Self { event: event.into(), data: data.into() }
    }

    /// The event type: the last `event:` value before dispatch, or [`DEFAULT_SSE_EVENT_TYPE`].
    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    /// The data: every `data:` value of the event, joined with LF. It may be empty, and it is not
    /// interpreted (JSON or not is the caller's question).
    #[must_use]
    pub fn data(&self) -> &str {
        &self.data
    }

    /// Splits the event into its type and its data.
    #[must_use]
    pub fn into_parts(self) -> (String, String) {
        (self.event, self.data)
    }
}

impl fmt::Debug for SseEventV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SseEventV1")
            .field("event", &self.event)
            .field("data_byte_count", &self.data.len())
            .finish()
    }
}

/// The field-level state both entry points share: everything after line splitting.
struct EventBuilder {
    at_stream_start: bool,
    event_type: String,
    data: String,
}

impl EventBuilder {
    const fn new() -> Self {
        Self { at_stream_start: true, event_type: String::new(), data: String::new() }
    }

    /// Interprets one line, given without its terminator.
    fn line(&mut self, line: &[u8]) -> Result<Option<SseEventV1>, SseErrorV1> {
        if line.len() > MAX_SSE_LINE_BYTES {
            return Err(SseErrorV1::LineTooLong);
        }
        let mut line = line;
        if mem::replace(&mut self.at_stream_start, false)
            && let Some(rest) = line.strip_prefix(BOM)
        {
            line = rest;
        }
        let line = std::str::from_utf8(line).map_err(|_| SseErrorV1::NotUtf8)?;
        if line.is_empty() {
            return Ok(self.dispatch());
        }
        // A comment. The field rule below would ignore it too (its field name is empty), so this
        // branch only states the rule; a mutation that deletes it is equivalent.
        if line.starts_with(':') {
            return Ok(None);
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => {
                if value.len().saturating_add(self.data.len()) > MAX_SSE_EVENT_BYTES {
                    return Err(SseErrorV1::EventTooLarge);
                }
                self.event_type.clear();
                self.event_type.push_str(value);
            }
            "data" => {
                let size = self.event_type.len().saturating_add(self.data.len());
                if size.saturating_add(value.len()).saturating_add(1) > MAX_SSE_EVENT_BYTES {
                    return Err(SseErrorV1::EventTooLarge);
                }
                self.data.push_str(value);
                self.data.push('\n');
            }
            _ => {}
        }
        Ok(None)
    }

    /// Dispatches the pending event, if it has any data, and resets both buffers.
    fn dispatch(&mut self) -> Option<SseEventV1> {
        if self.data.is_empty() {
            self.event_type.clear();
            return None;
        }
        self.data.pop();
        let event = if self.event_type.is_empty() {
            DEFAULT_SSE_EVENT_TYPE.to_owned()
        } else {
            mem::take(&mut self.event_type)
        };
        Some(SseEventV1 { event, data: mem::take(&mut self.data) })
    }
}

/// The position of the first line terminator byte (CR or LF) in `bytes`.
fn terminator(bytes: &[u8]) -> Option<usize> {
    bytes.iter().position(|byte| *byte == CR || *byte == LF)
}

/// The incremental SSE decoder.
///
/// Feed upstream chunks split anywhere with [`push`](Self::push), pull events with
/// [`next_event`](Self::next_event) until it returns `Ok(None)`, and call
/// [`finish`](Self::finish) at end of input for the event the end of input dispatches. However
/// the input is chunked, the sequence of events, their [`position`](Self::position)s and the first
/// error are identical, and the events equal [`decode_sse_v1`] on the whole input (except
/// [`SseErrorV1::TooManyEvents`], which only the whole-body function raises). A chunk may end
/// anywhere: inside a line, between the CR and LF of a CRLF, or inside a multi-byte character.
///
/// **Frame boundaries.** Right after `next_event` returns an event, [`position`](Self::position)
/// is the end of the frame that dispatched it: the stream offset just past the blank line's
/// terminator. A host that forwards the upstream's own bytes (`north_passthrough`) cuts the
/// stream there, so each forwarded slice is one whole frame and decodes on its own to exactly
/// that event. To know whether a CR is the first half of a CRLF the decoder needs the byte after
/// it, so a line ended by a CR is read only once that byte arrives (or at `finish`).
///
/// Errors are sticky: after the first one, every later call returns it again and pushed bytes are
/// discarded. A caller that drains after every push holds at most one unterminated line, itself
/// bounded by [`MAX_SSE_LINE_BYTES`], the latest chunk, and one pending event bounded by
/// [`MAX_SSE_EVENT_BYTES`].
///
/// ```
/// use south_host_grammars::{SseDecoderV1, SseErrorV1, SseEventV1};
///
/// # fn run(chunks: &[&[u8]]) -> Result<Vec<(SseEventV1, u64)>, SseErrorV1> {
/// let mut decoder = SseDecoderV1::new();
/// let mut frames = Vec::new();
/// for chunk in chunks {
///     decoder.push(chunk);
///     while let Some(event) = decoder.next_event()? {
///         frames.push((event, decoder.position()));
///     }
/// }
/// let end = decoder.position() + decoder.buffered_len() as u64;
/// frames.extend(decoder.finish()?.map(|event| (event, end)));
/// # Ok(frames)
/// # }
/// let frames = run(&[b"data: 1\r", b"\n\r\nevent: done\ndata: {\"ok\":", b"true}"])?;
/// assert_eq!(
///     frames,
///     [(SseEventV1::new("message", "1"), 11), (SseEventV1::new("done", "{\"ok\":true}"), 40)],
/// );
/// # Ok::<(), SseErrorV1>(())
/// ```
pub struct SseDecoderV1 {
    buffer: Vec<u8>,
    read_pos: usize,
    /// Where the search for the next terminator resumes, so a long line delivered in many small
    /// chunks is scanned once rather than once per chunk.
    scan_pos: usize,
    /// Stream offset of `buffer[read_pos]`: every byte of every line read so far.
    position: u64,
    builder: EventBuilder,
    failure: Option<SseErrorV1>,
}

impl fmt::Debug for SseDecoderV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SseDecoderV1")
            .field("position", &self.position)
            .field("buffered_byte_count", &self.buffered_len())
            .field("failure", &self.failure)
            .finish_non_exhaustive()
    }
}

impl Default for SseDecoderV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl SseDecoderV1 {
    /// Creates a decoder at the start of a stream, with nothing buffered.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buffer: Vec::new(),
            read_pos: 0,
            scan_pos: 0,
            position: 0,
            builder: EventBuilder::new(),
            failure: None,
        }
    }

    /// Appends one chunk of upstream bytes. Bytes pushed after an error are discarded.
    pub fn push(&mut self, chunk: &[u8]) {
        if self.failure.is_some() {
            return;
        }
        if self.read_pos > 0 {
            self.buffer.drain(..self.read_pos);
            self.scan_pos = self.scan_pos.saturating_sub(self.read_pos);
            self.read_pos = 0;
        }
        self.buffer.extend_from_slice(chunk);
    }

    /// Returns the next dispatched event, `Ok(None)` when more bytes are needed, or the first
    /// error.
    pub fn next_event(&mut self) -> Result<Option<SseEventV1>, SseErrorV1> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        match self.advance() {
            Ok(event) => Ok(event),
            Err(error) => {
                self.failure = Some(error);
                self.buffer = Vec::new();
                self.read_pos = 0;
                self.scan_pos = 0;
                Err(error)
            }
        }
    }

    fn advance(&mut self) -> Result<Option<SseEventV1>, SseErrorV1> {
        loop {
            let start = self.read_pos;
            let from = self.scan_pos.max(start);
            let unscanned = self.buffer.get(from..).unwrap_or_default();
            let Some(offset) = terminator(unscanned) else {
                self.scan_pos = self.buffer.len();
                if self.buffer.len() - start > MAX_SSE_LINE_BYTES {
                    return Err(SseErrorV1::LineTooLong);
                }
                if start == self.buffer.len() {
                    self.buffer.clear();
                    self.read_pos = 0;
                    self.scan_pos = 0;
                }
                return Ok(None);
            };
            let end = from + offset;
            if end - start > MAX_SSE_LINE_BYTES {
                return Err(SseErrorV1::LineTooLong);
            }
            let terminator_len = match (self.buffer.get(end), self.buffer.get(end + 1)) {
                (Some(&CR), None) => {
                    // A CR that may be the first half of a CRLF: wait for the next byte.
                    self.scan_pos = end;
                    return Ok(None);
                }
                (Some(&CR), Some(&LF)) => 2,
                _ => 1,
            };
            self.read_pos = end + terminator_len;
            self.scan_pos = self.read_pos;
            self.position += (self.read_pos - start) as u64;
            let line = self.buffer.get(start..end).unwrap_or_default();
            if let Some(event) = self.builder.line(line)? {
                return Ok(Some(event));
            }
        }
    }

    /// The stream offset up to which lines have been read: every byte of every line read so far,
    /// terminators included. Right after [`next_event`](Self::next_event) returns an event, this
    /// is the end of the frame that dispatched it (see the type docs).
    #[must_use]
    pub const fn position(&self) -> u64 {
        self.position
    }

    /// Returns how many pushed bytes are not yet part of a line that has been read.
    #[must_use]
    pub const fn buffered_len(&self) -> usize {
        self.buffer.len().saturating_sub(self.read_pos)
    }

    /// Ends the input and returns the event the end of input dispatches, if any.
    ///
    /// A last line ended by a CR is read, an unterminated last line is read as a line, and then
    /// the pending event is dispatched (the first departure in the module docs). Its frame is
    /// everything after [`position`](Self::position), [`buffered_len`](Self::buffered_len) bytes.
    /// A complete event the caller never pulled is [`SseErrorV1::UndrainedEvent`]; a sticky error
    /// is returned again.
    pub fn finish(mut self) -> Result<Option<SseEventV1>, SseErrorV1> {
        if self.next_event()?.is_some() {
            return Err(SseErrorV1::UndrainedEvent);
        }
        let rest = self.buffer.get(self.read_pos..).unwrap_or_default();
        if !rest.is_empty() {
            // `advance` left at most one line here, either unterminated or ended by a final CR.
            let line = rest.strip_suffix(&[CR]).unwrap_or(rest);
            if let Some(event) = self.builder.line(line)? {
                return Ok(Some(event));
            }
        }
        Ok(self.builder.dispatch())
    }
}

fn push_bounded(events: &mut Vec<SseEventV1>, event: SseEventV1) -> Result<(), SseErrorV1> {
    if events.len() == MAX_SSE_EVENTS {
        return Err(SseErrorV1::TooManyEvents);
    }
    events.push(event);
    Ok(())
}

/// Decodes a complete SSE body into its events.
///
/// Equivalent to pushing `body` into an [`SseDecoderV1`], draining it and calling `finish`,
/// without copying the body, and bounded by [`MAX_SSE_EVENTS`]. An empty body has no events.
///
/// ```
/// use south_host_grammars::{SseEventV1, decode_sse_v1};
///
/// let body = b"\xEF\xBB\xBF: keep-alive\r\ndata: a\r\ndata: b\r\n\r\nevent: done\ndata:\n";
/// assert_eq!(
///     decode_sse_v1(body)?,
///     [SseEventV1::new("message", "a\nb"), SseEventV1::new("done", "")],
/// );
/// # Ok::<(), south_host_grammars::SseErrorV1>(())
/// ```
pub fn decode_sse_v1(body: &[u8]) -> Result<Vec<SseEventV1>, SseErrorV1> {
    let mut builder = EventBuilder::new();
    let mut events = Vec::new();
    let mut rest = body;
    while let Some(end) = terminator(rest) {
        let line = rest.get(..end).unwrap_or_default();
        if let Some(event) = builder.line(line)? {
            push_bounded(&mut events, event)?;
        }
        let crlf = rest.get(end) == Some(&CR) && rest.get(end + 1) == Some(&LF);
        rest = rest.get(end + 1 + usize::from(crlf)..).unwrap_or_default();
    }
    if !rest.is_empty()
        && let Some(event) = builder.line(rest)?
    {
        push_bounded(&mut events, event)?;
    }
    if let Some(event) = builder.dispatch() {
        push_bounded(&mut events, event)?;
    }
    Ok(events)
}
