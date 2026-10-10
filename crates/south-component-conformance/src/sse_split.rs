//! An incremental SSE event splitter a component links (`OpenAI` Responses upstream record §6, as
//! amended 2026-10-10).
//!
//! `decode_sse_v1` lives in the host-only crate `south-host-grammars`, which no component may
//! link (boundary record §13.13), so a component splits its own stream. This splitter reads the
//! same WHATWG event-stream rules that decoder implements, so that the frames a host cuts with
//! `SseDecoderV1::position` under `north_passthrough` are frames this splitter recognises: LF, CR
//! and CRLF line ends; one leading byte-order mark dropped; comment lines dropped; `data` lines
//! joined with LF; an empty or absent event type is `message`; `id`, `retry` and unknown fields
//! ignored; a blank line with no `data` line dispatches nothing. Like the decoder, the end of input
//! reads an unterminated last line and dispatches the pending event, and a line that is not UTF-8
//! is refused rather than replaced.
//!
//! The decoder's golden vectors run against this splitter in `openai_responses_vocabulary_v1`
//! (with `south-host-grammars` as a dev-dependency only), which is how the two sides are shown to
//! cut the same events.
//!
//! Memory is bounded: a line or an event longer than [`MAX_SSE_SPLIT_BYTES`] is refused.

/// The most bytes one line, or one event's joined data, may hold: 16 MiB, the decoder's own
/// bound for each.
pub const MAX_SSE_SPLIT_BYTES: usize = 16 * 1024 * 1024;

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// One dispatched event: its type (`message` when the stream named none) and its data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseFrameV1 {
    pub event: String,
    pub data: String,
}

/// Why the splitter refused the stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseSplitErrorV1 {
    /// A line is not UTF-8.
    NotUtf8,
    /// A line or an event's data outgrew [`MAX_SSE_SPLIT_BYTES`].
    TooLarge,
}

/// The splitter's state between chunks.
#[derive(Debug, Default)]
pub struct SseSplitterV1 {
    /// Bytes not yet consumed as whole lines.
    tail: Vec<u8>,
    /// Whether the leading byte-order mark question is settled.
    bom_settled: bool,
    /// The previous line ended with a CR, so an LF that follows is part of that line end.
    after_cr: bool,
    event: String,
    data: String,
    has_data: bool,
}

impl SseSplitterV1 {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the splitter holds no part of an unfinished event: no unread bytes and no pending
    /// field. A pending CR line end is not a partial frame.
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        self.tail.is_empty() && !self.has_data && self.event.is_empty()
    }

    /// Appends `chunk` and returns every event it completed, in order.
    ///
    /// # Errors
    ///
    /// The first line that is not UTF-8, or a line or event over the bound. The splitter is then
    /// unusable; a caller ends the stream.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseFrameV1>, SseSplitErrorV1> {
        let mut chunk = chunk;
        if self.after_cr && !chunk.is_empty() {
            self.after_cr = false;
            if chunk[0] == b'\n' {
                chunk = &chunk[1..];
            }
        }
        self.tail.extend_from_slice(chunk);
        if !self.bom_settled {
            if self.tail.len() < BOM.len() && BOM.starts_with(&self.tail) {
                return Ok(Vec::new());
            }
            if self.tail.starts_with(BOM) {
                self.tail.drain(..BOM.len());
            }
            self.bom_settled = true;
        }
        let mut frames = Vec::new();
        let mut start = 0;
        while let Some(offset) = self.tail[start..].iter().position(|b| matches!(b, b'\r' | b'\n'))
        {
            let end = start + offset;
            let line = std::str::from_utf8(&self.tail[start..end])
                .map_err(|_| SseSplitErrorV1::NotUtf8)?
                .to_owned();
            let terminator = self.tail[end];
            start = end + 1;
            if terminator == b'\r' {
                match self.tail.get(start) {
                    Some(b'\n') => start += 1,
                    Some(_) => {}
                    None => self.after_cr = true,
                }
            }
            if let Some(frame) = self.line(&line)? {
                frames.push(frame);
            }
        }
        self.tail.drain(..start);
        if self.tail.len() > MAX_SSE_SPLIT_BYTES {
            return Err(SseSplitErrorV1::TooLarge);
        }
        Ok(frames)
    }

    /// The end of input: reads an unterminated last line, then dispatches the pending event, as
    /// the decoder does.
    ///
    /// # Errors
    ///
    /// The last line is not UTF-8 (including an incomplete byte-order mark or a truncated
    /// multi-byte sequence).
    pub fn finish(&mut self) -> Result<Option<SseFrameV1>, SseSplitErrorV1> {
        self.bom_settled = true;
        self.after_cr = false;
        if !self.tail.is_empty() {
            // A non-empty line never dispatches, so only the blank line below can.
            let tail = std::mem::take(&mut self.tail);
            let line = std::str::from_utf8(&tail).map_err(|_| SseSplitErrorV1::NotUtf8)?;
            self.line(line)?;
        }
        self.line("")
    }

    /// One line, without its terminator.
    fn line(&mut self, line: &str) -> Result<Option<SseFrameV1>, SseSplitErrorV1> {
        if line.is_empty() {
            let event = std::mem::take(&mut self.event);
            if !std::mem::take(&mut self.has_data) {
                return Ok(None);
            }
            let data = std::mem::take(&mut self.data);
            let event = if event.is_empty() { "message".to_owned() } else { event };
            return Ok(Some(SseFrameV1 { event, data }));
        }
        if line.starts_with(':') {
            return Ok(None);
        }
        let (field, value) = line
            .split_once(':')
            .map_or((line, ""), |(field, value)| (field, value.strip_prefix(' ').unwrap_or(value)));
        match field {
            "event" => value.clone_into(&mut self.event),
            "data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
                if self.data.len() > MAX_SSE_SPLIT_BYTES {
                    return Err(SseSplitErrorV1::TooLarge);
                }
            }
            _ => {}
        }
        Ok(None)
    }
}
