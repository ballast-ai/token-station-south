//! Canonical fixtures for the header-secret auth conformance suite.

use std::fmt;

use south_contracts::SecretHeaderV1;

use crate::{
    ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderCallInputV1, ProviderCallRawResponseV1,
    input,
    stream::{ProviderStreamRawHeadV1, ProviderStreamRawStreamV1, ProviderStreamTerminalV1},
};

/// The header-auth conformance suite version.
pub const HEADER_AUTH_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The stable identifier for header-auth conformance version one.
pub const HEADER_AUTH_CONFORMANCE_SUITE_ID: &str = "south.header-auth.v1";

/// Synthetic test-only header-secret material used by the reference executor.
pub const FAKE_HEADER_SECRET_V1: &str = "south-test-only-fake-header-secret-v1";

/// The closed set of canonical header-auth cases.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HeaderAuthCaseIdV1 {
    /// One successful buffered exchange authenticated through a sanctioned header.
    BufferedHeaderSecretSuccess,
    /// One successful streaming exchange authenticated through a sanctioned header.
    StreamingHeaderSecretSuccess,
    /// A header-secret request whose valid slot differs from the binding.
    HeaderSecretSlotMismatch,
}

fixed_debug!(HeaderAuthCaseIdV1 {
    BufferedHeaderSecretSuccess => "BufferedHeaderSecretSuccess",
    StreamingHeaderSecretSuccess => "StreamingHeaderSecretSuccess",
    HeaderSecretSlotMismatch => "HeaderSecretSlotMismatch",
});

/// A raw upstream exchange or fake-transport behavior for a canonical header-auth case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HeaderAuthUpstreamV1 {
    /// Complete one buffered exchange with this raw response.
    Response(ProviderCallRawResponseV1),
    /// Open a 2xx stream and script its chunks and terminal.
    Stream(ProviderStreamRawStreamV1),
    /// The transport boundary must not be reached.
    NotReached,
}

impl fmt::Debug for HeaderAuthUpstreamV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(raw) => formatter.debug_tuple("Response").field(raw).finish(),
            Self::Stream(raw) => formatter.debug_tuple("Stream").field(raw).finish(),
            Self::NotReached => formatter.write_str("NotReached"),
        }
    }
}

/// The exact expected terminal shape of one canonical header-auth case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HeaderAuthExpectedOutcomeV1 {
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

impl fmt::Debug for HeaderAuthExpectedOutcomeV1 {
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

/// Expected resolver, transport, and wire-shape boundary evidence.
///
/// The wire-shape booleans are adapter-reported like every other evidence field: a passing report
/// alone is insufficient, and the host-adoption review must confirm they are measured at the real
/// transport boundary.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HeaderAuthExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    sanctioned_header_exact: bool,
    authorization_header_absent: bool,
}

impl HeaderAuthExpectedEvidenceV1 {
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

    /// Returns whether the declared sanctioned header must carry the resolved secret byte for
    /// byte at the transport boundary. `false` when the transport must never be reached.
    #[must_use]
    pub const fn sanctioned_header_exact(&self) -> bool {
        self.sanctioned_header_exact
    }

    /// Returns whether no `authorization` header may exist at the transport boundary. Vacuously
    /// `true` when the transport must never be reached.
    #[must_use]
    pub const fn authorization_header_absent(&self) -> bool {
        self.authorization_header_absent
    }
}

impl fmt::Debug for HeaderAuthExpectedEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HeaderAuthExpectedEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field("sanctioned_header_exact", &self.sanctioned_header_exact)
            .field("authorization_header_absent", &self.authorization_header_absent)
            .finish()
    }
}

/// The expected outcome and boundary evidence for one header-auth fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HeaderAuthExpectedV1 {
    outcome: HeaderAuthExpectedOutcomeV1,
    evidence: HeaderAuthExpectedEvidenceV1,
}

impl HeaderAuthExpectedV1 {
    /// Returns the expected terminal shape.
    #[must_use]
    pub const fn outcome(&self) -> &HeaderAuthExpectedOutcomeV1 {
        &self.outcome
    }

    /// Returns the expected boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &HeaderAuthExpectedEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for HeaderAuthExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HeaderAuthExpectedV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// One immutable canonical header-auth fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HeaderAuthFixtureV1 {
    case_id: HeaderAuthCaseIdV1,
    input: ProviderCallInputV1,
    secret_header: SecretHeaderV1,
    upstream: HeaderAuthUpstreamV1,
    expected: HeaderAuthExpectedV1,
}

impl HeaderAuthFixtureV1 {
    /// Returns the stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> HeaderAuthCaseIdV1 {
        self.case_id
    }

    /// Returns the immutable raw input shared with the provider-call suite shape.
    #[must_use]
    pub const fn input(&self) -> &ProviderCallInputV1 {
        &self.input
    }

    /// Returns the sanctioned header the request declares.
    #[must_use]
    pub const fn secret_header(&self) -> SecretHeaderV1 {
        self.secret_header
    }

    /// Returns the canonical fake-upstream behavior.
    #[must_use]
    pub const fn upstream(&self) -> &HeaderAuthUpstreamV1 {
        &self.upstream
    }

    /// Returns the exact expected outcome and evidence.
    #[must_use]
    pub const fn expected(&self) -> &HeaderAuthExpectedV1 {
        &self.expected
    }
}

impl fmt::Debug for HeaderAuthFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HeaderAuthFixtureV1")
            .field("case_id", &self.case_id)
            .field("secret_header", &self.secret_header)
            .field("input", &self.input)
            .field("upstream", &self.upstream)
            .field("expected", &self.expected)
            .finish()
    }
}

const HEADER_AUTH_PATH: &str = "path-debug-sentinel";
const HEADER_AUTH_BOUND_SLOT: &str = "bound-slot-debug-sentinel";
const HEADER_AUTH_DIFFERENT_SLOT: &str = "requested-slot-debug-sentinel";
const HEADER_AUTH_RESPONSE_BODY: &str = r#"{"value":"response-body-debug-sentinel"}"#;
const HEADER_AUTH_CONTENT_TYPE: &str = "content-type-debug-sentinel";
const HEADER_AUTH_RETRY_AFTER: &str = "retry-after-debug-sentinel";
const HEADER_AUTH_CHUNK_ONE: &[u8] = b"header-auth-chunk-one-debug-sentinel";
const HEADER_AUTH_CHUNK_TWO: &[u8] = b"header-auth-chunk-two-debug-sentinel";
const HEADER_AUTH_CHUNKS: &[&[u8]] = &[HEADER_AUTH_CHUNK_ONE, HEADER_AUTH_CHUNK_TWO];

const fn wire_evidence(
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    sanctioned_header_exact: bool,
) -> HeaderAuthExpectedEvidenceV1 {
    HeaderAuthExpectedEvidenceV1 {
        resolver_calls,
        transport_calls,
        sanctioned_header_exact,
        authorization_header_absent: true,
    }
}

const HEADER_AUTH_FIXTURES: &[HeaderAuthFixtureV1] = &[
    HeaderAuthFixtureV1 {
        case_id: HeaderAuthCaseIdV1::BufferedHeaderSecretSuccess,
        input: input(HEADER_AUTH_PATH, HEADER_AUTH_BOUND_SLOT),
        secret_header: SecretHeaderV1::XApiKey,
        upstream: HeaderAuthUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 201,
            body: HEADER_AUTH_RESPONSE_BODY,
            content_type: Some(HEADER_AUTH_CONTENT_TYPE),
            retry_after: Some(HEADER_AUTH_RETRY_AFTER),
        }),
        expected: HeaderAuthExpectedV1 {
            outcome: HeaderAuthExpectedOutcomeV1::Response {
                status: 201,
                body: HEADER_AUTH_RESPONSE_BODY,
                content_type: Some(HEADER_AUTH_CONTENT_TYPE),
                retry_after: Some(HEADER_AUTH_RETRY_AFTER),
            },
            evidence: wire_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        },
    },
    HeaderAuthFixtureV1 {
        case_id: HeaderAuthCaseIdV1::StreamingHeaderSecretSuccess,
        input: input(HEADER_AUTH_PATH, HEADER_AUTH_BOUND_SLOT),
        secret_header: SecretHeaderV1::XGoogApiKey,
        upstream: HeaderAuthUpstreamV1::Stream(ProviderStreamRawStreamV1::assemble(
            ProviderStreamRawHeadV1::assemble(200, Some(HEADER_AUTH_CONTENT_TYPE), None),
            HEADER_AUTH_CHUNKS,
            ProviderStreamTerminalV1::CleanEof,
        )),
        expected: HeaderAuthExpectedV1 {
            outcome: HeaderAuthExpectedOutcomeV1::Opened {
                status: 200,
                content_type: Some(HEADER_AUTH_CONTENT_TYPE),
                retry_after: None,
                chunks: HEADER_AUTH_CHUNKS,
            },
            evidence: wire_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        },
    },
    HeaderAuthFixtureV1 {
        case_id: HeaderAuthCaseIdV1::HeaderSecretSlotMismatch,
        input: input(HEADER_AUTH_PATH, HEADER_AUTH_DIFFERENT_SLOT),
        secret_header: SecretHeaderV1::ApiKey,
        upstream: HeaderAuthUpstreamV1::NotReached,
        expected: HeaderAuthExpectedV1 {
            outcome: HeaderAuthExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::CredentialBindingMismatch,
            },
            evidence: wire_evidence(ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false),
        },
    },
];

/// Returns the immutable canonical header-auth fixture table.
#[must_use]
pub const fn header_auth_fixtures_v1() -> &'static [HeaderAuthFixtureV1] {
    HEADER_AUTH_FIXTURES
}
