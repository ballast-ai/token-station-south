#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Host-neutral contracts for provider execution.

use std::{collections::BTreeMap, fmt, sync::Arc, time::Duration};

use bytes::Bytes;
use http::{HeaderName, HeaderValue, StatusCode};
use serde::{Deserialize, de::IgnoredAny};
use thiserror::Error;
use url::Url;

/// The version of the buffered HTTP request and response contract.
pub const HTTP_CONTRACT_VERSION: u16 = 1;

/// The version of the provider authentication declaration contract.
///
/// Version two is additive: a version-one request is exactly a version-two request using the
/// [`ProviderAuthV1::Bearer`] arm. Version two adds the sanctioned header-secret scheme.
pub const AUTH_CONTRACT_VERSION: u16 = 2;

/// The version of the stable provider-call error contract.
pub const ERROR_CONTRACT_VERSION: u16 = 1;

/// The version of the byte-level streaming provider call contract.
pub const STREAM_CONTRACT_VERSION: Option<u16> = Some(1);

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

/// The maximum byte length of the buffered error body attached to a rejected stream.
pub const MAX_STREAM_ERROR_BODY_BYTES: usize = 64 * 1024;

/// The maximum byte length of one chunk yielded by a streaming transport.
pub const MAX_STREAM_CHUNK_BYTES: usize = 64 * 1024;

/// The largest transport-owned total timeout accepted by the provider call contracts.
pub const MAX_TRANSPORT_TIMEOUT: Duration = Duration::from_hours(24);

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
            || has_invalid_or_forbidden_percent_encoding(input)
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
#[derive(PartialEq, Eq)]
pub struct JsonBodyV1 {
    value: Arc<str>,
}

impl JsonBodyV1 {
    /// Validates one complete JSON value without normalizing the supplied UTF-8 text.
    pub fn parse(input: &str) -> Result<Self, ContractErrorV1> {
        if input.len() > MAX_JSON_REQUEST_BODY_BYTES {
            return Err(ContractErrorV1::RequestBodyTooLarge);
        }
        let mut deserializer = serde_json::Deserializer::from_str(input);
        IgnoredAny::deserialize(&mut deserializer).map_err(|_| ContractErrorV1::InvalidJsonBody)?;
        deserializer.end().map_err(|_| ContractErrorV1::InvalidJsonBody)?;
        Ok(Self { value: Arc::from(input) })
    }

    /// Returns the exact validated JSON text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Shares the validated backing allocation with an asynchronous transport.
    ///
    /// This is the only ownership-sharing escape hatch. It avoids copying a body that may be at
    /// the contract limit while keeping `JsonBodyV1` itself non-cloneable.
    #[must_use]
    pub fn shared_owner(&self) -> Arc<str> {
        Arc::clone(&self.value)
    }

    /// Returns the request body's byte length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.value.len()
    }

    /// Returns whether the request body is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
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

/// The frozen set of sanctioned secret-bearing headers.
///
/// This enum is fieldless and closed on purpose: every variant maps to a vetted provider family,
/// and adding one is a deliberate contract bump with a conformance case, not a host-side
/// configuration. Every name in this set must stay on the reserved-header blacklist so a
/// provider can never smuggle the same header through the plain [`SafeHeaders`] channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretHeaderV1 {
    /// `api-key` (Azure `OpenAI`, Azure AI Foundry, Ideogram).
    ApiKey,
    /// `x-api-key` (Anthropic).
    XApiKey,
    /// `x-goog-api-key` (Gemini).
    XGoogApiKey,
    /// `xi-api-key` (`ElevenLabs`).
    XiApiKey,
    /// `ocp-apim-subscription-key` (Azure Speech).
    OcpApimSubscriptionKey,
}

impl SecretHeaderV1 {
    /// Every sanctioned header, in declaration order.
    ///
    /// Tests that must cover the whole sanctioned set iterate this constant instead of writing
    /// their own array: a new variant then reaches those tests automatically. The exhaustive match
    /// in [`SecretHeaderV1::header_name`] makes adding a variant without a wire name a compile
    /// error, and this constant makes adding one without test coverage impossible to miss.
    pub const ALL: [Self; 5] = [
        Self::ApiKey,
        Self::XApiKey,
        Self::XGoogApiKey,
        Self::XiApiKey,
        Self::OcpApimSubscriptionKey,
    ];

    /// Returns the lowercase wire name of the sanctioned header.
    #[must_use]
    pub const fn header_name(&self) -> &'static str {
        match self {
            Self::ApiKey => "api-key",
            Self::XApiKey => "x-api-key",
            Self::XGoogApiKey => "x-goog-api-key",
            Self::XiApiKey => "xi-api-key",
            Self::OcpApimSubscriptionKey => "ocp-apim-subscription-key",
        }
    }
}

/// A provider authentication declaration naming a scheme and a host-resolved credential slot.
///
/// Neither arm carries a credential value. The header-secret arm reuses [`BearerAuthV1`] as its
/// credential-slot carrier: the type is really "a credential-slot declaration", and version one
/// froze its name.
#[derive(Clone, PartialEq, Eq)]
pub enum ProviderAuthV1 {
    /// The secret travels as `Authorization: Bearer …`.
    Bearer(BearerAuthV1),
    /// The secret travels verbatim in one sanctioned provider-specific header.
    HeaderSecret {
        /// The sanctioned secret-bearing header selected by the provider.
        header: SecretHeaderV1,
        /// The credential-slot declaration resolved by the host, exactly as in the Bearer arm.
        slot: BearerAuthV1,
    },
}

impl ProviderAuthV1 {
    /// Returns the credential slot requested by the provider, regardless of scheme.
    #[must_use]
    pub const fn credential_slot(&self) -> &CredentialSlotV1 {
        match self {
            Self::Bearer(slot) | Self::HeaderSecret { slot, .. } => slot.credential_slot(),
        }
    }
}

impl From<BearerAuthV1> for ProviderAuthV1 {
    fn from(auth: BearerAuthV1) -> Self {
        Self::Bearer(auth)
    }
}

impl fmt::Debug for ProviderAuthV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer(slot) => formatter.debug_tuple("Bearer").field(slot).finish(),
            Self::HeaderSecret { header, slot } => formatter
                .debug_struct("HeaderSecret")
                .field("header", header)
                .field("slot", slot)
                .finish(),
        }
    }
}

/// A bounded provider request for one JSON POST operation.
#[derive(PartialEq, Eq)]
pub struct JsonPostRequestV1 {
    relative_path: RelativePathV1,
    headers: SafeHeaders,
    body: JsonBodyV1,
    auth: ProviderAuthV1,
}

impl JsonPostRequestV1 {
    /// Creates a request from independently validated, bounded fields.
    ///
    /// The auth parameter accepts a bare [`BearerAuthV1`] unchanged, so auth-contract-version-one
    /// call sites keep compiling, as well as any explicit [`ProviderAuthV1`] declaration.
    #[must_use]
    pub fn new(
        relative_path: RelativePathV1,
        headers: SafeHeaders,
        body: JsonBodyV1,
        auth: impl Into<ProviderAuthV1>,
    ) -> Self {
        Self { relative_path, headers, body, auth: auth.into() }
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

    /// Returns the provider authentication declaration.
    #[must_use]
    pub const fn auth(&self) -> &ProviderAuthV1 {
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
#[derive(PartialEq, Eq)]
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
        if status.is_redirection() {
            return Err(TransportErrorV1::RedirectDenied);
        }
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

/// The bounded, headers-ready metadata of one streaming HTTP exchange.
///
/// The head is handed to the host before any body byte is pulled, so the host can branch on
/// status and the two explicitly allowed metadata fields without touching the stream.
#[derive(Clone, PartialEq, Eq)]
pub struct StreamingResponseHeadV1 {
    status: StatusCode,
    content_type: Option<String>,
    retry_after: Option<String>,
}

impl StreamingResponseHeadV1 {
    /// Validates a streaming response head and its two allowed metadata fields.
    ///
    /// Redirect statuses are refused because the streaming transport must never follow one.
    pub fn try_from_parts(
        status: StatusCode,
        content_type: Option<String>,
        retry_after: Option<String>,
    ) -> Result<Self, TransportErrorV1> {
        if status.is_redirection() {
            return Err(TransportErrorV1::RedirectDenied);
        }
        validate_response_metadata(content_type.as_deref(), MAX_RESPONSE_CONTENT_TYPE_BYTES)?;
        validate_response_metadata(retry_after.as_deref(), MAX_RESPONSE_RETRY_AFTER_BYTES)?;

        Ok(Self { status, content_type, retry_after })
    }

    /// Returns the upstream HTTP status.
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        self.status
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

impl fmt::Debug for StreamingResponseHeadV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamingResponseHeadV1")
            .field("stream_contract_version", &STREAM_CONTRACT_VERSION)
            .field("status", &self.status)
            .field("has_content_type", &self.content_type.is_some())
            .field("has_retry_after", &self.retry_after.is_some())
            .finish_non_exhaustive()
    }
}

/// A non-2xx streaming exchange collapsed into its head and a bounded error body.
///
/// A rejected exchange never yields a stream object. The error body feeds host-side failure
/// classifiers, so an oversized upstream body is truncated at
/// [`MAX_STREAM_ERROR_BODY_BYTES`] instead of failing the rejection.
#[derive(PartialEq, Eq)]
pub struct StreamRejectedV1 {
    head: StreamingResponseHeadV1,
    body: Vec<u8>,
}

impl StreamRejectedV1 {
    /// Attaches a bounded error body to a rejected streaming head, truncating any excess bytes.
    #[must_use]
    pub fn new(head: StreamingResponseHeadV1, mut body: Vec<u8>) -> Self {
        body.truncate(MAX_STREAM_ERROR_BODY_BYTES);
        Self { head, body }
    }

    /// Returns the headers-ready metadata of the rejected exchange.
    #[must_use]
    pub const fn head(&self) -> &StreamingResponseHeadV1 {
        &self.head
    }

    /// Returns the bounded, possibly truncated upstream error body.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

impl fmt::Debug for StreamRejectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamRejectedV1")
            .field("head", &self.head)
            .field("body_byte_count", &self.body.len())
            .finish_non_exhaustive()
    }
}

/// One bounded chunk of upstream response bytes yielded to the host.
///
/// The wrapped [`Bytes`] shares the transport allocation, so pulling a chunk never copies the
/// streamed body. A transport must re-chunk larger network reads instead of exceeding
/// [`MAX_STREAM_CHUNK_BYTES`].
#[derive(Clone, PartialEq, Eq)]
pub struct StreamChunkV1 {
    bytes: Bytes,
}

impl StreamChunkV1 {
    /// Wraps one delivery-bounded chunk of upstream bytes.
    pub fn try_new(bytes: Bytes) -> Result<Self, StreamReadErrorV1> {
        if bytes.len() > MAX_STREAM_CHUNK_BYTES {
            return Err(StreamReadErrorV1::ChunkNotDeliverable);
        }
        Ok(Self { bytes })
    }

    /// Returns the chunk bytes without copying.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consumes the chunk and returns its shared backing bytes.
    #[must_use]
    pub fn into_bytes(self) -> Bytes {
        self.bytes
    }

    /// Returns the chunk's byte length.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns whether the chunk carries no bytes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl fmt::Debug for StreamChunkV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamChunkV1")
            .field("byte_count", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

/// A stable failure after a streaming exchange has already yielded its head.
///
/// Mid-stream codes are deliberately distinct from their pre-stream cousins so a host can tell
/// "failed before any byte" from "failed after N bytes" by the code alone. The host counts the
/// bytes it has already forwarded; this contract only makes the phase unambiguous.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum StreamReadErrorV1 {
    /// The upstream connection broke while the body was streaming.
    #[error("stream read failed")]
    StreamReadFailed,
    /// The transport idle guard expired between chunks.
    ///
    /// When a transport is configured with a bounded total timeout, its mid-stream expiry is
    /// indistinguishable from the idle guard on reqwest's stable error surface and reports this
    /// same code. A host that needs precise attribution should bound the stream with the caller
    /// deadline instead, which maps to `STREAM_DEADLINE_EXCEEDED`.
    #[error("stream idle guard expired")]
    StreamIdleTimeout,
    /// The caller's absolute deadline expired while the stream was open.
    #[error("stream deadline was exceeded")]
    StreamDeadlineExceeded,
    /// The caller's cancellation token fired while the stream was open.
    #[error("stream was cancelled")]
    StreamCancelled,
    /// The transport produced a chunk that violates the delivery contract.
    #[error("stream chunk is not deliverable")]
    ChunkNotDeliverable,
}

impl StreamReadErrorV1 {
    /// Returns the stable machine-readable error code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::StreamReadFailed => "STREAM_READ_FAILED",
            Self::StreamIdleTimeout => "STREAM_IDLE_TIMEOUT",
            Self::StreamDeadlineExceeded => "STREAM_DEADLINE_EXCEEDED",
            Self::StreamCancelled => "STREAM_CANCELLED",
            Self::ChunkNotDeliverable => "STREAM_CHUNK_INVALID",
        }
    }
}

/// Explicit timeouts for one dedicated streaming transport.
///
/// A `None` total timeout is the production streaming shape: a long generation is legitimate
/// wall-clock work. It is legal only because the idle guard is not optional — every silent
/// upstream dies within `idle_timeout`, which also bounds the time to the first byte.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StreamTransportConfigV1 {
    total: Option<Duration>,
    connect: Duration,
    idle: Duration,
}

impl StreamTransportConfigV1 {
    /// Validates the optional total bound and the mandatory connect and idle guards.
    ///
    /// A bounded total must obey [`MAX_TRANSPORT_TIMEOUT`] and be at least as large as both
    /// mandatory guards.
    pub fn try_new(
        total_timeout: Option<Duration>,
        connect_timeout: Duration,
        idle_timeout: Duration,
    ) -> Result<Self, TransportErrorV1> {
        if connect_timeout.is_zero() || idle_timeout.is_zero() {
            return Err(TransportErrorV1::ClientBuildFailed);
        }
        if let Some(total) = total_timeout
            && (total.is_zero()
                || total > MAX_TRANSPORT_TIMEOUT
                || total < connect_timeout
                || total < idle_timeout)
        {
            return Err(TransportErrorV1::ClientBuildFailed);
        }

        Ok(Self { total: total_timeout, connect: connect_timeout, idle: idle_timeout })
    }

    /// Returns the optional transport-wide wall-clock bound.
    #[must_use]
    pub const fn total_timeout(self) -> Option<Duration> {
        self.total
    }

    /// Returns the TCP connect timeout.
    #[must_use]
    pub const fn connect_timeout(self) -> Duration {
        self.connect
    }

    /// Returns the stall guard applied to each read await, which also caps time to first byte.
    #[must_use]
    pub const fn idle_timeout(self) -> Duration {
        self.idle
    }
}

impl fmt::Debug for StreamTransportConfigV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamTransportConfigV1")
            .field("total_timeout", &self.total)
            .field("connect_timeout", &self.connect)
            .field("idle_timeout", &self.idle)
            .finish()
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
        || has_invalid_or_forbidden_percent_encoding(path)
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

fn has_invalid_or_forbidden_percent_encoding(path: &str) -> bool {
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        let Some(high) = bytes.get(index + 1).and_then(|byte| hex_nibble(*byte)) else {
            return true;
        };
        let Some(low) = bytes.get(index + 2).and_then(|byte| hex_nibble(*byte)) else {
            return true;
        };
        let decoded = (high << 4) | low;
        if matches!(decoded, b'.' | b'/' | b'\\' | b'%') {
            return true;
        }
        index += 3;
    }
    false
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
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
