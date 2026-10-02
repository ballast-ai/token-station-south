//! The fixture pack a component package ships, and how it is read.
//!
//! A case is a pair of files inside the directory the manifest's
//! `conformance.fixtures` names:
//!
//! ```text
//! provider.request.chat.input.json
//! provider.request.chat.expected.json
//! ```
//!
//! The name is `provider.<family>.<case>`. `family` selects which component
//! function the input is fed to, and `case` is free but required even when a
//! family has one case: fixture names appear in reports that outlive the pack.
//!
//! A response case may expect a refusal instead of a chat response: its
//! expected file is then exactly `{"error": <ErrorEnvelope>}`. A response case
//! may also carry a sidecar `provider.response.<case>.meta.json` holding
//! `{"usage_pointer": "/usage"}` — where the usage object sits in the upstream
//! body — which `UsageNeverDefaulted` deletes to prove the component refuses
//! rather than reports zeros (B1, `docs/design/2026-09-30-host-zero-vendor-boundary.md`
//! §6.2).
//!
//! The same directory may hold `credential.<family>.<case>` pairs: gate ②'s credential-recipe
//! samples (B4, host-zero-vendor-boundary §3.7). They feed no component function — south's
//! reference recipe interpreter runs them against the manifest's `credentials` section — and
//! their shape, families and coverage rule are documented in `credential_fixture.rs`. Both this
//! loader and the task-v2 loader read them into [`FixturePackV1::credentials`] and its task-v2
//! counterpart; the suites run them only for a manifest that declares `credentials`.
//!
//! Inputs are the Canonical IR, not the provider's wire format. A fixture that
//! could hold a credential would be a way to smuggle one past the type system,
//! so the IR's own boundaries — `SafeHeaders`, `ProviderEndpoint` — re-apply
//! on deserialization here exactly as they do anywhere else.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::credential_fixture::CredentialFixturePackV1;

const MAX_FIXTURE_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// The token every fixture file in a provider-component pack starts with.
pub const FIXTURE_KIND_V1: &str = "provider";

/// Which component function a fixture family exercises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFamilyV1 {
    /// `ProviderConfig` -> `list<ModelCapability>`.
    Capabilities,
    /// `{ provider_config, chat_request }` -> `HttpRequestDescriptor`.
    Request,
    /// `HttpResponseParts` -> `ChatResponse`.
    Response,
    /// `{ chunks: [string] }` (or `chunks_bytes: [[u8]]` for binary dialects)
    /// -> `list<StreamEvent>`.
    Stream,
    /// `HttpResponseParts` -> `ErrorEnvelope`.
    Error,
}

impl ProviderFamilyV1 {
    /// Every family, so `CheckV1::Coverage` can name a missing one.
    pub const ALL: [Self; 5] =
        [Self::Capabilities, Self::Request, Self::Response, Self::Stream, Self::Error];

    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Capabilities => "capabilities",
            Self::Request => "request",
            Self::Response => "response",
            Self::Stream => "stream",
            Self::Error => "error",
        }
    }

    /// Where inside the case's input JSON an unknown field may be injected, as
    /// a JSON Pointer. `None` when the family's input has nowhere
    /// forward-compatible to put one: a stream chunk is opaque bytes, and
    /// `StreamEvent` carries no `extensions` on purpose.
    #[must_use]
    pub const fn unknown_field_pointer(self) -> Option<&'static str> {
        match self {
            Self::Request => Some("/chat_request"),
            Self::Capabilities | Self::Response | Self::Error => Some(""),
            Self::Stream => None,
        }
    }

    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| family.token() == token)
    }
}

/// One input, and what the component must produce from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseV1 {
    /// The full `provider.<family>.<case>` name, as it appears in a report.
    pub name: String,
    pub family: ProviderFamilyV1,
    pub input: Value,
    pub expected: Value,
    /// Where the usage object sits in a response case's upstream body, as a
    /// JSON Pointer into the parsed `body`, read from the optional sidecar
    /// `provider.response.<case>.meta.json` (`{"usage_pointer": "/usage"}`).
    /// `UsageNeverDefaulted` deletes what it points at and requires an error.
    /// Written by the fixture author, so it proves only what the author
    /// pointed at.
    pub usage_pointer: Option<String>,
}

/// Every case a component package ships, in a stable order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixturePackV1 {
    cases: Vec<CaseV1>,
    credentials: CredentialFixturePackV1,
}

impl FixturePackV1 {
    /// Reads every `provider.*.input.json` in `directory` and pairs it with
    /// its expected output. Files not starting with the `provider.` kind are
    /// ignored rather than refused.
    ///
    /// # Errors
    ///
    /// Returns the first [`FixtureErrorV1`] found. A pack that does not load
    /// is not a pack that fails conformance — it is a malformed package, and
    /// the registry has to tell those apart.
    pub fn load(directory: &Path) -> Result<Self, FixtureErrorV1> {
        let entries = fs::read_dir(directory).map_err(|source| FixtureErrorV1::Unreadable {
            path: directory.to_path_buf(),
            detail: source.to_string(),
        })?;

        let mut inputs = Vec::new();
        for entry in entries {
            let path = entry
                .map_err(|source| FixtureErrorV1::Unreadable {
                    path: directory.to_path_buf(),
                    detail: source.to_string(),
                })?
                .path();

            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(stem) = file_name.strip_suffix(".input.json") else {
                continue;
            };
            inputs.push((stem.to_owned(), path));
        }
        inputs.sort_by(|left, right| left.0.cmp(&right.0));

        let mut cases = Vec::new();
        for (stem, input_path) in inputs {
            let Some(family) = family_of(&stem)? else {
                continue;
            };
            let expected_path = directory.join(format!("{stem}.expected.json"));
            let meta_path = directory.join(format!("{stem}.meta.json"));
            let usage_pointer = if meta_path.exists() {
                Some(usage_pointer_of(&read_json(&meta_path, &stem)?, &stem, family)?)
            } else {
                None
            };
            cases.push(CaseV1 {
                input: read_json(&input_path, &stem)?,
                expected: read_json(&expected_path, &stem)?,
                family,
                name: stem,
                usage_pointer,
            });
        }

        Ok(Self { cases, credentials: CredentialFixturePackV1::load(directory)? })
    }

    #[must_use]
    pub const fn from_cases(cases: Vec<CaseV1>) -> Self {
        Self { cases, credentials: CredentialFixturePackV1::from_cases(Vec::new()) }
    }

    /// The same pack with these credential cases.
    #[must_use]
    pub fn with_credentials(mut self, credentials: CredentialFixturePackV1) -> Self {
        self.credentials = credentials;
        self
    }

    /// The `credential.*` cases in the same directory.
    #[must_use]
    pub const fn credentials(&self) -> &CredentialFixturePackV1 {
        &self.credentials
    }

    #[must_use]
    pub fn cases(&self) -> &[CaseV1] {
        &self.cases
    }

    /// Families with no case. Empty is the only passing answer.
    #[must_use]
    pub fn missing_families(&self) -> Vec<ProviderFamilyV1> {
        ProviderFamilyV1::ALL
            .into_iter()
            .filter(|family| !self.cases.iter().any(|case| case.family == *family))
            .collect()
    }
}

/// A case's sidecar metadata: exactly `{"usage_pointer": "/…"}`, on a response
/// case. Anything else is refused, so a misspelt key cannot silently disable
/// the check it was meant to enable.
fn usage_pointer_of(
    meta: &Value,
    case: &str,
    family: ProviderFamilyV1,
) -> Result<String, FixtureErrorV1> {
    let invalid = |detail: &str| FixtureErrorV1::InvalidMeta {
        case: case.to_owned(),
        detail: detail.to_owned(),
    };
    if family != ProviderFamilyV1::Response {
        return Err(invalid("only response cases carry metadata"));
    }
    let Some(map) = meta.as_object().filter(|map| map.len() == 1) else {
        return Err(invalid("metadata must be an object with the single key `usage_pointer`"));
    };
    match map.get("usage_pointer").and_then(Value::as_str) {
        Some(pointer) if pointer.starts_with('/') => Ok(pointer.to_owned()),
        _ => Err(invalid("`usage_pointer` must be a non-root JSON Pointer string")),
    }
}

/// `provider.<family>.<case>` -> the family, or `None` when the file belongs
/// to another pack sharing the directory.
fn family_of(stem: &str) -> Result<Option<ProviderFamilyV1>, FixtureErrorV1> {
    let mut segments = stem.splitn(3, '.');
    let (Some(kind), Some(token), Some(case)) = (segments.next(), segments.next(), segments.next())
    else {
        return Err(FixtureErrorV1::MalformedName { name: stem.to_owned() });
    };

    if kind != FIXTURE_KIND_V1 {
        return Ok(None);
    }
    if case.is_empty() {
        return Err(FixtureErrorV1::MalformedName { name: stem.to_owned() });
    }

    ProviderFamilyV1::parse(token)
        .ok_or_else(|| FixtureErrorV1::UnknownFamily {
            name: stem.to_owned(),
            family: token.to_owned(),
        })
        .map(Some)
}

pub fn read_json(path: &Path, case: &str) -> Result<Value, FixtureErrorV1> {
    let file = fs::File::open(path).map_err(|source| FixtureErrorV1::Unreadable {
        path: path.to_path_buf(),
        detail: source.to_string(),
    })?;
    let metadata = file.metadata().map_err(|source| FixtureErrorV1::Unreadable {
        path: path.to_path_buf(),
        detail: source.to_string(),
    })?;
    if metadata.len() > MAX_FIXTURE_FILE_BYTES {
        return Err(FixtureErrorV1::Unreadable {
            path: path.to_path_buf(),
            detail: format!("fixture exceeds the {MAX_FIXTURE_FILE_BYTES} byte limit"),
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(MAX_FIXTURE_FILE_BYTES + 1).read_to_end(&mut bytes).map_err(|source| {
        FixtureErrorV1::Unreadable { path: path.to_path_buf(), detail: source.to_string() }
    })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FIXTURE_FILE_BYTES {
        return Err(FixtureErrorV1::Unreadable {
            path: path.to_path_buf(),
            detail: format!("fixture exceeds the {MAX_FIXTURE_FILE_BYTES} byte limit"),
        });
    }
    let source = String::from_utf8(bytes).map_err(|source| FixtureErrorV1::Unreadable {
        path: path.to_path_buf(),
        detail: source.to_string(),
    })?;
    serde_json::from_str(&source).map_err(|source| FixtureErrorV1::NotJson {
        case: case.to_owned(),
        detail: source.to_string(),
    })
}

/// Why a fixture pack could not be read.
///
/// Distinct from a conformance failure: a pack that fails the suite is a
/// component that does not work; a pack that will not load is a package that
/// was built wrong, and the registry stores a different reason for each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureErrorV1 {
    Unreadable {
        path: PathBuf,
        detail: String,
    },
    /// Not `provider.<family>.<case>.input.json`, or missing its
    /// `.expected.json`.
    MalformedName {
        name: String,
    },
    UnknownFamily {
        name: String,
        family: String,
    },
    NotJson {
        case: String,
        detail: String,
    },
    /// A `.meta.json` sidecar that is not `{"usage_pointer": "/…"}` on a
    /// response case.
    InvalidMeta {
        case: String,
        detail: String,
    },
}

impl fmt::Display for FixtureErrorV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, detail } => {
                write!(f, "cannot read `{}`: {detail}", path.display())
            }
            Self::MalformedName { name } => {
                write!(f, "fixture `{name}` is not named `provider.<family>.<case>.input.json`")
            }
            Self::UnknownFamily { name, family } => {
                write!(f, "fixture `{name}` names no such family `{family}`")
            }
            Self::NotJson { case, detail } => write!(f, "fixture `{case}` is not JSON: {detail}"),
            Self::InvalidMeta { case, detail } => {
                write!(f, "fixture `{case}` has invalid metadata: {detail}")
            }
        }
    }
}

impl Error for FixtureErrorV1 {}
