#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Host-neutral contracts for provider execution.

mod task;

pub use task::{
    HostMintedValuesV1, MAX_CALLBACK_URL_BYTES, MAX_TASK_ID_BYTES, TASK_CONTRACT_VERSION,
    TaskContractErrorV1, TaskFailureKindV1, TaskObservationV1,
};

use std::{collections::BTreeMap, fmt, sync::Arc, time::Duration};

use bytes::Bytes;
use http::{HeaderName, HeaderValue, StatusCode};
use serde::{Deserialize, de::IgnoredAny};
use thiserror::Error;
use url::Url;

/// The version of the buffered HTTP request and response contract.
///
/// Version three is additive: a version-two request is exactly a version-three request with no
/// controlled user-agent declaration, just as a version-one request is a version-two request with
/// no query.
pub const HTTP_CONTRACT_VERSION: u16 = 3;

/// The version of the provider authentication declaration contract.
///
/// Each version is additive: a version-one request is exactly a version-two request using the
/// [`ProviderAuthV1::Bearer`] arm, version two adds the sanctioned header-secret scheme, and
/// version three adds [`ProviderAuthV1::HostSigned`] — the arm whose credential never crosses
/// into South at all.
pub const AUTH_CONTRACT_VERSION: u16 = 3;

/// The version of the stable provider-call error contract.
///
/// Version two is additive: it adds the two request-finalization preparation codes. A consumer
/// pinned to version one sees them through [`PreparationErrorV1::code`] like any other, which is
/// why the enum is `#[non_exhaustive]`.
pub const ERROR_CONTRACT_VERSION: u16 = 2;

/// The version of the byte-level streaming provider call contract.
pub const STREAM_CONTRACT_VERSION: Option<u16> = Some(1);

/// The maximum byte length of a trusted provider base endpoint.
pub const MAX_ENDPOINT_BYTES: usize = 8 * 1024;

/// The maximum byte length of a provider-selected relative path.
pub const MAX_RELATIVE_PATH_BYTES: usize = 2 * 1024;

/// The maximum byte length of one sanctioned query parameter value.
pub const MAX_QUERY_VALUE_BYTES: usize = 64;

/// The maximum byte length of a serialized query, excluding the leading `?`.
pub const MAX_QUERY_TOTAL_BYTES: usize = 256;

/// The maximum byte length of a controlled user-agent value.
///
/// Generous against the audited host inventory (34 bytes at its longest) for the same reason the
/// query value bound is: the character class, not the length, does the security work, and a bound
/// that tracked observed values would break on the next host release without adding protection.
pub const MAX_USER_AGENT_BYTES: usize = 256;

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

/// The version of the closed provider quota response metadata contract.
pub const PROVIDER_QUOTA_METADATA_CONTRACT_VERSION: u16 = 1;

/// The exact number of approved provider quota response metadata fields.
pub const PROVIDER_QUOTA_METADATA_FIELD_COUNT: usize = 9;

/// The maximum byte length of one provider quota response metadata value.
pub const MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES: usize = 256;

/// The maximum combined byte length of all provider quota response metadata values.
pub const MAX_PROVIDER_QUOTA_METADATA_TOTAL_BYTES: usize =
    PROVIDER_QUOTA_METADATA_FIELD_COUNT * MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES;

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
    "x-amz-content-sha256",
    "x-amz-date",
    "x-amz-security-token",
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
        self.resolve_against_with_query(endpoint, None)
    }

    /// Appends this path and an optional sanctioned query, then rechecks the binding boundary.
    ///
    /// The query cannot ride along inside the path: `set_path` percent-encodes `?` into `%3F`,
    /// which would silently turn a query into a literal path segment. It is therefore set
    /// separately, and the post-normalization recheck proves the wire query is byte-for-byte what
    /// was declared. That equality is what makes the recheck a proof rather than a formality: a
    /// `#` or a re-encoded byte inside a value makes `reparsed.query()` differ from the input, and
    /// preparation fails.
    ///
    /// The origin and traversal ring is unchanged by the query. `same_origin` reads scheme, host,
    /// and effective port; `inside_base` reads `path()`. A query lives in `query()`, dot segments
    /// are not normalized across the `?` boundary, and a `#` terminates the query rather than
    /// extending the path — so none of those checks can be moved by query content.
    pub fn resolve_against_with_query(
        &self,
        endpoint: &ProviderEndpointV1,
        query: Option<&QueryStringV1>,
    ) -> Result<Url, PreparationErrorV1> {
        let mut destination = endpoint.url.clone();
        let destination_path = format!("{}{relative}", endpoint.url.path(), relative = self.value);
        destination.set_path(&destination_path);
        if let Some(query) = query {
            destination.set_query(Some(query.as_str()));
        }

        let reparsed =
            Url::parse(destination.as_str()).map_err(|_| PreparationErrorV1::UrlOutsideBinding)?;
        let same_origin = reparsed.scheme() == endpoint.url.scheme()
            && reparsed.host_str() == endpoint.url.host_str()
            && reparsed.port_or_known_default() == endpoint.url.port_or_known_default();
        let inside_base = reparsed.path().starts_with(endpoint.url.path());
        let query_intact = query.map_or_else(
            || reparsed.query().is_none(),
            |declared| reparsed.query() == Some(declared.as_str()),
        );
        if !same_origin
            || !inside_base
            || !query_intact
            || !reparsed.username().is_empty()
            || reparsed.password().is_some()
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

/// The frozen set of headers a host request finalizer is permitted to emit.
///
/// Closed and fieldless for the same reason [`SecretHeaderV1`] is, and for one more: these names
/// carry a signature over the whole request, so a provider that could name its own would be
/// choosing what the signature covers. Every name here is on `RESERVED_HEADERS`, which a unit
/// test asserts — the plain [`SafeHeaders`] channel can never carry one, in either direction.
///
/// Adding a variant is a deliberate contract bump with a conformance case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SignedHeaderV1 {
    /// `authorization` — the AWS `SigV4` credential scope, signed headers list, and signature.
    Authorization,
    /// `x-amz-date` — the signing timestamp the signature is bound to.
    XAmzDate,
    /// `x-amz-content-sha256` — the payload hash the signature commits to.
    XAmzContentSha256,
    /// `x-amz-security-token` — the STS session token, when the credential carries one.
    XAmzSecurityToken,
}

impl SignedHeaderV1 {
    /// Every permitted header, in canonical declaration order.
    ///
    /// [`SignedHeaderSetV1`] normalizes to this order, so two declarations naming the same
    /// headers compare equal regardless of the order the caller wrote them in.
    pub const ALL: [Self; 4] =
        [Self::Authorization, Self::XAmzDate, Self::XAmzContentSha256, Self::XAmzSecurityToken];

    /// Returns the lowercase wire name of the permitted header.
    #[must_use]
    pub const fn header_name(self) -> &'static str {
        match self {
            Self::Authorization => "authorization",
            Self::XAmzDate => "x-amz-date",
            Self::XAmzContentSha256 => "x-amz-content-sha256",
            Self::XAmzSecurityToken => "x-amz-security-token",
        }
    }
}

/// A rejected [`SignedHeaderSetV1`] construction.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum SignedHeaderSetErrorV1 {
    /// The declaration named no headers.
    ///
    /// An empty set would make the allow-list diff vacuous: a finalizer could emit nothing and
    /// still satisfy it, producing an unsigned request that looks finalized.
    #[error("a host-signed declaration must name at least one header")]
    Empty,
    /// The declaration named the same header twice.
    #[error("a host-signed declaration must not name the same header twice")]
    Duplicate,
}

/// The non-empty, duplicate-free set of headers a [`ProviderAuthV1::HostSigned`] declaration
/// promises its finalizer will emit.
///
/// This is the allow-list South diffs the finalizer's output against, in **both** directions: an
/// undeclared name is a finalizer exceeding its declaration, and a declared name that never
/// arrived is a broken signer — a signature missing `x-amz-security-token` is not a request with
/// one fewer header, it is a request that will be rejected upstream for reasons the log will not
/// explain. Being part of the request declaration, it is fixed before finalisation runs and the
/// host cannot widen it at finalise time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedHeaderSetV1 {
    headers: Vec<SignedHeaderV1>,
}

impl SignedHeaderSetV1 {
    /// Validates and normalizes a declaration.
    ///
    /// # Errors
    ///
    /// Returns [`SignedHeaderSetErrorV1`] when the declaration is empty or names a duplicate.
    pub fn new(headers: &[SignedHeaderV1]) -> Result<Self, SignedHeaderSetErrorV1> {
        if headers.is_empty() {
            return Err(SignedHeaderSetErrorV1::Empty);
        }
        let mut normalized = Vec::with_capacity(headers.len());
        for candidate in SignedHeaderV1::ALL {
            if headers.contains(&candidate) {
                normalized.push(candidate);
            }
        }
        if normalized.len() != headers.len() {
            return Err(SignedHeaderSetErrorV1::Duplicate);
        }
        Ok(Self { headers: normalized })
    }

    /// Returns the declared headers in canonical order.
    #[must_use]
    pub fn headers(&self) -> &[SignedHeaderV1] {
        &self.headers
    }

    /// Reports whether the declaration names this header.
    #[must_use]
    pub fn contains(&self, header: SignedHeaderV1) -> bool {
        self.headers.contains(&header)
    }

    /// Returns how many headers the declaration names; never zero.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.headers.len()
    }

    /// Always `false`; a validated declaration names at least one header.
    ///
    /// Present because a bare `len()` invites the clippy lint, and because a caller reading
    /// `is_empty()` should get the invariant rather than reach for `len() == 0`.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// The frozen set of sanctioned query parameters.
///
/// Closed and fieldless for the same reason [`SecretHeaderV1`] is: a query is the most reliably
/// logged component of an HTTP request — proxies, CDNs, upstream access logs, and `Referer`
/// propagation all capture it — so it is the twin of the plain-header channel that
/// `RESERVED_HEADERS` governs. Freezing the names is the structural answer: a provider cannot
/// name `key`, `access_token`, or `sig`, because those names do not exist in this type. No
/// blocklist is consulted and none is needed.
///
/// Values never come from credential resolution. [`ProviderAuthV1`] remains the sole path a
/// secret takes to the wire; there is deliberately no conversion from a resolved secret into a
/// query value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryParameterV1 {
    /// `api-version` (Azure `OpenAI`, Azure AI Foundry).
    ApiVersion,
    /// `alt` (Gemini native streaming).
    Alt,
}

impl QueryParameterV1 {
    /// Every sanctioned parameter, in canonical declaration order.
    ///
    /// Serialization follows this order, so a request's query is byte-identical regardless of the
    /// order the host declared its parameters in.
    pub const ALL: [Self; 2] = [Self::ApiVersion, Self::Alt];

    /// Returns the wire name of the sanctioned parameter.
    #[must_use]
    pub const fn wire_name(&self) -> &'static str {
        match self {
            Self::ApiVersion => "api-version",
            Self::Alt => "alt",
        }
    }

    /// Checks a candidate value against this parameter's own grammar.
    ///
    /// Per-parameter rather than one shared character class: the sanctioned set is small and each
    /// grammar is known exactly, so a shared rule would admit values no upstream accepts and turn
    /// a contract error into a runtime rejection.
    #[must_use]
    fn accepts(self, value: &str) -> bool {
        match self {
            // Real Azure versions are dated (`2024-10-21`, `2025-04-01-preview`) or the literal
            // `v1`. This is deliberately stricter than the adopting host's own handling, which
            // applies no sanitization to the field today.
            Self::ApiVersion => {
                !value.is_empty()
                    && value.len() <= MAX_QUERY_VALUE_BYTES
                    && value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                    })
                    // A value made only of separators (`..`, `.`, `--`) is meaningless as a
                    // version and is the one accepted shape that reads as a dot segment to an
                    // intermediary that re-joins the URL. It cannot move this library's path —
                    // `url` does not normalize dot segments across the `?` boundary — but the
                    // grammar is frozen contract, and narrowing it later is harder than now.
                    && value.bytes().any(|byte| byte.is_ascii_alphanumeric())
            }
            // A closed value set: the upstream accepts nothing else.
            Self::Alt => matches!(value, "sse" | "json"),
        }
    }
}

/// A bounded, ordered, duplicate-free query declaration.
///
/// Constructed only from [`QueryParameterV1`] and values that satisfy that parameter's grammar.
/// Serialization is canonical (declaration order), which is what lets the URL join prove after
/// normalization that the wire query is byte-for-byte what was declared.
#[derive(Clone, PartialEq, Eq)]
pub struct QueryStringV1 {
    serialized: String,
}

impl QueryStringV1 {
    /// Validates and serializes a sanctioned query declaration.
    ///
    /// A repeated parameter is [`ContractErrorV1::DuplicateQueryParameter`] rather than a
    /// last-wins normalization: parameter pollution, where this library and the upstream disagree
    /// about which duplicate wins, is the classic failure mode of permissive query handling.
    pub fn try_from_iter<'v, I>(parameters: I) -> Result<Self, ContractErrorV1>
    where
        I: IntoIterator<Item = (QueryParameterV1, &'v str)>,
    {
        let mut declared: Vec<(QueryParameterV1, &str)> = Vec::new();
        for (parameter, value) in parameters {
            if !parameter.accepts(value) {
                return Err(ContractErrorV1::InvalidQueryValue);
            }
            if declared.iter().any(|(seen, _)| *seen == parameter) {
                return Err(ContractErrorV1::DuplicateQueryParameter);
            }
            declared.push((parameter, value));
        }
        if declared.is_empty() {
            return Err(ContractErrorV1::EmptyQuery);
        }

        let mut serialized = String::new();
        let mut emitted = 0_usize;
        for parameter in QueryParameterV1::ALL {
            let Some((_, value)) = declared.iter().find(|(seen, _)| *seen == parameter) else {
                continue;
            };
            if !serialized.is_empty() {
                serialized.push('&');
            }
            serialized.push_str(parameter.wire_name());
            serialized.push('=');
            serialized.push_str(value);
            emitted += 1;
        }
        // Serialization iterates `ALL` while validation iterates the caller's input, so a variant
        // missing from `ALL` would validate and then contribute nothing — silently dropping a
        // parameter, or producing a bare trailing `?` when it was the only one. Unlike
        // `wire_name`'s exhaustive match, `ALL` membership is not compiler-enforced, so compare
        // the two counts and fail loudly instead of emitting a URL the caller did not ask for.
        if emitted != declared.len() || serialized.is_empty() {
            return Err(ContractErrorV1::EmptyQuery);
        }
        if serialized.len() > MAX_QUERY_TOTAL_BYTES {
            return Err(ContractErrorV1::QueryTooLarge);
        }

        Ok(Self { serialized })
    }

    /// Returns the canonical serialized query, without a leading `?`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.serialized
    }
}

impl fmt::Debug for QueryStringV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Values are provider-authored and land in logs; print only the shape.
        formatter
            .debug_struct("QueryStringV1")
            .field("contract_version", &HTTP_CONTRACT_VERSION)
            .field("byte_count", &self.serialized.len())
            .finish_non_exhaustive()
    }
}

/// A sanctioned `user-agent` declaration for one provider request.
///
/// The header name is fixed: this type sets `user-agent` and nothing else, so the sanctioned
/// channel cannot become a generic header channel, the way the Bearer arm is fixed to
/// `authorization`. The ordinary [`SafeHeaders`] channel keeps rejecting the name via
/// `RESERVED_HEADERS`, which together with the single typed slot makes "exactly one `user-agent`
/// on the wire" structural rather than checked.
///
/// The value is `&'static str` by construction: it must exist in host program text. That is the
/// provenance every audited host consumer actually has — compile-time impersonation literals —
/// and it closes the value channel one level stronger than a validator could: no path exists from
/// resolver output, configuration, or request data to a user-agent value. (`String::leak` defeats
/// this, so `'static` provenance is a discipline claim against accidental flows, not a proof
/// against a hostile host.)
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ControlledUserAgentV1 {
    value: &'static str,
}

impl ControlledUserAgentV1 {
    /// Validates a compile-time user-agent literal against the frozen value grammar.
    ///
    /// Accepted: non-empty, at most [`MAX_USER_AGENT_BYTES`], every byte printable ASCII
    /// including space (`0x20..=0x7E`), and no leading or trailing space. The grammar is strictly
    /// narrower than an HTTP header value, so an accepted value can never fail header encoding at
    /// a transport; control bytes and CR/LF are unrepresentable, which closes header injection
    /// before any boundary.
    ///
    /// # Errors
    ///
    /// Returns [`ContractErrorV1::InvalidUserAgentValue`] when the value violates the grammar.
    pub const fn try_from_static(value: &'static str) -> Result<Self, ContractErrorV1> {
        let bytes = value.as_bytes();
        if bytes.is_empty() || bytes.len() > MAX_USER_AGENT_BYTES {
            return Err(ContractErrorV1::InvalidUserAgentValue);
        }
        if bytes[0] == b' ' || bytes[bytes.len() - 1] == b' ' {
            return Err(ContractErrorV1::InvalidUserAgentValue);
        }
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] < 0x20 || bytes[index] > 0x7E {
                return Err(ContractErrorV1::InvalidUserAgentValue);
            }
            index += 1;
        }
        Ok(Self { value })
    }

    /// Returns the declared user-agent value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.value
    }
}

impl fmt::Debug for ControlledUserAgentV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The value is a compile-time literal rather than a secret, but it lands in logs like any
        // provider-adjacent string, and the redaction discipline is uniform: print only the shape.
        formatter
            .debug_struct("ControlledUserAgentV1")
            .field("contract_version", &HTTP_CONTRACT_VERSION)
            .field("byte_count", &self.value.len())
            .finish_non_exhaustive()
    }
}

/// A provider authentication declaration naming a scheme and a host-resolved credential slot.
///
/// Neither arm carries a credential value. The header-secret arm reuses [`BearerAuthV1`] as its
/// credential-slot carrier: the type is really "a credential-slot declaration", and version one
/// froze its name.
///
/// `#[non_exhaustive]` since 0.7.0 (host-prelude D2): the host-signed slice adds a third arm,
/// and an exhaustive enum would make that addition a breaking change for every downstream
/// `match`. Downstream wildcard arms must fail closed — treat an unknown arm as ineligible,
/// never as a default scheme.
#[non_exhaustive]
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
    /// The host signs the finalised request and emits exactly the declared headers.
    ///
    /// Unlike the other two arms, South never resolves this slot: the signing material stays
    /// entirely host-side and South sees only the emitted header values (host-signed D2). The
    /// slot still participates in the binding check, so a request cannot be signed with an
    /// identity the binding does not authorize.
    HostSigned {
        /// The signing identity the host finalizer will use; checked against the binding.
        slot: BearerAuthV1,
        /// The headers the finalizer promises to emit, enforced in both directions.
        emits: SignedHeaderSetV1,
    },
}

impl ProviderAuthV1 {
    /// Returns the credential slot requested by the provider, regardless of scheme.
    #[must_use]
    pub const fn credential_slot(&self) -> &CredentialSlotV1 {
        match self {
            Self::Bearer(slot)
            | Self::HeaderSecret { slot, .. }
            | Self::HostSigned { slot, .. } => slot.credential_slot(),
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
            Self::HostSigned { slot, emits } => formatter
                .debug_struct("HostSigned")
                .field("slot", slot)
                .field("emits", &emits.headers())
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
    query: Option<QueryStringV1>,
    user_agent: Option<ControlledUserAgentV1>,
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
        Self { relative_path, headers, body, auth: auth.into(), query: None, user_agent: None }
    }

    /// Attaches a sanctioned query declaration to this request.
    ///
    /// A separate builder rather than a `new` parameter so http-contract-version-one call sites
    /// keep compiling unchanged: a v1 request is exactly a v2 request with no query.
    #[must_use]
    pub fn with_query(mut self, query: QueryStringV1) -> Self {
        self.query = Some(query);
        self
    }

    /// Returns the sanctioned query declaration, when one was attached.
    #[must_use]
    pub const fn query(&self) -> Option<&QueryStringV1> {
        self.query.as_ref()
    }

    /// Attaches a sanctioned user-agent declaration to this request.
    ///
    /// A separate builder rather than a `new` parameter so http-contract-version-two call sites
    /// keep compiling unchanged: a v2 request is exactly a v3 request with no user-agent.
    #[must_use]
    pub const fn with_user_agent(mut self, user_agent: ControlledUserAgentV1) -> Self {
        self.user_agent = Some(user_agent);
        self
    }

    /// Returns the sanctioned user-agent declaration, when one was attached.
    #[must_use]
    pub const fn user_agent(&self) -> Option<ControlledUserAgentV1> {
        self.user_agent
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

/// The closed set of provider quota response metadata fields.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderQuotaMetadataFieldV1 {
    /// `x-ratelimit-limit-tokens`.
    XRateLimitLimitTokens,
    /// `x-ratelimit-remaining-tokens`.
    XRateLimitRemainingTokens,
    /// `x-ratelimit-reset-tokens`.
    XRateLimitResetTokens,
    /// `anthropic-ratelimit-tokens-limit`.
    AnthropicRateLimitTokensLimit,
    /// `anthropic-ratelimit-tokens-remaining`.
    AnthropicRateLimitTokensRemaining,
    /// `anthropic-ratelimit-tokens-reset`.
    AnthropicRateLimitTokensReset,
    /// `anthropic-ratelimit-unified-limit`.
    AnthropicRateLimitUnifiedLimit,
    /// `anthropic-ratelimit-unified-remaining`.
    AnthropicRateLimitUnifiedRemaining,
    /// `anthropic-ratelimit-unified-reset`.
    AnthropicRateLimitUnifiedReset,
}

impl ProviderQuotaMetadataFieldV1 {
    /// Returns the canonical lowercase HTTP header name.
    #[must_use]
    pub const fn as_header_name(self) -> &'static str {
        match self {
            Self::XRateLimitLimitTokens => "x-ratelimit-limit-tokens",
            Self::XRateLimitRemainingTokens => "x-ratelimit-remaining-tokens",
            Self::XRateLimitResetTokens => "x-ratelimit-reset-tokens",
            Self::AnthropicRateLimitTokensLimit => "anthropic-ratelimit-tokens-limit",
            Self::AnthropicRateLimitTokensRemaining => "anthropic-ratelimit-tokens-remaining",
            Self::AnthropicRateLimitTokensReset => "anthropic-ratelimit-tokens-reset",
            Self::AnthropicRateLimitUnifiedLimit => "anthropic-ratelimit-unified-limit",
            Self::AnthropicRateLimitUnifiedRemaining => "anthropic-ratelimit-unified-remaining",
            Self::AnthropicRateLimitUnifiedReset => "anthropic-ratelimit-unified-reset",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::XRateLimitLimitTokens => 0,
            Self::XRateLimitRemainingTokens => 1,
            Self::XRateLimitResetTokens => 2,
            Self::AnthropicRateLimitTokensLimit => 3,
            Self::AnthropicRateLimitTokensRemaining => 4,
            Self::AnthropicRateLimitTokensReset => 5,
            Self::AnthropicRateLimitUnifiedLimit => 6,
            Self::AnthropicRateLimitUnifiedRemaining => 7,
            Self::AnthropicRateLimitUnifiedReset => 8,
        }
    }
}

impl fmt::Debug for ProviderQuotaMetadataFieldV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::XRateLimitLimitTokens => "XRateLimitLimitTokens",
            Self::XRateLimitRemainingTokens => "XRateLimitRemainingTokens",
            Self::XRateLimitResetTokens => "XRateLimitResetTokens",
            Self::AnthropicRateLimitTokensLimit => "AnthropicRateLimitTokensLimit",
            Self::AnthropicRateLimitTokensRemaining => "AnthropicRateLimitTokensRemaining",
            Self::AnthropicRateLimitTokensReset => "AnthropicRateLimitTokensReset",
            Self::AnthropicRateLimitUnifiedLimit => "AnthropicRateLimitUnifiedLimit",
            Self::AnthropicRateLimitUnifiedRemaining => "AnthropicRateLimitUnifiedRemaining",
            Self::AnthropicRateLimitUnifiedReset => "AnthropicRateLimitUnifiedReset",
        })
    }
}

/// Exactly the approved, bounded provider quota response metadata values.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProviderQuotaMetadataV1 {
    values: Option<Arc<[Option<String>; PROVIDER_QUOTA_METADATA_FIELD_COUNT]>>,
}

impl ProviderQuotaMetadataV1 {
    /// Validates a closed sequence of provider quota metadata fields.
    pub fn try_from_iter<I>(fields: I) -> Result<Self, TransportErrorV1>
    where
        I: IntoIterator<Item = (ProviderQuotaMetadataFieldV1, String)>,
    {
        let mut values: [Option<String>; PROVIDER_QUOTA_METADATA_FIELD_COUNT] = Default::default();
        let mut total_bytes = 0_usize;
        let mut present_field_count = 0_usize;

        for (field, value) in fields {
            if value.len() > MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES
                || HeaderValue::from_str(&value).is_err()
            {
                return Err(TransportErrorV1::ResponseMetadataInvalid);
            }
            total_bytes = total_bytes
                .checked_add(value.len())
                .ok_or(TransportErrorV1::ResponseMetadataInvalid)?;
            if total_bytes > MAX_PROVIDER_QUOTA_METADATA_TOTAL_BYTES {
                return Err(TransportErrorV1::ResponseMetadataInvalid);
            }
            let slot = &mut values[field.index()];
            if slot.is_some() {
                return Err(TransportErrorV1::ResponseMetadataInvalid);
            }
            *slot = Some(value);
            present_field_count += 1;
        }

        Ok(Self { values: (present_field_count != 0).then(|| Arc::new(values)) })
    }

    /// Returns one approved field when present.
    #[must_use]
    pub fn value(&self, field: ProviderQuotaMetadataFieldV1) -> Option<&str> {
        self.values.as_ref().and_then(|values| values[field.index()].as_deref())
    }

    /// Returns the number of present approved fields.
    #[must_use]
    pub fn present_field_count(&self) -> usize {
        self.values.as_ref().map_or(0, |values| values.iter().flatten().count())
    }

    /// Returns `x-ratelimit-limit-tokens` when present.
    #[must_use]
    pub fn x_ratelimit_limit_tokens(&self) -> Option<&str> {
        self.value(ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens)
    }

    /// Returns `x-ratelimit-remaining-tokens` when present.
    #[must_use]
    pub fn x_ratelimit_remaining_tokens(&self) -> Option<&str> {
        self.value(ProviderQuotaMetadataFieldV1::XRateLimitRemainingTokens)
    }

    /// Returns `x-ratelimit-reset-tokens` when present.
    #[must_use]
    pub fn x_ratelimit_reset_tokens(&self) -> Option<&str> {
        self.value(ProviderQuotaMetadataFieldV1::XRateLimitResetTokens)
    }

    /// Returns `anthropic-ratelimit-tokens-limit` when present.
    #[must_use]
    pub fn anthropic_ratelimit_tokens_limit(&self) -> Option<&str> {
        self.value(ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensLimit)
    }

    /// Returns `anthropic-ratelimit-tokens-remaining` when present.
    #[must_use]
    pub fn anthropic_ratelimit_tokens_remaining(&self) -> Option<&str> {
        self.value(ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensRemaining)
    }

    /// Returns `anthropic-ratelimit-tokens-reset` when present.
    #[must_use]
    pub fn anthropic_ratelimit_tokens_reset(&self) -> Option<&str> {
        self.value(ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensReset)
    }

    /// Returns `anthropic-ratelimit-unified-limit` when present.
    #[must_use]
    pub fn anthropic_ratelimit_unified_limit(&self) -> Option<&str> {
        self.value(ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedLimit)
    }

    /// Returns `anthropic-ratelimit-unified-remaining` when present.
    #[must_use]
    pub fn anthropic_ratelimit_unified_remaining(&self) -> Option<&str> {
        self.value(ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedRemaining)
    }

    /// Returns `anthropic-ratelimit-unified-reset` when present.
    #[must_use]
    pub fn anthropic_ratelimit_unified_reset(&self) -> Option<&str> {
        self.value(ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedReset)
    }
}

impl fmt::Debug for ProviderQuotaMetadataV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderQuotaMetadataV1")
            .field("contract_version", &PROVIDER_QUOTA_METADATA_CONTRACT_VERSION)
            .field("present_field_count", &self.present_field_count())
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
    provider_quota_metadata: ProviderQuotaMetadataV1,
}

impl BufferedHttpResponseV1 {
    /// Validates a buffered response with the legacy empty quota metadata shape.
    pub fn try_from_parts(
        status: StatusCode,
        body: Vec<u8>,
        content_type: Option<String>,
        retry_after: Option<String>,
    ) -> Result<Self, TransportErrorV1> {
        Self::try_from_parts_with_provider_quota_metadata(
            status,
            body,
            content_type,
            retry_after,
            ProviderQuotaMetadataV1::default(),
        )
    }

    /// Validates a buffered response and all explicitly allowed metadata.
    pub fn try_from_parts_with_provider_quota_metadata(
        status: StatusCode,
        body: Vec<u8>,
        content_type: Option<String>,
        retry_after: Option<String>,
        provider_quota_metadata: ProviderQuotaMetadataV1,
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

        Ok(Self { status, body, content_type, retry_after, provider_quota_metadata })
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

    /// Returns the bounded provider quota response metadata.
    #[must_use]
    pub const fn provider_quota_metadata(&self) -> &ProviderQuotaMetadataV1 {
        &self.provider_quota_metadata
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
            .field("provider_quota_metadata", &self.provider_quota_metadata)
            .finish_non_exhaustive()
    }
}

/// The bounded, headers-ready metadata of one streaming HTTP exchange.
///
/// The head is handed to the host before any body byte is pulled, so the host can branch on
/// status and all explicitly allowed metadata without touching the stream.
#[derive(Clone, PartialEq, Eq)]
pub struct StreamingResponseHeadV1 {
    status: StatusCode,
    content_type: Option<String>,
    retry_after: Option<String>,
    provider_quota_metadata: ProviderQuotaMetadataV1,
}

impl StreamingResponseHeadV1 {
    /// Validates a streaming response head with the legacy empty quota metadata shape.
    ///
    /// Redirect statuses are refused because the streaming transport must never follow one.
    pub fn try_from_parts(
        status: StatusCode,
        content_type: Option<String>,
        retry_after: Option<String>,
    ) -> Result<Self, TransportErrorV1> {
        Self::try_from_parts_with_provider_quota_metadata(
            status,
            content_type,
            retry_after,
            ProviderQuotaMetadataV1::default(),
        )
    }

    /// Validates a streaming response head and all explicitly allowed metadata.
    pub fn try_from_parts_with_provider_quota_metadata(
        status: StatusCode,
        content_type: Option<String>,
        retry_after: Option<String>,
        provider_quota_metadata: ProviderQuotaMetadataV1,
    ) -> Result<Self, TransportErrorV1> {
        if status.is_redirection() {
            return Err(TransportErrorV1::RedirectDenied);
        }
        validate_response_metadata(content_type.as_deref(), MAX_RESPONSE_CONTENT_TYPE_BYTES)?;
        validate_response_metadata(retry_after.as_deref(), MAX_RESPONSE_RETRY_AFTER_BYTES)?;

        Ok(Self { status, content_type, retry_after, provider_quota_metadata })
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

    /// Returns the bounded provider quota response metadata.
    #[must_use]
    pub const fn provider_quota_metadata(&self) -> &ProviderQuotaMetadataV1 {
        &self.provider_quota_metadata
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
            .field("provider_quota_metadata", &self.provider_quota_metadata)
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
    /// A query value violates its sanctioned parameter's grammar.
    #[error("query parameter value is invalid")]
    InvalidQueryValue,
    /// The same sanctioned parameter was declared more than once.
    #[error("query parameter is declared more than once")]
    DuplicateQueryParameter,
    /// A query was declared with no parameters.
    #[error("query declares no parameters")]
    EmptyQuery,
    /// The serialized query exceeds the contract limit.
    #[error("query exceeds the boundary limit")]
    QueryTooLarge,
    /// A controlled user-agent value violates the frozen value grammar.
    #[error("user-agent value is invalid")]
    InvalidUserAgentValue,
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
            Self::InvalidQueryValue => "INVALID_QUERY_VALUE",
            Self::DuplicateQueryParameter => "DUPLICATE_QUERY_PARAMETER",
            Self::EmptyQuery => "EMPTY_QUERY",
            Self::QueryTooLarge => "QUERY_TOO_LARGE",
            Self::InvalidUserAgentValue => "INVALID_USER_AGENT_VALUE",
        }
    }
}

/// A failure while binding and preparing a validated provider request.
///
/// `#[non_exhaustive]` since 0.7.0 (host-prelude D2/D4): the host-signed slice adds finalizer
/// variants additively. Downstream matches need a wildcard arm; route it through [`Self::code`]
/// when only the stable string matters.
#[non_exhaustive]
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
    /// The auth declaration uses an arm this orchestration build does not support.
    ///
    /// Structurally unreachable while the two frozen arms are the whole of [`ProviderAuthV1`];
    /// load-bearing from the first release that adds an arm a deployed orchestration crate
    /// predates. The wildcard arm that `#[non_exhaustive]` forces on cross-crate matches must
    /// fail closed with this code instead of panicking.
    #[error("auth shape is not supported by this build")]
    UnsupportedAuthShape,
    /// The host request finalizer returned an error; nothing reached the network.
    #[error("host request finalization failed")]
    RequestFinalizationFailed,
    /// The finalizer's emitted headers did not match its declaration; nothing reached the network.
    ///
    /// Covers all four rejections: an undeclared name, a declared name that never arrived, an
    /// empty value, and a duplicate.
    #[error("host request finalization produced headers outside its declaration")]
    RequestFinalizationRejected,
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
            Self::UnsupportedAuthShape => "UNSUPPORTED_AUTH_SHAPE",
            Self::RequestFinalizationFailed => "REQUEST_FINALIZATION_FAILED",
            Self::RequestFinalizationRejected => "REQUEST_FINALIZATION_REJECTED",
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

#[cfg(test)]
mod query_serialization_completeness_tests {
    use super::{ContractErrorV1, QueryParameterV1, QueryStringV1};

    #[test]
    fn every_variant_is_serializable_so_no_declaration_can_be_silently_dropped() {
        // Serialization iterates `ALL` while validation iterates the caller's input. `ALL`
        // membership is not compiler-enforced (unlike `wire_name`'s exhaustive match), so a
        // variant missing from `ALL` would validate and then contribute nothing — dropping a
        // parameter, or emitting a bare `?` when it was the only one. Proving every constructible
        // variant round-trips is what makes that drift impossible to ship silently.
        for parameter in QueryParameterV1::ALL {
            let value = match parameter {
                QueryParameterV1::ApiVersion => "v1",
                QueryParameterV1::Alt => "sse",
            };
            let query = QueryStringV1::try_from_iter([(parameter, value)])
                .expect("a sanctioned parameter with a valid value must construct");
            assert_eq!(query.as_str(), format!("{}={value}", parameter.wire_name()));
        }
    }

    #[test]
    fn an_empty_serialization_is_never_returned_as_success() {
        // The guard is on the serialized value, not the declaration count, so it holds even if
        // `ALL` drifts away from the enum.
        let empty: [(QueryParameterV1, &str); 0] = [];
        assert_eq!(QueryStringV1::try_from_iter(empty), Err(ContractErrorV1::EmptyQuery));
    }
}

#[cfg(test)]
mod query_intactness_tests {
    use super::{ProviderEndpointV1, QueryStringV1, RelativePathV1};

    /// Constructs a query that the public grammar would never produce.
    ///
    /// The intactness check in [`RelativePathV1::resolve_against_with_query`] compares the wire
    /// query against the declaration byte for byte. No value the grammar accepts can falsify it —
    /// `url` treats every accepted byte as an identity map — so a purely public test can only
    /// confirm it vacuously, and deleting the check leaves the whole suite green. These tests
    /// reach past the grammar so the check has a real tripwire: they fail the moment it is
    /// weakened, which is what protects a future grammar relaxation from shipping unnoticed.
    fn unchecked_query(serialized: &str) -> QueryStringV1 {
        QueryStringV1 { serialized: serialized.to_owned() }
    }

    #[test]
    fn a_value_url_would_re_encode_is_refused_rather_than_silently_rewritten() {
        let endpoint = ProviderEndpointV1::parse("https://example.com/base/").unwrap();
        let path = RelativePathV1::parse("v1/resource").unwrap();

        // `url` percent-encodes each of these inside a query, so the wire would carry different
        // bytes than were declared. Preparation must fail instead of sending the rewritten form.
        for rewritten in ["api-version=a b", "api-version=a\"b", "api-version=a<b", "alt=a\u{0}b"] {
            let query = unchecked_query(rewritten);
            assert!(
                path.resolve_against_with_query(&endpoint, Some(&query)).is_err(),
                "{rewritten:?} is re-encoded by url and must not survive the intactness check"
            );
        }
    }

    #[test]
    fn a_value_url_would_delete_is_refused() {
        let endpoint = ProviderEndpointV1::parse("https://example.com/base/").unwrap();
        let path = RelativePathV1::parse("v1/resource").unwrap();

        // `url` silently *deletes* CR and LF from a query. Byte-for-byte comparison is the only
        // thing standing between that deletion and a URL nobody declared.
        for deleted in ["api-version=a\nb", "api-version=a\rb"] {
            let query = unchecked_query(deleted);
            assert!(
                path.resolve_against_with_query(&endpoint, Some(&query)).is_err(),
                "{deleted:?} loses bytes in url and must not survive the intactness check"
            );
        }
    }

    #[test]
    fn a_fragment_in_a_value_cannot_reach_the_wire() {
        let endpoint = ProviderEndpointV1::parse("https://example.com/base/").unwrap();
        let path = RelativePathV1::parse("v1/resource").unwrap();

        // A `#` would terminate the query and open a fragment. The check rejects it because the
        // surviving query no longer equals the declaration.
        let query = unchecked_query("api-version=v1#frag");
        assert!(path.resolve_against_with_query(&endpoint, Some(&query)).is_err());
    }

    #[test]
    fn a_declared_query_free_request_cannot_acquire_a_query() {
        // The `None` arm is the version-one regression guard: a bare path must never gain a query
        // through the join. It is deleted by the same mutation that deletes the positive arm.
        let endpoint = ProviderEndpointV1::parse("https://example.com/base/").unwrap();
        let path = RelativePathV1::parse("v1/resource").unwrap();
        let resolved = path.resolve_against_with_query(&endpoint, None).unwrap();
        assert!(resolved.query().is_none());
    }
}
