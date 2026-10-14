//! Fixture packs for the speech world (`docs/design/2026-09-30-speech-world.md` §11), beside the
//! image world's.
//!
//! File convention `speech-v1.<family>.<case>.{input,expected}.json`, with the three families of
//! the record. A directory may hold other kinds; this loader ignores them and reads the
//! `credential.*` cases beside it. The shapes:
//!
//! - `capabilities`: `{"provider_config": ProviderConfig}` -> the capability list, or
//!   `{"error": ErrorEnvelope}`.
//! - `prepare`: `{"provider_config", "request": <request view>, "context": SpeechCallContextV1}`
//!   -> the prepared call, or `{"error": ErrorEnvelope}` (a pre-dispatch refusal). The request
//!   view is the JSON the host hands the component (`{"json": …}` for synthesis,
//!   `{"multipart": {"parts": …}}` for recognition).
//! - `response`: `{"prepare_case": "speech-v1.prepare.<case>", "response": {"status",
//!   "headers"?, "body"? | "body_text"? | "body_base64"?, "content_type"?}}`, where the body is the
//!   raw upstream answer: a JSON value (`body`), text (`body_text`, which is how an `sse` body is
//!   written) or bytes (`body_base64`). The suite prepares the named case, builds the response view
//!   the host would — its declared body form and elision paths; an `sse` body is first split into
//!   events — and hands it to `parse-response`. Expected: the outcome, or `{"error":
//!   ErrorEnvelope}` for a component failure.
//!
//! A response case may carry `speech-v1.response.<case>.meta.json`, `{"missing": "<fact>"}`, naming
//! what its 2xx lacks: `tokens` (the whole `usage`), `tokens.input` or `tokens.output` (one count;
//! `missing_meter_is_not_zero`), `seconds` (the duration; `absent_duration_is_null_or_unknown`)
//! or `audio` (no audio at all; `empty_audio_is_unknown`).

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::credential_fixture::CredentialFixturePackV1;
use crate::fixture::{FixtureErrorV1, read_json};

/// The filename prefix a speech fixture carries.
pub const SPEECH_FIXTURE_KIND_V1: &str = "speech-v1";

/// The facts a response sidecar may name.
pub const SPEECH_META_FACTS_V1: [&str; 5] =
    ["tokens", "tokens.input", "tokens.output", "seconds", "audio"];

/// Which speech-component function a fixture family exercises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechFamilyV1 {
    /// `model-capabilities`.
    Capabilities,
    /// `prepare`.
    Prepare,
    /// `parse-response`, through the host's view building.
    Response,
}

impl SpeechFamilyV1 {
    /// Every family.
    pub const ALL: [Self; 3] = [Self::Capabilities, Self::Prepare, Self::Response];

    /// The family's filename token.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Capabilities => "capabilities",
            Self::Prepare => "prepare",
            Self::Response => "response",
        }
    }

    /// The family a filename token names.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| family.token() == token)
    }
}

/// One speech fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechCaseV1 {
    /// The full `speech-v1.<family>.<case>` name.
    pub name: String,
    /// The family.
    pub family: SpeechFamilyV1,
    /// The input.
    pub input: Value,
    /// The expected output.
    pub expected: Value,
    /// The fact a response sidecar says the 2xx lacks, when present.
    pub missing: Option<String>,
}

/// Every speech case found in one directory, in name order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpeechFixturePackV1 {
    cases: Vec<SpeechCaseV1>,
    credentials: CredentialFixturePackV1,
}

impl SpeechFixturePackV1 {
    /// Reads every `speech-v1.*.input.json` in `directory` with its expected output and sidecar.
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
            let missing = if meta_path.exists() {
                Some(meta_of(&read_json(&meta_path, &stem)?, &stem, family)?)
            } else {
                None
            };
            cases.push(SpeechCaseV1 {
                input: read_json(&directory.join(format!("{stem}.input.json")), &stem)?,
                expected: read_json(&directory.join(format!("{stem}.expected.json")), &stem)?,
                family,
                name: stem,
                missing,
            });
        }
        Ok(Self { cases, credentials: CredentialFixturePackV1::load(directory)? })
    }

    /// A pack of these cases, without credential cases.
    #[must_use]
    pub fn from_cases(cases: Vec<SpeechCaseV1>) -> Self {
        Self { cases, credentials: CredentialFixturePackV1::default() }
    }

    /// The cases, in name order.
    #[must_use]
    pub fn cases(&self) -> &[SpeechCaseV1] {
        &self.cases
    }

    /// The `credential.*` cases in the same directory.
    #[must_use]
    pub const fn credentials(&self) -> &CredentialFixturePackV1 {
        &self.credentials
    }

    /// The case with this full name.
    #[must_use]
    pub fn case(&self, name: &str) -> Option<&SpeechCaseV1> {
        self.cases.iter().find(|case| case.name == name)
    }
}

/// A response sidecar: exactly `{"missing": "<fact>"}`.
fn meta_of(meta: &Value, case: &str, family: SpeechFamilyV1) -> Result<String, FixtureErrorV1> {
    let invalid = |detail: &str| FixtureErrorV1::InvalidMeta {
        case: case.to_owned(),
        detail: detail.to_owned(),
    };
    if family != SpeechFamilyV1::Response {
        return Err(invalid("only response cases carry metadata"));
    }
    let Some(fact) = meta
        .as_object()
        .filter(|map| map.len() == 1)
        .and_then(|map| map.get("missing"))
        .and_then(Value::as_str)
    else {
        return Err(invalid("metadata must be an object with the single key `missing`"));
    };
    if SPEECH_META_FACTS_V1.contains(&fact) {
        Ok(fact.to_owned())
    } else {
        Err(invalid(
            "the fact must be `tokens`, `tokens.input`, `tokens.output`, `seconds` or `audio`",
        ))
    }
}

/// `speech-v1.<family>.<case>` -> the family, or `None` when the file belongs to another pack.
fn family_of(stem: &str) -> Result<Option<SpeechFamilyV1>, FixtureErrorV1> {
    let mut segments = stem.splitn(3, '.');
    let (Some(kind), Some(token), Some(case)) = (segments.next(), segments.next(), segments.next())
    else {
        return Ok(None);
    };
    if kind != SPEECH_FIXTURE_KIND_V1 {
        return Ok(None);
    }
    if case.is_empty() {
        return Err(FixtureErrorV1::MalformedName { name: stem.to_owned() });
    }
    SpeechFamilyV1::parse(token)
        .ok_or_else(|| FixtureErrorV1::UnknownFamily {
            name: stem.to_owned(),
            family: token.to_owned(),
        })
        .map(Some)
}
