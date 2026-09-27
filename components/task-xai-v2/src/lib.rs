//! The task-v2 xAI video guest is a thin shell over the shared native dialect.

wit_bindgen::generate!({
    path: "../../crates/south-provider-api/wit/task-adapter-v2.wit",
    world: "task-adapter-v2",
});

use exports::token_station::task_adapter::task_adapter::{
    AdapterHealth, AdapterMetadata, Guest, HealthStatus,
};
use south_component_conformance::reference_xai_task_v2::XaiTaskComponentV2;
use south_component_conformance::{abi_task_v2 as abi, TaskComponentV2};

struct XaiTask;
impl Guest for XaiTask {
    fn metadata() -> AdapterMetadata {
        let metadata = XaiTaskComponentV2.metadata();
        AdapterMetadata {
            name: metadata.name,
            version: metadata.version,
            api_version: metadata.api_version,
        }
    }
    fn healthcheck() -> AdapterHealth {
        AdapterHealth { status: HealthStatus::Ready, detail: None }
    }
    fn build_submit_request(
        config: String,
        request: String,
        minted: String,
    ) -> Result<String, String> {
        abi::build_submit_request_json(&XaiTaskComponentV2, &config, &request, &minted)
    }
    fn parse_submit_response(parts: String) -> Result<String, String> {
        abi::parse_submit_response_json(&XaiTaskComponentV2, &parts)
    }
    fn build_observe_request(
        config: String,
        model: String,
        id: String,
        locator: String,
    ) -> Result<String, String> {
        abi::build_observe_request_json(&XaiTaskComponentV2, &config, &model, &id, &locator)
    }
    fn parse_observation(parts: String) -> Result<String, String> {
        abi::parse_observation_json(&XaiTaskComponentV2, &parts)
    }
    fn build_artifact_request(
        config: String,
        locator: String,
        observation: String,
    ) -> Result<String, String> {
        abi::build_artifact_request_json(&XaiTaskComponentV2, &config, &locator, &observation)
    }
    fn render_success(
        observation: String,
        fetched: Option<String>,
        context: String,
    ) -> Result<String, String> {
        abi::render_success_json(&XaiTaskComponentV2, &observation, fetched.as_deref(), &context)
    }
    fn map_terminal_failure(observation: String) -> Result<String, String> {
        abi::map_terminal_failure_json(&XaiTaskComponentV2, &observation)
    }
}
export!(XaiTask);
