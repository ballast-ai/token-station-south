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

/// Every task case found in one directory.
#[derive(Debug, Clone, Default)]
pub struct TaskFixturePackV2 {
    cases: Vec<TaskCaseV2>,
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

        for stem in names {
            let Some(family) = family_of(&stem)? else {
                continue;
            };
            let input = read_json(&directory.join(format!("{stem}.input.json")))?;
            let expected = read_json(&directory.join(format!("{stem}.expected.json")))?;
            cases.push(TaskCaseV2 { name: stem, family, input, expected });
        }
        Ok(Self { cases })
    }

    #[must_use]
    pub fn cases(&self) -> &[TaskCaseV2] {
        &self.cases
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
