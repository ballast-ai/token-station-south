//! Canonical fixtures for the controlled-query conformance suite.

use std::fmt;

use south_contracts::QueryParameterV1;
use south_provider_api::{QueryParameterDeclarationV1, QueryValueSyntaxV1};

use crate::{
    ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderCallInputV1, ProviderCallRawResponseV1,
    input,
    stream::{ProviderStreamRawHeadV1, ProviderStreamRawStreamV1, ProviderStreamTerminalV1},
};

/// The controlled-query conformance suite version.
pub const CONTROLLED_QUERY_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The stable identifier for controlled-query conformance version one.
pub const CONTROLLED_QUERY_CONFORMANCE_SUITE_ID: &str = "south.controlled-query.v1";

/// The closed set of canonical controlled-query cases.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ControlledQueryCaseIdV1 {
    /// One successful buffered exchange carrying a sanctioned `api-version`.
    BufferedQuerySuccess,
    /// One successful streaming exchange carrying the sanctioned `alt=sse`.
    StreamingQuerySuccess,
    /// A declared value violating its parameter's grammar, refused before any boundary.
    InvalidQueryValueRejected,
    /// Two parameters declared in reverse canonical order, proving the wire order is canonical.
    ///
    /// Every other case declares a single parameter, which makes ordering unobservable. Without
    /// this case an adapter could serialize in host-declaration order, pass the suite, and still
    /// disagree with another host on the wire bytes for the same declaration.
    ReversedDeclarationOrderIsCanonicalized,
    /// A request declaring no query at all, reaching the transport and expecting `false`.
    ///
    /// This case exists to close a blind spot measured on a real host adapter during the first
    /// adoption (2026-08-18). Before it, the four expected `wire_query_exact` values were
    /// `true / true / false / true`, and the only `false` belonged to the zero-call case — where
    /// the probe is never invoked. That `false` was therefore held by "structurally never
    /// reached" rather than by "measured and found false", so an adapter whose probe
    /// unconditionally reported `true` without ever reading
    /// `PreparedHttpRequestV1::url()` passed the entire suite: the three `true` expectations were
    /// satisfied by the hardcoded value and the `false` one by the zero call.
    ///
    /// This is the first case that both reaches the transport *and* expects `false`, which is the
    /// combination the table was missing. A correct probe reads the prepared URL, finds no query,
    /// compares it against a request that declared none, and reports `false` under the presence
    /// polarity of [`ControlledQueryExpectedEvidenceV1::wire_query_exact`] — nothing was observed
    /// carrying a declared query, because nothing was declared. A probe that hardcodes `true`
    /// reports `true` here and fails with a `WireQuery` mismatch.
    QueryFreeRequestReachesTheWire,
    /// One successful buffered exchange carrying the sanctioned `GroupId` (contract version five).
    ///
    /// The first case for a parameter admitted after the initial set: it proves the new name
    /// serializes with the upstream's exact casing and lands on the wire, so an adapter that
    /// lower-cases names or re-validates against a stale two-parameter table fails here and
    /// nowhere else.
    BufferedGroupIdQuerySuccess,
    // B7a (HTTP contract version ten): declared-instance cases, appended so no earlier case moves.
    /// A parameter the package declared in its manifest, beside a sanctioned one declared after
    /// it, on one buffered exchange.
    ///
    /// The host must turn the manifest declaration into the contract's declared form (in
    /// production through `south_component_conformance::DeclaredInstancesV1`) and the wire must
    /// carry the sanctioned parameter first and the declared one after it. A host that drops
    /// unknown names, or sends them in declaration order, fails here.
    BufferedDeclaredQuerySuccess,
    /// A declared `enum` parameter carrying a value outside its list, refused before any boundary.
    ///
    /// The value grammar of a declared name is its declared syntax; a host that admits any value
    /// for a declared name would send it.
    DeclaredQueryValueOutsideSyntaxRejected,
    /// A declaration with a credential-shaped name (`api_key`), refused before any boundary.
    ///
    /// Gate ① refuses such a manifest; this row proves a host that builds the declared form
    /// itself still goes through the contract's refusal rather than sending the raw pair.
    ReservedDeclaredNameRejected,
}

fixed_debug!(ControlledQueryCaseIdV1 {
    BufferedQuerySuccess => "BufferedQuerySuccess",
    StreamingQuerySuccess => "StreamingQuerySuccess",
    InvalidQueryValueRejected => "InvalidQueryValueRejected",
    ReversedDeclarationOrderIsCanonicalized => "ReversedDeclarationOrderIsCanonicalized",
    QueryFreeRequestReachesTheWire => "QueryFreeRequestReachesTheWire",
    BufferedGroupIdQuerySuccess => "BufferedGroupIdQuerySuccess",
    BufferedDeclaredQuerySuccess => "BufferedDeclaredQuerySuccess",
    DeclaredQueryValueOutsideSyntaxRejected => "DeclaredQueryValueOutsideSyntaxRejected",
    ReservedDeclaredNameRejected => "ReservedDeclaredNameRejected",
});

/// The value syntax of a declared parameter in a fixture: the manifest's closed set, in a form a
/// `const` table can hold.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ControlledQueryDeclaredSyntaxV1 {
    /// `"digits"`.
    Digits,
    /// `"token"`.
    Token,
    /// `"date"`.
    Date,
    /// `{"enum": [...]}`.
    Enum(&'static [&'static str]),
}

impl ControlledQueryDeclaredSyntaxV1 {
    /// The manifest syntax this fixture syntax stands for.
    #[must_use]
    pub fn manifest_syntax(self) -> QueryValueSyntaxV1 {
        match self {
            Self::Digits => QueryValueSyntaxV1::Digits,
            Self::Token => QueryValueSyntaxV1::Token,
            Self::Date => QueryValueSyntaxV1::Date,
            Self::Enum(values) => {
                QueryValueSyntaxV1::Enum(values.iter().map(|value| (*value).to_owned()).collect())
            }
        }
    }
    /// The contract syntax this fixture syntax stands for, as gate ① admits it into the
    /// `south_contracts::DeclaredQueryParameterV1` form.
    #[must_use]
    pub fn contract_syntax(self) -> south_contracts::QueryValueSyntaxV1 {
        match self {
            Self::Digits => south_contracts::QueryValueSyntaxV1::Digits,
            Self::Token => south_contracts::QueryValueSyntaxV1::Token,
            Self::Date => south_contracts::QueryValueSyntaxV1::Date,
            Self::Enum(values) => south_contracts::QueryValueSyntaxV1::Enum(
                values.iter().map(|value| (*value).to_owned()).collect(),
            ),
        }
    }
}

impl fmt::Debug for ControlledQueryDeclaredSyntaxV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Digits => formatter.write_str("Digits"),
            Self::Token => formatter.write_str("Token"),
            Self::Date => formatter.write_str("Date"),
            Self::Enum(values) => formatter.debug_tuple("Enum").field(&values.len()).finish(),
        }
    }
}

/// One parameter a case declares the way a package does (B7a): the manifest's `query_parameters`
/// entry, plus the value the request carries under that name.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ControlledQueryDeclaredParameterV1 {
    name: &'static str,
    syntax: ControlledQueryDeclaredSyntaxV1,
    value: &'static str,
}

impl ControlledQueryDeclaredParameterV1 {
    /// Returns the declared wire name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the declared value syntax.
    #[must_use]
    pub const fn syntax(&self) -> ControlledQueryDeclaredSyntaxV1 {
        self.syntax
    }

    /// Returns the value the request carries under this name.
    #[must_use]
    pub const fn value(&self) -> &'static str {
        self.value
    }

    /// Returns the declaration exactly as a manifest's `query_parameters` entry reads.
    #[must_use]
    pub fn manifest_declaration(&self) -> QueryParameterDeclarationV1 {
        QueryParameterDeclarationV1 {
            name: self.name.to_owned(),
            syntax: self.syntax.manifest_syntax(),
        }
    }
}

impl fmt::Debug for ControlledQueryDeclaredParameterV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlledQueryDeclaredParameterV1")
            .field("syntax", &self.syntax)
            .field("value_byte_count", &self.value.len())
            .finish_non_exhaustive()
    }
}

/// A raw upstream exchange or fake-transport behavior for a canonical controlled-query case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ControlledQueryUpstreamV1 {
    /// Complete one buffered exchange with this raw response.
    Response(ProviderCallRawResponseV1),
    /// Open a 2xx stream and script its chunks and terminal.
    Stream(ProviderStreamRawStreamV1),
    /// The transport boundary must not be reached.
    NotReached,
}

impl fmt::Debug for ControlledQueryUpstreamV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(raw) => formatter.debug_tuple("Response").field(raw).finish(),
            Self::Stream(raw) => formatter.debug_tuple("Stream").field(raw).finish(),
            Self::NotReached => formatter.write_str("NotReached"),
        }
    }
}

/// The exact expected terminal shape of one canonical controlled-query case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ControlledQueryExpectedOutcomeV1 {
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

impl fmt::Debug for ControlledQueryExpectedOutcomeV1 {
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

/// Expected resolver, transport, and wire-query boundary evidence.
///
/// The wire-query boolean is adapter-reported like every other evidence field: a passing report
/// alone is insufficient, and the host-adoption review must confirm it is measured at the real
/// transport boundary.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ControlledQueryExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    wire_query_exact: bool,
}

impl ControlledQueryExpectedEvidenceV1 {
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

    /// Returns whether the URL reaching the transport boundary must carry a query byte for byte
    /// equal to the declared canonical serialization.
    ///
    /// This is a *presence* claim, not an absence claim, which is why it is the mirror image of
    /// the header-auth suite's `authorization_header_absent`: it can only become true by observing
    /// a wire carrying a declared query, so a case whose transport must never be reached expects
    /// `false`, and so does a case that reaches the transport having declared no query at all.
    /// That polarity is what makes the negative case's zero-call discipline checkable — an adapter
    /// that quietly sent the rejected request would report `true` here and fail.
    ///
    /// The two ways of expecting `false` are not redundant.
    /// [`ControlledQueryCaseIdV1::InvalidQueryValueRejected`] proves the wire is never reached;
    /// [`ControlledQueryCaseIdV1::QueryFreeRequestReachesTheWire`] proves the probe is actually
    /// measuring, because it is the only case where the transport runs and the answer is still
    /// `false`.
    #[must_use]
    pub const fn wire_query_exact(&self) -> bool {
        self.wire_query_exact
    }
}

impl fmt::Debug for ControlledQueryExpectedEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlledQueryExpectedEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field("wire_query_exact", &self.wire_query_exact)
            .finish()
    }
}

/// The expected outcome and boundary evidence for one controlled-query fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ControlledQueryExpectedV1 {
    outcome: ControlledQueryExpectedOutcomeV1,
    evidence: ControlledQueryExpectedEvidenceV1,
}

impl ControlledQueryExpectedV1 {
    /// Returns the expected terminal shape.
    #[must_use]
    pub const fn outcome(&self) -> &ControlledQueryExpectedOutcomeV1 {
        &self.outcome
    }

    /// Returns the expected boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ControlledQueryExpectedEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ControlledQueryExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlledQueryExpectedV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// One immutable canonical controlled-query fixture.
///
/// The declared parameters are retained *raw* rather than as a constructed `QueryStringV1`: the
/// negative case exists precisely to exercise the construction failure, so the fixture must be
/// able to carry a value the contract rejects, and the query-free case must be able to carry no
/// parameters at all — a shape `QueryStringV1` refuses to represent.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ControlledQueryFixtureV1 {
    case_id: ControlledQueryCaseIdV1,
    input: ProviderCallInputV1,
    declared_query: &'static [(QueryParameterV1, &'static str)],
    declared_parameters: &'static [ControlledQueryDeclaredParameterV1],
    upstream: ControlledQueryUpstreamV1,
    expected: ControlledQueryExpectedV1,
}

impl ControlledQueryFixtureV1 {
    /// Returns the stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> ControlledQueryCaseIdV1 {
        self.case_id
    }

    /// Returns the immutable raw input shared with the provider-call suite shape.
    #[must_use]
    pub const fn input(&self) -> &ProviderCallInputV1 {
        &self.input
    }

    /// Returns the raw sanctioned parameters the request declares, in declaration order.
    ///
    /// An empty slice means the request declares no query, which is a valid fixture shape and
    /// must not be handed to `QueryStringV1::try_from_iter` — that constructor rejects an empty
    /// declaration rather than producing an empty query.
    #[must_use]
    pub const fn declared_query(&self) -> &'static [(QueryParameterV1, &'static str)] {
        self.declared_query
    }

    /// Returns the parameters the request declares through a package manifest (B7a), after the
    /// sanctioned ones in [`Self::declared_query`].
    ///
    /// An executor builds each parameter's declared form from
    /// [`ControlledQueryDeclaredParameterV1::manifest_declaration`] the way its production path
    /// does, and adds it to the same query as the sanctioned pairs. Empty for every case before
    /// HTTP contract version ten.
    #[must_use]
    pub const fn declared_parameters(&self) -> &'static [ControlledQueryDeclaredParameterV1] {
        self.declared_parameters
    }

    /// Returns the canonical fake-upstream behavior.
    #[must_use]
    pub const fn upstream(&self) -> &ControlledQueryUpstreamV1 {
        &self.upstream
    }

    /// Returns the exact expected outcome and evidence.
    #[must_use]
    pub const fn expected(&self) -> &ControlledQueryExpectedV1 {
        &self.expected
    }
}

impl fmt::Debug for ControlledQueryFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlledQueryFixtureV1")
            .field("case_id", &self.case_id)
            .field("declared_parameter_count", &self.declared_query.len())
            .field("manifest_parameters", &self.declared_parameters)
            .field("input", &self.input)
            .field("upstream", &self.upstream)
            .field("expected", &self.expected)
            .finish()
    }
}

const CONTROLLED_QUERY_PATH: &str = "path-debug-sentinel";
const CONTROLLED_QUERY_BOUND_SLOT: &str = "bound-slot-debug-sentinel";
const CONTROLLED_QUERY_RESPONSE_BODY: &str = r#"{"value":"response-body-debug-sentinel"}"#;
const CONTROLLED_QUERY_CONTENT_TYPE: &str = "content-type-debug-sentinel";
const CONTROLLED_QUERY_RETRY_AFTER: &str = "retry-after-debug-sentinel";
const CONTROLLED_QUERY_CHUNK_ONE: &[u8] = b"controlled-query-chunk-one-debug-sentinel";
const CONTROLLED_QUERY_CHUNK_TWO: &[u8] = b"controlled-query-chunk-two-debug-sentinel";
const CONTROLLED_QUERY_CHUNKS: &[&[u8]] = &[CONTROLLED_QUERY_CHUNK_ONE, CONTROLLED_QUERY_CHUNK_TWO];

/// A dated Azure-shaped version, chosen because it exercises the full accepted class.
const CONTROLLED_QUERY_API_VERSION: &str = "2025-04-01-preview";
/// The only value Gemini native streaming accepts for `alt`.
const CONTROLLED_QUERY_ALT: &str = "sse";
/// A value carrying a separator the `api-version` grammar rejects. It is deliberately a plain
/// grammar violation rather than an injection payload: the suite proves the contract refuses
/// before the wire, not that a particular exploit string is neutralized.
const CONTROLLED_QUERY_INVALID_API_VERSION: &str = "invalid value-debug-sentinel";

const BUFFERED_QUERY: &[(QueryParameterV1, &str)] =
    &[(QueryParameterV1::ApiVersion, CONTROLLED_QUERY_API_VERSION)];
const STREAMING_QUERY: &[(QueryParameterV1, &str)] =
    &[(QueryParameterV1::Alt, CONTROLLED_QUERY_ALT)];
const INVALID_QUERY: &[(QueryParameterV1, &str)] =
    &[(QueryParameterV1::ApiVersion, CONTROLLED_QUERY_INVALID_API_VERSION)];
/// Declared `alt` first, `api-version` second — the reverse of canonical order. The wire must
/// still carry `api-version=…&alt=sse`.
const REVERSED_ORDER_QUERY: &[(QueryParameterV1, &str)] = &[
    (QueryParameterV1::Alt, CONTROLLED_QUERY_ALT),
    (QueryParameterV1::ApiVersion, CONTROLLED_QUERY_API_VERSION),
];
/// No declaration at all. The request must reach the transport carrying no query, which is what
/// makes this the one case whose probe runs and must still answer `false`.
const QUERY_FREE_QUERY: &[(QueryParameterV1, &str)] = &[];
/// A nineteen-digit group id, the shape the `MiniMax` platform issues today.
const CONTROLLED_QUERY_GROUP_ID: &str = "1782000000000000000";
const GROUP_ID_QUERY: &[(QueryParameterV1, &str)] =
    &[(QueryParameterV1::GroupId, CONTROLLED_QUERY_GROUP_ID)];

const NO_DECLARED_PARAMETERS: &[ControlledQueryDeclaredParameterV1] = &[];
/// A sanctioned parameter the declared-instance success case pairs with its declared one.
const DECLARED_CASE_SANCTIONED_QUERY: &[(QueryParameterV1, &str)] =
    &[(QueryParameterV1::ApiVersion, CONTROLLED_QUERY_API_VERSION)];
/// The declared-instance success case's manifest parameter: a token name no sanctioned set has.
const DECLARED_TOKEN_PARAMETER: &[ControlledQueryDeclaredParameterV1] =
    &[ControlledQueryDeclaredParameterV1 {
        name: "declared-mode",
        syntax: ControlledQueryDeclaredSyntaxV1::Token,
        value: "declared.value_1",
    }];
/// An `enum` parameter whose value is not in its list.
const DECLARED_OUT_OF_SYNTAX_PARAMETER: &[ControlledQueryDeclaredParameterV1] =
    &[ControlledQueryDeclaredParameterV1 {
        name: "declared-region",
        syntax: ControlledQueryDeclaredSyntaxV1::Enum(&["eu", "us"]),
        value: "ap",
    }];
/// A credential-shaped declared name.
const DECLARED_RESERVED_NAME_PARAMETER: &[ControlledQueryDeclaredParameterV1] =
    &[ControlledQueryDeclaredParameterV1 {
        name: "api_key",
        syntax: ControlledQueryDeclaredSyntaxV1::Token,
        value: "declared-reserved-debug-sentinel",
    }];

const fn query_evidence(
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    wire_query_exact: bool,
) -> ControlledQueryExpectedEvidenceV1 {
    ControlledQueryExpectedEvidenceV1 { resolver_calls, transport_calls, wire_query_exact }
}

const CONTROLLED_QUERY_FIXTURES: &[ControlledQueryFixtureV1] = &[
    ControlledQueryFixtureV1 {
        case_id: ControlledQueryCaseIdV1::BufferedQuerySuccess,
        input: input(CONTROLLED_QUERY_PATH, CONTROLLED_QUERY_BOUND_SLOT),
        declared_parameters: NO_DECLARED_PARAMETERS,
        declared_query: BUFFERED_QUERY,
        upstream: ControlledQueryUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 201,
            body: CONTROLLED_QUERY_RESPONSE_BODY,
            content_type: Some(CONTROLLED_QUERY_CONTENT_TYPE),
            retry_after: Some(CONTROLLED_QUERY_RETRY_AFTER),
        }),
        expected: ControlledQueryExpectedV1 {
            outcome: ControlledQueryExpectedOutcomeV1::Response {
                status: 201,
                body: CONTROLLED_QUERY_RESPONSE_BODY,
                content_type: Some(CONTROLLED_QUERY_CONTENT_TYPE),
                retry_after: Some(CONTROLLED_QUERY_RETRY_AFTER),
            },
            evidence: query_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        },
    },
    ControlledQueryFixtureV1 {
        case_id: ControlledQueryCaseIdV1::StreamingQuerySuccess,
        input: input(CONTROLLED_QUERY_PATH, CONTROLLED_QUERY_BOUND_SLOT),
        declared_parameters: NO_DECLARED_PARAMETERS,
        declared_query: STREAMING_QUERY,
        upstream: ControlledQueryUpstreamV1::Stream(ProviderStreamRawStreamV1::assemble(
            ProviderStreamRawHeadV1::assemble(200, Some(CONTROLLED_QUERY_CONTENT_TYPE), None),
            CONTROLLED_QUERY_CHUNKS,
            ProviderStreamTerminalV1::CleanEof,
        )),
        expected: ControlledQueryExpectedV1 {
            outcome: ControlledQueryExpectedOutcomeV1::Opened {
                status: 200,
                content_type: Some(CONTROLLED_QUERY_CONTENT_TYPE),
                retry_after: None,
                chunks: CONTROLLED_QUERY_CHUNKS,
            },
            evidence: query_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        },
    },
    ControlledQueryFixtureV1 {
        case_id: ControlledQueryCaseIdV1::InvalidQueryValueRejected,
        input: input(CONTROLLED_QUERY_PATH, CONTROLLED_QUERY_BOUND_SLOT),
        declared_parameters: NO_DECLARED_PARAMETERS,
        declared_query: INVALID_QUERY,
        upstream: ControlledQueryUpstreamV1::NotReached,
        expected: ControlledQueryExpectedV1 {
            // Query contract errors are preparation-time destination failures with zero resolver
            // and transport calls, so the frozen nineteen-code set folds them into
            // `INVALID_RELATIVE_PATH` rather than widening for this suite. The finer
            // `ContractErrorV1` reason stays available to hosts that want it in their own logs.
            outcome: ControlledQueryExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::InvalidRelativePath,
            },
            evidence: query_evidence(ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false),
        },
    },
    ControlledQueryFixtureV1 {
        case_id: ControlledQueryCaseIdV1::ReversedDeclarationOrderIsCanonicalized,
        input: input(CONTROLLED_QUERY_PATH, CONTROLLED_QUERY_BOUND_SLOT),
        declared_parameters: NO_DECLARED_PARAMETERS,
        declared_query: REVERSED_ORDER_QUERY,
        upstream: ControlledQueryUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 200,
            body: CONTROLLED_QUERY_RESPONSE_BODY,
            content_type: Some(CONTROLLED_QUERY_CONTENT_TYPE),
            retry_after: None,
        }),
        expected: ControlledQueryExpectedV1 {
            outcome: ControlledQueryExpectedOutcomeV1::Response {
                status: 200,
                body: CONTROLLED_QUERY_RESPONSE_BODY,
                content_type: Some(CONTROLLED_QUERY_CONTENT_TYPE),
                retry_after: None,
            },
            // `wire_query_exact` compares the wire against the canonical serialization, so this
            // case fails for an adapter that emits parameters in host-declaration order.
            evidence: query_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        },
    },
    ControlledQueryFixtureV1 {
        case_id: ControlledQueryCaseIdV1::QueryFreeRequestReachesTheWire,
        input: input(CONTROLLED_QUERY_PATH, CONTROLLED_QUERY_BOUND_SLOT),
        declared_parameters: NO_DECLARED_PARAMETERS,
        declared_query: QUERY_FREE_QUERY,
        upstream: ControlledQueryUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 200,
            body: CONTROLLED_QUERY_RESPONSE_BODY,
            content_type: None,
            retry_after: None,
        }),
        expected: ControlledQueryExpectedV1 {
            outcome: ControlledQueryExpectedOutcomeV1::Response {
                status: 200,
                body: CONTROLLED_QUERY_RESPONSE_BODY,
                content_type: None,
                retry_after: None,
            },
            // The load-bearing row of the whole table: the transport *is* reached, and the answer
            // is still `false`. Every other `true` here can be satisfied by a probe that ignores
            // the prepared URL and hardcodes `true`; this row cannot. It fails such a probe with
            // a `WireQuery` mismatch, which is what turns the wire-query claim from a
            // review item into a machine-checkable fact.
            evidence: query_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, false),
        },
    },
    ControlledQueryFixtureV1 {
        case_id: ControlledQueryCaseIdV1::BufferedGroupIdQuerySuccess,
        input: input(CONTROLLED_QUERY_PATH, CONTROLLED_QUERY_BOUND_SLOT),
        declared_parameters: NO_DECLARED_PARAMETERS,
        declared_query: GROUP_ID_QUERY,
        upstream: ControlledQueryUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 200,
            body: CONTROLLED_QUERY_RESPONSE_BODY,
            content_type: Some(CONTROLLED_QUERY_CONTENT_TYPE),
            retry_after: None,
        }),
        expected: ControlledQueryExpectedV1 {
            outcome: ControlledQueryExpectedOutcomeV1::Response {
                status: 200,
                body: CONTROLLED_QUERY_RESPONSE_BODY,
                content_type: Some(CONTROLLED_QUERY_CONTENT_TYPE),
                retry_after: None,
            },
            // The wire must carry `GroupId=…` with the upstream's casing; the probe compares
            // against the canonical serialization, so a lower-cased name fails here.
            evidence: query_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        },
    },
    // B7a (HTTP contract version ten): declared-instance cases.
    ControlledQueryFixtureV1 {
        case_id: ControlledQueryCaseIdV1::BufferedDeclaredQuerySuccess,
        input: input(CONTROLLED_QUERY_PATH, CONTROLLED_QUERY_BOUND_SLOT),
        declared_query: DECLARED_CASE_SANCTIONED_QUERY,
        declared_parameters: DECLARED_TOKEN_PARAMETER,
        upstream: ControlledQueryUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 200,
            body: CONTROLLED_QUERY_RESPONSE_BODY,
            content_type: Some(CONTROLLED_QUERY_CONTENT_TYPE),
            retry_after: None,
        }),
        expected: ControlledQueryExpectedV1 {
            outcome: ControlledQueryExpectedOutcomeV1::Response {
                status: 200,
                body: CONTROLLED_QUERY_RESPONSE_BODY,
                content_type: Some(CONTROLLED_QUERY_CONTENT_TYPE),
                retry_after: None,
            },
            // The wire must be `api-version=…&declared-mode=declared.value_1`: sanctioned first,
            // declared after, both byte for byte.
            evidence: query_evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, true),
        },
    },
    ControlledQueryFixtureV1 {
        case_id: ControlledQueryCaseIdV1::DeclaredQueryValueOutsideSyntaxRejected,
        input: input(CONTROLLED_QUERY_PATH, CONTROLLED_QUERY_BOUND_SLOT),
        declared_query: QUERY_FREE_QUERY,
        declared_parameters: DECLARED_OUT_OF_SYNTAX_PARAMETER,
        upstream: ControlledQueryUpstreamV1::NotReached,
        expected: ControlledQueryExpectedV1 {
            // The same fold as the sanctioned grammar violation.
            outcome: ControlledQueryExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::InvalidRelativePath,
            },
            evidence: query_evidence(ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false),
        },
    },
    ControlledQueryFixtureV1 {
        case_id: ControlledQueryCaseIdV1::ReservedDeclaredNameRejected,
        input: input(CONTROLLED_QUERY_PATH, CONTROLLED_QUERY_BOUND_SLOT),
        declared_query: QUERY_FREE_QUERY,
        declared_parameters: DECLARED_RESERVED_NAME_PARAMETER,
        upstream: ControlledQueryUpstreamV1::NotReached,
        expected: ControlledQueryExpectedV1 {
            // A refused declaration is a preparation-time, zero-call declaration failure too.
            outcome: ControlledQueryExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::InvalidRelativePath,
            },
            evidence: query_evidence(ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false),
        },
    },
];

/// Returns the immutable canonical controlled-query fixture table.
#[must_use]
pub const fn controlled_query_fixtures_v1() -> &'static [ControlledQueryFixtureV1] {
    CONTROLLED_QUERY_FIXTURES
}
