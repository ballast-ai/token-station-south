//! Gate ② for the image world: `south.image-component.v1` (image record §12.1).
//!
//! Capabilities and prepare cases feed one component function each. A response case names the
//! prepare case it answers: the suite prepares that case, builds the response view a host would —
//! the declared body form and elision paths, through `build_media_response_view_v1` — and hands it
//! to `parse-response`, so a fixture pins what a host would see, not a view the fixture author
//! wrote by hand. A render case names the response cases it renders.
//!
//! The suite always runs with the package's manifest: its `auth_arms` are what
//! `DescriptorAuthWithinManifest` judges, its declared keys what `UndeclaredValuesIgnored` stays
//! clear of, its `secret_headers` what the response view drops.

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::{Value, json};
use south_contracts::image::{
    ImageCallContextV1, ImageModelCapabilitiesV1, ImageRenderContextV1, MeteringFormV1,
    PreparedImageCallV1, check_render_template_v1,
};
use south_contracts::media::{MediaLimitsV1, UpstreamRoundV1, build_media_response_view_v1};
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

use crate::image_fixture::{ImageCaseMetaV1, ImageCaseV1, ImageFamilyV1, ImageFixturePackV1};
use crate::image_json as wire;
use crate::{
    CheckV1, ImageComponentV1, ImageOutcomeV1, OutcomeV1, ReportV1, admit_media_descriptor_auth,
    credential_recipe_checks_v1,
};

/// The suite identifier, equal to the manifest's `conformance.required_suite`.
pub const IMAGE_COMPONENT_SUITE_V1: &str = south_provider_api::IMAGE_BEHAVIOR_SUITE;

/// The key injected to prove a component tolerates a newer host's field.
const UNKNOWN_FIELD: &str = "__conformance_unknown_field";
/// The prefix of the key injected to prove a component ignores an undeclared `declared` key.
const UNDECLARED_VALUE: &str = "conformance_undeclared_";
/// The placeholder namespace no output may carry (image record §6.2, §12.1).
const RESERVED_PREFIX: &str = "$south.";

/// What one case produced as JSON; `Err` is a fixture or boundary failure, never the component's
/// own refusal, which is a value (`{"error": …}`).
type Invoked = Result<Value, String>;

fn parse<T: for<'de> Deserialize<'de>>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone())
        .map_err(|error| format!("fixture input has the wrong shape: {error}"))
}

fn refused(error: &ErrorEnvelope) -> Value {
    json!({ "error": error })
}

#[derive(Deserialize)]
struct CapabilitiesInput {
    provider_config: ProviderConfig,
}

#[derive(Deserialize)]
struct PrepareInput {
    provider_config: ProviderConfig,
    request: Value,
    context: ImageCallContextV1,
}

#[derive(Deserialize)]
struct UpstreamInput {
    status: u16,
    #[serde(default)]
    headers: serde_json::Map<String, Value>,
    #[serde(default)]
    body: Option<Value>,
    #[serde(default)]
    body_base64: Option<String>,
    #[serde(default)]
    content_type: Option<String>,
}

#[derive(Deserialize)]
struct ResponseInput {
    prepare_case: String,
    response: UpstreamInput,
}

#[derive(Deserialize)]
struct RenderInput {
    response_cases: Vec<String>,
    context: ImageRenderContextV1,
}

/// One response case run end to end.
struct Answered {
    input: PrepareInput,
    prepared: PreparedImageCallV1,
    /// The upstream body as the upstream sent it (for `url` pointers).
    raw_body: Value,
    /// The view handed to the component.
    view: Value,
    outcome: Result<ImageOutcomeV1, ErrorEnvelope>,
}

/// The suite's view of one run.
struct Suite<'a> {
    component: &'a dyn ImageComponentV1,
    pack: &'a ImageFixturePackV1,
    manifest: &'a ComponentManifestV1,
}

impl Suite<'_> {
    fn prepare(&self, input: &PrepareInput) -> Result<PreparedImageCallV1, ErrorEnvelope> {
        self.component.prepare(&input.provider_config, &input.request, &input.context)
    }

    fn prepare_case(&self, name: &str) -> Result<(PrepareInput, PreparedImageCallV1), String> {
        let case =
            self.pack.case(name).ok_or_else(|| format!("names the missing case `{name}`"))?;
        if case.family != ImageFamilyV1::Prepare {
            return Err(format!("names `{name}`, which is not a prepare case"));
        }
        let input: PrepareInput = parse(&case.input)?;
        let prepared = self
            .prepare(&input)
            .map_err(|error| format!("the prepare case `{name}` is refused: {}", error.message))?;
        Ok((input, prepared))
    }

    fn answer(&self, input: &Value) -> Result<Answered, String> {
        let response: ResponseInput = parse(input)?;
        let (input, prepared) = self.prepare_case(&response.prepare_case)?;
        let upstream = response.response;
        let (raw_body, body_bytes) = match (&upstream.body, &upstream.body_base64) {
            (Some(body), None) => {
                (body.clone(), serde_json::to_vec(body).map_err(|error| error.to_string())?)
            }
            (None, Some(encoded)) => (
                Value::Null,
                south_contracts::media::decode_base64(encoded)
                    .map_err(|error| error.to_string())?,
            ),
            (None, None) => (Value::Null, Vec::new()),
            (Some(_), Some(_)) => {
                return Err("a response carries both `body` and `body_base64`".into());
            }
        };
        let headers: Vec<(String, String)> = upstream
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), value.as_str().unwrap_or_default().to_owned()))
            .collect();
        let secret_headers: Vec<&str> =
            self.manifest.secret_headers.iter().map(String::as_str).collect();
        let built = build_media_response_view_v1(
            UpstreamRoundV1 {
                status: upstream.status,
                headers: &headers,
                body: &body_bytes,
                content_type: upstream.content_type.as_deref(),
            },
            &secret_headers,
            prepared.response_body_form,
            &prepared.response_elision_paths,
            &MediaLimitsV1::V1,
        );
        let view = match built {
            Ok((view, _)) => {
                serde_json::from_str(&view.to_json()).map_err(|error| error.to_string())?
            }
            Err(error) => {
                // The host judges this round `unknown` without asking the component.
                return Ok(Answered {
                    input,
                    raw_body,
                    view: Value::Null,
                    outcome: Ok(ImageOutcomeV1::Unknown {
                        reason: format!("host: {error}"),
                        error: None,
                    }),
                    prepared,
                });
            }
        };
        let state: Value =
            serde_json::from_str(&prepared.state).map_err(|error| error.to_string())?;
        let outcome = self.component.parse_response(&state, &view);
        Ok(Answered { input, prepared, raw_body, view, outcome })
    }

    fn invoke(&self, case: &ImageCaseV1, input: &Value) -> Invoked {
        match case.family {
            ImageFamilyV1::Capabilities => {
                let input: CapabilitiesInput = parse(input)?;
                Ok(match self.component.model_capabilities(&input.provider_config) {
                    Ok(models) => {
                        serde_json::to_value(models).map_err(|error| error.to_string())?
                    }
                    Err(error) => refused(&error),
                })
            }
            ImageFamilyV1::Prepare => {
                let input: PrepareInput = parse(input)?;
                Ok(match self.prepare(&input) {
                    Ok(prepared) => serde_json::from_str(&prepared.to_json())
                        .map_err(|error| error.to_string())?,
                    Err(error) => refused(&error),
                })
            }
            ImageFamilyV1::Response => {
                let answered = self.answer(input)?;
                Ok(match &answered.outcome {
                    Ok(outcome) => wire::image_outcome_json(outcome)?,
                    Err(error) => refused(error),
                })
            }
            ImageFamilyV1::Render => {
                let render: RenderInput = parse(input)?;
                let (state, outcomes, _) = self.rendered_inputs(&render)?;
                Ok(match self.component.render(&state, &outcomes, &render.context) {
                    Ok(rendered) => json!({
                        "template": serde_json::from_str::<Value>(&rendered.template)
                            .map_err(|_| "the rendered template is not JSON")?,
                    }),
                    Err(error) => refused(&error),
                })
            }
        }
    }

    /// The shared state, the succeeded outcomes and their artifacts for a render case.
    fn rendered_inputs(
        &self,
        render: &RenderInput,
    ) -> Result<(Value, Vec<ImageOutcomeV1>, Vec<south_contracts::image::ImageArtifactV1>), String>
    {
        let mut state = None;
        let mut outcomes = Vec::new();
        let mut artifacts = Vec::new();
        for name in &render.response_cases {
            let case =
                self.pack.case(name).ok_or_else(|| format!("names the missing case `{name}`"))?;
            let answered = self.answer(&case.input)?;
            let this_state = answered.prepared.state.clone();
            if state.as_ref().is_some_and(|known: &String| *known != this_state) {
                return Err("render cases must share one prepare case".into());
            }
            state = Some(this_state);
            match answered.outcome {
                Ok(outcome @ ImageOutcomeV1::Succeeded { .. }) => {
                    if let ImageOutcomeV1::Succeeded { artifacts: round, .. } = &outcome {
                        artifacts.extend(round.iter().cloned());
                    }
                    outcomes.push(outcome);
                }
                _ => return Err(format!("`{name}` did not succeed")),
            }
        }
        let state = serde_json::from_str(&state.ok_or("a render case names no response case")?)
            .map_err(|error| error.to_string())?;
        Ok((state, outcomes, artifacts))
    }
}

/// Runs `south.image-component.v1` for one package.
#[must_use]
pub fn run_image_component_suite_v1(
    component: &dyn ImageComponentV1,
    pack: &ImageFixturePackV1,
    manifest: &ComponentManifestV1,
) -> ReportV1 {
    let suite = Suite { component, pack, manifest };
    let mut outcomes = coverage(pack);
    let mut declared_models: Vec<ImageModelCapabilitiesV1> = Vec::new();
    let mut tally = Tally::default();
    let mut saw_prepare_refusal = false;

    for case in pack.cases() {
        let first = suite.invoke(case, &case.input);
        outcomes.push(fixture_match(case, &first));
        outcomes.push(if first == suite.invoke(case, &case.input) {
            OutcomeV1::passed(CheckV1::Determinism, &case.name)
        } else {
            OutcomeV1::failed(
                CheckV1::Determinism,
                &case.name,
                "the same input produced different output twice",
            )
        });
        if let Ok(output) = &first {
            outcomes.push(no_reserved_keys(case, output));
        }
        match case.family {
            ImageFamilyV1::Capabilities => {
                if let Ok(models) = first.as_ref().map(|value| {
                    serde_json::from_value::<Vec<ImageModelCapabilitiesV1>>(value.clone())
                }) {
                    declared_models.extend(models.unwrap_or_default());
                }
            }
            ImageFamilyV1::Prepare => {
                outcomes.push(unknown_field_tolerance(&suite, case, &first, "/context"));
                outcomes.push(unknown_field_tolerance(&suite, case, &first, "/request"));
                outcomes.push(undeclared_values_ignored(&suite, case, &first));
                if first.as_ref().is_ok_and(|value| value.get("error").is_some()) {
                    saw_prepare_refusal = true;
                }
                outcomes.extend(prepare_checks(&suite, case, &first));
            }
            ImageFamilyV1::Response => {
                outcomes.extend(tally_response(&suite, case, &declared_models, &mut tally));
            }
            ImageFamilyV1::Render => outcomes.push(render_check(&suite, case, &first)),
        }
    }

    outcomes.extend(suite_rows(&declared_models, &tally));
    for (seen, check, detail) in [
        (tally.non_2xx, CheckV1::TerminalOnlyFromTheWire, "no response case is a non-2xx"),
        (
            tally.auth_refusal,
            CheckV1::AuthErrorsAreNotRetriable,
            "no response case is a 401 or 403",
        ),
        (saw_prepare_refusal, CheckV1::PreDispatchRefusal, "no prepare case is refused"),
    ] {
        if !seen {
            outcomes.push(OutcomeV1::failed(check, "image-v1", detail));
        }
    }
    if let Some(credentials) = &manifest.credentials {
        outcomes.extend(credential_recipe_checks_v1(credentials, pack.credentials()));
    }
    ReportV1::new(IMAGE_COMPONENT_SUITE_V1, outcomes)
}

fn coverage(pack: &ImageFixturePackV1) -> Vec<OutcomeV1> {
    ImageFamilyV1::ALL
        .iter()
        .map(|family| {
            let name = format!("image-v1.{}", family.token());
            if pack.cases().iter().any(|case| case.family == *family) {
                OutcomeV1::passed(CheckV1::Coverage, name)
            } else {
                OutcomeV1::failed(
                    CheckV1::Coverage,
                    name,
                    "the pack carries no case of this family",
                )
            }
        })
        .collect()
}

fn fixture_match(case: &ImageCaseV1, first: &Invoked) -> OutcomeV1 {
    match first {
        Ok(output) if *output == case.expected => {
            OutcomeV1::passed(CheckV1::FixtureMatch, &case.name)
        }
        Ok(output) => OutcomeV1::failed(
            CheckV1::FixtureMatch,
            &case.name,
            format!("expected {}, got {output}", case.expected),
        ),
        Err(detail) => OutcomeV1::failed(CheckV1::FixtureMatch, &case.name, detail.clone()),
    }
}

/// `ReferenceIntegrity` on any output: no `$south.blob` or `$south.ref` key, and no string above
/// the fallback threshold (a template's `$south.artifact` is judged by the render check).
fn no_reserved_keys(case: &ImageCaseV1, output: &Value) -> OutcomeV1 {
    fn walk(value: &Value, render: bool) -> Option<String> {
        match value {
            Value::String(text) if text.len() > MediaLimitsV1::V1.inline_string_bytes => {
                Some("an output string exceeds the fallback threshold".into())
            }
            Value::Array(items) => items.iter().find_map(|item| walk(item, render)),
            Value::Object(map) => map.iter().find_map(|(key, value)| {
                let allowed = render && key == "$south.artifact";
                if key.starts_with(RESERVED_PREFIX) && !allowed {
                    // A prepared call's template may carry `$south.ref`; it is judged there.
                    if key == "$south.ref" {
                        return walk(value, render);
                    }
                    return Some(format!("an output carries the reserved key `{key}`"));
                }
                walk(value, render)
            }),
            _ => None,
        }
    }
    walk(output, case.family == ImageFamilyV1::Render).map_or_else(
        || OutcomeV1::passed(CheckV1::ReferenceIntegrity, &case.name),
        |detail| OutcomeV1::failed(CheckV1::ReferenceIntegrity, &case.name, detail),
    )
}

fn unknown_field_tolerance(
    suite: &Suite<'_>,
    case: &ImageCaseV1,
    first: &Invoked,
    at: &str,
) -> OutcomeV1 {
    let mut mutated = case.input.clone();
    let Some(Value::Object(map)) = mutated.pointer_mut(at) else {
        return OutcomeV1::failed(
            CheckV1::UnknownFieldTolerance,
            &case.name,
            format!("no object at `{at}`"),
        );
    };
    map.insert(UNKNOWN_FIELD.to_owned(), json!("tolerate me"));
    if suite.invoke(case, &mutated) == *first {
        OutcomeV1::passed(CheckV1::UnknownFieldTolerance, &case.name)
    } else {
        OutcomeV1::failed(
            CheckV1::UnknownFieldTolerance,
            &case.name,
            format!("a field this ABI does not model, added at `{at}`, changed the answer"),
        )
    }
}

fn undeclared_values_ignored(suite: &Suite<'_>, case: &ImageCaseV1, first: &Invoked) -> OutcomeV1 {
    let check = CheckV1::UndeclaredValuesIgnored;
    let family = case.input.pointer("/provider_config/provider").and_then(Value::as_str);
    let declared = family.map(|family| suite.manifest.declared_keys(family)).unwrap_or_default();
    let key = (0..=declared.len())
        .map(|index| format!("{UNDECLARED_VALUE}{index}"))
        .find(|key| !declared.contains(key.as_str()))
        .unwrap_or_default();
    let mut mutated = case.input.clone();
    let Some(Value::Object(config)) = mutated.pointer_mut("/provider_config") else {
        return OutcomeV1::failed(check, &case.name, "fixture has no object at `/provider_config`");
    };
    let entry = config.entry("declared").or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Value::Object(values) = entry else {
        return OutcomeV1::failed(
            check,
            &case.name,
            "`/provider_config/declared` is not an object",
        );
    };
    values.insert(key.clone(), Value::String("undeclared".to_owned()));
    if suite.invoke(case, &mutated) == *first {
        OutcomeV1::passed(check, &case.name)
    } else {
        OutcomeV1::failed(
            check,
            &case.name,
            format!("the undeclared key `declared.{key}` changed the answer"),
        )
    }
}

/// Every blob id a request view holds: `$south.blob` placeholders and multipart blobs.
fn view_blobs(view: &Value, ids: &mut BTreeSet<String>) {
    match view {
        Value::Object(map) => {
            if let Some(id) = map.get("$south.blob").and_then(|blob| blob["id"].as_str()) {
                ids.insert(id.to_owned());
            }
            if let Some(id) = map.get("blob").and_then(Value::as_str) {
                ids.insert(id.to_owned());
            }
            for value in map.values() {
                view_blobs(value, ids);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| view_blobs(item, ids)),
        _ => {}
    }
}

/// `PreDispatchRefusal`, `ReferenceIntegrity` (descriptor references), `EndpointConfinement`
/// and `DescriptorAuthWithinManifest` on a prepare case.
fn prepare_checks(suite: &Suite<'_>, case: &ImageCaseV1, first: &Invoked) -> Vec<OutcomeV1> {
    let Ok(output) = first else { return Vec::new() };
    if let Some(error) = output.get("error") {
        let code = serde_json::from_value::<ErrorEnvelope>(error.clone()).map(|error| error.code);
        return vec![match code {
            Ok(ErrorCode::InvalidRequest | ErrorCode::Capability) => {
                OutcomeV1::passed(CheckV1::PreDispatchRefusal, &case.name)
            }
            _ => OutcomeV1::failed(
                CheckV1::PreDispatchRefusal,
                &case.name,
                "a prepare refusal must be `invalid_request` or `capability`",
            ),
        }];
    }
    let Ok(input) = parse::<PrepareInput>(&case.input) else { return Vec::new() };
    let Ok(prepared) = suite.prepare(&input) else { return Vec::new() };
    let mut outcomes = Vec::new();
    let mut ids = BTreeSet::new();
    view_blobs(&input.request, &mut ids);
    let dangling: Vec<String> = prepared
        .descriptor
        .referenced_blobs()
        .into_iter()
        .map(|id| id.as_str().to_owned())
        .filter(|id| !ids.contains(id))
        .collect();
    outcomes.push(if dangling.is_empty() {
        OutcomeV1::passed(CheckV1::ReferenceIntegrity, &case.name)
    } else {
        OutcomeV1::failed(
            CheckV1::ReferenceIntegrity,
            &case.name,
            format!("the descriptor references blobs the view does not hold: {dangling:?}"),
        )
    });
    // The descriptor parsed, so its path is relative and within the grammar.
    outcomes.push(OutcomeV1::passed(CheckV1::EndpointConfinement, &case.name));
    outcomes.push(
        match admit_media_descriptor_auth(
            suite.manifest,
            &input.provider_config,
            &prepared.descriptor,
        ) {
            Ok(_) => OutcomeV1::passed(CheckV1::DescriptorAuthWithinManifest, &case.name),
            Err(refusal) => OutcomeV1::failed(
                CheckV1::DescriptorAuthWithinManifest,
                &case.name,
                refusal.to_string(),
            ),
        },
    );
    let missing: Vec<&MeteringFormV1> = input
        .context
        .metering_required
        .iter()
        .filter(|form| !prepared.facts.metering_forms.contains(form))
        .collect();
    if !missing.is_empty() {
        outcomes.push(OutcomeV1::failed(
            CheckV1::MeteringSample,
            &case.name,
            format!("the facts do not echo the required metering forms {missing:?}"),
        ));
    }
    outcomes
}

fn auth_not_retriable(case: &ImageCaseV1, answered: &Answered) -> OutcomeV1 {
    let error = match &answered.outcome {
        Ok(ImageOutcomeV1::Rejected { error } | ImageOutcomeV1::ChargedFailure { error, .. }) => {
            Some(error)
        }
        Ok(ImageOutcomeV1::Unknown { error, .. }) => error.as_ref(),
        Err(error) => Some(error),
        Ok(ImageOutcomeV1::Succeeded { .. }) => None,
    };
    match error {
        Some(error) if !error.code.is_retriable_elsewhere() => {
            OutcomeV1::passed(CheckV1::AuthErrorsAreNotRetriable, &case.name)
        }
        _ => OutcomeV1::failed(
            CheckV1::AuthErrorsAreNotRetriable,
            &case.name,
            "a 401 or 403 mapped to a retriable error, or to none",
        ),
    }
}

/// The host's structural checks on a response case (§9.4 items 1 and 2, §12.1).
fn response_checks(
    case: &ImageCaseV1,
    answered: &Answered,
    declared_models: &[ImageModelCapabilitiesV1],
) -> Vec<OutcomeV1> {
    let mut outcomes = Vec::new();
    match (&case.meta, &answered.outcome) {
        (Some(ImageCaseMetaV1::Missing(fact)), outcome) => outcomes.push(match outcome {
            Ok(ImageOutcomeV1::Unknown { .. }) => {
                OutcomeV1::passed(CheckV1::MissingMeterIsNotZero, &case.name)
            }
            _ => OutcomeV1::failed(
                CheckV1::MissingMeterIsNotZero,
                &case.name,
                format!("a 2xx missing the required fact `{fact}` did not end `unknown`"),
            ),
        }),
        (Some(ImageCaseMetaV1::Absent(fact)), outcome) => outcomes.push(match outcome {
            Ok(ImageOutcomeV1::Succeeded { metering, .. }) if evidence_is_null(metering, fact) => {
                OutcomeV1::passed(CheckV1::EvidenceAbsentIsNull, &case.name)
            }
            _ => OutcomeV1::failed(
                CheckV1::EvidenceAbsentIsNull,
                &case.name,
                format!("a 2xx missing the evidence fact `{fact}` did not succeed with it `null`"),
            ),
        }),
        (None, _) => {}
    }
    if let Ok(ImageOutcomeV1::Succeeded { artifacts, metering, .. }) = &answered.outcome {
        if artifacts.is_empty() {
            outcomes.push(OutcomeV1::failed(
                CheckV1::ReferenceIntegrity,
                &case.name,
                "`succeeded` with no artifact",
            ));
        }
        for artifact in artifacts {
            let landed = match artifact {
                south_contracts::image::ImageArtifactV1::Inline { pointer, .. } => answered
                    .view
                    .pointer(&format!("/body/json{}", pointer.as_str()))
                    .is_some_and(|value| value.is_string() || value.get("$south.blob").is_some()),
                south_contracts::image::ImageArtifactV1::Url { pointer, .. } => {
                    answered.raw_body.pointer(pointer.as_str()).is_some_and(Value::is_string)
                }
                south_contracts::image::ImageArtifactV1::Body { .. } => {
                    answered.view.pointer("/body/opaque").is_some()
                }
            };
            if !landed {
                outcomes.push(OutcomeV1::failed(
                    CheckV1::ReferenceIntegrity,
                    &case.name,
                    "an artifact points at no string of the response",
                ));
            }
        }
        // §9.4 item 1: every required fact of every required form.
        let model = declared_models
            .iter()
            .find(|model| model.model == answered.input.context.upstream_model);
        for form in &answered.input.context.metering_required {
            let present = match form {
                MeteringFormV1::Tokens => model.is_some_and(|model| {
                    metering.tokens.is_some_and(|tokens| {
                        model.token_buckets.iter().all(|bucket| tokens.get(*bucket).is_some())
                    })
                }),
                MeteringFormV1::Credits => metering.credits.is_some(),
                MeteringFormV1::Images | MeteringFormV1::Requests => true,
            };
            if !present {
                outcomes.push(OutcomeV1::failed(
                    CheckV1::MissingMeterIsNotZero,
                    &case.name,
                    format!("`succeeded` without every required fact of the `{form:?}` form"),
                ));
            }
        }
    }
    outcomes
}

fn evidence_is_null(metering: &south_contracts::image::ImageMeteringV1, fact: &str) -> bool {
    match fact {
        "images_reported" => metering.images_reported.is_none(),
        "upstream_cost" => metering.upstream_cost.is_none(),
        _ => false,
    }
}

fn render_check(suite: &Suite<'_>, case: &ImageCaseV1, first: &Invoked) -> OutcomeV1 {
    let Ok(output) = first else {
        return OutcomeV1::failed(
            CheckV1::ReferenceIntegrity,
            &case.name,
            "the render case did not run",
        );
    };
    let Some(template) = output.get("template") else {
        return OutcomeV1::passed(CheckV1::ReferenceIntegrity, &case.name);
    };
    let artifacts = parse::<RenderInput>(&case.input)
        .and_then(|render| suite.rendered_inputs(&render))
        .map(|(_, _, artifacts)| artifacts);
    match artifacts {
        Ok(artifacts) => {
            match check_render_template_v1(&template.to_string(), &artifacts, &MediaLimitsV1::V1) {
                Ok(_) => OutcomeV1::passed(CheckV1::ReferenceIntegrity, &case.name),
                Err(error) => {
                    OutcomeV1::failed(CheckV1::ReferenceIntegrity, &case.name, error.to_string())
                }
            }
        }
        Err(detail) => OutcomeV1::failed(CheckV1::ReferenceIntegrity, &case.name, detail),
    }
}

/// What the response cases showed across the pack.
#[derive(Default)]
struct Tally {
    sampled_forms: BTreeSet<MeteringFormV1>,
    missing_facts: BTreeSet<String>,
    absent_facts: BTreeSet<String>,
    reported_evidence: BTreeSet<&'static str>,
    non_2xx: bool,
    auth_refusal: bool,
}

/// One response case's per-case checks, recording what the pack-wide rows need.
fn tally_response(
    suite: &Suite<'_>,
    case: &ImageCaseV1,
    declared_models: &[ImageModelCapabilitiesV1],
    tally: &mut Tally,
) -> Vec<OutcomeV1> {
    let Ok(answered) = suite.answer(&case.input) else {
        return Vec::new();
    };
    let mut outcomes = Vec::new();
    let status = case.input.pointer("/response/status").and_then(Value::as_u64).unwrap_or(0);
    if !(200..300).contains(&status) {
        tally.non_2xx = true;
        outcomes.push(match &answered.outcome {
            Ok(ImageOutcomeV1::Succeeded { .. }) => OutcomeV1::failed(
                CheckV1::TerminalOnlyFromTheWire,
                &case.name,
                "a non-2xx produced `succeeded`",
            ),
            _ => OutcomeV1::passed(CheckV1::TerminalOnlyFromTheWire, &case.name),
        });
        if matches!(status, 401 | 403) {
            tally.auth_refusal = true;
            outcomes.push(auth_not_retriable(case, &answered));
        }
    }
    if let Ok(ImageOutcomeV1::Succeeded { metering, .. }) = &answered.outcome {
        tally.sampled_forms.extend(answered.input.context.metering_required.iter().copied());
        if metering.images_reported.is_some() {
            tally.reported_evidence.insert("images_reported");
        }
        if metering.upstream_cost.is_some() {
            tally.reported_evidence.insert("upstream_cost");
        }
    }
    outcomes.extend(response_checks(case, &answered, declared_models));
    match &case.meta {
        Some(ImageCaseMetaV1::Missing(fact)) => {
            tally.missing_facts.insert(fact.clone());
        }
        Some(ImageCaseMetaV1::Absent(fact)) => {
            tally.absent_facts.insert(fact.clone());
        }
        None => {}
    }
    outcomes
}

/// The rows the record requires across the pack (§12.1).
fn suite_rows(declared_models: &[ImageModelCapabilitiesV1], tally: &Tally) -> Vec<OutcomeV1> {
    let Tally { sampled_forms, missing_facts, absent_facts, reported_evidence, .. } = tally;
    let mut outcomes = Vec::new();
    let forms: BTreeSet<MeteringFormV1> =
        declared_models.iter().flat_map(|model| model.metering_forms.iter().copied()).collect();
    for form in &forms {
        let row = format!("image-v1.metering.{form:?}").to_lowercase();
        outcomes.push(if sampled_forms.contains(form) {
            OutcomeV1::passed(CheckV1::MeteringSample, row)
        } else {
            OutcomeV1::failed(
                CheckV1::MeteringSample,
                row,
                "no response case succeeds with this form required",
            )
        });
    }
    let mut required: BTreeSet<String> = declared_models
        .iter()
        .filter(|model| model.metering_forms.contains(&MeteringFormV1::Tokens))
        .flat_map(|model| model.token_buckets.iter())
        .map(|bucket| {
            serde_json::to_value(bucket)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default()
        })
        .collect();
    if forms.contains(&MeteringFormV1::Credits) {
        required.insert("credits".to_owned());
    }
    for fact in required {
        let row = format!("image-v1.missing.{fact}");
        outcomes.push(if missing_facts.contains(&fact) {
            OutcomeV1::passed(CheckV1::MissingMeterIsNotZero, row)
        } else {
            OutcomeV1::failed(
                CheckV1::MissingMeterIsNotZero,
                row,
                "no 2xx case is missing this required fact",
            )
        });
    }
    for fact in reported_evidence {
        let row = format!("image-v1.absent.{fact}");
        outcomes.push(if absent_facts.contains(*fact) {
            OutcomeV1::passed(CheckV1::EvidenceAbsentIsNull, row)
        } else {
            OutcomeV1::failed(
                CheckV1::EvidenceAbsentIsNull,
                row,
                "no 2xx case is missing this evidence fact",
            )
        });
    }
    outcomes
}
