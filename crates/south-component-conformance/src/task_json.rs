//! The one JSON shape the task vocabulary crosses boundaries in.
//!
//! `south-contracts` publishes no serde derives — it uses serde to *validate*
//! input, never to publish a wire form of a vocabulary, and a derive would
//! make whatever JSON someone happens to write a de-facto part of the
//! contract. So the form lives here, with **one** definition shared by the
//! fixture pack, the conformance suite and the sandbox seam.
//!
//! Sharing matters more than it looks: a second copy would let a fixture pass
//! against the native reference and fail against the sandboxed one for no
//! reason but a spelling difference, which is exactly the kind of divergence
//! gate ② exists to rule out.

use serde::Deserialize;
use serde_json::Value;
use south_contracts::{TaskArtifactRefV1, TaskFailureKindV1, TaskMeterV1, TaskObservationV1};

use crate::component::SubmitOutcomeV1;

/// A `TaskObservationV1` as a fixture spells it.
///
/// The contract's own types carry no serde derives — that crate uses serde to
/// *validate* input, never to publish a wire form of a vocabulary, and adding
/// a derive would make every JSON shape someone happens to write a de-facto
/// part of the contract. The fixture layer owns its own representation
/// instead, and the mapping below is the one place the two meet.
#[derive(Debug, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ObservationInput {
    Running {
        status_word: String,
    },
    Succeeded {
        #[serde(default)]
        urls: Vec<String>,
        #[serde(default)]
        file_id: Option<String>,
        #[serde(default)]
        meter: Option<MeterInput>,
    },
    Failed {
        kind: String,
        #[serde(default)]
        code: Option<String>,
        #[serde(default)]
        message: Option<String>,
    },
    Unknown {
        reason: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "unit", content = "value", rename_all = "kebab-case")]
pub enum MeterInput {
    Seconds(f64),
    Tokens(i64),
    Milliunits(i64),
}

impl MeterInput {
    #[must_use]
    pub const fn build(&self) -> TaskMeterV1 {
        match *self {
            Self::Seconds(value) => TaskMeterV1::Seconds(value),
            Self::Tokens(value) => TaskMeterV1::Tokens(value),
            Self::Milliunits(value) => TaskMeterV1::Milliunits(value),
        }
    }
}

impl ObservationInput {
    pub fn build(self) -> Result<TaskObservationV1, String> {
        Ok(match self {
            Self::Running { status_word } => TaskObservationV1::Running { status_word },
            Self::Succeeded { urls, file_id, meter } => {
                let artifact = match (urls.is_empty(), file_id) {
                    (false, _) => {
                        TaskArtifactRefV1::urls(urls).map_err(|source| source.to_string())?
                    }
                    (true, Some(id)) => {
                        TaskArtifactRefV1::file_id(&id).map_err(|source| source.to_string())?
                    }
                    (true, None) => TaskArtifactRefV1::None,
                };
                TaskObservationV1::Succeeded { artifact, meter: meter.map(|m| m.build()) }
            }
            Self::Failed { kind, code, message } => {
                let kind = TaskFailureKindV1::ALL
                    .into_iter()
                    .find(|candidate| candidate.word() == kind)
                    .ok_or_else(|| format!("`{kind}` is not a task failure kind"))?;
                TaskObservationV1::Failed { kind, code, message }
            }
            Self::Unknown { reason } => TaskObservationV1::Unknown { reason },
        })
    }
}

/// Renders an observation into the same JSON shape a fixture writes.
///
/// The inverse of [`ObservationInput::build`], and here for the same reason:
/// the contract publishes no wire form, so the pack and the suite agree on one
/// here. Symmetry with the input form is what makes an `expected.json`
/// readable beside its `input.json`.
#[must_use]
pub fn observation_json(observation: &TaskObservationV1) -> Value {
    match observation {
        TaskObservationV1::Running { status_word } => {
            serde_json::json!({ "state": "running", "status_word": status_word })
        }
        TaskObservationV1::Succeeded { artifact, meter } => {
            let mut value = serde_json::json!({ "state": "succeeded" });
            match artifact {
                TaskArtifactRefV1::Urls(urls) => value["urls"] = serde_json::json!(urls),
                TaskArtifactRefV1::FileId(id) => value["file_id"] = serde_json::json!(id),
                TaskArtifactRefV1::None => {}
            }
            if let Some(meter) = meter {
                value["meter"] = match *meter {
                    TaskMeterV1::Seconds(v) => serde_json::json!({"unit": "seconds", "value": v}),
                    TaskMeterV1::Tokens(v) => serde_json::json!({"unit": "tokens", "value": v}),
                    TaskMeterV1::Milliunits(v) => {
                        serde_json::json!({"unit": "milliunits", "value": v})
                    }
                };
            }
            value
        }
        TaskObservationV1::Failed { kind, code, message } => {
            let mut value = serde_json::json!({ "state": "failed", "kind": kind.word() });
            if let Some(code) = code {
                value["code"] = serde_json::json!(code);
            }
            if let Some(message) = message {
                value["message"] = serde_json::json!(message);
            }
            value
        }
        TaskObservationV1::Unknown { reason } => {
            serde_json::json!({ "state": "unknown", "reason": reason })
        }
    }
}

/// Renders a submit outcome into the shape a fixture writes.
#[must_use]
pub fn submit_outcome_json(outcome: &SubmitOutcomeV1) -> Value {
    match outcome {
        SubmitOutcomeV1::Accepted(id) => {
            serde_json::json!({ "outcome": "accepted", "upstream_task_id": id })
        }
        SubmitOutcomeV1::AcceptedTerminal(body) => {
            serde_json::json!({ "outcome": "accepted-terminal", "body": body })
        }
        SubmitOutcomeV1::Rejected(envelope) => serde_json::json!({
            "outcome": "rejected",
            "code": format!("{:?}", envelope.code),
            "message": envelope.message,
        }),
        SubmitOutcomeV1::Unknown => serde_json::json!({ "outcome": "unknown" }),
    }
}
