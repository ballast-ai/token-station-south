#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Provider contract fixtures and conformance boundary.

use std::{fmt, time::Duration};

use south_contracts::TransportErrorV1;

macro_rules! fixed_debug {
    ($type:ty { $($variant:ident => $name:literal),+ $(,)? }) => {
        impl fmt::Debug for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    $(Self::$variant => formatter.write_str($name),)+
                }
            }
        }
    };
}

mod controlled_query;
mod controlled_user_agent;
mod header_auth;
mod quota;
mod stream;

pub use controlled_query::{
    CONTROLLED_QUERY_CONFORMANCE_SUITE_ID, CONTROLLED_QUERY_CONFORMANCE_SUITE_VERSION,
    ControlledQueryCaseIdV1, ControlledQueryExpectedEvidenceV1, ControlledQueryExpectedOutcomeV1,
    ControlledQueryExpectedV1, ControlledQueryFixtureV1, ControlledQueryUpstreamV1,
    controlled_query_fixtures_v1,
};
pub use controlled_user_agent::{
    CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_ID, CONTROLLED_USER_AGENT_CONFORMANCE_SUITE_VERSION,
    ControlledUserAgentCaseIdV1, ControlledUserAgentExpectedEvidenceV1,
    ControlledUserAgentExpectedOutcomeV1, ControlledUserAgentExpectedV1,
    ControlledUserAgentFixtureV1, ControlledUserAgentUpstreamV1, controlled_user_agent_fixtures_v1,
};
pub use header_auth::{
    FAKE_HEADER_SECRET_V1, HEADER_AUTH_CONFORMANCE_SUITE_ID, HEADER_AUTH_CONFORMANCE_SUITE_VERSION,
    HeaderAuthCaseIdV1, HeaderAuthExpectedEvidenceV1, HeaderAuthExpectedOutcomeV1,
    HeaderAuthExpectedV1, HeaderAuthFixtureV1, HeaderAuthUpstreamV1, header_auth_fixtures_v1,
};
pub use quota::{
    PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_ID,
    PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_VERSION, ProviderQuotaMetadataCaseIdV1,
    ProviderQuotaMetadataExpectedEvidenceV1, ProviderQuotaMetadataExpectedOutcomeV1,
    ProviderQuotaMetadataFixtureV1, ProviderQuotaMetadataRawV1, ProviderQuotaMetadataUpstreamV1,
    provider_quota_metadata_fixtures_v1,
};

pub use stream::{
    PROVIDER_STREAM_CONFORMANCE_DEADLINE_OFFSET_V1, PROVIDER_STREAM_CONFORMANCE_IDLE_TIMEOUT_V1,
    PROVIDER_STREAM_CONFORMANCE_SUITE_ID, PROVIDER_STREAM_CONFORMANCE_SUITE_VERSION,
    ProviderStreamCaseIdV1, ProviderStreamControlV1, ProviderStreamExpectedEvidenceV1,
    ProviderStreamExpectedOutcomeV1, ProviderStreamExpectedV1, ProviderStreamFixtureV1,
    ProviderStreamRawHeadV1, ProviderStreamRawRejectionV1, ProviderStreamRawStreamV1,
    ProviderStreamTerminalV1, ProviderStreamUpstreamV1, provider_stream_fixtures_v1,
};

/// The provider-call conformance suite version.
pub const PROVIDER_CALL_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The stable identifier for provider-call conformance version one.
pub const PROVIDER_CALL_CONFORMANCE_SUITE_ID: &str = "south.provider-call.v1";

/// The absolute-deadline offset used by the deadline fixture.
pub const PROVIDER_CALL_CONFORMANCE_DEADLINE_OFFSET_V1: Duration = Duration::from_secs(1);

/// Synthetic test-only Bearer material used by the reference executor.
pub const FAKE_BEARER_SECRET_V1: &str = "south-test-only-fake-bearer-v1";

/// The closed set of canonical provider-call cases.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderCallCaseIdV1 {
    /// One successful buffered response.
    Success,
    /// Raw input whose relative path must fail contract parsing.
    InvalidRelativePath,
    /// A valid requested credential slot that differs from the binding.
    CredentialSlotMismatch,
    /// A redirect rejected by transport policy.
    RedirectDenied,
    /// A response rejected at the response body limit.
    ResponseBodyTooLarge,
    /// Cancellation while credential resolution is pending.
    Cancelled,
    /// Deadline expiry while transport is pending.
    DeadlineExceeded,
}

fixed_debug!(ProviderCallCaseIdV1 {
    Success => "Success",
    InvalidRelativePath => "InvalidRelativePath",
    CredentialSlotMismatch => "CredentialSlotMismatch",
    RedirectDenied => "RedirectDenied",
    ResponseBodyTooLarge => "ResponseBodyTooLarge",
    Cancelled => "Cancelled",
    DeadlineExceeded => "DeadlineExceeded",
});

/// How the assembled executor must drive one canonical case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderCallControlV1 {
    /// Let the call complete without external interruption.
    Complete,
    /// Cancel after proving the resolver future is pending.
    CancelWhileResolverPending,
    /// Let the caller advance the injected deadline after transport starts.
    ExpireWhileTransportPending,
}

fixed_debug!(ProviderCallControlV1 {
    Complete => "Complete",
    CancelWhileResolverPending => "CancelWhileResolverPending",
    ExpireWhileTransportPending => "ExpireWhileTransportPending",
});

/// A raw response or fake-transport behavior for a canonical case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderCallUpstreamV1 {
    /// Return this raw response through the bounded production response contract.
    Response(ProviderCallRawResponseV1),
    /// Return this closed transport failure.
    TransportFailure(TransportErrorV1),
    /// Remain pending until the caller-owned absolute deadline expires.
    Pending,
    /// The transport boundary must not be reached.
    NotReached,
}

impl fmt::Debug for ProviderCallUpstreamV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(response) => formatter.debug_tuple("Response").field(response).finish(),
            Self::TransportFailure(error) => formatter
                .debug_tuple("TransportFailure")
                .field(&TransportCodeDebug(error.code()))
                .finish(),
            Self::Pending => formatter.write_str("Pending"),
            Self::NotReached => formatter.write_str("NotReached"),
        }
    }
}

/// A borrowed, allocation-free raw upstream response fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderCallRawResponseV1 {
    status: u16,
    body: &'static str,
    content_type: Option<&'static str>,
    retry_after: Option<&'static str>,
}

impl ProviderCallRawResponseV1 {
    /// Returns the raw HTTP status.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Returns the raw UTF-8 response body.
    #[must_use]
    pub const fn body(&self) -> &'static str {
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

impl fmt::Debug for ProviderCallRawResponseV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderCallRawResponseV1")
            .field("status", &self.status)
            .field("body_byte_count", &self.body.len())
            .field("content_type", &MetadataSummary(self.content_type))
            .field("retry_after", &MetadataSummary(self.retry_after))
            .finish()
    }
}

/// Raw provider-call input retained exactly as static test data.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderCallInputV1 {
    endpoint: &'static str,
    bound_credential_slot: &'static str,
    requested_credential_slot: &'static str,
    relative_path: &'static str,
    json_body: &'static str,
    headers: &'static [(&'static str, &'static str)],
}

impl ProviderCallInputV1 {
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

    /// Returns the raw JSON body.
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

impl fmt::Debug for ProviderCallInputV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderCallInputV1")
            .field("endpoint_byte_count", &self.endpoint.len())
            .field("bound_credential_slot_byte_count", &self.bound_credential_slot.len())
            .field("requested_credential_slot_byte_count", &self.requested_credential_slot.len())
            .field("relative_path_byte_count", &self.relative_path.len())
            .field("json_body_byte_count", &self.json_body.len())
            .field("header_count", &self.headers.len())
            .finish()
    }
}

/// A closed stable provider-call failure code.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderCallFailureCodeV1 {
    /// `INVALID_ENDPOINT`.
    InvalidEndpoint,
    /// `INVALID_RELATIVE_PATH`.
    InvalidRelativePath,
    /// `INVALID_CREDENTIAL_SLOT`.
    InvalidCredentialSlot,
    /// `INVALID_JSON_BODY`.
    InvalidJsonBody,
    /// `REQUEST_BODY_TOO_LARGE`.
    RequestBodyTooLarge,
    /// `URL_OUTSIDE_BINDING`.
    UrlOutsideBinding,
    /// `CREDENTIAL_BINDING_MISMATCH`.
    CredentialBindingMismatch,
    /// `CREDENTIAL_RESOLUTION_FAILED`.
    CredentialResolutionFailed,
    /// `CANCELLED`.
    Cancelled,
    /// `DEADLINE_EXCEEDED`.
    DeadlineExceeded,
    /// `CLIENT_BUILD_FAILED`.
    ClientBuildFailed,
    /// `TRANSPORT_TIMEOUT`.
    TransportTimeout,
    /// `CONNECT_FAILED`.
    ConnectFailed,
    /// `REQUEST_FAILED`.
    RequestFailed,
    /// `RESPONSE_READ_FAILED`.
    ResponseReadFailed,
    /// `RESPONSE_BODY_TOO_LARGE`.
    ResponseBodyTooLarge,
    /// `RESPONSE_BODY_NOT_UTF8`.
    ResponseBodyNotUtf8,
    /// `RESPONSE_METADATA_INVALID`.
    ResponseMetadataInvalid,
    /// `REDIRECT_DENIED`.
    RedirectDenied,
}

impl ProviderCallFailureCodeV1 {
    /// Returns the frozen machine-readable code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidEndpoint => "INVALID_ENDPOINT",
            Self::InvalidRelativePath => "INVALID_RELATIVE_PATH",
            Self::InvalidCredentialSlot => "INVALID_CREDENTIAL_SLOT",
            Self::InvalidJsonBody => "INVALID_JSON_BODY",
            Self::RequestBodyTooLarge => "REQUEST_BODY_TOO_LARGE",
            Self::UrlOutsideBinding => "URL_OUTSIDE_BINDING",
            Self::CredentialBindingMismatch => "CREDENTIAL_BINDING_MISMATCH",
            Self::CredentialResolutionFailed => "CREDENTIAL_RESOLUTION_FAILED",
            Self::Cancelled => "CANCELLED",
            Self::DeadlineExceeded => "DEADLINE_EXCEEDED",
            Self::ClientBuildFailed => "CLIENT_BUILD_FAILED",
            Self::TransportTimeout => "TRANSPORT_TIMEOUT",
            Self::ConnectFailed => "CONNECT_FAILED",
            Self::RequestFailed => "REQUEST_FAILED",
            Self::ResponseReadFailed => "RESPONSE_READ_FAILED",
            Self::ResponseBodyTooLarge => "RESPONSE_BODY_TOO_LARGE",
            Self::ResponseBodyNotUtf8 => "RESPONSE_BODY_NOT_UTF8",
            Self::ResponseMetadataInvalid => "RESPONSE_METADATA_INVALID",
            Self::RedirectDenied => "REDIRECT_DENIED",
        }
    }
}

fixed_debug!(ProviderCallFailureCodeV1 {
    InvalidEndpoint => "InvalidEndpoint",
    InvalidRelativePath => "InvalidRelativePath",
    InvalidCredentialSlot => "InvalidCredentialSlot",
    InvalidJsonBody => "InvalidJsonBody",
    RequestBodyTooLarge => "RequestBodyTooLarge",
    UrlOutsideBinding => "UrlOutsideBinding",
    CredentialBindingMismatch => "CredentialBindingMismatch",
    CredentialResolutionFailed => "CredentialResolutionFailed",
    Cancelled => "Cancelled",
    DeadlineExceeded => "DeadlineExceeded",
    ClientBuildFailed => "ClientBuildFailed",
    TransportTimeout => "TransportTimeout",
    ConnectFailed => "ConnectFailed",
    RequestFailed => "RequestFailed",
    ResponseReadFailed => "ResponseReadFailed",
    ResponseBodyTooLarge => "ResponseBodyTooLarge",
    ResponseBodyNotUtf8 => "ResponseBodyNotUtf8",
    ResponseMetadataInvalid => "ResponseMetadataInvalid",
    RedirectDenied => "RedirectDenied",
});

/// A bounded category for observed call counts.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderCallCountV1 {
    /// No call was observed.
    Zero,
    /// Exactly one call was observed.
    One,
    /// More than one call was observed.
    MoreThanOne,
}

impl ProviderCallCountV1 {
    /// Saturates an observed count into the closed evidence category.
    #[must_use]
    pub const fn from_usize(count: usize) -> Self {
        match count {
            0 => Self::Zero,
            1 => Self::One,
            _ => Self::MoreThanOne,
        }
    }
}

fixed_debug!(ProviderCallCountV1 {
    Zero => "Zero",
    One => "One",
    MoreThanOne => "MoreThanOne",
});

/// The exact expected response or closed failure code.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderCallExpectedOutcomeV1 {
    /// A bounded response matched field by field.
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

impl fmt::Debug for ProviderCallExpectedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response { status, body, content_type, retry_after } => formatter
                .debug_struct("Response")
                .field("status", status)
                .field("body_byte_count", &body.len())
                .field("content_type", &MetadataSummary(*content_type))
                .field("retry_after", &MetadataSummary(*retry_after))
                .finish(),
            Self::Failure { code } => {
                formatter.debug_struct("Failure").field("code", code).finish()
            }
        }
    }
}

/// Expected resolver/transport boundary evidence.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderCallExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    resolver_future_dropped_while_pending: bool,
    transport_future_dropped_while_pending: bool,
}

impl ProviderCallExpectedEvidenceV1 {
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

    /// Returns whether a pending resolver future must be dropped.
    #[must_use]
    pub const fn resolver_future_dropped_while_pending(&self) -> bool {
        self.resolver_future_dropped_while_pending
    }

    /// Returns whether a pending transport future must be dropped.
    #[must_use]
    pub const fn transport_future_dropped_while_pending(&self) -> bool {
        self.transport_future_dropped_while_pending
    }
}

impl fmt::Debug for ProviderCallExpectedEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderCallExpectedEvidenceV1")
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
            .finish()
    }
}

/// The expected outcome and boundary evidence for one fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderCallExpectedV1 {
    outcome: ProviderCallExpectedOutcomeV1,
    evidence: ProviderCallExpectedEvidenceV1,
}

impl ProviderCallExpectedV1 {
    /// Returns the expected response or failure.
    #[must_use]
    pub const fn outcome(&self) -> &ProviderCallExpectedOutcomeV1 {
        &self.outcome
    }

    /// Returns the expected resolver and transport evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderCallExpectedEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ProviderCallExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderCallExpectedV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// One immutable canonical provider-call fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderCallFixtureV1 {
    case_id: ProviderCallCaseIdV1,
    input: ProviderCallInputV1,
    control: ProviderCallControlV1,
    upstream: ProviderCallUpstreamV1,
    expected: ProviderCallExpectedV1,
}

impl ProviderCallFixtureV1 {
    /// Returns the stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> ProviderCallCaseIdV1 {
        self.case_id
    }

    /// Returns the immutable raw input.
    #[must_use]
    pub const fn input(&self) -> &ProviderCallInputV1 {
        &self.input
    }

    /// Returns the canonical control behavior.
    #[must_use]
    pub const fn control(&self) -> ProviderCallControlV1 {
        self.control
    }

    /// Returns the canonical fake-upstream behavior.
    #[must_use]
    pub const fn upstream(&self) -> &ProviderCallUpstreamV1 {
        &self.upstream
    }

    /// Returns the exact expected outcome and evidence.
    #[must_use]
    pub const fn expected(&self) -> &ProviderCallExpectedV1 {
        &self.expected
    }
}

impl fmt::Debug for ProviderCallFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderCallFixtureV1")
            .field("case_id", &self.case_id)
            .field("control", &self.control)
            .field("input", &self.input)
            .field("upstream", &self.upstream)
            .field("expected", &self.expected)
            .finish()
    }
}

const HEADERS: &[(&str, &str)] = &[("header-name-debug-sentinel", "header-value-debug-sentinel")];
const ENDPOINT: &str = "https://endpoint-debug-sentinel.invalid/base";
const BOUND_SLOT: &str = "bound-slot-debug-sentinel";
const REQUESTED_SLOT: &str = BOUND_SLOT;
const DIFFERENT_SLOT: &str = "requested-slot-debug-sentinel";
const PATH: &str = "path-debug-sentinel";
const INVALID_PATH: &str = "../path-debug-sentinel";
const REQUEST_BODY: &str = r#"{"value":"request-body-debug-sentinel"}"#;
const RESPONSE_BODY: &str = r#"{"value":"response-body-debug-sentinel"}"#;
const CONTENT_TYPE: &str = "content-type-debug-sentinel";
const RETRY_AFTER: &str = "retry-after-debug-sentinel";

const fn input(path: &'static str, requested_slot: &'static str) -> ProviderCallInputV1 {
    ProviderCallInputV1 {
        endpoint: ENDPOINT,
        bound_credential_slot: BOUND_SLOT,
        requested_credential_slot: requested_slot,
        relative_path: path,
        json_body: REQUEST_BODY,
        headers: HEADERS,
    }
}

const fn evidence(
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    resolver_drop: bool,
    transport_drop: bool,
) -> ProviderCallExpectedEvidenceV1 {
    ProviderCallExpectedEvidenceV1 {
        resolver_calls,
        transport_calls,
        resolver_future_dropped_while_pending: resolver_drop,
        transport_future_dropped_while_pending: transport_drop,
    }
}

const fn failure(
    code: ProviderCallFailureCodeV1,
    expected_evidence: ProviderCallExpectedEvidenceV1,
) -> ProviderCallExpectedV1 {
    ProviderCallExpectedV1 {
        outcome: ProviderCallExpectedOutcomeV1::Failure { code },
        evidence: expected_evidence,
    }
}

const FIXTURES: &[ProviderCallFixtureV1] = &[
    ProviderCallFixtureV1 {
        case_id: ProviderCallCaseIdV1::Success,
        input: input(PATH, REQUESTED_SLOT),
        control: ProviderCallControlV1::Complete,
        upstream: ProviderCallUpstreamV1::Response(ProviderCallRawResponseV1 {
            status: 201,
            body: RESPONSE_BODY,
            content_type: Some(CONTENT_TYPE),
            retry_after: Some(RETRY_AFTER),
        }),
        expected: ProviderCallExpectedV1 {
            outcome: ProviderCallExpectedOutcomeV1::Response {
                status: 201,
                body: RESPONSE_BODY,
                content_type: Some(CONTENT_TYPE),
                retry_after: Some(RETRY_AFTER),
            },
            evidence: evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, false, false),
        },
    },
    ProviderCallFixtureV1 {
        case_id: ProviderCallCaseIdV1::InvalidRelativePath,
        input: input(INVALID_PATH, REQUESTED_SLOT),
        control: ProviderCallControlV1::Complete,
        upstream: ProviderCallUpstreamV1::NotReached,
        expected: failure(
            ProviderCallFailureCodeV1::InvalidRelativePath,
            evidence(ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false, false),
        ),
    },
    ProviderCallFixtureV1 {
        case_id: ProviderCallCaseIdV1::CredentialSlotMismatch,
        input: input(PATH, DIFFERENT_SLOT),
        control: ProviderCallControlV1::Complete,
        upstream: ProviderCallUpstreamV1::NotReached,
        expected: failure(
            ProviderCallFailureCodeV1::CredentialBindingMismatch,
            evidence(ProviderCallCountV1::Zero, ProviderCallCountV1::Zero, false, false),
        ),
    },
    ProviderCallFixtureV1 {
        case_id: ProviderCallCaseIdV1::RedirectDenied,
        input: input(PATH, REQUESTED_SLOT),
        control: ProviderCallControlV1::Complete,
        upstream: ProviderCallUpstreamV1::TransportFailure(TransportErrorV1::RedirectDenied),
        expected: failure(
            ProviderCallFailureCodeV1::RedirectDenied,
            evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, false, false),
        ),
    },
    ProviderCallFixtureV1 {
        case_id: ProviderCallCaseIdV1::ResponseBodyTooLarge,
        input: input(PATH, REQUESTED_SLOT),
        control: ProviderCallControlV1::Complete,
        upstream: ProviderCallUpstreamV1::TransportFailure(TransportErrorV1::ResponseBodyTooLarge),
        expected: failure(
            ProviderCallFailureCodeV1::ResponseBodyTooLarge,
            evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, false, false),
        ),
    },
    ProviderCallFixtureV1 {
        case_id: ProviderCallCaseIdV1::Cancelled,
        input: input(PATH, REQUESTED_SLOT),
        control: ProviderCallControlV1::CancelWhileResolverPending,
        upstream: ProviderCallUpstreamV1::NotReached,
        expected: failure(
            ProviderCallFailureCodeV1::Cancelled,
            evidence(ProviderCallCountV1::One, ProviderCallCountV1::Zero, true, false),
        ),
    },
    ProviderCallFixtureV1 {
        case_id: ProviderCallCaseIdV1::DeadlineExceeded,
        input: input(PATH, REQUESTED_SLOT),
        control: ProviderCallControlV1::ExpireWhileTransportPending,
        upstream: ProviderCallUpstreamV1::Pending,
        expected: failure(
            ProviderCallFailureCodeV1::DeadlineExceeded,
            evidence(ProviderCallCountV1::One, ProviderCallCountV1::One, false, true),
        ),
    },
];

/// Returns the immutable canonical provider-call fixture table.
#[must_use]
pub const fn provider_call_fixtures_v1() -> &'static [ProviderCallFixtureV1] {
    FIXTURES
}

struct MetadataSummary(Option<&'static str>);

impl fmt::Debug for MetadataSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(value) => {
                formatter.debug_struct("Present").field("byte_count", &value.len()).finish()
            }
            None => formatter.write_str("Absent"),
        }
    }
}

struct TransportCodeDebug(&'static str);

impl fmt::Debug for TransportCodeDebug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}
