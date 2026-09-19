//! Gate ② for the task world — `south.task-component.v1`.
//!
//! Same shape as the provider suite: turn a case into a closure
//! `input -> output`, then ask the same questions of it. Only the questions
//! that need a typed view of the answer reach past that closure, and for this
//! world there is exactly one — the ruling that a failed *query* is never a
//! terminal.
//!
//! What this suite deliberately does **not** check is anything about timing,
//! polling cadence or retry. Those are host policy; a component has no clock
//! and no memory across calls, so there is nothing here a fixture could
//! express (2026-08-27 vocabulary record, D3 rule 5).

use serde::Deserialize;
use serde_json::Value;
use south_contracts::HostMintedValuesV1;
use token_station_protocol::{HttpResponseParts, ProviderConfig};

use crate::component::TaskComponentV1;
use crate::report::{CheckV1, OutcomeV1, ReportV1};
use crate::task_fixture::{TaskCaseV1, TaskFamilyV1, TaskFixturePackV1};
use crate::task_json::{ObservationInput, observation_json, submit_outcome_json};

/// The suite identifier, equal to the manifest's frozen
/// `conformance.required_suite`.
pub const TASK_COMPONENT_SUITE_V1: &str = south_provider_api::TASK_BEHAVIOR_SUITE;

/// The key injected to prove a component tolerates a newer peer's field.
const UNKNOWN_FIELD: &str = "__conformance_unknown_field";

type Invoked = Result<Value, Failure>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Failure {
    /// The component answered with an error.
    Component(String),
    /// The fixture did not deserialize into what the family feeds the
    /// component. Not the component's fault, and reported as its own reason.
    Fixture(String),
}

impl Failure {
    fn detail(&self) -> String {
        match self {
            Self::Component(detail) => format!("component returned an error: {detail}"),
            Self::Fixture(detail) => format!("fixture is not valid input: {detail}"),
        }
    }
}

fn parse<T: for<'de> Deserialize<'de>>(input: &Value) -> Result<T, Failure> {
    serde_json::from_value(input.clone()).map_err(|source| Failure::Fixture(source.to_string()))
}

fn encode<T: serde::Serialize>(value: &T) -> Invoked {
    serde_json::to_value(value).map_err(|source| Failure::Fixture(source.to_string()))
}

#[derive(Debug, Deserialize)]
struct SubmitInput {
    provider_config: ProviderConfig,
    request: Value,
    minted: MintedInput,
}

#[derive(Debug, Deserialize)]
struct ObserveInput {
    provider_config: ProviderConfig,
    upstream_model: String,
    upstream_task_id: String,
}

#[derive(Debug, Deserialize)]
struct RenderInput {
    observation: ObservationInput,
    minted: MintedInput,
    #[serde(default)]
    fetched: Option<HttpResponseParts>,
}

/// The host-minted values, as a fixture spells them. A fixture cannot
/// construct the validated type directly, so it names the parts.
#[derive(Debug, Deserialize)]
struct MintedInput {
    task_id: String,
    #[serde(default)]
    callback_url: Option<String>,
}

impl MintedInput {
    fn build(&self) -> Result<HostMintedValuesV1, Failure> {
        HostMintedValuesV1::new(&self.task_id, self.callback_url.as_deref())
            .map_err(|source| Failure::Fixture(source.to_string()))
    }
}

/// Runs `south.task-component.v1` against a task component.
///
/// Never panics on a component's behalf: a component that errors, or a
/// fixture that will not parse, becomes a failed check in the report, because
/// a host running this at admission time must not be taken down by the
/// package it is vetting.
#[must_use]
pub fn run_task_component_suite_v1(
    component: &dyn TaskComponentV1,
    pack: &TaskFixturePackV1,
) -> ReportV1 {
    let mut outcomes = coverage(pack);
    let mut a_failed_query_was_offered = false;

    for case in pack.cases() {
        let invoke = |input: &Value| invoke_component(component, case.family, input);
        let first = invoke(&case.input);

        outcomes.push(fixture_match(case, &first));
        outcomes.push(determinism(case, &first, &invoke(&case.input)));
        outcomes.push(unknown_field_tolerance(case, &invoke));

        if case.family == TaskFamilyV1::Observation
            && let Some(outcome) = a_failed_query_is_never_terminal(component, case)
        {
            a_failed_query_was_offered = true;
            outcomes.push(outcome);
        }
    }

    // A gate that never runs describes nothing. Without a fixture carrying a
    // non-2xx query, a component passes this ruling by never being asked — so
    // the missing fixture is itself the failure. The vocabulary record
    // requires one 404 row per family for exactly this reason.
    if !a_failed_query_was_offered {
        outcomes.push(OutcomeV1::failed(
            CheckV1::TerminalOnlyFromTheWire,
            "task.observation",
            "no fixture offers a non-2xx query, so the ruling that a failed query is not a \
             failed task never ran; every family owes one such row",
        ));
    }

    ReportV1::new(TASK_COMPONENT_SUITE_V1, outcomes)
}

fn invoke_component(
    component: &dyn TaskComponentV1,
    family: TaskFamilyV1,
    input: &Value,
) -> Invoked {
    let component_error = |error: &token_station_protocol::ErrorEnvelope| {
        Failure::Component(format!("{:?}: {}", error.code, error.message))
    };
    match family {
        TaskFamilyV1::Submit => {
            let SubmitInput { provider_config, request, minted } = parse(input)?;
            let minted = minted.build()?;
            encode(
                &component
                    .build_submit_request(&provider_config, &request, &minted)
                    .map_err(|e| component_error(&e))?,
            )
        }
        TaskFamilyV1::Created => {
            let parts: HttpResponseParts = parse(input)?;
            Ok(submit_outcome_json(
                &component.parse_submit_response(&parts).map_err(|e| component_error(&e))?,
            ))
        }
        TaskFamilyV1::Observe => {
            let ObserveInput { provider_config, upstream_model, upstream_task_id } = parse(input)?;
            encode(
                &component
                    .build_observe_request(&provider_config, &upstream_model, &upstream_task_id)
                    .map_err(|e| component_error(&e))?,
            )
        }
        TaskFamilyV1::Observation => {
            let parts: HttpResponseParts = parse(input)?;
            Ok(observation_json(
                &component.parse_observation(&parts).map_err(|e| component_error(&e))?,
            ))
        }
        TaskFamilyV1::Render => {
            let RenderInput { observation, minted, fetched } = parse(input)?;
            let observation = observation.build().map_err(Failure::Fixture)?;
            let minted = minted.build()?;
            encode(
                &component
                    .render_success(&observation, fetched.as_ref(), &minted)
                    .map_err(|e| component_error(&e))?,
            )
        }
        TaskFamilyV1::Failure => {
            let observation: ObservationInput = parse(input)?;
            let observation = observation.build().map_err(Failure::Fixture)?;
            encode(&component.map_terminal_failure(&observation).map_err(|e| component_error(&e))?)
        }
    }
}

fn coverage(pack: &TaskFixturePackV1) -> Vec<OutcomeV1> {
    let missing = pack.missing_families();
    if missing.is_empty() {
        return vec![OutcomeV1::passed(CheckV1::Coverage, TASK_COMPONENT_SUITE_V1)];
    }
    let names: Vec<&str> = missing.iter().map(|family| family.token()).collect();
    vec![OutcomeV1::failed(
        CheckV1::Coverage,
        TASK_COMPONENT_SUITE_V1,
        format!("no case for: {}", names.join(", ")),
    )]
}

fn fixture_match(case: &TaskCaseV1, actual: &Invoked) -> OutcomeV1 {
    match actual {
        Err(failure) => OutcomeV1::failed(CheckV1::FixtureMatch, &case.name, failure.detail()),
        Ok(actual) if *actual == case.expected => {
            OutcomeV1::passed(CheckV1::FixtureMatch, &case.name)
        }
        Ok(actual) => OutcomeV1::failed(
            CheckV1::FixtureMatch,
            &case.name,
            format!("expected {}, produced {}", truncate(&case.expected), truncate(actual)),
        ),
    }
}

fn determinism(case: &TaskCaseV1, first: &Invoked, second: &Invoked) -> OutcomeV1 {
    if first == second {
        OutcomeV1::passed(CheckV1::Determinism, &case.name)
    } else {
        OutcomeV1::failed(
            CheckV1::Determinism,
            &case.name,
            "the same input produced different output on a second invocation",
        )
    }
}

fn unknown_field_tolerance(case: &TaskCaseV1, invoke: &dyn Fn(&Value) -> Invoked) -> OutcomeV1 {
    let mut mutated = case.input.clone();
    if let Some(object) = mutated.as_object_mut() {
        object.insert(UNKNOWN_FIELD.to_owned(), Value::Bool(true));
    } else {
        // A non-object input cannot carry an unknown field; the check is
        // vacuous rather than failed.
        return OutcomeV1::passed(CheckV1::UnknownFieldTolerance, &case.name);
    }
    match invoke(&mutated) {
        Ok(_) => OutcomeV1::passed(CheckV1::UnknownFieldTolerance, &case.name),
        Err(failure) => OutcomeV1::failed(
            CheckV1::UnknownFieldTolerance,
            &case.name,
            format!("an input carrying an unmodelled field was refused: {}", failure.detail()),
        ),
    }
}

/// The one check that needs a typed view: a non-2xx query must never yield a
/// terminal.
///
/// A failed *query* is not a failed *task* — 429, 5xx, 404 and 401 all mean
/// the observation did not happen. A component that reads a 404 as "gone"
/// settles a task that may still be running, and the host releases a
/// reservation against live work.
fn a_failed_query_is_never_terminal(
    component: &dyn TaskComponentV1,
    case: &TaskCaseV1,
) -> Option<OutcomeV1> {
    let parts: HttpResponseParts = serde_json::from_value(case.input.clone()).ok()?;
    if (200..300).contains(&parts.status) {
        return None;
    }
    let verdict = match component.parse_observation(&parts) {
        Ok(observation) if observation.is_terminal() => OutcomeV1::failed(
            CheckV1::TerminalOnlyFromTheWire,
            &case.name,
            format!(
                "an http {} query produced `{}`, a terminal; a failed query means the \
                 observation did not happen, not that the task ended",
                parts.status,
                observation.state_word()
            ),
        ),
        Ok(_) => OutcomeV1::passed(CheckV1::TerminalOnlyFromTheWire, &case.name),
        Err(error) => OutcomeV1::failed(
            CheckV1::TerminalOnlyFromTheWire,
            &case.name,
            format!("a non-2xx query must yield an observation, not an error: {}", error.message),
        ),
    };
    Some(verdict)
}

fn truncate(value: &Value) -> String {
    const LIMIT: usize = 240;
    let rendered = value.to_string();
    if rendered.len() <= LIMIT {
        return rendered;
    }
    format!("{}…", &rendered[..LIMIT])
}
