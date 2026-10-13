//! Canonical cases for the pass-through delivery host suite, `south.north-passthrough-delivery.v1`
//! (gate ③ of the `OpenAI` Responses upstream record, `docs/design/2026-09-30-openai-responses-
//! upstream-component.md` §8.2 and §12.4; designed in
//! `docs/design/2026-10-11-responses-reasoning-replay-and-passthrough-suite.md` §3).
//!
//! A family that declares `north_passthrough: "responses"` lets a host forward the upstream's own
//! bytes to a Responses client instead of rendering the component's IR. That is safe only if the
//! host's wiring keeps four promises: it cuts the upstream bytes into whole frames with
//! `SseDecoderV1::position` and feeds the component one frame per call; it forwards a frame only
//! after that call returned cleanly; it releases the terminal frame only when its northbound type
//! is terminal **and** its call returned `Done`, and only after the settlement is persisted; and on
//! any failure it records the funds transition, never relays the upstream's failure frame, and
//! ends the stream with its own fixed-message `response.failed`. This suite drives a host's own
//! executor and checks those promises from the outside.
//!
//! The suite holds no IR. The host replaces the component with a double driven by the suite's
//! per-call [`NorthPassthroughFrameVerdictV1`] script, and reports an ordered
//! [`NorthPassthroughTimelineV1`]; the order of its steps is the evidence. South's reference
//! executor is in `south-testkit`.
//!
//! # Version two: the suite parameterised by protocol
//!
//! `south.north-passthrough-delivery.v2` (design record
//! `docs/design/2026-10-14-generate-content-request-passthrough.md` §6) runs the same checks for
//! one [`NorthPassthroughProtocolV2`] at a time: [`run_north_passthrough_conformance_v2`] with a
//! [`NorthPassthroughHarnessV2`], whose `stream` and `buffered` take the protocol. Its Responses
//! cases are exactly the version-one table ([`north_passthrough_fixtures_v1`]), so a host passing
//! version one passes version two for Responses; version one stays the Responses-only suite with
//! its own harness, identifier and 22 cases, unchanged. The `generate_content` cases
//! ([`north_passthrough_fixtures_v2`]) keep the frame, order and settlement rules and change three:
//!
//! - **Terminal frame**: a frame whose JSON data has some `candidates[*].finishReason` set (not
//!   null), and whose call returned `Done`.
//! - **Failure**: the protocol has no failure event, so the host ends the stream without writing
//!   any frame of its own; the suite requires that nothing but comment lines follows the relayed
//!   frames.
//! - **Non-2xx**: the upstream's status and body reach the client as the upstream sent them,
//!   after `map-provider-error` returned and the funds transition was recorded; the host reports
//!   the status it wrote as [`NorthPassthroughStepV1::StatusWritten`].
//!
//! A second suite identifier, rather than more cases under the first, keeps a host's claim
//! "passes `south.north-passthrough-delivery.v1`" meaning what it meant when it was made.

use std::fmt;

use serde_json::Value;
use south_host_grammars::{SseDecoderV1, SseErrorV1};
use south_provider_api::NorthProtocolV1;

mod runner;

pub use runner::{
    NorthPassthroughConformanceFailureV1, NorthPassthroughConformanceReportV1,
    NorthPassthroughHarnessV1, NorthPassthroughHarnessV2, NorthPassthroughMismatchCategoryV1,
    NorthPassthroughMismatchV1, NorthPassthroughStepV1, NorthPassthroughTimelineV1,
    run_north_passthrough_conformance_v1, run_north_passthrough_conformance_v2,
};

/// The pass-through delivery conformance suite version.
pub const NORTH_PASSTHROUGH_DELIVERY_SUITE_VERSION: u32 = 1;

/// The stable identifier for pass-through delivery conformance version one.
pub const NORTH_PASSTHROUGH_DELIVERY_SUITE_ID: &str = "south.north-passthrough-delivery.v1";

/// The northbound event types that end a successful Responses stream (record §8.2).
pub const NORTH_PASSTHROUGH_TERMINAL_TYPES_V1: [&str; 2] =
    ["response.completed", "response.incomplete"];

/// The pass-through delivery conformance suite version, parameterised by protocol.
pub const NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_VERSION: u32 = 2;

/// The stable identifier for pass-through delivery conformance version two.
pub const NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_ID: &str = "south.north-passthrough-delivery.v2";

/// A northbound protocol the delivery suite has rules for: the closed subset of
/// [`NorthProtocolV1`] a family may declare in `north_passthrough` and a host may be checked on.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NorthPassthroughProtocolV2 {
    /// `OpenAI` Responses: the version-one rules.
    Responses,
    /// The `generateContent` / `streamGenerateContent` surface (design record
    /// `docs/design/2026-10-14-generate-content-request-passthrough.md` §6).
    GenerateContent,
}

fixed_debug!(NorthPassthroughProtocolV2 {
    Responses => "Responses",
    GenerateContent => "GenerateContent",
});

impl NorthPassthroughProtocolV2 {
    /// Every protocol the suite has rules for, in order.
    pub const ALL: [Self; 2] = [Self::Responses, Self::GenerateContent];

    /// The manifest's spelling of the protocol.
    #[must_use]
    pub const fn north_protocol(self) -> NorthProtocolV1 {
        match self {
            Self::Responses => NorthProtocolV1::Responses,
            Self::GenerateContent => NorthProtocolV1::GenerateContent,
        }
    }

    /// The suite's protocol for a manifest protocol, or `None` when the suite has no rules for it
    /// (Chat Completions and Messages are never passed through).
    #[must_use]
    pub const fn from_north_protocol(protocol: NorthProtocolV1) -> Option<Self> {
        match protocol {
            NorthProtocolV1::Responses => Some(Self::Responses),
            NorthProtocolV1::GenerateContent => Some(Self::GenerateContent),
            NorthProtocolV1::ChatCompletions | NorthProtocolV1::Messages => None,
        }
    }
}

/// Whether one frame's JSON data marks the end of a successful stream of `protocol`: a terminal
/// `type` ([`NORTH_PASSTHROUGH_TERMINAL_TYPES_V1`]) for Responses, some
/// `candidates[*].finishReason` set (not null) for `generate_content`. A host releases such a
/// frame only when its call also returned `Done`.
#[must_use]
pub fn north_passthrough_is_terminal_data_v2(
    protocol: NorthPassthroughProtocolV2,
    data: &Value,
) -> bool {
    match protocol {
        NorthPassthroughProtocolV2::Responses => data
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| NORTH_PASSTHROUGH_TERMINAL_TYPES_V1.contains(&kind)),
        NorthPassthroughProtocolV2::GenerateContent => has_finish_reason(data),
    }
}

fn has_finish_reason(data: &Value) -> bool {
    data.get("candidates").and_then(Value::as_array).is_some_and(|candidates| {
        candidates
            .iter()
            .any(|candidate| candidate.get("finishReason").is_some_and(|reason| !reason.is_null()))
    })
}

/// One frame of an upstream stream, as the host cuts it.
#[derive(Clone, PartialEq, Eq)]
pub struct NorthPassthroughFrameV1 {
    bytes: Vec<u8>,
    kind: Option<String>,
    sequence_number: Option<u64>,
    finish_reason: bool,
    at_end_of_input: bool,
}

impl NorthPassthroughFrameV1 {
    /// The frame's bytes: everything after the previous frame up to the end of the line that
    /// dispatched this one, comments and blank lines before it included.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The `type` of the frame's JSON data, if it has one.
    #[must_use]
    pub fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }

    /// Whether the frame's northbound type is a terminal type ([`NORTH_PASSTHROUGH_TERMINAL_TYPES_V1`]).
    #[must_use]
    pub fn is_terminal_type(&self) -> bool {
        self.kind().is_some_and(|kind| NORTH_PASSTHROUGH_TERMINAL_TYPES_V1.contains(&kind))
    }

    /// The `sequence_number` of the frame's JSON data, if it has one.
    #[must_use]
    pub const fn sequence_number(&self) -> Option<u64> {
        self.sequence_number
    }

    /// Whether some `candidates[*].finishReason` of the frame's JSON data is set (not null).
    #[must_use]
    pub const fn has_finish_reason(&self) -> bool {
        self.finish_reason
    }

    /// Whether the frame marks the end of a successful stream of `protocol`
    /// ([`north_passthrough_is_terminal_data_v2`]).
    #[must_use]
    pub fn is_terminal_frame(&self, protocol: NorthPassthroughProtocolV2) -> bool {
        match protocol {
            NorthPassthroughProtocolV2::Responses => self.is_terminal_type(),
            NorthPassthroughProtocolV2::GenerateContent => self.finish_reason,
        }
    }

    /// Whether the end of input dispatched the frame (no blank line ended it).
    #[must_use]
    pub const fn at_end_of_input(&self) -> bool {
        self.at_end_of_input
    }

    /// The frame as the host forwards it: its bytes, plus the minimal terminator when the end of
    /// input dispatched it without a blank line (NP-Q10), so a standard client dispatches it too.
    #[must_use]
    pub fn forwarded_bytes(&self) -> Vec<u8> {
        let mut bytes = self.bytes.clone();
        bytes.extend_from_slice(north_passthrough_terminator_v1(&self.bytes));
        bytes
    }

    fn new(bytes: Vec<u8>, data: &str, at_end_of_input: bool) -> Self {
        let json: Option<Value> = serde_json::from_str(data).ok();
        let kind = json
            .as_ref()
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let sequence_number =
            json.as_ref().and_then(|value| value.get("sequence_number")).and_then(Value::as_u64);
        let finish_reason = json.as_ref().is_some_and(has_finish_reason);
        Self { bytes, kind, sequence_number, finish_reason, at_end_of_input }
    }
}

impl fmt::Debug for NorthPassthroughFrameV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NorthPassthroughFrameV1")
            .field("byte_count", &self.bytes.len())
            .field("kind", &self.kind)
            .field("sequence_number", &self.sequence_number)
            .field("finish_reason", &self.finish_reason)
            .field("at_end_of_input", &self.at_end_of_input)
            .finish()
    }
}

/// Cuts a complete upstream stream into the frames a host feeds the component.
///
/// One frame per `parse-stream-chunk` call: the spans `SseDecoderV1::position` marks. Bytes after the last
/// dispatched event that dispatch nothing (a trailing comment) belong to no frame.
///
/// A host cutting incrementally, chunk by chunk, gets the same frames: the decoder's positions do
/// not depend on chunking.
pub fn north_passthrough_frames_v1(
    body: &[u8],
) -> Result<Vec<NorthPassthroughFrameV1>, SseErrorV1> {
    let mut decoder = SseDecoderV1::new();
    decoder.push(body);
    let mut frames = Vec::new();
    let mut start = 0usize;
    while let Some(event) = decoder.next_event()? {
        let end = usize::try_from(decoder.position()).unwrap_or(body.len()).min(body.len());
        frames.push(NorthPassthroughFrameV1::new(
            body.get(start..end).unwrap_or_default().to_vec(),
            event.data(),
            false,
        ));
        start = end;
    }
    if let Some(event) = decoder.finish()? {
        frames.push(NorthPassthroughFrameV1::new(
            body.get(start..).unwrap_or_default().to_vec(),
            event.data(),
            true,
        ));
    }
    Ok(frames)
}

/// The bytes a host appends to a frame the end of input dispatched.
///
/// They make its last line followed by a blank line (NP-Q10): nothing when the frame already ends with a blank line, the
/// frame's own line ending once when its last line is terminated, and `\n\n` when it is not.
#[must_use]
pub fn north_passthrough_terminator_v1(frame: &[u8]) -> &'static [u8] {
    let without_last = if let Some(rest) = frame.strip_suffix(b"\r\n") {
        rest
    } else if let Some(rest) = frame.strip_suffix(b"\n") {
        rest
    } else if let Some(rest) = frame.strip_suffix(b"\r") {
        rest
    } else {
        return b"\n\n";
    };
    if frame.is_empty() || without_last.ends_with(b"\n") || without_last.ends_with(b"\r") {
        return b"";
    }
    if frame.ends_with(b"\r") && !frame.ends_with(b"\r\n") { b"\r" } else { b"\n" }
}

/// An upstream body the suite feeds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NorthPassthroughBodyV1 {
    /// A completed stream mixing LF, CR and CRLF line ends, with a byte order mark, comment
    /// keep-alives and a `data` field split over two lines.
    MixedFraming,
    /// `provider.stream.text` of the `OpenAI` Responses fixture pack: a completed text answer.
    OpenAiText,
    /// `provider.stream.incomplete` of the pack: a stream ending in `response.incomplete`.
    OpenAiIncomplete,
    /// `provider.stream.failed` of the pack: two lifecycle frames, then `response.failed`.
    OpenAiFailed,
    /// `provider.stream.error-event-without-sequence` of the pack: one `error` frame, nothing
    /// before it.
    OpenAiErrorFirst,
    /// [`Self::MixedFraming`]'s first two frames, then an `error` frame without
    /// `sequence_number`.
    ErrorEventAfterFrames,
    /// [`Self::MixedFraming`] without its terminal frame: the upstream stopped early.
    Truncated,
    /// [`Self::MixedFraming`] whose terminal frame's last line ends the input, with no blank line.
    TerminalWithoutBlankLine,
    /// [`Self::MixedFraming`] whose terminal frame's last line has no line ending at all.
    TerminalWithoutLineEnd,
    /// A completed `generate_content` stream mixing LF, CR and CRLF line ends, with a byte order
    /// mark, comment keep-alives and a `data` field split over two lines: two text frames, then
    /// one with `finishReason: "STOP"` and `usageMetadata`.
    GeminiMixedFraming,
    /// `provider.stream.text-and-terminal` of the Gemini fixture pack: a completed text answer.
    GeminiText,
    /// `provider.stream.eof-terminates-without-a-usage-frame` of the pack: a stream ending in
    /// `finishReason: "MAX_TOKENS"`.
    GeminiMaxTokens,
    /// A CRLF stream of two candidates whose last frame sets `finishReason` on the second
    /// candidate only.
    GeminiSecondCandidateFinishes,
    /// One `error` frame (the shape Gemini streams an error in), nothing before it.
    GeminiErrorFirst,
    /// [`Self::GeminiMixedFraming`]'s first two frames, then an `error` frame.
    GeminiErrorAfterFrames,
    /// [`Self::GeminiMixedFraming`] without its terminal frame: the upstream stopped early.
    GeminiTruncated,
    /// [`Self::GeminiMixedFraming`] whose terminal frame's last line ends the input, with no
    /// blank line.
    GeminiTerminalWithoutBlankLine,
    /// [`Self::GeminiMixedFraming`] whose terminal frame's last line has no line ending at all.
    GeminiTerminalWithoutLineEnd,
}

fixed_debug!(NorthPassthroughBodyV1 {
    MixedFraming => "MixedFraming",
    OpenAiText => "OpenAiText",
    OpenAiIncomplete => "OpenAiIncomplete",
    OpenAiFailed => "OpenAiFailed",
    OpenAiErrorFirst => "OpenAiErrorFirst",
    ErrorEventAfterFrames => "ErrorEventAfterFrames",
    Truncated => "Truncated",
    TerminalWithoutBlankLine => "TerminalWithoutBlankLine",
    TerminalWithoutLineEnd => "TerminalWithoutLineEnd",
    GeminiMixedFraming => "GeminiMixedFraming",
    GeminiText => "GeminiText",
    GeminiMaxTokens => "GeminiMaxTokens",
    GeminiSecondCandidateFinishes => "GeminiSecondCandidateFinishes",
    GeminiErrorFirst => "GeminiErrorFirst",
    GeminiErrorAfterFrames => "GeminiErrorAfterFrames",
    GeminiTruncated => "GeminiTruncated",
    GeminiTerminalWithoutBlankLine => "GeminiTerminalWithoutBlankLine",
    GeminiTerminalWithoutLineEnd => "GeminiTerminalWithoutLineEnd",
});

impl NorthPassthroughBodyV1 {
    /// The body's bytes, exactly as the upstream sends them.
    #[must_use]
    pub fn bytes(self) -> Vec<u8> {
        match self {
            Self::MixedFraming => [MIXED_HEAD, MIXED_TERMINAL].concat(),
            Self::OpenAiText => OPENAI_TEXT.as_bytes().to_vec(),
            Self::OpenAiIncomplete => OPENAI_INCOMPLETE.as_bytes().to_vec(),
            Self::OpenAiFailed => OPENAI_FAILED.as_bytes().to_vec(),
            Self::OpenAiErrorFirst => OPENAI_ERROR_FIRST.as_bytes().to_vec(),
            Self::ErrorEventAfterFrames => [MIXED_HEAD, ERROR_EVENT].concat(),
            Self::Truncated => MIXED_HEAD.to_vec(),
            Self::TerminalWithoutBlankLine => {
                [MIXED_HEAD, MIXED_TERMINAL.strip_suffix(b"\n").unwrap_or(MIXED_TERMINAL)].concat()
            }
            Self::TerminalWithoutLineEnd => {
                [MIXED_HEAD, MIXED_TERMINAL.strip_suffix(b"\n\n").unwrap_or(MIXED_TERMINAL)]
                    .concat()
            }
            Self::GeminiMixedFraming => [GEMINI_MIXED_HEAD, GEMINI_MIXED_TERMINAL].concat(),
            Self::GeminiText => GEMINI_TEXT.as_bytes().to_vec(),
            Self::GeminiMaxTokens => GEMINI_MAX_TOKENS.as_bytes().to_vec(),
            Self::GeminiSecondCandidateFinishes => GEMINI_SECOND_CANDIDATE.to_vec(),
            Self::GeminiErrorFirst => GEMINI_ERROR_FIRST.to_vec(),
            Self::GeminiErrorAfterFrames => [GEMINI_MIXED_HEAD, GEMINI_ERROR_EVENT].concat(),
            Self::GeminiTruncated => GEMINI_MIXED_HEAD.to_vec(),
            Self::GeminiTerminalWithoutBlankLine => [
                GEMINI_MIXED_HEAD,
                GEMINI_MIXED_TERMINAL.strip_suffix(b"\n").unwrap_or(GEMINI_MIXED_TERMINAL),
            ]
            .concat(),
            Self::GeminiTerminalWithoutLineEnd => [
                GEMINI_MIXED_HEAD,
                GEMINI_MIXED_TERMINAL.strip_suffix(b"\n\n").unwrap_or(GEMINI_MIXED_TERMINAL),
            ]
            .concat(),
        }
    }

    /// The upstream's own failure text, which must never reach the client.
    #[must_use]
    pub const fn upstream_failure_text(self) -> Option<&'static str> {
        match self {
            Self::OpenAiFailed => Some(OPENAI_FAILED_TEXT),
            Self::OpenAiErrorFirst => Some(OPENAI_ERROR_FIRST_TEXT),
            Self::ErrorEventAfterFrames => Some(ERROR_EVENT_TEXT),
            Self::GeminiErrorFirst => Some(GEMINI_ERROR_FIRST_TEXT),
            Self::GeminiErrorAfterFrames => Some(GEMINI_ERROR_EVENT_TEXT),
            Self::MixedFraming
            | Self::OpenAiText
            | Self::OpenAiIncomplete
            | Self::Truncated
            | Self::TerminalWithoutBlankLine
            | Self::TerminalWithoutLineEnd
            | Self::GeminiMixedFraming
            | Self::GeminiText
            | Self::GeminiMaxTokens
            | Self::GeminiSecondCandidateFinishes
            | Self::GeminiTruncated
            | Self::GeminiTerminalWithoutBlankLine
            | Self::GeminiTerminalWithoutLineEnd => None,
        }
    }
}

/// What the component double answers to one `parse-stream-chunk` call.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NorthPassthroughFrameVerdictV1 {
    /// Events that do not end the stream (possibly none).
    Events,
    /// The terminal evidence: `Finish`, `Usage`, `Done`.
    Done,
    /// A `StreamEvent::Error` (an upstream failure frame, or a protocol violation).
    Error,
    /// The call itself fails (a trap, or an `Err` from the component).
    Trap,
}

fixed_debug!(NorthPassthroughFrameVerdictV1 {
    Events => "Events",
    Done => "Done",
    Error => "Error",
    Trap => "Trap",
});

/// What the component double answers to `finish`, at end of input.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NorthPassthroughFinishVerdictV1 {
    /// Nothing more.
    Clean,
    /// `Done`: a component that held a frame back emits its terminal only now, with no frame of
    /// its own.
    Done,
    /// `transport_truncated`: the stream ended before a terminal frame.
    Truncated,
}

fixed_debug!(NorthPassthroughFinishVerdictV1 {
    Clean => "Clean",
    Done => "Done",
    Truncated => "Truncated",
});

/// Whether persisting the settlement succeeds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NorthPassthroughSettlementV1 {
    /// The settlement is persisted.
    Persists,
    /// Persisting the settlement fails.
    Fails,
}

fixed_debug!(NorthPassthroughSettlementV1 {
    Persists => "Persists",
    Fails => "Fails",
});

/// The component double's script for one streaming exchange.
#[derive(Clone, PartialEq, Eq)]
pub struct NorthPassthroughScriptV1 {
    frames: Vec<NorthPassthroughFrameVerdictV1>,
    finish: NorthPassthroughFinishVerdictV1,
    settlement: NorthPassthroughSettlementV1,
}

impl NorthPassthroughScriptV1 {
    /// The verdict for the `index`-th `parse-stream-chunk` call. A call beyond the script is a
    /// [`NorthPassthroughFrameVerdictV1::Trap`]: a host that feeds more calls than frames fails.
    #[must_use]
    pub fn frame(&self, index: usize) -> NorthPassthroughFrameVerdictV1 {
        self.frames.get(index).copied().unwrap_or(NorthPassthroughFrameVerdictV1::Trap)
    }

    /// The verdict for `finish`.
    #[must_use]
    pub const fn finish(&self) -> NorthPassthroughFinishVerdictV1 {
        self.finish
    }

    /// Whether persisting the settlement succeeds.
    #[must_use]
    pub const fn settlement(&self) -> NorthPassthroughSettlementV1 {
        self.settlement
    }
}

impl fmt::Debug for NorthPassthroughScriptV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NorthPassthroughScriptV1")
            .field("frames", &self.frames)
            .field("finish", &self.finish)
            .field("settlement", &self.settlement)
            .finish()
    }
}

/// How the component double answers a non-streaming exchange.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NorthPassthroughBufferedVerdictV1 {
    /// `parse-response` (or, for a non-2xx status, `map-provider-error`) succeeds.
    Parses,
    /// `parse-response` returns an error.
    Refuses,
}

fixed_debug!(NorthPassthroughBufferedVerdictV1 {
    Parses => "Parses",
    Refuses => "Refuses",
});

/// The request facts that decide whether a host passes bytes through (record §8.2 conditions 1
/// and 2, and the declaration).
#[derive(Clone, PartialEq, Eq)]
pub struct NorthPassthroughSetupV1 {
    inbound: NorthProtocolV1,
    declared: Option<NorthProtocolV1>,
    client_tools: Value,
}

impl NorthPassthroughSetupV1 {
    /// The northbound protocol the client spoke.
    #[must_use]
    pub const fn inbound(&self) -> NorthProtocolV1 {
        self.inbound
    }

    /// The family's `north_passthrough` declaration, if any.
    #[must_use]
    pub const fn declared(&self) -> Option<NorthProtocolV1> {
        self.declared
    }

    /// The client's `tools`, in the inbound protocol's wire form. The host runs its own lossless
    /// test on them (record §8.2 condition 2).
    #[must_use]
    pub const fn client_tools(&self) -> &Value {
        &self.client_tools
    }
}

impl fmt::Debug for NorthPassthroughSetupV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tools = self.client_tools.as_array().map_or(0, Vec::len);
        formatter
            .debug_struct("NorthPassthroughSetupV1")
            .field("inbound", &self.inbound)
            .field("declared", &self.declared)
            .field("client_tool_count", &tools)
            .finish()
    }
}

/// The delivery a host chose before dispatch.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NorthPassthroughDeliveryV1 {
    /// The upstream's bytes are forwarded.
    PassThrough,
    /// The component's IR is rendered through the north codec.
    Rendered,
    /// The host refused the request before dispatch (a host that admits only plain functions).
    RefusedBeforeDispatch,
}

fixed_debug!(NorthPassthroughDeliveryV1 {
    PassThrough => "PassThrough",
    Rendered => "Rendered",
    RefusedBeforeDispatch => "RefusedBeforeDispatch",
});

/// How a streaming case must end.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NorthPassthroughStreamOutcomeV1 {
    /// Every frame is forwarded unchanged, the terminal frame (repaired if the end of input
    /// dispatched it) only after the settlement is persisted.
    Completes,
    /// The first `relayed` frames are forwarded, then, after the funds transition, the host's own
    /// failure event (Responses) or nothing at all (`generate_content`); the stream is never
    /// settled as a success.
    FailsAfter {
        /// How many upstream frames the client receives before the failure event.
        relayed: usize,
        /// How many `parse-stream-chunk` calls the host makes.
        fed: usize,
    },
}

impl fmt::Debug for NorthPassthroughStreamOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Completes => formatter.write_str("Completes"),
            Self::FailsAfter { relayed, fed } => formatter
                .debug_struct("FailsAfter")
                .field("relayed", relayed)
                .field("fed", fed)
                .finish(),
        }
    }
}

/// How the suite hands a body to the host's streaming path.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NorthPassthroughSplitV1 {
    /// As one chunk.
    Whole,
    /// Once as two chunks for every interior split point, then once as one byte per chunk.
    EveryByteBoundary,
}

fixed_debug!(NorthPassthroughSplitV1 {
    Whole => "Whole",
    EveryByteBoundary => "EveryByteBoundary",
});

/// What one case drives.
#[derive(Clone, PartialEq, Eq)]
pub enum NorthPassthroughPathV1 {
    /// The delivery decision for each setup; every one must give an allowed answer.
    Delivery {
        /// The setups.
        setups: Vec<NorthPassthroughSetupV1>,
        /// Whether pass-through is the expected answer (`true`), or must not be given (`false`).
        passes_through: bool,
    },
    /// A streaming exchange.
    Stream {
        /// The upstream body.
        body: NorthPassthroughBodyV1,
        /// How it arrives.
        split: NorthPassthroughSplitV1,
        /// The component double's script.
        script: NorthPassthroughScriptV1,
        /// How it must end.
        outcome: NorthPassthroughStreamOutcomeV1,
    },
    /// A non-streaming exchange.
    Buffered {
        /// The upstream status.
        status: u16,
        /// The upstream body.
        body: &'static str,
        /// The double's answer.
        verdict: NorthPassthroughBufferedVerdictV1,
        /// Whether the body must be forwarded (`true`) or only the host's own error.
        forwarded: bool,
    },
}

impl fmt::Debug for NorthPassthroughPathV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Delivery { setups, passes_through } => formatter
                .debug_struct("Delivery")
                .field("setups", setups)
                .field("passes_through", passes_through)
                .finish(),
            Self::Stream { body, split, script, outcome } => formatter
                .debug_struct("Stream")
                .field("body", body)
                .field("split", split)
                .field("script", script)
                .field("outcome", outcome)
                .finish(),
            Self::Buffered { status, body, verdict, forwarded } => formatter
                .debug_struct("Buffered")
                .field("status", status)
                .field("body_byte_count", &body.len())
                .field("verdict", verdict)
                .field("forwarded", forwarded)
                .finish(),
        }
    }
}

/// The closed set of canonical pass-through delivery cases.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NorthPassthroughCaseIdV1 {
    /// A Responses request to a family declaring `responses`, with plain client functions, passes
    /// through.
    DeliveryPassesThroughWhenEveryConditionHolds,
    /// A client tool the codec renames (a namespaced tool) is rendered or refused, never passed
    /// through.
    LossyToolMapRenders,
    /// A family that declares no `north_passthrough` is rendered.
    UndeclaredFamilyRenders,
    /// A Chat or Messages client, or a declaration naming another protocol, is rendered.
    OtherSurfaceRenders,
    /// The upstream's bytes reach the client unchanged, fed one frame per call, at every split.
    ForwardedUnchanged,
    /// No frame's byte is forwarded before that frame's call returned.
    ForwardAfterReturn,
    /// The terminal frame is released only after the settlement is persisted.
    TerminalWithheldUntilSettled,
    /// `response.incomplete` is a terminal type like `response.completed`.
    IncompleteIsTerminal,
    /// A terminal frame (a terminal type; for `generate_content`, a `finishReason`) whose call did
    /// not return `Done` parks the stream.
    TerminalTypeWithoutDoneParks,
    /// `Done` from a call whose frame is not a terminal frame parks the stream.
    DoneWithoutTerminalTypeParks,
    /// A component that holds a frame back (its `Done` arrives only at `finish`) parks the stream.
    LaggingComponentParks,
    /// An upstream `response.failed` frame is replaced by the host's failure event.
    UpstreamFailureReplaced,
    /// An upstream `error` frame without `sequence_number` is replaced by the host's failure event.
    ErrorEventReplaced,
    /// A component error on an ordinary frame forwards nothing more.
    ComponentErrorForwardsNothing,
    /// A failing component call forwards nothing more.
    ComponentTrapForwardsNothing,
    /// A failure on the first frame: the client sees only the host's own created and failed events
    /// (Responses), or no frame at all (`generate_content`).
    FailureBeforeFirstFrame,
    /// A stream that ends before its terminal frame is never settled as a success.
    TruncatedStreamFails,
    /// When persisting the settlement fails, the terminal frame is never released.
    SettlementFailureWithholdsTerminal,
    /// A terminal frame the end of input dispatched is released with the minimal terminator.
    TerminalAtEndOfInputIsRepaired,
    /// A non-streaming body is forwarded byte for byte, after the parse and the settlement.
    BufferedForwardedAfterParseAndSettlement,
    /// A non-streaming body the component refused is never forwarded.
    BufferedParseFailureNotRelayed,
    /// A non-2xx upstream body is never forwarded (Responses).
    BufferedNon2xxNotRelayed,
    /// `generate_content`: a `finishReason` other than `STOP` ends the stream like `STOP`.
    OtherFinishReasonIsTerminal,
    /// `generate_content`: a `finishReason` on any candidate, not only the first, marks the
    /// terminal frame.
    FinishReasonOnAnyCandidateIsTerminal,
    /// `generate_content`: an upstream `error` frame is never forwarded, and the host writes no
    /// frame of its own in its place.
    UpstreamErrorFrameNotRelayed,
    /// `generate_content`: a non-2xx upstream answer reaches the client as the upstream sent it,
    /// status and body, after `map-provider-error` returned and the funds transition.
    BufferedNon2xxDeliveredAsSent,
}

fixed_debug!(NorthPassthroughCaseIdV1 {
    DeliveryPassesThroughWhenEveryConditionHolds => "DeliveryPassesThroughWhenEveryConditionHolds",
    LossyToolMapRenders => "LossyToolMapRenders",
    UndeclaredFamilyRenders => "UndeclaredFamilyRenders",
    OtherSurfaceRenders => "OtherSurfaceRenders",
    ForwardedUnchanged => "ForwardedUnchanged",
    ForwardAfterReturn => "ForwardAfterReturn",
    TerminalWithheldUntilSettled => "TerminalWithheldUntilSettled",
    IncompleteIsTerminal => "IncompleteIsTerminal",
    TerminalTypeWithoutDoneParks => "TerminalTypeWithoutDoneParks",
    DoneWithoutTerminalTypeParks => "DoneWithoutTerminalTypeParks",
    LaggingComponentParks => "LaggingComponentParks",
    UpstreamFailureReplaced => "UpstreamFailureReplaced",
    ErrorEventReplaced => "ErrorEventReplaced",
    ComponentErrorForwardsNothing => "ComponentErrorForwardsNothing",
    ComponentTrapForwardsNothing => "ComponentTrapForwardsNothing",
    FailureBeforeFirstFrame => "FailureBeforeFirstFrame",
    TruncatedStreamFails => "TruncatedStreamFails",
    SettlementFailureWithholdsTerminal => "SettlementFailureWithholdsTerminal",
    TerminalAtEndOfInputIsRepaired => "TerminalAtEndOfInputIsRepaired",
    BufferedForwardedAfterParseAndSettlement => "BufferedForwardedAfterParseAndSettlement",
    BufferedParseFailureNotRelayed => "BufferedParseFailureNotRelayed",
    BufferedNon2xxNotRelayed => "BufferedNon2xxNotRelayed",
    OtherFinishReasonIsTerminal => "OtherFinishReasonIsTerminal",
    FinishReasonOnAnyCandidateIsTerminal => "FinishReasonOnAnyCandidateIsTerminal",
    UpstreamErrorFrameNotRelayed => "UpstreamErrorFrameNotRelayed",
    BufferedNon2xxDeliveredAsSent => "BufferedNon2xxDeliveredAsSent",
});

/// One immutable canonical pass-through delivery case.
#[derive(Clone, PartialEq, Eq)]
pub struct NorthPassthroughFixtureV1 {
    case_id: NorthPassthroughCaseIdV1,
    path: NorthPassthroughPathV1,
}

impl NorthPassthroughFixtureV1 {
    /// The stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> NorthPassthroughCaseIdV1 {
        self.case_id
    }

    /// What the case drives.
    #[must_use]
    pub const fn path(&self) -> &NorthPassthroughPathV1 {
        &self.path
    }
}

impl fmt::Debug for NorthPassthroughFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NorthPassthroughFixtureV1")
            .field("case_id", &self.case_id)
            .field("path", &self.path)
            .finish()
    }
}

// -- bodies ---------------------------------------------------------------------------------------

const MIXED_HEAD: &[u8] = b"\xEF\xBB\xBF: keep-alive\r\n\
event: response.created\r\n\
data: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"resp_mixed\",\
\"object\":\"response\",\"created_at\":1756367388,\"model\":\"gpt-5.5\",\"status\":\"in_progress\",\
\"output\":[]}}\r\n\
\r\n\
event: response.output_text.delta\r\
data: {\"type\":\"response.output_text.delta\",\r\
data: \"sequence_number\":1,\"output_index\":0,\"content_index\":0,\"delta\":\"Hi\"}\r\
\r\
: ping\n\
\n";

const MIXED_TERMINAL: &[u8] = b"event: response.completed\n\
data: {\"type\":\"response.completed\",\"sequence_number\":2,\"response\":{\"id\":\"resp_mixed\",\
\"object\":\"response\",\"created_at\":1756367388,\"model\":\"gpt-5.5\",\"status\":\"completed\",\
\"output\":[],\"usage\":{\"input_tokens\":3,\"output_tokens\":1,\"total_tokens\":4}}}\n\
\n";

const ERROR_EVENT_TEXT: &str = "Upstream overloaded; retry in 7 seconds (internal shard 31).";

const ERROR_EVENT: &[u8] = b"event: error\n\
data: {\"type\":\"error\",\"error\":{\"type\":\"server_error\",\"code\":\"server_error\",\
\"message\":\"Upstream overloaded; retry in 7 seconds (internal shard 31).\"}}\n\
\n";

const OPENAI_FAILED_TEXT: &str =
    "The server had an error while processing your request. Sorry about that!";

const OPENAI_ERROR_FIRST_TEXT: &str = "Your input exceeds the context window of this model.";

/// `provider.stream.text` of `fixtures-openai-responses`, its chunks joined.
const OPENAI_TEXT: &str = include_str!("north_passthrough/openai_text.sse");
/// `provider.stream.incomplete` of `fixtures-openai-responses`, its chunks joined.
const OPENAI_INCOMPLETE: &str = include_str!("north_passthrough/openai_incomplete.sse");
/// `provider.stream.failed` of `fixtures-openai-responses`, its chunks joined.
const OPENAI_FAILED: &str = include_str!("north_passthrough/openai_failed.sse");
/// `provider.stream.error-event-without-sequence` of `fixtures-openai-responses`.
const OPENAI_ERROR_FIRST: &str = include_str!("north_passthrough/openai_error_first.sse");

const GEMINI_MIXED_HEAD: &[u8] = b"\xEF\xBB\xBF: keep-alive\r\n\
data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"18C \"}]},\
\"index\":0}],\"modelVersion\":\"gemini-2.5-flash\",\"responseId\":\"gc_mixed\"}\r\n\
\r\n\
data: {\"candidates\":[{\"content\":{\"role\":\"model\",\r\
data: \"parts\":[{\"text\":\"and sunny.\"}]},\"index\":0}],\"modelVersion\":\"gemini-2.5-flash\",\
\"responseId\":\"gc_mixed\"}\r\
\r\
: ping\n\
\n";

const GEMINI_MIXED_TERMINAL: &[u8] = b"data: {\"candidates\":[{\"content\":{\"role\":\"model\",\
\"parts\":[]},\"finishReason\":\"STOP\",\"index\":0}],\"usageMetadata\":{\"promptTokenCount\":10,\
\"candidatesTokenCount\":5,\"totalTokenCount\":15},\"modelVersion\":\"gemini-2.5-flash\",\
\"responseId\":\"gc_mixed\"}\n\
\n";

const GEMINI_SECOND_CANDIDATE: &[u8] = b"data: {\"candidates\":[{\"content\":{\"role\":\"model\",\
\"parts\":[{\"text\":\"Yes.\"}]},\"finishReason\":null,\"index\":0},{\"content\":{\"role\":\"model\",\
\"parts\":[{\"text\":\"No.\"}]},\"index\":1}]}\r\n\
\r\n\
data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[]},\"index\":0},\
{\"content\":{\"role\":\"model\",\"parts\":[]},\"finishReason\":\"STOP\",\"index\":1}],\
\"usageMetadata\":{\"promptTokenCount\":4,\"candidatesTokenCount\":2,\"totalTokenCount\":6}}\r\n\
\r\n";

const GEMINI_ERROR_EVENT_TEXT: &str =
    "The model is overloaded. Please try again later. (backend pool 9c)";

const GEMINI_ERROR_EVENT: &[u8] = b"data: {\"error\":{\"code\":503,\"message\":\"The model is \
overloaded. Please try again later. (backend pool 9c)\",\"status\":\"UNAVAILABLE\"}}\r\n\
\r\n";

const GEMINI_ERROR_FIRST_TEXT: &str =
    "The input token count (1048577) exceeds the maximum number of tokens allowed (1048576).";

const GEMINI_ERROR_FIRST: &[u8] = b"data: {\"error\":{\"code\":400,\"message\":\"The input token \
count (1048577) exceeds the maximum number of tokens allowed (1048576).\",\"status\":\
\"INVALID_ARGUMENT\"}}\r\n\
\r\n";

/// `provider.stream.text-and-terminal` of `fixtures-gemini`, its chunks joined.
const GEMINI_TEXT: &str = include_str!("north_passthrough/gemini_text.sse");
/// `provider.stream.eof-terminates-without-a-usage-frame` of `fixtures-gemini`, its chunks joined.
const GEMINI_MAX_TOKENS: &str = include_str!("north_passthrough/gemini_max_tokens.sse");

const GEMINI_BUFFERED_OK: &str = "{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":\
[{\"text\":\"Hello.\"}]},\"finishReason\":\"STOP\",\"index\":0}],\"usageMetadata\":\
{\"promptTokenCount\":3,\"candidatesTokenCount\":2,\"totalTokenCount\":5},\"modelVersion\":\
\"gemini-2.5-flash\",\"responseId\":\"gc_buffered\"}";

const GEMINI_BUFFERED_REFUSED: &str = "{\"error\":{\"code\":500,\"message\":\"Internal error \
encountered in cell 4; request dropped.\",\"status\":\"INTERNAL\"}}";

const GEMINI_BUFFERED_429: &str = "{\"error\":{\"code\":429,\"message\":\"Resource has been \
exhausted (e.g. check quota).\",\"status\":\"RESOURCE_EXHAUSTED\"}}";

const BUFFERED_OK: &str = "{\"id\":\"resp_buffered\",\"object\":\"response\",\"created_at\":1756367388,\
\"status\":\"completed\",\"error\":null,\"model\":\"gpt-5.5\",\"output\":[{\"id\":\"msg_1\",\
\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\
\"output_text\",\"annotations\":[],\"text\":\"Hello.\"}]}],\"usage\":{\"input_tokens\":3,\
\"output_tokens\":2,\"total_tokens\":5}}";

const BUFFERED_REFUSED: &str = "{\"id\":\"resp_buffered\",\"object\":\"response\",\"status\":\
\"failed\",\"error\":{\"code\":\"server_error\",\"message\":\"Shard 12 lost its lease; request \
abandoned.\"},\"output\":[]}";

const BUFFERED_429: &str = "{\"error\":{\"message\":\"Rate limit reached for org-7Qx on tokens \
per min; limit 30000, used 29981.\",\"type\":\"tokens\",\"code\":\"rate_limit_exceeded\"}}";

/// The upstream's own text in each buffered body that must never reach the client. A body the
/// suite expects delivered as sent (a `generate_content` non-2xx answer) has none.
#[must_use]
pub fn north_passthrough_buffered_upstream_text_v1(body: &str) -> Option<&'static str> {
    if body == BUFFERED_REFUSED {
        Some("Shard 12 lost its lease; request abandoned.")
    } else if body == BUFFERED_429 {
        Some("Rate limit reached for org-7Qx on tokens per min; limit 30000, used 29981.")
    } else if body == GEMINI_BUFFERED_REFUSED {
        Some("Internal error encountered in cell 4; request dropped.")
    } else {
        None
    }
}

// -- table ----------------------------------------------------------------------------------------

fn plain_tools() -> Value {
    serde_json::json!([{"type": "function", "name": "get_weather",
                        "parameters": {"type": "object", "properties": {}}}])
}

fn namespaced_tools() -> Value {
    serde_json::json!([{"type": "function", "name": "get_weather", "namespace": "weather",
                        "parameters": {"type": "object", "properties": {}}}])
}

const fn setup(
    inbound: NorthProtocolV1,
    declared: Option<NorthProtocolV1>,
    client_tools: Value,
) -> NorthPassthroughSetupV1 {
    NorthPassthroughSetupV1 { inbound, declared, client_tools }
}

fn script(
    frames: &[NorthPassthroughFrameVerdictV1],
    finish: NorthPassthroughFinishVerdictV1,
    settlement: NorthPassthroughSettlementV1,
) -> NorthPassthroughScriptV1 {
    NorthPassthroughScriptV1 { frames: frames.to_vec(), finish, settlement }
}

const fn stream(
    case_id: NorthPassthroughCaseIdV1,
    body: NorthPassthroughBodyV1,
    split: NorthPassthroughSplitV1,
    script: NorthPassthroughScriptV1,
    outcome: NorthPassthroughStreamOutcomeV1,
) -> NorthPassthroughFixtureV1 {
    NorthPassthroughFixtureV1 {
        case_id,
        path: NorthPassthroughPathV1::Stream { body, split, script, outcome },
    }
}

/// The canonical pass-through delivery cases, in table order.
#[must_use]
#[expect(clippy::too_many_lines, reason = "one flat table of every case, in order")]
pub fn north_passthrough_fixtures_v1() -> Vec<NorthPassthroughFixtureV1> {
    use NorthPassthroughCaseIdV1 as Case;
    use NorthPassthroughFinishVerdictV1 as Finish;
    use NorthPassthroughFrameVerdictV1::{Done, Error, Events, Trap};
    use NorthPassthroughSettlementV1::{Fails, Persists};
    use NorthPassthroughSplitV1::{EveryByteBoundary, Whole};
    use NorthPassthroughStreamOutcomeV1::{Completes, FailsAfter};
    use NorthProtocolV1::{ChatCompletions, Messages, Responses};

    let completes = script(&[Events, Events, Done], Finish::Clean, Persists);
    let delivery = |case_id, setups, passes_through| NorthPassthroughFixtureV1 {
        case_id,
        path: NorthPassthroughPathV1::Delivery { setups, passes_through },
    };
    let buffered = |case_id, status, body, verdict, forwarded| NorthPassthroughFixtureV1 {
        case_id,
        path: NorthPassthroughPathV1::Buffered { status, body, verdict, forwarded },
    };
    let text_frames = 13;
    let incomplete_frames = 9;
    vec![
        delivery(
            Case::DeliveryPassesThroughWhenEveryConditionHolds,
            vec![
                setup(Responses, Some(Responses), plain_tools()),
                setup(Responses, Some(Responses), serde_json::json!([])),
            ],
            true,
        ),
        delivery(
            Case::LossyToolMapRenders,
            vec![setup(Responses, Some(Responses), namespaced_tools())],
            false,
        ),
        delivery(Case::UndeclaredFamilyRenders, vec![setup(Responses, None, plain_tools())], false),
        delivery(
            Case::OtherSurfaceRenders,
            vec![
                setup(ChatCompletions, Some(Responses), plain_tools()),
                setup(Messages, Some(Responses), plain_tools()),
                setup(Responses, Some(ChatCompletions), plain_tools()),
            ],
            false,
        ),
        stream(
            Case::ForwardedUnchanged,
            NorthPassthroughBodyV1::MixedFraming,
            EveryByteBoundary,
            completes.clone(),
            Completes,
        ),
        stream(
            Case::ForwardAfterReturn,
            NorthPassthroughBodyV1::OpenAiText,
            Whole,
            script(&[vec![Events; text_frames - 1], vec![Done]].concat(), Finish::Clean, Persists),
            Completes,
        ),
        stream(
            Case::TerminalWithheldUntilSettled,
            NorthPassthroughBodyV1::MixedFraming,
            Whole,
            completes.clone(),
            Completes,
        ),
        stream(
            Case::IncompleteIsTerminal,
            NorthPassthroughBodyV1::OpenAiIncomplete,
            Whole,
            script(
                &[vec![Events; incomplete_frames - 1], vec![Done]].concat(),
                Finish::Clean,
                Persists,
            ),
            Completes,
        ),
        stream(
            Case::TerminalTypeWithoutDoneParks,
            NorthPassthroughBodyV1::MixedFraming,
            Whole,
            script(&[Events, Events, Events], Finish::Clean, Persists),
            FailsAfter { relayed: 2, fed: 3 },
        ),
        stream(
            Case::DoneWithoutTerminalTypeParks,
            NorthPassthroughBodyV1::MixedFraming,
            Whole,
            script(&[Events, Done, Done], Finish::Clean, Persists),
            FailsAfter { relayed: 1, fed: 2 },
        ),
        stream(
            Case::LaggingComponentParks,
            NorthPassthroughBodyV1::MixedFraming,
            Whole,
            script(&[Events, Events, Events], Finish::Done, Persists),
            FailsAfter { relayed: 2, fed: 3 },
        ),
        stream(
            Case::UpstreamFailureReplaced,
            NorthPassthroughBodyV1::OpenAiFailed,
            Whole,
            script(&[Events, Events, Error], Finish::Clean, Persists),
            FailsAfter { relayed: 2, fed: 3 },
        ),
        stream(
            Case::ErrorEventReplaced,
            NorthPassthroughBodyV1::ErrorEventAfterFrames,
            Whole,
            script(&[Events, Events, Error], Finish::Clean, Persists),
            FailsAfter { relayed: 2, fed: 3 },
        ),
        stream(
            Case::ComponentErrorForwardsNothing,
            NorthPassthroughBodyV1::MixedFraming,
            Whole,
            script(&[Events, Error, Done], Finish::Clean, Persists),
            FailsAfter { relayed: 1, fed: 2 },
        ),
        stream(
            Case::ComponentTrapForwardsNothing,
            NorthPassthroughBodyV1::MixedFraming,
            Whole,
            script(&[Events, Trap, Done], Finish::Clean, Persists),
            FailsAfter { relayed: 1, fed: 2 },
        ),
        stream(
            Case::FailureBeforeFirstFrame,
            NorthPassthroughBodyV1::OpenAiErrorFirst,
            Whole,
            script(&[Error], Finish::Clean, Persists),
            FailsAfter { relayed: 0, fed: 1 },
        ),
        stream(
            Case::TruncatedStreamFails,
            NorthPassthroughBodyV1::Truncated,
            Whole,
            script(&[Events, Events], Finish::Truncated, Persists),
            FailsAfter { relayed: 2, fed: 2 },
        ),
        stream(
            Case::SettlementFailureWithholdsTerminal,
            NorthPassthroughBodyV1::MixedFraming,
            Whole,
            script(&[Events, Events, Done], Finish::Clean, Fails),
            FailsAfter { relayed: 2, fed: 3 },
        ),
        stream(
            Case::TerminalAtEndOfInputIsRepaired,
            NorthPassthroughBodyV1::TerminalWithoutBlankLine,
            Whole,
            completes.clone(),
            Completes,
        ),
        stream(
            Case::TerminalAtEndOfInputIsRepaired,
            NorthPassthroughBodyV1::TerminalWithoutLineEnd,
            Whole,
            completes,
            Completes,
        ),
        buffered(
            Case::BufferedForwardedAfterParseAndSettlement,
            200,
            BUFFERED_OK,
            NorthPassthroughBufferedVerdictV1::Parses,
            true,
        ),
        buffered(
            Case::BufferedParseFailureNotRelayed,
            200,
            BUFFERED_REFUSED,
            NorthPassthroughBufferedVerdictV1::Refuses,
            false,
        ),
        buffered(
            Case::BufferedNon2xxNotRelayed,
            429,
            BUFFERED_429,
            NorthPassthroughBufferedVerdictV1::Parses,
            false,
        ),
    ]
}

fn gemini_tools() -> Value {
    serde_json::json!([{"functionDeclarations": [{"name": "get_weather",
                        "parameters": {"type": "object", "properties": {}}}]}])
}

/// The canonical pass-through delivery cases of suite version two for `protocol`, in table order.
///
/// For [`NorthPassthroughProtocolV2::Responses`] they are exactly [`north_passthrough_fixtures_v1`].
#[must_use]
pub fn north_passthrough_fixtures_v2(
    protocol: NorthPassthroughProtocolV2,
) -> Vec<NorthPassthroughFixtureV1> {
    match protocol {
        NorthPassthroughProtocolV2::Responses => north_passthrough_fixtures_v1(),
        NorthPassthroughProtocolV2::GenerateContent => generate_content_fixtures(),
    }
}

/// The `generate_content` table (design record
/// `docs/design/2026-10-14-generate-content-request-passthrough.md` §6). The client's tools never
/// decide delivery: the request is passed through too, so no codec maps them.
#[expect(clippy::too_many_lines, reason = "one flat table of every case, in order")]
fn generate_content_fixtures() -> Vec<NorthPassthroughFixtureV1> {
    use NorthPassthroughBodyV1 as Body;
    use NorthPassthroughCaseIdV1 as Case;
    use NorthPassthroughFinishVerdictV1 as Finish;
    use NorthPassthroughFrameVerdictV1::{Done, Error, Events, Trap};
    use NorthPassthroughSettlementV1::{Fails, Persists};
    use NorthPassthroughSplitV1::{EveryByteBoundary, Whole};
    use NorthPassthroughStreamOutcomeV1::{Completes, FailsAfter};
    use NorthProtocolV1::{ChatCompletions, GenerateContent, Messages, Responses};

    let completes = script(&[Events, Events, Done], Finish::Clean, Persists);
    let delivery = |case_id, setups, passes_through| NorthPassthroughFixtureV1 {
        case_id,
        path: NorthPassthroughPathV1::Delivery { setups, passes_through },
    };
    let buffered = |case_id, status, body, verdict, forwarded| NorthPassthroughFixtureV1 {
        case_id,
        path: NorthPassthroughPathV1::Buffered { status, body, verdict, forwarded },
    };
    vec![
        delivery(
            Case::DeliveryPassesThroughWhenEveryConditionHolds,
            vec![
                setup(GenerateContent, Some(GenerateContent), gemini_tools()),
                setup(GenerateContent, Some(GenerateContent), serde_json::json!([])),
            ],
            true,
        ),
        delivery(
            Case::UndeclaredFamilyRenders,
            vec![setup(GenerateContent, None, gemini_tools())],
            false,
        ),
        delivery(
            Case::OtherSurfaceRenders,
            vec![
                setup(Responses, Some(GenerateContent), plain_tools()),
                setup(ChatCompletions, Some(GenerateContent), plain_tools()),
                setup(Messages, Some(GenerateContent), plain_tools()),
                setup(GenerateContent, Some(Responses), gemini_tools()),
            ],
            false,
        ),
        stream(
            Case::ForwardedUnchanged,
            Body::GeminiMixedFraming,
            EveryByteBoundary,
            completes.clone(),
            Completes,
        ),
        stream(
            Case::ForwardAfterReturn,
            Body::GeminiText,
            Whole,
            script(&[Events, Events, Done], Finish::Clean, Persists),
            Completes,
        ),
        stream(
            Case::TerminalWithheldUntilSettled,
            Body::GeminiMixedFraming,
            Whole,
            completes.clone(),
            Completes,
        ),
        stream(
            Case::OtherFinishReasonIsTerminal,
            Body::GeminiMaxTokens,
            Whole,
            script(&[Events, Done], Finish::Clean, Persists),
            Completes,
        ),
        stream(
            Case::FinishReasonOnAnyCandidateIsTerminal,
            Body::GeminiSecondCandidateFinishes,
            Whole,
            script(&[Events, Done], Finish::Clean, Persists),
            Completes,
        ),
        stream(
            Case::TerminalTypeWithoutDoneParks,
            Body::GeminiMixedFraming,
            Whole,
            script(&[Events, Events, Events], Finish::Clean, Persists),
            FailsAfter { relayed: 2, fed: 3 },
        ),
        stream(
            Case::DoneWithoutTerminalTypeParks,
            Body::GeminiMixedFraming,
            Whole,
            script(&[Events, Done, Done], Finish::Clean, Persists),
            FailsAfter { relayed: 1, fed: 2 },
        ),
        stream(
            Case::LaggingComponentParks,
            Body::GeminiMixedFraming,
            Whole,
            script(&[Events, Events, Events], Finish::Done, Persists),
            FailsAfter { relayed: 2, fed: 3 },
        ),
        stream(
            Case::UpstreamErrorFrameNotRelayed,
            Body::GeminiErrorAfterFrames,
            Whole,
            script(&[Events, Events, Error], Finish::Clean, Persists),
            FailsAfter { relayed: 2, fed: 3 },
        ),
        stream(
            Case::ComponentErrorForwardsNothing,
            Body::GeminiMixedFraming,
            Whole,
            script(&[Events, Error, Done], Finish::Clean, Persists),
            FailsAfter { relayed: 1, fed: 2 },
        ),
        stream(
            Case::ComponentTrapForwardsNothing,
            Body::GeminiMixedFraming,
            Whole,
            script(&[Events, Trap, Done], Finish::Clean, Persists),
            FailsAfter { relayed: 1, fed: 2 },
        ),
        stream(
            Case::FailureBeforeFirstFrame,
            Body::GeminiErrorFirst,
            Whole,
            script(&[Error], Finish::Clean, Persists),
            FailsAfter { relayed: 0, fed: 1 },
        ),
        stream(
            Case::TruncatedStreamFails,
            Body::GeminiTruncated,
            Whole,
            script(&[Events, Events], Finish::Truncated, Persists),
            FailsAfter { relayed: 2, fed: 2 },
        ),
        stream(
            Case::SettlementFailureWithholdsTerminal,
            Body::GeminiMixedFraming,
            Whole,
            script(&[Events, Events, Done], Finish::Clean, Fails),
            FailsAfter { relayed: 2, fed: 3 },
        ),
        stream(
            Case::TerminalAtEndOfInputIsRepaired,
            Body::GeminiTerminalWithoutBlankLine,
            Whole,
            completes.clone(),
            Completes,
        ),
        stream(
            Case::TerminalAtEndOfInputIsRepaired,
            Body::GeminiTerminalWithoutLineEnd,
            Whole,
            completes,
            Completes,
        ),
        buffered(
            Case::BufferedForwardedAfterParseAndSettlement,
            200,
            GEMINI_BUFFERED_OK,
            NorthPassthroughBufferedVerdictV1::Parses,
            true,
        ),
        buffered(
            Case::BufferedParseFailureNotRelayed,
            200,
            GEMINI_BUFFERED_REFUSED,
            NorthPassthroughBufferedVerdictV1::Refuses,
            false,
        ),
        buffered(
            Case::BufferedNon2xxDeliveredAsSent,
            429,
            GEMINI_BUFFERED_429,
            NorthPassthroughBufferedVerdictV1::Parses,
            true,
        ),
    ]
}
