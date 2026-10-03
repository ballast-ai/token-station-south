//! Declared secret headers: the per-package instances of the header-secret mechanism (auth
//! contract version five, reserved-header policy version two).
//!
//! [`SecretHeaderV1`] closes the *list* of secret-bearing header names, so a provider whose key
//! travels in a header nobody vetted needs a South release and a host rebuild. What has to stay
//! closed is the *mechanism* and its safety rules, not the list
//! (`docs/design/2026-09-30-host-zero-vendor-boundary.md` §10, rule R1). A package therefore
//! declares its own names in its manifest (`secret_headers`), gate ① validates them against the
//! rules in [`DeclaredSecretHeaderV1::parse`], and for that package's requests every declared name
//! joins the reserved set in both directions:
//!
//! - **Request side**: [`SafeHeaders::try_from_iter_with_secret_headers`] refuses a declared name
//!   on the ordinary header channel, so the name can only reach the wire through the auth arm,
//!   whose value is zeroized and never logged.
//! - **Response side**: [`ResponseTranscriptV1::capture_redacting`] drops a declared name from the
//!   display transcript, as it drops `set-cookie`.
//!
//! The rules themselves are fixed here, in one place: a lowercase RFC 9110 token of at most
//! [`MAX_SECRET_HEADER_NAME_BYTES`] bytes that is not on [`UNDECLARABLE_SECRET_HEADER_NAMES`].
//! `south-provider-api` repeats them for gate ① because that crate depends on no other South
//! crate; a conformance test pins the two copies together.
//!
//! [`SafeHeaders::try_from_iter_with_secret_headers`]: crate::SafeHeaders::try_from_iter_with_secret_headers
//! [`ResponseTranscriptV1::capture_redacting`]: crate::ResponseTranscriptV1::capture_redacting
//! [`SecretHeaderV1`]: crate::SecretHeaderV1

use std::{fmt, sync::Arc};

use crate::ContractErrorV1;

/// The maximum byte length of a declared secret header name.
///
/// The longest sanctioned name, `ocp-apim-subscription-key`, is 25 bytes; the bound leaves room
/// without letting a name become a payload.
pub const MAX_SECRET_HEADER_NAME_BYTES: usize = 64;

/// The maximum number of secret headers one package may declare.
pub const MAX_DECLARED_SECRET_HEADERS: usize = 8;

/// The names no package may declare as a secret header, sorted.
///
/// Each is already reserved by South for another purpose, so declaring it would either be
/// redundant or give one name two meanings:
///
/// - the request-side reserved headers: transport framing and hop-by-hop headers, `host`,
///   `expect`, `authorization`, `cookie`, `user-agent`, the host-signed headers, and the five
///   sanctioned [`SecretHeaderV1`](crate::SecretHeaderV1) names, which need no declaration;
/// - the response-side transcript denials (`proxy-authenticate`, `set-cookie2`);
/// - `accept`, which the transport adds on its own behalf;
/// - `content-type`, `content-encoding` and `retry-after`, which describe or govern a response
///   the contracts read;
/// - the closed response diagnostic and provider quota metadata names, which hosts read
///   programmatically and which a transcript redaction would contradict.
///
/// A unit test proves this list covers every one of those sources, so a name added to one of them
/// cannot stay declarable by accident.
pub const UNDECLARABLE_SECRET_HEADER_NAMES: &[&str] = &[
    "accept",
    "anthropic-ratelimit-tokens-limit",
    "anthropic-ratelimit-tokens-remaining",
    "anthropic-ratelimit-tokens-reset",
    "anthropic-ratelimit-unified-limit",
    "anthropic-ratelimit-unified-remaining",
    "anthropic-ratelimit-unified-reset",
    "anthropic-request-id",
    "api-key",
    "authorization",
    "cf-ray",
    "connection",
    "content-encoding",
    "content-length",
    "content-type",
    "cookie",
    "expect",
    "host",
    "keep-alive",
    "ocp-apim-subscription-key",
    "openai-organization",
    "openai-processing-ms",
    "openai-version",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "request-id",
    "retry-after",
    "server",
    "set-cookie",
    "set-cookie2",
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
    "x-ratelimit-limit-tokens",
    "x-ratelimit-remaining-tokens",
    "x-ratelimit-reset-tokens",
    "x-request-id",
    "xi-api-key",
];

/// One validated, package-declared secret-bearing header name.
///
/// The open counterpart of [`SecretHeaderV1`](crate::SecretHeaderV1): the name comes from a
/// package manifest rather than from this crate, so it is checked rather than enumerated. It is
/// stored inline and is `Copy`, so the auth arms that carry it — `south_core::raw::RawAuthV1` in
/// the host prelude, the descriptor admission's result — stay `Copy` as they were.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeclaredSecretHeaderV1 {
    len: u8,
    name: [u8; MAX_SECRET_HEADER_NAME_BYTES],
}

impl DeclaredSecretHeaderV1 {
    /// Validates one declared secret header name.
    ///
    /// The name must be 1 to [`MAX_SECRET_HEADER_NAME_BYTES`] bytes of lowercase RFC 9110 `tchar`
    /// (`a`–`z`, `0`–`9` and ``!#$%&'*+-.^_`|~``) and must not be on
    /// [`UNDECLARABLE_SECRET_HEADER_NAMES`]. Uppercase is refused rather than folded: a manifest
    /// names its headers in their wire form, once.
    ///
    /// # Errors
    ///
    /// Returns [`ContractErrorV1::InvalidSecretHeaderName`] otherwise.
    pub fn parse(input: &str) -> Result<Self, ContractErrorV1> {
        let bytes = input.as_bytes();
        let valid = !bytes.is_empty()
            && bytes.len() <= MAX_SECRET_HEADER_NAME_BYTES
            && bytes.iter().copied().all(is_lowercase_tchar)
            && !UNDECLARABLE_SECRET_HEADER_NAMES.contains(&input);
        if !valid {
            return Err(ContractErrorV1::InvalidSecretHeaderName);
        }
        let len =
            u8::try_from(bytes.len()).map_err(|_| ContractErrorV1::InvalidSecretHeaderName)?;
        let mut name = [0_u8; MAX_SECRET_HEADER_NAME_BYTES];
        if let Some(prefix) = name.get_mut(..bytes.len()) {
            prefix.copy_from_slice(bytes);
        }
        Ok(Self { len, name })
    }

    /// Returns the lowercase wire name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Only `parse` constructs this type, and it admits ASCII alone, so neither fallback is
        // reachable; they exist because this crate denies `unwrap` outside tests.
        let bytes = self.name.get(..usize::from(self.len)).unwrap_or_default();
        std::str::from_utf8(bytes).unwrap_or_default()
    }
}

impl fmt::Debug for DeclaredSecretHeaderV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A header *name* is not a secret; the value bound to it never enters this type.
        formatter.debug_tuple("DeclaredSecretHeaderV1").field(&self.as_str()).finish()
    }
}

/// `tchar` from RFC 9110 §5.6.2, restricted to lowercase letters.
const fn is_lowercase_tchar(byte: u8) -> bool {
    matches!(
        byte,
        b'a'..=b'z'
            | b'0'..=b'9'
            | b'!'
            | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'*'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~'
    )
}

/// One package's declared secret headers: the per-package addition to the reserved-header policy.
///
/// Bounded by [`MAX_DECLARED_SECRET_HEADERS`] and duplicate-free. Cheap to clone, because every
/// [`SafeHeaders`](crate::SafeHeaders) validated under it keeps a copy: that copy is how a
/// transport learns which response headers to drop from the transcript.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct DeclaredSecretHeadersV1 {
    names: Option<Arc<[DeclaredSecretHeaderV1]>>,
}

static NO_DECLARED_SECRET_HEADERS: DeclaredSecretHeadersV1 =
    DeclaredSecretHeadersV1 { names: None };

impl DeclaredSecretHeadersV1 {
    /// The empty declaration: a package that declares no secret header, or a call outside any
    /// package. Policy version two with no addition is exactly policy version one.
    #[must_use]
    pub const fn none() -> &'static Self {
        &NO_DECLARED_SECRET_HEADERS
    }

    /// Builds a declaration from validated names, refusing duplicates and an over-long list.
    ///
    /// # Errors
    ///
    /// Returns [`ContractErrorV1::InvalidSecretHeaderSet`] for a repeated name or more than
    /// [`MAX_DECLARED_SECRET_HEADERS`] names.
    pub fn try_new<I>(headers: I) -> Result<Self, ContractErrorV1>
    where
        I: IntoIterator<Item = DeclaredSecretHeaderV1>,
    {
        let mut names: Vec<DeclaredSecretHeaderV1> = Vec::new();
        for header in headers {
            if names.len() >= MAX_DECLARED_SECRET_HEADERS || names.contains(&header) {
                return Err(ContractErrorV1::InvalidSecretHeaderSet);
            }
            names.push(header);
        }
        Ok(Self { names: (!names.is_empty()).then(|| Arc::from(names.into_boxed_slice())) })
    }

    /// Parses a manifest's `secret_headers` list into a declaration.
    ///
    /// # Errors
    ///
    /// Returns [`ContractErrorV1::InvalidSecretHeaderName`] for a name
    /// [`DeclaredSecretHeaderV1::parse`] refuses, or [`ContractErrorV1::InvalidSecretHeaderSet`]
    /// as [`Self::try_new`] does.
    pub fn try_from_names<'a, I>(names: I) -> Result<Self, ContractErrorV1>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let parsed =
            names.into_iter().map(DeclaredSecretHeaderV1::parse).collect::<Result<Vec<_>, _>>()?;
        Self::try_new(parsed)
    }

    /// Returns whether `name` is declared, comparing ASCII case-insensitively.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.as_slice().iter().any(|declared| declared.as_str().eq_ignore_ascii_case(name))
    }

    /// Returns the declared names, in declaration order.
    #[must_use]
    pub fn as_slice(&self) -> &[DeclaredSecretHeaderV1] {
        self.names.as_deref().unwrap_or(&[])
    }

    /// Returns the number of declared names.
    #[must_use]
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    /// Returns whether nothing is declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }
}

impl fmt::Debug for DeclaredSecretHeadersV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.as_slice()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DeclaredSecretHeaderV1, DeclaredSecretHeadersV1, MAX_DECLARED_SECRET_HEADERS,
        MAX_SECRET_HEADER_NAME_BYTES, UNDECLARABLE_SECRET_HEADER_NAMES,
    };
    use crate::{
        ContractErrorV1, RESERVED_HEADERS, RESPONSE_TRANSCRIPT_DENIED_HEADERS,
        ResponseDiagnosticFieldV1, SecretHeaderV1,
    };

    #[test]
    fn the_undeclarable_list_covers_every_name_south_reserves_elsewhere() {
        let quota_names = [
            "x-ratelimit-limit-tokens",
            "x-ratelimit-remaining-tokens",
            "x-ratelimit-reset-tokens",
            "anthropic-ratelimit-tokens-limit",
            "anthropic-ratelimit-tokens-remaining",
            "anthropic-ratelimit-tokens-reset",
            "anthropic-ratelimit-unified-limit",
            "anthropic-ratelimit-unified-remaining",
            "anthropic-ratelimit-unified-reset",
        ];
        let sources = RESERVED_HEADERS
            .iter()
            .chain(RESPONSE_TRANSCRIPT_DENIED_HEADERS)
            .copied()
            .chain(SecretHeaderV1::ALL.iter().map(SecretHeaderV1::header_name))
            .chain(ResponseDiagnosticFieldV1::ALL.iter().map(|field| field.as_header_name()))
            .chain(quota_names)
            .chain(["accept", "content-type", "content-encoding", "retry-after"]);
        for name in sources {
            assert!(
                UNDECLARABLE_SECRET_HEADER_NAMES.contains(&name),
                "`{name}` is reserved elsewhere but could be declared as a secret header"
            );
            assert_eq!(
                DeclaredSecretHeaderV1::parse(name),
                Err(ContractErrorV1::InvalidSecretHeaderName)
            );
        }
        let mut sorted = UNDECLARABLE_SECRET_HEADER_NAMES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, UNDECLARABLE_SECRET_HEADER_NAMES, "the list stays sorted and unique");
    }

    #[test]
    fn a_name_is_a_bounded_lowercase_token() {
        for valid in ["x-acme-key", "k", "x_acme.key~1", "a!#$%&'*+-.^_`|~z"] {
            assert_eq!(
                DeclaredSecretHeaderV1::parse(valid).map(|h| h.as_str().to_owned()),
                Ok(valid.to_owned())
            );
        }
        let longest = "k".repeat(MAX_SECRET_HEADER_NAME_BYTES);
        assert!(DeclaredSecretHeaderV1::parse(&longest).is_ok());
        let too_long = "k".repeat(MAX_SECRET_HEADER_NAME_BYTES + 1);
        for invalid in
            ["", "X-Acme-Key", "x acme", "x:acme", "x-acme\r\n", "x/acme", "é", too_long.as_str()]
        {
            assert_eq!(
                DeclaredSecretHeaderV1::parse(invalid),
                Err(ContractErrorV1::InvalidSecretHeaderName),
                "{invalid:?} must be refused"
            );
        }
    }

    #[test]
    fn a_declaration_refuses_repeats_and_overflow() {
        let header = |name: &str| DeclaredSecretHeaderV1::parse(name).unwrap();
        assert_eq!(
            DeclaredSecretHeadersV1::try_new([header("x-a"), header("x-a")]),
            Err(ContractErrorV1::InvalidSecretHeaderSet)
        );
        let names: Vec<String> =
            (0..=MAX_DECLARED_SECRET_HEADERS).map(|i| format!("x-{i}")).collect();
        assert_eq!(
            DeclaredSecretHeadersV1::try_from_names(names.iter().map(String::as_str)),
            Err(ContractErrorV1::InvalidSecretHeaderSet)
        );
        let declared = DeclaredSecretHeadersV1::try_from_names(
            names[..MAX_DECLARED_SECRET_HEADERS].iter().map(String::as_str),
        )
        .unwrap();
        assert_eq!(declared.len(), MAX_DECLARED_SECRET_HEADERS);
        assert!(declared.contains("X-0"), "lookups compare without case");
        assert!(!declared.contains("x-8"));
        assert!(DeclaredSecretHeadersV1::none().is_empty());
        assert_eq!(DeclaredSecretHeadersV1::try_new([]).unwrap(), *DeclaredSecretHeadersV1::none());
    }
}
