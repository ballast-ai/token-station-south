//! Per-family endpoint templates and non-secret configuration keys (B2,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §7.3).
//!
//! A host used to seed each provider type's URL and "required extra fields" from its own table. A
//! family now declares them: `endpoint` is an `https` template whose parameters are the family's
//! `config_schema` keys, each with a value syntax from one closed set. South fills and checks the
//! template, so both hosts build the same origin from the same operator values.
//!
//! Until the channel of §16 Q14 exists, a config key can only feed the endpoint template: a key
//! the component itself would read has no way to reach it, so gate ① refuses a key the template
//! does not use rather than admit a declaration that can never take effect.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ComponentManifestV1, ManifestErrorV1};

/// The longest endpoint template.
const MAX_TEMPLATE_BYTES: usize = 512;
/// The longest `printable_ascii` bound a key may declare.
const MAX_PRINTABLE_ASCII: u16 = 4096;

/// A value syntax from the closed set shared by config keys, endpoint parameters and (with phase
/// B4) credential fields (§3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueSyntaxV1 {
    /// An AWS region: lowercase letters and digits with inner hyphens, at most 32 bytes.
    AwsRegion,
    /// An AWS ARN: `arn:` and at least five more `:`-separated parts.
    AwsArn,
    /// A Google Cloud project id: 6 to 30 lowercase letters, digits and inner hyphens, starting
    /// with a letter.
    GcpProjectId,
    /// An API version date, `YYYY-MM-DD`, optionally followed by `-preview`.
    ApiVersionDate,
    /// One to 32 ASCII digits.
    Digits,
    /// An RFC 9110 token of at most 256 bytes.
    Token,
    /// Printable ASCII (space included) of at most the given number of bytes.
    PrintableAscii(u16),
    /// One of the listed values, exactly.
    Enum(Vec<String>),
}

impl ValueSyntaxV1 {
    /// Whether `value` has this syntax.
    #[must_use]
    pub fn admits(&self, value: &str) -> bool {
        match self {
            Self::AwsRegion => value.len() <= 32 && is_dns_label(value),
            Self::AwsArn => {
                value.len() <= 2048
                    && value.starts_with("arn:")
                    && value.matches(':').count() >= 5
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b":/._+=,@*-".contains(&byte))
            }
            Self::GcpProjectId => {
                (6..=30).contains(&value.len())
                    && value.starts_with(|first: char| first.is_ascii_lowercase())
                    && is_dns_label(value)
            }
            Self::ApiVersionDate => {
                let date = value.strip_suffix("-preview").unwrap_or(value);
                date.len() == 10
                    && date.bytes().enumerate().all(|(index, byte)| match index {
                        4 | 7 => byte == b'-',
                        _ => byte.is_ascii_digit(),
                    })
            }
            Self::Digits => {
                (1..=32).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit())
            }
            Self::Token => {
                (1..=256).contains(&value.len())
                    && value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
                    })
            }
            Self::PrintableAscii(max) => {
                !value.is_empty()
                    && value.len() <= usize::from(*max)
                    && value.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
            }
            Self::Enum(values) => values.iter().any(|allowed| allowed == value),
        }
    }

    /// Whether every value of this syntax can stand inside a DNS label, which is what a parameter
    /// in an endpoint's host part must be: it may choose a label, never the domain.
    pub(crate) fn is_label_safe(&self) -> bool {
        match self {
            Self::AwsRegion | Self::GcpProjectId | Self::Digits => true,
            Self::Enum(values) => values.iter().all(|value| is_dns_label(value)),
            Self::AwsArn | Self::ApiVersionDate | Self::Token | Self::PrintableAscii(_) => false,
        }
    }

    pub(crate) fn is_well_formed(&self) -> bool {
        match self {
            Self::PrintableAscii(max) => (1..=MAX_PRINTABLE_ASCII).contains(max),
            Self::Enum(values) => {
                !values.is_empty()
                    && values.iter().enumerate().all(|(index, value)| {
                        (1..=64).contains(&value.len())
                            && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
                            && !values[..index].contains(value)
                    })
            }
            _ => true,
        }
    }
}

/// One non-secret configuration key of a family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigKeyV1 {
    pub syntax: ValueSyntaxV1,
    #[serde(default)]
    pub required: bool,
    /// One line for the operator form.
    pub description: String,
}

/// Why operator values do not fit a family's declarations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigErrorV1 {
    #[error("`{0}` is not a configuration key of this family")]
    UnknownKey(String),
    #[error("the required configuration key `{0}` has no value")]
    MissingKey(String),
    #[error("the value of `{0}` does not have the declared syntax")]
    InvalidValue(String),
}

/// A lowercase DNS label: letters, digits and inner hyphens, at most 63 bytes.
fn is_dns_label(value: &str) -> bool {
    (1..=63).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part<'t> {
    Literal(&'t str),
    Param(&'t str),
}

/// Splits a template into literals and `{name}` parameters, or `None` when the braces are not
/// well formed.
fn parts(template: &str) -> Option<Vec<Part<'_>>> {
    let mut parts = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        if rest[..open].contains('}') {
            return None;
        }
        if open > 0 {
            parts.push(Part::Literal(&rest[..open]));
        }
        let close = rest[open..].find('}')? + open;
        let name = &rest[open + 1..close];
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return None;
        }
        parts.push(Part::Param(name));
        rest = &rest[close + 1..];
    }
    if rest.contains('}') {
        return None;
    }
    if !rest.is_empty() {
        parts.push(Part::Literal(rest));
    }
    Some(parts)
}

/// Percent-encodes a path parameter as one segment (RFC 3986 `pchar`).
fn encode_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

/// The parameters of an `https` endpoint template, each with whether it sits in the host part, or
/// why the template is refused.
///
/// Shared by family endpoints (§7.3) and credential-recipe endpoints (§3.3, §3.4): the domain is
/// fixed — the host part ends with a literal of at least two labels, so a parameter may choose a
/// label but never the domain — and there is no port, query, fragment, userinfo or escape.
pub fn template_params(template: &str) -> Result<Vec<(&str, bool)>, &'static str> {
    let rest = template
        .strip_prefix("https://")
        .filter(|_| template.len() <= MAX_TEMPLATE_BYTES)
        .filter(|rest| {
            rest.bytes().all(|byte| byte.is_ascii_graphic() && !b"?#@\\%".contains(&byte))
        })
        .ok_or(
            "the endpoint must be an https template without query, fragment, userinfo or escapes",
        )?;
    let (authority, path) = rest.find('/').map_or((rest, ""), |slash| rest.split_at(slash));
    let authority_parts = parts(authority).ok_or("the endpoint's braces are malformed")?;
    let path_parts = parts(path).ok_or("the endpoint's braces are malformed")?;
    match authority_parts.last() {
        Some(Part::Literal(suffix))
            if suffix.trim_start_matches('.').contains('.')
                && suffix.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte)
                }) => {}
        _ => return Err("the endpoint's host must end in a fixed domain without a port"),
    }
    Ok(authority_parts
        .iter()
        .map(|part| (part, true))
        .chain(path_parts.iter().map(|part| (part, false)))
        .filter_map(|(part, in_host)| match part {
            Part::Param(name) => Some((*name, in_host)),
            Part::Literal(_) => None,
        })
        .collect())
}

impl ComponentManifestV1 {
    pub(crate) fn validate_endpoints(&self) -> Result<(), ManifestErrorV1> {
        for (family, keys) in &self.config_schema {
            let invalid = |detail: &str| ManifestErrorV1::InvalidEndpoint {
                family: family.clone(),
                detail: detail.to_owned(),
            };
            if !self.providers.contains(family) {
                return Err(invalid("config_schema names a family the manifest does not declare"));
            }
            for (key, declaration) in keys {
                if key.is_empty()
                    || key.len() > 64
                    || !key.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                    })
                {
                    return Err(invalid("a config key name is not lowercase snake_case"));
                }
                if !declaration.syntax.is_well_formed() {
                    return Err(invalid("a config key's syntax is malformed"));
                }
            }
        }
        for (family, template) in &self.endpoint {
            let invalid = |detail: &str| ManifestErrorV1::InvalidEndpoint {
                family: family.clone(),
                detail: detail.to_owned(),
            };
            if !self.providers.contains(family) {
                return Err(invalid("endpoint names a family the manifest does not declare"));
            }
            let keys = self.config_schema.get(family);
            let params = template_params(template).map_err(invalid)?;
            let mut used = Vec::new();
            for (name, in_host) in params {
                let Some(declaration) = keys.and_then(|keys| keys.get(name)) else {
                    return Err(invalid("an endpoint parameter is not a config key of the family"));
                };
                if !declaration.required {
                    return Err(invalid("an endpoint parameter must be a required config key"));
                }
                if in_host && !declaration.syntax.is_label_safe() {
                    return Err(invalid("a host parameter's syntax must fit inside a DNS label"));
                }
                used.push(name);
            }
            if let Some(unused) =
                keys.and_then(|keys| keys.keys().find(|key| !used.contains(&key.as_str())))
            {
                return Err(ManifestErrorV1::InvalidEndpoint {
                    family: family.clone(),
                    detail: format!(
                        "config key `{unused}` is not used by the endpoint; until a channel into the component exists (§16 Q14), a config key can only feed the endpoint"
                    ),
                });
            }
        }
        if let Some(family) =
            self.config_schema.keys().find(|family| !self.endpoint.contains_key(*family))
        {
            return Err(ManifestErrorV1::InvalidEndpoint {
                family: family.clone(),
                detail: "config_schema without an endpoint has nothing to feed".to_owned(),
            });
        }
        Ok(())
    }

    /// Checks operator values for `family` against its `config_schema`: no unknown key, every
    /// required key present, every value of its declared syntax.
    ///
    /// # Errors
    ///
    /// Returns the first [`ConfigErrorV1`] found.
    pub fn validate_config_values(
        &self,
        family: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<(), ConfigErrorV1> {
        let empty = BTreeMap::new();
        let keys = self.config_schema.get(family).unwrap_or(&empty);
        if let Some(unknown) = values.keys().find(|key| !keys.contains_key(*key)) {
            return Err(ConfigErrorV1::UnknownKey(unknown.clone()));
        }
        for (key, declaration) in keys {
            match values.get(key) {
                None if declaration.required => return Err(ConfigErrorV1::MissingKey(key.clone())),
                Some(value) if !declaration.syntax.admits(value) => {
                    return Err(ConfigErrorV1::InvalidValue(key.clone()));
                }
                None | Some(_) => {}
            }
        }
        Ok(())
    }

    /// The endpoint `family` declares, filled from validated operator values; `None` when the
    /// family declares no endpoint. Host parameters are substituted as validated; path parameters
    /// are encoded as one segment.
    ///
    /// # Errors
    ///
    /// Returns the [`ConfigErrorV1`] that made the values unusable.
    pub fn fill_endpoint(
        &self,
        family: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<Option<String>, ConfigErrorV1> {
        let Some(template) = self.endpoint.get(family) else {
            return Ok(None);
        };
        self.validate_config_values(family, values)?;
        let authority_end = template["https://".len()..]
            .find('/')
            .map_or(template.len(), |slash| slash + "https://".len());
        let mut filled = String::with_capacity(template.len());
        let mut offset = 0;
        for part in parts(template).unwrap_or_default() {
            match part {
                Part::Literal(literal) => {
                    filled.push_str(literal);
                    offset += literal.len();
                }
                Part::Param(name) => {
                    let value = values
                        .get(name)
                        .ok_or_else(|| ConfigErrorV1::MissingKey(name.to_owned()))?;
                    if offset < authority_end {
                        filled.push_str(value);
                    } else {
                        filled.push_str(&encode_segment(value));
                    }
                    offset += name.len() + 2;
                }
            }
        }
        Ok(Some(filled))
    }

    /// Whether an operator-entered `base_url` is the family's endpoint for some admissible values:
    /// every literal matches and every parameter's text has its declared syntax. `true` when the
    /// family declares no endpoint (the operator's URL is then the only anchor, as today).
    #[must_use]
    pub fn endpoint_admits(&self, family: &str, base_url: &str) -> bool {
        let Some(template) = self.endpoint.get(family) else {
            return true;
        };
        let Some(template_parts) = parts(template) else {
            return false;
        };
        let keys = self.config_schema.get(family);
        let syntax = |name: &str| keys.and_then(|keys| keys.get(name)).map(|key| &key.syntax);
        matches_parts(&template_parts, base_url.trim_end_matches('/'), &syntax)
    }
}

fn matches_parts<'s>(
    parts: &[Part<'_>],
    text: &str,
    syntax: &dyn Fn(&str) -> Option<&'s ValueSyntaxV1>,
) -> bool {
    match parts.split_first() {
        None => text.is_empty(),
        Some((Part::Literal(literal), rest)) => {
            text.strip_prefix(literal).is_some_and(|text| matches_parts(rest, text, syntax))
        }
        Some((Part::Param(name), rest)) => {
            let Some(declared) = syntax(name) else {
                return false;
            };
            // A parameter's text never spans a label or a segment boundary.
            (1..=text.len()).filter(|end| text.is_char_boundary(*end)).any(|end| {
                let candidate = &text[..end];
                !candidate.contains(['/', '.'])
                    && declared.admits(candidate)
                    && matches_parts(rest, &text[end..], syntax)
            })
        }
    }
}
