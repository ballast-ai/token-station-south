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
//! # Three fields that are not free
//!
//! An adopting host seals the outbound bytes against what it authorized, so a
//! custom wire is free everywhere except where that seal looks. Measured
//! against a real host rather than assumed:
//!
//! 1. **`model`, top level, equal to the routed upstream model.** The host
//!    authorized one model; if the identity travels in the body it checks for
//!    it there. The IR's `model` already carries the routed name — the host
//!    overwrites it before the component is called — so it is copied verbatim.
//! 2. **The output cap at top-level `max_tokens`.** Hosts admit only a closed
//!    set of cap paths, and for a chat-shaped operation that set is
//!    `max_completion_tokens` or `max_tokens`. A wire that invented its own cap
//!    field is refused at the seal, and the refusal reads as a component fault
//!    rather than the policy it is. Copied verbatim from
//!    `sampling.max_output_tokens`.
//! 3. **`stream`, top level, agreeing with the request.** The host seals the
//!    streaming mode too; a body that disagrees with it is refused.
//!
//! Everything else — the path, the header, the payload shape, the response
//! shape — is this wire's own, which is what makes it a canary.
//!
//! # Streaming
//!
//! Two frame shapes carry the same three event kinds (`say`, `meter`, `end`):
//!
//! - `t03-event: <json>\n\n` — this wire's own line, not SSE at all. A host
//!   that quietly fell back to a built-in SSE parser gets no `data:` payload
//!   and therefore no events, rather than plausible ones.
//! - Standard SSE: `event: t03\ndata: <json>\n\n` (multiple `data:` lines join
//!   with `\n` as the spec says; `id:`/`retry:`/comment lines are ignored).
//!   This shape exists for the adopting host's R03 acceptance: a `data:` line
//!   whose JSON no incumbent parser understands must still reach *this*
//!   component, not the host's OpenAI/Anthropic/Gemini usage reader. The
//!   event name must be `t03`; any other name, or a `data:` frame without an
//!   event line, is refused as not this wire.
//!
//! Anything else is refused rather than skipped: a silently dropped frame is
//! how a stream ends up short without anyone noticing. A fragment may end
//! mid-frame — the acceptance splits one event across two TCP writes — so the
//! unparsed tail is held in instance state until it completes.
//!
//! # Rogue modes, keyed by the routed model name
//!
//! The acceptance also demands negative evidence: a component that declares
//! something the host's boundary must refuse — an out-of-bounds URL, a reserved
//! header, a credential slot it was never granted, a method the surface does
//! not serve — must produce **zero upstream calls**, and the refusal must not
//! be quietly replaced by a host-built request. None of that can be induced
//! from a catalog row alone: the host hands this component only `{provider,
//! base_url}`, and `base_url` is the very endpoint it bounds URLs against, so
//! the well-behaved wire is always in bounds.
//!
//! So the rogue behaviour is keyed by the routed upstream model name, which the
//! component already reads. A host test seeds one catalog row per sentinel and
//! asserts on its own side. The sentinels are deliberately not real model names:
//!
//! - `rogue-url`    — URL on a host that is not `base_url` (and not `.invalid`,
//!                    which some hosts special-case as a placeholder).
//! - `rogue-header` — declares `authorization`, a reserved header.
//! - `rogue-secret` — declares an `auth` slot the host never granted.
//! - `rogue-method` — declares `GET` on a POST-only surface.
//!
//! Any other model name is the well-behaved wire above.

use std::sync::Mutex;

// The path names the single file, not the `wit/` directory: that directory
// resolves more than one package, and a directory path would make the
// generated module layout depend on which packages sit beside this one.
wit_bindgen::generate!({
    path: "../../../../south-provider-api/wit/provider-adapter.wit",
    world: "provider-adapter-v2",
});

use exports::token_station::adapter::provider_adapter::{AdapterHealth, AdapterMetadata, Guest};
use serde_json::{json, Value};
use token_station::adapter::common::HealthStatus;

/// The unparsed tail of the stream this instance is holding. Instance state on
/// purpose: the host promises one component instance per stream.
static STREAM_BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// The wire's own frame prefix (see the module header, "Streaming").
const FRAME_PREFIX: &str = "t03-event: ";

/// The SSE event name of the standard-shaped frame. Anything else is not this
/// wire.
const SSE_EVENT_NAME: &str = "t03";

/// The JSON payload of one complete frame, whichever of the two shapes it is
/// in. `None` means the frame is neither — the caller refuses it.
fn frame_payload(frame: &str) -> Option<String> {
    let trimmed = frame.trim_end_matches(['\r', '\n']);
    if let Some(payload) = trimmed.strip_prefix(FRAME_PREFIX) {
        return Some(payload.to_owned());
    }
    // Standard SSE: `event:` names the wire, `data:` lines carry the JSON.
    // Parsed the way the spec reads a frame — field name up to the first
    // colon, one optional leading space stripped from the value — so a host
    // relaying the bytes verbatim and one re-encoding them agree.
    let mut event = None;
    let mut data: Vec<&str> = Vec::new();
    for line in trimmed.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with(':') {
            continue; // comment
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => event = Some(value),
            "data" => data.push(value),
            _ => {} // id / retry / unknown fields: ignored, as the spec says
        }
    }
    (event == Some(SSE_EVENT_NAME) && !data.is_empty()).then(|| data.join("\n"))
}

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
        Value::Array(parts) => {
            parts.iter().filter_map(|part| part["text"].as_str()).collect::<Vec<_>>().join("")
        }
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
        let cap = request["sampling"]["max_output_tokens"].as_u64().ok_or_else(|| {
            error_envelope("capability", 400, "t03-canary-wire requires sampling.max_output_tokens")
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
                },
                // The three fields the host's seal looks for; see the module
                // header. Everything above this line is the canary's own wire.
                "model": request["model"],
                "max_tokens": cap,
                "stream": request["stream"].as_bool().unwrap_or(false),
            },
        });

        if let Some(secret) = config.get("auth").and_then(Value::as_str) {
            descriptor["auth"] = json!({ "scheme": "bearer", "secret": secret });
        }

        // Rogue modes; see the module header. Each one changes exactly one
        // field, so a host refusal points at that field and nothing else.
        match request["model"].as_str() {
            Some("rogue-url") => {
                // Port 9 is `discard`: nothing listens, so a host that wrongly
                // sent here would fail fast instead of reaching anything real.
                descriptor["url"] = json!("http://127.0.0.1:9/t03/infer");
            }
            Some("rogue-header") => {
                descriptor["headers"]["authorization"] = json!("Bearer rogue");
            }
            Some("rogue-secret") => {
                descriptor["auth"] = json!({ "scheme": "bearer", "secret": "slot-never-granted" });
            }
            Some("rogue-method") => {
                descriptor["method"] = json!("GET");
            }
            _ => {}
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
            let Some(payload) = frame_payload(&frame) else {
                // A frame in some other shape is not this wire. Refuse rather
                // than skip: a silently dropped frame is how a stream ends up
                // short without anyone noticing.
                return Err(protocol_error("stream frame is not a t03-event"));
            };
            let parsed: Value = serde_json::from_str(&payload)
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

#[cfg(test)]
mod tests {
    use super::frame_payload;

    #[test]
    fn the_wires_own_prefix_still_yields_the_payload() {
        assert_eq!(
            frame_payload("t03-event: {\"kind\":\"end\"}\n\n").as_deref(),
            Some("{\"kind\":\"end\"}")
        );
    }

    #[test]
    fn a_standard_sse_frame_named_t03_yields_its_data_lines_joined() {
        assert_eq!(
            frame_payload("event: t03\ndata: {\"kind\":\"say\",\ndata: \"text\":\"hi\"}\n\n")
                .as_deref(),
            Some("{\"kind\":\"say\",\n\"text\":\"hi\"}")
        );
        assert_eq!(
            frame_payload(": keepalive\r\nid: 7\r\nevent: t03\r\ndata:{\"kind\":\"end\"}\r\n\r\n")
                .as_deref(),
            Some("{\"kind\":\"end\"}"),
            "comments, ids and CRLF line ends are tolerated; a missing space after the colon too"
        );
    }

    #[test]
    fn frames_that_are_not_this_wire_are_refused() {
        assert_eq!(frame_payload("data: {\"kind\":\"end\"}\n\n"), None, "no event name");
        assert_eq!(
            frame_payload("event: message\ndata: {\"kind\":\"end\"}\n\n"),
            None,
            "another event name"
        );
        assert_eq!(frame_payload("event: t03\n\n"), None, "no data at all");
        assert_eq!(frame_payload("data: [DONE]\n\n"), None, "an OpenAI sentinel is not this wire");
    }
}
