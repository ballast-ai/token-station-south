//! A speech component for a family no host knows: `chorus-canary-wire`.
//!
//! It exists for an adopting host's "unknown speech family" acceptance, the
//! speech-world sibling of `t09-canary-task`. The host is built once and its
//! binary digest recorded; afterwards only this package, a binding, a catalog
//! row with a speech route and a price are added. If a synthesis request comes
//! back with the upstream's audio bytes and settles at the per-character price,
//! the host served a speech family that is absent from its own code.
//!
//! # Why the wire is deliberately unlike the known families
//!
//! Synthesis posts `{"script", "timbre", "engine"}` to `<base>/voicebox/v3/utter`
//! and the upstream answers with the WAV bytes as the body. No released speech
//! family spells any of these.
//!
//! # Deliberately inside the speech contract
//!
//! Synthesis only, one JSON request, a bearer credential, the audio as the
//! response body and the `characters` metering form counted from the request.
//! Recognition is not served: a `transcribe` call is refused before dispatch.
//!
//! # Input
//!
//! A JSON request view: `input` (required non-empty string), `voice`
//! (optional string, default `plain`) and `response_format` (optional, only
//! `wav`). Everything else is ignored.

wit_bindgen::generate!({
    path: "../../../../south-provider-api/wit/speech-adapter.wit",
    world: "speech-adapter-v1",
});

use exports::token_station::speech_adapter::speech_adapter::{
    AdapterHealth, AdapterMetadata, Guest, HealthStatus,
};
use serde_json::{json, Value};

const NAME: &str = "speech-canary";
const VERSION: &str = "1.0.0";
const WORLD: &str = "speech-adapter-v1";
const FAMILY: &str = "chorus-canary-wire";
const UTTER_PATH: &str = "voicebox/v3/utter";

fn error(code: &str, http_status: u16, message: &str) -> String {
    json!({ "code": code, "http_status": http_status, "message": message }).to_string()
}

fn parse(input: &str, what: &str) -> Result<Value, String> {
    serde_json::from_str(input)
        .map_err(|_| error("internal", 500, &format!("speech canary: {what} is not JSON")))
}

fn serves(config: &Value) -> Result<(), String> {
    if config.get("provider").and_then(Value::as_str) == Some(FAMILY) {
        Ok(())
    } else {
        Err(error("capability", 400, &format!("this package serves the {FAMILY} family")))
    }
}

fn capabilities(config: &Value) -> Value {
    let models = config.get("models").and_then(Value::as_array).cloned().unwrap_or_default();
    Value::Array(
        models
            .iter()
            .filter_map(|model| model.get("model").and_then(Value::as_str))
            .map(|model| json!({ "model": model, "operations": ["synthesize"], "metering_forms": ["characters"] }))
            .collect(),
    )
}

fn prepare(config: &Value, view: &Value, context: &Value) -> Result<Value, String> {
    serves(config)?;
    if context.get("operation").and_then(Value::as_str) != Some("synthesize") {
        return Err(error("capability", 400, "the speech canary only synthesizes"));
    }
    let Some(request) = view.get("json") else {
        return Err(error("capability", 400, "the speech canary only reads a JSON request"));
    };
    let input = request
        .get("input")
        .and_then(Value::as_str)
        .filter(|input| !input.is_empty())
        .ok_or_else(|| error("invalid_request", 400, "Missing 'input' field"))?;
    match request.get("response_format") {
        None | Some(Value::Null) => {}
        Some(Value::String(format)) if format == "wav" => {}
        Some(_) => {
            return Err(error("invalid_request", 400, "the speech canary only answers 'wav'"))
        }
    }
    let timbre = request.get("voice").and_then(Value::as_str).unwrap_or("plain");
    let engine = context.get("upstream_model").and_then(Value::as_str).ok_or_else(|| {
        error("internal", 500, "speech canary: the call context has no upstream model")
    })?;
    let required: Vec<&str> = context
        .get("metering_required")
        .and_then(Value::as_array)
        .map(|forms| forms.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if required.iter().any(|form| *form != "characters") {
        return Err(error("capability", 400, "the speech canary meters characters only"));
    }
    let characters = input.chars().count();
    let auth = config
        .get("auth")
        .and_then(Value::as_str)
        .map(|slot| json!({ "arm": "bearer", "slot": slot }));
    Ok(json!({
        "descriptor": {
            "method": "POST",
            "path": UTTER_PATH,
            "auth": auth,
            "body": { "json": { "template": { "script": input, "timbre": timbre, "engine": engine } } }
        },
        "facts": {
            "operation": "synthesize",
            "characters": { "count": characters, "upper": characters },
            "metering_forms": ["characters"],
            "output_media_type": "audio/wav"
        },
        "response_body_form": "binary",
        "response_elision_paths": [],
        "immutable_body_paths": ["script", "engine"],
        "state": { "characters": characters }
    }))
}

fn parse_response(state: &Value, response: &Value) -> Value {
    let status = response.get("status").and_then(Value::as_u64).unwrap_or(0);
    match status {
        200..=299 => match state.get("characters").and_then(Value::as_u64) {
            Some(characters) => json!({
                "outcome": "succeeded",
                "artifact": { "form": "body", "media_type": "audio/wav" },
                "metering": {
                    "characters": { "value": characters, "source": "request" },
                    "seconds": null,
                    "tokens": null
                }
            }),
            None => {
                json!({ "outcome": "unknown", "reason": "speech canary: the prepared state lost its count" })
            }
        },
        400..=499 => json!({
            "outcome": "rejected",
            "error": {
                "code": "invalid_request",
                "http_status": status,
                "message": "the upstream refused the utterance",
                "provider_message": response.pointer("/body/json/complaint").and_then(Value::as_str).unwrap_or("")
            }
        }),
        _ => json!({ "outcome": "unknown", "reason": format!("speech canary: http {status}") }),
    }
}

struct SpeechCanary;

impl Guest for SpeechCanary {
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

    fn model_capabilities(provider_config: String) -> Result<String, String> {
        let config = parse(&provider_config, "the provider config")?;
        serves(&config)?;
        Ok(capabilities(&config).to_string())
    }

    fn prepare(
        provider_config: String,
        request: String,
        context: String,
    ) -> Result<String, String> {
        let config = parse(&provider_config, "the provider config")?;
        let view = parse(&request, "the request view")?;
        let context = parse(&context, "the call context")?;
        prepare(&config, &view, &context).map(|prepared| prepared.to_string())
    }

    fn parse_response(state: String, response: String) -> Result<String, String> {
        let state = parse(&state, "the prepared state")?;
        Ok(parse_response(&state, &parse(&response, "the response view")?).to_string())
    }
}

export!(SpeechCanary);
