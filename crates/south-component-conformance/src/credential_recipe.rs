//! The reference credential-recipe interpreter (B4, `docs/design/2026-09-30-host-zero-vendor-boundary.md`
//! §3.3, §3.5, §3.7).
//!
//! Gate ② judges a package's credential recipes by running them against fixtures. Running one
//! needs an interpreter, and this is south's: it runs **only in tests**, sees **only fixture fake
//! values**, and is not the production executor — that one is the host's, with its locks, its
//! storage, its egress guard and its confirmed endpoints (§3.6). What the two share is the meaning
//! of the vocabulary, which is what a fixture pins.
//!
//! The interpreter owns no effect. It has no network, no clock and no cryptography of its own:
//!
//! - the time is the `now` of [`RecipeEffectsV1`], in epoch seconds;
//! - a `jwt_sign` step builds the JWS compact form itself and asks a [`JwtSignerV1`] only for the
//!   signature bytes, so south gains no crypto dependency ([`FixtureSignerV1`] is a deterministic
//!   stand-in, not a signature);
//! - every exchange is rendered into a [`RenderedRequestV1`] and answered by a
//!   [`RecipeResponderV1`].
//!
//! What it decides, in order:
//!
//! 1. Field values: an empty value is absent, a declared `default` fills an absent non-secret
//!    field, and every present value must have its declared syntax. A missing `required` field or
//!    an unmet `require_one_of` group is a configuration error.
//! 2. The slot: a `static` slot mints nothing; a selector picks the first rule whose predicate
//!    holds (`field_in` compares ASCII case-insensitively).
//! 3. The steps, in order. A step whose `requires` is unmet stops the recipe before any request:
//!    as [`RecipeOutcomeV1::UseStored`] when the recipe declares `without_refresh_material:
//!    use_stored`, otherwise as a configuration error. A step that is the target of some `goto` is
//!    entered only through that `goto`: reaching it by falling through ends the recipe, so the
//!    alternative branch of a successful step does not run.
//! 4. Each response's status: an exact `on_status` entry, then a 2xx success, then the class entry
//!    (`4xx` / `5xx`), then the defaults (4xx is `reauth_required`, anything else `transient`).
//! 5. Extraction on success (an `http_probe` extracts nothing). A missing or `null` value fails the
//!    step as `transient` unless it is `optional`. A clock form that yields no expiry falls back to
//!    `default_seconds` and is otherwise `transient`. `must_equal_field` that disagrees with the
//!    stored field fails as `reauth_required` before anything is written back.
//! 6. The outcome: the first `present` candidate that is present and non-empty, its expiry clamped
//!    to the host's 60 s to 24 h range as narrowed by the recipe (§3.5), write-back values (an
//!    absent or empty output keeps the stored value: the no-wipe invariant), and the exported
//!    attributes, taken only from fields. With no candidate present the run is `transient`.
//!
//! The expiry of a presented step output is the clock extracted by the step that produced it, or
//! else `now + default_seconds`; with neither, the run is `transient`. A presented field (§13.5 D1)
//! has no step, so its expiry is `now + validity_seconds`, its own and never `default_seconds`.
//!
//! A slot that names a field (§13.5 D3) is not minted; [`stored_slot_value_v1`] reads it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use south_provider_api::{
    ConstantV1, CredentialsV1, EncodingV1, ExtractV1, HOST_MAX_TTL_SECONDS, HOST_MIN_TTL_SECONDS,
    JwtAlgorithmV1, PredicateV1, PresentCandidateV1, PresentFieldV1, RecipeV1, SlotV1,
    StatusActionV1, StepKindV1, StepMethodV1, StepV1, ValueSourceV1, WithoutRefreshMaterialV1,
};

use crate::url_segment;

/// Signs a JWS signing input. The interpreter builds the header, the claims and the signing input;
/// the signer only turns `signing_input` into signature bytes with `key`.
pub trait JwtSignerV1 {
    /// # Errors
    ///
    /// A key the signer cannot use; the recipe then stops as a configuration error.
    fn sign(
        &self,
        alg: JwtAlgorithmV1,
        key: &[u8],
        signing_input: &[u8],
    ) -> Result<Vec<u8>, String>;
}

/// The signer gate ② fixtures are judged with. **Not a signature.**
///
/// It is FNV-1a (64 bit) over the algorithm name, the key and the signing input, and secure in no
/// sense; it only makes the signature part of a fixture depend on the key and on every signed
/// byte, deterministically.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FixtureSignerV1;

impl JwtSignerV1 for FixtureSignerV1 {
    fn sign(
        &self,
        alg: JwtAlgorithmV1,
        key: &[u8],
        signing_input: &[u8],
    ) -> Result<Vec<u8>, String> {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let parts: [&[u8]; 5] = [alg_name(alg).as_bytes(), &[0], key, &[0], signing_input];
        for byte in parts.into_iter().flatten() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        Ok(hash.to_be_bytes().to_vec())
    }
}

/// One exchange request, exactly as the recipe renders it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderedRequestV1 {
    /// The step that sends it.
    pub step: String,
    /// `GET` or `POST`.
    pub method: String,
    /// The endpoint with its template filled; a `GET` carries its params as the query.
    pub url: String,
    /// Lowercase names. `authorization` is the step's `auth` (`<scheme> <value>`), and
    /// `content-type` names the body's encoding.
    pub headers: BTreeMap<String, String>,
    /// The encoded body: `application/x-www-form-urlencoded` or compact JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// A fake response to one exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FakeResponseV1 {
    pub status: u16,
    /// The JSON body; `null` when there is none.
    #[serde(default)]
    pub body: Value,
}

/// Answers the exchanges a recipe makes. `None` is a transport failure: no response arrived.
pub trait RecipeResponderV1 {
    fn respond(&mut self, request: &RenderedRequestV1) -> Option<FakeResponseV1>;
}

impl<F> RecipeResponderV1 for F
where
    F: FnMut(&RenderedRequestV1) -> Option<FakeResponseV1>,
{
    fn respond(&mut self, request: &RenderedRequestV1) -> Option<FakeResponseV1> {
        self(request)
    }
}

/// Everything outside the recipe the interpreter needs, injected.
pub struct RecipeEffectsV1<'a> {
    /// Now, in epoch seconds.
    pub now: i64,
    pub signer: &'a dyn JwtSignerV1,
    pub responder: &'a mut dyn RecipeResponderV1,
}

/// A minted slot value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MintedV1 {
    /// The `present` output: what the slot holds.
    pub present: String,
    /// When it expires, in epoch seconds, after the TTL clamp.
    pub expires_at: i64,
    /// Every output the steps produced, keyed `step.name`; a clock output is epoch seconds before
    /// the clamp.
    pub outputs: BTreeMap<String, Value>,
    /// Fields the host writes back. A field whose output was absent or empty is not listed: the
    /// stored value stays (§3.5).
    pub write_back: BTreeMap<String, String>,
    /// Exported attributes, taken from fields only.
    pub attributes: BTreeMap<String, String>,
}

/// How a run ended. The `detail` strings are for people and are not part of a fixture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeOutcomeV1 {
    Minted(MintedV1),
    /// The credential has no refresh material and the recipe says to keep using the stored value.
    UseStored,
    /// The stored credential cannot run this recipe; no retry helps until the operator edits it.
    Configuration {
        #[serde(skip_serializing_if = "Option::is_none")]
        field: Option<String>,
        #[serde(skip)]
        detail: String,
    },
    /// The upstream refused the credential; no retry until the operator acts.
    ReauthRequired {
        step: String,
        status: u16,
        #[serde(skip)]
        detail: String,
    },
    /// Try again later.
    Transient {
        #[serde(skip_serializing_if = "Option::is_none")]
        step: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<u16>,
        #[serde(skip)]
        detail: String,
    },
}

impl RecipeOutcomeV1 {
    /// Why the run did not mint; empty for a minted value or `use_stored`.
    #[must_use]
    pub fn detail(&self) -> &str {
        match self {
            Self::Minted(_) | Self::UseStored => "",
            Self::Configuration { detail, .. }
            | Self::ReauthRequired { detail, .. }
            | Self::Transient { detail, .. } => detail,
        }
    }
}

/// One run of a slot's recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecipeRunV1 {
    /// The recipe that ran, after any selector; `None` when none was reached.
    pub recipe: Option<String>,
    /// Every exchange, in order.
    pub requests: Vec<RenderedRequestV1>,
    pub outcome: RecipeOutcomeV1,
}

/// Mints `slot` from the stored `fields` with `credentials`' recipe.
///
/// Never panics: a recipe that gate ① would refuse ends as a configuration error.
#[must_use]
pub fn run_recipe_v1(
    credentials: &CredentialsV1,
    slot: &str,
    fields: &BTreeMap<String, String>,
    effects: &mut RecipeEffectsV1<'_>,
) -> RecipeRunV1 {
    let mut requests = Vec::new();
    let mut chosen = None;
    let outcome = match mint(credentials, slot, fields, effects, &mut requests, &mut chosen) {
        Ok(minted) => RecipeOutcomeV1::Minted(minted),
        Err(stop) => stop,
    };
    RecipeRunV1 { recipe: chosen, requests, outcome }
}

/// The value of a slot that names a field (§13.5 D3).
///
/// It is that field's stored value, under the same field rules as a run: an empty value is absent,
/// a default fills a non-secret field, every present value has its syntax, and `required` and
/// `require_one_of` hold.
///
/// # Errors
///
/// A configuration outcome when the slot names no field (it is minted, static or undeclared) or
/// the stored fields break a rule.
pub fn stored_slot_value_v1(
    credentials: &CredentialsV1,
    slot: &str,
    fields: &BTreeMap<String, String>,
) -> Result<String, RecipeOutcomeV1> {
    let Some(SlotV1::Field(field)) = credentials.slots.get(slot) else {
        return Err(configuration(None, format!("slot `{slot}` does not name a field")));
    };
    resolve_fields(credentials, fields)?
        .get(field.as_str())
        .map(|value| (*value).to_owned())
        .ok_or_else(|| configuration(Some(field), format!("field `{field}` is absent")))
}

/// The headers `alg` names in a JWS header.
const fn alg_name(alg: JwtAlgorithmV1) -> &'static str {
    match alg {
        JwtAlgorithmV1::HS256 => "HS256",
        JwtAlgorithmV1::RS256 => "RS256",
        JwtAlgorithmV1::ES256 => "ES256",
    }
}

/// The registered claims of RFC 7519 §4.1, serialized first and in this order; every other claim
/// follows in byte order. That is deterministic, and for the claims Kling uses (`iss`, `exp`,
/// `nbf`) it is the order the host serializes them in today.
const REGISTERED_CLAIMS: [&str; 7] = ["iss", "sub", "aud", "exp", "nbf", "iat", "jti"];

fn configuration(field: Option<&str>, detail: impl Into<String>) -> RecipeOutcomeV1 {
    RecipeOutcomeV1::Configuration { field: field.map(str::to_owned), detail: detail.into() }
}

fn transient(
    step: Option<&str>,
    status: Option<u16>,
    detail: impl Into<String>,
) -> RecipeOutcomeV1 {
    RecipeOutcomeV1::Transient { step: step.map(str::to_owned), status, detail: detail.into() }
}

fn mint(
    credentials: &CredentialsV1,
    slot: &str,
    stored: &BTreeMap<String, String>,
    effects: &mut RecipeEffectsV1<'_>,
    requests: &mut Vec<RenderedRequestV1>,
    chosen: &mut Option<String>,
) -> Result<MintedV1, RecipeOutcomeV1> {
    let fields = resolve_fields(credentials, stored)?;
    let name = match credentials.slots.get(slot) {
        Some(SlotV1::Minted(name)) => name.as_str(),
        Some(SlotV1::Static | SlotV1::Field(_)) | None => {
            return Err(configuration(None, format!("slot `{slot}` is not minted by a recipe")));
        }
    };
    let (name, recipe) = select(credentials, name, &fields)?;
    *chosen = Some(name.to_owned());
    let mut run = Run {
        credentials,
        recipe,
        fields,
        now: effects.now,
        outputs: BTreeMap::new(),
        clocks: BTreeMap::new(),
    };
    run.steps(effects, requests)?;
    run.outcome()
}

/// Present field values: non-empty, or the declared default, each with its declared syntax.
fn resolve_fields<'a>(
    credentials: &'a CredentialsV1,
    stored: &'a BTreeMap<String, String>,
) -> Result<BTreeMap<&'a str, &'a str>, RecipeOutcomeV1> {
    let mut present = BTreeMap::new();
    for (name, declared) in &credentials.fields {
        let value = stored
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
            .or_else(|| declared.default.as_deref().filter(|value| !value.is_empty()));
        match value {
            Some(value) => {
                if declared.syntax.as_ref().is_some_and(|syntax| !syntax.admits(value)) {
                    return Err(configuration(
                        Some(name),
                        format!("field `{name}` does not have its declared syntax"),
                    ));
                }
                present.insert(name.as_str(), value);
            }
            None if declared.required => {
                return Err(configuration(Some(name), format!("required field `{name}` is empty")));
            }
            None => {}
        }
    }
    for group in &credentials.require_one_of {
        if !group.iter().any(|field| present.contains_key(field.as_str())) {
            return Err(configuration(
                group.first().map(String::as_str),
                format!("none of {group:?} is present"),
            ));
        }
    }
    Ok(present)
}

fn select<'a>(
    credentials: &'a CredentialsV1,
    name: &'a str,
    fields: &BTreeMap<&str, &str>,
) -> Result<(&'a str, &'a RecipeV1), RecipeOutcomeV1> {
    let recipe = credentials
        .recipes
        .get(name)
        .ok_or_else(|| configuration(None, format!("no recipe `{name}`")))?;
    if recipe.select.is_empty() {
        return Ok((name, recipe));
    }
    let present = |field: &String| fields.contains_key(field.as_str());
    let rule = recipe
        .select
        .iter()
        .find(|rule| match &rule.when {
            None => true,
            Some(PredicateV1::FieldPresent(field)) => present(field),
            Some(PredicateV1::AllPresent(all)) => all.iter().all(present),
            Some(PredicateV1::FieldIn { field, values }) => fields
                .get(field.as_str())
                .is_some_and(|value| values.iter().any(|v| v.eq_ignore_ascii_case(value))),
        })
        .ok_or_else(|| configuration(None, format!("no rule of selector `{name}` matched")))?;
    let target = credentials
        .recipes
        .get_key_value(&rule.recipe)
        .filter(|(_, target)| target.select.is_empty())
        .ok_or_else(|| {
            configuration(
                None,
                format!("selector `{name}` names no complete recipe `{}`", rule.recipe),
            )
        })?;
    Ok((target.0.as_str(), target.1))
}

/// What a clock form read.
#[derive(Clone, Copy)]
enum Clock {
    Other,
    Expiry(i64),
    /// A clock form whose value is absent or malformed.
    Missing,
}

/// What a status does to a step.
enum Transition<'a> {
    Success,
    Goto(&'a str),
    ReauthRequired,
    Transient,
}

fn transition(step: &StepV1, status: u16) -> Transition<'_> {
    let class = match status {
        400..=499 => Some("4xx"),
        500..=599 => Some("5xx"),
        _ => None,
    };
    let declared = step.on_status.get(&status.to_string()).or_else(|| {
        if (200..300).contains(&status) {
            None
        } else {
            class.and_then(|key| step.on_status.get(key))
        }
    });
    match declared {
        Some(StatusActionV1::Goto(target)) => Transition::Goto(target),
        None if (200..300).contains(&status) => Transition::Success,
        Some(StatusActionV1::ReauthRequired) => Transition::ReauthRequired,
        None if class == Some("4xx") => Transition::ReauthRequired,
        Some(StatusActionV1::Transient) | None => Transition::Transient,
    }
}

struct Run<'a> {
    credentials: &'a CredentialsV1,
    recipe: &'a RecipeV1,
    fields: BTreeMap<&'a str, &'a str>,
    now: i64,
    outputs: BTreeMap<String, Value>,
    /// The expiry each step's clock extraction produced, by step id.
    clocks: BTreeMap<String, i64>,
}

impl Run<'_> {
    fn field(&self, name: &str) -> Result<&str, RecipeOutcomeV1> {
        self.fields
            .get(name)
            .copied()
            .ok_or_else(|| configuration(Some(name), format!("field `{name}` is absent")))
    }

    fn steps(
        &mut self,
        effects: &mut RecipeEffectsV1<'_>,
        requests: &mut Vec<RenderedRequestV1>,
    ) -> Result<(), RecipeOutcomeV1> {
        let steps = &self.recipe.steps;
        let goto_targets: BTreeSet<&str> = steps
            .iter()
            .flat_map(|step| step.on_status.values())
            .filter_map(|action| match action {
                StatusActionV1::Goto(target) => Some(target.as_str()),
                _ => None,
            })
            .collect();
        let mut index = 0;
        let mut entered_by_goto = false;
        while let Some(step) = steps.get(index) {
            if !entered_by_goto && index > 0 && goto_targets.contains(step.id.as_str()) {
                break;
            }
            entered_by_goto = false;
            if let Some(missing) =
                step.requires.iter().find(|field| !self.fields.contains_key(field.as_str()))
            {
                return Err(
                    if self.recipe.without_refresh_material
                        == Some(WithoutRefreshMaterialV1::UseStored)
                    {
                        RecipeOutcomeV1::UseStored
                    } else {
                        configuration(
                            Some(missing),
                            format!("step `{}` requires field `{missing}`", step.id),
                        )
                    },
                );
            }
            if step.kind == StepKindV1::JwtSign {
                let jwt = self.sign(step, effects.signer)?;
                self.outputs.insert(format!("{}.jwt", step.id), Value::String(jwt));
                index += 1;
                continue;
            }
            let request = self.render(step)?;
            requests.push(request.clone());
            let Some(response) = effects.responder.respond(&request) else {
                return Err(transient(Some(&step.id), None, "no response arrived"));
            };
            match transition(step, response.status) {
                Transition::Success => {
                    if step.kind != StepKindV1::HttpProbe {
                        self.extract(step, &response)?;
                    }
                    index += 1;
                }
                Transition::Goto(target) => {
                    index = steps
                        .iter()
                        .position(|candidate| candidate.id == target)
                        .filter(|position| *position > index)
                        .ok_or_else(|| {
                            configuration(None, format!("goto `{target}` names no later step"))
                        })?;
                    entered_by_goto = true;
                }
                Transition::ReauthRequired => {
                    return Err(RecipeOutcomeV1::ReauthRequired {
                        step: step.id.clone(),
                        status: response.status,
                        detail: format!("status {} requires re-authentication", response.status),
                    });
                }
                Transition::Transient => {
                    return Err(transient(
                        Some(&step.id),
                        Some(response.status),
                        format!("status {} is transient", response.status),
                    ));
                }
            }
        }
        Ok(())
    }

    fn source(&self, source: &ValueSourceV1) -> Result<Value, RecipeOutcomeV1> {
        if let Some(constant) = &source.constant {
            return Ok(match constant {
                ConstantV1::Text(text) => Value::String(text.clone()),
                ConstantV1::Integer(integer) => Value::from(*integer),
                ConstantV1::Boolean(boolean) => Value::Bool(*boolean),
            });
        }
        if let Some(field) = &source.field {
            let value = self.field(field)?;
            let Some(pointer) = &source.pointer else {
                return Ok(Value::String(value.to_owned()));
            };
            let document: Value = serde_json::from_str(value).map_err(|_| {
                configuration(Some(field), format!("field `{field}` is not a JSON document"))
            })?;
            return document.pointer(pointer).filter(|value| !value.is_null()).cloned().ok_or_else(
                || {
                    configuration(
                        Some(field),
                        format!("field `{field}` has nothing at `{pointer}`"),
                    )
                },
            );
        }
        if let Some(output) = &source.output {
            return self.outputs.get(output).cloned().ok_or_else(|| {
                let step = output.split_once('.').map_or(output.as_str(), |(step, _)| step);
                transient(Some(step), None, format!("output `{output}` is absent"))
            });
        }
        if let Some(offset) = source.now_plus {
            return Ok(Value::from(self.now.saturating_add(offset)));
        }
        if let Some(step) = &source.endpoint_of {
            return self.url_of(step).map(Value::String);
        }
        Err(configuration(None, "a value names no source"))
    }

    /// The step's endpoint with its template parameters filled from syntax-checked fields.
    fn url_of(&self, step_id: &str) -> Result<String, RecipeOutcomeV1> {
        let step = self
            .recipe
            .steps
            .iter()
            .find(|step| step.id == step_id)
            .ok_or_else(|| configuration(None, format!("no step `{step_id}`")))?;
        let endpoint = step
            .endpoint
            .as_deref()
            .ok_or_else(|| configuration(None, format!("step `{step_id}` has no endpoint")))?;
        let malformed = || configuration(None, format!("step `{step_id}`'s template is malformed"));
        let mut url = String::with_capacity(endpoint.len());
        let mut rest = endpoint;
        while let Some(open) = rest.find('{') {
            url.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            let close = after.find('}').ok_or_else(malformed)?;
            let reference = step.endpoint_params.get(&after[..close]).ok_or_else(malformed)?;
            let checked = self
                .credentials
                .fields
                .get(&reference.field)
                .is_some_and(|field| field.syntax.is_some() && !field.secret);
            if !checked || reference.pointer.is_some() {
                return Err(malformed());
            }
            url.push_str(&url_segment::encode(self.field(&reference.field)?));
            rest = &after[close + 1..];
        }
        url.push_str(rest);
        Ok(url)
    }

    fn render(&self, step: &StepV1) -> Result<RenderedRequestV1, RecipeOutcomeV1> {
        let method = match (step.kind, step.method) {
            (StepKindV1::Oauth2Token, _) | (_, Some(StepMethodV1::Post)) => "POST",
            (_, Some(StepMethodV1::Get)) => "GET",
            (_, None) => {
                return Err(configuration(None, format!("step `{}` has no method", step.id)));
            }
        };
        let mut url = self.url_of(&step.id)?;
        let mut headers = BTreeMap::new();
        for (name, source) in &step.headers {
            headers.insert(name.clone(), text(&self.source(source)?));
        }
        if let Some(auth) = &step.auth {
            let value = text(&self.source(&auth.value)?);
            headers.insert("authorization".to_owned(), format!("{} {value}", auth.scheme));
        }
        let mut params = serde_json::Map::new();
        for (name, source) in &step.params {
            params.insert(name.clone(), self.source(source)?);
        }
        let body = if params.is_empty() {
            None
        } else if method == "GET" {
            url.push('?');
            url.push_str(&form(&params));
            None
        } else if step.encoding == Some(EncodingV1::Json) {
            headers.insert("content-type".to_owned(), "application/json".to_owned());
            Some(Value::Object(params).to_string())
        } else {
            headers
                .insert("content-type".to_owned(), "application/x-www-form-urlencoded".to_owned());
            Some(form(&params))
        };
        Ok(RenderedRequestV1 {
            step: step.id.clone(),
            method: method.to_owned(),
            url,
            headers,
            body,
        })
    }

    /// The JWS compact form: `base64url(header) . base64url(claims) . base64url(signature)`.
    fn sign(&self, step: &StepV1, signer: &dyn JwtSignerV1) -> Result<String, RecipeOutcomeV1> {
        let invalid =
            || configuration(None, format!("step `{}` is not a complete jwt_sign", step.id));
        let alg = step.alg.ok_or_else(invalid)?;
        let key_source = step.key.as_ref().ok_or_else(invalid)?;
        let key = text(&self.source(key_source)?);
        let ordered =
            REGISTERED_CLAIMS.iter().filter_map(|name| step.claims.get_key_value(*name)).chain(
                step.claims.iter().filter(|(name, _)| !REGISTERED_CLAIMS.contains(&name.as_str())),
            );
        let mut claims = String::from("{");
        for (name, source) in ordered {
            if claims.len() > 1 {
                claims.push(',');
            }
            claims.push_str(&Value::String(name.clone()).to_string());
            claims.push(':');
            claims.push_str(&self.source(source)?.to_string());
        }
        claims.push('}');
        // The order `jsonwebtoken`'s `Header::new` serializes, which the host uses today.
        let header = format!("{{\"typ\":\"JWT\",\"alg\":\"{}\"}}", alg_name(alg));
        let signing_input = format!(
            "{}.{}",
            base64url_encode(header.as_bytes()),
            base64url_encode(claims.as_bytes())
        );
        let signature =
            signer.sign(alg, key.as_bytes(), signing_input.as_bytes()).map_err(|error| {
                configuration(key_source.field.as_deref(), format!("the key cannot sign: {error}"))
            })?;
        Ok(format!("{signing_input}.{}", base64url_encode(&signature)))
    }

    fn extract(&mut self, step: &StepV1, response: &FakeResponseV1) -> Result<(), RecipeOutcomeV1> {
        let status = response.status;
        for (name, rule) in &step.extract {
            let clock = self.clock(rule, &response.body);
            let value = if let Clock::Expiry(_) | Clock::Missing = clock {
                let expiry = match (clock, self.recipe.default_seconds) {
                    (Clock::Expiry(expiry), _) => expiry,
                    (_, Some(default)) => self.now.saturating_add(i64::from(default)),
                    (_, None) => {
                        return Err(transient(
                            Some(&step.id),
                            Some(status),
                            format!("clock `{name}` yields no expiry and there is no default"),
                        ));
                    }
                };
                self.clocks.insert(step.id.clone(), expiry);
                Some(Value::from(expiry))
            } else if let Some(claim) = &rule.jwt_claim {
                response
                    .body
                    .pointer(&claim.token)
                    .and_then(Value::as_str)
                    .and_then(jwt_payload)
                    .and_then(|payload| payload.pointer(&claim.pointer).cloned())
                    .filter(|value| !value.is_null())
            } else {
                rule.pointer
                    .as_deref()
                    .and_then(|pointer| response.body.pointer(pointer))
                    .filter(|value| !value.is_null())
                    .cloned()
            };
            let Some(value) = value else {
                if rule.optional {
                    continue;
                }
                return Err(transient(
                    Some(&step.id),
                    Some(status),
                    format!("the response lacks `{name}`"),
                ));
            };
            if let Some(field) = &rule.must_equal_field
                && let Some(stored) = self.fields.get(field.as_str())
                && text(&value) != *stored
            {
                return Err(RecipeOutcomeV1::ReauthRequired {
                    step: step.id.clone(),
                    status,
                    detail: format!("`{name}` does not equal the stored `{field}`"),
                });
            }
            self.outputs.insert(format!("{}.{name}", step.id), value);
        }
        Ok(())
    }

    /// What a clock form reads from `body`; [`Clock::Other`] for an extraction that is no clock.
    fn clock(&self, rule: &ExtractV1, body: &Value) -> Clock {
        let integer = |pointer: &String| body.pointer(pointer).and_then(Value::as_i64);
        let read = if let Some(pointer) = &rule.relative_seconds {
            integer(pointer).map(|seconds| self.now.saturating_add(seconds))
        } else if let Some(pointer) = &rule.epoch_seconds {
            integer(pointer)
        } else if let Some(pointer) = &rule.epoch_millis {
            integer(pointer).map(|millis| millis.div_euclid(1000))
        } else if let Some(pointer) = &rule.jwt_exp {
            body.pointer(pointer)
                .and_then(Value::as_str)
                .and_then(jwt_payload)
                .and_then(|payload| payload.get("exp").and_then(Value::as_i64))
        } else if rule.fixed_window == Some(true) {
            self.recipe
                .fixed_validity_seconds
                .map(|window| self.now.saturating_add(i64::from(window)))
        } else {
            return Clock::Other;
        };
        read.map_or(Clock::Missing, Clock::Expiry)
    }

    /// The first `present` candidate that is present and non-empty, and its expiry.
    fn presented(&self) -> Result<(String, i64), RecipeOutcomeV1> {
        let candidates = self
            .recipe
            .present
            .as_ref()
            .map(south_provider_api::PresentV1::as_slice)
            .filter(|candidates| !candidates.is_empty())
            .ok_or_else(|| configuration(None, "the recipe names no present output"))?;
        for candidate in candidates {
            match candidate {
                PresentCandidateV1::Output(name) => {
                    let Some(value) =
                        self.outputs.get(name).and_then(Value::as_str).filter(|v| !v.is_empty())
                    else {
                        continue;
                    };
                    let step = step_of(name);
                    let expiry = match (self.clocks.get(step), self.recipe.default_seconds) {
                        (Some(expiry), _) => *expiry,
                        (None, Some(default)) => self.now.saturating_add(i64::from(default)),
                        (None, None) => {
                            return Err(transient(
                                Some(step),
                                None,
                                "the presented value has no clock and the recipe no default_seconds",
                            ));
                        }
                    };
                    return Ok((value.to_owned(), expiry));
                }
                PresentCandidateV1::Field(PresentFieldV1 { field, validity_seconds }) => {
                    if let Some(value) = self.fields.get(field.as_str()) {
                        let expiry = self.now.saturating_add(i64::from(*validity_seconds));
                        return Ok(((*value).to_owned(), expiry));
                    }
                }
            }
        }
        // As v0.43.0 reported a single absent output: transient, at the first output's step.
        let step = candidates.iter().find_map(|candidate| match candidate {
            PresentCandidateV1::Output(name) => Some(step_of(name)),
            PresentCandidateV1::Field(_) => None,
        });
        Err(transient(step, None, "no present candidate is present"))
    }

    fn outcome(&self) -> Result<MintedV1, RecipeOutcomeV1> {
        let (present, expiry) = self.presented()?;
        // §3.5: the host's range, narrowed (never widened) by the recipe.
        let floor =
            self.recipe.min_ttl_seconds.unwrap_or(HOST_MIN_TTL_SECONDS).max(HOST_MIN_TTL_SECONDS);
        let ceiling = self
            .recipe
            .max_ttl_seconds
            .unwrap_or(HOST_MAX_TTL_SECONDS)
            .min(HOST_MAX_TTL_SECONDS)
            .max(floor);
        let ttl = expiry.saturating_sub(self.now).clamp(i64::from(floor), i64::from(ceiling));

        let write_back = self
            .recipe
            .write_back
            .iter()
            .filter_map(|(field, output)| {
                let value = self.outputs.get(output).map(text).filter(|value| !value.is_empty())?;
                Some((field.clone(), value))
            })
            .collect();
        let attributes = self
            .recipe
            .attributes
            .iter()
            .filter(|(_, attribute)| {
                attribute.export
                    && self
                        .credentials
                        .fields
                        .get(&attribute.field)
                        .is_some_and(|field| !field.secret)
            })
            .filter_map(|(name, attribute)| {
                let value = self.fields.get(attribute.field.as_str())?;
                Some((name.clone(), (*value).to_owned()))
            })
            .collect();
        Ok(MintedV1 {
            present,
            expires_at: self.now.saturating_add(ttl),
            outputs: self.outputs.clone(),
            write_back,
            attributes,
        })
    }
}

/// The step of a `step.output` name.
fn step_of(output: &str) -> &str {
    output.split_once('.').map_or(output, |(step, _)| step)
}

/// A value as header, form or key text: a string as is, anything else as compact JSON.
fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// `application/x-www-form-urlencoded`, in the params' key order.
fn form(params: &serde_json::Map<String, Value>) -> String {
    let encode = |value: &str| {
        let mut encoded = String::with_capacity(value.len());
        for byte in value.bytes() {
            match byte {
                b' ' => encoded.push('+'),
                b'*' | b'-' | b'.' | b'_' => encoded.push(char::from(byte)),
                _ if byte.is_ascii_alphanumeric() => encoded.push(char::from(byte)),
                _ => {
                    encoded.push('%');
                    encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                    encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
                }
            }
        }
        encoded
    };
    params
        .iter()
        .map(|(name, value)| format!("{}={}", encode(name), encode(&text(value))))
        .collect::<Vec<_>>()
        .join("&")
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64url without padding (RFC 7515 §2).
fn base64url_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = chunk
            .iter()
            .enumerate()
            .fold(0usize, |group, (index, byte)| group | (usize::from(*byte) << (16 - 8 * index)));
        for index in 0..=chunk.len() {
            encoded.push(char::from(BASE64URL[(group >> (18 - 6 * index)) & 0x3f]));
        }
    }
    encoded
}

/// Base64url, padded or not; `None` on any other byte.
fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    let mut decoded = Vec::with_capacity(text.len() * 3 / 4);
    let mut group = 0u32;
    let mut bits = 0u32;
    for byte in text.bytes() {
        let sextet = BASE64URL.iter().position(|candidate| *candidate == byte)?;
        group = (group << 6) | u32::try_from(sextet).ok()?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            decoded.push(u8::try_from((group >> bits) & 0xff).ok()?);
        }
    }
    Some(decoded)
}

/// A JWT's payload, decoded without verifying anything.
fn jwt_payload(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let (_, payload) = (parts.next()?, parts.next()?);
    serde_json::from_slice(&base64url_decode(payload)?).ok()
}

#[cfg(test)]
mod tests {
    use super::{base64url_decode, base64url_encode};

    #[test]
    fn base64url_round_trips_every_tail_length() {
        for (bytes, encoded) in [
            (&b""[..], ""),
            (b"f", "Zg"),
            (b"fo", "Zm8"),
            (b"foo", "Zm9v"),
            (b"\xfb\xff", "-_8"),
            (b"{\"typ\":\"JWT\",\"alg\":\"HS256\"}", "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9"),
        ] {
            assert_eq!(base64url_encode(bytes), encoded);
            assert_eq!(base64url_decode(encoded).as_deref(), Some(bytes));
        }
        assert_eq!(base64url_decode("Zg==").as_deref(), Some(&b"f"[..]));
        assert_eq!(base64url_decode("Z+g"), None);
    }
}
