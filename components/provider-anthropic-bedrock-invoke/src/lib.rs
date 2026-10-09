//! The official Bedrock InvokeModel Anthropic provider component: a wit-bindgen shell around the
//! conformance crate's native reference implementation.
//!
//! Gate ② froze its fixture pack against `AnthropicBedrockInvokeReferenceV1`; this crate compiles
//! that same implementation to `wasm32-wasip2`, so "the sandboxed output equals the native output"
//! is a property of construction that the sandbox parity test then proves end to end.
//!
//! Like the Converse package, this component is on the `host_signed` arm: it never sees a
//! credential, and the host's `aws-sigv4` finalizer signs the request after `build_http_request`
//! returns. The arm is declared in `manifest.json` and honoured by the reference implementation
//! leaving `descriptor.auth` unset.

use std::sync::Mutex;

// The path names the single file, not the `wit/` directory: that directory resolves more than
// one package, and a directory path would make the generated module layout depend on which
// packages happen to sit beside this one.
wit_bindgen::generate!({
    path: "../../crates/south-provider-api/wit/provider-adapter.wit",
    world: "provider-adapter-v2",
});

use exports::token_station::adapter::provider_adapter::{AdapterHealth, AdapterMetadata, Guest};
use south_component_conformance::reference_anthropic_bedrock_invoke::AnthropicBedrockInvokeReferenceV1;
use south_component_conformance::{ProviderComponentV1, abi};
use token_station::adapter::common::HealthStatus;

/// The stream this instance is holding. Instance state on purpose: the host instantiates one
/// component per stream, so this only ever sees one provider's body.
static STREAM: Mutex<Option<abi::StreamAbiV1>> = Mutex::new(None);

struct AnthropicBedrockInvoke;

impl Guest for AnthropicBedrockInvoke {
    fn metadata() -> AdapterMetadata {
        let reported = AnthropicBedrockInvokeReferenceV1.metadata();
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
        abi::model_capabilities_json(&AnthropicBedrockInvokeReferenceV1, &provider_config)
    }

    fn build_http_request(chat_request: String, provider_config: String) -> Result<String, String> {
        abi::build_http_request_json(
            &AnthropicBedrockInvokeReferenceV1,
            &chat_request,
            &provider_config,
        )
    }

    fn parse_response(response_parts: String) -> Result<String, String> {
        abi::parse_response_json(&AnthropicBedrockInvokeReferenceV1, &response_parts)
    }

    fn parse_stream_chunk(chunk: Vec<u8>) -> Result<String, String> {
        let mut stream = STREAM.lock().expect("single-threaded guest");
        stream
            .get_or_insert_with(|| abi::StreamAbiV1::new(&AnthropicBedrockInvokeReferenceV1))
            .parse_chunk_json(&chunk)
    }

    fn map_provider_error(response_parts: String) -> Result<String, String> {
        abi::map_provider_error_json(&AnthropicBedrockInvokeReferenceV1, &response_parts)
    }
}

export!(AnthropicBedrockInvoke);
