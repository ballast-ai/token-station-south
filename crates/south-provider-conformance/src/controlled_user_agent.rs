//! Canonical fixtures for the controlled user-agent conformance suite.

use std::fmt;

use crate::{
    BOUND_SLOT, ENDPOINT, ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderCallInputV1,
    ProviderCallRawResponseV1, input,
    stream::{ProviderStreamRawHeadV1, ProviderStreamRawStreamV1, ProviderStreamTerminalV1},
};

/// The controlled user-agent conformance suite version.
pub const CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The stable identifier for controlled user-agent conformance version one.
pub const CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_ID: &str = "south.controlled-user-agent.v1";

/// The closed set of canonical controlled user-agent cases.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ControlledUserAgentCaseIdV1 {
    /// One successful buffered exchange declaring a sanctioned user-agent.
    BufferedUserAgentSuccess,
    /// One successful streaming exchange declaring a sanctioned user-agent.
    StreamingUserAgentSuccess,
    /// A declared value violating the frozen grammar, refused before any boundary.
    InvalidUserAgentValueRejected,
    /// A request declaring no user-agent at all, reaching the transport and expecting `false`.
    ///
    /// The controlled-query suite had to add this row after a real host adapter passed the whole
    /// suite with a probe that hardcoded `true` and never read the prepared request — the only
    /// expected `false` belonged to a case whose probe was never invoked. This suite inherits the
    /// row from day one: it is the only case that both reaches the transport and expects `false`,
    /// so a probe must actually measure to pass it.
    UserAgentFreeRequestReachesTheWire,
    /// A plain `user-agent` header in the ordinary channel, refused by header validation.
    ///
    /// The sanctioned declaration is an opt-in, not a relaxation: `user-agent` stays on the
    /// reserved-header list, and this row proves the assembled path refuses it with zero resolver
    /// and transport calls. Without it, a host adapter that stopped routing its companion headers
    /// through `SafeHeaders` could smuggle the name outside the sanctioned channel and no case
    /// would notice.
    ReservedHeaderDeclarationStillRejected,
}

fixed_debug!(ControlledUserAgentCaseIdV1 {
    BufferedUserAgentSuccess => "BufferedUserAgentSuccess",
    StreamingUserAgentSuccess => "StreamingUserAgentSuccess",
    InvalidUserAgentValueRejected => "InvalidUserAgentValueRejected",
    UserAgentFreeRequestReachesTheWire => "UserAgentFreeRequestReachesTheWire",
    ReservedHeaderDeclarationStillRejected => "ReservedHeaderDeclarationStillRejected",
});

/// A raw upstream exchange or fake-transport behavior for a canonical controlled user-agent case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ControlledUserAgentUpstreamV1 {
    /// Complete one buffered exchange with this raw response.
    Response(ProviderCallRawResponseV1),
    /// Open a 2xx stream and script its chunks and terminal.
    Stream(ProviderStreamRawStreamV1),
    /// The transport boundary must not be reached.
    NotReached,
}

impl fmt::Debug for ControlledUserAgentUpstreamV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(raw) => formatter.debug_tuple("Response").field(raw).finish(),
            Self::Stream(raw) => formatter.debug_tuple("Stream").field(raw).finish(),
            Self::NotReached => formatter.write_str("NotReached"),
        }
    }
}

/// The exact expected terminal shape of one canonical controlled user-agent case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ControlledUserAgentExpectedOutcomeV1 {
    /// A bounded buffered response matched field by field.
    Response {
        /// Expected status.
        status: u16,
        /// Expected body.
        body: &'static str,
        /// Expected `content-type`, preserving presence.
        content_type: Option<&'static str>,
        /// Expected `retry-after`, preserving presence.
        retry_after: Option<&'static str>,
    },
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
    /// A known stable failure.
    Failure {
        /// Expected closed failure code.
        code: ProviderCallFailureCodeV1,
    },
}

impl fmt::Debug for ControlledUserAgentExpectedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response { status, body, content_type, retry_after } => formatter
                .debug_struct("Response")
                .field("status", status)
                .field("body_byte_count", &body.len())
                .field("has_content_type", &content_type.is_some())
                .field("has_retry_after", &retry_after.is_some())
                .finish(),
            Self::Opened { status, content_type, retry_after, chunks } => formatter
                .debug_struct("Opened")
                .field("status", status)
                .field("has_content_type", &content_type.is_some())
                .field("has_retry_after", &retry_after.is_some())
                .field("chunk_count", &chunks.len())
                .finish(),
            Self::Failure { code } => {
                formatter.debug_struct("Failure").field("code", code).finish()
            }
        }
    }
}

/// Expected resolver, transport, and wire user-agent boundary evidence.
///
/// The wire boolean is adapter-reported like every other evidence field: a passing report alone is
/// insufficient, and the host-adoption review must confirm it is measured at the real transport
/// boundary.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ControlledUserAgentExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    wire_user_agent_exact: bool,
}

impl ControlledUserAgentExpectedEvidenceV1 {
    /// Returns the expected resolver call category.
    #[must_use]
    pub const fn resolver_calls(&self) -> ProviderCallCountV1 {
        self.resolver_calls
    }

    /// Returns the expected transport call category.
    #[must_use]
    pub const fn transport_calls(&self) -> ProviderCallCountV1 {
        self.transport_calls
    }

    /// Returns whether the request reaching the transport boundary must carry a user-agent byte
    /// for byte equal to the declared value.
    ///
    /// A *presence* claim with the same polarity as the controlled-query suite's
    /// `wire_query_exact`: it can only become true by observing a wire carrying a declared
    /// user-agent, so a case whose transport is never reached expects `false`, and so does a case
    /// that reaches the transport having declared nothing. The two `false` reasons are not
    /// redundant — the rejection rows prove the wire is never reached, and
    /// [`ControlledUserAgentCaseIdV1::UserAgentFreeRequestReachesTheWire`] proves the probe is
    /// actually measuring, because it is the only case where the transport runs and the answer is
    /// still `false`.
    #[must_use]
    pub const fn wire_user_agent_exact(&self) -> bool {
        self.wire_user_agent_exact
    }
}

impl fmt::Debug for ControlledUserAgentExpectedEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlledUserAgentExpectedEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field("wire_user_agent_exact", &self.wire_user_agent_exact)
            .finish()
    }
}

/// The expected outcome and boundary evidence for one controlled user-agent fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ControlledUserAgentExpectedV1 {
    outcome: ControlledUserAgentExpectedOutcomeV1,
    evidence: ControlledUserAgentExpectedEvidenceV1,
}

impl ControlledUserAgentExpectedV1 {
    /// Returns the expected terminal shape.
    #[must_use]
    pub const fn outcome(&self) -> &ControlledUserAgentExpectedOutcomeV1 {
        &self.outcome
    }

    /// Returns the expected boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ControlledUserAgentExpectedEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ControlledUserAgentExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlledUserAgentExpectedV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// One immutable canonical controlled user-agent fixture.
///
/// The declared user-agent is retained *raw* rather than as a constructed
/// `ControlledUserAgentV1`: the negative case exists precisely to exercise the construction
/// failure, so the fixture must be able to carry a value the contract rejects, and the
/// declaration-free case must be able to carry no value at all.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ControlledUserAgentFixtureV1 {
    case_id: ControlledUserAgentCaseIdV1,
    input: ProviderCallInputV1,
    declared_user_agent: Option<&'static str>,
    upstream: ControlledUserAgentUpstreamV1,
    expected: ControlledUserAgentExpectedV1,
}

impl ControlledUserAgentFixtureV1 {
    /// Returns the stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> ControlledUserAgentCaseIdV1 {
        self.case_id
    }

    /// Returns the immutable raw input shared with the provider-call suite shape.
    #[must_use]
    pub const fn input(&self) -> &ProviderCallInputV1 {
        &self.input
    }

    /// Returns the raw user-agent value the request declares, when it declares one.
    ///
    /// `None` means the request declares no user-agent, which is a valid fixture shape: the
    /// declaration-free case must reach the transport rather than fail preparation, so an absent
    /// declaration must never be turned into a construction attempt.
    #[must_use]
    pub const fn declared_user_agent(&self) -> Option<&'static str> {
        self.declared_user_agent
    }

    /// Returns the canonical fake-upstream behavior.
    #[must_use]
    pub const fn upstream(&self) -> &ControlledUserAgentUpstreamV1 {
        &self.upstream
    }

    /// Returns the exact expected outcome and evidence.
    #[must_use]
    pub const fn expected(&self) -> &ControlledUserAgentExpectedV1 {
        &self.expected
    }
}

impl fmt::Debug for ControlledUserAgentFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlledUserAgentFixtureV1")
            .field("case_id", &self.case_id)
            .field("declares_user_agent", &self.declared_user_agent.is_some())
            .field("input", &self.input)
            .field("upstream", &self.upstream)
            .field("expected", &self.expected)
            .finish()
    }
}

const CONTROLLED_USER_AGENT_PATH: &str = "path-debug-sentinel";
const CONTROLLED_USER_AGENT_BOUND_SLOT: &str = "bound-slot-debug-sentinel";
const CONTROLLED_USER_AGENT_RESPONSE_BODY: &str = r#"{"value":"response-body-debug-sentinel"}"#;
const CONTROLLED_USER_AGENT_CONTENT_TYPE: &str = "content-type-debug-sentinel";
const CONTROLLED_USER_AGENT_RETRY_AFTER: &str = "retry-after-debug-sentinel";
const CONTROLLED_USER_AGENT_CHUNK_ONE: &[u8] = b"controlled-user-agent-chunk-one-debug-sentinel";
const CONTROLLED_USER_AGENT_CHUNK_TWO: &[u8] = b"controlled-user-agent-chunk-two-debug-sentinel";
const CONTROLLED_USER_AGENT_CHUNKS: &[&[u8]] =
    &[CONTROLLED_USER_AGENT_CHUNK_ONE, CONTROLLED_USER_AGENT_CHUNK_TWO];

/// A product-token value with an interior space and parentheses, exercising the full accepted
/// class the audited host inventory needs.
const CONTROLLED_USER_AGENT_VALUE: &str = "user-agent-value-debug-sentinel/1.0 (conformance)";
/// A value carrying a leading space, which the grammar rejects at both edges. Deliberately a plain
/// grammar violation rather than an injection payload: the suite proves the contract refuses
/// before the wire, not that a particular exploit string is neutralized.
const CONTROLLED_USER_AGENT_INVALID_VALUE: &str = " user-agent-invalid-debug-sentinel";
/// The value smuggled through the ordinary header channel by the reserved-header case.
const CONTROLLED_USER_AGENT_PLAIN_CHANNEL_VALUE: &str = "user-agent-plain-debug-sentinel/1.0";

/// The reserved-header case's ordinary headers: the canonical pair plus a plain `user-agent`.
///
/// Header validation must refuse the whole set, which is what keeps the sanctioned typed slot the
/// only source of the header on the wire.
const RESERVED_HEADER_CASE_HEADERS: &[(&str, &str)] = &[
    ("header-name-debug-sentinel", "header-value-debug-sentinel"),
    ("user-agent", CONTROLLED_USER_AGENT_PLAIN_CHANNEL_VALUE),
];

const fn input_with_plain_user_agent_header() -> ProviderCallInputV1 {
    ProviderCallInputV1 {
        endpoint: ENDPOINT,
        bound_credential_slot: BOUND_SLOT,
        requested_credential_slot: CONTROLLED_USER_AGENT_BOUND_SLOT,
        relative_path: CONTROLLED_USER_AGENT_PATH,
        json_body: crate::REQUEST_BODY,
        headers: RESERVED_HEADER_CASE_HEADERS,
    }
}

const fn user_agent_evidence(
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    wire_user_agent_exact: bool,
) -> ControlledUserAgentExpectedEvidenceV1 {
    ControlledUserAgentExpectedEvidenceV1 { resolver_calls, transport_calls, wire_user_agent_exact }
}

const CONTROLLED_USER_AGENT_FIXTURES: &[ControlledUserAgentFixtureV1] = &[
    ControlledUserAgentFixtureV1 {
        case_id: ControlledUserAgentCaseIdV1::BufferedUserAgentSuccess,
        input: input(CONTROLLED_USER_AGENT_PATH, CONTROLLED_USER_AGENT_BOUND_SLOT),
        declared_user_agent: Some(CONTROLLED_USER_AGENT_VALUE),
        upstream: ControlledUserAgentUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 201,
            body: CONTROLLED_USER_AGENT_RESPONSE_BODY,
            content_type: Some(CONTROLLED_USER_AGENT_CONTENT_TYPE),
            retry_after: Some(CONTROLLED_USER_AGENT_RETRY_AFTER),
        }),
        expected: ControlledUserAgentExpectedV1 {
            outcome: ControlledUserAgentExpectedOutcomeV1::Response {
                status: 201,
                body: CONTROLLED_USER_AGENT_RESPONSE_BODY,
                content_type: Some(CONTROLLED_USER_AGENT_CONTENT_TYPE),
                retry_after: Some(CONTROLLED_USER_AGENT_RETRY_AFTER),
            },
            evidence: user_agent_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        },
    },
    ControlledUserAgentFixtureV1 {
        case_id: ControlledUserAgentCaseIdV1::StreamingUserAgentSuccess,
        input: input(CONTROLLED_USER_AGENT_PATH, CONTROLLED_USER_AGENT_BOUND_SLOT),
        declared_user_agent: Some(CONTROLLED_USER_AGENT_VALUE),
        upstream: ControlledUserAgentUpstreamV1::Stream(ProviderStreamRawStreamV1::assemble(
            ProviderStreamRawHeadV1::assemble(200, Some(CONTROLLED_USER_AGENT_CONTENT_TYPE), None),
            CONTROLLED_USER_AGENT_CHUNKS,
            ProviderStreamTerminalV1::CleanEof,
        )),
        expected: ControlledUserAgentExpectedV1 {
            outcome: ControlledUserAgentExpectedOutcomeV1::Opened {
                status: 200,
                content_type: Some(CONTROLLED_USER_AGENT_CONTENT_TYPE),
                retry_after: None,
                chunks: CONTROLLED_USER_AGENT_CHUNKS,
            },
            evidence: user_agent_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        },
    },
    ControlledUserAgentFixtureV1 {
        case_id: ControlledUserAgentCaseIdV1::InvalidUserAgentValueRejected,
        input: input(CONTROLLED_USER_AGENT_PATH, CONTROLLED_USER_AGENT_BOUND_SLOT),
        declared_user_agent: Some(CONTROLLED_USER_AGENT_INVALID_VALUE),
        upstream: ControlledUserAgentUpstreamV1::NotReached,
        expected: ControlledUserAgentExpectedV1 {
            // User-agent contract errors are preparation-time failures with zero resolver and
            // transport calls, and the frozen nineteen-code set has exactly one preparation-time
            // provider-declaration code. Both sanctioned channels fold their declaration errors
            // into it — the query slice established the fold — rather than widening the contract.
            // The finer `ContractErrorV1` reason stays available to hosts in their own logs.
            outcome: ControlledUserAgentExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::InvalidRelativePath,
            },
            evidence: user_agent_evidence(
                ProviderCallCountV1::Zero,
                ProviderCallCountV1::Zero,
                false,
            ),
        },
    },
    ControlledUserAgentFixtureV1 {
        case_id: ControlledUserAgentCaseIdV1::UserAgentFreeRequestReachesTheWire,
        input: input(CONTROLLED_USER_AGENT_PATH, CONTROLLED_USER_AGENT_BOUND_SLOT),
        declared_user_agent: None,
        upstream: ControlledUserAgentUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 200,
            body: CONTROLLED_USER_AGENT_RESPONSE_BODY,
            content_type: None,
            retry_after: None,
        }),
        expected: ControlledUserAgentExpectedV1 {
            outcome: ControlledUserAgentExpectedOutcomeV1::Response {
                status: 200,
                body: CONTROLLED_USER_AGENT_RESPONSE_BODY,
                content_type: None,
                retry_after: None,
            },
            // The load-bearing row of the table, inherited from the controlled-query suite's
            // measured hole: the transport *is* reached, and the answer is still `false`. Every
            // other `true` can be satisfied by a probe that ignores the prepared request and
            // hardcodes `true`; this row cannot.
            evidence: user_agent_evidence(
                ProviderCallCountV1::One,
                ProviderCallCountV1::One,
                false,
            ),
        },
    },
    ControlledUserAgentFixtureV1 {
        case_id: ControlledUserAgentCaseIdV1::ReservedHeaderDeclarationStillRejected,
        input: input_with_plain_user_agent_header(),
        declared_user_agent: None,
        upstream: ControlledUserAgentUpstreamV1::NotReached,
        expected: ControlledUserAgentExpectedV1 {
            // Header-policy failures have no code of their own in the frozen set (a decision that
            // predates this suite), so they surface through the context-free request fallback.
            // The zero-call evidence is what separates this deterministic refusal from a broken
            // transport reporting the same code.
            outcome: ControlledUserAgentExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::RequestFailed,
            },
            evidence: user_agent_evidence(
                ProviderCallCountV1::Zero,
                ProviderCallCountV1::Zero,
                false,
            ),
        },
    },
];

/// Returns the immutable canonical controlled user-agent fixture table.
#[must_use]
pub const fn controlled_user_agent_fixtures_v1() -> &'static [ControlledUserAgentFixtureV1] {
    CONTROLLED_USER_AGENT_FIXTURES
}
