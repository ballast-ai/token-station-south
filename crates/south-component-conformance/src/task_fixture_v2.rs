//! Fixture packs for the task world, beside the provider world's.
//!
//! Same file convention — `task-v2.<family>.<case>.{input,expected}.json` — and
//! deliberately a separate kind rather than a family added to the provider
//! pack: the two worlds version independently, and a pack that loaded both
//! would make a task fixture's validity depend on a chat-side release.
//!
//! A directory may hold both kinds. Each loader ignores the other's files
//! rather than refusing them, exactly as the provider loader already does.

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::credential_fixture::CredentialFixturePackV1;
use crate::fixture::FixtureErrorV1;

/// The filename prefix a task fixture carries.
pub const TASK_FIXTURE_KIND_V2: &str = "task-v2";

/// Which task-component function a fixture family exercises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFamilyV2 {
    /// `{ provider_config, request, minted }` -> `PreparedTaskV2`.
    Prepare,
    /// `HttpResponseParts` -> `SubmitOutcomeV2`.
    Created,
    /// `{ provider_config, upstream_model, upstream_task_id, locator }` ->
    /// `HttpRequestDescriptor`.
    Observe,
    /// `HttpResponseParts` -> `TaskObservationV2`.
    Observation,
    /// `{ observation, context, fetched? }` -> the success body.
    Render,
    /// Artifact lookup descriptor or explicit absence.
    Artifact,
    /// `TaskObservationV2` (a failure) -> `ErrorEnvelope`.
    Failure,
}

impl TaskFamilyV2 {
    /// Every family, so a coverage check can name a missing one.
    pub const ALL: [Self; 7] = [
        Self::Prepare,
        Self::Created,
        Self::Observe,
        Self::Observation,
        Self::Render,
        Self::Artifact,
        Self::Failure,
    ];

    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Prepare => "prepare",
            Self::Created => "created",
            Self::Observe => "observe",
            Self::Observation => "observation",
            Self::Render => "render",
            Self::Artifact => "artifact",
            Self::Failure => "failure",
        }
    }

    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| family.token() == token)
    }
}

/// One task fixture: an input, the output it must produce, and its family.
#[derive(Debug, Clone)]
pub struct TaskCaseV2 {
    pub name: String,
    pub family: TaskFamilyV2,
    pub input: Value,
    pub expected: Value,
}

/// Where a success fixture's usage sits in its upstream body, for task contract 8's usage checks.
///
/// The sidecar is `task-v2.observation.<case>.meta.json`, exactly `{"usage_pointer": "/…"}` plus, optionally,
/// `"bucket_pointers": {"<bucket>": "/…", …}` naming the wire member each required bucket is read
/// from. The suite deletes each and requires `unknown` (`UsageNeverDefaulted`,
/// `MissingMeterIsNotZero`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskUsageMetaV2 {
    /// Where the usage object is.
    pub usage_pointer: String,
    /// Where each required bucket's wire member is.
    pub bucket_pointers: std::collections::BTreeMap<String, String>,
}

/// Every task case found in one directory.
#[derive(Debug, Clone, Default)]
pub struct TaskFixturePackV2 {
    cases: Vec<TaskCaseV2>,
    credentials: CredentialFixturePackV1,
    usage_meta: std::collections::BTreeMap<String, TaskUsageMetaV2>,
}

impl TaskFixturePackV2 {
    /// Reads every `task-v2.*.input.json` in `directory` and pairs it with its
    /// expected output.
    ///
    /// # Errors
    ///
    /// Returns the first [`FixtureErrorV1`] found. A pack that does not load
    /// is a malformed package, not a component that fails conformance, and
    /// the registry has to tell those apart.
    pub fn load(directory: &Path) -> Result<Self, FixtureErrorV1> {
        let entries = fs::read_dir(directory).map_err(|source| FixtureErrorV1::Unreadable {
            path: directory.to_path_buf(),
            detail: source.to_string(),
        })?;

        let mut cases = Vec::new();
        let mut names: Vec<String> = Vec::new();
        for entry in entries {
            let path = entry
                .map_err(|source| FixtureErrorV1::Unreadable {
                    path: directory.to_path_buf(),
                    detail: source.to_string(),
                })?
                .path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(stem) = name.strip_suffix(".input.json") else {
                continue;
            };
            names.push(stem.to_owned());
        }
        names.sort();

        let mut usage_meta = std::collections::BTreeMap::new();
        for stem in names {
            let Some(family) = family_of(&stem)? else {
                continue;
            };
            let input = read_json(&directory.join(format!("{stem}.input.json")))?;
            let expected = read_json(&directory.join(format!("{stem}.expected.json")))?;
            let meta_path = directory.join(format!("{stem}.meta.json"));
            if meta_path.exists() {
                let meta = usage_meta_of(&stem, family, &read_json(&meta_path)?)?;
                usage_meta.insert(stem.clone(), meta);
            }
            cases.push(TaskCaseV2 { name: stem, family, input, expected });
        }
        Ok(Self { cases, credentials: CredentialFixturePackV1::load(directory)?, usage_meta })
    }

    /// A case's usage sidecar, when it has one (task contract 8).
    #[must_use]
    pub fn usage_meta(&self, case: &str) -> Option<&TaskUsageMetaV2> {
        self.usage_meta.get(case)
    }

    #[must_use]
    pub fn cases(&self) -> &[TaskCaseV2] {
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

    /// Families with no case. Empty is the only passing answer.
    #[must_use]
    pub fn missing_families(&self) -> Vec<TaskFamilyV2> {
        TaskFamilyV2::ALL
            .into_iter()
            .filter(|family| !self.cases.iter().any(|case| case.family == *family))
            .collect()
    }
}

/// `task-v2.<family>.<case>` -> the family, or `None` when the file belongs to
/// another pack sharing the directory.
fn family_of(stem: &str) -> Result<Option<TaskFamilyV2>, FixtureErrorV1> {
    let mut segments = stem.splitn(3, '.');
    let (Some(kind), Some(token), Some(case)) = (segments.next(), segments.next(), segments.next())
    else {
        return Err(FixtureErrorV1::MalformedName { name: stem.to_owned() });
    };
    if kind != TASK_FIXTURE_KIND_V2 {
        return Ok(None);
    }
    if case.is_empty() {
        return Err(FixtureErrorV1::MalformedName { name: stem.to_owned() });
    }
    TaskFamilyV2::parse(token)
        .ok_or_else(|| FixtureErrorV1::UnknownFamily {
            name: stem.to_owned(),
            family: token.to_owned(),
        })
        .map(Some)
}

/// Parses a usage sidecar; only an observation case may carry one.
fn usage_meta_of(
    stem: &str,
    family: TaskFamilyV2,
    meta: &Value,
) -> Result<TaskUsageMetaV2, FixtureErrorV1> {
    let invalid = || FixtureErrorV1::InvalidMeta {
        case: stem.to_owned(),
        detail: "a task usage sidecar is `{\"usage_pointer\": \"/…\"}` with optional \
                 `bucket_pointers`, on an observation case"
            .to_owned(),
    };
    let pointer = |value: &Value| {
        value.as_str().filter(|text| text.starts_with('/')).map(str::to_owned).ok_or_else(invalid)
    };
    let object =
        meta.as_object().filter(|_| family == TaskFamilyV2::Observation).ok_or_else(invalid)?;
    if object.keys().any(|key| key != "usage_pointer" && key != "bucket_pointers") {
        return Err(invalid());
    }
    let usage_pointer = pointer(object.get("usage_pointer").ok_or_else(invalid)?)?;
    let bucket_pointers = match object.get("bucket_pointers") {
        None => std::collections::BTreeMap::new(),
        Some(map) => map
            .as_object()
            .ok_or_else(invalid)?
            .iter()
            .map(|(bucket, value)| Ok((bucket.clone(), pointer(value)?)))
            .collect::<Result<_, FixtureErrorV1>>()?,
    };
    Ok(TaskUsageMetaV2 { usage_pointer, bucket_pointers })
}

fn read_json(path: &Path) -> Result<Value, FixtureErrorV1> {
    let file = fs::File::open(path).map_err(|source| FixtureErrorV1::Unreadable {
        path: path.to_path_buf(),
        detail: source.to_string(),
    })?;
    serde_json::from_reader(file).map_err(|source| FixtureErrorV1::Unreadable {
        path: path.to_path_buf(),
        detail: source.to_string(),
    })
}
