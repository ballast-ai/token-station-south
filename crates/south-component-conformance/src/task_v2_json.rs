//! The unique strict task-v2 JSON codec shared by fixtures, ABI and sandbox.
use crate::{PreparedTaskV2, SubmitOutcomeV2};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use south_contracts::{
    MAX_ARTIFACT_REF_BYTES, MAX_JSON_REQUEST_BODY_BYTES, TaskArtifactRefV2, TaskArtifactV2,
    TaskFailureKindV1, TaskLocatorV2, TaskObservationV2, TaskRenderContextV2, TaskScalarV2,
    TaskUsageFactsV2,
};
use token_station_protocol::{ErrorEnvelope, HttpRequestDescriptor};

/// Maximum encoded fact frame, including worst-case JSON string escaping.
pub const MAX_TASK_V2_FACT_JSON_BYTES: usize = 4 * 1024 * 1024;
/// Locator frames cannot hide large ignored fields beside a small route.
pub const MAX_TASK_V2_LOCATOR_JSON_BYTES: usize = 16 * 1024;
fn parse<T: DeserializeOwned>(input: &str, limit: usize) -> Result<T, String> {
    if input.len() > limit {
        return Err("task-v2 JSON exceeds the boundary limit".into());
    }
    serde_json::from_str(input).map_err(|_| "invalid task-v2 JSON".into())
}
fn bounded(value: Value, limit: usize) -> Result<Value, String> {
    if value.to_string().len() > limit {
        return Err("task-v2 JSON exceeds the boundary limit".into());
    }
    Ok(value)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocatorWire {
    schema_version: u16,
    route: String,
}
impl LocatorWire {
    fn build(self) -> Result<TaskLocatorV2, String> {
        TaskLocatorV2::new(self.schema_version, &self.route).map_err(|error| error.to_string())
    }
}
/// Decodes a strict, versioned and bounded recovery locator.
pub fn parse_locator_json(input: &str) -> Result<TaskLocatorV2, String> {
    parse::<LocatorWire>(input, MAX_TASK_V2_LOCATOR_JSON_BYTES)?.build()
}
/// Encodes an already validated immutable locator.
#[must_use]
pub fn locator_json(value: &TaskLocatorV2) -> Value {
    json!({"schema_version":value.schema_version(),"route":value.route()})
}
/// Converts a closed scalar without routing large integers through f64.
pub fn scalar_from_json(value: &Value) -> Result<TaskScalarV2, String> {
    let scalar = match value {
        Value::Null => TaskScalarV2::Null,
        Value::String(value) => TaskScalarV2::String(value.clone()),
        Value::Number(value) if value.is_u64() => {
            TaskScalarV2::Unsigned(value.as_u64().ok_or("invalid unsigned task scalar")?)
        }
        Value::Number(value) if value.is_i64() => {
            TaskScalarV2::Signed(value.as_i64().ok_or("invalid signed task scalar")?)
        }
        Value::Number(value) => {
            TaskScalarV2::Float(value.as_f64().ok_or("invalid floating task scalar")?)
        }
        _ => return Err("task artifact scalar must be a number, string or null".into()),
    };
    scalar.validate().map_err(|error| error.to_string())?;
    Ok(scalar)
}
/// Validates public scalar enum construction before encoding.
pub fn scalar_json(value: &TaskScalarV2) -> Result<Value, String> {
    value.validate().map_err(|error| error.to_string())?;
    Ok(match value {
        TaskScalarV2::Null => Value::Null,
        TaskScalarV2::String(value) => json!(value),
        TaskScalarV2::Signed(value) => json!(value),
        TaskScalarV2::Unsigned(value) => json!(value),
        TaskScalarV2::Float(value) => json!(value),
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactWire {
    url: String,
    id: Value,
    duration: Value,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum ArtifactsWire {
    Urls { items: Vec<ArtifactWire> },
    FileId { file_id: String },
    None {},
}
impl ArtifactsWire {
    fn build(self) -> Result<TaskArtifactRefV2, String> {
        match self {
            Self::None {} => Ok(TaskArtifactRefV2::None),
            Self::FileId { file_id } => {
                TaskArtifactRefV2::file_id(&file_id).map_err(|error| error.to_string())
            }
            Self::Urls { items } => {
                let items = items
                    .into_iter()
                    .map(|item| {
                        TaskArtifactV2::new(
                            &item.url,
                            scalar_from_json(&item.id)?,
                            scalar_from_json(&item.duration)?,
                        )
                        .map_err(|error| error.to_string())
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                TaskArtifactRefV2::urls(items).map_err(|error| error.to_string())
            }
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UsageWire {
    seconds: Option<f64>,
    milliunits: Option<i64>,
    tokens: Option<i64>,
}
impl UsageWire {
    fn build(self) -> Result<TaskUsageFactsV2, String> {
        TaskUsageFactsV2::new(self.seconds, self.milliunits, self.tokens)
            .map_err(|error| error.to_string())
    }
}
#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
enum ObservationWire {
    Progress { running: bool, status_word: String },
    Succeeded { artifacts: ArtifactsWire, usage: UsageWire },
    Failed { kind: String, code: Option<String>, message: Option<String> },
    Unknown { reason: String },
}
impl ObservationWire {
    fn build(self) -> Result<TaskObservationV2, String> {
        let observation = match self {
            Self::Progress { running, status_word } => {
                TaskObservationV2::Progress { running, status_word }
            }
            Self::Succeeded { artifacts, usage } => TaskObservationV2::Succeeded {
                artifacts: artifacts.build()?,
                usage: usage.build()?,
            },
            Self::Failed { kind, code, message } => {
                let kind = TaskFailureKindV1::ALL
                    .into_iter()
                    .find(|candidate| candidate.word() == kind)
                    .ok_or("invalid task failure kind")?;
                TaskObservationV2::Failed { kind, code, message }
            }
            Self::Unknown { reason } => TaskObservationV2::Unknown { reason },
        };
        observation.validate().map_err(|error| error.to_string())?;
        Ok(observation)
    }
}
/// Decodes facts and validates their complete closed shape.
pub fn parse_observation_json(input: &str) -> Result<TaskObservationV2, String> {
    parse::<ObservationWire>(input, MAX_TASK_V2_FACT_JSON_BYTES)?.build()
}
/// Encodes facts only after validating public enum construction.
pub fn observation_json(observation: &TaskObservationV2) -> Result<Value, String> {
    observation.validate().map_err(|error| error.to_string())?;
    let value = match observation {
        TaskObservationV2::Progress { running, status_word } => {
            json!({"state":"progress","running":running,"status_word":status_word})
        }
        TaskObservationV2::Unknown { reason } => json!({"state":"unknown","reason":reason}),
        TaskObservationV2::Failed { kind, code, message } => {
            json!({"state":"failed","kind":kind.word(),"code":code,"message":message})
        }
        TaskObservationV2::Succeeded { artifacts, usage } => {
            let artifacts = match artifacts {
                TaskArtifactRefV2::None => json!({"kind":"none"}),
                TaskArtifactRefV2::FileId(id) => json!({"kind":"file-id","file_id":id}),
                TaskArtifactRefV2::Urls(items) => {
                    let items=items.iter().map(|item|Ok(json!({"url":item.url(),"id":scalar_json(item.id())?,"duration":scalar_json(item.duration())?})))
                        .collect::<Result<Vec<Value>,String>>()?;
                    json!({"kind":"urls","items":items})
                }
            };
            json!({"state":"succeeded","artifacts":artifacts,"usage":{
                "seconds":usage.seconds(),"milliunits":usage.milliunits(),"tokens":usage.tokens()}})
        }
    };
    bounded(value, MAX_TASK_V2_FACT_JSON_BYTES)
}
fn validate_upstream_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > MAX_ARTIFACT_REF_BYTES {
        return Err("invalid upstream task identifier".into());
    }
    Ok(())
}
fn validate_terminal(observation: &TaskObservationV2) -> Result<(), String> {
    observation.validate().map_err(|error| error.to_string())?;
    if !matches!(
        observation,
        TaskObservationV2::Succeeded { .. } | TaskObservationV2::Failed { .. }
    ) {
        return Err("accepted-terminal must carry a terminal task observation".into());
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case", deny_unknown_fields)]
enum SubmitWire {
    Accepted { upstream_task_id: String },
    AcceptedTerminal { observation: ObservationWire },
    Rejected { error: ErrorEnvelope },
    Unknown {},
}
/// Decodes acceptance without turning uncertainty into rejection.
pub fn parse_submit_outcome_json(input: &str) -> Result<SubmitOutcomeV2, String> {
    let outcome = match parse::<SubmitWire>(input, MAX_TASK_V2_FACT_JSON_BYTES)? {
        SubmitWire::Accepted { upstream_task_id } => {
            validate_upstream_id(&upstream_task_id)?;
            SubmitOutcomeV2::Accepted(upstream_task_id)
        }
        SubmitWire::AcceptedTerminal { observation } => {
            let observation = observation.build()?;
            validate_terminal(&observation)?;
            SubmitOutcomeV2::AcceptedTerminal(observation)
        }
        SubmitWire::Rejected { error } => SubmitOutcomeV2::Rejected(error),
        SubmitWire::Unknown {} => SubmitOutcomeV2::Unknown,
    };
    // Canonical JSON may expand numeric spellings or add descriptor defaults.
    submit_outcome_json(&outcome)?;
    Ok(outcome)
}
/// Validates all public outcome variants before encoding.
pub fn submit_outcome_json(outcome: &SubmitOutcomeV2) -> Result<Value, String> {
    let value = match outcome {
        SubmitOutcomeV2::Accepted(id) => {
            validate_upstream_id(id)?;
            json!({"outcome":"accepted","upstream_task_id":id})
        }
        SubmitOutcomeV2::AcceptedTerminal(observation) => {
            validate_terminal(observation)?;
            json!({"outcome":"accepted-terminal","observation":observation_json(observation)?})
        }
        SubmitOutcomeV2::Rejected(error) => json!({"outcome":"rejected","error":error}),
        SubmitOutcomeV2::Unknown => json!({"outcome":"unknown"}),
    };
    bounded(value, MAX_TASK_V2_FACT_JSON_BYTES)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedWire {
    descriptor: HttpRequestDescriptor,
    locator: LocatorWire,
}
/// Decodes a descriptor plus strict locator; network authorization remains host-owned.
pub fn parse_prepared_task_json(input: &str) -> Result<PreparedTaskV2, String> {
    let value = parse::<PreparedWire>(input, MAX_JSON_REQUEST_BODY_BYTES)?;
    let prepared = PreparedTaskV2 { descriptor: value.descriptor, locator: value.locator.build()? };
    prepared_task_json(&prepared)?;
    Ok(prepared)
}
/// Encodes a request without changing its descriptor or weakening locator validation.
pub fn prepared_task_json(value: &PreparedTaskV2) -> Result<Value, String> {
    TaskLocatorV2::new(value.locator.schema_version(), value.locator.route())
        .map_err(|error| error.to_string())?;
    bounded(
        json!({"descriptor":value.descriptor,"locator":locator_json(&value.locator)}),
        MAX_JSON_REQUEST_BODY_BYTES,
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextWire {
    task_id: String,
    created: i64,
    model: String,
    provider: String,
    upstream_task_id: Option<String>,
}
/// Decodes explicit bounded host metadata, never a callback declaration.
pub fn parse_render_context_json(input: &str) -> Result<TaskRenderContextV2, String> {
    let value = parse::<ContextWire>(input, MAX_TASK_V2_FACT_JSON_BYTES)?;
    TaskRenderContextV2::new(
        &value.task_id,
        value.created,
        &value.model,
        &value.provider,
        value.upstream_task_id.as_deref(),
    )
    .map_err(|error| error.to_string())
}
/// Encodes already validated immutable host metadata.
#[must_use]
pub fn render_context_json(value: &TaskRenderContextV2) -> Value {
    json!({"task_id":value.task_id(),"created":value.created(),"model":value.model(),
        "provider":value.provider(),"upstream_task_id":value.upstream_task_id()})
}
