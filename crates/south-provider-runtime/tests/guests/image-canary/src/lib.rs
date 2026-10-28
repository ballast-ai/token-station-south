//! An image component for a family no host knows: `pigment-canary-wire`.
//!
//! It exists for an adopting host's "unknown image family" acceptance, the
//! image-world sibling of `t09-canary-task`. The host is built once and its
//! binary digest recorded; afterwards only this package, a binding, a catalog
//! row with an image route and a price are added. If a generation request
//! comes back with the upstream's pixels and settles at the per-image price,
//! the host served an image family that is absent from its own code.
//!
//! # Why the wire is deliberately unlike the known families
//!
//! Generation posts `{"brief", "variant", "count"}` to `<base>/canvas/v7/paint`;
//! the upstream answers `{"canvas": {"layers": [<base64>...]}, "tally":
//! {"painted": n}}`. No released image family spells any of these, so a host
//! that quietly routed the call through anything it already knows sends a
//! request the controlled upstream refuses and reads a response it cannot
//! parse.
//!
//! # Deliberately inside the image contract
//!
//! The acceptance proves that a family *within* image contract 1 needs no host
//! rebuild, not that any family can be served. So this wire only uses what the
//! contract already expresses: one JSON round per call, a bearer credential,
//! inline base64 artifacts and the `images` metering form.
//!
//! # Input
//!
//! A JSON request view: `prompt` (required string) and `n` (optional whole
//! number from 1 to 4, default 1). Everything else is ignored. A multipart
//! view (the edit surface) is refused: this family only generates.
//!
//! # Response
//!
//! Read from the view's JSON body (`{"status", "headers", "body": {"json": ...}}`).
//!
//! - 2xx: every entry of `canvas.layers` is one inline base64 PNG artifact;
//!   `tally.painted` (required whole number) is `images_reported`.
//! - 4xx: `rejected` with the upstream's `complaint` as the provider message.
//! - anything else: `unknown`.

wit_bindgen::generate!({
    path: "../../../../south-provider-api/wit/image-adapter.wit",
    world: "image-adapter-v1",
});

use exports::token_station::image_adapter::image_adapter::{
    AdapterHealth, AdapterMetadata, Guest, HealthStatus,
};
use serde_json::{json, Value};

const NAME: &str = "image-canary";
const VERSION: &str = "1.0.0";
const WORLD: &str = "image-adapter-v1";
const FAMILY: &str = "pigment-canary-wire";
const PAINT_PATH: &str = "canvas/v7/paint";
const LAYERS: &str = "/canvas/layers";
const MAX_COUNT: u64 = 4;

fn error(code: &str, http_status: u16, message: &str) -> String {
    json!({ "code": code, "http_status": http_status, "message": message }).to_string()
}

fn parse(input: &str, what: &str) -> Result<Value, String> {
    serde_json::from_str(input)
        .map_err(|_| error("internal", 500, &format!("image canary: {what} is not JSON")))
}

/// Refuses a provider configuration that names another family.
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
            .map(|model| {
                json!({
                    "model": model,
                    "operations": ["generate"],
                    "metering_forms": ["images"],
                    "token_buckets": [],
                    "tier_dimensions": [],
                    "input_roles": [],
                    "request_elision_paths": [],
                    "response_formats": ["b64_json"],
                    "repeat": false
                })
            })
            .collect(),
    )
}

/// The requested count: absent is 1; anything but a whole number from 1 to 4 is refused.
fn count(request: &Value) -> Result<u64, String> {
    match request.get("n") {
        None | Some(Value::Null) => Ok(1),
        Some(value) => value.as_u64().filter(|n| (1..=MAX_COUNT).contains(n)).ok_or_else(|| {
            error(
                "invalid_request",
                400,
                &format!("'n' must be a whole number from 1 to {MAX_COUNT}"),
            )
        }),
    }
}

fn prepare(config: &Value, view: &Value, context: &Value) -> Result<Value, String> {
    serves(config)?;
    let Some(request) = view.get("json") else {
        return Err(error(
            "capability",
            400,
            "the image canary only generates from a JSON request",
        ));
    };
    let prompt = request
        .get("prompt")
        .and_then(Value::as_str)
        .filter(|prompt| !prompt.is_empty())
        .ok_or_else(|| error("invalid_request", 400, "Missing 'prompt' field"))?;
    let count = count(request)?;
    let variant = context.get("upstream_model").and_then(Value::as_str).ok_or_else(|| {
        error("internal", 500, "image canary: the call context has no upstream model")
    })?;
    let required: Vec<&str> = context
        .get("metering_required")
        .and_then(Value::as_array)
        .map(|forms| forms.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if required.iter().any(|form| *form != "images") {
        return Err(error("capability", 400, "the image canary meters images only"));
    }
    let auth = config
        .get("auth")
        .and_then(Value::as_str)
        .map(|slot| json!({ "arm": "bearer", "slot": slot }));
    Ok(json!({
        "descriptor": {
            "method": "POST",
            "path": PAINT_PATH,
            "auth": auth,
            "body": { "json": { "template": { "brief": prompt, "variant": variant, "count": count } } }
        },
        "facts": {
            "operation": "generate",
            "requested_outputs": count,
            "inputs": { "input_image": 0, "mask": 0, "reference_image": 0 },
            "metering_forms": ["images"],
            "tier": {}
        },
        "immutable_body_paths": ["variant", "count"],
        "repeat": 1,
        "response_body_form": "json",
        "response_elision_paths": [format!("{LAYERS}/*")],
        "state": { "count": count }
    }))
}

fn parse_response(response: &Value) -> Value {
    let status = response.get("status").and_then(Value::as_u64).unwrap_or(0);
    // The view's body is `{"json": <elided view>}` under the declared JSON form; a text or opaque
    // body has no layers and no complaint to read.
    let body = response.pointer("/body/json").cloned().unwrap_or(Value::Null);
    match status {
        200..=299 => {
            let layers = body.pointer(LAYERS).and_then(Value::as_array);
            let painted = body.pointer("/tally/painted").and_then(Value::as_u64);
            match (layers, painted) {
                (Some(layers), Some(painted)) if !layers.is_empty() => json!({
                    "outcome": "succeeded",
                    "artifacts": (0..layers.len())
                        .map(|index| json!({
                            "form": "inline",
                            "encoding": "base64",
                            "media_type": "image/png",
                            "pointer": format!("{LAYERS}/{index}")
                        }))
                        .collect::<Vec<_>>(),
                    "metering": {
                        "images_reported": painted,
                        "tokens": null,
                        "credits": null,
                        "upstream_cost": null
                    },
                    "extras": null
                }),
                _ => {
                    json!({ "outcome": "unknown", "reason": "image canary: a 2xx without layers and a tally" })
                }
            }
        }
        400..=499 => json!({
            "outcome": "rejected",
            "error": {
                "code": "invalid_request",
                "http_status": status,
                "message": "the upstream refused the painting",
                "provider_message": body.get("complaint").and_then(Value::as_str).unwrap_or("")
            }
        }),
        _ => json!({ "outcome": "unknown", "reason": format!("image canary: http {status}") }),
    }
}

fn render(outcomes: &Value, context: &Value) -> Value {
    let artifacts: usize = outcomes
        .as_array()
        .map(|rounds| {
            rounds
                .iter()
                .map(|round| round.get("artifacts").and_then(Value::as_array).map_or(0, Vec::len))
                .sum()
        })
        .unwrap_or(0);
    json!({
        "template": {
            "created": context.get("created").cloned().unwrap_or(Value::Null),
            "data": (0..artifacts)
                .map(|index| json!({ "b64_json": { "$south.artifact": { "as": "b64_json", "index": index } } }))
                .collect::<Vec<_>>()
        }
    })
}

struct ImageCanary;

impl Guest for ImageCanary {
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

    fn parse_response(_state: String, response: String) -> Result<String, String> {
        Ok(parse_response(&parse(&response, "the response view")?).to_string())
    }

    fn render(_state: String, outcomes: String, render_context: String) -> Result<String, String> {
        let outcomes = parse(&outcomes, "the outcomes")?;
        let context = parse(&render_context, "the render context")?;
        Ok(render(&outcomes, &context).to_string())
    }
}

export!(ImageCanary);
