//! The official AWS Bedrock Converse provider component for Bedrock API keys: a
//! wit-bindgen shell around the conformance crate's native reference
//! implementation, `BedrockConverseBearerReferenceV1`.
//!
//! Gate ② froze its fixture pack against `BedrockConverseBearerReferenceV1`; this
//! crate compiles that same implementation to `wasm32-wasip2`, so "the sandboxed
//! output equals the native output" is a property of construction that the
//! sandbox parity test then proves end to end.
//!
//! It is the Bearer sibling of `provider-bedrock-converse` (host feedback SF16,
//! host-zero-vendor-boundary §13.6): the same request, response and stream
//! translation, on the `bearer` arm and the `bedrock-bearer` family instead of
//! `host_signed` and `bedrock`. `host_signed` admits no second arm, so the two
//! credential forms are two packages, and a host picks one by the row's family,
//! never by the shape of the stored credential.

use std::sync::Mutex;

// The path names the single file, not the `wit/` directory: since the task
// world arrived (2026-09-18) that directory resolves two packages, and a
// directory path would make the generated module layout depend on which
// packages happen to sit beside this one.
wit_bindgen::generate!({
    path: "../../crates/south-provider-api/wit/provider-adapter.wit",
    world: "provider-adapter-v2",
});

use exports::token_station::adapter::provider_adapter::{AdapterHealth, AdapterMetadata, Guest};
use south_component_conformance::reference_bedrock_converse::BedrockConverseBearerReferenceV1;
use south_component_conformance::{ProviderComponentV1, abi};
use token_station::adapter::common::HealthStatus;

/// The stream this instance is holding. Instance state on purpose: the host
/// instantiates one component per stream, so this only ever sees one
/// provider's body.
static STREAM: Mutex<Option<abi::StreamAbiV1>> = Mutex::new(None);

struct BedrockConverseBearer;

impl Guest for BedrockConverseBearer {
    fn metadata() -> AdapterMetadata {
        let reported = BedrockConverseBearerReferenceV1.metadata();
        AdapterMetadata {
            name: reported.name,
            version: reported.version,
            api_version: reported.api_version,
        }
    }

    fn healthcheck() -> AdapterHealth {
        AdapterHealth { status: HealthStatus::Ready, detail: None }
    }

    fn model_capabilities(provider_config: String) -> Result<String, String> {
        abi::model_capabilities_json(&BedrockConverseBearerReferenceV1, &provider_config)
    }

    fn build_http_request(chat_request: String, provider_config: String) -> Result<String, String> {
        abi::build_http_request_json(&BedrockConverseBearerReferenceV1, &chat_request, &provider_config)
    }

    fn parse_response(response_parts: String) -> Result<String, String> {
        abi::parse_response_json(&BedrockConverseBearerReferenceV1, &response_parts)
    }

    fn parse_stream_chunk(chunk: Vec<u8>) -> Result<String, String> {
        let mut stream = STREAM.lock().expect("single-threaded guest");
        stream
            .get_or_insert_with(|| abi::StreamAbiV1::new(&BedrockConverseBearerReferenceV1))
            .parse_chunk_json(&chunk)
    }

    fn map_provider_error(response_parts: String) -> Result<String, String> {
        abi::map_provider_error_json(&BedrockConverseBearerReferenceV1, &response_parts)
    }
}

export!(BedrockConverseBearer);
