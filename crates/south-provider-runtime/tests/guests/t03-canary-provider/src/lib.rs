//! A provider component for a wire no host knows: `t03-canary-wire`.
//!
//! It exists for an adopting host's "unknown dialect" acceptance. The host is
//! built once and its binary digest recorded; afterwards only this package and
//! a catalog row are added. If the exchange below completes, the host served a
//! dialect that is absent from its own enums.
//!
//! # Why the wire is deliberately unlike the three known ones
//!
//! The request carries `t03_payload` with `turns`, and no top-level `model` or
//! `messages`. A standard OpenAI- or Anthropic-shaped upstream therefore
//! rejects it, which is half of the acceptance: the controlled upstream must be
//! unable to serve a request that any incumbent translator could have built.
//! The response is `t03_result`, which no incumbent parser understands.
//!
//! # The one field that is not free
//!
//! The body must carry the caller's output cap at `max_tokens`, top level.
//! Adopting hosts bind an authorized cost ceiling to the outbound bytes and
//! admit only a closed set of cap paths; a wire that invented its own cap field
//! would be refused at that seal, and the refusal would look like a component
//! failure rather than the policy it is. The cap value is copied verbatim from
//! `sampling.max_output_tokens` — changing it is what the seal is there to
//! catch.
//!
//! # Streaming
//!
//! Frames are `t03-event: <json>\n\n`, not `data: `. A fragment may end
//! mid-frame — the acceptance splits one event across two TCP writes — so the
//! unparsed tail is held in instance state until it completes.

use std::sync::Mutex;

// The path names the single file, not the `wit/` directory: that directory
// resolves more than one package, and a directory path would make the
// generated module layout depend on which packages sit beside this one.
wit_bindgen::generate!({
    path: "../../../../south-provider-api/wit/provider-adapter.wit",
    world: "provider-adapter-v2",
});

use exports::token_station::adapter::provider_adapter::{AdapterHealth, AdapterMetadata, Guest};
use serde_json::{Value, json};
use token_station::adapter::common::HealthStatus;

/// The unparsed tail of the stream this instance is holding. Instance state on
/// purpose: the host promises one component instance per stream.
static STREAM_BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// The frame prefix. Nothing else in the exchange is SSE-shaped, so a host that
/// quietly fell back to a built-in SSE parser would produce no events at all
/// rather than plausible ones.
const FRAME_PREFIX: &str = "t03-event: ";

struct T03Canary;

fn error_envelope(code: &str, http_status: u16, message: &str) -> String {
    json!({ "code": code, "http_status": http_status, "message": message }).to_string()
}

fn protocol_error(message: &str) -> String {
    error_envelope("provider_protocol_error", 502, message)
}

fn parse(input: &str) -> Result<Value, String> {
    serde_json::from_str(input)
        .map_err(|error| error_envelope("internal", 500, &format!("input is not JSON: {error}")))
}

/// The assistant text of one turn, whatever shape the IR used for it.
///
/// `Content` is a bare string for plain text and an array of parts otherwise;
/// both reach a component, so both are read here rather than assuming the
/// simple one.
fn turn_text(message: &Value) -> String {
    match &message["content"] {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

impl Guest for T03Canary {
    fn metadata() -> AdapterMetadata {
        // Must equal this package's manifest, or gate ① refuses the component:
        // a package whose report disagrees with its declaration has been
        // repackaged around its vetting.
        AdapterMetadata {
            name: "t03-canary-provider".to_owned(),
            version: "1.0.0".to_owned(),
            api_version: "provider-adapter-v2".to_owned(),
        }
    }

    fn healthcheck() -> AdapterHealth {
        AdapterHealth { status: HealthStatus::Ready, detail: None }
    }

    fn model_capabilities(provider_config: String) -> Result<String, String> {
        let config = parse(&provider_config)?;
        // A component has no network, so the operator's declaration is all
        // there is to report.
        Ok(config.get("models").cloned().unwrap_or_else(|| json!([])).to_string())
    }

    fn build_http_request(chat_request: String, provider_config: String) -> Result<String, String> {
        let request = parse(&chat_request)?;
        let config = parse(&provider_config)?;

        let base_url = config
            .get("base_url")
            .and_then(Value::as_str)
            .ok_or_else(|| error_envelope("internal", 500, "config has no base_url"))?;

        // The cap the host authorized. Absent means the host did not bound this
        // request, and this wire has no unbounded form: refusing here is louder
        // than sending a request the seal will reject.
        let cap = request["sampling"]["max_output_tokens"]
            .as_u64()
            .ok_or_else(|| {
                error_envelope(
                    "capability",
                    400,
                    "t03-canary-wire requires sampling.max_output_tokens",
                )
            })?;

        let turns: Vec<Value> = request["messages"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|message| {
                json!({
                    "who": message["role"],
                    "said": turn_text(message),
                })
            })
            .collect();

        let mut descriptor = json!({
            "method": "POST",
            // A path this host's own builder never produces, under the
            // configured endpoint so the host's authorization admits it.
            "url": format!("{base_url}/t03/infer"),
            "headers": {
                "content-type": "application/json",
                "x-t03-wire": "1",
            },
            "body": {
                "t03_payload": {
                    "served_model": request["model"],
                    "turns": turns,
                    "want_stream": request["stream"],
                },
                // Top level and named `max_tokens`: see the module header.
                "max_tokens": cap,
            },
        });

        if let Some(secret) = config.get("auth").and_then(Value::as_str) {
            descriptor["auth"] = json!({ "scheme": "bearer", "secret": secret });
        }

        Ok(descriptor.to_string())
    }

    fn parse_response(response_parts: String) -> Result<String, String> {
        let parts = parse(&response_parts)?;
        let body: Value = serde_json::from_str(parts["body"].as_str().unwrap_or(""))
            .map_err(|error| protocol_error(&format!("body is not JSON: {error}")))?;

        // Only this wire carries `t03_result`. Refusing without it is what makes
        // "the host really used this component" observable: an incumbent parser
        // reading the same body would answer, and this one does not.
        let result = body
            .get("t03_result")
            .ok_or_else(|| protocol_error("the upstream 2xx response has no t03_result"))?;

        let meter = &result["meter"];
        let (input, output) = (meter["in"].as_u64(), meter["out"].as_u64());
        // Usage is funds evidence: a 2xx that cannot yield an exact count is an
        // error, never a zero.
        let (input, output) = match (input, output) {
            (Some(input), Some(output)) => (input, output),
            _ => return Err(protocol_error("t03_result.meter lacks exact in/out counts")),
        };

        Ok(json!({
            "id": result["ticket"],
            "model": result["served_model"],
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": result["answer"],
                },
                "finish_reason": "stop",
            }],
            "usage": { "input_tokens": input, "output_tokens": output },
        })
        .to_string())
    }

    fn parse_stream_chunk(chunk: Vec<u8>) -> Result<String, String> {
        let mut buffer = STREAM_BUFFER.lock().expect("single-threaded guest");
        buffer.extend_from_slice(&chunk);

        let mut events = Vec::new();
        while let Some(end) = buffer.windows(2).position(|window| window == b"\n\n") {
            let frame: Vec<u8> = buffer.drain(..end + 2).collect();
            let frame = String::from_utf8_lossy(&frame);
            let Some(payload) = frame.trim_end().strip_prefix(FRAME_PREFIX) else {
                // A frame in some other shape is not this wire. Refuse rather
                // than skip: a silently dropped frame is how a stream ends up
                // short without anyone noticing.
                return Err(protocol_error("stream frame is not a t03-event"));
            };
            let parsed: Value = serde_json::from_str(payload)
                .map_err(|error| protocol_error(&format!("t03-event is not JSON: {error}")))?;

            match parsed["kind"].as_str() {
                Some("say") => events.push(json!({
                    "type": "delta",
                    "index": 0,
                    "content": parsed["text"].as_str().unwrap_or_default(),
                })),
                Some("meter") => {
                    let (input, output) = (parsed["in"].as_u64(), parsed["out"].as_u64());
                    let (Some(input), Some(output)) = (input, output) else {
                        return Err(protocol_error("meter frame lacks exact in/out counts"));
                    };
                    events.push(json!({
                        "type": "usage",
                        "usage": { "input_tokens": input, "output_tokens": output },
                    }));
                }
                Some("end") => events.push(json!({
                    "type": "done",
                    "finish_reason": "stop",
                })),
                _ => return Err(protocol_error("unknown t03-event kind")),
            }
        }
        Ok(Value::Array(events).to_string())
    }

    fn map_provider_error(response_parts: String) -> Result<String, String> {
        let parts = parse(&response_parts)?;
        let status = parts["status"].as_u64().unwrap_or(500);
        let (code, message) = match status {
            401 | 403 => ("auth", "the upstream rejected the credential"),
            429 => ("rate_limit", "the upstream rate limited this request"),
            400..=499 => ("capability", "the upstream refused the request"),
            _ => ("upstream_unavailable", "the upstream failed"),
        };
        Ok(error_envelope(code, u16::try_from(status).unwrap_or(500), message))
    }
}

export!(T03Canary);
