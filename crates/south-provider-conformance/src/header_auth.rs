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
    /// One successful buffered exchange under the combined arm (auth contract version four): the
    /// same resolved secret must reach the wire twice — verbatim in the sanctioned header **and**
    /// `Bearer `-prefixed in `authorization` — with exactly one resolver call behind both.
    ///
    /// This is the only case whose `authorization_header_absent` expectation is `false`. An
    /// adapter that hardcodes the absence claim, or resolves the slot once per header, fails
    /// here and nowhere else.
    BufferedBearerAndHeaderSecretSuccess,
    /// One successful buffered exchange under a package-declared secret header (auth contract
    /// version five): the declared name carries the resolved secret verbatim, alone.
    BufferedDeclaredHeaderSecretSuccess,
    /// The declared name smuggled through the ordinary header channel of the declaring package's
    /// request (reserved-header policy version two), refused before resolver and transport.
    ///
    /// An adapter that validates ordinary headers without the package's declaration passes every
    /// other case and sends the smuggled value here.
    DeclaredHeaderSmuggledThroughOrdinaryChannel,
    /// An upstream that echoes the declared header, and a sanctioned one, in its response: the
    /// transcript keeps an ordinary control header and drops both secret names.
    ///
    /// An adapter whose transport captures the transcript without the request's declaration
    /// fails here and nowhere else.
    DeclaredHeaderRedactedFromTranscript,
}

fixed_debug!(HeaderAuthCaseIdV1 {
    BufferedHeaderSecretSuccess => "BufferedHeaderSecretSuccess",
    StreamingHeaderSecretSuccess => "StreamingHeaderSecretSuccess",
    HeaderSecretSlotMismatch => "HeaderSecretSlotMismatch",
    BufferedBearerAndHeaderSecretSuccess => "BufferedBearerAndHeaderSecretSuccess",
    BufferedDeclaredHeaderSecretSuccess => "BufferedDeclaredHeaderSecretSuccess",
    DeclaredHeaderSmuggledThroughOrdinaryChannel => "DeclaredHeaderSmuggledThroughOrdinaryChannel",
    DeclaredHeaderRedactedFromTranscript => "DeclaredHeaderRedactedFromTranscript",
});

/// The secret-bearing header a canonical header-auth request presents its credential in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HeaderAuthHeaderV1 {
    /// A sanctioned name, presented through the closed `HeaderSecret` arm, or through
    /// `BearerAndHeaderSecret` when [`HeaderAuthFixtureV1::bearer_alongside`] is set.
    Sanctioned(SecretHeaderV1),
    /// A name the package declares (auth contract version five), presented through
    /// `DeclaredHeaderSecret`. It is always one of
    /// [`HeaderAuthFixtureV1::declared_secret_headers`].
    Declared(&'static str),
}

impl HeaderAuthHeaderV1 {
    /// Returns the lowercase wire name.
    #[must_use]
    pub const fn header_name(self) -> &'static str {
        match self {
            Self::Sanctioned(header) => header.header_name(),
            Self::Declared(name) => name,
        }
    }
}

impl fmt::Debug for HeaderAuthHeaderV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Header names are not secrets; values never enter this type.
        match self {
            Self::Sanctioned(header) => formatter.debug_tuple("Sanctioned").field(header).finish(),
            Self::Declared(name) => formatter.debug_tuple("Declared").field(name).finish(),
        }
    }
}

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

/// What the buffered response's display transcript must and must not hold.
///
/// Not an exact match: a real transport over a real socket transcribes headers the fixture never
/// scripted (`date`, `content-length`), so the claim is containment. `retained` proves a transcript
/// was captured at all, so dropping every header cannot pass as redaction.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HeaderAuthExpectedTranscriptV1 {
    retained: &'static [(&'static str, &'static str)],
    redacted: &'static [&'static str],
}

impl HeaderAuthExpectedTranscriptV1 {
    /// Returns the name and value pairs the transcript must contain.
    #[must_use]
    pub const fn retained(&self) -> &'static [(&'static str, &'static str)] {
        self.retained
    }

    /// Returns the names the transcript must not contain, under any value.
    #[must_use]
    pub const fn redacted(&self) -> &'static [&'static str] {
        self.redacted
    }
}

impl fmt::Debug for HeaderAuthExpectedTranscriptV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HeaderAuthExpectedTranscriptV1")
            .field("retained_count", &self.retained.len())
            .field("redacted", &self.redacted)
            .finish()
    }
}

/// The expected outcome and boundary evidence for one header-auth fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HeaderAuthExpectedV1 {
    outcome: HeaderAuthExpectedOutcomeV1,
    evidence: HeaderAuthExpectedEvidenceV1,
    transcript: Option<HeaderAuthExpectedTranscriptV1>,
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

    /// Returns the transcript expectation, for the cases that make one. Only a buffered
    /// response case does.
    #[must_use]
    pub const fn transcript(&self) -> Option<&HeaderAuthExpectedTranscriptV1> {
        self.transcript.as_ref()
    }
}

impl fmt::Debug for HeaderAuthExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HeaderAuthExpectedV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .field("transcript", &self.transcript)
            .finish()
    }
}

/// One immutable canonical header-auth fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HeaderAuthFixtureV1 {
    case_id: HeaderAuthCaseIdV1,
    input: ProviderCallInputV1,
    header: HeaderAuthHeaderV1,
    bearer_alongside: bool,
    declared_secret_headers: &'static [&'static str],
    upstream: HeaderAuthUpstreamV1,
    upstream_response_headers: &'static [(&'static str, &'static str)],
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

    /// Returns the header the request presents its credential in.
    ///
    /// Replaced `secret_header()` when auth contract version five admitted declared names: a
    /// [`HeaderAuthHeaderV1::Declared`] header has no [`SecretHeaderV1`] to return, and an
    /// executor must choose the declared arm for it rather than a sanctioned one.
    #[must_use]
    pub const fn header(&self) -> HeaderAuthHeaderV1 {
        self.header
    }

    /// Returns whether the request declares the combined arm — the secret also travels as
    /// `Authorization: Bearer …` — rather than the header-secret arm alone. Only ever set with a
    /// sanctioned header.
    #[must_use]
    pub const fn bearer_alongside(&self) -> bool {
        self.bearer_alongside
    }

    /// Returns the secret headers the request's package declares (reserved-header policy version
    /// two). The adapter validates the input's ordinary headers under this declaration and passes
    /// it to whatever captures the response transcript. Empty for the sanctioned-header cases.
    #[must_use]
    pub const fn declared_secret_headers(&self) -> &'static [&'static str] {
        self.declared_secret_headers
    }

    /// Returns the canonical fake-upstream behavior.
    #[must_use]
    pub const fn upstream(&self) -> &HeaderAuthUpstreamV1 {
        &self.upstream
    }

    /// Returns the extra headers the fake upstream sends, verbatim and in this order, on a
    /// buffered [`HeaderAuthUpstreamV1::Response`]. Empty except for the transcript case.
    #[must_use]
    pub const fn upstream_response_headers(&self) -> &'static [(&'static str, &'static str)] {
        self.upstream_response_headers
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
            .field("header", &self.header)
            .field("bearer_alongside", &self.bearer_alongside)
            .field("declared_secret_headers", &self.declared_secret_headers)
            .field("input", &self.input)
            .field("upstream", &self.upstream)
            .field("upstream_response_header_count", &self.upstream_response_headers.len())
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

/// A synthetic package-declared secret header: no provider uses it, and it is on no reserved list.
const DECLARED_HEADER: &str = "x-south-fixture-key";
const DECLARED_SECRET_HEADERS: &[&str] = &[DECLARED_HEADER];
/// The shared ordinary header plus the declared name carrying a value of the adapter's choosing.
const SMUGGLING_HEADERS: &[(&str, &str)] = &[
    ("header-name-debug-sentinel", "header-value-debug-sentinel"),
    (DECLARED_HEADER, "smuggled-value-debug-sentinel"),
];
const TRANSCRIPT_CONTROL: (&str, &str) =
    ("x-south-transcript-control", "transcript-control-debug-sentinel");
/// The upstream echoes both secret names and one ordinary control header.
const ECHOING_RESPONSE_HEADERS: &[(&str, &str)] = &[
    (DECLARED_HEADER, "echoed-declared-secret-debug-sentinel"),
    ("x-api-key", "echoed-sanctioned-secret-debug-sentinel"),
    TRANSCRIPT_CONTROL,
];

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

/// The combined arm's evidence: the sanctioned header is exact **and** `authorization` is on the
/// wire, so the absence claim is expected `false` — the only such row in the table.
const fn dual_wire_evidence(
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
) -> HeaderAuthExpectedEvidenceV1 {
    HeaderAuthExpectedEvidenceV1 {
        resolver_calls,
        transport_calls,
        sanctioned_header_exact: true,
        authorization_header_absent: false,
    }
}

const HEADER_AUTH_FIXTURES: &[HeaderAuthFixtureV1] = &[
    HeaderAuthFixtureV1 {
        case_id: HeaderAuthCaseIdV1::BufferedHeaderSecretSuccess,
        input: input(HEADER_AUTH_PATH, HEADER_AUTH_BOUND_SLOT),
        header: HeaderAuthHeaderV1::Sanctioned(SecretHeaderV1::XApiKey),
        bearer_alongside: false,
        declared_secret_headers: &[],
        upstream: HeaderAuthUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 201,
            body: HEADER_AUTH_RESPONSE_BODY,
            content_type: Some(HEADER_AUTH_CONTENT_TYPE),
            retry_after: Some(HEADER_AUTH_RETRY_AFTER),
        }),
        upstream_response_headers: &[],
        expected: HeaderAuthExpectedV1 {
            outcome: HeaderAuthExpectedOutcomeV1::Response {
                status: 201,
                body: HEADER_AUTH_RESPONSE_BODY,
                content_type: Some(HEADER_AUTH_CONTENT_TYPE),
                retry_after: Some(HEADER_AUTH_RETRY_AFTER),
            },
            evidence: wire_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, true),
            transcript: None,
        },
    },
    HeaderAuthFixtureV1 {
        case_id: HeaderAuthCaseIdV1::StreamingHeaderSecretSuccess,
        input: input(HEADER_AUTH_PATH, HEADER_AUTH_BOUND_SLOT),
        header: HeaderAuthHeaderV1::Sanctioned(SecretHeaderV1::XGoogApiKey),
        bearer_alongside: false,
        declared_secret_headers: &[],
        upstream: HeaderAuthUpstreamV1::Stream(ProviderStreamRawStreamV1::assemble(
            ProviderStreamRawHeadV1::assemble(200, Some(HEADER_AUTH_CONTENT_TYPE), None),
            HEADER_AUTH_CHUNKS,
            ProviderStreamTerminalV1::CleanEof,
        )),
        upstream_response_headers: &[],
        expected: HeaderAuthExpectedV1 {
            outcome: HeaderAuthExpectedOutcomeV1::Opened {
                status: 200,
                content_type: Some(HEADER_AUTH_CONTENT_TYPE),
                retry_after: None,
                chunks: HEADER_AUTH_CHUNKS,
            },
            evidence: wire_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, true),
            transcript: None,
        },
    },
    HeaderAuthFixtureV1 {
        case_id: HeaderAuthCaseIdV1::HeaderSecretSlotMismatch,
        input: input(HEADER_AUTH_PATH, HEADER_AUTH_DIFFERENT_SLOT),
        header: HeaderAuthHeaderV1::Sanctioned(SecretHeaderV1::ApiKey),
        bearer_alongside: false,
        declared_secret_headers: &[],
        upstream: HeaderAuthUpstreamV1::NotReached,
        upstream_response_headers: &[],
        expected: HeaderAuthExpectedV1 {
            outcome: HeaderAuthExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::CredentialBindingMismatch,
            },
            evidence: wire_evidence(ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false),
            transcript: None,
        },
    },
    HeaderAuthFixtureV1 {
        case_id: HeaderAuthCaseIdV1::BufferedBearerAndHeaderSecretSuccess,
        input: input(HEADER_AUTH_PATH, HEADER_AUTH_BOUND_SLOT),
        // The one production shape that needs both: Gemini's OpenAI-compatible surface.
        header: HeaderAuthHeaderV1::Sanctioned(SecretHeaderV1::XGoogApiKey),
        bearer_alongside: true,
        declared_secret_headers: &[],
        upstream: HeaderAuthUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 200,
            body: HEADER_AUTH_RESPONSE_BODY,
            content_type: Some(HEADER_AUTH_CONTENT_TYPE),
            retry_after: None,
        }),
        upstream_response_headers: &[],
        expected: HeaderAuthExpectedV1 {
            outcome: HeaderAuthExpectedOutcomeV1::Response {
                status: 200,
                body: HEADER_AUTH_RESPONSE_BODY,
                content_type: Some(HEADER_AUTH_CONTENT_TYPE),
                retry_after: None,
            },
            evidence: dual_wire_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One),
            transcript: None,
        },
    },
    HeaderAuthFixtureV1 {
        case_id: HeaderAuthCaseIdV1::BufferedDeclaredHeaderSecretSuccess,
        input: input(HEADER_AUTH_PATH, HEADER_AUTH_BOUND_SLOT),
        header: HeaderAuthHeaderV1::Declared(DECLARED_HEADER),
        bearer_alongside: false,
        declared_secret_headers: DECLARED_SECRET_HEADERS,
        upstream: HeaderAuthUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 201,
            body: HEADER_AUTH_RESPONSE_BODY,
            content_type: Some(HEADER_AUTH_CONTENT_TYPE),
            retry_after: Some(HEADER_AUTH_RETRY_AFTER),
        }),
        upstream_response_headers: &[],
        expected: HeaderAuthExpectedV1 {
            outcome: HeaderAuthExpectedOutcomeV1::Response {
                status: 201,
                body: HEADER_AUTH_RESPONSE_BODY,
                content_type: Some(HEADER_AUTH_CONTENT_TYPE),
                retry_after: Some(HEADER_AUTH_RETRY_AFTER),
            },
            evidence: wire_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, true),
            transcript: None,
        },
    },
    HeaderAuthFixtureV1 {
        case_id: HeaderAuthCaseIdV1::DeclaredHeaderSmuggledThroughOrdinaryChannel,
        input: ProviderCallInputV1 {
            headers: SMUGGLING_HEADERS,
            ..input(HEADER_AUTH_PATH, HEADER_AUTH_BOUND_SLOT)
        },
        header: HeaderAuthHeaderV1::Declared(DECLARED_HEADER),
        bearer_alongside: false,
        declared_secret_headers: DECLARED_SECRET_HEADERS,
        upstream: HeaderAuthUpstreamV1::NotReached,
        upstream_response_headers: &[],
        expected: HeaderAuthExpectedV1 {
            // The frozen nineteen-code set has no header-policy code; a reserved name on the
            // ordinary channel folds into the context-free `REQUEST_FAILED` at zero calls, as the
            // controlled user-agent suite's smuggled `user-agent` case already does.
            outcome: HeaderAuthExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::RequestFailed,
            },
            evidence: wire_evidence(ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false),
            transcript: None,
        },
    },
    HeaderAuthFixtureV1 {
        case_id: HeaderAuthCaseIdV1::DeclaredHeaderRedactedFromTranscript,
        input: input(HEADER_AUTH_PATH, HEADER_AUTH_BOUND_SLOT),
        header: HeaderAuthHeaderV1::Declared(DECLARED_HEADER),
        bearer_alongside: false,
        declared_secret_headers: DECLARED_SECRET_HEADERS,
        upstream: HeaderAuthUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 200,
            body: HEADER_AUTH_RESPONSE_BODY,
            content_type: Some(HEADER_AUTH_CONTENT_TYPE),
            retry_after: None,
        }),
        upstream_response_headers: ECHOING_RESPONSE_HEADERS,
        expected: HeaderAuthExpectedV1 {
            outcome: HeaderAuthExpectedOutcomeV1::Response {
                status: 200,
                body: HEADER_AUTH_RESPONSE_BODY,
                content_type: Some(HEADER_AUTH_CONTENT_TYPE),
                retry_after: None,
            },
            evidence: wire_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, true),
            transcript: Some(HeaderAuthExpectedTranscriptV1 {
                retained: &[TRANSCRIPT_CONTROL],
                redacted: &[DECLARED_HEADER, "x-api-key"],
            }),
        },
    },
];

/// Returns the immutable canonical header-auth fixture table.
#[must_use]
pub const fn header_auth_fixtures_v1() -> &'static [HeaderAuthFixtureV1] {
    HEADER_AUTH_FIXTURES
}
