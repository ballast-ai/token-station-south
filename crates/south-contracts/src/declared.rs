//! Provider instances a package declares in its manifest (B7a,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §10).
//!
//! The closed sets beside these types — [`QueryParameterV1`]'s sanctioned names,
//! [`ProviderQuotaMetadataFieldV1`]'s header names, [`ControlledUserAgentV1`]'s host literals —
//! name provider *instances*, so every new provider that needed a new name had to wait for a south
//! release and a host re-pin. Rule R1 of that record moves the instances into the manifest and
//! keeps only the *mechanisms* closed:
//!
//! - a declared query parameter is a restricted name plus one value syntax from a closed set;
//! - a declared quota header maps one response header name onto one closed normalized field;
//! - a declared user-agent is a manifest value checked against the unchanged value grammar.
//!
//! Gate ① (`south-provider-api`) validates the manifest with the same rules; that crate depends on
//! no other south crate, so the shared lists are repeated there and pinned to these by a test in
//! `south-component-conformance`.

use std::{fmt, sync::Arc};

use crate::{
    ContractErrorV1, ControlledUserAgentV1, HTTP_CONTRACT_VERSION, MAX_QUERY_VALUE_BYTES,
    MAX_USER_AGENT_BYTES, PROVIDER_QUOTA_METADATA_CONTRACT_VERSION, ProviderQuotaMetadataFieldV1,
    QueryParameterV1,
};

// ───────────────── declared query parameters (HTTP contract version ten) ─────────────────

/// The longest declared query parameter name.
pub const MAX_DECLARED_QUERY_NAME_BYTES: usize = 64;

/// The most values an `enum` query syntax may list.
pub const MAX_QUERY_ENUM_VALUES: usize = 16;

/// The most digits a `digits` query value may carry.
pub const MAX_QUERY_DIGITS: usize = 32;

/// Normalized names a declared query parameter may never take.
///
/// A name is normalized by lowercasing it and dropping `-`, `_` and `.`, so `api_key`, `API-Key`
/// and `apikey` are one name. The closed set kept these out structurally — they did not exist in
/// the type; a declared name needs a denylist instead. Values never come from credential
/// resolution regardless of the name, so this is defense in depth against a non-secret
/// configuration field that an operator filled with a secret.
pub const DECLARED_QUERY_DENIED_NAMES: &[&str] =
    &["auth", "authorization", "code", "key", "sig", "token"];

/// Fragments a normalized declared query parameter name may not contain.
pub const DECLARED_QUERY_DENIED_FRAGMENTS: &[&str] = &[
    "accesstoken",
    "apikey",
    "credential",
    "idtoken",
    "passwd",
    "password",
    "refreshtoken",
    "secret",
    "securitytoken",
    "sessiontoken",
    "signature",
];

/// A query value syntax from the closed set a package may declare.
///
/// Every syntax admits only RFC 3986 unreserved bytes, so a value can never split the query
/// (`&`, `=`), open a fragment (`#`), escape (`%`) or be re-encoded by the URL join, and every
/// accepted value is at most [`MAX_QUERY_VALUE_BYTES`] long.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryValueSyntaxV1 {
    /// One to [`MAX_QUERY_DIGITS`] ASCII digits.
    Digits,
    /// One to [`MAX_QUERY_VALUE_BYTES`] unreserved bytes (`A-Z a-z 0-9 - . _ ~`), at least one of
    /// them alphanumeric.
    ///
    /// Narrower than an RFC 9110 token on purpose: a token admits `&`, `#`, `%` and `+`, each of
    /// which changes what a query means.
    Token,
    /// A date, `YYYY-MM-DD`, optionally followed by `-preview`.
    Date,
    /// Exactly one of the listed values. Each value is itself a [`Self::Token`].
    Enum(Vec<String>),
}

impl QueryValueSyntaxV1 {
    /// Checks that the syntax itself is well formed: an `enum` lists one to
    /// [`MAX_QUERY_ENUM_VALUES`] distinct token values.
    ///
    /// # Errors
    ///
    /// Returns [`ContractErrorV1::InvalidQueryDeclaration`] otherwise.
    pub fn validate(&self) -> Result<(), ContractErrorV1> {
        let well_formed = match self {
            Self::Digits | Self::Token | Self::Date => true,
            Self::Enum(values) => {
                (1..=MAX_QUERY_ENUM_VALUES).contains(&values.len())
                    && values.iter().enumerate().all(|(index, value)| {
                        is_query_token(value) && !values[..index].contains(value)
                    })
            }
        };
        if well_formed { Ok(()) } else { Err(ContractErrorV1::InvalidQueryDeclaration) }
    }

    /// Whether `value` has this syntax.
    #[must_use]
    pub fn admits(&self, value: &str) -> bool {
        match self {
            Self::Digits => {
                (1..=MAX_QUERY_DIGITS).contains(&value.len())
                    && value.bytes().all(|byte| byte.is_ascii_digit())
            }
            Self::Token => is_query_token(value),
            Self::Date => {
                let date = value.strip_suffix("-preview").unwrap_or(value);
                date.len() == 10
                    && date.bytes().enumerate().all(|(index, byte)| match index {
                        4 | 7 => byte == b'-',
                        _ => byte.is_ascii_digit(),
                    })
            }
            Self::Enum(values) => values.iter().any(|allowed| allowed == value),
        }
    }
}

/// A token query value: unreserved bytes only, bounded, and not made of separators alone (which
/// would read as a dot segment to anything that re-joins the URL).
fn is_query_token(value: &str) -> bool {
    (1..=MAX_QUERY_VALUE_BYTES).contains(&value.len())
        && value.bytes().all(is_unreserved)
        && value.bytes().any(|byte| byte.is_ascii_alphanumeric())
}

const fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

/// Lowercases a name and drops `-`, `_` and `.`, the form the denylists are written in.
fn normalized_query_name(name: &str) -> String {
    name.bytes()
        .filter(|byte| !matches!(byte, b'-' | b'_' | b'.'))
        .map(|byte| char::from(byte.to_ascii_lowercase()))
        .collect()
}

/// A query parameter a package declared in its manifest, with its validated value syntax.
///
/// Constructed only through [`Self::try_new`], which applies the same rules gate ① applies to the
/// manifest's `query_parameters`. It names a parameter and a syntax, never where a value comes
/// from: values are request data the component wrote into its URL, and there is no conversion
/// from a resolved credential into a query value.
#[derive(Clone, PartialEq, Eq)]
pub struct DeclaredQueryParameterV1 {
    name: Arc<str>,
    syntax: QueryValueSyntaxV1,
}

impl DeclaredQueryParameterV1 {
    /// Validates a declared name and syntax.
    ///
    /// The name is one to [`MAX_DECLARED_QUERY_NAME_BYTES`] unreserved bytes starting with an
    /// ASCII letter. Its normalized form (see [`DECLARED_QUERY_DENIED_NAMES`]) must not be a denied
    /// name, contain a denied fragment, or equal a sanctioned [`QueryParameterV1`] name — one wire
    /// name has one grammar, and the sanctioned names keep theirs.
    ///
    /// # Errors
    ///
    /// Returns [`ContractErrorV1::InvalidQueryDeclaration`] when the name or the syntax is refused.
    pub fn try_new(name: &str, syntax: QueryValueSyntaxV1) -> Result<Self, ContractErrorV1> {
        if !is_declared_query_name(name) {
            return Err(ContractErrorV1::InvalidQueryDeclaration);
        }
        syntax.validate()?;
        Ok(Self { name: Arc::from(name), syntax })
    }

    /// Returns the wire name, exactly as declared.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the declared value syntax.
    #[must_use]
    pub const fn syntax(&self) -> &QueryValueSyntaxV1 {
        &self.syntax
    }
}

impl fmt::Debug for DeclaredQueryParameterV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The name and syntax are public manifest data, not request values.
        formatter
            .debug_struct("DeclaredQueryParameterV1")
            .field("name", &self.name)
            .field("syntax", &self.syntax)
            .finish()
    }
}

/// Whether `name` may be declared as a query parameter name.
fn is_declared_query_name(name: &str) -> bool {
    if name.is_empty()
        || name.len() > MAX_DECLARED_QUERY_NAME_BYTES
        || !name.as_bytes()[0].is_ascii_alphabetic()
        || !name.bytes().all(is_unreserved)
    {
        return false;
    }
    let normalized = normalized_query_name(name);
    !DECLARED_QUERY_DENIED_NAMES.contains(&normalized.as_str())
        && !DECLARED_QUERY_DENIED_FRAGMENTS.iter().any(|fragment| normalized.contains(fragment))
        && !QueryParameterV1::ALL
            .iter()
            .any(|sanctioned| normalized_query_name(sanctioned.wire_name()) == normalized)
}

// ───────────────── declared user-agent (HTTP contract version ten) ─────────────────

/// The frozen user-agent value grammar, shared by [`ControlledUserAgentV1`] and
/// [`DeclaredUserAgentV1`]: non-empty, at most [`MAX_USER_AGENT_BYTES`], every byte printable ASCII
/// including space (`0x20..=0x7E`), and no leading or trailing space.
pub const fn user_agent_grammar_admits(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > MAX_USER_AGENT_BYTES {
        return false;
    }
    if bytes[0] == b' ' || bytes[bytes.len() - 1] == b' ' {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] < 0x20 || bytes[index] > 0x7E {
            return false;
        }
        index += 1;
    }
    true
}

/// A `user-agent` value a package declared for one provider family in its manifest.
///
/// The 2026-08-20 record closed the value channel by type: a [`ControlledUserAgentV1`] can only be
/// built from host program text. Amended 2026-10-02 (§16 Q15): a value may also come from a
/// manifest's per-family `user_agent`, which gate ① has checked against the same grammar. This is
/// the type for that value. Its only constructor is [`Self::from_manifest_value`]; a host must call
/// it with the value of a manifest that passed `ComponentManifestV1::validate`, and with nothing
/// else — not configuration, not request data, not resolver output.
/// `south_component_conformance::DeclaredInstancesV1` is the sanctioned path: it validates the
/// manifest and then reads the value. That provenance is still a discipline claim, as the
/// `'static` one was (a `String::leak` defeats that one): the manifest crate depends on no other
/// south crate, so the type system cannot carry "gate ① passed" across. What this type does
/// guarantee is the grammar, so an accepted value is a valid header value with no CR, LF or other
/// control byte.
///
/// The header name stays fixed and reserved: the value fills the request's single user-agent slot
/// ([`UserAgentV1`]), so "exactly one `user-agent` on the wire" still holds.
#[derive(Clone, PartialEq, Eq)]
pub struct DeclaredUserAgentV1 {
    value: Arc<str>,
}

impl DeclaredUserAgentV1 {
    /// Parses a package's manifest `user_agent` value for one family.
    ///
    /// # Errors
    ///
    /// Returns [`ContractErrorV1::InvalidUserAgentValue`] when the value violates the grammar.
    pub fn from_manifest_value(value: &str) -> Result<Self, ContractErrorV1> {
        if user_agent_grammar_admits(value.as_bytes()) {
            Ok(Self { value: Arc::from(value) })
        } else {
            Err(ContractErrorV1::InvalidUserAgentValue)
        }
    }

    /// Returns the declared value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

impl fmt::Debug for DeclaredUserAgentV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The same shape-only discipline as `ControlledUserAgentV1`.
        formatter
            .debug_struct("DeclaredUserAgentV1")
            .field("contract_version", &HTTP_CONTRACT_VERSION)
            .field("byte_count", &self.value.len())
            .finish_non_exhaustive()
    }
}

/// The single user-agent slot of a request: a host literal or a declared manifest value.
///
/// One slot, so whichever source a request uses, at most one `user-agent` reaches the wire; the
/// ordinary header channel still refuses the name.
///
/// `#[non_exhaustive]`: a downstream match needs a wildcard arm, and [`Self::as_str`] is what a
/// transport needs.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq)]
pub enum UserAgentV1 {
    /// A host program literal.
    Controlled(ControlledUserAgentV1),
    /// A manifest value that passed gate ①.
    Declared(DeclaredUserAgentV1),
}

impl UserAgentV1 {
    /// Returns the value the transport must send.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Controlled(user_agent) => user_agent.as_str(),
            Self::Declared(user_agent) => user_agent.as_str(),
        }
    }
}

impl From<ControlledUserAgentV1> for UserAgentV1 {
    fn from(user_agent: ControlledUserAgentV1) -> Self {
        Self::Controlled(user_agent)
    }
}

impl From<DeclaredUserAgentV1> for UserAgentV1 {
    fn from(user_agent: DeclaredUserAgentV1) -> Self {
        Self::Declared(user_agent)
    }
}

impl fmt::Debug for UserAgentV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Controlled(user_agent) => {
                formatter.debug_tuple("Controlled").field(user_agent).finish()
            }
            Self::Declared(user_agent) => {
                formatter.debug_tuple("Declared").field(user_agent).finish()
            }
        }
    }
}

// ───────────────── declared quota headers (quota metadata contract version two) ─────────────────

/// The longest declared quota header name.
pub const MAX_QUOTA_HEADER_NAME_BYTES: usize = 128;

/// Response headers a package may never declare as a quota header.
///
/// Credential-bearing and hop-by-hop names (the response transcript's exclusions), plus the
/// headers that already have their own typed slot or describe the body rather than the quota.
pub const PROVIDER_QUOTA_HEADER_DENIED_NAMES: &[&str] = &[
    "authorization",
    "connection",
    "content-encoding",
    "content-length",
    "content-type",
    "cookie",
    "keep-alive",
    "location",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "retry-after",
    "set-cookie",
    "set-cookie2",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "www-authenticate",
];

/// Which response header feeds each normalized quota metadata field.
///
/// The field set stays closed ([`ProviderQuotaMetadataFieldV1`]); what a package declares is the
/// header that carries each field on its upstream. The value is captured verbatim — the
/// declaration asserts the header has the field's meaning and value format, which South cannot
/// check.
///
/// [`Self::canonical`] (the [`Default`]) reads each field from its own canonical header, which is
/// exactly what a quota metadata contract version-one transport read. A package that declares
/// `quota_headers` replaces it: only the declared headers are captured.
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderQuotaHeaderMapV1 {
    entries: Arc<[(Box<str>, ProviderQuotaMetadataFieldV1)]>,
}

impl ProviderQuotaHeaderMapV1 {
    /// Every field read from its own canonical header name.
    #[must_use]
    pub fn canonical() -> Self {
        Self {
            entries: ProviderQuotaMetadataFieldV1::ALL
                .iter()
                .map(|field| (Box::from(field.as_header_name()), *field))
                .collect(),
        }
    }

    /// Validates a package's declared quota headers.
    ///
    /// Each name is one to [`MAX_QUOTA_HEADER_NAME_BYTES`] bytes of lowercase letters, digits and
    /// inner hyphens, starting with a letter, and not in [`PROVIDER_QUOTA_HEADER_DENIED_NAMES`].
    /// No name and no field may appear twice: one field has one source, so a capture can never
    /// meet two values for it.
    ///
    /// # Errors
    ///
    /// Returns [`ContractErrorV1::InvalidQuotaHeaderDeclaration`] for an empty declaration or any
    /// refused entry.
    pub fn try_from_iter<'n, I>(entries: I) -> Result<Self, ContractErrorV1>
    where
        I: IntoIterator<Item = (&'n str, ProviderQuotaMetadataFieldV1)>,
    {
        let mut declared: Vec<(Box<str>, ProviderQuotaMetadataFieldV1)> = Vec::new();
        for (name, field) in entries {
            if !is_quota_header_name(name)
                || declared.iter().any(|(seen, seen_field)| **seen == *name || *seen_field == field)
            {
                return Err(ContractErrorV1::InvalidQuotaHeaderDeclaration);
            }
            declared.push((Box::from(name), field));
        }
        if declared.is_empty() {
            return Err(ContractErrorV1::InvalidQuotaHeaderDeclaration);
        }
        Ok(Self { entries: declared.into() })
    }

    /// Returns each captured header name with the field it feeds, in declaration order.
    #[must_use]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, ProviderQuotaMetadataFieldV1)> {
        self.entries.iter().map(|(name, field)| (&**name, *field))
    }

    /// Returns the header that feeds `field`, when one does.
    #[must_use]
    pub fn header_for(&self, field: ProviderQuotaMetadataFieldV1) -> Option<&str> {
        self.iter().find(|(_, candidate)| *candidate == field).map(|(name, _)| name)
    }
}

impl Default for ProviderQuotaHeaderMapV1 {
    fn default() -> Self {
        Self::canonical()
    }
}

impl fmt::Debug for ProviderQuotaHeaderMapV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderQuotaHeaderMapV1")
            .field("contract_version", &PROVIDER_QUOTA_METADATA_CONTRACT_VERSION)
            .field("entries", &self.entries)
            .finish()
    }
}

/// Whether `name` may be declared as a quota header.
fn is_quota_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_QUOTA_HEADER_NAME_BYTES
        && name.as_bytes()[0].is_ascii_lowercase()
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !PROVIDER_QUOTA_HEADER_DENIED_NAMES.contains(&name)
}
