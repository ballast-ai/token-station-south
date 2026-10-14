//! The synthetic speech guest is a thin shell over the speech suite's native reference.

// The path names the single file, not the `wit/` directory, which resolves several packages.
wit_bindgen::generate!({
    path: "../../../../south-provider-api/wit/speech-adapter.wit",
    world: "speech-adapter-v1",
});

use exports::token_station::speech_adapter::speech_adapter::{
    AdapterHealth, AdapterMetadata, Guest, HealthStatus,
};
use south_component_conformance::reference_synthetic_speech::SyntheticSpeechReferenceV1;
use south_component_conformance::{abi_speech as abi, SpeechComponentV1};

struct SyntheticSpeech;
impl Guest for SyntheticSpeech {
    fn metadata() -> AdapterMetadata {
        let metadata = SyntheticSpeechReferenceV1.metadata();
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
        abi::model_capabilities_json(&SyntheticSpeechReferenceV1, &provider_config)
    }
    fn prepare(
        provider_config: String,
        request: String,
        context: String,
    ) -> Result<String, String> {
        abi::prepare_json(&SyntheticSpeechReferenceV1, &provider_config, &request, &context)
    }
    fn parse_response(state: String, response: String) -> Result<String, String> {
        abi::parse_response_json(&SyntheticSpeechReferenceV1, &state, &response)
    }
}
export!(SyntheticSpeech);
