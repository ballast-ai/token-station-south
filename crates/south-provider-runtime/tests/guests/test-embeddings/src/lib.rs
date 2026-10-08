//! The runtime's embeddings-world test guest.
//!
//! It exercises the world's plumbing — admission, the identity probe, the
//! three calls and their bounds — and nothing else. Each call checks that its
//! inputs are JSON and answers with a canned document that echoes them, so a
//! test can tell the arguments arrived in order; a non-JSON input is answered
//! on the error channel with an error envelope.

// The path names the single file, not the `wit/` directory, which resolves
// several packages.
wit_bindgen::generate!({
    path: "../../../../south-provider-api/wit/embeddings-adapter.wit",
    world: "embeddings-adapter-v1",
});

use exports::token_station::embeddings_adapter::embeddings_adapter::{
    AdapterHealth, AdapterMetadata, Guest, HealthStatus,
};
use serde_json::{json, Value};

struct TestEmbeddings;

fn error_envelope(http_status: u16, message: &str) -> Value {
    json!({ "code": "internal", "http_status": http_status, "message": message })
}

fn parse(input: &str, what: &str) -> Result<Value, String> {
    serde_json::from_str(input)
        .map_err(|_| error_envelope(500, &format!("{what} is not JSON")).to_string())
}

impl Guest for TestEmbeddings {
    fn metadata() -> AdapterMetadata {
        AdapterMetadata {
            name: "test-embeddings".to_owned(),
            version: "1.0.0".to_owned(),
            api_version: "embeddings-adapter-v1".to_owned(),
        }
    }

    fn healthcheck() -> AdapterHealth {
        AdapterHealth { status: HealthStatus::Ready, detail: None }
    }

    fn build_embeddings_request(
        provider_config: String,
        embeddings_request: String,
    ) -> Result<String, String> {
        let config = parse(&provider_config, "provider-config")?;
        let request = parse(&embeddings_request, "embeddings-request")?;
        Ok(json!({
            "descriptor": {
                "method": "POST",
                "url": "https://embeddings.example.test/v1/embeddings",
                "body": request,
            },
            "parse_context": { "config": config },
        })
        .to_string())
    }

    fn parse_embeddings_response(
        response_parts: String,
        parse_context: String,
    ) -> Result<String, String> {
        let parts = parse(&response_parts, "response-parts")?;
        let context = parse(&parse_context, "parse-context")?;
        Ok(json!({ "parts": parts, "context": context }).to_string())
    }

    fn map_provider_error(response_parts: String) -> Result<String, String> {
        let parts = parse(&response_parts, "response-parts")?;
        let error = error_envelope(502, "upstream failed");
        Ok(json!({ "outcome": "unknown", "error": error, "parts": parts }).to_string())
    }
}

export!(TestEmbeddings);
