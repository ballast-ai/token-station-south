//! Canonical fixtures for the buffered-GET conformance suite.
//!
//! A body-less GET (HTTP contract version six) is a different call shape, not a variant of the
//! JSON POST the provider-call suite freezes, so it has its own table (buffered-GET record, D4).
//! The four cases prove what the task-polling consumer relies on: the request reaches the
//! transport as a `GET` with no body slot, under each credential arm the submit leg already
//! uses, the binding check runs before any boundary, and a `task_id` declaration lands on the
//! wire exactly.

use std::fmt;

use south_contracts::{QueryParameterV1, SecretHeaderV1};

use crate::{
    BOUND_SLOT, DIFFERENT_SLOT, ENDPOINT, HEADERS, ProviderCallCountV1, ProviderCallFailureCodeV1,
    ProviderCallRawResponseV1,
};

/// The buffered-GET conformance suite version.
pub const PROVIDER_GET_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The stable identifier for buffered-GET conformance version one.
pub const PROVIDER_GET_CONFORMANCE_SUITE_ID: &str = "south.provider-get.v1";

/// The closed set of canonical buffered-GET cases.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderGetCaseIdV1 {
    /// One successful buffered GET under the Bearer arm, declaring no query.
    BufferedGetBearerSuccess,
    /// One successful buffered GET authenticated through a sanctioned header, declaring no query.
    BufferedGetHeaderSecretSuccess,
    /// A GET whose valid requested slot differs from the binding, refused before resolver and
    /// transport.
    GetSlotMismatch,
    /// One successful buffered GET declaring `task_id` (the one polling family that carries the
    /// id as a query rather than a path segment), expecting the wire query to be exact.
    BufferedGetTaskIdQuerySuccess,
}

fixed_debug!(ProviderGetCaseIdV1 {
    BufferedGetBearerSuccess => "BufferedGetBearerSuccess",
    BufferedGetHeaderSecretSuccess => "BufferedGetHeaderSecretSuccess",
    GetSlotMismatch => "GetSlotMismatch",
    BufferedGetTaskIdQuerySuccess => "BufferedGetTaskIdQuerySuccess",
});

/// The credential arm a canonical GET declares.
///
/// Closed to the two arms the polling consumer uses. The combined arm and the host-signed arm
/// are exercised by their own suites on the POST shape; a GET binds credentials through exactly
/// the same code, so repeating them here would measure nothing new.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderGetAuthArmV1 {
    /// `Authorization: Bearer …`.
    Bearer,
    /// The secret verbatim in one sanctioned header, no `authorization`.
    HeaderSecret(SecretHeaderV1),
}

impl fmt::Debug for ProviderGetAuthArmV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer => formatter.write_str("Bearer"),
            Self::HeaderSecret(header) => {
                formatter.debug_tuple("HeaderSecret").field(header).finish()
            }
        }
    }
}

/// Raw buffered-GET input retained exactly as static test data.
///
/// The provider-call input shape minus its JSON body: a GET fixture must not be able to carry a
/// body, or an executor could send one and still match the table.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderGetInputV1 {
    endpoint: &'static str,
    bound_credential_slot: &'static str,
    requested_credential_slot: &'static str,
    relative_path: &'static str,
    headers: &'static [(&'static str, &'static str)],
}

impl ProviderGetInputV1 {
    /// Returns the raw trusted endpoint.
    #[must_use]
    pub const fn endpoint(&self) -> &'static str {
        self.endpoint
    }

    /// Returns the raw credential slot bound to the endpoint.
    #[must_use]
    pub const fn bound_credential_slot(&self) -> &'static str {
        self.bound_credential_slot
    }

    /// Returns the raw credential slot requested by the provider.
    #[must_use]
    pub const fn requested_credential_slot(&self) -> &'static str {
        self.requested_credential_slot
    }

    /// Returns the raw relative path.
    #[must_use]
    pub const fn relative_path(&self) -> &'static str {
        self.relative_path
    }

    /// Returns the borrowed ordinary header pairs.
    #[must_use]
    pub const fn headers(&self) -> &'static [(&'static str, &'static str)] {
        self.headers
    }
}

impl fmt::Debug for ProviderGetInputV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderGetInputV1")
            .field("endpoint_byte_count", &self.endpoint.len())
            .field("bound_credential_slot_byte_count", &self.bound_credential_slot.len())
            .field("requested_credential_slot_byte_count", &self.requested_credential_slot.len())
            .field("relative_path_byte_count", &self.relative_path.len())
            .field("header_count", &self.headers.len())
            .finish()
    }
}

/// A raw upstream response or fake-transport behavior for a canonical buffered-GET case.
///
/// Buffered only (buffered-GET record, D2): there is no streaming GET to script.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderGetUpstreamV1 {
    /// Complete one buffered exchange with this raw response.
    Response(ProviderCallRawResponseV1),
    /// The transport boundary must not be reached.
    NotReached,
}

impl fmt::Debug for ProviderGetUpstreamV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(raw) => formatter.debug_tuple("Response").field(raw).finish(),
            Self::NotReached => formatter.write_str("NotReached"),
        }
    }
}

/// The exact expected terminal shape of one canonical buffered-GET case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderGetExpectedOutcomeV1 {
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
    /// A known stable failure.
    Failure {
        /// Expected closed failure code.
        code: ProviderCallFailureCodeV1,
    },
}

impl fmt::Debug for ProviderGetExpectedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response { status, body, content_type, retry_after } => formatter
                .debug_struct("Response")
                .field("status", status)
                .field("body_byte_count", &body.len())
                .field("has_content_type", &content_type.is_some())
                .field("has_retry_after", &retry_after.is_some())
                .finish(),
            Self::Failure { code } => {
                formatter.debug_struct("Failure").field("code", code).finish()
            }
        }
    }
}

/// Expected resolver, transport, and wire-shape boundary evidence.
///
/// The three wire-shape booleans are adapter-reported like every other evidence field, and each
/// has a fixed polarity so that a value the transport never measured cannot pass as a measured
/// one:
///
/// - `wire_method_get` is a presence claim — `true` only when a transport call observed the
///   method `GET`; `false` when the transport is never reached.
/// - `wire_body_absent` is an absence claim — vacuously `true` when the transport is never
///   reached, and `true` at the boundary only when the prepared request had no body slot at all.
/// - `wire_query_exact` is a presence claim with the controlled-query suite's polarity — `true`
///   only when the request declared a query *and* the wire carried it byte for byte. Two rows of
///   this table reach the transport and still expect `false`, which is what catches a probe that
///   hardcodes `true` without reading the prepared URL.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderGetExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    wire_method_get: bool,
    wire_body_absent: bool,
    wire_query_exact: bool,
}

impl ProviderGetExpectedEvidenceV1 {
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

    /// Returns whether a transport call must have observed the method `GET`. `false` when the
    /// transport must never be reached.
    #[must_use]
    pub const fn wire_method_get(&self) -> bool {
        self.wire_method_get
    }

    /// Returns whether no body may exist at the transport boundary. Vacuously `true` when the
    /// transport must never be reached.
    #[must_use]
    pub const fn wire_body_absent(&self) -> bool {
        self.wire_body_absent
    }

    /// Returns whether the wire must carry exactly the query the request declared. `false` when
    /// nothing was declared, even though the request reaches the transport.
    #[must_use]
    pub const fn wire_query_exact(&self) -> bool {
        self.wire_query_exact
    }
}

impl fmt::Debug for ProviderGetExpectedEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderGetExpectedEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field("wire_method_get", &self.wire_method_get)
            .field("wire_body_absent", &self.wire_body_absent)
            .field("wire_query_exact", &self.wire_query_exact)
            .finish()
    }
}

/// The expected outcome and boundary evidence for one buffered-GET fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderGetExpectedV1 {
    outcome: ProviderGetExpectedOutcomeV1,
    evidence: ProviderGetExpectedEvidenceV1,
}

impl ProviderGetExpectedV1 {
    /// Returns the expected terminal shape.
    #[must_use]
    pub const fn outcome(&self) -> &ProviderGetExpectedOutcomeV1 {
        &self.outcome
    }

    /// Returns the expected boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderGetExpectedEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ProviderGetExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderGetExpectedV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// One immutable canonical buffered-GET fixture.
///
/// The declared parameters are retained raw, as the controlled-query table does: an empty slice
/// means the request declares no query and must not be handed to `QueryStringV1::try_from_iter`,
/// which rejects an empty declaration.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderGetFixtureV1 {
    case_id: ProviderGetCaseIdV1,
    input: ProviderGetInputV1,
    auth_arm: ProviderGetAuthArmV1,
    declared_query: &'static [(QueryParameterV1, &'static str)],
    upstream: ProviderGetUpstreamV1,
    expected: ProviderGetExpectedV1,
}

impl ProviderGetFixtureV1 {
    /// Returns the stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> ProviderGetCaseIdV1 {
        self.case_id
    }

    /// Returns the immutable raw input.
    #[must_use]
    pub const fn input(&self) -> &ProviderGetInputV1 {
        &self.input
    }

    /// Returns the credential arm the request declares.
    #[must_use]
    pub const fn auth_arm(&self) -> ProviderGetAuthArmV1 {
        self.auth_arm
    }

    /// Returns the raw sanctioned parameters the request declares, in declaration order.
    #[must_use]
    pub const fn declared_query(&self) -> &'static [(QueryParameterV1, &'static str)] {
        self.declared_query
    }

    /// Returns the canonical fake-upstream behavior.
    #[must_use]
    pub const fn upstream(&self) -> &ProviderGetUpstreamV1 {
        &self.upstream
    }

    /// Returns the exact expected outcome and evidence.
    #[must_use]
    pub const fn expected(&self) -> &ProviderGetExpectedV1 {
        &self.expected
    }
}

impl fmt::Debug for ProviderGetFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderGetFixtureV1")
            .field("case_id", &self.case_id)
            .field("auth_arm", &self.auth_arm)
            .field("declared_parameter_count", &self.declared_query.len())
            .field("input", &self.input)
            .field("upstream", &self.upstream)
            .field("expected", &self.expected)
            .finish()
    }
}

const PROVIDER_GET_PATH: &str = "path-debug-sentinel";
const PROVIDER_GET_RESPONSE_BODY: &str = r#"{"value":"response-body-debug-sentinel"}"#;
const PROVIDER_GET_CONTENT_TYPE: &str = "content-type-debug-sentinel";
const PROVIDER_GET_RETRY_AFTER: &str = "retry-after-debug-sentinel";
/// A fifteen-digit task id, the shape the `MiniMax` platform returns from `video_generation`.
const PROVIDER_GET_TASK_ID: &str = "276843862449040";

const NO_QUERY: &[(QueryParameterV1, &str)] = &[];
const TASK_ID_QUERY: &[(QueryParameterV1, &str)] =
    &[(QueryParameterV1::TaskId, PROVIDER_GET_TASK_ID)];

const fn get_input(requested_slot: &'static str) -> ProviderGetInputV1 {
    ProviderGetInputV1 {
        endpoint: ENDPOINT,
        bound_credential_slot: BOUND_SLOT,
        requested_credential_slot: requested_slot,
        relative_path: PROVIDER_GET_PATH,
        headers: HEADERS,
    }
}

/// Evidence for a case that reaches the transport once: the method is measured `GET`, the body
/// slot is measured absent, and the query claim is whatever the declaration allows.
const fn reached(wire_query_exact: bool) -> ProviderGetExpectedEvidenceV1 {
    ProviderGetExpectedEvidenceV1 {
        resolver_calls: ProviderCallCountV1::One,
        transport_calls: ProviderCallCountV1::One,
        wire_method_get: true,
        wire_body_absent: true,
        wire_query_exact,
    }
}

/// Evidence for a case refused before any boundary: nothing was measured, so the presence
/// claims are `false` and the absence claim is vacuously `true`.
const NOT_REACHED: ProviderGetExpectedEvidenceV1 = ProviderGetExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1::Zero,
    transport_calls: ProviderCallCountV1::Zero,
    wire_method_get: false,
    wire_body_absent: true,
    wire_query_exact: false,
};

const PROVIDER_GET_FIXTURES: &[ProviderGetFixtureV1] = &[
    ProviderGetFixtureV1 {
        case_id: ProviderGetCaseIdV1::BufferedGetBearerSuccess,
        input: get_input(BOUND_SLOT),
        auth_arm: ProviderGetAuthArmV1::Bearer,
        declared_query: NO_QUERY,
        upstream: ProviderGetUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 200,
            body: PROVIDER_GET_RESPONSE_BODY,
            content_type: Some(PROVIDER_GET_CONTENT_TYPE),
            retry_after: Some(PROVIDER_GET_RETRY_AFTER),
        }),
        expected: ProviderGetExpectedV1 {
            outcome: ProviderGetExpectedOutcomeV1::Response {
                status: 200,
                body: PROVIDER_GET_RESPONSE_BODY,
                content_type: Some(PROVIDER_GET_CONTENT_TYPE),
                retry_after: Some(PROVIDER_GET_RETRY_AFTER),
            },
            evidence: reached(false),
        },
    },
    ProviderGetFixtureV1 {
        case_id: ProviderGetCaseIdV1::BufferedGetHeaderSecretSuccess,
        input: get_input(BOUND_SLOT),
        auth_arm: ProviderGetAuthArmV1::HeaderSecret(SecretHeaderV1::XApiKey),
        declared_query: NO_QUERY,
        upstream: ProviderGetUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 200,
            body: PROVIDER_GET_RESPONSE_BODY,
            content_type: Some(PROVIDER_GET_CONTENT_TYPE),
            retry_after: None,
        }),
        expected: ProviderGetExpectedV1 {
            outcome: ProviderGetExpectedOutcomeV1::Response {
                status: 200,
                body: PROVIDER_GET_RESPONSE_BODY,
                content_type: Some(PROVIDER_GET_CONTENT_TYPE),
                retry_after: None,
            },
            evidence: reached(false),
        },
    },
    ProviderGetFixtureV1 {
        case_id: ProviderGetCaseIdV1::GetSlotMismatch,
        input: get_input(DIFFERENT_SLOT),
        auth_arm: ProviderGetAuthArmV1::Bearer,
        declared_query: NO_QUERY,
        upstream: ProviderGetUpstreamV1::NotReached,
        expected: ProviderGetExpectedV1 {
            outcome: ProviderGetExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::CredentialBindingMismatch,
            },
            evidence: NOT_REACHED,
        },
    },
    ProviderGetFixtureV1 {
        case_id: ProviderGetCaseIdV1::BufferedGetTaskIdQuerySuccess,
        input: get_input(BOUND_SLOT),
        auth_arm: ProviderGetAuthArmV1::Bearer,
        declared_query: TASK_ID_QUERY,
        upstream: ProviderGetUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 200,
            body: PROVIDER_GET_RESPONSE_BODY,
            content_type: Some(PROVIDER_GET_CONTENT_TYPE),
            retry_after: None,
        }),
        expected: ProviderGetExpectedV1 {
            outcome: ProviderGetExpectedOutcomeV1::Response {
                status: 200,
                body: PROVIDER_GET_RESPONSE_BODY,
                content_type: Some(PROVIDER_GET_CONTENT_TYPE),
                retry_after: None,
            },
            // The wire must carry `task_id=…` exactly; the probe compares against the canonical
            // serialization, so a renamed or re-encoded parameter fails here.
            evidence: reached(true),
        },
    },
];

/// Returns the immutable canonical buffered-GET fixture table.
#[must_use]
pub const fn provider_get_fixtures_v1() -> &'static [ProviderGetFixtureV1] {
    PROVIDER_GET_FIXTURES
}
