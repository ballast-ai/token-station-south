//! The MiMo speech guest is a thin shell over the shared native reference.

// The path names the single file, not the `wit/` directory, which resolves several packages.
wit_bindgen::generate!({
    path: "../../crates/south-provider-api/wit/speech-adapter.wit",
    world: "speech-adapter-v1",
});

use exports::token_station::speech_adapter::speech_adapter::{
    AdapterHealth, AdapterMetadata, Guest, HealthStatus,
};
use south_component_conformance::reference_mimo_speech::MimoSpeechReferenceV1;
use south_component_conformance::{abi_speech as abi, SpeechComponentV1};

struct MimoSpeech;
impl Guest for MimoSpeech {
    fn metadata() -> AdapterMetadata {
        let metadata = MimoSpeechReferenceV1.metadata();
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
        abi::model_capabilities_json(&MimoSpeechReferenceV1, &provider_config)
    }
    fn prepare(
        provider_config: String,
        request: String,
        context: String,
    ) -> Result<String, String> {
        abi::prepare_json(&MimoSpeechReferenceV1, &provider_config, &request, &context)
    }
    fn parse_response(state: String, response: String) -> Result<String, String> {
        abi::parse_response_json(&MimoSpeechReferenceV1, &state, &response)
    }
}
export!(MimoSpeech);
