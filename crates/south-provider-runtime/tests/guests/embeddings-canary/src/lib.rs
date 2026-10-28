//! An embeddings component for a family no host knows: `lexicon-canary-wire`.
//!
//! It exists for an adopting host's "unknown embeddings family" acceptance, the
//! embeddings-world sibling of `t09-canary-task`. The host is built once and its
//! binary digest recorded; afterwards only this package, a binding, a catalog
//! row with an embeddings route and a price are added. If an embeddings request
//! comes back with the upstream's vectors and settles at the reported token
//! count, the host served an embeddings family that is absent from its own code.
//!
//! # Why the wire is deliberately unlike the known families
//!
//! The request posts `{"passages", "lens"}` to `<base>/lexicon/v2/fold`; the
//! upstream answers `{"grains": [{"coordinates": [...]}...], "meter": {"read":
//! n}, "lens"}`. No released embeddings family spells any of these, so the host
//! can only find the vectors through the locator this component declares.
//!
//! # Deliberately inside the embeddings contract
//!
//! Text inputs only, a bearer credential, the `array` vector locator and the
//! upstream's reported token count. Token-id and media inputs are refused
//! before dispatch.

wit_bindgen::generate!({
    path: "../../../../south-provider-api/wit/embeddings-adapter.wit",
    world: "embeddings-adapter-v1",
});

use exports::token_station::embeddings_adapter::embeddings_adapter::{
    AdapterHealth, AdapterMetadata, Guest, HealthStatus,
};
use serde_json::{json, Value};

const NAME: &str = "embeddings-canary";
const VERSION: &str = "1.0.0";
const WORLD: &str = "embeddings-adapter-v1";
const FAMILY: &str = "lexicon-canary-wire";
const FOLD_PATH: &str = "lexicon/v2/fold";

fn error(code: &str, http_status: u16, message: &str) -> String {
    json!({ "code": code, "http_status": http_status, "message": message }).to_string()
}

fn parse(input: &str, what: &str) -> Result<Value, String> {
    serde_json::from_str(input)
        .map_err(|_| error("internal", 500, &format!("embeddings canary: {what} is not JSON")))
}

fn build(config: &Value, request: &Value) -> Result<Value, String> {
    if config.get("provider").and_then(Value::as_str) != Some(FAMILY) {
        return Err(error("capability", 400, &format!("this package serves the {FAMILY} family")));
    }
    let base = config.get("base_url").and_then(Value::as_str).ok_or_else(|| {
        error("internal", 500, "embeddings canary: the provider config has no base URL")
    })?;
    let lens = request
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| error("internal", 500, "embeddings canary: the request has no model"))?;
    let inputs = request.get("inputs").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut passages = Vec::with_capacity(inputs.len());
    for input in &inputs {
        match input.get("text").and_then(Value::as_str) {
            Some(text) => passages.push(Value::String(text.to_owned())),
            None => {
                return Err(error(
                    "capability",
                    400,
                    "the embeddings canary only folds text inputs",
                ));
            }
        }
    }
    if passages.is_empty() {
        return Err(error(
            "invalid_request",
            400,
            "the embeddings canary needs at least one input",
        ));
    }
    let auth = config
        .get("auth")
        .and_then(Value::as_str)
        .map(|slot| json!({ "scheme": "bearer", "secret": slot }));
    Ok(json!({
        "descriptor": {
            "method": "POST",
            "url": format!("{}/{FOLD_PATH}", base.trim_end_matches('/')),
            "headers": { "content-type": "application/json" },
            "body": { "passages": passages, "lens": lens },
            "auth": auth
        },
        "estimate": { "fallback_input_tokens": null, "max_input_tokens": null },
        "vectors": { "kind": "array", "array": "/grains", "vector": "/coordinates", "index": null },
        "immutable_body_paths": ["passages", "lens"],
        "parse_context": null
    }))
}

fn parse_response(parts: &Value) -> Result<Value, String> {
    let body = parts
        .get("body")
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .ok_or_else(|| {
            error("provider_protocol_error", 502, "embeddings canary: the body is not JSON")
        })?;
    let grains = body
        .get("grains")
        .and_then(Value::as_array)
        .ok_or_else(|| error("provider_protocol_error", 502, "embeddings canary: no grains"))?;
    let read = body.pointer("/meter/read").and_then(Value::as_u64).ok_or_else(|| {
        error("provider_protocol_error", 502, "embeddings canary: no meter reading")
    })?;
    Ok(json!({
        "usage": { "source": "reported", "input_tokens": read, "per_input_tokens": null },
        "vector_count": grains.len(),
        "upstream_model": body.get("lens").cloned().unwrap_or(Value::Null)
    }))
}

fn map_error(parts: &Value) -> Value {
    let status = parts.get("status").and_then(Value::as_u64).unwrap_or(0);
    let complaint = parts
        .get("body")
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|body| body.get("complaint").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default();
    if (400..500).contains(&status) {
        json!({
            "outcome": "rejected",
            "error": {
                "code": "invalid_request",
                "http_status": status,
                "message": "the upstream refused the passages",
                "provider_message": complaint
            }
        })
    } else {
        json!({
            "outcome": "unknown",
            "error": {
                "code": "upstream_unavailable",
                "http_status": 502,
                "message": "the upstream is unavailable",
                "provider_message": complaint
            }
        })
    }
}

struct EmbeddingsCanary;

impl Guest for EmbeddingsCanary {
    fn metadata() -> AdapterMetadata {
        AdapterMetadata {
            name: NAME.to_owned(),
            version: VERSION.to_owned(),
            api_version: WORLD.to_owned(),
        }
    }

    fn healthcheck() -> AdapterHealth {
        AdapterHealth { status: HealthStatus::Ready, detail: None }
    }

    fn build_embeddings_request(
        provider_config: String,
        embeddings_request: String,
    ) -> Result<String, String> {
        let config = parse(&provider_config, "the provider config")?;
        let request = parse(&embeddings_request, "the embeddings request")?;
        build(&config, &request).map(|prepared| prepared.to_string())
    }

    fn parse_embeddings_response(
        response_parts: String,
        _parse_context: String,
    ) -> Result<String, String> {
        parse_response(&parse(&response_parts, "the response parts")?)
            .map(|parsed| parsed.to_string())
    }

    fn map_provider_error(response_parts: String) -> Result<String, String> {
        Ok(map_error(&parse(&response_parts, "the response parts")?).to_string())
    }
}

export!(EmbeddingsCanary);
