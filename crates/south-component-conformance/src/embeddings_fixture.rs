//! Fixture packs for the embeddings world, beside the provider and task worlds'.
//!
//! Same file convention — `embeddings.<family>.<case>.{input,expected}.json` — and a separate
//! kind for the reason the task packs give: the worlds version independently. A response case may
//! carry the provider pack's sidecar, `embeddings.response.<case>.meta.json` holding
//! `{"usage_pointer": "/usage"}`, which `UsageNeverDefaulted` deletes. A directory may hold other
//! kinds; this loader ignores them, and reads the `credential.*` cases beside it.
//!
//! The shapes (record §10):
//!
//! - `request`: `{"provider_config": ProviderConfig, "request": EmbeddingsRequestV1}` ->
//!   the prepared JSON, or `{"error": ErrorEnvelope}`.
//! - `response`: `{"request_case": "embeddings.request.<case>", "response": HttpResponseParts}`,
//!   where the body is the raw upstream 2xx with its vectors. The suite builds the named request
//!   case, extracts and erases the vectors with its locator, hands the skeleton and the parse
//!   context to the component, and runs the host's consistency checks. Expected: the parsed facts,
//!   `{"error": ErrorEnvelope}` for a component refusal, or `{"host_refusal": "<word>"}` when the
//!   host's extraction or checks refuse.
//! - `error`: `HttpResponseParts` (a non-2xx) -> `{"outcome": ..., "error": ErrorEnvelope}`, or
//!   `{"error": ErrorEnvelope}` for a component refusal.

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::credential_fixture::CredentialFixturePackV1;
use crate::fixture::{FixtureErrorV1, read_json};

/// The filename prefix an embeddings fixture carries.
pub const EMBEDDINGS_FIXTURE_KIND_V1: &str = "embeddings";

/// Which embeddings-component function a fixture family exercises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingsFamilyV1 {
    /// `build-embeddings-request`.
    Request,
    /// `parse-embeddings-response`, through the host's extraction and checks.
    Response,
    /// `map-provider-error`.
    Error,
}

impl EmbeddingsFamilyV1 {
    /// Every family.
    pub const ALL: [Self; 3] = [Self::Request, Self::Response, Self::Error];

    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Response => "response",
            Self::Error => "error",
        }
    }

    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| family.token() == token)
    }
}

/// One embeddings fixture: an input, the output it must produce, and its family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingsCaseV1 {
    /// The full `embeddings.<family>.<case>` name, as it appears in a report.
    pub name: String,
    pub family: EmbeddingsFamilyV1,
    pub input: Value,
    pub expected: Value,
    /// Where the usage object sits in a response case's upstream body, from the sidecar.
    pub usage_pointer: Option<String>,
}

/// Every embeddings case found in one directory, in name order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmbeddingsFixturePackV1 {
    cases: Vec<EmbeddingsCaseV1>,
    credentials: CredentialFixturePackV1,
}

impl EmbeddingsFixturePackV1 {
    /// Reads every `embeddings.*.input.json` in `directory` and pairs it with its expected
    /// output and, for a response case, its optional sidecar.
    ///
    /// # Errors
    ///
    /// Returns the first [`FixtureErrorV1`] found: a pack that does not load is a malformed
    /// package, not a component that fails conformance.
    pub fn load(directory: &Path) -> Result<Self, FixtureErrorV1> {
        let unreadable =
            |detail: String| FixtureErrorV1::Unreadable { path: directory.to_path_buf(), detail };
        let mut stems = Vec::new();
        for entry in fs::read_dir(directory).map_err(|source| unreadable(source.to_string()))? {
            let path = entry.map_err(|source| unreadable(source.to_string()))?.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if let Some(stem) = name.strip_suffix(".input.json") {
                stems.push(stem.to_owned());
            }
        }
        stems.sort();

        let mut cases = Vec::new();
        for stem in stems {
            let Some(family) = family_of(&stem)? else {
                continue;
            };
            let meta_path = directory.join(format!("{stem}.meta.json"));
            let usage_pointer = if meta_path.exists() {
                Some(usage_pointer_of(&read_json(&meta_path, &stem)?, &stem, family)?)
            } else {
                None
            };
            cases.push(EmbeddingsCaseV1 {
                input: read_json(&directory.join(format!("{stem}.input.json")), &stem)?,
                expected: read_json(&directory.join(format!("{stem}.expected.json")), &stem)?,
                family,
                name: stem,
                usage_pointer,
            });
        }
        Ok(Self { cases, credentials: CredentialFixturePackV1::load(directory)? })
    }

    /// A pack of these cases, without credential cases.
    #[must_use]
    pub fn from_cases(cases: Vec<EmbeddingsCaseV1>) -> Self {
        Self { cases, credentials: CredentialFixturePackV1::default() }
    }

    #[must_use]
    pub fn cases(&self) -> &[EmbeddingsCaseV1] {
        &self.cases
    }

    /// The `credential.*` cases in the same directory (`credential_fixture.rs`).
    #[must_use]
    pub const fn credentials(&self) -> &CredentialFixturePackV1 {
        &self.credentials
    }

    /// The same pack with these credential cases.
    #[must_use]
    pub fn with_credentials(mut self, credentials: CredentialFixturePackV1) -> Self {
        self.credentials = credentials;
        self
    }

    /// The case with this full name.
    #[must_use]
    pub fn case(&self, name: &str) -> Option<&EmbeddingsCaseV1> {
        self.cases.iter().find(|case| case.name == name)
    }
}

/// A response case's sidecar: exactly `{"usage_pointer": "/…"}`. Anything else is refused, so a
/// misspelt key cannot silently disable the check it was meant to enable.
fn usage_pointer_of(
    meta: &Value,
    case: &str,
    family: EmbeddingsFamilyV1,
) -> Result<String, FixtureErrorV1> {
    let invalid = |detail: &str| FixtureErrorV1::InvalidMeta {
        case: case.to_owned(),
        detail: detail.to_owned(),
    };
    if family != EmbeddingsFamilyV1::Response {
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

/// `embeddings.<family>.<case>` -> the family, or `None` when the file belongs to another pack
/// sharing the directory.
fn family_of(stem: &str) -> Result<Option<EmbeddingsFamilyV1>, FixtureErrorV1> {
    let mut segments = stem.splitn(3, '.');
    let (Some(kind), Some(token), Some(case)) = (segments.next(), segments.next(), segments.next())
    else {
        return Err(FixtureErrorV1::MalformedName { name: stem.to_owned() });
    };
    if kind != EMBEDDINGS_FIXTURE_KIND_V1 {
        return Ok(None);
    }
    if case.is_empty() {
        return Err(FixtureErrorV1::MalformedName { name: stem.to_owned() });
    }
    EmbeddingsFamilyV1::parse(token)
        .ok_or_else(|| FixtureErrorV1::UnknownFamily {
            name: stem.to_owned(),
            family: token.to_owned(),
        })
        .map(Some)
}
