//! Credential recipes: the component describes how a credential is minted, the host executes it
//! (B4, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §3).
//!
//! A manifest's optional `credentials` section declares which fields a kind of credential consists
//! of, how they are imported from a file, which secret slots are minted rather than entered, and
//! the recipes that mint them — small state machines over a closed set of step kinds. Nothing here
//! executes a recipe: the executor, the material, the locks and the write-back stay in the host
//! (§3.5, §3.6). This module only says what a well-formed recipe is, which is gate ①.
//!
//! The section is untrusted input like the rest of the manifest. Gate ① therefore also enforces
//! the trust rules of §3.4 that can be checked statically: a signed assertion sent to a step is
//! bound to that step's endpoint, no assertion names a constant subject, and an exported attribute
//! comes only from a field declared non-secret. The rules a host enforces at run time — the
//! operator confirms the endpoints a package's recipes reach, per package digest (§16 Q18), and
//! recipes run only for verified first-party packages until package signing exists (§16 Q11) —
//! rely on [`CredentialsV1::endpoints`] to list what must be confirmed.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::config::template_params;
use crate::{ManifestErrorV1, ValueSyntaxV1};

/// The schema tag of a `credentials` section.
pub const CREDENTIAL_RECIPE_SCHEMA: &str = "south.credential-recipe.v1";

/// The most steps one recipe may have.
pub const MAX_RECIPE_STEPS: usize = 4;

/// The host's TTL clamp (§3.5): every expiry lands between these, whatever its source. A recipe
/// may only narrow the range.
pub const HOST_MIN_TTL_SECONDS: u32 = 60;
/// See [`HOST_MIN_TTL_SECONDS`].
pub const HOST_MAX_TTL_SECONDS: u32 = 24 * 60 * 60;

/// A manifest's `credentials` section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialsV1 {
    /// Must be [`CREDENTIAL_RECIPE_SCHEMA`].
    pub schema: String,
    /// The fields this kind of credential consists of.
    pub fields: BTreeMap<String, CredentialFieldV1>,
    /// Each group needs at least one present field when the credential is saved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub require_one_of: Vec<Vec<String>>,
    /// How fields are imported from a credential file, keyed by field.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub import: BTreeMap<String, ImportRuleV1>,
    /// A usable minted value already in an imported file, so the first request needs no exchange.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<SeedV1>,
    /// Each secret slot that is minted rather than entered. A slot without an entry is `static`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub slots: BTreeMap<String, SlotV1>,
    /// The minting recipes, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub recipes: BTreeMap<String, RecipeV1>,
}

/// One field of a credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialFieldV1 {
    /// Stored encrypted, redacted, and never exported.
    pub secret: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax: Option<ValueSyntaxV1>,
    /// A media type for a field holding a whole document, e.g. `application/json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    /// A non-secret value used when the field is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// Where in an imported file a field's value is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportRuleV1 {
    /// The kind of file, e.g. `service-account-json`.
    pub file: String,
    /// Ordered candidate JSON Pointers; the first present one wins. `""` is the whole file.
    pub pointers: Vec<String>,
}

/// A minted value already present in an imported file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedV1 {
    pub file: String,
    /// The slot the seeded value fills.
    pub slot: String,
    /// The value's pointer; it is always stored as secret.
    pub present: String,
    pub expires_at: SeedClockV1,
}

/// How a seed's expiry is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedClockV1 {
    EpochSeconds(String),
    EpochMillis(String),
    JwtExp(String),
    RelativeSeconds(String),
    /// An RFC 3339 timestamp or epoch seconds, as credential files written by other tools mix them.
    Rfc3339OrEpochSeconds(String),
}

/// A secret slot: entered as is, or minted by a recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotV1 {
    Static,
    Minted(String),
}

/// One minting recipe, or a selector over recipes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeV1 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<StepV1>,
    /// `step.output` that becomes the slot value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub present: Option<String>,
    /// Whether a refresh replaces the refresh material. Required, no default: getting it backwards
    /// raises no error and loses the credential.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotates_refresh_material: Option<bool>,
    /// Field -> `step.output` where rotated material goes; required whenever the recipe rotates.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub write_back: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_margin_seconds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub without_refresh_material: Option<WithoutRefreshMaterialV1>,
    /// The expiry used when the clock convention yields none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_seconds: Option<u32>,
    /// The window of a `fixed_window` clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_validity_seconds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_ttl_seconds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_ttl_seconds: Option<u32>,
    /// A selector: ordered rules, the first match wins, the last has no test.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub select: Vec<SelectRuleV1>,
    /// Non-secret values exported to the component.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, AttributeV1>,
}

/// What a recipe does when the stored credential has no refresh material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WithoutRefreshMaterialV1 {
    UseStored,
    Fail,
}

/// One selector rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectRuleV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<PredicateV1>,
    pub recipe: String,
}

/// A selector test. Presence may be tested on secret fields; values only on non-secret ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateV1 {
    FieldPresent(String),
    AllPresent(Vec<String>),
    /// The field's value is one of these, compared ASCII case-insensitively.
    FieldIn {
        field: String,
        values: Vec<String>,
    },
}

/// An exported attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributeV1 {
    /// A field declared non-secret.
    pub field: String,
    pub export: bool,
    /// The host keeps the last value and uses it when the field is later absent.
    #[serde(default)]
    pub persist: bool,
}

/// One step of a recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepV1 {
    pub id: String,
    pub kind: StepKindV1,
    /// HTTP steps: the method. `oauth2_token` is always `POST`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<StepMethodV1>,
    /// HTTP steps: a constant `https` URL or a template whose parameters come from
    /// `endpoint_params`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub endpoint_params: BTreeMap<String, FieldRefV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<EncodingV1>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ValueSourceV1>,
    /// Ordinary headers; credentials go through `auth`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, ValueSourceV1>,
    /// Presents a value under an RFC 7235 auth-scheme, e.g. `token` or `Bearer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<StepAuthV1>,
    /// Fields that must be present before the step runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// Status code (`"404"`) or class (`"4xx"`, `"5xx"`) -> action.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub on_status: BTreeMap<String, StatusActionV1>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extract: BTreeMap<String, ExtractV1>,
    /// `jwt_sign`: the algorithm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alg: Option<JwtAlgorithmV1>,
    /// `jwt_sign`: the signing key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<ValueSourceV1>,
    /// `jwt_sign`: the claims. Its output is `<step>.jwt`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub claims: BTreeMap<String, ValueSourceV1>,
}

/// The closed set of step kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKindV1 {
    /// RFC 6749 token endpoint (refresh, client credentials, RFC 7523 JWT bearer).
    Oauth2Token,
    /// RFC 7519 JWT signed with a closed-set algorithm.
    JwtSign,
    /// A plain exchange with declared parameter names.
    HttpExchange,
    /// As `http_exchange`, but only the status matters.
    HttpProbe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum StepMethodV1 {
    Get,
    Post,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncodingV1 {
    Form,
    Json,
}

/// The closed set of JWT algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JwtAlgorithmV1 {
    HS256,
    RS256,
    ES256,
}

/// A field reference, optionally into a JSON document the field holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldRefV1 {
    pub field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
}

/// Where a parameter, header, claim or key value comes from. Exactly one source.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueSourceV1 {
    #[serde(default, rename = "const", skip_serializing_if = "Option::is_none")]
    pub constant: Option<ConstantV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// With `field`: a JSON Pointer into the document the field holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    /// `step.output` of an earlier step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Now plus this many seconds, as epoch seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_plus: Option<i64>,
    /// The URL of the named step: the only admitted `aud` for an assertion sent anywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_of: Option<String>,
}

/// A constant parameter value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConstantV1 {
    Text(String),
    Integer(i64),
    Boolean(bool),
}

/// Presenting a value under an auth-scheme on an exchange request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepAuthV1 {
    pub scheme: String,
    pub value: ValueSourceV1,
}

/// What a status does to the recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusActionV1 {
    /// The credential is not usable; no retry until the operator acts.
    ReauthRequired,
    /// Try again later.
    Transient,
    /// Continue at a later step.
    Goto(String),
}

/// One extracted value. Exactly one source: `pointer`, one clock form, or `jwt_claim`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_seconds: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch_seconds: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch_millis: Option<String>,
    /// A pointer to a JWT whose `exp` is the expiry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwt_exp: Option<String>,
    /// Ignore any expiry in the response; use the recipe's `fixed_validity_seconds`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_window: Option<bool>,
    /// Decodes a JWT payload without verifying it: a check, never a source of exports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwt_claim: Option<JwtClaimV1>,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub optional: bool,
    /// When this value and the named stored field are both present and differ, the refresh fails
    /// as `reauth_required` and nothing is written back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub must_equal_field: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JwtClaimV1 {
    /// A pointer to the JWT in the step's response.
    pub token: String,
    /// The claim, as a pointer into the decoded payload.
    pub pointer: String,
}

fn is_name(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// A JSON Pointer; `""` (the whole document) is allowed where the caller says so.
fn is_pointer(pointer: &str, root_allowed: bool) -> bool {
    if pointer.is_empty() {
        return root_allowed;
    }
    pointer.len() <= 256
        && pointer.starts_with('/')
        && pointer[1..]
            .split('~')
            .skip(1)
            .all(|rest| rest.starts_with('0') || rest.starts_with('1'))
}

/// Headers a recipe may not set: credentials go through `auth`, and the transport owns the rest.
const RESERVED_STEP_HEADERS: [&str; 8] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "host",
    "content-length",
    "content-type",
    "transfer-encoding",
    "connection",
];

fn is_header_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !RESERVED_STEP_HEADERS.contains(&name)
}

impl CredentialsV1 {
    /// Every endpoint (constant or template) the package's recipes can reach, sorted and
    /// deduplicated: what an operator confirms for this package digest before any recipe runs
    /// (§3.4 rule 1, §16 Q18).
    #[must_use]
    pub fn endpoints(&self) -> Vec<String> {
        self.recipes
            .values()
            .flat_map(|recipe| recipe.steps.iter())
            .filter_map(|step| step.endpoint.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Gate ① for the section (§3.7). `secret_slots` are the manifest's `permissions.secrets`.
    pub(crate) fn validate(&self, secret_slots: &[String]) -> Result<(), ManifestErrorV1> {
        self.check(secret_slots).map_err(ManifestErrorV1::InvalidCredentials)
    }

    fn check(&self, secret_slots: &[String]) -> Result<(), String> {
        if self.schema != CREDENTIAL_RECIPE_SCHEMA {
            return Err(format!("schema must be `{CREDENTIAL_RECIPE_SCHEMA}`"));
        }
        self.check_fields()?;
        for (slot, kind) in &self.slots {
            if !secret_slots.contains(slot) {
                return Err(format!("slot `{slot}` is not declared under permissions.secrets"));
            }
            if let SlotV1::Minted(recipe) = kind
                && !self.recipes.contains_key(recipe)
            {
                return Err(format!("slot `{slot}` names no recipe `{recipe}`"));
            }
        }
        if let Some(seed) = &self.seed {
            if !matches!(self.slots.get(&seed.slot), Some(SlotV1::Minted(_))) {
                return Err("the seed must fill a minted slot".to_owned());
            }
            let clock = match &seed.expires_at {
                SeedClockV1::EpochSeconds(pointer)
                | SeedClockV1::EpochMillis(pointer)
                | SeedClockV1::JwtExp(pointer)
                | SeedClockV1::RelativeSeconds(pointer)
                | SeedClockV1::Rfc3339OrEpochSeconds(pointer) => pointer,
            };
            if !is_name_like_file(&seed.file)
                || !is_pointer(&seed.present, false)
                || !is_pointer(clock, false)
            {
                return Err("the seed names a malformed file kind or pointer".to_owned());
            }
        }
        for (name, recipe) in &self.recipes {
            if !is_name(name) {
                return Err(format!("recipe name `{name}` is not lowercase snake_case"));
            }
            self.check_recipe(name, recipe)
                .map_err(|detail| format!("recipe `{name}`: {detail}"))?;
        }
        Ok(())
    }

    fn check_fields(&self) -> Result<(), String> {
        for (name, field) in &self.fields {
            if !is_name(name) {
                return Err(format!("field name `{name}` is not lowercase snake_case"));
            }
            if field.syntax.as_ref().is_some_and(|syntax| !syntax.is_well_formed()) {
                return Err(format!("field `{name}` has a malformed syntax"));
            }
            if let Some(default) = &field.default {
                if field.secret {
                    return Err(format!("field `{name}` is secret and may not have a default"));
                }
                if field.syntax.as_ref().is_some_and(|syntax| !syntax.admits(default)) {
                    return Err(format!("field `{name}`'s default does not have its syntax"));
                }
            }
            if field.media.as_ref().is_some_and(|media| {
                !media.contains('/') || !media.bytes().all(|byte| byte.is_ascii_graphic())
            }) {
                return Err(format!("field `{name}` has a malformed media type"));
            }
        }
        for group in &self.require_one_of {
            if group.is_empty() || group.iter().any(|field| !self.fields.contains_key(field)) {
                return Err(
                    "a require_one_of group is empty or names an undeclared field".to_owned()
                );
            }
        }
        for (field, rule) in &self.import {
            if !self.fields.contains_key(field) {
                return Err(format!("import names an undeclared field `{field}`"));
            }
            if !is_name_like_file(&rule.file)
                || rule.pointers.is_empty()
                || rule.pointers.iter().any(|pointer| !is_pointer(pointer, true))
            {
                return Err(format!("import of `{field}` has a malformed file kind or pointer"));
            }
        }
        Ok(())
    }

    fn is_secret(&self, field: &str) -> Option<bool> {
        self.fields.get(field).map(|declared| declared.secret)
    }

    fn check_recipe(&self, name: &str, recipe: &RecipeV1) -> Result<(), String> {
        if !recipe.select.is_empty() {
            return self.check_selector(name, recipe);
        }
        if recipe.steps.is_empty() || recipe.steps.len() > MAX_RECIPE_STEPS {
            return Err(format!("a recipe has one to {MAX_RECIPE_STEPS} steps"));
        }
        let rotates = recipe
            .rotates_refresh_material
            .ok_or("rotates_refresh_material is required and has no default")?;
        if rotates && recipe.write_back.is_empty() {
            return Err("a recipe that rotates must declare write_back".to_owned());
        }
        if !rotates && !recipe.write_back.is_empty() {
            return Err("write_back is declared only by a recipe that rotates".to_owned());
        }
        if let (Some(min), Some(max)) = (recipe.min_ttl_seconds, recipe.max_ttl_seconds)
            && min > max
        {
            return Err("min_ttl_seconds exceeds max_ttl_seconds".to_owned());
        }
        // A margin at or above the shortest validity a minted value can have makes every freshly
        // minted value already due for refresh, so the host would exchange on every request.
        if let Some(margin) = recipe.refresh_margin_seconds
            && margin >= recipe.min_ttl_seconds.unwrap_or(HOST_MIN_TTL_SECONDS)
        {
            return Err(
                "refresh_margin_seconds must be shorter than the shortest validity after the clamp"
                    .to_owned(),
            );
        }
        for ttl in [recipe.min_ttl_seconds, recipe.max_ttl_seconds].into_iter().flatten() {
            if !(HOST_MIN_TTL_SECONDS..=HOST_MAX_TTL_SECONDS).contains(&ttl) {
                return Err("a TTL bound may only narrow the host's 60 s to 24 h clamp".to_owned());
            }
        }
        for (attribute, declared) in &recipe.attributes {
            if !is_name(attribute) || !declared.export {
                return Err(format!(
                    "attribute `{attribute}` must be a snake_case name with export: true"
                ));
            }
            // §3.4 rule 4: never from a secret field, never from an exchange response.
            if self.is_secret(&declared.field) != Some(false) {
                return Err(format!(
                    "attribute `{attribute}` must come from a field declared non-secret"
                ));
            }
        }

        // Outputs each step makes available to later steps, in order.
        let mut outputs: BTreeSet<String> = BTreeSet::new();
        let ids: Vec<&str> = recipe.steps.iter().map(|step| step.id.as_str()).collect();
        let mut uses_fixed_window = false;
        for (index, step) in recipe.steps.iter().enumerate() {
            if !is_name(&step.id) || ids[..index].contains(&step.id.as_str()) {
                return Err(format!("step id `{}` is malformed or repeated", step.id));
            }
            let later: Vec<&str> = ids[index + 1..].to_vec();
            self.check_step(step, &outputs, &later)
                .map_err(|detail| format!("step `{}`: {detail}", step.id))?;
            uses_fixed_window |=
                step.extract.values().any(|extract| extract.fixed_window == Some(true));
            if step.kind == StepKindV1::JwtSign {
                outputs.insert(format!("{}.jwt", step.id));
            }
            for output in step.extract.keys() {
                outputs.insert(format!("{}.{output}", step.id));
            }
        }
        if uses_fixed_window && recipe.fixed_validity_seconds.is_none() {
            return Err("a fixed_window clock needs fixed_validity_seconds".to_owned());
        }
        let present = recipe.present.as_deref().ok_or("present is required")?;
        if !outputs.contains(present) {
            return Err(format!("present `{present}` is not an output of any step"));
        }
        for (field, output) in &recipe.write_back {
            if self.is_secret(field) != Some(true) {
                return Err(format!("write_back target `{field}` must be a declared secret field"));
            }
            if !outputs.contains(output) {
                return Err(format!("write_back source `{output}` is not an output of any step"));
            }
        }
        check_assertions(recipe)
    }

    fn check_selector(&self, name: &str, recipe: &RecipeV1) -> Result<(), String> {
        let only_select = RecipeV1 { select: recipe.select.clone(), ..RecipeV1::default() };
        if *recipe != only_select {
            return Err("a selector holds only select".to_owned());
        }
        let (last, rules) = recipe.select.split_last().ok_or("select is empty")?;
        if last.when.is_some() || rules.iter().any(|rule| rule.when.is_none()) {
            return Err("every select rule but the last has a test; the last has none".to_owned());
        }
        for rule in &recipe.select {
            let target = self
                .recipes
                .get(&rule.recipe)
                .ok_or_else(|| format!("select names no recipe `{}`", rule.recipe))?;
            if rule.recipe == name || !target.select.is_empty() {
                return Err("a selector names only complete recipes".to_owned());
            }
            match &rule.when {
                Some(PredicateV1::FieldPresent(field)) if self.is_secret(field).is_none() => {
                    return Err(format!("select tests an undeclared field `{field}`"));
                }
                Some(PredicateV1::AllPresent(fields))
                    if fields.is_empty()
                        || fields.iter().any(|field| self.is_secret(field).is_none()) =>
                {
                    return Err("all_present names no field or an undeclared one".to_owned());
                }
                // Values only of non-secret fields: a selector must not branch on a secret.
                Some(PredicateV1::FieldIn { field, values })
                    if self.is_secret(field) != Some(false) || values.is_empty() =>
                {
                    return Err(format!(
                        "field_in may test only a non-secret field, `{field}` is not one"
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn check_source(
        &self,
        source: &ValueSourceV1,
        outputs: &BTreeSet<String>,
    ) -> Result<(), String> {
        let count = [
            source.constant.is_some(),
            source.field.is_some(),
            source.output.is_some(),
            source.now_plus.is_some(),
            source.endpoint_of.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count();
        if count != 1 {
            return Err("a value has exactly one source".to_owned());
        }
        if source.pointer.is_some() && source.field.is_none() {
            return Err("pointer is allowed only with field".to_owned());
        }
        if let Some(field) = &source.field {
            if !self.fields.contains_key(field) {
                return Err(format!("names an undeclared field `{field}`"));
            }
            if source.pointer.as_deref().is_some_and(|pointer| !is_pointer(pointer, false)) {
                return Err("a malformed pointer".to_owned());
            }
        }
        if let Some(output) = &source.output
            && !outputs.contains(output)
        {
            return Err(format!("`{output}` is not an output of an earlier step"));
        }
        Ok(())
    }

    fn check_step(
        &self,
        step: &StepV1,
        outputs: &BTreeSet<String>,
        later: &[&str],
    ) -> Result<(), String> {
        for field in &step.requires {
            if !self.fields.contains_key(field) {
                return Err(format!("requires an undeclared field `{field}`"));
            }
        }
        if step.kind == StepKindV1::JwtSign {
            self.check_jwt_step(step, outputs)?;
        } else {
            self.check_http_step(step)?;
        }
        for (name, source) in step.params.iter().chain(step.headers.iter()) {
            self.check_source(source, outputs).map_err(|detail| format!("`{name}` {detail}"))?;
            if source.endpoint_of.is_some() {
                return Err("endpoint_of is admitted only as a claim".to_owned());
            }
        }
        if let Some(name) = step.headers.keys().find(|name| !is_header_name(name)) {
            return Err(format!("header `{name}` is malformed or reserved"));
        }
        if let Some(auth) = &step.auth {
            if !is_header_name(&auth.scheme.to_ascii_lowercase()) {
                return Err("auth scheme is not an RFC 7235 token".to_owned());
            }
            self.check_source(&auth.value, outputs).map_err(|detail| format!("auth {detail}"))?;
        }
        for (status, action) in &step.on_status {
            let class = matches!(status.as_str(), "4xx" | "5xx");
            let exact = status.len() == 3
                && status.parse::<u16>().is_ok_and(|code| (100..=599).contains(&code));
            if !class && !exact {
                return Err(format!("on_status key `{status}` is not a status code or class"));
            }
            // Forward only, so the graph is acyclic by construction.
            if let StatusActionV1::Goto(target) = action
                && !later.contains(&target.as_str())
            {
                return Err(format!("goto `{target}` must name a later step"));
            }
        }
        self.check_extractions(step)
    }

    fn check_http_step(&self, step: &StepV1) -> Result<(), String> {
        let endpoint = step.endpoint.as_deref().ok_or("an HTTP step needs an endpoint")?;
        let params = template_params(endpoint)?;
        let named: BTreeSet<&str> = params.iter().map(|(name, _)| *name).collect();
        if named.len() != step.endpoint_params.len()
            || step.endpoint_params.keys().any(|key| !named.contains(key.as_str()))
        {
            return Err("endpoint_params must fill exactly the template's parameters".to_owned());
        }
        for (param, in_host) in params {
            let reference = &step.endpoint_params[param];
            let field = self
                .fields
                .get(&reference.field)
                .ok_or_else(|| format!("endpoint parameter `{param}` names an undeclared field"))?;
            // §3.3: template parameters take only non-secret fields validated by syntax; the host
            // part is never chosen by credential contents.
            let Some(syntax) =
                field.syntax.as_ref().filter(|_| !field.secret && reference.pointer.is_none())
            else {
                return Err(format!(
                    "endpoint parameter `{param}` must come from a whole non-secret field with a syntax"
                ));
            };
            if in_host && !syntax.is_label_safe() {
                return Err(format!(
                    "host parameter `{param}`'s syntax must fit inside a DNS label"
                ));
            }
        }
        match (step.kind, step.method) {
            (StepKindV1::Oauth2Token, None | Some(StepMethodV1::Post)) => {}
            (StepKindV1::Oauth2Token, Some(StepMethodV1::Get)) => {
                return Err("oauth2_token is a POST".to_owned());
            }
            (_, None) => return Err("an exchange declares its method".to_owned()),
            _ => {}
        }
        if !step.params.is_empty()
            && step.encoding.is_none()
            && step.method != Some(StepMethodV1::Get)
        {
            return Err("a step with a body declares its encoding".to_owned());
        }
        if step.alg.is_some() || step.key.is_some() || !step.claims.is_empty() {
            return Err("alg, key and claims belong to jwt_sign".to_owned());
        }
        Ok(())
    }

    fn check_jwt_step(&self, step: &StepV1, outputs: &BTreeSet<String>) -> Result<(), String> {
        if step.endpoint.is_some()
            || step.method.is_some()
            || !step.params.is_empty()
            || !step.headers.is_empty()
            || step.auth.is_some()
            || !step.on_status.is_empty()
            || !step.extract.is_empty()
        {
            return Err(
                "jwt_sign has no endpoint, method, params, headers, auth, on_status or extract"
                    .to_owned(),
            );
        }
        step.alg.ok_or("jwt_sign needs alg")?;
        let key = step.key.as_ref().ok_or("jwt_sign needs key")?;
        if key.field.as_deref().and_then(|field| self.is_secret(field)) != Some(true) {
            return Err("the signing key must come from a secret field".to_owned());
        }
        if step.claims.is_empty() {
            return Err("jwt_sign needs claims".to_owned());
        }
        for (claim, source) in &step.claims {
            self.check_source(source, outputs)
                .map_err(|detail| format!("claim `{claim}` {detail}"))?;
            // §3.4 rule 3: no constant subject.
            if claim == "sub" && source.constant.is_some() {
                return Err("sub may not be a constant".to_owned());
            }
            if source.endpoint_of.is_some() && claim != "aud" {
                return Err("endpoint_of is admitted only as aud".to_owned());
            }
        }
        Ok(())
    }

    fn check_extractions(&self, step: &StepV1) -> Result<(), String> {
        for (name, extract) in &step.extract {
            if !is_name(name) {
                return Err(format!("extraction `{name}` is not snake_case"));
            }
            let sources = [
                extract.pointer.as_ref(),
                extract.relative_seconds.as_ref(),
                extract.epoch_seconds.as_ref(),
                extract.epoch_millis.as_ref(),
                extract.jwt_exp.as_ref(),
            ];
            let count = sources.iter().filter(|source| source.is_some()).count()
                + usize::from(extract.fixed_window == Some(true))
                + usize::from(extract.jwt_claim.is_some());
            if count != 1 || extract.fixed_window == Some(false) {
                return Err(format!("extraction `{name}` has exactly one source"));
            }
            if sources.iter().flatten().any(|pointer| !is_pointer(pointer, false)) {
                return Err(format!("extraction `{name}` has a malformed pointer"));
            }
            if let Some(claim) = &extract.jwt_claim
                && (!is_pointer(&claim.token, false) || !is_pointer(&claim.pointer, false))
            {
                return Err(format!("extraction `{name}` has a malformed jwt_claim"));
            }
            if extract
                .must_equal_field
                .as_ref()
                .is_some_and(|field| !self.fields.contains_key(field))
            {
                return Err(format!("extraction `{name}` must equal an undeclared field"));
            }
        }
        Ok(())
    }
}

/// §3.4 rule 2: a signed assertion that is sent to a step carries `aud` = that step's
/// endpoint. An assertion that is only presented (Kling: the JWT is the bearer) is confined by
/// the inference endpoint instead and needs no `aud`.
fn check_assertions(recipe: &RecipeV1) -> Result<(), String> {
    for signer in recipe.steps.iter().filter(|step| step.kind == StepKindV1::JwtSign) {
        if let Some(target) =
            signer.claims.values().find_map(|source| source.endpoint_of.as_deref())
            && !recipe
                .steps
                .iter()
                .any(|step| step.id == target && step.kind != StepKindV1::JwtSign)
        {
            return Err(format!("endpoint_of `{target}` names no HTTP step of this recipe"));
        }
        let jwt = format!("{}.jwt", signer.id);
        let receivers: Vec<&str> = recipe
            .steps
            .iter()
            .filter(|step| {
                step.params
                    .values()
                    .chain(step.headers.values())
                    .chain(step.auth.iter().map(|auth| &auth.value))
                    .any(|source| source.output.as_deref() == Some(jwt.as_str()))
            })
            .map(|step| step.id.as_str())
            .collect();
        let aud = signer.claims.get("aud");
        match receivers.as_slice() {
            [] => {}
            [receiver] => {
                if aud.and_then(|aud| aud.endpoint_of.as_deref()) != Some(*receiver) {
                    return Err(format!(
                        "the assertion of step `{}` is sent to `{receiver}`, so its aud must be \
                         {{\"endpoint_of\": \"{receiver}\"}}",
                        signer.id
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "the assertion of step `{}` is sent to more than one step",
                    signer.id
                ));
            }
        }
    }
    Ok(())
}

fn is_name_like_file(kind: &str) -> bool {
    (1..=64).contains(&kind.len())
        && kind
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}
