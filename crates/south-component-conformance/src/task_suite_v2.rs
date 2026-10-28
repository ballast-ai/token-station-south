//! Public behavior gate for the candidate task-v2 world.
use crate::task_v2_json as wire;
use crate::{
    CheckV1, OutcomeV1, ReportV1, TaskComponentV2, TaskFamilyV2, TaskFixturePackV2,
    credential_recipe_checks_v1,
};
use serde::Deserialize;
use serde_json::{Value, json};
use south_contracts::{
    HostMintedValuesV1, TaskArtifactRefV2, TaskInlineArtifactV2, TaskObservationV2,
    TaskUsageFactsV2, fill_task_artifact_markers_v1,
};
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{ErrorEnvelope, HttpResponseParts, ProviderConfig};

/// Independently versioned suite; v1 fixtures remain frozen.
pub const TASK_COMPONENT_SUITE_V2: &str = "south.task-component.v2";

type Invoked = Result<Value, String>;
fn parse<T: for<'de> Deserialize<'de>>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|_| "fixture input has the wrong shape".into())
}
fn encode<T: serde::Serialize>(value: &T) -> Invoked {
    serde_json::to_value(value).map_err(|_| "component output could not be encoded".into())
}
fn component_result<T>(
    result: Result<T, ErrorEnvelope>,
    render: impl FnOnce(T) -> Invoked,
) -> Invoked {
    match result {
        Ok(value) => render(value),
        Err(error) => Ok(json!({"error":error})),
    }
}
#[derive(Deserialize)]
struct PrepareInput {
    provider_config: ProviderConfig,
    request: Value,
    minted: MintedInput,
}
#[derive(Deserialize)]
struct MintedInput {
    task_id: String,
    #[serde(default)]
    callback_url: Option<String>,
}
#[derive(Deserialize)]
struct ObserveInput {
    provider_config: ProviderConfig,
    upstream_model: String,
    upstream_task_id: String,
    locator: Value,
}
#[derive(Deserialize)]
struct RenderInput {
    observation: Value,
    context: Value,
    #[serde(default)]
    fetched: Option<HttpResponseParts>,
}
#[derive(Deserialize)]
struct ArtifactInput {
    provider_config: ProviderConfig,
    locator: Value,
    observation: Value,
}
fn invoke(component: &dyn TaskComponentV2, family: TaskFamilyV2, input: &Value) -> Invoked {
    match family {
        TaskFamilyV2::Prepare => {
            let arg: PrepareInput = parse(input)?;
            let minted =
                HostMintedValuesV1::new(&arg.minted.task_id, arg.minted.callback_url.as_deref())
                    .map_err(|_| "fixture minted values are invalid".to_owned())?;
            component_result(
                component.build_submit_request(&arg.provider_config, &arg.request, &minted),
                |value| wire::prepared_task_json(&value),
            )
        }
        TaskFamilyV2::Created => {
            component_result(component.parse_submit_response(&parse(input)?), |value| {
                wire::submit_outcome_json(&value)
            })
        }
        TaskFamilyV2::Observe => {
            let arg: ObserveInput = parse(input)?;
            let locator = wire::parse_locator_json(&arg.locator.to_string())?;
            component_result(
                component.build_observe_request(
                    &arg.provider_config,
                    &arg.upstream_model,
                    &arg.upstream_task_id,
                    &locator,
                ),
                |value| encode(&value),
            )
        }
        TaskFamilyV2::Observation => {
            component_result(component.parse_observation(&parse(input)?), |value| {
                wire::observation_json(&value)
            })
        }
        TaskFamilyV2::Render => {
            let arg: RenderInput = parse(input)?;
            let observation = wire::parse_observation_json(&arg.observation.to_string())?;
            let context = wire::parse_render_context_json(&arg.context.to_string())?;
            component_result(
                component.render_success(&observation, arg.fetched.as_ref(), &context),
                Ok,
            )
        }
        TaskFamilyV2::Artifact => {
            let arg: ArtifactInput = parse(input)?;
            let locator = wire::parse_locator_json(&arg.locator.to_string())?;
            let observation = wire::parse_observation_json(&arg.observation.to_string())?;
            component_result(
                component.build_artifact_request(&arg.provider_config, &locator, &observation),
                |value| encode(&value),
            )
        }
        TaskFamilyV2::Failure => {
            let observation = wire::parse_observation_json(&input.to_string())?;
            component_result(component.map_terminal_failure(&observation), |value| encode(&value))
        }
    }
}

/// Runs frozen behavior, determinism, input extension and failed-query checks.
///
/// Error fixtures compare the complete typed error envelope. Invalid fixture
/// inputs remain failed checks and cannot masquerade as expected rejections.
#[must_use]
pub fn run_task_component_suite_v2(
    component: &dyn TaskComponentV2,
    pack: &TaskFixturePackV2,
) -> ReportV1 {
    let missing = pack.missing_families();
    let mut outcomes = vec![if missing.is_empty() {
        OutcomeV1::passed(CheckV1::Coverage, TASK_COMPONENT_SUITE_V2)
    } else {
        OutcomeV1::failed(
            CheckV1::Coverage,
            TASK_COMPONENT_SUITE_V2,
            "missing task-v2 fixture families",
        )
    }];
    let mut failed_query = false;
    for case in pack.cases() {
        let first = invoke(component, case.family, &case.input);
        outcomes.push(match &first {
            Ok(actual) if actual == &case.expected => {
                OutcomeV1::passed(CheckV1::FixtureMatch, &case.name)
            }
            Ok(_) => OutcomeV1::failed(
                CheckV1::FixtureMatch,
                &case.name,
                "component output differs from frozen expected output",
            ),
            Err(_) => OutcomeV1::failed(
                CheckV1::FixtureMatch,
                &case.name,
                "fixture input or component output failed validation",
            ),
        });
        outcomes.push(if first == invoke(component, case.family, &case.input) {
            OutcomeV1::passed(CheckV1::Determinism, &case.name)
        } else {
            OutcomeV1::failed(
                CheckV1::Determinism,
                &case.name,
                "identical input produced different output",
            )
        });
        // Mutate the operation input wrapper, not bounded locator/context
        // contracts, whose parsers intentionally reject unknown fields.
        if matches!(
            case.family,
            TaskFamilyV2::Prepare
                | TaskFamilyV2::Observe
                | TaskFamilyV2::Render
                | TaskFamilyV2::Artifact
        ) {
            let mut extended = case.input.clone();
            if let Some(object) = extended.as_object_mut() {
                object.insert("__conformance_unknown_field".into(), json!(true));
            }
            outcomes.push(if first == invoke(component, case.family, &extended) {
                OutcomeV1::passed(CheckV1::UnknownFieldTolerance, &case.name)
            } else {
                OutcomeV1::failed(
                    CheckV1::UnknownFieldTolerance,
                    &case.name,
                    "unknown wrapper field changed behavior",
                )
            });
        }
        if case.family == TaskFamilyV2::Observation
            && let Ok(parts) = parse::<HttpResponseParts>(&case.input)
            && !(200..300).contains(&parts.status)
        {
            failed_query = true;
            outcomes.push(
                if matches!(
                    component.parse_observation(&parts),
                    Ok(TaskObservationV2::Unknown { .. })
                ) {
                    OutcomeV1::passed(CheckV1::TerminalOnlyFromTheWire, &case.name)
                } else {
                    OutcomeV1::failed(
                        CheckV1::TerminalOnlyFromTheWire,
                        &case.name,
                        "failed query did not produce Unknown",
                    )
                },
            );
        }
    }
    if !failed_query {
        outcomes.push(OutcomeV1::failed(
            CheckV1::TerminalOnlyFromTheWire,
            TASK_COMPONENT_SUITE_V2,
            "no fixture offers a failed query",
        ));
    }
    ReportV1::new(TASK_COMPONENT_SUITE_V2, outcomes)
}

/// Runs the suite for a task component with its manifest.
///
/// Everything [`run_task_component_suite_v2`] runs, and, when the manifest
/// declares `credentials`, the credential-recipe checks over the pack's
/// `credential.*` cases (B4, host-zero-vendor-boundary §3.7). This is the
/// entry point an admitting host and a release's gate ② report use.
#[must_use]
pub fn run_task_component_suite_v2_for_manifest(
    component: &dyn TaskComponentV2,
    pack: &TaskFixturePackV2,
    manifest: &ComponentManifestV1,
) -> ReportV1 {
    let mut outcomes = run_task_component_suite_v2(component, pack).outcomes().to_vec();
    if let Some(credentials) = &manifest.credentials {
        outcomes.extend(credential_recipe_checks_v1(credentials, pack.credentials()));
    }
    if !manifest.response_blob_paths.is_empty() || !manifest.token_buckets.is_empty() {
        outcomes.extend(contract_8_checks(component, pack, manifest));
    }
    outcomes.extend(failure_category_declared(component, pack, manifest));
    ReportV1::new(TASK_COMPONENT_SUITE_V2, outcomes)
}

/// The first task contract whose failed observation may carry a `category`.
const TASK_FAILURE_CATEGORY_CONTRACT: u32 = 9;

/// `FailureCategoryDeclared` (the served models record §4–§5): every fixture failure that carries
/// a category needs task contract 9, and a package declaring 9 shows one.
fn failure_category_declared(
    component: &dyn TaskComponentV2,
    pack: &TaskFixturePackV2,
    manifest: &ComponentManifestV1,
) -> Vec<OutcomeV1> {
    let declares = manifest
        .compatibility
        .contracts
        .get("task")
        .is_some_and(|contract| *contract >= TASK_FAILURE_CATEGORY_CONTRACT);
    let mut outcomes = Vec::new();
    for case in pack.cases() {
        let Ok(parts) = parse::<HttpResponseParts>(&case.input) else { continue };
        let observation = match case.family {
            TaskFamilyV2::Observation => component.parse_observation(&parts).ok(),
            TaskFamilyV2::Created => match component.parse_submit_response(&parts) {
                Ok(crate::SubmitOutcomeV2::AcceptedTerminal(observation)) => Some(observation),
                _ => None,
            },
            _ => None,
        };
        let Some(TaskObservationV2::Failed { category: Some(category), .. }) = observation else {
            continue;
        };
        outcomes.push(if declares {
            OutcomeV1::passed(CheckV1::FailureCategoryDeclared, &case.name)
        } else {
            OutcomeV1::failed(
                CheckV1::FailureCategoryDeclared,
                &case.name,
                format!(
                    "reports category `{}` but declares task contract {:?}, not \
                     {TASK_FAILURE_CATEGORY_CONTRACT}",
                    category.word(),
                    manifest.compatibility.contracts.get("task")
                ),
            )
        });
    }
    if declares && outcomes.is_empty() {
        outcomes.push(OutcomeV1::failed(
            CheckV1::FailureCategoryDeclared,
            TASK_COMPONENT_SUITE_V2,
            "declares task contract 9 but no fixture failure carries a category",
        ));
    }
    outcomes
}

/// The checks that bind every task contract 8 package (docs/design/2026-10-01-omni-task-component.md
/// §10): `DeclaredBucketsOnly` on every success, `ReferenceIntegrity` on every inline artifact and
/// rendered body, and `UsageNeverDefaulted` / `MissingMeterIsNotZero` on every success observation
/// that points at its usage with a sidecar.
fn contract_8_checks(
    component: &dyn TaskComponentV2,
    pack: &TaskFixturePackV2,
    manifest: &ComponentManifestV1,
) -> Vec<OutcomeV1> {
    let mut outcomes = Vec::new();
    let mut pointed = false;
    for case in pack.cases() {
        match case.family {
            TaskFamilyV2::Observation | TaskFamilyV2::Created => {
                let Ok(parts) = parse::<HttpResponseParts>(&case.input) else { continue };
                let observation = if case.family == TaskFamilyV2::Observation {
                    component.parse_observation(&parts).ok()
                } else {
                    match component.parse_submit_response(&parts) {
                        Ok(crate::SubmitOutcomeV2::AcceptedTerminal(observation)) => {
                            Some(observation)
                        }
                        _ => None,
                    }
                };
                let Some(TaskObservationV2::Succeeded { artifacts, usage }) = observation else {
                    continue;
                };
                outcomes.push(declared_buckets_only(&case.name, &usage, manifest));
                if let TaskArtifactRefV2::Inline(items) = &artifacts {
                    outcomes.push(inline_lands_on_blobs(&case.name, &parts.body, items));
                }
                if let Some(meta) = pack.usage_meta(&case.name) {
                    pointed = true;
                    outcomes.extend(usage_deletions(component, &case.name, &parts, meta, manifest));
                }
            }
            TaskFamilyV2::Render => outcomes.extend(render_references(component, case)),
            _ => {}
        }
    }
    if !pointed && !manifest.required_token_buckets.is_empty() {
        outcomes.push(OutcomeV1::failed(
            CheckV1::UsageNeverDefaulted,
            TASK_COMPONENT_SUITE_V2,
            "no success observation fixture carries a usage sidecar, so nothing is deleted",
        ));
    }
    outcomes
}

fn declared_buckets_only(
    case: &str,
    usage: &TaskUsageFactsV2,
    manifest: &ComponentManifestV1,
) -> OutcomeV1 {
    let reported: Vec<&str> = usage
        .token_buckets()
        .map(|buckets| buckets.iter().map(|(bucket, _)| bucket.word()).collect())
        .unwrap_or_default();
    if let Some(stray) =
        reported.iter().find(|word| !manifest.token_buckets.iter().any(|d| d == *word))
    {
        return OutcomeV1::failed(
            CheckV1::DeclaredBucketsOnly,
            case,
            format!("reports `{stray}`, which token_buckets does not declare"),
        );
    }
    if let Some(missing) =
        manifest.required_token_buckets.iter().find(|word| !reported.contains(&word.as_str()))
    {
        return OutcomeV1::failed(
            CheckV1::DeclaredBucketsOnly,
            case,
            format!("a success without the required bucket `{missing}`"),
        );
    }
    OutcomeV1::passed(CheckV1::DeclaredBucketsOnly, case)
}

/// Every inline pointer lands on a `$south.blob` placeholder of the response the component read:
/// a pointer names which upstream string is the artifact, never bytes of its own.
fn inline_lands_on_blobs(case: &str, body: &str, items: &[TaskInlineArtifactV2]) -> OutcomeV1 {
    let view: Option<Value> = serde_json::from_str(body).ok();
    let lands = |item: &TaskInlineArtifactV2| {
        view.as_ref()
            .and_then(|view| view.pointer(item.pointer()))
            .and_then(Value::as_object)
            .is_some_and(|node| {
                node.len() == 1 && node.contains_key(south_contracts::media::BLOB_KEY)
            })
    };
    if items.iter().all(lands) {
        OutcomeV1::passed(CheckV1::ReferenceIntegrity, case)
    } else {
        OutcomeV1::failed(
            CheckV1::ReferenceIntegrity,
            case,
            "an inline pointer does not land on a $south.blob of the response view",
        )
    }
}

/// A rendered body names only primary artifacts of its observation, through well-formed
/// `$south.artifact` markers, and carries no other `$south.` key.
fn render_references(
    component: &dyn TaskComponentV2,
    case: &crate::TaskCaseV2,
) -> Option<OutcomeV1> {
    let arg: RenderInput = parse(&case.input).ok()?;
    let observation = wire::parse_observation_json(&arg.observation.to_string()).ok()?;
    let context = wire::parse_render_context_json(&arg.context.to_string()).ok()?;
    let mut body = component.render_success(&observation, arg.fetched.as_ref(), &context).ok()?;
    let TaskObservationV2::Succeeded { artifacts, .. } = &observation else { return None };
    Some(
        match fill_task_artifact_markers_v1(&mut body, artifacts, |index| format!("art_{index}")) {
            Ok(_) => OutcomeV1::passed(CheckV1::ReferenceIntegrity, &case.name),
            Err(_) => OutcomeV1::failed(
                CheckV1::ReferenceIntegrity,
                &case.name,
                "the rendered body names a missing artifact or leaves a $south. key",
            ),
        },
    )
}

/// Removes the member an RFC 6901 pointer names; `false` when it names nothing.
fn remove_at(document: &mut Value, pointer: &str) -> bool {
    let Some((parent, last)) = pointer.rsplit_once('/') else { return false };
    let last = last.replace("~1", "/").replace("~0", "~");
    match document.pointer_mut(parent) {
        Some(Value::Object(members)) => members.remove(&last).is_some(),
        Some(Value::Array(items)) => match last.parse::<usize>() {
            Ok(index) if index < items.len() => {
                items.remove(index);
                true
            }
            _ => false,
        },
        _ => false,
    }
}

/// Deleting the usage object, or the wire member of any required bucket, makes the component
/// answer `unknown` rather than bill a zero the upstream never said.
fn usage_deletions(
    component: &dyn TaskComponentV2,
    case: &str,
    parts: &HttpResponseParts,
    meta: &crate::TaskUsageMetaV2,
    manifest: &ComponentManifestV1,
) -> Vec<OutcomeV1> {
    let deleted = |pointer: &str| -> Option<bool> {
        let mut body: Value = serde_json::from_str(&parts.body).ok()?;
        if !remove_at(&mut body, pointer) {
            return None;
        }
        let mut stripped = parts.clone();
        stripped.body = body.to_string();
        Some(matches!(
            component.parse_observation(&stripped),
            Ok(TaskObservationV2::Unknown { .. })
        ))
    };
    let mut outcomes = vec![match deleted(&meta.usage_pointer) {
        Some(true) => OutcomeV1::passed(CheckV1::UsageNeverDefaulted, case),
        Some(false) => OutcomeV1::failed(
            CheckV1::UsageNeverDefaulted,
            case,
            "deleting the usage object did not make the observation unknown",
        ),
        None => OutcomeV1::failed(
            CheckV1::UsageNeverDefaulted,
            case,
            "usage_pointer names nothing in the body",
        ),
    }];
    for bucket in &manifest.required_token_buckets {
        let name = format!("{case}#{bucket}");
        outcomes.push(match meta.bucket_pointers.get(bucket).map(|pointer| deleted(pointer)) {
            Some(Some(true)) => OutcomeV1::passed(CheckV1::MissingMeterIsNotZero, &name),
            Some(Some(false)) => OutcomeV1::failed(
                CheckV1::MissingMeterIsNotZero,
                &name,
                "deleting a required bucket's wire member did not make the observation unknown",
            ),
            Some(None) => OutcomeV1::failed(
                CheckV1::MissingMeterIsNotZero,
                &name,
                "the bucket pointer names nothing in the body",
            ),
            None => OutcomeV1::failed(
                CheckV1::MissingMeterIsNotZero,
                &name,
                "the usage sidecar names no wire member for this required bucket",
            ),
        });
    }
    outcomes
}
