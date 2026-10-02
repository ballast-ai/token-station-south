//! Provider instances a package declares: query parameters, quota headers and per-family
//! user-agents (B7a, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §10).
//!
//! Rule R1 of that record keeps mechanisms closed and moves instances into the manifest, so a new
//! provider with a new query name, rate-limit header or client user-agent is a new package rather
//! than a south release and a host re-pin. Gate ① checks each declaration here; the host turns it
//! into the `south-contracts` type (`DeclaredQueryParameterV1`, `ProviderQuotaHeaderMapV1`,
//! `DeclaredUserAgentV1`), whose constructors apply the same rules.
//!
//! This crate depends on no other south crate, so the lists and the user-agent grammar those
//! constructors use are repeated below; a `south-component-conformance` test pins each copy to
//! its `south-contracts` original.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{ComponentManifestV1, ManifestErrorV1, PROVIDER_WORLD, ValueSyntaxV1, WorldSchemaV1};

/// The fixed query names of `south_contracts::QueryParameterV1::ALL`, which a package may not
/// declare again: one wire name has one grammar.
pub const SANCTIONED_QUERY_NAMES: &[&str] =
    &["api-version", "alt", "GroupId", "task_id", "file_id"];

/// Normalized names a declared query parameter may never take (lowercased, `-`, `_` and `.`
/// dropped); `south_contracts::DECLARED_QUERY_DENIED_NAMES`.
pub const DECLARED_QUERY_DENIED_NAMES: &[&str] =
    &["auth", "authorization", "code", "key", "sig", "token"];

/// Fragments a normalized declared query parameter name may not contain;
/// `south_contracts::DECLARED_QUERY_DENIED_FRAGMENTS`.
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

/// The longest declared query parameter name.
pub const MAX_DECLARED_QUERY_NAME_BYTES: usize = 64;

/// The most query parameters one package may declare.
pub const MAX_DECLARED_QUERY_PARAMETERS: usize = 16;

/// The most values an `enum` query syntax may list.
pub const MAX_QUERY_ENUM_VALUES: usize = 16;

/// The longest query value (`south_contracts::MAX_QUERY_VALUE_BYTES`).
const MAX_QUERY_VALUE_BYTES: usize = 64;

/// The closed quota metadata fields, each named by its canonical header name
/// (`south_contracts::ProviderQuotaMetadataFieldV1::ALL`).
pub const QUOTA_METADATA_FIELDS: &[&str] = &[
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

/// Response headers a package may never declare as a quota header;
/// `south_contracts::PROVIDER_QUOTA_HEADER_DENIED_NAMES`.
pub const QUOTA_HEADER_DENIED_NAMES: &[&str] = &[
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

/// The longest declared quota header name.
pub const MAX_QUOTA_HEADER_NAME_BYTES: usize = 128;

/// The longest user-agent value (`south_contracts::MAX_USER_AGENT_BYTES`).
pub const MAX_USER_AGENT_BYTES: usize = 256;

/// A query value syntax from the closed set of §10.
///
/// Written like [`ValueSyntaxV1`] (`"digits"`, `"token"`, `"date"`, `{"enum": [...]}`) and, where it
/// fits, checked by it: `digits` is [`ValueSyntaxV1::Digits`], `date` is
/// [`ValueSyntaxV1::ApiVersionDate`], and `enum` is [`ValueSyntaxV1::Enum`] whose values must also
/// be query tokens. `token` is not [`ValueSyntaxV1::Token`]: an RFC 9110 token admits `&`, `#`,
/// `%` and `+`, each of which changes what a query means, so a query token is RFC 3986 unreserved
/// bytes only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryValueSyntaxV1 {
    /// One to 32 ASCII digits.
    Digits,
    /// One to 64 unreserved bytes (`A-Z a-z 0-9 - . _ ~`), at least one alphanumeric.
    Token,
    /// `YYYY-MM-DD`, optionally followed by `-preview`.
    Date,
    /// Exactly one of the listed tokens.
    Enum(Vec<String>),
}

impl QueryValueSyntaxV1 {
    /// The shared syntax this one is checked by, when one fits.
    #[must_use]
    pub fn shared(&self) -> Option<ValueSyntaxV1> {
        match self {
            Self::Digits => Some(ValueSyntaxV1::Digits),
            Self::Date => Some(ValueSyntaxV1::ApiVersionDate),
            Self::Enum(values) => Some(ValueSyntaxV1::Enum(values.clone())),
            Self::Token => None,
        }
    }

    /// Whether `value` has this syntax.
    #[must_use]
    pub fn admits(&self, value: &str) -> bool {
        self.shared().map_or_else(|| is_query_token(value), |shared| shared.admits(value))
    }

    fn is_well_formed(&self) -> bool {
        match self {
            Self::Enum(values) => {
                values.len() <= MAX_QUERY_ENUM_VALUES
                    && ValueSyntaxV1::Enum(values.clone()).is_well_formed()
                    && values.iter().all(|value| is_query_token(value))
            }
            Self::Digits | Self::Token | Self::Date => true,
        }
    }
}

/// One query parameter a package declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryParameterDeclarationV1 {
    /// The wire name, exactly as it goes on the wire.
    pub name: String,
    pub syntax: QueryValueSyntaxV1,
}

/// One response header a package's transport captures into a closed quota field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaHeaderDeclarationV1 {
    /// The lowercase response header name.
    pub header: String,
    /// The field it feeds, named by the field's canonical header name ([`QUOTA_METADATA_FIELDS`]).
    pub field: String,
}

/// Whether `value` satisfies the controlled user-agent value grammar: non-empty, at most
/// [`MAX_USER_AGENT_BYTES`], printable ASCII including space, and no leading or trailing space.
///
/// The grammar of `south_contracts::ControlledUserAgentV1`, unchanged (§16 Q15).
#[must_use]
pub fn is_user_agent_value(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_USER_AGENT_BYTES
        && bytes.first() != Some(&b' ')
        && bytes.last() != Some(&b' ')
        && bytes.iter().all(|byte| (0x20..=0x7e).contains(byte))
}

const fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn is_query_token(value: &str) -> bool {
    (1..=MAX_QUERY_VALUE_BYTES).contains(&value.len())
        && value.bytes().all(is_unreserved)
        && value.bytes().any(|byte| byte.is_ascii_alphanumeric())
}

fn normalized_query_name(name: &str) -> String {
    name.bytes()
        .filter(|byte| !matches!(byte, b'-' | b'_' | b'.'))
        .map(|byte| char::from(byte.to_ascii_lowercase()))
        .collect()
}

/// Why a query name is refused, or `None` when it may be declared.
fn query_name_refusal(name: &str) -> Option<&'static str> {
    if name.is_empty()
        || name.len() > MAX_DECLARED_QUERY_NAME_BYTES
        || !name.as_bytes()[0].is_ascii_alphabetic()
        || !name.bytes().all(is_unreserved)
    {
        return Some(
            "a name is 1 to 64 unreserved bytes (letters, digits, `-`, `.`, `_`, `~`) starting with a letter",
        );
    }
    let normalized = normalized_query_name(name);
    if DECLARED_QUERY_DENIED_NAMES.contains(&normalized.as_str())
        || DECLARED_QUERY_DENIED_FRAGMENTS.iter().any(|fragment| normalized.contains(fragment))
    {
        return Some(
            "the name is reserved: it reads as a credential, and a query never carries one",
        );
    }
    if SANCTIONED_QUERY_NAMES
        .iter()
        .any(|sanctioned| normalized_query_name(sanctioned) == normalized)
    {
        return Some("the name is a fixed sanctioned parameter, which keeps its own grammar");
    }
    None
}

fn is_quota_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_QUOTA_HEADER_NAME_BYTES
        && name.as_bytes()[0].is_ascii_lowercase()
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

impl ComponentManifestV1 {
    /// Gate ① for the B7a instance declarations.
    pub(crate) fn validate_instances(&self, world: &WorldSchemaV1) -> Result<(), ManifestErrorV1> {
        if world.world != PROVIDER_WORLD {
            let declared = [
                ("query_parameters", !self.query_parameters.is_empty()),
                ("quota_headers", !self.quota_headers.is_empty()),
                ("user_agent", !self.user_agent.is_empty()),
            ];
            if let Some((field, _)) = declared.iter().find(|(_, present)| *present) {
                return Err(ManifestErrorV1::InstanceIsAProviderWorldDeclaration(
                    (*field).to_owned(),
                ));
            }
            return Ok(());
        }
        self.validate_query_parameters()?;
        self.validate_quota_headers()?;
        self.validate_user_agents()
    }

    fn validate_query_parameters(&self) -> Result<(), ManifestErrorV1> {
        if self.query_parameters.len() > MAX_DECLARED_QUERY_PARAMETERS {
            return Err(ManifestErrorV1::InvalidQueryParameter {
                name: String::new(),
                detail: format!("at most {MAX_DECLARED_QUERY_PARAMETERS} query parameters"),
            });
        }
        let mut seen = BTreeSet::new();
        for declaration in &self.query_parameters {
            let invalid = |detail: &str| ManifestErrorV1::InvalidQueryParameter {
                name: declaration.name.clone(),
                detail: detail.to_owned(),
            };
            if let Some(detail) = query_name_refusal(&declaration.name) {
                return Err(invalid(detail));
            }
            if !declaration.syntax.is_well_formed() {
                return Err(invalid(
                    "an enum lists 1 to 16 distinct values, each a token of unreserved bytes",
                ));
            }
            if !seen.insert(declaration.name.as_str()) {
                return Err(invalid("the name is declared twice"));
            }
        }
        Ok(())
    }

    fn validate_quota_headers(&self) -> Result<(), ManifestErrorV1> {
        let mut headers = BTreeSet::new();
        let mut fields = BTreeSet::new();
        for declaration in &self.quota_headers {
            let invalid = |detail: &str| ManifestErrorV1::InvalidQuotaHeader {
                header: declaration.header.clone(),
                detail: detail.to_owned(),
            };
            if !is_quota_header_name(&declaration.header) {
                return Err(invalid(
                    "a header name is 1 to 128 lowercase letters, digits and inner hyphens, starting with a letter",
                ));
            }
            if QUOTA_HEADER_DENIED_NAMES.contains(&declaration.header.as_str()) {
                return Err(invalid(
                    "the header is reserved: it carries a credential, framing, or its own typed slot",
                ));
            }
            // A declared secret header is dropped from every response transcript; reading the
            // same name into quota metadata would hand a host the value the transcript hides.
            if self.secret_headers.iter().any(|name| name.eq_ignore_ascii_case(&declaration.header))
            {
                return Err(invalid("the header is declared in `secret_headers`"));
            }
            if !QUOTA_METADATA_FIELDS.contains(&declaration.field.as_str()) {
                return Err(invalid("the field is not one of the closed quota metadata fields"));
            }
            if !headers.insert(declaration.header.as_str()) {
                return Err(invalid("the header is declared twice"));
            }
            if !fields.insert(declaration.field.as_str()) {
                return Err(invalid("another header already feeds this field"));
            }
        }
        Ok(())
    }

    fn validate_user_agents(&self) -> Result<(), ManifestErrorV1> {
        for (family, value) in &self.user_agent {
            let invalid = |detail: &str| ManifestErrorV1::InvalidUserAgent {
                family: family.clone(),
                detail: detail.to_owned(),
            };
            if !self.providers.contains(family) {
                return Err(invalid("names a family the manifest does not declare"));
            }
            if !is_user_agent_value(value) {
                return Err(invalid(
                    "the value must be 1 to 256 bytes of printable ASCII without a leading or trailing space",
                ));
            }
        }
        Ok(())
    }

    /// The user-agent `family` declares, if any. A host passes it, and only it, to
    /// `south_contracts::DeclaredUserAgentV1::from_manifest_value` once [`Self::validate`] passed.
    #[must_use]
    pub fn user_agent_for(&self, family: &str) -> Option<&str> {
        self.user_agent.get(family).map(String::as_str)
    }
}
