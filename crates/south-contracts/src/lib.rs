#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Host-neutral contracts for provider execution.

use std::{collections::BTreeMap, fmt};

use http::{HeaderName, HeaderValue};
use thiserror::Error;

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
