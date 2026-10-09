//! Canonical fixtures for the media-binary conformance suite.
//!
//! HTTP contract version twelve adds two shapes for the media worlds in one bump (image record
//! §6.3a and §18.1, speech record D3a): a multipart POST whose answer is read as bytes, and a text
//! POST — an SSML document under a media type the contract renders — that is only ever read as
//! bytes. Both are new *pairings* rather than new wire behaviour on an existing pairing, so they
//! get their own table instead of rows in `south.provider-multipart.v1` or
//! `south.provider-binary.v1`: those suites' case counts are what two hosts' `verified` evidence
//! names, and growing them would silently demote that evidence (image record §19, S-I-2).
//!
//! The six cases prove what the image edit and SSML speech call sites rely on: on the multipart
//! arm, the encoded bytes still reach the wire under the rendered boundary media type and the
//! answer — success or rejection — comes back byte for byte; on the text arm, the document reaches
//! the wire unmodified under `application/ssml+xml` and nothing else, under each credential arm
//! the speech surface uses, the binding check still runs before any boundary, and a `content-type`
//! smuggled through the ordinary header channel is refused while the request is still a value.
//!
//! Which entry point a row drives is fixed by its body: a multipart body goes through
//! `execute_multipart_binary_call_v1`, a text body through `execute_text_binary_call_v1`. Neither
//! shape has a UTF-8 or streaming counterpart to confuse it with, so the table carries no separate
//! entry-arm column.

use std::fmt;

use south_contracts::SecretHeaderV1;

use crate::{
    BOUND_SLOT, DIFFERENT_SLOT, ENDPOINT, HEADERS, ProviderCallCountV1, ProviderCallFailureCodeV1,
};

/// The media-binary conformance suite version.
pub const PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The stable identifier for media-binary conformance version one.
pub const PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_ID: &str = "south.provider-media-binary.v1";

/// The closed set of canonical media-binary cases.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderMediaBinaryCaseIdV1 {
    /// A multipart POST whose 2xx answer is not valid UTF-8, delivered byte for byte.
    ///
    /// The case the multipart twin exists for. On HTTP contract version eleven a multipart POST
    /// could only be read as UTF-8, so an image edit's answer above the UTF-8 cap — or any answer
    /// that is not text — was refused after the upstream had already succeeded and charged.
    MultipartBinarySuccess,
    /// A multipart POST whose non-2xx answer is JSON, returned as bytes with the upstream status
    /// and its `retry-after`.
    ///
    /// The binary-response record's D3 on the new pairing: bytes come back on every status, and
    /// the host decodes the rejection itself. An implementation that turned a non-2xx into a
    /// transport error, or dropped the metadata on this path, fails here.
    MultipartBinaryRejectionCarriesBody,
    /// A text POST under the Bearer arm whose 2xx answer is audio, delivered byte for byte.
    TextBinaryBearerSuccess,
    /// The same text POST authenticated through `ocp-apim-subscription-key`, no `authorization`.
    ///
    /// The arm the adopting host's Azure Speech synthesis path uses. Identical to the row above
    /// except for the credential arm, which is what makes the pair a controlled experiment on it.
    TextBinaryHeaderSecretSuccess,
    /// A text POST whose valid requested slot differs from the binding, refused before resolver
    /// and transport.
    ///
    /// The structural row every suite carries, here because the text shape brings a new
    /// projection into the shared flow: the binding check must reach it like every other shape.
    TextBinarySlotMismatch,
    /// A `content-type` smuggled through the ordinary header channel of a text POST, refused at
    /// construction.
    ///
    /// The smuggled value is the *correct* media type, so the case cannot be passed by the value
    /// happening to disagree. An adapter that silently drops the header instead of refusing the
    /// request passes every other row in this table and fails this one.
    TextContentTypeSmuggled,
}

fixed_debug!(ProviderMediaBinaryCaseIdV1 {
    MultipartBinarySuccess => "MultipartBinarySuccess",
    MultipartBinaryRejectionCarriesBody => "MultipartBinaryRejectionCarriesBody",
    TextBinaryBearerSuccess => "TextBinaryBearerSuccess",
    TextBinaryHeaderSecretSuccess => "TextBinaryHeaderSecretSuccess",
    TextBinarySlotMismatch => "TextBinarySlotMismatch",
    TextContentTypeSmuggled => "TextContentTypeSmuggled",
});

/// The credential arm a canonical media-binary request declares.
///
/// Closed to the two arms the media call sites use, for the reason the multipart suite gives: the
/// combined and host-signed arms bind credentials through exactly the same code on every request
/// shape, and neither new entry point has a signed twin.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderMediaBinaryAuthArmV1 {
    /// `Authorization: Bearer …`.
    Bearer,
    /// The secret verbatim in one sanctioned header, no `authorization`.
    HeaderSecret(SecretHeaderV1),
}

impl fmt::Debug for ProviderMediaBinaryAuthArmV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer => formatter.write_str("Bearer"),
            Self::HeaderSecret(header) => {
                formatter.debug_tuple("HeaderSecret").field(header).finish()
            }
        }
    }
}

/// The raw request body of one canonical case, which also selects the entry point it drives.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderMediaBinaryRequestBodyV1 {
    /// Already-encoded multipart bytes and the boundary that delimits them, driven through
    /// `execute_multipart_binary_call_v1`.
    Multipart {
        /// The raw body bytes.
        bytes: &'static [u8],
        /// The raw boundary, without its `--` prefix.
        boundary: &'static str,
    },
    /// A UTF-8 document and the raw media type it declares, driven through
    /// `execute_text_binary_call_v1`.
    Text {
        /// The raw document.
        text: &'static str,
        /// The raw media type, parsed through the contract's closed set.
        media_type: &'static str,
    },
}

impl ProviderMediaBinaryRequestBodyV1 {
    /// Returns the exact bytes a correct implementation puts on the wire.
    #[must_use]
    pub const fn bytes(&self) -> &'static [u8] {
        match self {
            Self::Multipart { bytes, .. } => bytes,
            Self::Text { text, .. } => text.as_bytes(),
        }
    }

    /// Returns the media type a correct implementation renders for this body.
    ///
    /// Built here from the raw input by the contract's rule — the boundary parameter for
    /// multipart, the media type verbatim for text — so an executor comparing against it is
    /// comparing against the rule rather than against South's own answer.
    #[must_use]
    pub fn expected_content_type(&self) -> String {
        match self {
            Self::Multipart { boundary, .. } => format!("multipart/form-data; boundary={boundary}"),
            Self::Text { media_type, .. } => (*media_type).to_owned(),
        }
    }
}

impl fmt::Debug for ProviderMediaBinaryRequestBodyV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Multipart { bytes, boundary } => formatter
                .debug_struct("Multipart")
                .field("byte_count", &bytes.len())
                .field("boundary_byte_count", &boundary.len())
                .finish(),
            Self::Text { text, media_type } => formatter
                .debug_struct("Text")
                .field("byte_count", &text.len())
                .field("media_type_byte_count", &media_type.len())
                .finish(),
        }
    }
}

/// Raw media-binary input retained exactly as static test data.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderMediaBinaryInputV1 {
    endpoint: &'static str,
    bound_credential_slot: &'static str,
    requested_credential_slot: &'static str,
    relative_path: &'static str,
    headers: &'static [(&'static str, &'static str)],
    body: ProviderMediaBinaryRequestBodyV1,
}

impl ProviderMediaBinaryInputV1 {
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

    /// Returns the raw request body, which also selects the entry point.
    #[must_use]
    pub const fn body(&self) -> &ProviderMediaBinaryRequestBodyV1 {
        &self.body
    }

    /// Returns the media type a correct implementation renders for this input.
    #[must_use]
    pub fn expected_content_type(&self) -> String {
        self.body.expected_content_type()
    }
}

impl fmt::Debug for ProviderMediaBinaryInputV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMediaBinaryInputV1")
            .field("endpoint_byte_count", &self.endpoint.len())
            .field("bound_credential_slot_byte_count", &self.bound_credential_slot.len())
            .field("requested_credential_slot_byte_count", &self.requested_credential_slot.len())
            .field("relative_path_byte_count", &self.relative_path.len())
            .field("header_count", &self.headers.len())
            .field("body", &self.body)
            .finish()
    }
}

/// A borrowed raw upstream response whose body is bytes rather than text.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderMediaBinaryRawResponseV1 {
    status: u16,
    body: &'static [u8],
    content_type: Option<&'static str>,
    retry_after: Option<&'static str>,
}

impl ProviderMediaBinaryRawResponseV1 {
    /// Returns the raw HTTP status.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Returns the raw response body.
    #[must_use]
    pub const fn body(&self) -> &'static [u8] {
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

impl fmt::Debug for ProviderMediaBinaryRawResponseV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMediaBinaryRawResponseV1")
            .field("status", &self.status)
            .field("body_byte_count", &self.body.len())
            .field("has_content_type", &self.content_type.is_some())
            .field("has_retry_after", &self.retry_after.is_some())
            .finish()
    }
}

/// A raw upstream response or fake-transport behavior for a canonical media-binary case.
///
/// Buffered only: neither shape has a streaming twin.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderMediaBinaryUpstreamV1 {
    /// Complete one buffered exchange with this raw response.
    Response(ProviderMediaBinaryRawResponseV1),
    /// The transport boundary must not be reached.
    NotReached,
}

impl fmt::Debug for ProviderMediaBinaryUpstreamV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(raw) => formatter.debug_tuple("Response").field(raw).finish(),
            Self::NotReached => formatter.write_str("NotReached"),
        }
    }
}

/// The exact expected terminal shape of one canonical media-binary case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderMediaBinaryExpectedOutcomeV1 {
    /// A bounded buffered binary response matched field by field.
    Response {
        /// Expected status.
        status: u16,
        /// Expected body bytes.
        body: &'static [u8],
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

impl fmt::Debug for ProviderMediaBinaryExpectedOutcomeV1 {
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
/// All three wire booleans are **presence claims**: `false` until a transport call observed the
/// fact, so the two rows that never reach the transport expect `false` for all three, and an
/// adapter whose probe answers without reading the prepared request fails those rows.
///
/// - `wire_content_type_exact` — the media type the transport emits is exactly the one rebuilt
///   from the raw input by the contract's rule.
/// - `wire_request_body_exact` — the bytes the transport sends are exactly the raw body.
/// - `wire_binary_response_observed` — the bytes-reading transport seam carried the exchange.
///   This is the claim that separates the new entry points from the UTF-8 multipart one: an
///   adapter that routed the multipart rows through `execute_multipart_call_v1` and re-encoded the
///   answer would match every outcome and fail here.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderMediaBinaryExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    wire_content_type_exact: bool,
    wire_request_body_exact: bool,
    wire_binary_response_observed: bool,
}

impl ProviderMediaBinaryExpectedEvidenceV1 {
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
    pub const fn wire_request_body_exact(&self) -> bool {
        self.wire_request_body_exact
    }

    /// Returns whether the bytes-reading transport seam must have carried the exchange.
    #[must_use]
    pub const fn wire_binary_response_observed(&self) -> bool {
        self.wire_binary_response_observed
    }
}

impl fmt::Debug for ProviderMediaBinaryExpectedEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMediaBinaryExpectedEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field("wire_content_type_exact", &self.wire_content_type_exact)
            .field("wire_request_body_exact", &self.wire_request_body_exact)
            .field("wire_binary_response_observed", &self.wire_binary_response_observed)
            .finish()
    }
}

/// The expected outcome and boundary evidence for one media-binary fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderMediaBinaryExpectedV1 {
    outcome: ProviderMediaBinaryExpectedOutcomeV1,
    evidence: ProviderMediaBinaryExpectedEvidenceV1,
}

impl ProviderMediaBinaryExpectedV1 {
    /// Returns the expected terminal shape.
    #[must_use]
    pub const fn outcome(&self) -> &ProviderMediaBinaryExpectedOutcomeV1 {
        &self.outcome
    }

    /// Returns the expected boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderMediaBinaryExpectedEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ProviderMediaBinaryExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMediaBinaryExpectedV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// One immutable canonical media-binary fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderMediaBinaryFixtureV1 {
    case_id: ProviderMediaBinaryCaseIdV1,
    input: ProviderMediaBinaryInputV1,
    auth_arm: ProviderMediaBinaryAuthArmV1,
    upstream: ProviderMediaBinaryUpstreamV1,
    expected: ProviderMediaBinaryExpectedV1,
}

impl ProviderMediaBinaryFixtureV1 {
    /// Returns the stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> ProviderMediaBinaryCaseIdV1 {
        self.case_id
    }

    /// Returns the immutable raw input.
    #[must_use]
    pub const fn input(&self) -> &ProviderMediaBinaryInputV1 {
        &self.input
    }

    /// Returns the credential arm the request declares.
    #[must_use]
    pub const fn auth_arm(&self) -> ProviderMediaBinaryAuthArmV1 {
        self.auth_arm
    }

    /// Returns the canonical fake-upstream behavior.
    #[must_use]
    pub const fn upstream(&self) -> &ProviderMediaBinaryUpstreamV1 {
        &self.upstream
    }

    /// Returns the exact expected outcome and evidence.
    #[must_use]
    pub const fn expected(&self) -> &ProviderMediaBinaryExpectedV1 {
        &self.expected
    }
}

impl fmt::Debug for ProviderMediaBinaryFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMediaBinaryFixtureV1")
            .field("case_id", &self.case_id)
            .field("auth_arm", &self.auth_arm)
            .field("input", &self.input)
            .field("upstream", &self.upstream)
            .field("expected", &self.expected)
            .finish()
    }
}

const MEDIA_BINARY_PATH: &str = "path-debug-sentinel";
const MEDIA_BINARY_CONTENT_TYPE: &str = "content-type-debug-sentinel";
const MEDIA_BINARY_JSON_CONTENT_TYPE: &str = "json-content-type-debug-sentinel";
const MEDIA_BINARY_RETRY_AFTER: &str = "retry-after-debug-sentinel";

/// The declared multipart boundary.
const MEDIA_BINARY_BOUNDARY: &str = "boundary-debug-sentinel";

/// A minimal well-formed multipart body: one text part, opened and closed by
/// [`MEDIA_BINARY_BOUNDARY`]. Shaped like the field an image edit splices.
const MEDIA_BINARY_MULTIPART_BODY: &[u8] = b"--boundary-debug-sentinel\r\n\
Content-Disposition: form-data; name=\"model\"\r\n\
\r\n\
body-value-debug-sentinel\r\n\
--boundary-debug-sentinel--\r\n";

/// The one media type the closed text set admits, spelled as the contract renders it.
const MEDIA_BINARY_SSML_MEDIA_TYPE: &str = "application/ssml+xml";

/// A well-formed SSML document carrying both characters a component must have escaped, so the
/// row also pins that the transport sends the escaped text as given rather than re-escaping or
/// decoding it.
const MEDIA_BINARY_SSML: &str = "<speak version='1.0' xml:lang='en-US'>\
<voice name='voice-debug-sentinel'>ssml-text-debug-sentinel &amp; &lt;more&gt;</voice></speak>";

/// An answer that is not valid UTF-8: an MPEG-1 Layer III frame sync followed by bytes no decoder
/// accepts. Stands for both the synthesised audio and an image edit's bytes.
const MEDIA_BINARY_AUDIO_BODY: &[u8] = &[0xFF, 0xFB, 0x90, 0x64, 0x00, 0x80, 0xFE, 0xFF];

/// The JSON body a rejected exchange carries, held as bytes because that is how it arrives.
const MEDIA_BINARY_REJECTION_BODY: &[u8] = br#"{"value":"rejection-body-debug-sentinel"}"#;

/// Ordinary headers carrying the correct `content-type`, which the text shape must refuse.
const MEDIA_BINARY_SMUGGLED_HEADERS: &[(&str, &str)] = &[
    ("header-name-debug-sentinel", "header-value-debug-sentinel"),
    ("content-type", MEDIA_BINARY_SSML_MEDIA_TYPE),
];

const MULTIPART_BODY: ProviderMediaBinaryRequestBodyV1 =
    ProviderMediaBinaryRequestBodyV1::Multipart {
        bytes: MEDIA_BINARY_MULTIPART_BODY,
        boundary: MEDIA_BINARY_BOUNDARY,
    };

const TEXT_BODY: ProviderMediaBinaryRequestBodyV1 = ProviderMediaBinaryRequestBodyV1::Text {
    text: MEDIA_BINARY_SSML,
    media_type: MEDIA_BINARY_SSML_MEDIA_TYPE,
};

const fn media_input(
    requested_slot: &'static str,
    headers: &'static [(&'static str, &'static str)],
    body: ProviderMediaBinaryRequestBodyV1,
) -> ProviderMediaBinaryInputV1 {
    ProviderMediaBinaryInputV1 {
        endpoint: ENDPOINT,
        bound_credential_slot: BOUND_SLOT,
        requested_credential_slot: requested_slot,
        relative_path: MEDIA_BINARY_PATH,
        headers,
        body,
    }
}

/// Evidence for a case that reached the bytes-reading seam once and was measured there.
const RETURNED: ProviderMediaBinaryExpectedEvidenceV1 = ProviderMediaBinaryExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1::One,
    transport_calls: ProviderCallCountV1::One,
    wire_content_type_exact: true,
    wire_request_body_exact: true,
    wire_binary_response_observed: true,
};

/// Evidence for a case refused before any boundary: nothing was observed, so every presence claim
/// is `false`.
const NOT_REACHED: ProviderMediaBinaryExpectedEvidenceV1 = ProviderMediaBinaryExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1::Zero,
    transport_calls: ProviderCallCountV1::Zero,
    wire_content_type_exact: false,
    wire_request_body_exact: false,
    wire_binary_response_observed: false,
};

const fn audio_response() -> ProviderMediaBinaryRawResponseV1 {
    ProviderMediaBinaryRawResponseV1 {
        status: 200,
        body: MEDIA_BINARY_AUDIO_BODY,
        content_type: Some(MEDIA_BINARY_CONTENT_TYPE),
        retry_after: None,
    }
}

const fn returned(raw: ProviderMediaBinaryRawResponseV1) -> ProviderMediaBinaryExpectedV1 {
    ProviderMediaBinaryExpectedV1 {
        outcome: ProviderMediaBinaryExpectedOutcomeV1::Response {
            status: raw.status,
            body: raw.body,
            content_type: raw.content_type,
            retry_after: raw.retry_after,
        },
        evidence: RETURNED,
    }
}

const fn refused(code: ProviderCallFailureCodeV1) -> ProviderMediaBinaryExpectedV1 {
    ProviderMediaBinaryExpectedV1 {
        outcome: ProviderMediaBinaryExpectedOutcomeV1::Failure { code },
        evidence: NOT_REACHED,
    }
}

/// A 429 carrying JSON and `retry-after`: the metadata contracts are unchanged on this pairing,
/// and a rejection is the exchange most likely to carry them.
const REJECTION: ProviderMediaBinaryRawResponseV1 = ProviderMediaBinaryRawResponseV1 {
    status: 429,
    body: MEDIA_BINARY_REJECTION_BODY,
    content_type: Some(MEDIA_BINARY_JSON_CONTENT_TYPE),
    retry_after: Some(MEDIA_BINARY_RETRY_AFTER),
};

const PROVIDER_MEDIA_BINARY_FIXTURES: &[ProviderMediaBinaryFixtureV1] = &[
    ProviderMediaBinaryFixtureV1 {
        case_id: ProviderMediaBinaryCaseIdV1::MultipartBinarySuccess,
        input: media_input(BOUND_SLOT, HEADERS, MULTIPART_BODY),
        auth_arm: ProviderMediaBinaryAuthArmV1::Bearer,
        upstream: ProviderMediaBinaryUpstreamV1::Response(audio_response()),
        expected: returned(audio_response()),
    },
    ProviderMediaBinaryFixtureV1 {
        case_id: ProviderMediaBinaryCaseIdV1::MultipartBinaryRejectionCarriesBody,
        input: media_input(BOUND_SLOT, HEADERS, MULTIPART_BODY),
        // The arm the adopting host's Azure image-edit path uses.
        auth_arm: ProviderMediaBinaryAuthArmV1::HeaderSecret(SecretHeaderV1::ApiKey),
        upstream: ProviderMediaBinaryUpstreamV1::Response(REJECTION),
        expected: returned(REJECTION),
    },
    ProviderMediaBinaryFixtureV1 {
        case_id: ProviderMediaBinaryCaseIdV1::TextBinaryBearerSuccess,
        input: media_input(BOUND_SLOT, HEADERS, TEXT_BODY),
        auth_arm: ProviderMediaBinaryAuthArmV1::Bearer,
        upstream: ProviderMediaBinaryUpstreamV1::Response(audio_response()),
        expected: returned(audio_response()),
    },
    ProviderMediaBinaryFixtureV1 {
        case_id: ProviderMediaBinaryCaseIdV1::TextBinaryHeaderSecretSuccess,
        input: media_input(BOUND_SLOT, HEADERS, TEXT_BODY),
        auth_arm: ProviderMediaBinaryAuthArmV1::HeaderSecret(
            SecretHeaderV1::OcpApimSubscriptionKey,
        ),
        upstream: ProviderMediaBinaryUpstreamV1::Response(audio_response()),
        expected: returned(audio_response()),
    },
    ProviderMediaBinaryFixtureV1 {
        case_id: ProviderMediaBinaryCaseIdV1::TextBinarySlotMismatch,
        input: media_input(DIFFERENT_SLOT, HEADERS, TEXT_BODY),
        auth_arm: ProviderMediaBinaryAuthArmV1::Bearer,
        upstream: ProviderMediaBinaryUpstreamV1::NotReached,
        expected: refused(ProviderCallFailureCodeV1::CredentialBindingMismatch),
    },
    ProviderMediaBinaryFixtureV1 {
        case_id: ProviderMediaBinaryCaseIdV1::TextContentTypeSmuggled,
        input: media_input(BOUND_SLOT, MEDIA_BINARY_SMUGGLED_HEADERS, TEXT_BODY),
        auth_arm: ProviderMediaBinaryAuthArmV1::Bearer,
        upstream: ProviderMediaBinaryUpstreamV1::NotReached,
        // The frozen nineteen-code set has no shape-specific code; a request refused at
        // construction is a preparation-time, zero-call declaration failure, which folds into
        // `INVALID_RELATIVE_PATH` exactly as the multipart suite's smuggling row does.
        expected: refused(ProviderCallFailureCodeV1::InvalidRelativePath),
    },
];

/// Returns the immutable canonical media-binary fixture table.
#[must_use]
pub const fn provider_media_binary_fixtures_v1() -> &'static [ProviderMediaBinaryFixtureV1] {
    PROVIDER_MEDIA_BINARY_FIXTURES
}
