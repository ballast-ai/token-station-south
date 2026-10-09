//! Fixture packs for the image world (`docs/design/2026-09-30-image-world.md` §12.1), beside the
//! provider, task and embeddings worlds'.
//!
//! File convention `image-v1.<family>.<case>.{input,expected}.json`, with the four families of
//! the record. A directory may hold other kinds; this loader ignores them and reads the
//! `credential.*` cases beside it. The shapes:
//!
//! - `capabilities`: `{"provider_config": ProviderConfig}` -> the capability list, or
//!   `{"error": ErrorEnvelope}`.
//! - `prepare`: `{"provider_config", "request": <request view>, "context": ImageCallContextV1}`
//!   -> the prepared call, or `{"error": ErrorEnvelope}` (a pre-dispatch refusal). The request
//!   view is the JSON the host hands the component (`{"json": …}` or `{"multipart": {"parts": …}}`).
//! - `response`: `{"prepare_case": "image-v1.prepare.<case>", "response": {"status", "headers"?,
//!   "body"? | "body_base64"?, "content_type"?}}`, where the body is the raw upstream answer. The
//!   suite prepares the named case, builds the response view the host would (its declared body
//!   form and elision paths) and hands it to `parse-response`. Expected: the outcome, or
//!   `{"error": ErrorEnvelope}` for a component failure.
//! - `render`: `{"response_cases": ["image-v1.response.<case>", …], "context":
//!   ImageRenderContextV1}`; every named case must end `succeeded`, and all must share one
//!   prepare case. Expected: `{"template": …}`, or `{"error": ErrorEnvelope}`.
//!
//! A response case may carry `image-v1.response.<case>.meta.json`: `{"missing": "<fact>"}` marks a
//! 2xx whose required fact is absent (`missing_meter_is_not_zero`), `{"absent": "<fact>"}` one
//! whose evidence fact is absent (`evidence_absent_is_null`). A fact is a token bucket word,
//! `credits`, `images_reported` or `upstream_cost`.

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::credential_fixture::CredentialFixturePackV1;
use crate::fixture::{FixtureErrorV1, read_json};

/// The filename prefix an image fixture carries.
pub const IMAGE_FIXTURE_KIND_V1: &str = "image-v1";

/// Which image-component function a fixture family exercises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFamilyV1 {
    /// `model-capabilities`.
    Capabilities,
    /// `prepare`.
    Prepare,
    /// `parse-response`, through the host's view building.
    Response,
    /// `render`.
    Render,
}

impl ImageFamilyV1 {
    /// Every family.
    pub const ALL: [Self; 4] = [Self::Capabilities, Self::Prepare, Self::Response, Self::Render];

    /// The family's filename token.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Capabilities => "capabilities",
            Self::Prepare => "prepare",
            Self::Response => "response",
            Self::Render => "render",
        }
    }

    /// The family a filename token names.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| family.token() == token)
    }
}

/// What a response case's sidecar says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageCaseMetaV1 {
    /// A 2xx whose required fact is absent: the outcome must be `unknown`.
    Missing(String),
    /// A 2xx whose evidence fact is absent: the outcome must be `succeeded` with it `null`.
    Absent(String),
}

/// One image fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageCaseV1 {
    /// The full `image-v1.<family>.<case>` name.
    pub name: String,
    /// The family.
    pub family: ImageFamilyV1,
    /// The input.
    pub input: Value,
    /// The expected output.
    pub expected: Value,
    /// The response sidecar, when present.
    pub meta: Option<ImageCaseMetaV1>,
}

/// Every image case found in one directory, in name order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageFixturePackV1 {
    cases: Vec<ImageCaseV1>,
    credentials: CredentialFixturePackV1,
}

/// The facts a sidecar may name.
pub const IMAGE_META_FACTS_V1: [&str; 12] = [
    "text_input",
    "image_input",
    "cached_text_input",
    "cached_image_input",
    "cached_input",
    "text_output",
    "image_output",
    "total_input",
    "total_output",
    "credits",
    "images_reported",
    "upstream_cost",
];

impl ImageFixturePackV1 {
    /// Reads every `image-v1.*.input.json` in `directory` with its expected output and sidecar.
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
            let meta = if meta_path.exists() {
                Some(meta_of(&read_json(&meta_path, &stem)?, &stem, family)?)
            } else {
                None
            };
            cases.push(ImageCaseV1 {
                input: read_json(&directory.join(format!("{stem}.input.json")), &stem)?,
                expected: read_json(&directory.join(format!("{stem}.expected.json")), &stem)?,
                family,
                name: stem,
                meta,
            });
        }
        Ok(Self { cases, credentials: CredentialFixturePackV1::load(directory)? })
    }

    /// A pack of these cases, without credential cases.
    #[must_use]
    pub fn from_cases(cases: Vec<ImageCaseV1>) -> Self {
        Self { cases, credentials: CredentialFixturePackV1::default() }
    }

    /// The cases, in name order.
    #[must_use]
    pub fn cases(&self) -> &[ImageCaseV1] {
        &self.cases
    }

    /// The `credential.*` cases in the same directory.
    #[must_use]
    pub const fn credentials(&self) -> &CredentialFixturePackV1 {
        &self.credentials
    }

    /// The case with this full name.
    #[must_use]
    pub fn case(&self, name: &str) -> Option<&ImageCaseV1> {
        self.cases.iter().find(|case| case.name == name)
    }
}

/// A response sidecar: exactly `{"missing": "<fact>"}` or `{"absent": "<fact>"}`.
fn meta_of(
    meta: &Value,
    case: &str,
    family: ImageFamilyV1,
) -> Result<ImageCaseMetaV1, FixtureErrorV1> {
    let invalid = |detail: &str| FixtureErrorV1::InvalidMeta {
        case: case.to_owned(),
        detail: detail.to_owned(),
    };
    if family != ImageFamilyV1::Response {
        return Err(invalid("only response cases carry metadata"));
    }
    let Some((key, value)) =
        meta.as_object().filter(|map| map.len() == 1).and_then(|map| map.iter().next())
    else {
        return Err(invalid(
            "metadata must be an object with the single key `missing` or `absent`",
        ));
    };
    let fact = value
        .as_str()
        .filter(|fact| IMAGE_META_FACTS_V1.contains(fact))
        .ok_or_else(|| {
            invalid(
                "the fact must be a token bucket, `credits`, `images_reported` or `upstream_cost`",
            )
        })?
        .to_owned();
    match key.as_str() {
        "missing" => Ok(ImageCaseMetaV1::Missing(fact)),
        "absent" => Ok(ImageCaseMetaV1::Absent(fact)),
        _ => Err(invalid("the key must be `missing` or `absent`")),
    }
}

/// `image-v1.<family>.<case>` -> the family, or `None` when the file belongs to another pack.
fn family_of(stem: &str) -> Result<Option<ImageFamilyV1>, FixtureErrorV1> {
    let mut segments = stem.splitn(3, '.');
    let (Some(kind), Some(token), Some(case)) = (segments.next(), segments.next(), segments.next())
    else {
        return Ok(None);
    };
    if kind != IMAGE_FIXTURE_KIND_V1 {
        return Ok(None);
    }
    if case.is_empty() {
        return Err(FixtureErrorV1::MalformedName { name: stem.to_owned() });
    }
    ImageFamilyV1::parse(token)
        .ok_or_else(|| FixtureErrorV1::UnknownFamily {
            name: stem.to_owned(),
            family: token.to_owned(),
        })
        .map(Some)
}
