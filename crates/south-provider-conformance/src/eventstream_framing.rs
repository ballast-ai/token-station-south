//! Canonical cases for the eventstream-framing host suite (gate ③ of B2,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §5.2 and §5.4).
//!
//! South provides the AWS eventstream deframer and its canonical SSE re-encoding
//! (`south_contracts::AwsEventStreamDeframerV1`, `south_contracts::reencode_eventstream_v1`), and
//! golden vectors pin both. They cannot pin the host's wiring around them: that the host picks the
//! framing by the package's `stream_framing` declaration, feeds `parse-stream-chunk` the canonical
//! re-encoding rather than the upstream bytes, carries a frame split across chunks, calls `finish`
//! at end of input, and ends a faulty stream as failed after delivering every message before the
//! fault — never passing raw bytes through and never skipping a frame. On the buffered path
//! (`aws-eventstream` with a family declaring `request_facts.stream: "none"`) it must hand
//! `parse-response` the concatenated re-encoding, or nothing at all. This suite drives a host's own
//! framing executor and checks those from the outside.
//!
//! The suite is host-implemented: a host provides [`EventStreamFramingHarnessV1`] around the code
//! between its transport and the component, and runs [`run_eventstream_framing_conformance_v1`].
//! The upstream bodies are built by a test-only encoder kept in this crate; one of its frames is
//! pinned as hex computed outside Rust, so the encoder cannot drift unnoticed.

use std::fmt;

use south_provider_api::StreamFramingV1;

mod encoder;
mod runner;

pub use runner::{
    EventStreamBufferedObservationV1, EventStreamFeedObservationV1,
    EventStreamFramingConformanceFailureV1, EventStreamFramingConformanceReportV1,
    EventStreamFramingHarnessV1, EventStreamFramingMismatchCategoryV1,
    EventStreamFramingMismatchV1, EventStreamOutcomeV1, run_eventstream_framing_conformance_v1,
};

/// The eventstream-framing conformance suite version.
pub const EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The stable identifier for eventstream-framing conformance version one.
pub const EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_ID: &str = "south.eventstream-framing.v1";

/// An upstream body the suite feeds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EventStreamFramingBodyV1 {
    /// A plain SSE byte stream, as an upstream under `bytes` framing sends it.
    SseText,
    /// A complete Converse stream of five `event` messages whose payloads carry insignificant
    /// whitespace, member orders no serializer would choose and a number in exponent form.
    ConverseStream,
    /// One `event`, one `exception` and one `error` message.
    ExceptionAndError,
    /// A valid message, a message whose message checksum is wrong, then another valid message.
    ChecksumMismatchMidStream,
    /// A valid message, then the first 20 bytes of another.
    TruncatedTail,
    /// A valid message, then an `event` message whose payload is not JSON.
    NonJsonPayload,
}

fixed_debug!(EventStreamFramingBodyV1 {
    SseText => "SseText",
    ConverseStream => "ConverseStream",
    ExceptionAndError => "ExceptionAndError",
    ChecksumMismatchMidStream => "ChecksumMismatchMidStream",
    TruncatedTail => "TruncatedTail",
    NonJsonPayload => "NonJsonPayload",
});

impl EventStreamFramingBodyV1 {
    /// The body's bytes, exactly as the upstream sends them.
    #[must_use]
    pub fn bytes(self) -> Vec<u8> {
        match self {
            Self::SseText => SSE_TEXT.to_vec(),
            Self::ConverseStream => encoder::converse_stream(),
            Self::ExceptionAndError => encoder::exception_and_error(),
            Self::ChecksumMismatchMidStream => encoder::checksum_mismatch_mid_stream(),
            Self::TruncatedTail => encoder::truncated_tail(),
            Self::NonJsonPayload => encoder::non_json_payload(),
        }
    }
}

/// How the suite hands a body to the host's streaming path.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EventStreamSplitV1 {
    /// As one chunk.
    Whole,
    /// Once as two chunks for every interior split point, then once as one byte per chunk. The
    /// host must deliver the same bytes and the same outcome every time.
    EveryByteBoundary,
}

fixed_debug!(EventStreamSplitV1 {
    Whole => "Whole",
    EveryByteBoundary => "EveryByteBoundary",
});

/// Which of the host's paths a case drives.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EventStreamFramingPathV1 {
    /// A streaming caller: the host feeds `parse-stream-chunk`.
    Streaming {
        /// The package's `stream_framing` declaration.
        framing: StreamFramingV1,
        /// How the body arrives.
        split: EventStreamSplitV1,
    },
    /// A non-streaming caller of an `aws-eventstream` package whose family declares
    /// `request_facts.stream: "none"`: the host hands `parse-response` one text.
    Buffered,
}

impl fmt::Debug for EventStreamFramingPathV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Streaming { framing, split } => formatter
                .debug_struct("Streaming")
                .field("framing", framing)
                .field("split", split)
                .finish(),
            Self::Buffered => formatter.write_str("Buffered"),
        }
    }
}

/// What the host must deliver to the component.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EventStreamDeliveredV1 {
    /// The upstream body's own bytes, unchanged.
    BodyUnchanged,
    /// Exactly this text: the concatenated canonical re-encoding of the messages delivered.
    Text(&'static str),
}

impl EventStreamDeliveredV1 {
    /// The bytes expected for `body`.
    #[must_use]
    pub fn bytes(self, body: &[u8]) -> Vec<u8> {
        match self {
            Self::BodyUnchanged => body.to_vec(),
            Self::Text(text) => text.as_bytes().to_vec(),
        }
    }
}

impl fmt::Debug for EventStreamDeliveredV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BodyUnchanged => formatter.write_str("BodyUnchanged"),
            Self::Text(text) => {
                formatter.debug_struct("Text").field("byte_count", &text.len()).finish()
            }
        }
    }
}

/// The expected result of one case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventStreamFramingExpectedV1 {
    delivered: EventStreamDeliveredV1,
    completes: bool,
}

impl EventStreamFramingExpectedV1 {
    /// What the host delivers. On the streaming path, the concatenation of everything fed to
    /// `parse-stream-chunk`, also when the stream fails. On the buffered path, the text handed to
    /// `parse-response`; a buffered case that fails expects the empty text, because the host hands
    /// over nothing.
    #[must_use]
    pub const fn delivered(&self) -> EventStreamDeliveredV1 {
        self.delivered
    }

    /// Whether the stream ends completed (`true`) or failed, or whether the buffered path succeeds.
    #[must_use]
    pub const fn completes(&self) -> bool {
        self.completes
    }
}

impl fmt::Debug for EventStreamFramingExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EventStreamFramingExpectedV1")
            .field("delivered", &self.delivered)
            .field("completes", &self.completes)
            .finish()
    }
}

/// The closed set of canonical eventstream-framing cases.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventStreamFramingCaseIdV1 {
    /// Under `bytes` framing an SSE stream reaches the component unchanged, however it is split.
    BytesFramingPassesBytesUnchanged,
    /// Under `aws-eventstream` each `event` message is fed as its canonical re-encoding, byte for
    /// byte.
    EventFramesReencodeCanonically,
    /// `exception` and `error` messages are fed in their canonical forms, not dropped.
    ExceptionAndErrorFramesPassThrough,
    /// The same stream split at every byte boundary, and one byte per chunk, delivers the same
    /// bytes.
    SplitAtEveryByteBoundaryIsIdentical,
    /// A message checksum mismatch fails the stream after delivering the messages before it, and
    /// nothing after it.
    ChecksumMismatchFailsAfterEarlierMessages,
    /// A partial frame at end of input fails the stream: the host calls `finish`.
    TruncatedTailFailsAtEndOfInput,
    /// A payload that is not JSON fails the stream; it is never passed through.
    NonJsonPayloadFails,
    /// The buffered path hands `parse-response` the concatenated re-encoding.
    BufferedPathConcatenatesTheReencoding,
    /// The buffered path fails on a checksum mismatch, a truncated tail and a non-JSON payload,
    /// and hands over nothing.
    BufferedPathFailsOnAnyFramingError,
}

fixed_debug!(EventStreamFramingCaseIdV1 {
    BytesFramingPassesBytesUnchanged => "BytesFramingPassesBytesUnchanged",
    EventFramesReencodeCanonically => "EventFramesReencodeCanonically",
    ExceptionAndErrorFramesPassThrough => "ExceptionAndErrorFramesPassThrough",
    SplitAtEveryByteBoundaryIsIdentical => "SplitAtEveryByteBoundaryIsIdentical",
    ChecksumMismatchFailsAfterEarlierMessages => "ChecksumMismatchFailsAfterEarlierMessages",
    TruncatedTailFailsAtEndOfInput => "TruncatedTailFailsAtEndOfInput",
    NonJsonPayloadFails => "NonJsonPayloadFails",
    BufferedPathConcatenatesTheReencoding => "BufferedPathConcatenatesTheReencoding",
    BufferedPathFailsOnAnyFramingError => "BufferedPathFailsOnAnyFramingError",
});

/// One immutable canonical eventstream-framing case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventStreamFramingFixtureV1 {
    case_id: EventStreamFramingCaseIdV1,
    path: EventStreamFramingPathV1,
    bodies: &'static [EventStreamFramingBodyV1],
    expected: EventStreamFramingExpectedV1,
}

impl EventStreamFramingFixtureV1 {
    /// The stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> EventStreamFramingCaseIdV1 {
        self.case_id
    }

    /// The path the case drives.
    #[must_use]
    pub const fn path(&self) -> EventStreamFramingPathV1 {
        self.path
    }

    /// The bodies the case feeds, each on its own. Every one must give the expected result.
    #[must_use]
    pub const fn bodies(&self) -> &'static [EventStreamFramingBodyV1] {
        self.bodies
    }

    /// The expected result for every body.
    #[must_use]
    pub const fn expected(&self) -> &EventStreamFramingExpectedV1 {
        &self.expected
    }
}

impl fmt::Debug for EventStreamFramingFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EventStreamFramingFixtureV1")
            .field("case_id", &self.case_id)
            .field("path", &self.path)
            .field("bodies", &self.bodies)
            .field("expected", &self.expected)
            .finish()
    }
}

const SSE_TEXT: &[u8] = b"event: message\r\ndata: {\"delta\": \"Hel\"}\r\n\r\n: keep-alive\n\n\
data: {\"delta\":\"lo\"}\n\ndata: [DONE]\n\n";

/// The canonical re-encoding of [`EventStreamFramingBodyV1::ConverseStream`].
const CONVERSE_STREAM_SSE: &str = concat!(
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

/// The canonical re-encoding of [`EventStreamFramingBodyV1::ExceptionAndError`].
const EXCEPTION_AND_ERROR_SSE: &str = concat!(
    "event: messageStart\n",
    "data: {\"role\":\"assistant\"}\n\n",
    "event: exception:throttlingException\n",
    "data: {\"message\":\"Rate exceeded\"}\n\n",
    "event: error:InternalFailure\n",
    "data: {\"message\":\"bad \\\"thing\\\"\"}\n\n",
);

/// The canonical re-encoding of the one valid message every fault body starts with.
const FIRST_MESSAGE_SSE: &str = "event: messageStart\ndata: {\"role\":\"assistant\"}\n\n";

const fn streaming(
    framing: StreamFramingV1,
    split: EventStreamSplitV1,
) -> EventStreamFramingPathV1 {
    EventStreamFramingPathV1::Streaming { framing, split }
}

const AWS_WHOLE: EventStreamFramingPathV1 =
    streaming(StreamFramingV1::AwsEventstream, EventStreamSplitV1::Whole);

const fn completes(delivered: EventStreamDeliveredV1) -> EventStreamFramingExpectedV1 {
    EventStreamFramingExpectedV1 { delivered, completes: true }
}

const fn fails_after(text: &'static str) -> EventStreamFramingExpectedV1 {
    EventStreamFramingExpectedV1 { delivered: EventStreamDeliveredV1::Text(text), completes: false }
}

const FIXTURES: &[EventStreamFramingFixtureV1] = &[
    EventStreamFramingFixtureV1 {
        case_id: EventStreamFramingCaseIdV1::BytesFramingPassesBytesUnchanged,
        path: streaming(StreamFramingV1::Bytes, EventStreamSplitV1::EveryByteBoundary),
        bodies: &[EventStreamFramingBodyV1::SseText],
        expected: completes(EventStreamDeliveredV1::BodyUnchanged),
    },
    EventStreamFramingFixtureV1 {
        case_id: EventStreamFramingCaseIdV1::EventFramesReencodeCanonically,
        path: AWS_WHOLE,
        bodies: &[EventStreamFramingBodyV1::ConverseStream],
        expected: completes(EventStreamDeliveredV1::Text(CONVERSE_STREAM_SSE)),
    },
    EventStreamFramingFixtureV1 {
        case_id: EventStreamFramingCaseIdV1::ExceptionAndErrorFramesPassThrough,
        path: AWS_WHOLE,
        bodies: &[EventStreamFramingBodyV1::ExceptionAndError],
        expected: completes(EventStreamDeliveredV1::Text(EXCEPTION_AND_ERROR_SSE)),
    },
    EventStreamFramingFixtureV1 {
        case_id: EventStreamFramingCaseIdV1::SplitAtEveryByteBoundaryIsIdentical,
        path: streaming(StreamFramingV1::AwsEventstream, EventStreamSplitV1::EveryByteBoundary),
        bodies: &[EventStreamFramingBodyV1::ConverseStream],
        expected: completes(EventStreamDeliveredV1::Text(CONVERSE_STREAM_SSE)),
    },
    EventStreamFramingFixtureV1 {
        case_id: EventStreamFramingCaseIdV1::ChecksumMismatchFailsAfterEarlierMessages,
        path: AWS_WHOLE,
        bodies: &[EventStreamFramingBodyV1::ChecksumMismatchMidStream],
        expected: fails_after(FIRST_MESSAGE_SSE),
    },
    EventStreamFramingFixtureV1 {
        case_id: EventStreamFramingCaseIdV1::TruncatedTailFailsAtEndOfInput,
        path: AWS_WHOLE,
        bodies: &[EventStreamFramingBodyV1::TruncatedTail],
        expected: fails_after(FIRST_MESSAGE_SSE),
    },
    EventStreamFramingFixtureV1 {
        case_id: EventStreamFramingCaseIdV1::NonJsonPayloadFails,
        path: AWS_WHOLE,
        bodies: &[EventStreamFramingBodyV1::NonJsonPayload],
        expected: fails_after(FIRST_MESSAGE_SSE),
    },
    EventStreamFramingFixtureV1 {
        case_id: EventStreamFramingCaseIdV1::BufferedPathConcatenatesTheReencoding,
        path: EventStreamFramingPathV1::Buffered,
        bodies: &[EventStreamFramingBodyV1::ConverseStream],
        expected: completes(EventStreamDeliveredV1::Text(CONVERSE_STREAM_SSE)),
    },
    EventStreamFramingFixtureV1 {
        case_id: EventStreamFramingCaseIdV1::BufferedPathFailsOnAnyFramingError,
        path: EventStreamFramingPathV1::Buffered,
        bodies: &[
            EventStreamFramingBodyV1::ChecksumMismatchMidStream,
            EventStreamFramingBodyV1::TruncatedTail,
            EventStreamFramingBodyV1::NonJsonPayload,
        ],
        expected: fails_after(""),
    },
];

/// Returns the immutable canonical eventstream-framing case table.
#[must_use]
pub const fn eventstream_framing_fixtures_v1() -> &'static [EventStreamFramingFixtureV1] {
    FIXTURES
}
