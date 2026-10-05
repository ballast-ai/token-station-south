//! Canonical cases for the request-signing host suite (gate ③ of B2,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §5.3 and §5.4).
//!
//! A `host_signed` package declares how it is signed: `signing.scheme` picks one of the host's
//! public-standard finalizers, `signing.service` and the endpoint parameter named by
//! `signing.region.template_param` set the scope, and `signing.credentials` maps each input the
//! scheme needs to a credential field. Gate ① checks the declaration is coherent; nothing in South
//! sees the host's finalizer run. This suite drives the host's own generic finalizer, selected by
//! declaration, and **verifies** what it emits rather than comparing bytes to one implementation:
//! it recomputes AWS Signature Version 4 from the request and the emitted `SignedHeaders` list and
//! checks the signature, the credential scope, the timestamp, the payload hash, the session token,
//! and that the emitted names stay within `emits`.
//!
//! The suite is host-implemented: a host provides [`RequestSigningHarnessV1`] around its finalizer
//! and runs [`run_request_signing_conformance_v1`]. Time is injected, so the host's finalizer
//! accepts an injected clock in test builds.

use std::{collections::BTreeMap, fmt};

use south_contracts::SignedHeaderV1;
use south_provider_api::{SigningSchemeV1, SigningV1, TemplateParamV1};

mod runner;
mod sigv4;

pub use runner::{
    RequestSigningConformanceFailureV1, RequestSigningConformanceReportV1, RequestSigningHarnessV1,
    RequestSigningInputV1, RequestSigningMismatchCategoryV1, RequestSigningMismatchV1,
    RequestSigningRefusedV1, run_request_signing_conformance_v1,
};

/// The request-signing conformance suite version.
pub const REQUEST_SIGNING_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The stable identifier for request-signing conformance version one.
pub const REQUEST_SIGNING_CONFORMANCE_SUITE_ID: &str = "south.request-signing.v1";

/// The finished request the host signs, exactly as its transport will send it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RequestSigningRequestV1 {
    method: &'static str,
    url: &'static str,
    headers: &'static [(&'static str, &'static str)],
    body: &'static [u8],
}

impl RequestSigningRequestV1 {
    /// The method.
    #[must_use]
    pub const fn method(&self) -> &'static str {
        self.method
    }

    /// The URL: the filled family endpoint and the path, which may carry a percent-encoded
    /// segment. It has no query.
    #[must_use]
    pub const fn url(&self) -> &'static str {
        self.url
    }

    /// The ordinary headers, lowercase. The host may sign them or not; `host` is derived from the
    /// URL and is not among them.
    #[must_use]
    pub const fn headers(&self) -> &'static [(&'static str, &'static str)] {
        self.headers
    }

    /// The exact body bytes.
    #[must_use]
    pub const fn body(&self) -> &'static [u8] {
        self.body
    }
}

impl fmt::Debug for RequestSigningRequestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestSigningRequestV1")
            .field("method", &self.method)
            .field("header_count", &self.headers.len())
            .field("body_byte_count", &self.body.len())
            .finish_non_exhaustive()
    }
}

/// The expected result of one case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RequestSigningExpectedV1 {
    /// The host signs, emitting exactly these headers, each once; the suite verifies them.
    Signed(&'static [SignedHeaderV1]),
    /// The host refuses to sign and emits nothing.
    Refused,
}

impl fmt::Debug for RequestSigningExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signed(headers) => formatter.debug_tuple("Signed").field(headers).finish(),
            Self::Refused => formatter.write_str("Refused"),
        }
    }
}

/// The closed set of canonical request-signing cases.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestSigningCaseIdV1 {
    /// A POST whose JSON body has insignificant whitespace and a non-ASCII character signs and
    /// verifies; the payload hash covers the exact body bytes.
    PostSignsTheExactBodyBytes,
    /// A session token is sent as `x-amz-security-token` and covered by `SignedHeaders`.
    SessionTokenIsSentAndSigned,
    /// A declaration that admits a session token, for a credential without one, emits exactly
    /// three headers.
    AbsentSessionTokenEmitsThreeHeaders,
    /// The service is the declaration's and the region is the configuration value of the
    /// declared template parameter (`sagemaker`, `eu-central-1`, parameter `aws_region`).
    ServiceAndRegionComeFromTheDeclaration,
    /// The credential is read through the declaration's input-to-field mapping, under field names
    /// that differ from the input names.
    CredentialFieldsComeFromTheMapping,
    /// A path segment already percent-encoded on the wire (an inference-profile ARN) is encoded
    /// again in the canonical URI.
    PathSegmentsAreEncodedTwice,
    /// A credential without its secret access key is refused, with nothing emitted.
    MissingSecretAccessKeyIsRefused,
}

fixed_debug!(RequestSigningCaseIdV1 {
    PostSignsTheExactBodyBytes => "PostSignsTheExactBodyBytes",
    SessionTokenIsSentAndSigned => "SessionTokenIsSentAndSigned",
    AbsentSessionTokenEmitsThreeHeaders => "AbsentSessionTokenEmitsThreeHeaders",
    ServiceAndRegionComeFromTheDeclaration => "ServiceAndRegionComeFromTheDeclaration",
    CredentialFieldsComeFromTheMapping => "CredentialFieldsComeFromTheMapping",
    PathSegmentsAreEncodedTwice => "PathSegmentsAreEncodedTwice",
    MissingSecretAccessKeyIsRefused => "MissingSecretAccessKeyIsRefused",
});

/// One immutable canonical request-signing case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RequestSigningFixtureV1 {
    case_id: RequestSigningCaseIdV1,
    service: &'static str,
    region_param: &'static str,
    credential_inputs: &'static [(&'static str, &'static str)],
    emits: &'static [SignedHeaderV1],
    config_values: &'static [(&'static str, &'static str)],
    credential_fields: &'static [(&'static str, &'static str)],
    now_unix_seconds: i64,
    request: RequestSigningRequestV1,
    expected: RequestSigningExpectedV1,
}

impl RequestSigningFixtureV1 {
    /// The stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> RequestSigningCaseIdV1 {
        self.case_id
    }

    /// The package's `signing` declaration, exactly as a manifest would carry it.
    #[must_use]
    pub fn signing(&self) -> SigningV1 {
        SigningV1 {
            scheme: SigningSchemeV1::AwsSigv4,
            service: self.service.to_owned(),
            region: TemplateParamV1 { template_param: self.region_param.to_owned() },
            credentials: pairs(self.credential_inputs),
        }
    }

    /// The package's `emits` declaration.
    #[must_use]
    pub const fn emits(&self) -> &'static [SignedHeaderV1] {
        self.emits
    }

    /// The family's configuration values, keyed by `config_schema` key.
    #[must_use]
    pub fn config_values(&self) -> BTreeMap<String, String> {
        pairs(self.config_values)
    }

    /// The credential's field values, keyed by field name.
    #[must_use]
    pub fn credential_fields(&self) -> BTreeMap<String, String> {
        pairs(self.credential_fields)
    }

    /// The signing time, in Unix seconds.
    #[must_use]
    pub const fn now_unix_seconds(&self) -> i64 {
        self.now_unix_seconds
    }

    /// The request to sign.
    #[must_use]
    pub const fn request(&self) -> &RequestSigningRequestV1 {
        &self.request
    }

    /// The expected result.
    #[must_use]
    pub const fn expected(&self) -> RequestSigningExpectedV1 {
        self.expected
    }
}

impl fmt::Debug for RequestSigningFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestSigningFixtureV1")
            .field("case_id", &self.case_id)
            .field("service", &self.service)
            .field("region_param", &self.region_param)
            .field("credential_inputs", &self.credential_inputs)
            .field("emits", &self.emits)
            .field("config_values", &self.config_values)
            .field(
                "credential_field_names",
                &self.credential_fields.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            )
            .field("now_unix_seconds", &self.now_unix_seconds)
            .field("request", &self.request)
            .field("expected", &self.expected)
            .finish()
    }
}

fn pairs(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

/// AWS's published example credentials from the `SigV4` test suite.
const ACCESS_KEY_ID: &str = "AKIDEXAMPLE";
const SECRET_ACCESS_KEY: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
const SESSION_TOKEN: &str = "south-test-only-session-token";

const STANDARD_INPUTS: &[(&str, &str)] =
    &[("access_key_id", "access_key_id"), ("secret_access_key", "secret_access_key")];
const TOKEN_INPUTS: &[(&str, &str)] = &[
    ("access_key_id", "access_key_id"),
    ("secret_access_key", "secret_access_key"),
    ("session_token", "session_token"),
];
const STANDARD_FIELDS: &[(&str, &str)] =
    &[("access_key_id", ACCESS_KEY_ID), ("secret_access_key", SECRET_ACCESS_KEY)];

const THREE: &[SignedHeaderV1] =
    &[SignedHeaderV1::Authorization, SignedHeaderV1::XAmzDate, SignedHeaderV1::XAmzContentSha256];
const FOUR: &[SignedHeaderV1] = &SignedHeaderV1::ALL;

const US_EAST_1: &[(&str, &str)] = &[("region", "us-east-1")];
const JSON: &[(&str, &str)] = &[("content-type", "application/json")];

/// A compact body whose members are already in sorted order, so a host that re-serializes it
/// before hashing still hashes the same bytes; only the first case's body tells the two apart.
const COMPACT_BODY: &[u8] = br#"{"messages":[{"content":[{"text":"hi"}],"role":"user"}]}"#;

const fn post(url: &'static str, body: &'static [u8]) -> RequestSigningRequestV1 {
    RequestSigningRequestV1 { method: "POST", url, headers: JSON, body }
}

const BEDROCK_CONVERSE: &str =
    "https://bedrock-runtime.us-east-1.amazonaws.com/model/example-model/converse";

const FIXTURES: &[RequestSigningFixtureV1] = &[
    RequestSigningFixtureV1 {
        case_id: RequestSigningCaseIdV1::PostSignsTheExactBodyBytes,
        service: "bedrock",
        region_param: "region",
        credential_inputs: STANDARD_INPUTS,
        emits: THREE,
        config_values: US_EAST_1,
        credential_fields: STANDARD_FIELDS,
        now_unix_seconds: 1_440_938_160,
        request: post(
            BEDROCK_CONVERSE,
            "{\n  \"messages\": [ { \"role\": \"user\", \"content\": [ { \"text\": \"h\u{e9}llo\" } ] } ]\n}"
                .as_bytes(),
        ),
        expected: RequestSigningExpectedV1::Signed(THREE),
    },
    RequestSigningFixtureV1 {
        case_id: RequestSigningCaseIdV1::SessionTokenIsSentAndSigned,
        service: "bedrock",
        region_param: "region",
        credential_inputs: TOKEN_INPUTS,
        emits: FOUR,
        config_values: US_EAST_1,
        credential_fields: &[
            ("access_key_id", ACCESS_KEY_ID),
            ("secret_access_key", SECRET_ACCESS_KEY),
            ("session_token", SESSION_TOKEN),
        ],
        now_unix_seconds: 1_759_665_723,
        request: post(BEDROCK_CONVERSE, COMPACT_BODY),
        expected: RequestSigningExpectedV1::Signed(FOUR),
    },
    RequestSigningFixtureV1 {
        case_id: RequestSigningCaseIdV1::AbsentSessionTokenEmitsThreeHeaders,
        service: "bedrock",
        region_param: "region",
        credential_inputs: TOKEN_INPUTS,
        emits: FOUR,
        config_values: US_EAST_1,
        credential_fields: STANDARD_FIELDS,
        now_unix_seconds: 1_759_665_723,
        request: post(BEDROCK_CONVERSE, COMPACT_BODY),
        expected: RequestSigningExpectedV1::Signed(THREE),
    },
    RequestSigningFixtureV1 {
        case_id: RequestSigningCaseIdV1::ServiceAndRegionComeFromTheDeclaration,
        service: "sagemaker",
        region_param: "aws_region",
        credential_inputs: STANDARD_INPUTS,
        emits: THREE,
        config_values: &[("aws_region", "eu-central-1")],
        credential_fields: STANDARD_FIELDS,
        now_unix_seconds: 1_759_665_723,
        request: post(
            "https://runtime.sagemaker.eu-central-1.amazonaws.com/endpoints/example-endpoint/invocations",
            COMPACT_BODY,
        ),
        expected: RequestSigningExpectedV1::Signed(THREE),
    },
    RequestSigningFixtureV1 {
        case_id: RequestSigningCaseIdV1::CredentialFieldsComeFromTheMapping,
        service: "bedrock",
        region_param: "region",
        credential_inputs: &[("access_key_id", "aws_key"), ("secret_access_key", "aws_secret")],
        emits: THREE,
        config_values: US_EAST_1,
        credential_fields: &[
            ("aws_key", "AKIDSOUTHTESTONLY"),
            ("aws_secret", "south-test-only-secret-access-key"),
        ],
        now_unix_seconds: 1_759_665_723,
        request: post(BEDROCK_CONVERSE, COMPACT_BODY),
        expected: RequestSigningExpectedV1::Signed(THREE),
    },
    RequestSigningFixtureV1 {
        case_id: RequestSigningCaseIdV1::PathSegmentsAreEncodedTwice,
        service: "bedrock",
        region_param: "region",
        credential_inputs: STANDARD_INPUTS,
        emits: THREE,
        config_values: US_EAST_1,
        credential_fields: STANDARD_FIELDS,
        now_unix_seconds: 1_759_665_723,
        request: post(
            concat!(
                "https://bedrock-runtime.us-east-1.amazonaws.com/model/",
                "arn%3Aaws%3Abedrock%3Aus-east-1%3A123456789012%3Ainference-profile%2F",
                "us.anthropic.claude-3-5-sonnet-20241022-v2%3A0/converse",
            ),
            COMPACT_BODY,
        ),
        expected: RequestSigningExpectedV1::Signed(THREE),
    },
    RequestSigningFixtureV1 {
        case_id: RequestSigningCaseIdV1::MissingSecretAccessKeyIsRefused,
        service: "bedrock",
        region_param: "region",
        credential_inputs: STANDARD_INPUTS,
        emits: THREE,
        config_values: US_EAST_1,
        credential_fields: &[("access_key_id", ACCESS_KEY_ID)],
        now_unix_seconds: 1_759_665_723,
        request: post(BEDROCK_CONVERSE, COMPACT_BODY),
        expected: RequestSigningExpectedV1::Refused,
    },
];

/// Returns the immutable canonical request-signing case table.
#[must_use]
pub const fn request_signing_fixtures_v1() -> &'static [RequestSigningFixtureV1] {
    FIXTURES
}
