//! Canonical fixtures for the buffered-binary conformance suite.
//!
//! A buffered binary response (HTTP contract version eight) is the first response-side shape South
//! offers beside the frozen UTF-8 one, so it has its own table rather than widening the
//! provider-call suite (binary-response record, D8). The six cases prove what an adopting host's
//! speech-synthesis and image-rendering paths rely on: a body that is not UTF-8 arrives byte for
//! byte, a rejection's JSON body arrives the same way instead of becoming a transport error, the
//! binary cap is the binary one rather than the text one, and — the row the whole slice is
//! judged by — the frozen UTF-8 path still refuses those same bytes.
//!
//! Bearer is the only credential arm here. Every call site this shape exists for rides
//! `ProviderType::Openai`, and the sanctioned-header and combined arms bind credentials through
//! exactly the same code on every request shape, so repeating them would measure nothing the
//! header-auth and multipart suites do not already measure.

use std::fmt;

use south_contracts::{MAX_BINARY_RESPONSE_BODY_BYTES, MAX_RESPONSE_BODY_BYTES};

use crate::{
    BOUND_SLOT, DIFFERENT_SLOT, ENDPOINT, HEADERS, ProviderCallCountV1, ProviderCallFailureCodeV1,
};

/// The buffered-binary conformance suite version.
pub const PROVIDER_BINARY_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The stable identifier for buffered-binary conformance version one.
pub const PROVIDER_BINARY_CONFORMANCE_SUITE_ID: &str = "south.provider-binary.v1";

/// The closed set of canonical buffered-binary cases.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderBinaryCaseIdV1 {
    /// A 2xx whose body is not valid UTF-8, delivered byte for byte.
    ///
    /// The case the slice exists for: this exchange fails at the transport boundary on HTTP
    /// contract version seven, because every buffered response was UTF-8 by construction. Nothing
    /// downstream could work around it — the bytes were gone before the host saw them.
    BinarySuccessNonUtf8Body,
    /// A non-2xx whose body is JSON, returned as bytes with the upstream status.
    ///
    /// Defends D3. An upstream that answers a success with audio answers a rejection with JSON, so
    /// a type whose body shape depended on the status would be the worst of both worlds. South
    /// hands back bytes on every status and the host decodes the error body itself. An
    /// implementation that turned a non-2xx into a transport error fails here.
    BinaryRejectionCarriesJsonBody,
    /// A body above the UTF-8 cap and within the binary cap, accepted.
    ///
    /// Defends D4, and it is the only row that can catch a binary arm which silently reuses the
    /// 32 MiB text cap. Such an implementation passes every other case in this table and refuses
    /// this one, which in production is a silent fallback to a host's legacy path for exactly the
    /// artifacts this shape exists to carry.
    BinaryBodyAboveTextCapSucceeds,
    /// A body one byte above the binary cap, refused.
    ///
    /// The other side of D4: the cap moved, it did not disappear. An implementation that dropped
    /// the bound entirely passes the row above and fails this one.
    BinaryBodyAboveBinaryCapRefused,
    /// A valid requested slot that differs from the binding, refused before resolver and
    /// transport.
    ///
    /// The structural row every suite carries: the binding check runs before any boundary, so a
    /// binary call cannot reach a credential or a wire it was never bound to.
    BinarySlotMismatch,
    /// The same non-UTF-8 bytes as the first case, driven through the frozen UTF-8 entry point,
    /// still refused.
    ///
    /// The polarity row, and the one that judges D1. An implementation that widened
    /// [`crate::ProviderCallCaseIdV1`]'s response type instead of adding a second one passes cases
    /// one through five and fails this one. It is what proves the UTF-8 guarantee two hosts
    /// already depend on was not quietly removed to make the other five pass.
    TextArmStillRefusesNonUtf8,
}

fixed_debug!(ProviderBinaryCaseIdV1 {
    BinarySuccessNonUtf8Body => "BinarySuccessNonUtf8Body",
    BinaryRejectionCarriesJsonBody => "BinaryRejectionCarriesJsonBody",
    BinaryBodyAboveTextCapSucceeds => "BinaryBodyAboveTextCapSucceeds",
    BinaryBodyAboveBinaryCapRefused => "BinaryBodyAboveBinaryCapRefused",
    BinarySlotMismatch => "BinarySlotMismatch",
    TextArmStillRefusesNonUtf8 => "TextArmStillRefusesNonUtf8",
});

/// Which South entry point a canonical case drives.
///
/// Carried by the fixture rather than inferred by the runner, so a table row cannot be driven
/// through the wrong arm by accident. Five rows take the binary entry point; one deliberately
/// takes the frozen UTF-8 one, and that row is the suite's regression guard rather than a
/// description of anything a host should build.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderBinaryEntryArmV1 {
    /// `execute_binary_call_v1`, returning a body that was never required to be UTF-8.
    Binary,
    /// `execute_provider_call_v1`, the frozen UTF-8 entry point, which must still refuse
    /// non-UTF-8 bytes.
    Utf8,
}

fixed_debug!(ProviderBinaryEntryArmV1 {
    Binary => "Binary",
    Utf8 => "Utf8",
});

/// A canonical response body, either retained literally or materialised on demand.
///
/// The two cap rows need bodies of 32 and 64 mebibytes, and a fixture table is static data that
/// two hosts compile: embedding those as literals would put a hundred megabytes of zeroes in
/// every consumer's binary to assert two bounds. The synthesized form keeps the table small and
/// lets the executor build the buffer only while the case runs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderBinaryBodyV1 {
    /// Bytes retained exactly as static test data.
    Literal(&'static [u8]),
    /// `length` repetitions of `fill`, materialised by the executor.
    Synthesized {
        /// The repeated byte.
        fill: u8,
        /// The total byte length.
        length: usize,
    },
}

impl ProviderBinaryBodyV1 {
    /// Returns the body's byte length without materialising it.
    #[must_use]
    pub const fn len(&self) -> usize {
        match self {
            Self::Literal(bytes) => bytes.len(),
            Self::Synthesized { length, .. } => *length,
        }
    }

    /// Returns whether the body is empty. No canonical row is, but the accessor pair belongs
    /// together.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Materialises the body's bytes.
    ///
    /// Only an executor about to hand bytes to a fake upstream needs this; comparing an observed
    /// body against the table uses [`Self::matches`], which allocates nothing.
    #[must_use]
    pub fn materialize(&self) -> Vec<u8> {
        match self {
            Self::Literal(bytes) => bytes.to_vec(),
            Self::Synthesized { fill, length } => vec![*fill; *length],
        }
    }

    /// Returns whether observed bytes are exactly this body.
    ///
    /// Deliberately allocation-free: the cap rows would otherwise build a second 64 MiB buffer per
    /// comparison, which is a lot of memory to spend proving a length.
    #[must_use]
    pub fn matches(&self, observed: &[u8]) -> bool {
        match self {
            Self::Literal(bytes) => observed == *bytes,
            Self::Synthesized { fill, length } => {
                observed.len() == *length && observed.iter().all(|byte| byte == fill)
            }
        }
    }
}

impl fmt::Debug for ProviderBinaryBodyV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Literal(bytes) => {
                formatter.debug_struct("Literal").field("byte_count", &bytes.len()).finish()
            }
            Self::Synthesized { length, .. } => {
                formatter.debug_struct("Synthesized").field("byte_count", length).finish()
            }
        }
    }
}

/// A borrowed raw upstream response whose body is bytes rather than text.
///
/// The provider-call raw response with its `&'static str` body replaced by
/// [`ProviderBinaryBodyV1`]. A binary fixture cannot carry a UTF-8 body, and the type says so.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderBinaryRawResponseV1 {
    status: u16,
    body: ProviderBinaryBodyV1,
    content_type: Option<&'static str>,
    retry_after: Option<&'static str>,
}

impl ProviderBinaryRawResponseV1 {
    /// Returns the raw HTTP status.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Returns the raw response body.
    #[must_use]
    pub const fn body(&self) -> ProviderBinaryBodyV1 {
        self.body
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

impl fmt::Debug for ProviderBinaryRawResponseV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderBinaryRawResponseV1")
            .field("status", &self.status)
            .field("body", &self.body)
            .field("has_content_type", &self.content_type.is_some())
            .field("has_retry_after", &self.retry_after.is_some())
            .finish()
    }
}

/// Raw buffered-binary input retained exactly as static test data.
///
/// The provider-call input shape unchanged: a binary response is a property of the *answer*, so
/// the request side of this suite is an ordinary JSON POST and the table says so by carrying one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderBinaryInputV1 {
    endpoint: &'static str,
    bound_credential_slot: &'static str,
    requested_credential_slot: &'static str,
    relative_path: &'static str,
    json_body: &'static str,
    headers: &'static [(&'static str, &'static str)],
}

impl ProviderBinaryInputV1 {
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

    /// Returns the raw JSON request body.
    #[must_use]
    pub const fn json_body(&self) -> &'static str {
        self.json_body
    }

    /// Returns the borrowed ordinary header pairs.
    #[must_use]
    pub const fn headers(&self) -> &'static [(&'static str, &'static str)] {
        self.headers
    }
}

impl fmt::Debug for ProviderBinaryInputV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderBinaryInputV1")
            .field("endpoint_byte_count", &self.endpoint.len())
            .field("bound_credential_slot_byte_count", &self.bound_credential_slot.len())
            .field("requested_credential_slot_byte_count", &self.requested_credential_slot.len())
            .field("relative_path_byte_count", &self.relative_path.len())
            .field("json_body_byte_count", &self.json_body.len())
            .field("header_count", &self.headers.len())
            .finish()
    }
}

/// A raw upstream response or fake-transport behavior for a canonical buffered-binary case.
///
/// Buffered only (binary-response record, D9): binary streaming was never constrained, so there is
/// no streaming binary exchange to script.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderBinaryUpstreamV1 {
    /// Complete one buffered exchange with this raw response.
    Response(ProviderBinaryRawResponseV1),
    /// The transport boundary must not be reached.
    NotReached,
}

impl fmt::Debug for ProviderBinaryUpstreamV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(raw) => formatter.debug_tuple("Response").field(raw).finish(),
            Self::NotReached => formatter.write_str("NotReached"),
        }
    }
}

/// The exact expected terminal shape of one canonical buffered-binary case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderBinaryExpectedOutcomeV1 {
    /// A bounded buffered binary response matched field by field.
    Response {
        /// Expected status.
        status: u16,
        /// Expected body bytes.
        body: ProviderBinaryBodyV1,
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

impl fmt::Debug for ProviderBinaryExpectedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response { status, body, content_type, retry_after } => formatter
                .debug_struct("Response")
                .field("status", status)
                .field("body", body)
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
/// Both wire-shape booleans are **presence claims**: `false` until an exchange actually produced
/// the fact, never vacuously true. There is deliberately no absence claim here — a binary response
/// is judged by what came back, and everything worth asserting is something that must be *seen*.
///
/// - `wire_binary_response_observed` — `true` only when the response was carried by the binary
///   transport seam. `false` on the row refused before any boundary, and `false` on the UTF-8 row,
///   which reaches a transport and still must report `false`. That row is what catches a probe
///   answering from the fixture rather than from the seam it actually used.
/// - `wire_body_bytes_exact` — `true` only when a response came back and its bytes are exactly
///   what the fake upstream produced. `false` wherever no response was produced at all, including
///   the over-cap row, which reaches the transport and still must report `false`.
///
/// Two of the six rows therefore reach a transport and expect `false` for at least one claim,
/// which is what makes the table unpassable by an adapter whose probe hardcodes `true`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderBinaryExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    wire_binary_response_observed: bool,
    wire_body_bytes_exact: bool,
}

impl ProviderBinaryExpectedEvidenceV1 {
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

    /// Returns whether the binary transport seam must have produced the response.
    #[must_use]
    pub const fn wire_binary_response_observed(&self) -> bool {
        self.wire_binary_response_observed
    }

    /// Returns whether the returned body must be exactly the bytes the upstream produced.
    #[must_use]
    pub const fn wire_body_bytes_exact(&self) -> bool {
        self.wire_body_bytes_exact
    }
}

impl fmt::Debug for ProviderBinaryExpectedEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderBinaryExpectedEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field("wire_binary_response_observed", &self.wire_binary_response_observed)
            .field("wire_body_bytes_exact", &self.wire_body_bytes_exact)
            .finish()
    }
}

/// The expected outcome and boundary evidence for one buffered-binary fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderBinaryExpectedV1 {
    outcome: ProviderBinaryExpectedOutcomeV1,
    evidence: ProviderBinaryExpectedEvidenceV1,
}

impl ProviderBinaryExpectedV1 {
    /// Returns the expected terminal shape.
    #[must_use]
    pub const fn outcome(&self) -> &ProviderBinaryExpectedOutcomeV1 {
        &self.outcome
    }

    /// Returns the expected boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderBinaryExpectedEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ProviderBinaryExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderBinaryExpectedV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// One immutable canonical buffered-binary fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderBinaryFixtureV1 {
    case_id: ProviderBinaryCaseIdV1,
    input: ProviderBinaryInputV1,
    entry_arm: ProviderBinaryEntryArmV1,
    upstream: ProviderBinaryUpstreamV1,
    expected: ProviderBinaryExpectedV1,
}

impl ProviderBinaryFixtureV1 {
    /// Returns the stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> ProviderBinaryCaseIdV1 {
        self.case_id
    }

    /// Returns the immutable raw input.
    #[must_use]
    pub const fn input(&self) -> &ProviderBinaryInputV1 {
        &self.input
    }

    /// Returns which South entry point the case must be driven through.
    #[must_use]
    pub const fn entry_arm(&self) -> ProviderBinaryEntryArmV1 {
        self.entry_arm
    }

    /// Returns the canonical fake-upstream behavior.
    #[must_use]
    pub const fn upstream(&self) -> &ProviderBinaryUpstreamV1 {
        &self.upstream
    }

    /// Returns the exact expected outcome and evidence.
    #[must_use]
    pub const fn expected(&self) -> &ProviderBinaryExpectedV1 {
        &self.expected
    }
}

impl fmt::Debug for ProviderBinaryFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderBinaryFixtureV1")
            .field("case_id", &self.case_id)
            .field("entry_arm", &self.entry_arm)
            .field("input", &self.input)
            .field("upstream", &self.upstream)
            .field("expected", &self.expected)
            .finish()
    }
}

const PROVIDER_BINARY_PATH: &str = "path-debug-sentinel";
const PROVIDER_BINARY_REQUEST_BODY: &str = r#"{"value":"request-body-debug-sentinel"}"#;
const PROVIDER_BINARY_CONTENT_TYPE: &str = "content-type-debug-sentinel";
const PROVIDER_BINARY_JSON_CONTENT_TYPE: &str = "json-content-type-debug-sentinel";
const PROVIDER_BINARY_RETRY_AFTER: &str = "retry-after-debug-sentinel";

/// An audio payload that is not valid UTF-8, shaped like the answers this slice exists to carry.
///
/// The first two bytes are an MPEG-1 Layer III frame sync, which is also why the sequence is
/// unambiguously invalid: `0xFF` is never a legal UTF-8 start byte, and `0x80` later in the
/// sequence is a continuation byte with nothing leading it. No decoder, lenient or strict, can
/// read this as text.
const PROVIDER_BINARY_AUDIO_BODY: &[u8] = &[0xFF, 0xFB, 0x90, 0x64, 0x00, 0x80, 0xFE, 0xFF];

/// The JSON body a rejected exchange carries, held as bytes because that is how it arrives.
const PROVIDER_BINARY_REJECTION_BODY: &[u8] = br#"{"value":"rejection-body-debug-sentinel"}"#;

/// One byte past the UTF-8 cap: large enough to prove the binary arm does not reuse it, small
/// enough to stay within the binary cap.
const ABOVE_TEXT_CAP_BYTES: usize = MAX_RESPONSE_BODY_BYTES + 1;

/// One byte past the binary cap.
const ABOVE_BINARY_CAP_BYTES: usize = MAX_BINARY_RESPONSE_BODY_BYTES + 1;

/// The filler byte for both synthesized bodies. Not valid UTF-8 on its own, so neither cap row can
/// accidentally pass through a text path.
const SYNTHESIZED_FILL: u8 = 0x80;

const fn binary_input(requested_slot: &'static str) -> ProviderBinaryInputV1 {
    ProviderBinaryInputV1 {
        endpoint: ENDPOINT,
        bound_credential_slot: BOUND_SLOT,
        requested_credential_slot: requested_slot,
        relative_path: PROVIDER_BINARY_PATH,
        json_body: PROVIDER_BINARY_REQUEST_BODY,
        headers: HEADERS,
    }
}

/// Evidence for a case whose binary exchange completed and returned the upstream's bytes.
const RETURNED: ProviderBinaryExpectedEvidenceV1 = ProviderBinaryExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1::One,
    transport_calls: ProviderCallCountV1::One,
    wire_binary_response_observed: true,
    wire_body_bytes_exact: true,
};

/// Evidence for a case that reached the binary seam but produced no response: the seam claim is
/// `true` because it was used, the bytes claim is `false` because nothing came back to compare.
const REACHED_WITHOUT_BODY: ProviderBinaryExpectedEvidenceV1 = ProviderBinaryExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1::One,
    transport_calls: ProviderCallCountV1::One,
    wire_binary_response_observed: true,
    wire_body_bytes_exact: false,
};

/// Evidence for the UTF-8 row: a transport is reached, but not the binary seam, and no bytes come
/// back. Both presence claims are `false` while the transport count is one.
const UTF8_ARM_REACHED: ProviderBinaryExpectedEvidenceV1 = ProviderBinaryExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1::One,
    transport_calls: ProviderCallCountV1::One,
    wire_binary_response_observed: false,
    wire_body_bytes_exact: false,
};

/// Evidence for a case refused before any boundary: nothing was observed, so both presence claims
/// are `false`.
const NOT_REACHED: ProviderBinaryExpectedEvidenceV1 = ProviderBinaryExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1::Zero,
    transport_calls: ProviderCallCountV1::Zero,
    wire_binary_response_observed: false,
    wire_body_bytes_exact: false,
};

const PROVIDER_BINARY_FIXTURES: &[ProviderBinaryFixtureV1] = &[
    ProviderBinaryFixtureV1 {
        case_id: ProviderBinaryCaseIdV1::BinarySuccessNonUtf8Body,
        input: binary_input(BOUND_SLOT),
        entry_arm: ProviderBinaryEntryArmV1::Binary,
        upstream: ProviderBinaryUpstreamV1::Response(ProviderBinaryRawResponseV1 {
            status: 200,
            body: ProviderBinaryBodyV1::Literal(PROVIDER_BINARY_AUDIO_BODY),
            content_type: Some(PROVIDER_BINARY_CONTENT_TYPE),
            retry_after: None,
        }),
        expected: ProviderBinaryExpectedV1 {
            outcome: ProviderBinaryExpectedOutcomeV1::Response {
                status: 200,
                body: ProviderBinaryBodyV1::Literal(PROVIDER_BINARY_AUDIO_BODY),
                content_type: Some(PROVIDER_BINARY_CONTENT_TYPE),
                retry_after: None,
            },
            evidence: RETURNED,
        },
    },
    ProviderBinaryFixtureV1 {
        case_id: ProviderBinaryCaseIdV1::BinaryRejectionCarriesJsonBody,
        input: binary_input(BOUND_SLOT),
        entry_arm: ProviderBinaryEntryArmV1::Binary,
        // A 429 with `retry-after`: the metadata contracts are unchanged on this arm, and a
        // rejection is the exchange most likely to carry them.
        upstream: ProviderBinaryUpstreamV1::Response(ProviderBinaryRawResponseV1 {
            status: 429,
            body: ProviderBinaryBodyV1::Literal(PROVIDER_BINARY_REJECTION_BODY),
            content_type: Some(PROVIDER_BINARY_JSON_CONTENT_TYPE),
            retry_after: Some(PROVIDER_BINARY_RETRY_AFTER),
        }),
        expected: ProviderBinaryExpectedV1 {
            outcome: ProviderBinaryExpectedOutcomeV1::Response {
                status: 429,
                body: ProviderBinaryBodyV1::Literal(PROVIDER_BINARY_REJECTION_BODY),
                content_type: Some(PROVIDER_BINARY_JSON_CONTENT_TYPE),
                retry_after: Some(PROVIDER_BINARY_RETRY_AFTER),
            },
            evidence: RETURNED,
        },
    },
    ProviderBinaryFixtureV1 {
        case_id: ProviderBinaryCaseIdV1::BinaryBodyAboveTextCapSucceeds,
        input: binary_input(BOUND_SLOT),
        entry_arm: ProviderBinaryEntryArmV1::Binary,
        upstream: ProviderBinaryUpstreamV1::Response(ProviderBinaryRawResponseV1 {
            status: 200,
            body: ProviderBinaryBodyV1::Synthesized {
                fill: SYNTHESIZED_FILL,
                length: ABOVE_TEXT_CAP_BYTES,
            },
            content_type: Some(PROVIDER_BINARY_CONTENT_TYPE),
            retry_after: None,
        }),
        expected: ProviderBinaryExpectedV1 {
            outcome: ProviderBinaryExpectedOutcomeV1::Response {
                status: 200,
                body: ProviderBinaryBodyV1::Synthesized {
                    fill: SYNTHESIZED_FILL,
                    length: ABOVE_TEXT_CAP_BYTES,
                },
                content_type: Some(PROVIDER_BINARY_CONTENT_TYPE),
                retry_after: None,
            },
            evidence: RETURNED,
        },
    },
    ProviderBinaryFixtureV1 {
        case_id: ProviderBinaryCaseIdV1::BinaryBodyAboveBinaryCapRefused,
        input: binary_input(BOUND_SLOT),
        entry_arm: ProviderBinaryEntryArmV1::Binary,
        upstream: ProviderBinaryUpstreamV1::Response(ProviderBinaryRawResponseV1 {
            status: 200,
            body: ProviderBinaryBodyV1::Synthesized {
                fill: SYNTHESIZED_FILL,
                length: ABOVE_BINARY_CAP_BYTES,
            },
            content_type: Some(PROVIDER_BINARY_CONTENT_TYPE),
            retry_after: None,
        }),
        expected: ProviderBinaryExpectedV1 {
            outcome: ProviderBinaryExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::ResponseBodyTooLarge,
            },
            evidence: REACHED_WITHOUT_BODY,
        },
    },
    ProviderBinaryFixtureV1 {
        case_id: ProviderBinaryCaseIdV1::BinarySlotMismatch,
        input: binary_input(DIFFERENT_SLOT),
        entry_arm: ProviderBinaryEntryArmV1::Binary,
        upstream: ProviderBinaryUpstreamV1::NotReached,
        expected: ProviderBinaryExpectedV1 {
            outcome: ProviderBinaryExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::CredentialBindingMismatch,
            },
            evidence: NOT_REACHED,
        },
    },
    ProviderBinaryFixtureV1 {
        case_id: ProviderBinaryCaseIdV1::TextArmStillRefusesNonUtf8,
        input: binary_input(BOUND_SLOT),
        entry_arm: ProviderBinaryEntryArmV1::Utf8,
        // Byte for byte the first case's body. The two rows differing only in the entry arm is
        // what makes the pair a controlled experiment on the arm rather than on the payload.
        upstream: ProviderBinaryUpstreamV1::Response(ProviderBinaryRawResponseV1 {
            status: 200,
            body: ProviderBinaryBodyV1::Literal(PROVIDER_BINARY_AUDIO_BODY),
            content_type: Some(PROVIDER_BINARY_CONTENT_TYPE),
            retry_after: None,
        }),
        expected: ProviderBinaryExpectedV1 {
            outcome: ProviderBinaryExpectedOutcomeV1::Failure {
                code: ProviderCallFailureCodeV1::ResponseBodyNotUtf8,
            },
            evidence: UTF8_ARM_REACHED,
        },
    },
];

/// Returns the immutable canonical buffered-binary fixture table.
#[must_use]
pub const fn provider_binary_fixtures_v1() -> &'static [ProviderBinaryFixtureV1] {
    PROVIDER_BINARY_FIXTURES
}
