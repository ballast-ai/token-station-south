//! Canonical fixtures for the multipart conformance suite.
//!
//! A multipart POST (HTTP contract version seven) carries opaque bytes under a media type the
//! contract renders, so it is a different call shape rather than a variant of the JSON POST the
//! provider-call suite freezes (multipart record, D6). The five cases prove what an adopting
//! host's audio-transcription and image-edit paths rely on: the bytes reach the transport
//! unmodified under each credential arm those paths use, the rendered `content-type` matches the
//! declared boundary exactly, and the two ways a host can hand over an inconsistent request —
//! a body that is not delimited by the boundary it declared, and a `content-type` smuggled
//! through the ordinary header channel — are both refused before any boundary is touched.

use std::fmt;

use south_contracts::SecretHeaderV1;

use crate::{ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderCallRawResponseV1};

/// The multipart conformance suite version.
pub const PROVIDER_MULTIPART_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The stable identifier for multipart conformance version one.
pub const PROVIDER_MULTIPART_CONFORMANCE_SUITE_ID: &str = "south.provider-multipart.v1";

/// The closed set of canonical multipart cases.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderMultipartCaseIdV1 {
    /// One successful buffered multipart POST under the Bearer arm.
    BufferedMultipartBearerSuccess,
    /// One successful buffered multipart POST authenticated through a sanctioned header.
    BufferedMultipartHeaderSecretSuccess,
    /// A multipart POST whose valid requested slot differs from the binding, refused before
    /// resolver and transport.
    MultipartSlotMismatch,
    /// A body that is not delimited by the boundary it declared, refused at construction.
    ///
    /// This is the case the shape exists to catch. A host splices a client's multipart body in
    /// place — replacing a text field's value, never touching a binary part — and a splice that
    /// damaged the boundary would otherwise put bytes on the wire under a `content-type` that no
    /// longer describes them. Nothing downstream can detect that; the upstream simply reports a
    /// malformed request, and the host reads it as a provider problem.
    MultipartBodyBoundaryMismatch,
    /// A `content-type` smuggled through the ordinary header channel, refused at construction.
    ///
    /// The second way a body and its media type can come to disagree: this shape renders its own
    /// `content-type`, so a host-supplied one would be a second source. An adapter that silently
    /// drops the smuggled header instead of refusing the request passes every other case in this
    /// table and fails this one.
    MultipartContentTypeSmuggled,
}

fixed_debug!(ProviderMultipartCaseIdV1 {
    BufferedMultipartBearerSuccess => "BufferedMultipartBearerSuccess",
    BufferedMultipartHeaderSecretSuccess => "BufferedMultipartHeaderSecretSuccess",
    MultipartSlotMismatch => "MultipartSlotMismatch",
    MultipartBodyBoundaryMismatch => "MultipartBodyBoundaryMismatch",
    MultipartContentTypeSmuggled => "MultipartContentTypeSmuggled",
});

/// The credential arm a canonical multipart request declares.
///
/// Closed to the two arms the adopting host's multipart paths use. The combined and host-signed
/// arms bind credentials through exactly the same code on every request shape, so repeating them
/// here would measure nothing the header-auth and host-signed suites do not already measure.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderMultipartAuthArmV1 {
    /// `Authorization: Bearer …`.
    Bearer,
    /// The secret verbatim in one sanctioned header, no `authorization`.
    HeaderSecret(SecretHeaderV1),
}

impl fmt::Debug for ProviderMultipartAuthArmV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer => formatter.write_str("Bearer"),
            Self::HeaderSecret(header) => {
                formatter.debug_tuple("HeaderSecret").field(header).finish()
            }
        }
    }
}

/// Raw multipart input retained exactly as static test data.
///
/// The provider-call input shape with the JSON body replaced by opaque bytes and the boundary
/// that delimits them. A multipart fixture cannot carry a JSON body, and the type says so.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderMultipartInputV1 {
    endpoint: &'static str,
    bound_credential_slot: &'static str,
    requested_credential_slot: &'static str,
    relative_path: &'static str,
    headers: &'static [(&'static str, &'static str)],
    body: &'static [u8],
    boundary: &'static str,
}

impl ProviderMultipartInputV1 {
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
    ///
    /// One case deliberately carries a `content-type` here, which the request shape must refuse.
    #[must_use]
    pub const fn headers(&self) -> &'static [(&'static str, &'static str)] {
        self.headers
    }

    /// Returns the raw multipart body bytes.
    #[must_use]
    pub const fn body(&self) -> &'static [u8] {
        self.body
    }

    /// Returns the raw boundary, without its `--` prefix.
    #[must_use]
    pub const fn boundary(&self) -> &'static str {
        self.boundary
    }

    /// Returns the media type a correct implementation renders for this input.
    ///
    /// Built here from the same boundary the request declares, so an executor comparing against
    /// it is comparing against the contract's rule rather than against South's own answer.
    #[must_use]
    pub fn expected_content_type(&self) -> String {
        format!("multipart/form-data; boundary={}", self.boundary)
    }
}

impl fmt::Debug for ProviderMultipartInputV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMultipartInputV1")
            .field("endpoint_byte_count", &self.endpoint.len())
            .field("bound_credential_slot_byte_count", &self.bound_credential_slot.len())
            .field("requested_credential_slot_byte_count", &self.requested_credential_slot.len())
            .field("relative_path_byte_count", &self.relative_path.len())
            .field("header_count", &self.headers.len())
            .field("body_byte_count", &self.body.len())
            .field("boundary_byte_count", &self.boundary.len())
            .finish()
    }
}

/// A raw upstream response or fake-transport behavior for a canonical multipart case.
///
/// Buffered only (multipart record, D5): there is no streaming multipart to script.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderMultipartUpstreamV1 {
    /// Complete one buffered exchange with this raw response.
    Response(ProviderCallRawResponseV1),
    /// The transport boundary must not be reached.
    NotReached,
}

impl fmt::Debug for ProviderMultipartUpstreamV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(raw) => formatter.debug_tuple("Response").field(raw).finish(),
            Self::NotReached => formatter.write_str("NotReached"),
        }
    }
}

/// The exact expected terminal shape of one canonical multipart case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderMultipartExpectedOutcomeV1 {
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

impl fmt::Debug for ProviderMultipartExpectedOutcomeV1 {
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
/// Both wire-shape booleans are **presence claims**: `false` until a transport call observed the
/// fact, so the three rows that never reach the transport expect `false` for both, and an
/// adapter whose probe answers without reading the prepared request fails those rows. There is
/// deliberately no absence claim in this table — everything worth asserting about a multipart
/// request is something that must be *seen* on the wire.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderMultipartExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    wire_content_type_exact: bool,
    wire_body_bytes_exact: bool,
}

impl ProviderMultipartExpectedEvidenceV1 {
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

    /// Returns whether the rendered media type must have reached the transport byte for byte.
    #[must_use]
    pub const fn wire_content_type_exact(&self) -> bool {
        self.wire_content_type_exact
    }

    /// Returns whether the declared body bytes must have reached the transport unmodified.
    #[must_use]
    pub const fn wire_body_bytes_exact(&self) -> bool {
        self.wire_body_bytes_exact
    }
}

impl fmt::Debug for ProviderMultipartExpectedEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMultipartExpectedEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field("wire_content_type_exact", &self.wire_content_type_exact)
            .field("wire_body_bytes_exact", &self.wire_body_bytes_exact)
            .finish()
    }
}

/// The expected outcome and boundary evidence for one multipart fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderMultipartExpectedV1 {
    outcome: ProviderMultipartExpectedOutcomeV1,
    evidence: ProviderMultipartExpectedEvidenceV1,
}

impl ProviderMultipartExpectedV1 {
    /// Returns the expected terminal shape.
    #[must_use]
    pub const fn outcome(&self) -> &ProviderMultipartExpectedOutcomeV1 {
        &self.outcome
    }

    /// Returns the expected boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderMultipartExpectedEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ProviderMultipartExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMultipartExpectedV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// One immutable canonical multipart fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderMultipartFixtureV1 {
    case_id: ProviderMultipartCaseIdV1,
    input: ProviderMultipartInputV1,
    auth_arm: ProviderMultipartAuthArmV1,
    upstream: ProviderMultipartUpstreamV1,
    expected: ProviderMultipartExpectedV1,
}

impl ProviderMultipartFixtureV1 {
    /// Returns the stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> ProviderMultipartCaseIdV1 {
        self.case_id
    }

    /// Returns the immutable raw input.
    #[must_use]
    pub const fn input(&self) -> &ProviderMultipartInputV1 {
        &self.input
    }

    /// Returns the credential arm the request declares.
    #[must_use]
    pub const fn auth_arm(&self) -> ProviderMultipartAuthArmV1 {
        self.auth_arm
    }

    /// Returns the canonical fake-upstream behavior.
    #[must_use]
    pub const fn upstream(&self) -> &ProviderMultipartUpstreamV1 {
        &self.upstream
    }

    /// Returns the exact expected outcome and evidence.
    #[must_use]
    pub const fn expected(&self) -> &ProviderMultipartExpectedV1 {
        &self.expected
    }
}

impl fmt::Debug for ProviderMultipartFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMultipartFixtureV1")
            .field("case_id", &self.case_id)
            .field("auth_arm", &self.auth_arm)
            .field("input", &self.input)
            .field("upstream", &self.upstream)
            .field("expected", &self.expected)
            .finish()
    }
}

const MULTIPART_ENDPOINT: &str = "https://endpoint-debug-sentinel.invalid/base";
const MULTIPART_PATH: &str = "path-debug-sentinel";
const MULTIPART_BOUND_SLOT: &str = "bound-slot-debug-sentinel";
const MULTIPART_DIFFERENT_SLOT: &str = "requested-slot-debug-sentinel";
const MULTIPART_HEADERS: &[(&str, &str)] =
    &[("header-name-debug-sentinel", "header-value-debug-sentinel")];
const MULTIPART_RESPONSE_BODY: &str = r#"{"value":"response-body-debug-sentinel"}"#;
const MULTIPART_CONTENT_TYPE: &str = "content-type-debug-sentinel";

/// The declared boundary. Hyphens and letters only, well inside the RFC 2046 §5.1.1 grammar.
const MULTIPART_BOUNDARY: &str = "boundary-debug-sentinel";
/// A different, equally valid boundary — the one the mismatched body is actually delimited by.
const MULTIPART_OTHER_BOUNDARY: &str = "other-boundary-debug-sentinel";

/// A minimal well-formed body: one text part, opened and closed by [`MULTIPART_BOUNDARY`].
///
/// Deliberately shaped like the field an adopting host splices — a `model` part whose value it
/// rewrites to the canonical upstream name — so the bytes this table pins are the bytes a real
/// call site produces.
const MULTIPART_BODY: &[u8] = b"--boundary-debug-sentinel\r\n\
Content-Disposition: form-data; name=\"model\"\r\n\
\r\n\
body-value-debug-sentinel\r\n\
--boundary-debug-sentinel--\r\n";

/// The same body delimited by a *different* boundary than the request declares: what a splice
/// that damaged the delimiter leaves behind.
const MULTIPART_MISMATCHED_BODY: &[u8] = b"--other-boundary-debug-sentinel\r\n\
Content-Disposition: form-data; name=\"model\"\r\n\
\r\n\
body-value-debug-sentinel\r\n\
--other-boundary-debug-sentinel--\r\n";

/// Ordinary headers carrying a `content-type` the request shape must refuse.
const MULTIPART_SMUGGLED_HEADERS: &[(&str, &str)] = &[
    ("header-name-debug-sentinel", "header-value-debug-sentinel"),
    ("content-type", "multipart/form-data; boundary=boundary-debug-sentinel"),
];

const fn multipart_input(
    requested_slot: &'static str,
    headers: &'static [(&'static str, &'static str)],
    body: &'static [u8],
) -> ProviderMultipartInputV1 {
    ProviderMultipartInputV1 {
        endpoint: MULTIPART_ENDPOINT,
        bound_credential_slot: MULTIPART_BOUND_SLOT,
        requested_credential_slot: requested_slot,
        relative_path: MULTIPART_PATH,
        headers,
        body,
        boundary: MULTIPART_BOUNDARY,
    }
}

/// Evidence for a case that reaches the transport once and is measured there.
const REACHED: ProviderMultipartExpectedEvidenceV1 = ProviderMultipartExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1::One,
    transport_calls: ProviderCallCountV1::One,
    wire_content_type_exact: true,
    wire_body_bytes_exact: true,
};

/// Evidence for a case refused before any boundary: nothing was observed, so both presence
/// claims are `false`.
const NOT_REACHED: ProviderMultipartExpectedEvidenceV1 = ProviderMultipartExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1::Zero,
    transport_calls: ProviderCallCountV1::Zero,
    wire_content_type_exact: false,
    wire_body_bytes_exact: false,
};

const fn success_response(retry_after: Option<&'static str>) -> ProviderMultipartUpstreamV1 {
    ProviderMultipartUpstreamV1::Response(ProviderCallRawResponseV1 {
        status: 200,
        body: MULTIPART_RESPONSE_BODY,
        content_type: Some(MULTIPART_CONTENT_TYPE),
        retry_after,
    })
}

const fn success_expected(retry_after: Option<&'static str>) -> ProviderMultipartExpectedV1 {
    ProviderMultipartExpectedV1 {
        outcome: ProviderMultipartExpectedOutcomeV1::Response {
            status: 200,
            body: MULTIPART_RESPONSE_BODY,
            content_type: Some(MULTIPART_CONTENT_TYPE),
            retry_after,
        },
        evidence: REACHED,
    }
}

const fn refused(code: ProviderCallFailureCodeV1) -> ProviderMultipartExpectedV1 {
    ProviderMultipartExpectedV1 {
        outcome: ProviderMultipartExpectedOutcomeV1::Failure { code },
        evidence: NOT_REACHED,
    }
}

const PROVIDER_MULTIPART_FIXTURES: &[ProviderMultipartFixtureV1] = &[
    ProviderMultipartFixtureV1 {
        case_id: ProviderMultipartCaseIdV1::BufferedMultipartBearerSuccess,
        input: multipart_input(MULTIPART_BOUND_SLOT, MULTIPART_HEADERS, MULTIPART_BODY),
        auth_arm: ProviderMultipartAuthArmV1::Bearer,
        upstream: success_response(None),
        expected: success_expected(None),
    },
    ProviderMultipartFixtureV1 {
        case_id: ProviderMultipartCaseIdV1::BufferedMultipartHeaderSecretSuccess,
        input: multipart_input(MULTIPART_BOUND_SLOT, MULTIPART_HEADERS, MULTIPART_BODY),
        // The arm the adopting host's Azure image-edit path uses.
        auth_arm: ProviderMultipartAuthArmV1::HeaderSecret(SecretHeaderV1::ApiKey),
        upstream: success_response(None),
        expected: success_expected(None),
    },
    ProviderMultipartFixtureV1 {
        case_id: ProviderMultipartCaseIdV1::MultipartSlotMismatch,
        input: multipart_input(MULTIPART_DIFFERENT_SLOT, MULTIPART_HEADERS, MULTIPART_BODY),
        auth_arm: ProviderMultipartAuthArmV1::Bearer,
        upstream: ProviderMultipartUpstreamV1::NotReached,
        expected: refused(ProviderCallFailureCodeV1::CredentialBindingMismatch),
    },
    ProviderMultipartFixtureV1 {
        case_id: ProviderMultipartCaseIdV1::MultipartBodyBoundaryMismatch,
        input: multipart_input(MULTIPART_BOUND_SLOT, MULTIPART_HEADERS, MULTIPART_MISMATCHED_BODY),
        auth_arm: ProviderMultipartAuthArmV1::Bearer,
        upstream: ProviderMultipartUpstreamV1::NotReached,
        // The frozen nineteen-code set has no multipart-specific code and deliberately stays
        // frozen; a body rejected at construction is a preparation-time, zero-call declaration
        // failure, which is the shape that set folds into `INVALID_RELATIVE_PATH`. Folding into
        // `INVALID_JSON_BODY` would state that a JSON body was invalid, and there is none.
        expected: refused(ProviderCallFailureCodeV1::InvalidRelativePath),
    },
    ProviderMultipartFixtureV1 {
        case_id: ProviderMultipartCaseIdV1::MultipartContentTypeSmuggled,
        input: multipart_input(MULTIPART_BOUND_SLOT, MULTIPART_SMUGGLED_HEADERS, MULTIPART_BODY),
        auth_arm: ProviderMultipartAuthArmV1::Bearer,
        upstream: ProviderMultipartUpstreamV1::NotReached,
        expected: refused(ProviderCallFailureCodeV1::InvalidRelativePath),
    },
];

/// Returns the immutable canonical multipart fixture table.
#[must_use]
pub const fn provider_multipart_fixtures_v1() -> &'static [ProviderMultipartFixtureV1] {
    PROVIDER_MULTIPART_FIXTURES
}

/// Returns the boundary the mismatched-body case is actually delimited by.
///
/// Exposed so a fixture test can prove the two boundaries differ without hardcoding either — the
/// case is only meaningful while they do.
#[must_use]
pub const fn provider_multipart_mismatched_boundary_v1() -> &'static str {
    MULTIPART_OTHER_BOUNDARY
}
