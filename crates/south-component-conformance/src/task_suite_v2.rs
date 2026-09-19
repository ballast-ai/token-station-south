//! Public behavior gate for the candidate task-v2 world.
use crate::task_v2_json as wire;
use crate::{CheckV1, OutcomeV1, ReportV1, TaskComponentV2, TaskFamilyV2, TaskFixturePackV2};
use serde::Deserialize;
use serde_json::{Value, json};
use south_contracts::{HostMintedValuesV1, TaskObservationV2};
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
