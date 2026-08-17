//! Canonical fixtures for the byte-level provider streaming conformance suite.

use std::{fmt, time::Duration};

use south_contracts::{MAX_STREAM_ERROR_BODY_BYTES, StreamReadErrorV1, TransportErrorV1};

use crate::{ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderCallInputV1, input};

/// The provider-stream conformance suite version.
pub const PROVIDER_STREAM_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The stable identifier for provider-stream conformance version one.
pub const PROVIDER_STREAM_CONFORMANCE_SUITE_ID: &str = "south.provider-stream.v1";

/// The absolute-deadline offset used by the mid-stream deadline fixture.
pub const PROVIDER_STREAM_CONFORMANCE_DEADLINE_OFFSET_V1: Duration = Duration::from_secs(1);

/// The virtual idle bound at which the stalled-upstream fixture must fail.
pub const PROVIDER_STREAM_CONFORMANCE_IDLE_TIMEOUT_V1: Duration = Duration::from_secs(2);

/// The closed set of canonical provider-stream cases.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderStreamCaseIdV1 {
    /// Headers-ready, then byte-identical chunks, then a clean EOF.
    StreamSuccess,
    /// A non-2xx status short-circuits into a bounded error body without a stream object.
    RejectedUpstreamStatus,
    /// A redirect refused before any body pull.
    RedirectDenied,
    /// The cancellation token fires between pulls and drops the in-flight future.
    CancelBetweenChunks,
    /// A silent upstream after the first chunk fails at the virtual idle bound.
    IdleTimeoutMidStream,
    /// The caller's absolute deadline fires while a pull is pending.
    DeadlineMidStream,
    /// The transport breaks after the first chunk; later pulls return `None`.
    UpstreamBreakMidStream,
    /// Raw input whose relative path must fail contract parsing before any boundary call.
    InvalidRelativePath,
    /// A rejected-path error body above the bound is truncated, not failed.
    ErrorBodyTooLargeIsTruncated,
}

fixed_debug!(ProviderStreamCaseIdV1 {
    StreamSuccess => "StreamSuccess",
    RejectedUpstreamStatus => "RejectedUpstreamStatus",
    RedirectDenied => "RedirectDenied",
    CancelBetweenChunks => "CancelBetweenChunks",
    IdleTimeoutMidStream => "IdleTimeoutMidStream",
    DeadlineMidStream => "DeadlineMidStream",
    UpstreamBreakMidStream => "UpstreamBreakMidStream",
    InvalidRelativePath => "InvalidRelativePath",
    ErrorBodyTooLargeIsTruncated => "ErrorBodyTooLargeIsTruncated",
});

/// How the assembled executor must drive one canonical streaming case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderStreamControlV1 {
    /// Let the exchange run to its scripted terminal without external interruption.
    Complete,
    /// Cancel the caller token after proving a chunk pull is pending.
    CancelWhileChunkPending,
    /// Let the caller advance the virtual clock to the idle bound while a pull is pending.
    AdvanceIdleWhileChunkPending,
    /// Let the caller advance the injected absolute deadline while a pull is pending.
    ExpireWhileChunkPending,
}

fixed_debug!(ProviderStreamControlV1 {
    Complete => "Complete",
    CancelWhileChunkPending => "CancelWhileChunkPending",
    AdvanceIdleWhileChunkPending => "AdvanceIdleWhileChunkPending",
    ExpireWhileChunkPending => "ExpireWhileChunkPending",
});

/// Borrowed, allocation-free raw head values for a canonical streaming exchange.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderStreamRawHeadV1 {
    status: u16,
    content_type: Option<&'static str>,
    retry_after: Option<&'static str>,
}

impl ProviderStreamRawHeadV1 {
    /// Assembles one canonical raw head for a fixture table in this crate.
    pub(crate) const fn assemble(
        status: u16,
        content_type: Option<&'static str>,
        retry_after: Option<&'static str>,
    ) -> Self {
        Self { status, content_type, retry_after }
    }

    /// Returns the raw HTTP status.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Returns the optional raw `content-type` value.
    #[must_use]
    pub const fn content_type(&self) -> Option<&'static str> {
        self.content_type
    }

    /// Returns the optional raw `retry-after` value.
    #[must_use]
    pub const fn retry_after(&self) -> Option<&'static str> {
        self.retry_after
    }
}

impl fmt::Debug for ProviderStreamRawHeadV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderStreamRawHeadV1")
            .field("status", &self.status)
            .field("has_content_type", &self.content_type.is_some())
            .field("has_retry_after", &self.retry_after.is_some())
            .finish()
    }
}

/// The scripted behavior of a fake byte source after its listed chunks are exhausted.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderStreamTerminalV1 {
    /// Yield a clean end of stream.
    CleanEof,
    /// Yield a `STREAM_READ_FAILED` terminal error.
    BreakWithReadFailure,
    /// Stall silently; the fake transport's idle guard fires at the virtual idle bound.
    IdleStall,
    /// Remain pending until an external cancellation or deadline drops the pull.
    PendingForever,
}

fixed_debug!(ProviderStreamTerminalV1 {
    CleanEof => "CleanEof",
    BreakWithReadFailure => "BreakWithReadFailure",
    IdleStall => "IdleStall",
    PendingForever => "PendingForever",
});

/// A borrowed raw 2xx streaming exchange fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderStreamRawStreamV1 {
    head: ProviderStreamRawHeadV1,
    chunks: &'static [&'static [u8]],
    terminal: ProviderStreamTerminalV1,
}

impl ProviderStreamRawStreamV1 {
    /// Assembles one canonical raw stream for a fixture table in this crate.
    pub(crate) const fn assemble(
        head: ProviderStreamRawHeadV1,
        chunks: &'static [&'static [u8]],
        terminal: ProviderStreamTerminalV1,
    ) -> Self {
        Self { head, chunks, terminal }
    }

    /// Returns the raw headers-ready values.
    #[must_use]
    pub const fn head(&self) -> &ProviderStreamRawHeadV1 {
        &self.head
    }

    /// Returns the raw chunk byte slices in delivery order.
    #[must_use]
    pub const fn chunks(&self) -> &'static [&'static [u8]] {
        self.chunks
    }

    /// Returns the scripted terminal behavior after the listed chunks.
    #[must_use]
    pub const fn terminal(&self) -> ProviderStreamTerminalV1 {
        self.terminal
    }
}

impl fmt::Debug for ProviderStreamRawStreamV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderStreamRawStreamV1")
            .field("head", &self.head)
            .field("chunk_count", &self.chunks.len())
            .field("terminal", &self.terminal)
            .finish()
    }
}

/// A borrowed raw non-2xx rejection fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderStreamRawRejectionV1 {
    head: ProviderStreamRawHeadV1,
    body: &'static [u8],
}

impl ProviderStreamRawRejectionV1 {
    /// Returns the raw headers-ready values.
    #[must_use]
    pub const fn head(&self) -> &ProviderStreamRawHeadV1 {
        &self.head
    }

    /// Returns the raw upstream error body before any truncation.
    #[must_use]
    pub const fn body(&self) -> &'static [u8] {
        self.body
    }
}

impl fmt::Debug for ProviderStreamRawRejectionV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderStreamRawRejectionV1")
            .field("head", &self.head)
            .field("body_byte_count", &self.body.len())
            .finish()
    }
}

/// A raw streaming exchange or fake-transport behavior for a canonical case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderStreamUpstreamV1 {
    /// Open a 2xx stream and script its chunks and terminal.
    Stream(ProviderStreamRawStreamV1),
    /// Collapse the exchange into a rejection with this raw error body.
    Rejected(ProviderStreamRawRejectionV1),
    /// Fail the open with this closed transport failure.
    TransportFailure(TransportErrorV1),
    /// The transport boundary must not be reached.
    NotReached,
}

impl fmt::Debug for ProviderStreamUpstreamV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stream(raw) => formatter.debug_tuple("Stream").field(raw).finish(),
            Self::Rejected(raw) => formatter.debug_tuple("Rejected").field(raw).finish(),
            Self::TransportFailure(error) => formatter
                .debug_tuple("TransportFailure")
                .field(&StreamTransportCodeDebug(error.code()))
                .finish(),
            Self::NotReached => formatter.write_str("NotReached"),
        }
    }
}

/// The exact expected terminal shape of one canonical streaming case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderStreamExpectedOutcomeV1 {
    /// A live 2xx stream whose head and chunk bytes matched exactly.
    Opened {
        /// Expected status.
        status: u16,
        /// Expected `content-type`, preserving presence.
        content_type: Option<&'static str>,
        /// Expected `retry-after`, preserving presence.
        retry_after: Option<&'static str>,
        /// Expected chunk bytes in delivery order.
        chunks: &'static [&'static [u8]],
    },
    /// A rejected exchange whose head and bounded error body matched exactly.
    Rejected {
        /// Expected status.
        status: u16,
        /// Expected `content-type`, preserving presence.
        content_type: Option<&'static str>,
        /// Expected `retry-after`, preserving presence.
        retry_after: Option<&'static str>,
        /// Expected bounded, possibly truncated error body.
        body: &'static [u8],
    },
    /// A known stable open-phase failure.
    Failure {
        /// Expected closed failure code.
        code: ProviderCallFailureCodeV1,
    },
}

impl fmt::Debug for ProviderStreamExpectedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Opened { status, content_type, retry_after, chunks } => formatter
                .debug_struct("Opened")
                .field("status", status)
                .field("has_content_type", &content_type.is_some())
                .field("has_retry_after", &retry_after.is_some())
                .field("chunk_count", &chunks.len())
                .finish(),
            Self::Rejected { status, content_type, retry_after, body } => formatter
                .debug_struct("Rejected")
                .field("status", status)
                .field("has_content_type", &content_type.is_some())
                .field("has_retry_after", &retry_after.is_some())
                .field("body_byte_count", &body.len())
                .finish(),
            Self::Failure { code } => {
                formatter.debug_struct("Failure").field("code", code).finish()
            }
        }
    }
}

/// Expected resolver, transport, and stream-phase boundary evidence.
///
/// The two pending-drop booleans keep their buffered meaning, with one extension: the transport
/// flag covers every pending transport-owned future, so a chunk pull dropped mid-flight by
/// cancellation or the caller deadline must set it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderStreamExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    resolver_future_dropped_while_pending: bool,
    transport_future_dropped_while_pending: bool,
    chunks_pulled: usize,
    poststream_error_code: Option<StreamReadErrorV1>,
}

impl ProviderStreamExpectedEvidenceV1 {
    /// Returns the expected resolver call category.
    #[must_use]
    pub const fn resolver_calls(&self) -> ProviderCallCountV1 {
        self.resolver_calls
    }

    /// Returns the expected transport open call category.
    #[must_use]
    pub const fn transport_calls(&self) -> ProviderCallCountV1 {
        self.transport_calls
    }

    /// Returns whether a pending resolver future must be dropped.
    #[must_use]
    pub const fn resolver_future_dropped_while_pending(&self) -> bool {
        self.resolver_future_dropped_while_pending
    }

    /// Returns whether a pending transport-owned future must be dropped.
    #[must_use]
    pub const fn transport_future_dropped_while_pending(&self) -> bool {
        self.transport_future_dropped_while_pending
    }

    /// Returns the exact number of successful chunk pulls.
    #[must_use]
    pub const fn chunks_pulled(&self) -> usize {
        self.chunks_pulled
    }

    /// Returns the expected terminal stream error, or `None` for a clean EOF or no stream.
    #[must_use]
    pub const fn poststream_error_code(&self) -> Option<StreamReadErrorV1> {
        self.poststream_error_code
    }
}

impl fmt::Debug for ProviderStreamExpectedEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderStreamExpectedEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field(
                "resolver_future_dropped_while_pending",
                &self.resolver_future_dropped_while_pending,
            )
            .field(
                "transport_future_dropped_while_pending",
                &self.transport_future_dropped_while_pending,
            )
            .field("chunks_pulled", &self.chunks_pulled)
            .field("poststream_error_code", &self.poststream_error_code)
            .finish()
    }
}

/// The expected outcome and boundary evidence for one streaming fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderStreamExpectedV1 {
    outcome: ProviderStreamExpectedOutcomeV1,
    evidence: ProviderStreamExpectedEvidenceV1,
}

impl ProviderStreamExpectedV1 {
    /// Returns the expected terminal shape.
    #[must_use]
    pub const fn outcome(&self) -> &ProviderStreamExpectedOutcomeV1 {
        &self.outcome
    }

    /// Returns the expected boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderStreamExpectedEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ProviderStreamExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderStreamExpectedV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// One immutable canonical provider-stream fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderStreamFixtureV1 {
    case_id: ProviderStreamCaseIdV1,
    input: ProviderCallInputV1,
    control: ProviderStreamControlV1,
    upstream: ProviderStreamUpstreamV1,
    expected: ProviderStreamExpectedV1,
}

impl ProviderStreamFixtureV1 {
    /// Returns the stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> ProviderStreamCaseIdV1 {
        self.case_id
    }

    /// Returns the immutable raw input shared with the provider-call suite shape.
    #[must_use]
    pub const fn input(&self) -> &ProviderCallInputV1 {
        &self.input
    }

    /// Returns the canonical control behavior.
    #[must_use]
    pub const fn control(&self) -> ProviderStreamControlV1 {
        self.control
    }

    /// Returns the canonical fake-upstream behavior.
    #[must_use]
    pub const fn upstream(&self) -> &ProviderStreamUpstreamV1 {
        &self.upstream
    }

    /// Returns the exact expected outcome and evidence.
    #[must_use]
    pub const fn expected(&self) -> &ProviderStreamExpectedV1 {
        &self.expected
    }
}

impl fmt::Debug for ProviderStreamFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderStreamFixtureV1")
            .field("case_id", &self.case_id)
            .field("control", &self.control)
            .field("input", &self.input)
            .field("upstream", &self.upstream)
            .field("expected", &self.expected)
            .finish()
    }
}

const STREAM_PATH: &str = "path-debug-sentinel";
const STREAM_INVALID_PATH: &str = "../path-debug-sentinel";
const STREAM_SLOT: &str = "bound-slot-debug-sentinel";
const STREAM_CONTENT_TYPE: &str = "content-type-debug-sentinel";
const STREAM_RETRY_AFTER: &str = "retry-after-debug-sentinel";
const STREAM_CHUNK_ONE: &[u8] = b"stream-chunk-one-debug-sentinel";
const STREAM_CHUNK_TWO: &[u8] = b"stream-chunk-two-debug-sentinel";
const STREAM_CHUNK_THREE: &[u8] = b"stream-chunk-three-debug-sentinel";
const STREAM_SUCCESS_CHUNKS: &[&[u8]] = &[STREAM_CHUNK_ONE, STREAM_CHUNK_TWO, STREAM_CHUNK_THREE];
const STREAM_SINGLE_CHUNK: &[&[u8]] = &[STREAM_CHUNK_ONE];
const STREAM_REJECTED_BODY: &[u8] = b"stream-rejected-body-debug-sentinel";
// The truncation fixture needs one real over-limit body (64 KiB + 1). It must stay `const`, not
// `static`, because the `const` fixture table below cannot refer to a `static` item.
#[allow(clippy::large_const_arrays)]
const OVERSIZED_ERROR_BODY_ARRAY: [u8; MAX_STREAM_ERROR_BODY_BYTES + 1] =
    [b'x'; MAX_STREAM_ERROR_BODY_BYTES + 1];
const OVERSIZED_ERROR_BODY: &[u8] = &OVERSIZED_ERROR_BODY_ARRAY;
const TRUNCATED_ERROR_BODY: &[u8] = OVERSIZED_ERROR_BODY.split_at(MAX_STREAM_ERROR_BODY_BYTES).0;

const STREAM_SUCCESS_HEAD: ProviderStreamRawHeadV1 = ProviderStreamRawHeadV1 {
    status: 200,
    content_type: Some(STREAM_CONTENT_TYPE),
    retry_after: None,
};

const fn streamed(
    chunks: &'static [&'static [u8]],
    terminal: ProviderStreamTerminalV1,
) -> ProviderStreamUpstreamV1 {
    ProviderStreamUpstreamV1::Stream(ProviderStreamRawStreamV1 {
        head: STREAM_SUCCESS_HEAD,
        chunks,
        terminal,
    })
}

const fn opened(
    chunks: &'static [&'static [u8]],
    chunks_pulled: usize,
    transport_pending_drop: bool,
    poststream_error_code: Option<StreamReadErrorV1>,
) -> ProviderStreamExpectedV1 {
    ProviderStreamExpectedV1 {
        outcome: ProviderStreamExpectedOutcomeV1::Opened {
            status: 200,
            content_type: Some(STREAM_CONTENT_TYPE),
            retry_after: None,
            chunks,
        },
        evidence: ProviderStreamExpectedEvidenceV1 {
            resolver_calls: ProviderCallCountV1::One,
            transport_calls: ProviderCallCountV1::One,
            resolver_future_dropped_while_pending: false,
            transport_future_dropped_while_pending: transport_pending_drop,
            chunks_pulled,
            poststream_error_code,
        },
    }
}

const fn boundary_evidence(
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
) -> ProviderStreamExpectedEvidenceV1 {
    ProviderStreamExpectedEvidenceV1 {
        resolver_calls,
        transport_calls,
        resolver_future_dropped_while_pending: false,
        transport_future_dropped_while_pending: false,
        chunks_pulled: 0,
        poststream_error_code: None,
    }
}

const STREAM_FIXTURES: &[ProviderStreamFixtureV1] = &[
    ProviderStreamFixtureV1 {
        case_id: ProviderStreamCaseIdV1::StreamSuccess,
        input: input(STREAM_PATH, STREAM_SLOT),
        control: ProviderStreamControlV1::Complete,
        upstream: streamed(STREAM_SUCCESS_CHUNKS, ProviderStreamTerminalV1::CleanEof),
        expected: opened(STREAM_SUCCESS_CHUNKS, 3, false, None),
    },
    ProviderStreamFixtureV1 {
        case_id: ProviderStreamCaseIdV1::RejectedUpstreamStatus,
        input: input(STREAM_PATH, STREAM_SLOT),
        control: ProviderStreamControlV1::Complete,
        upstream: ProviderStreamUpstreamV1::Rejected(ProviderStreamRawRejectionV1 {
            head: ProviderStreamRawHeadV1 {
                status: 429,
                content_type: Some(STREAM_CONTENT_TYPE),
                retry_after: Some(STREAM_RETRY_AFTER),
            },
            body: STREAM_REJECTED_BODY,
        }),
        expected: ProviderStreamExpectedV1 {
            outcome: ProviderStreamExpectedOutcomeV1::Rejected {
                status: 429,
                content_type: Some(STREAM_CONTENT_TYPE),
                retry_after: Some(STREAM_RETRY_AFTER),
                body: STREAM_REJECTED_BODY,
            },
            evidence: boundary_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One),
        },
    },
    ProviderStreamFixtureV1 {
        case_id: ProviderStreamCaseIdV1::RedirectDenied,
        input: input(STREAM_PATH, STREAM_SLOT),
        control: ProviderStreamControlV1::Complete,
        upstream: ProviderStreamUpstreamV1::TransportFailure(TransportErrorV1::RedirectDenied),
        expected: ProviderStreamExpectedV1 {
            outcome: ProviderStreamExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::RedirectDenied,
            },
            evidence: boundary_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One),
        },
    },
    ProviderStreamFixtureV1 {
        case_id: ProviderStreamCaseIdV1::CancelBetweenChunks,
        input: input(STREAM_PATH, STREAM_SLOT),
        control: ProviderStreamControlV1::CancelWhileChunkPending,
        upstream: streamed(STREAM_SINGLE_CHUNK, ProviderStreamTerminalV1::PendingForever),
        expected: opened(STREAM_SINGLE_CHUNK, 1, true, Some(StreamReadErrorV1::StreamCancelled)),
    },
    ProviderStreamFixtureV1 {
        case_id: ProviderStreamCaseIdV1::IdleTimeoutMidStream,
        input: input(STREAM_PATH, STREAM_SLOT),
        control: ProviderStreamControlV1::AdvanceIdleWhileChunkPending,
        upstream: streamed(STREAM_SINGLE_CHUNK, ProviderStreamTerminalV1::IdleStall),
        expected: opened(STREAM_SINGLE_CHUNK, 1, false, Some(StreamReadErrorV1::StreamIdleTimeout)),
    },
    ProviderStreamFixtureV1 {
        case_id: ProviderStreamCaseIdV1::DeadlineMidStream,
        input: input(STREAM_PATH, STREAM_SLOT),
        control: ProviderStreamControlV1::ExpireWhileChunkPending,
        upstream: streamed(STREAM_SINGLE_CHUNK, ProviderStreamTerminalV1::PendingForever),
        expected: opened(
            STREAM_SINGLE_CHUNK,
            1,
            true,
            Some(StreamReadErrorV1::StreamDeadlineExceeded),
        ),
    },
    ProviderStreamFixtureV1 {
        case_id: ProviderStreamCaseIdV1::UpstreamBreakMidStream,
        input: input(STREAM_PATH, STREAM_SLOT),
        control: ProviderStreamControlV1::Complete,
        upstream: streamed(STREAM_SINGLE_CHUNK, ProviderStreamTerminalV1::BreakWithReadFailure),
        expected: opened(STREAM_SINGLE_CHUNK, 1, false, Some(StreamReadErrorV1::StreamReadFailed)),
    },
    ProviderStreamFixtureV1 {
        case_id: ProviderStreamCaseIdV1::InvalidRelativePath,
        input: input(STREAM_INVALID_PATH, STREAM_SLOT),
        control: ProviderStreamControlV1::Complete,
        upstream: ProviderStreamUpstreamV1::NotReached,
        expected: ProviderStreamExpectedV1 {
            outcome: ProviderStreamExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::InvalidRelativePath,
            },
            evidence: boundary_evidence(ProviderCallCountV1::Zero, ProviderCallCountV1::Zero),
        },
    },
    ProviderStreamFixtureV1 {
        case_id: ProviderStreamCaseIdV1::ErrorBodyTooLargeIsTruncated,
        input: input(STREAM_PATH, STREAM_SLOT),
        control: ProviderStreamControlV1::Complete,
        upstream: ProviderStreamUpstreamV1::Rejected(ProviderStreamRawRejectionV1 {
            head: ProviderStreamRawHeadV1 { status: 502, content_type: None, retry_after: None },
            body: OVERSIZED_ERROR_BODY,
        }),
        expected: ProviderStreamExpectedV1 {
            outcome: ProviderStreamExpectedOutcomeV1::Rejected {
                status: 502,
                content_type: None,
                retry_after: None,
                body: TRUNCATED_ERROR_BODY,
            },
            evidence: boundary_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One),
        },
    },
];

/// Returns the immutable canonical provider-stream fixture table.
#[must_use]
pub const fn provider_stream_fixtures_v1() -> &'static [ProviderStreamFixtureV1] {
    STREAM_FIXTURES
}

struct StreamTransportCodeDebug(&'static str);

impl fmt::Debug for StreamTransportCodeDebug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}
