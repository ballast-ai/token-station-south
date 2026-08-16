#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Host-neutral contracts for provider execution.

use std::{collections::BTreeMap, fmt};

use http::{HeaderName, HeaderValue, StatusCode};
use thiserror::Error;
use url::Url;

/// The version of the buffered HTTP request and response contract.
pub const HTTP_CONTRACT_VERSION: u16 = 1;

/// The version of the provider authentication declaration contract.
pub const AUTH_CONTRACT_VERSION: u16 = 1;

/// The version of the stable provider-call error contract.
pub const ERROR_CONTRACT_VERSION: u16 = 1;

/// Streaming is not part of the version-one buffered HTTP contract.
pub const STREAM_CONTRACT_VERSION: Option<u16> = None;

/// The maximum byte length of a trusted provider base endpoint.
pub const MAX_ENDPOINT_BYTES: usize = 8 * 1024;

/// The maximum byte length of a provider-selected relative path.
pub const MAX_RELATIVE_PATH_BYTES: usize = 2 * 1024;

/// The maximum byte length of a provider-selected credential slot identifier.
pub const MAX_CREDENTIAL_SLOT_BYTES: usize = 64;

/// The maximum byte length of a buffered JSON request body.
pub const MAX_JSON_REQUEST_BODY_BYTES: usize = 32 * 1024 * 1024;

/// The maximum byte length of a buffered UTF-8 response body.
pub const MAX_RESPONSE_BODY_BYTES: usize = 32 * 1024 * 1024;

/// The maximum byte length of the response `content-type` value.
pub const MAX_RESPONSE_CONTENT_TYPE_BYTES: usize = 256;

/// The maximum byte length of the response `retry-after` value.
pub const MAX_RESPONSE_RETRY_AFTER_BYTES: usize = 256;

/// The version of the reserved-header policy enforced by [`SafeHeaders`].
pub const RESERVED_HEADER_POLICY_VERSION: u16 = 1;

/// The maximum number of ordinary provider headers in one request descriptor.
pub const MAX_PROVIDER_HEADER_COUNT: usize = 64;

/// The maximum byte length of one provider header name.
pub const MAX_PROVIDER_HEADER_NAME_BYTES: usize = 256;

/// The maximum byte length of one provider header value.
pub const MAX_PROVIDER_HEADER_VALUE_BYTES: usize = 16 * 1024;

/// The maximum combined byte length of provider header names and values.
pub const MAX_PROVIDER_HEADER_TOTAL_BYTES: usize = 64 * 1024;

const RESERVED_HEADERS: &[&str] = &[
    "api-key",
    "authorization",
    "connection",
    "content-length",
    "cookie",
    "expect",
    "host",
    "keep-alive",
    "ocp-apim-subscription-key",
    "proxy-authorization",
    "proxy-connection",
    "set-cookie",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "user-agent",
    "x-api-key",
    "x-goog-api-key",
    "xi-api-key",
];

/// A bounded, validated set of ordinary HTTP headers supplied by a provider adapter.
///
/// Authentication declarations and resolved credentials use separate contracts. This type
/// intentionally exposes no unchecked collection or deserialization entry point.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SafeHeaders {
    values: BTreeMap<String, String>,
}

impl SafeHeaders {
    /// Validates and normalizes a set of provider-supplied HTTP headers.
    ///
    /// Header names are stored in their lowercase canonical form. Duplicate names are rejected
    /// after normalization rather than silently overwriting an earlier value.
    pub fn try_from_iter<I, N, V>(headers: I) -> Result<Self, HeaderPolicyError>
    where
        I: IntoIterator<Item = (N, V)>,
        N: AsRef<str>,
        V: AsRef<str>,
    {
        let mut values = BTreeMap::new();
        let mut header_count = 0_usize;
        let mut total_bytes = 0_usize;

        for (name, value) in headers {
            header_count = header_count.checked_add(1).ok_or(HeaderPolicyError::TooManyHeaders)?;
            if header_count > MAX_PROVIDER_HEADER_COUNT {
                return Err(HeaderPolicyError::TooManyHeaders);
            }

            let raw_name = name.as_ref();
            let raw_value = value.as_ref();
            if raw_name.len() > MAX_PROVIDER_HEADER_NAME_BYTES {
                return Err(HeaderPolicyError::NameTooLong);
            }
            if raw_value.len() > MAX_PROVIDER_HEADER_VALUE_BYTES {
                return Err(HeaderPolicyError::ValueTooLong);
            }

            total_bytes = total_bytes
                .checked_add(raw_name.len())
                .and_then(|size| size.checked_add(raw_value.len()))
                .ok_or(HeaderPolicyError::TotalSizeExceeded)?;
            if total_bytes > MAX_PROVIDER_HEADER_TOTAL_BYTES {
                return Err(HeaderPolicyError::TotalSizeExceeded);
            }

            let normalized_name = normalize_name(raw_name)?;
            if is_reserved(&normalized_name) {
                return Err(HeaderPolicyError::ReservedHeader);
            }

            HeaderValue::from_str(raw_value).map_err(|_| HeaderPolicyError::InvalidValue)?;

            if values.insert(normalized_name, raw_value.to_owned()).is_some() {
                return Err(HeaderPolicyError::DuplicateHeader);
            }
        }

        Ok(Self { values })
    }

    /// Returns a header value using an ASCII case-insensitive name lookup.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        let normalized_name = normalize_name(name).ok()?;
        self.values.get(&normalized_name).map(String::as_str)
    }

    /// Returns whether the set contains no headers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns the number of validated headers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Iterates over validated header names and values without exposing mutable storage.
    #[must_use]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &str)> {
        self.values.iter().map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

impl fmt::Debug for SafeHeaders {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeHeaders")
            .field("count", &self.values.len())
            .field("policy_version", &RESERVED_HEADER_POLICY_VERSION)
            .finish_non_exhaustive()
    }
}

/// A typed failure produced while validating provider-supplied headers.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HeaderPolicyError {
    /// The header name is not valid according to the HTTP grammar.
    #[error("header name is invalid")]
    InvalidName,

    /// The header name exceeds the explicit boundary limit.
    #[error("header name exceeds the provider header limit")]
    NameTooLong,

    /// The header value is not valid according to the HTTP grammar.
    #[error("header value is invalid")]
    InvalidValue,

    /// The header value exceeds the explicit boundary limit.
    #[error("header value exceeds the provider header limit")]
    ValueTooLong,

    /// The provider attempted to set a header owned by the host transport or authentication layer.
    #[error("header is reserved for the host transport or authentication boundary")]
    ReservedHeader,

    /// The same header appeared more than once after case normalization.
    #[error("provider header is duplicated")]
    DuplicateHeader,

    /// The request descriptor contains too many ordinary provider headers.
    #[error("provider header count exceeds the boundary limit")]
    TooManyHeaders,

    /// The combined provider header name and value bytes exceed the boundary limit.
    #[error("provider header bytes exceed the boundary limit")]
    TotalSizeExceeded,
}

impl HeaderPolicyError {
    /// Returns a stable machine-readable error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidName => "INVALID_HEADER_NAME",
            Self::NameTooLong => "HEADER_NAME_TOO_LONG",
            Self::InvalidValue => "INVALID_HEADER_VALUE",
            Self::ValueTooLong => "HEADER_VALUE_TOO_LONG",
            Self::ReservedHeader => "RESERVED_HEADER_FORBIDDEN",
            Self::DuplicateHeader => "DUPLICATE_HEADER",
            Self::TooManyHeaders => "TOO_MANY_HEADERS",
            Self::TotalSizeExceeded => "HEADER_TOTAL_SIZE_EXCEEDED",
        }
    }
}

fn normalize_name(name: &str) -> Result<String, HeaderPolicyError> {
    HeaderName::from_bytes(name.as_bytes())
        .map(|parsed| parsed.as_str().to_owned())
        .map_err(|_| HeaderPolicyError::InvalidName)
}

fn is_reserved(name: &str) -> bool {
    RESERVED_HEADERS.contains(&name)
}

/// A trusted, validated provider base endpoint.
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderEndpointV1 {
    url: Url,
}

impl ProviderEndpointV1 {
    /// Parses an absolute HTTP or HTTPS endpoint and normalizes its base path to a trailing slash.
    pub fn parse(input: &str) -> Result<Self, ContractErrorV1> {
        if input.is_empty()
            || input.len() > MAX_ENDPOINT_BYTES
            || input.bytes().any(is_forbidden_raw_url_byte)
        {
            return Err(ContractErrorV1::InvalidEndpoint);
        }

        let raw_path = endpoint_raw_path(input).ok_or(ContractErrorV1::InvalidEndpoint)?;
        validate_endpoint_path(raw_path)?;

        let mut url = Url::parse(input).map_err(|_| ContractErrorV1::InvalidEndpoint)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ContractErrorV1::InvalidEndpoint);
        }

        if !url.path().ends_with('/') {
            let mut normalized_path = url.path().to_owned();
            normalized_path.push('/');
            url.set_path(&normalized_path);
        }
        if url.as_str().len() > MAX_ENDPOINT_BYTES {
            return Err(ContractErrorV1::InvalidEndpoint);
        }

        Ok(Self { url })
    }

    /// Returns the normalized endpoint string for trusted host binding and transport assembly.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }
}

impl fmt::Debug for ProviderEndpointV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderEndpointV1")
            .field("contract_version", &HTTP_CONTRACT_VERSION)
            .finish_non_exhaustive()
    }
}

/// A provider-selected path relative to its trusted host binding.
#[derive(Clone, PartialEq, Eq)]
pub struct RelativePathV1 {
    value: String,
}

impl RelativePathV1 {
    /// Validates a provider-selected relative path.
    pub fn parse(input: &str) -> Result<Self, ContractErrorV1> {
        if input.is_empty()
            || input.len() > MAX_RELATIVE_PATH_BYTES
            || !input.is_ascii()
            || input.starts_with('/')
            || input.contains(['?', '#'])
            || input.bytes().any(is_forbidden_path_byte)
            || has_forbidden_percent_encoding(input)
            || !has_safe_segments(input)
            || has_scheme_like_first_segment(input)
        {
            return Err(ContractErrorV1::InvalidRelativePath);
        }

        Ok(Self { value: input.to_owned() })
    }

    /// Returns the validated relative path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Appends this path to a trusted endpoint and rechecks the complete binding boundary.
    pub fn resolve_against(
        &self,
        endpoint: &ProviderEndpointV1,
    ) -> Result<Url, PreparationErrorV1> {
        let mut destination = endpoint.url.clone();
        let destination_path = format!("{}{relative}", endpoint.url.path(), relative = self.value);
        destination.set_path(&destination_path);

        let reparsed =
            Url::parse(destination.as_str()).map_err(|_| PreparationErrorV1::UrlOutsideBinding)?;
        let same_origin = reparsed.scheme() == endpoint.url.scheme()
            && reparsed.host_str() == endpoint.url.host_str()
            && reparsed.port_or_known_default() == endpoint.url.port_or_known_default();
        let inside_base = reparsed.path().starts_with(endpoint.url.path());
        if !same_origin
            || !inside_base
            || !reparsed.username().is_empty()
            || reparsed.password().is_some()
            || reparsed.query().is_some()
            || reparsed.fragment().is_some()
        {
            return Err(PreparationErrorV1::UrlOutsideBinding);
        }

        Ok(reparsed)
    }
}

impl fmt::Debug for RelativePathV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelativePathV1")
            .field("byte_count", &self.value.len())
            .finish_non_exhaustive()
    }
}

/// A provider-selected identifier for a credential authorized by the host.
#[derive(Clone, PartialEq, Eq)]
pub struct CredentialSlotV1 {
    value: String,
}

impl CredentialSlotV1 {
    /// Validates a credential slot identifier.
    pub fn parse(input: &str) -> Result<Self, ContractErrorV1> {
        let mut bytes = input.bytes();
        let valid_first = bytes.next().is_some_and(|byte| byte.is_ascii_lowercase());
        let valid_rest = bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        });
        if input.len() > MAX_CREDENTIAL_SLOT_BYTES || !valid_first || !valid_rest {
            return Err(ContractErrorV1::InvalidCredentialSlot);
        }

        Ok(Self { value: input.to_owned() })
    }

    /// Returns the validated credential slot identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

impl fmt::Debug for CredentialSlotV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialSlotV1")
            .field("byte_count", &self.value.len())
            .finish_non_exhaustive()
    }
}

/// An exact, bounded UTF-8 representation of one complete JSON value.
#[derive(Clone, PartialEq, Eq)]
pub struct JsonBodyV1 {
    value: String,
}

impl JsonBodyV1 {
    /// Validates one complete JSON value without normalizing the supplied UTF-8 text.
    pub fn parse(input: &str) -> Result<Self, ContractErrorV1> {
        if input.len() > MAX_JSON_REQUEST_BODY_BYTES {
            return Err(ContractErrorV1::RequestBodyTooLarge);
        }
        serde_json::from_str::<serde_json::Value>(input)
            .map_err(|_| ContractErrorV1::InvalidJsonBody)?;
        Ok(Self { value: input.to_owned() })
    }

    /// Returns the exact validated JSON text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Returns the request body's byte length.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.value.len()
    }

    /// Returns whether the request body is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
}

impl fmt::Debug for JsonBodyV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JsonBodyV1")
            .field("byte_count", &self.value.len())
            .finish_non_exhaustive()
    }
}

/// A Bearer authentication declaration containing only a host-resolved credential slot.
#[derive(Clone, PartialEq, Eq)]
pub struct BearerAuthV1 {
    credential_slot: CredentialSlotV1,
}

impl BearerAuthV1 {
    /// Creates a Bearer authentication declaration from a validated slot.
    #[must_use]
    pub const fn new(credential_slot: CredentialSlotV1) -> Self {
        Self { credential_slot }
    }

    /// Returns the credential slot requested by the provider.
    #[must_use]
    pub const fn credential_slot(&self) -> &CredentialSlotV1 {
        &self.credential_slot
    }
}

impl fmt::Debug for BearerAuthV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BearerAuthV1")
            .field("contract_version", &AUTH_CONTRACT_VERSION)
            .finish_non_exhaustive()
    }
}

/// A bounded provider request for one JSON POST operation.
#[derive(Clone, PartialEq, Eq)]
pub struct JsonPostRequestV1 {
    relative_path: RelativePathV1,
    headers: SafeHeaders,
    body: JsonBodyV1,
    auth: BearerAuthV1,
}

impl JsonPostRequestV1 {
    /// Creates a request from independently validated, bounded fields.
    #[must_use]
    pub const fn new(
        relative_path: RelativePathV1,
        headers: SafeHeaders,
        body: JsonBodyV1,
        auth: BearerAuthV1,
    ) -> Self {
        Self { relative_path, headers, body, auth }
    }

    /// Returns the provider-selected relative path.
    #[must_use]
    pub const fn relative_path(&self) -> &RelativePathV1 {
        &self.relative_path
    }

    /// Returns the validated ordinary request headers.
    #[must_use]
    pub const fn headers(&self) -> &SafeHeaders {
        &self.headers
    }

    /// Returns the exact validated JSON request body.
    #[must_use]
    pub const fn body(&self) -> &JsonBodyV1 {
        &self.body
    }

    /// Returns the Bearer credential declaration.
    #[must_use]
    pub const fn auth(&self) -> &BearerAuthV1 {
        &self.auth
    }
}

impl fmt::Debug for JsonPostRequestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JsonPostRequestV1")
            .field("http_contract_version", &HTTP_CONTRACT_VERSION)
            .field("auth_contract_version", &AUTH_CONTRACT_VERSION)
            .field("header_count", &self.headers.len())
            .field("body_byte_count", &self.body.len())
            .finish_non_exhaustive()
    }
}

/// A bounded UTF-8 HTTP response with only explicitly reviewed metadata.
#[derive(Clone, PartialEq, Eq)]
pub struct BufferedHttpResponseV1 {
    status: StatusCode,
    body: String,
    content_type: Option<String>,
    retry_after: Option<String>,
}

impl BufferedHttpResponseV1 {
    /// Validates a buffered response and its two allowed metadata fields.
    pub fn try_from_parts(
        status: StatusCode,
        body: Vec<u8>,
        content_type: Option<String>,
        retry_after: Option<String>,
    ) -> Result<Self, TransportErrorV1> {
        if body.len() > MAX_RESPONSE_BODY_BYTES {
            return Err(TransportErrorV1::ResponseBodyTooLarge);
        }
        validate_response_metadata(content_type.as_deref(), MAX_RESPONSE_CONTENT_TYPE_BYTES)?;
        validate_response_metadata(retry_after.as_deref(), MAX_RESPONSE_RETRY_AFTER_BYTES)?;
        let body = String::from_utf8(body).map_err(|_| TransportErrorV1::ResponseBodyNotUtf8)?;

        Ok(Self { status, body, content_type, retry_after })
    }

    /// Returns the upstream HTTP status.
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns the bounded UTF-8 response body.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Returns the bounded `content-type` value when present.
    #[must_use]
    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    /// Returns the bounded `retry-after` value when present.
    #[must_use]
    pub fn retry_after(&self) -> Option<&str> {
        self.retry_after.as_deref()
    }
}

impl fmt::Debug for BufferedHttpResponseV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BufferedHttpResponseV1")
            .field("contract_version", &HTTP_CONTRACT_VERSION)
            .field("status", &self.status)
            .field("body_byte_count", &self.body.len())
            .field("has_content_type", &self.content_type.is_some())
            .field("has_retry_after", &self.retry_after.is_some())
            .finish_non_exhaustive()
    }
}

/// A provider request contract validation failure.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ContractErrorV1 {
    /// The trusted base endpoint is invalid.
    #[error("provider endpoint is invalid")]
    InvalidEndpoint,
    /// The provider-selected relative path is invalid.
    #[error("provider relative path is invalid")]
    InvalidRelativePath,
    /// The provider-selected credential slot is invalid.
    #[error("provider credential slot is invalid")]
    InvalidCredentialSlot,
    /// The request body is not exactly one complete JSON value.
    #[error("request body is not valid JSON")]
    InvalidJsonBody,
    /// The request body exceeds the contract limit.
    #[error("request body exceeds the boundary limit")]
    RequestBodyTooLarge,
}

impl ContractErrorV1 {
    /// Returns the stable machine-readable error code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidEndpoint => "INVALID_ENDPOINT",
            Self::InvalidRelativePath => "INVALID_RELATIVE_PATH",
            Self::InvalidCredentialSlot => "INVALID_CREDENTIAL_SLOT",
            Self::InvalidJsonBody => "INVALID_JSON_BODY",
            Self::RequestBodyTooLarge => "REQUEST_BODY_TOO_LARGE",
        }
    }
}

/// A failure while binding and preparing a validated provider request.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PreparationErrorV1 {
    /// The resolved request URL escaped the trusted host binding.
    #[error("request URL is outside the provider binding")]
    UrlOutsideBinding,
    /// The requested credential slot does not match the trusted host binding.
    #[error("credential slot does not match the provider binding")]
    CredentialBindingMismatch,
    /// The host could not resolve the bound credential.
    #[error("credential resolution failed")]
    CredentialResolutionFailed,
    /// Execution was cancelled.
    #[error("provider request was cancelled")]
    Cancelled,
    /// The caller's absolute deadline was exceeded.
    #[error("provider request deadline was exceeded")]
    DeadlineExceeded,
}

impl PreparationErrorV1 {
    /// Returns the stable machine-readable error code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UrlOutsideBinding => "URL_OUTSIDE_BINDING",
            Self::CredentialBindingMismatch => "CREDENTIAL_BINDING_MISMATCH",
            Self::CredentialResolutionFailed => "CREDENTIAL_RESOLUTION_FAILED",
            Self::Cancelled => "CANCELLED",
            Self::DeadlineExceeded => "DEADLINE_EXCEEDED",
        }
    }
}

/// A stable failure reported by an HTTP transport implementation.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum TransportErrorV1 {
    /// The hardened HTTP client could not be constructed.
    #[error("HTTP client construction failed")]
    ClientBuildFailed,
    /// A transport-owned timeout elapsed before the caller's absolute deadline.
    #[error("HTTP transport timed out")]
    TransportTimeout,
    /// The transport could not connect to the endpoint.
    #[error("HTTP connection failed")]
    ConnectFailed,
    /// The transport could not send the request.
    #[error("HTTP request failed")]
    RequestFailed,
    /// The transport could not read the response.
    #[error("HTTP response read failed")]
    ResponseReadFailed,
    /// The buffered response body exceeds the contract limit.
    #[error("response body exceeds the boundary limit")]
    ResponseBodyTooLarge,
    /// The buffered response body is not valid UTF-8.
    #[error("response body is not valid UTF-8")]
    ResponseBodyNotUtf8,
    /// One of the explicitly allowed response metadata values is invalid.
    #[error("response metadata is invalid")]
    ResponseMetadataInvalid,
    /// The upstream returned a redirect, which the transport must not follow.
    #[error("HTTP redirect is denied")]
    RedirectDenied,
}

impl TransportErrorV1 {
    /// Returns the stable machine-readable error code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
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

fn endpoint_raw_path(input: &str) -> Option<&str> {
    let scheme_end = input.find("://")?;
    let authority_and_path = input.get(scheme_end + 3..)?;
    let path_start = authority_and_path.find('/').unwrap_or(authority_and_path.len());
    let authority = authority_and_path.get(..path_start)?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    if path_start == authority_and_path.len() {
        Some("/")
    } else {
        authority_and_path.get(path_start..)
    }
}

fn validate_endpoint_path(path: &str) -> Result<(), ContractErrorV1> {
    if !path.is_ascii()
        || !path.starts_with('/')
        || (path != "/" && path.contains("//"))
        || path.bytes().any(is_forbidden_path_byte)
        || has_forbidden_percent_encoding(path)
    {
        return Err(ContractErrorV1::InvalidEndpoint);
    }

    let without_leading = &path[1..];
    let without_trailing = without_leading.strip_suffix('/').unwrap_or(without_leading);
    if (!without_trailing.is_empty() && !has_safe_segments(without_trailing))
        || has_scheme_like_first_segment(without_trailing)
    {
        return Err(ContractErrorV1::InvalidEndpoint);
    }
    Ok(())
}

fn has_safe_segments(path: &str) -> bool {
    path.split('/').all(|segment| !segment.is_empty() && !matches!(segment, "." | ".."))
}

fn has_scheme_like_first_segment(path: &str) -> bool {
    let first = path.split('/').next().unwrap_or_default();
    let Some(colon) = first.find(':') else {
        return false;
    };
    let scheme = &first[..colon];
    let mut bytes = scheme.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn has_forbidden_percent_encoding(path: &str) -> bool {
    path.as_bytes().windows(3).any(|window| {
        window[0] == b'%'
            && matches!(
                (window[1].to_ascii_lowercase(), window[2].to_ascii_lowercase()),
                (b'2', b'e' | b'f' | b'5') | (b'5', b'c')
            )
    })
}

const fn is_forbidden_path_byte(byte: u8) -> bool {
    byte <= b' ' || byte == 0x7f || byte == b'\\'
}

const fn is_forbidden_raw_url_byte(byte: u8) -> bool {
    is_forbidden_path_byte(byte)
}

fn validate_response_metadata(
    value: Option<&str>,
    maximum_bytes: usize,
) -> Result<(), TransportErrorV1> {
    if value
        .is_some_and(|value| value.len() > maximum_bytes || HeaderValue::from_str(value).is_err())
    {
        return Err(TransportErrorV1::ResponseMetadataInvalid);
    }
    Ok(())
}
