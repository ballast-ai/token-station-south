//! The official Kling task component: a wit-bindgen shell around the
//! conformance crate's native reference implementation.
//!
//! Gate ② froze its fixture pack against `KlingTaskReferenceV1`; this crate
//! compiles that same implementation to `wasm32-wasip2`, so "the sandboxed
//! output equals the native output" is a property of construction rather than
//! a coincidence two code paths have to maintain.
//!
//! # What does not exist yet
//!
//! Nothing instantiates this. `south-provider-runtime` binds
//! `provider-adapter-v2` and only that world, so there is no sandbox seam for
//! a task component and therefore no parity test beside the three provider
//! ones. Building the guest is still worth doing on its own: it proves the
//! reference compiles under the guest's constraints (no std::time, no
//! network, no filesystem) and that the world's WIT and the implementation
//! agree, which is exactly what would otherwise be discovered late.
//!
//! The runtime's second world is its own slice, with its own design record —
//! the manifest-schema record listed "whether the runtime can host more than
//! one world" among the things it did not decide.

wit_bindgen::generate!({
    path: "../../crates/south-provider-api/wit/task-adapter.wit",
    world: "task-adapter-v1",
});

use exports::token_station::task_adapter::task_adapter::{
    AdapterHealth, AdapterMetadata, Guest, SubmitOutcome,
};
use south_component_conformance::reference_kling_task::KlingTaskReferenceV1;
use south_component_conformance::{SubmitOutcomeV1, TaskComponentV1};
use south_contracts::{HostMintedValuesV1, TaskArtifactRefV1, TaskMeterV1, TaskObservationV1};
use token_station::task_adapter::common::HealthStatus;

struct KlingTask;

/// An error the guest reports as the world's `json` error payload.
fn envelope(detail: &str) -> String {
    serde_json::json!({ "code": "internal", "http_status": 500, "message": detail }).to_string()
}

fn parse<T: serde::de::DeserializeOwned>(raw: &str, what: &str) -> Result<T, String> {
    serde_json::from_str(raw).map_err(|source| envelope(&format!("{what} is not valid: {source}")))
}

/// The host-minted values as they cross the boundary.
#[derive(serde::Deserialize)]
struct MintedWire {
    task_id: String,
    #[serde(default)]
    callback_url: Option<String>,
}

impl MintedWire {
    fn build(&self) -> Result<HostMintedValuesV1, String> {
        HostMintedValuesV1::new(&self.task_id, self.callback_url.as_deref())
            .map_err(|source| envelope(&format!("host-minted values refused: {source}")))
    }
}

/// The observation as it crosses the boundary, in the same shape the fixture
/// pack writes. Kept here rather than derived on the contract type for the
/// reason that crate records: it validates input, it does not publish a wire
/// form of a vocabulary.
#[derive(serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
enum ObservationWire {
    Running {
        status_word: String,
    },
    Succeeded {
        #[serde(default)]
        urls: Vec<String>,
        #[serde(default)]
        file_id: Option<String>,
        #[serde(default)]
        meter: Option<MeterWire>,
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

#[derive(serde::Deserialize)]
#[serde(tag = "unit", content = "value", rename_all = "kebab-case")]
enum MeterWire {
    Seconds(f64),
    Tokens(i64),
    Milliunits(i64),
}

impl ObservationWire {
    fn build(self) -> Result<TaskObservationV1, String> {
        Ok(match self {
            Self::Running { status_word } => TaskObservationV1::Running { status_word },
            Self::Succeeded { urls, file_id, meter } => {
                let artifact = match (urls.is_empty(), file_id) {
                    (false, _) => TaskArtifactRefV1::urls(urls)
                        .map_err(|e| envelope(&format!("artifact refused: {e}")))?,
                    (true, Some(id)) => TaskArtifactRefV1::file_id(&id)
                        .map_err(|e| envelope(&format!("artifact refused: {e}")))?,
                    (true, None) => TaskArtifactRefV1::None,
                };
                let meter = meter.map(|m| match m {
                    MeterWire::Seconds(v) => TaskMeterV1::Seconds(v),
                    MeterWire::Tokens(v) => TaskMeterV1::Tokens(v),
                    MeterWire::Milliunits(v) => TaskMeterV1::Milliunits(v),
                });
                TaskObservationV1::Succeeded { artifact, meter }
            }
            Self::Failed { kind, code, message } => {
                let kind = south_contracts::TaskFailureKindV1::ALL
                    .into_iter()
                    .find(|candidate| candidate.word() == kind)
                    .ok_or_else(|| envelope(&format!("`{kind}` is not a task failure kind")))?;
                TaskObservationV1::Failed { kind, code, message }
            }
            Self::Unknown { reason } => TaskObservationV1::Unknown { reason },
        })
    }
}

fn observation_json(observation: &TaskObservationV1) -> serde_json::Value {
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

fn encode<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value)
        .map_err(|source| envelope(&format!("the component's own output is not json: {source}")))
}

impl Guest for KlingTask {
    fn metadata() -> AdapterMetadata {
        let reported = KlingTaskReferenceV1.metadata();
        AdapterMetadata {
            name: reported.name,
            version: reported.version,
            api_version: reported.api_version,
        }
    }

    fn healthcheck() -> AdapterHealth {
        // Stateless and pure: there is nothing that can be unwell between
        // calls. A component exporting the world at all is the load-time fact
        // the runtime checks.
        AdapterHealth { status: HealthStatus::Ready, detail: None }
    }

    fn build_submit_request(
        provider_config: String,
        task_request: String,
        host_minted: String,
    ) -> Result<String, String> {
        let config = parse(&provider_config, "provider-config")?;
        let request = parse(&task_request, "task-request")?;
        let minted: MintedWire = parse(&host_minted, "host-minted")?;
        let descriptor = KlingTaskReferenceV1
            .build_submit_request(&config, &request, &minted.build()?)
            .map_err(|e| envelope(&e.message))?;
        encode(&descriptor)
    }

    fn parse_submit_response(response_parts: String) -> Result<SubmitOutcome, String> {
        let parts = parse(&response_parts, "response-parts")?;
        let outcome =
            KlingTaskReferenceV1.parse_submit_response(&parts).map_err(|e| envelope(&e.message))?;
        // The world models this as a variant, so it crosses typed rather than
        // as a JSON string: the four cases are the contract, and a string
        // would let a typo become a fifth.
        Ok(match outcome {
            SubmitOutcomeV1::Accepted(id) => SubmitOutcome::Accepted(id),
            SubmitOutcomeV1::AcceptedTerminal(body) => {
                SubmitOutcome::AcceptedTerminal(body.to_string())
            }
            SubmitOutcomeV1::Rejected(refusal) => SubmitOutcome::Rejected(
                serde_json::json!({
                    "code": format!("{:?}", refusal.code),
                    "message": refusal.message,
                })
                .to_string(),
            ),
            SubmitOutcomeV1::Unknown => SubmitOutcome::Unknown,
        })
    }

    fn build_observe_request(
        provider_config: String,
        upstream_model: String,
        upstream_task_id: String,
    ) -> Result<String, String> {
        let config = parse(&provider_config, "provider-config")?;
        let descriptor = KlingTaskReferenceV1
            .build_observe_request(&config, &upstream_model, &upstream_task_id)
            .map_err(|e| envelope(&e.message))?;
        encode(&descriptor)
    }

    fn parse_observation(response_parts: String) -> Result<String, String> {
        let parts = parse(&response_parts, "response-parts")?;
        let observation =
            KlingTaskReferenceV1.parse_observation(&parts).map_err(|e| envelope(&e.message))?;
        Ok(observation_json(&observation).to_string())
    }

    fn build_artifact_request(
        provider_config: String,
        observation: String,
    ) -> Result<String, String> {
        let config = parse(&provider_config, "provider-config")?;
        let observation: ObservationWire = parse(&observation, "observation")?;
        let request = KlingTaskReferenceV1
            .build_artifact_request(&config, &observation.build()?)
            .map_err(|e| envelope(&e.message))?;
        // `none` crosses as JSON null: "already have it", not "not supported".
        match request {
            Some(descriptor) => encode(&descriptor),
            None => Ok("null".to_owned()),
        }
    }

    fn render_success(
        observation: String,
        fetched: Option<String>,
        host_minted: String,
    ) -> Result<String, String> {
        let observation: ObservationWire = parse(&observation, "observation")?;
        let minted: MintedWire = parse(&host_minted, "host-minted")?;
        let fetched = match fetched {
            Some(raw) => Some(parse(&raw, "fetched")?),
            None => None,
        };
        let body = KlingTaskReferenceV1
            .render_success(&observation.build()?, fetched.as_ref(), &minted.build()?)
            .map_err(|e| envelope(&e.message))?;
        Ok(body.to_string())
    }

    fn map_terminal_failure(observation: String) -> Result<String, String> {
        let observation: ObservationWire = parse(&observation, "observation")?;
        let mapped = KlingTaskReferenceV1
            .map_terminal_failure(&observation.build()?)
            .map_err(|e| envelope(&e.message))?;
        encode(&mapped)
    }
}

export!(KlingTask);
