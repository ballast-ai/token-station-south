//! The OpenAI-compatible embeddings guest is a thin shell over the shared native reference.

wit_bindgen::generate!({
    path: "../../crates/south-provider-api/wit/embeddings-adapter.wit",
    world: "embeddings-adapter-v1",
});

use exports::token_station::embeddings_adapter::embeddings_adapter::{
    AdapterHealth, AdapterMetadata, Guest, HealthStatus,
};
use south_component_conformance::reference_openai_compatible_embeddings::OpenAiCompatibleEmbeddingsReferenceV1;
use south_component_conformance::{abi_embeddings as abi, EmbeddingsComponentV1};

struct OpenAiCompatibleEmbeddings;
impl Guest for OpenAiCompatibleEmbeddings {
    fn metadata() -> AdapterMetadata {
        let metadata = OpenAiCompatibleEmbeddingsReferenceV1.metadata();
        AdapterMetadata {
            name: metadata.name,
            version: metadata.version,
            api_version: metadata.api_version,
        }
    }
    fn healthcheck() -> AdapterHealth {
        AdapterHealth { status: HealthStatus::Ready, detail: None }
    }
    fn build_embeddings_request(
        provider_config: String,
        embeddings_request: String,
    ) -> Result<String, String> {
        abi::build_embeddings_request_json(
            &OpenAiCompatibleEmbeddingsReferenceV1,
            &provider_config,
            &embeddings_request,
        )
    }
    fn parse_embeddings_response(
        response_parts: String,
        parse_context: String,
    ) -> Result<String, String> {
        abi::parse_embeddings_response_json(
            &OpenAiCompatibleEmbeddingsReferenceV1,
            &response_parts,
            &parse_context,
        )
    }
    fn map_provider_error(response_parts: String) -> Result<String, String> {
        abi::map_provider_error_json(&OpenAiCompatibleEmbeddingsReferenceV1, &response_parts)
    }
}
export!(OpenAiCompatibleEmbeddings);
