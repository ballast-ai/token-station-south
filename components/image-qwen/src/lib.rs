//! The Bailian Qwen-Image image guest is a thin shell over the shared native reference.

wit_bindgen::generate!({
    path: "../../crates/south-provider-api/wit/image-adapter.wit",
    world: "image-adapter-v1",
});

use exports::token_station::image_adapter::image_adapter::{
    AdapterHealth, AdapterMetadata, Guest, HealthStatus,
};
use south_component_conformance::reference_qwen_image::QwenImageReferenceV1;
use south_component_conformance::{abi_image as abi, ImageComponentV1};

struct QwenImage;
impl Guest for QwenImage {
    fn metadata() -> AdapterMetadata {
        let metadata = QwenImageReferenceV1.metadata();
        AdapterMetadata {
            name: metadata.name,
            version: metadata.version,
            api_version: metadata.api_version,
        }
    }
    fn healthcheck() -> AdapterHealth {
        AdapterHealth { status: HealthStatus::Ready, detail: None }
    }
    fn model_capabilities(provider_config: String) -> Result<String, String> {
        abi::model_capabilities_json(&QwenImageReferenceV1, &provider_config)
    }
    fn prepare(
        provider_config: String,
        request: String,
        context: String,
    ) -> Result<String, String> {
        abi::prepare_json(&QwenImageReferenceV1, &provider_config, &request, &context)
    }
    fn parse_response(state: String, response: String) -> Result<String, String> {
        abi::parse_response_json(&QwenImageReferenceV1, &state, &response)
    }
    fn render(state: String, outcomes: String, render_context: String) -> Result<String, String> {
        abi::render_json(&QwenImageReferenceV1, &state, &outcomes, &render_context)
    }
}
export!(QwenImage);
