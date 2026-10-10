//! The official OpenAI Responses provider component: a wit-bindgen shell around the conformance
//! crate's native reference implementation.
//!
//! Gate ② froze its fixture pack against `OpenAiResponsesReferenceV1`; this crate compiles that
//! same implementation to `wasm32-wasip2`, so "the sandboxed output equals the native output" is a
//! property of construction that the sandbox parity test then proves end to end.
//!
//! The component splits its upstream's SSE stream with the conformance crate's own splitter. It
//! does not link `south-host-grammars`: that crate is host-only (boundary record §13.13), and the
//! shipped-packages test fails on a component lockfile that names it.

use std::sync::Mutex;

// The path names the single file, not the `wit/` directory: that directory resolves more than
// one package, and a directory path would make the generated module layout depend on which
// packages happen to sit beside this one.
wit_bindgen::generate!({
    path: "../../crates/south-provider-api/wit/provider-adapter.wit",
    world: "provider-adapter-v2",
});

use exports::token_station::adapter::provider_adapter::{AdapterHealth, AdapterMetadata, Guest};
use south_component_conformance::reference_openai_responses::OpenAiResponsesReferenceV1;
use south_component_conformance::{ProviderComponentV1, abi};
use token_station::adapter::common::HealthStatus;

/// The stream this instance is holding. Instance state on purpose: the host instantiates one
/// component per stream, so this only ever sees one provider's body.
static STREAM: Mutex<Option<abi::StreamAbiV1>> = Mutex::new(None);

struct OpenAiResponses;

impl Guest for OpenAiResponses {
    fn metadata() -> AdapterMetadata {
        let reported = OpenAiResponsesReferenceV1.metadata();
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
        abi::model_capabilities_json(&OpenAiResponsesReferenceV1, &provider_config)
    }

    fn build_http_request(chat_request: String, provider_config: String) -> Result<String, String> {
        abi::build_http_request_json(&OpenAiResponsesReferenceV1, &chat_request, &provider_config)
    }

    fn parse_response(response_parts: String) -> Result<String, String> {
        abi::parse_response_json(&OpenAiResponsesReferenceV1, &response_parts)
    }

    fn parse_stream_chunk(chunk: Vec<u8>) -> Result<String, String> {
        let mut stream = STREAM.lock().expect("single-threaded guest");
        stream
            .get_or_insert_with(|| abi::StreamAbiV1::new(&OpenAiResponsesReferenceV1))
            .parse_chunk_json(&chunk)
    }

    fn map_provider_error(response_parts: String) -> Result<String, String> {
        abi::map_provider_error_json(&OpenAiResponsesReferenceV1, &response_parts)
    }
}

export!(OpenAiResponses);
