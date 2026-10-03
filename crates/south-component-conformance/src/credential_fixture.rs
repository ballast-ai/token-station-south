//! Gate ② for credential recipes: the `credential.*` fixture family and the
//! [`CheckV1::CredentialRecipeMatch`] check (B4, `docs/design/2026-09-30-host-zero-vendor-boundary.md`
//! §3.7).
//!
//! A case is a pair of files beside a package's other fixtures, in the directory its manifest's
//! `conformance.fixtures` names:
//!
//! ```text
//! credential.<family>.<case>.input.json
//! credential.<family>.<case>.expected.json
//! ```
//!
//! `family` is the sample the case is offered as — `rotation`, `on-status` or `clock` — and
//! `case` is free. The input is
//!
//! ```json
//! { "slot": "provider_api_key", "now": 1767225600,
//!   "fields": { "refresh_token": "fake-refresh" },
//!   "responses": { "refresh": { "status": 200, "body": { "access_token": "fake-access" } } } }
//! ```
//!
//! — the slot to mint, the fixed time in epoch seconds, the stored field values (fake), and the
//! fake response to each step by step id (`responses` may be omitted when no step makes a request).
//! The expected file is the [`RecipeRunV1`] the reference interpreter must produce, serialized:
//! `recipe` (after any selector), every rendered `requests` entry (`step`, `method`, `url`,
//! `headers`, `body`), and the `outcome` — `{"minted": {"present", "expires_at", "outputs",
//! "write_back", "attributes"}}`, `"use_stored"`, `{"configuration": {"field"}}`,
//! `{"reauth_required": {"step", "status"}}` or `{"transient": {"step", "status"}}`. A `jwt_sign`
//! step is signed with [`FixtureSignerV1`], so a minted JWT's last part is that stand-in's bytes.
//!
//! **Coverage.** A package whose manifest declares recipes ships at least one `clock` sample that
//! mints. When any recipe rotates its refresh material it also ships a `rotation` sample that
//! mints through a rotating recipe, and when any recipe makes an HTTP exchange an `on-status`
//! sample whose run met a non-2xx status. A family is judged by what its cases do, not by their
//! names, so a mislabelled case covers nothing. A missing family is a [`CheckV1::Coverage`]
//! failure named `credential.<family>`. A package whose recipes are only `jwt_sign` (Kling) owes
//! neither of the last two: it makes no exchange and holds no refresh material.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;
use south_provider_api::{CredentialsV1, StepKindV1};

use crate::credential_recipe::{
    FakeResponseV1, FixtureSignerV1, RecipeEffectsV1, RecipeOutcomeV1, RecipeRunV1,
    RenderedRequestV1, run_recipe_v1,
};
use crate::fixture::{FixtureErrorV1, read_json};
use crate::report::{CheckV1, OutcomeV1};

/// The filename prefix a credential fixture carries.
pub const CREDENTIAL_FIXTURE_KIND_V1: &str = "credential";

/// The sample a credential case is offered as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CredentialFamilyV1 {
    /// A refresh that rotates the refresh material: what is written back, and what is kept.
    Rotation,
    /// A non-2xx status and what `on_status` (or its defaults) made of it.
    OnStatus,
    /// How the expiry of a minted value is read and clamped.
    Clock,
}

impl CredentialFamilyV1 {
    /// Every family, so coverage can name a missing one.
    pub const ALL: [Self; 3] = [Self::Rotation, Self::OnStatus, Self::Clock];

    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Rotation => "rotation",
            Self::OnStatus => "on-status",
            Self::Clock => "clock",
        }
    }

    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| family.token() == token)
    }
}

/// One credential case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialCaseV1 {
    /// The full `credential.<family>.<case>` name.
    pub name: String,
    pub family: CredentialFamilyV1,
    pub input: Value,
    pub expected: Value,
}

/// Every credential case in one directory, in name order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CredentialFixturePackV1 {
    cases: Vec<CredentialCaseV1>,
}

impl CredentialFixturePackV1 {
    /// Reads every `credential.*.input.json` in `directory` with its expected file. Files of
    /// other kinds are ignored.
    ///
    /// # Errors
    ///
    /// The first [`FixtureErrorV1`] found: a malformed name, an unknown family, or a file that
    /// does not read as JSON.
    pub fn load(directory: &Path) -> Result<Self, FixtureErrorV1> {
        let unreadable =
            |detail: String| FixtureErrorV1::Unreadable { path: directory.to_path_buf(), detail };
        let mut stems = Vec::new();
        for entry in fs::read_dir(directory).map_err(|error| unreadable(error.to_string()))? {
            let path = entry.map_err(|error| unreadable(error.to_string()))?.path();
            if let Some(stem) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".input.json"))
            {
                stems.push(stem.to_owned());
            }
        }
        stems.sort();
        let mut cases = Vec::new();
        for stem in stems {
            let mut segments = stem.splitn(3, '.');
            let (Some(kind), Some(token), Some(case)) =
                (segments.next(), segments.next(), segments.next())
            else {
                continue;
            };
            if kind != CREDENTIAL_FIXTURE_KIND_V1 {
                continue;
            }
            if case.is_empty() {
                return Err(FixtureErrorV1::MalformedName { name: stem });
            }
            let family = CredentialFamilyV1::parse(token).ok_or_else(|| {
                FixtureErrorV1::UnknownFamily { name: stem.clone(), family: token.to_owned() }
            })?;
            cases.push(CredentialCaseV1 {
                input: read_json(&directory.join(format!("{stem}.input.json")), &stem)?,
                expected: read_json(&directory.join(format!("{stem}.expected.json")), &stem)?,
                family,
                name: stem,
            });
        }
        Ok(Self { cases })
    }

    #[must_use]
    pub const fn from_cases(cases: Vec<CredentialCaseV1>) -> Self {
        Self { cases }
    }

    #[must_use]
    pub fn cases(&self) -> &[CredentialCaseV1] {
        &self.cases
    }
}

/// A credential case's input.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialInputV1 {
    slot: String,
    now: i64,
    #[serde(default)]
    fields: BTreeMap<String, String>,
    #[serde(default)]
    responses: BTreeMap<String, FakeResponseV1>,
}

/// What one case did, beyond its run.
struct Ran {
    run: RecipeRunV1,
    /// Steps the fixture answers that the recipe never asked.
    unused: Vec<String>,
    /// Whether some response the recipe consumed was not a 2xx.
    met_non_success: bool,
}

fn run_case(credentials: &CredentialsV1, input: &Value) -> Result<Ran, String> {
    let input: CredentialInputV1 = serde_json::from_value(input.clone())
        .map_err(|error| format!("fixture is not valid input: {error}"))?;
    let mut asked = BTreeSet::new();
    let mut met_non_success = false;
    let mut responder = |request: &RenderedRequestV1| {
        asked.insert(request.step.clone());
        let response = input.responses.get(&request.step).cloned();
        met_non_success |=
            response.as_ref().is_some_and(|response| !(200..300).contains(&response.status));
        response
    };
    let run = run_recipe_v1(
        credentials,
        &input.slot,
        &input.fields,
        &mut RecipeEffectsV1 {
            now: input.now,
            signer: &FixtureSignerV1,
            responder: &mut responder,
        },
    );
    let unused = input.responses.keys().filter(|step| !asked.contains(*step)).cloned().collect();
    Ok(Ran { run, unused, met_non_success })
}

/// Whether a run is the sample its family asks for.
fn exhibits(credentials: &CredentialsV1, family: CredentialFamilyV1, ran: &Ran) -> bool {
    let minted = matches!(ran.run.outcome, RecipeOutcomeV1::Minted(_));
    match family {
        CredentialFamilyV1::Clock => minted,
        CredentialFamilyV1::Rotation => {
            minted
                && ran
                    .run
                    .recipe
                    .as_ref()
                    .and_then(|name| credentials.recipes.get(name))
                    .is_some_and(|recipe| recipe.rotates_refresh_material == Some(true))
        }
        CredentialFamilyV1::OnStatus => ran.met_non_success,
    }
}

/// The families a package with these recipes must cover.
fn required_families(credentials: &CredentialsV1) -> Vec<CredentialFamilyV1> {
    let recipes = || credentials.recipes.values();
    CredentialFamilyV1::ALL
        .into_iter()
        .filter(|family| match family {
            CredentialFamilyV1::Clock => !credentials.recipes.is_empty(),
            CredentialFamilyV1::Rotation => {
                recipes().any(|recipe| recipe.rotates_refresh_material == Some(true))
            }
            CredentialFamilyV1::OnStatus => recipes()
                .flat_map(|recipe| recipe.steps.iter())
                .any(|step| step.kind != StepKindV1::JwtSign),
        })
        .collect()
}

/// Runs every credential case against `credentials` with the reference interpreter, and the
/// coverage rule. Never panics on a bad fixture: it becomes a failed outcome.
#[must_use]
pub fn credential_recipe_checks_v1(
    credentials: &CredentialsV1,
    pack: &CredentialFixturePackV1,
) -> Vec<OutcomeV1> {
    let mut covered = BTreeSet::new();
    let mut matches = Vec::new();
    for case in pack.cases() {
        let ran = match run_case(credentials, &case.input) {
            Ok(ran) => ran,
            Err(detail) => {
                matches.push(OutcomeV1::failed(CheckV1::CredentialRecipeMatch, &case.name, detail));
                continue;
            }
        };
        if exhibits(credentials, case.family, &ran) {
            covered.insert(case.family);
        }
        let actual = serde_json::to_value(&ran.run).unwrap_or(Value::Null);
        matches.push(if !ran.unused.is_empty() {
            OutcomeV1::failed(
                CheckV1::CredentialRecipeMatch,
                &case.name,
                format!(
                    "the fixture answers steps the recipe never asked: {:?}; it was written for \
                     another recipe",
                    ran.unused
                ),
            )
        } else if actual == case.expected {
            OutcomeV1::passed(CheckV1::CredentialRecipeMatch, &case.name)
        } else {
            let detail = ran.run.outcome.detail();
            OutcomeV1::failed(
                CheckV1::CredentialRecipeMatch,
                &case.name,
                format!(
                    "the recipe's run differs from the expected run; actual: {actual}{}",
                    if detail.is_empty() { String::new() } else { format!(" ({detail})") }
                ),
            )
        });
    }

    let mut outcomes: Vec<OutcomeV1> = required_families(credentials)
        .into_iter()
        .filter(|family| !covered.contains(family))
        .map(|family| {
            OutcomeV1::failed(
                CheckV1::Coverage,
                format!("{CREDENTIAL_FIXTURE_KIND_V1}.{}", family.token()),
                "a package that declares these recipes must ship this credential sample, and no \
                 case of the family exhibits it",
            )
        })
        .collect();
    if outcomes.is_empty() && !credentials.recipes.is_empty() {
        outcomes.push(OutcomeV1::passed(CheckV1::Coverage, CREDENTIAL_FIXTURE_KIND_V1));
    }
    outcomes.extend(matches);
    outcomes
}
