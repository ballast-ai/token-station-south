//! A task component for a family no host knows: `t09-canary-wire`.
//!
//! It exists for an adopting host's "unknown task family" acceptance (T09.7).
//! The host is built once and its binary digest recorded; afterwards only this
//! package, a catalog row and a price are added. If a job submitted through it
//! reaches a terminal state — across a controlled restart, settled exactly
//! once — the host ran a task family that is absent from its own enums.
//!
//! # Why the wire is deliberately unlike the known families
//!
//! Submission posts `t09_job` to `<base>/t09/render-jobs`; the upstream answers
//! with `t09_ticket.handle`; observation reads `t09_state`, `t09_reels` and
//! `t09_billed_seconds`. None of MiniMax, Bailian or Kling spells any of these,
//! so a host that quietly routed the job through a built-in translator sends a
//! request the controlled upstream refuses, and reads a response it cannot
//! parse — the job never reaches a terminal state by accident.
//!
//! # Deliberately inside the capability contract
//!
//! The acceptance proves that a family *within* the task-adapter-v2 contract
//! needs no host rebuild, not that any family can be served. So this wire only
//! uses what the ABI already expresses: one submit, polling, artifact URLs in
//! the observation itself (no `artifact_fetch`), and the task contract 6 facts
//! (see "Contract 6 facts" below).
//!
//! # Input
//!
//! The task request is the host's normalized request, opaque to the ABI. This
//! component reads `prompt` (required), `model`, `duration` (optional,
//! seconds, default 4), `n` (optional output count) and `image` (optional
//! input image, forwarded verbatim) and ignores everything else.
//!
//! # Rogue modes, keyed by the routed model name
//!
//! The acceptance also needs negative evidence (T09.8): a component whose
//! pricing basis the host cannot honour must be refused before any money moves,
//! not quietly priced some other way. The host hands this component only the
//! routed upstream model, so — as in `t03-canary-provider` — the rogue
//! behaviour is keyed by it. The sentinels are deliberately not real model
//! names:
//!
//! - `rogue-milliunits` — prices in milliunits per second (a basis the ABI
//!   expresses but a host may not bill).
//! - `rogue-no-seconds` — reports no requested seconds at all.
//!
//! Any other model name is the well-behaved wire above.
//!
//! # Capability modes (host capabilities beyond bearer and polling)
//!
//! Positive evidence for host capabilities added after T09 (token-station-server
//! P13), each switched on by something the host already hands over:
//!
//! - model `header-auth` — every descriptor (submit and observe) asks for the
//!   credential in the sanctioned `x-goog-api-key` header instead of a bearer
//!   token. Proves a host serving a `header_secret` auth recipe sends the key
//!   where the component asked and nowhere else.
//! - a submit response that is already terminal (`t09_state: "done"` with reels,
//!   no ticket) — the component answers `accepted-terminal` with that
//!   observation. Proves a host settles an upstream that finishes at submit,
//!   instead of holding the reservation for a job it can never poll.
//!
//! # Contract 6 facts (token-station-server P13 DoD #3)
//!
//! Each task contract 6 fact has a switch here, so a host can prove it honours
//! the fact without a rebuild. Wire-driven facts are read from the request or
//! the upstream and stay null when absent — absent and zero are different
//! facts, and the existing scenarios, which send none of them, keep their
//! contract 5 shape:
//!
//! - request `n` → `request_estimate.requested_outputs` (count settlement; an
//!   image-modality host prices per image by it).
//! - upstream `t09_billed_outputs` → `usage.outputs`; `t09_billed_tokens` →
//!   `usage.tokens`.
//! - a reel with `"sealed": true` → `fetch_with_credential: true`: the host
//!   must fetch it with the task's pinned credential and never hand the raw
//!   URL to the client.
//! - a reel's `"role"` is copied onto the artifact verbatim (task contract 7;
//!   absent → `null`, the primary role). A `"last_frame"` reel is rendered
//!   into `data[0].last_frame_url` of the reel before it and never as its own
//!   `data[]` entry: the host must leave it out of its output count, delivery
//!   envelope and storage. Any other word is for the host's codec to refuse.
//! - request `image` → `t09_job.still`, verbatim. A host that prefetches input
//!   images hands the component a `data:` URI, and the upstream sees it.
//!
//! Model-keyed, because the estimate and the body layout are statements the
//! component makes, not facts the wire reports:
//!
//! - model `token-rate` — `tokens_per_second` is [`TOKEN_RATE`] (token
//!   metering).
//! - model `pinned-body` — declares `t09_job.reel_seconds` and
//!   `t09_job.reel_model` immutable (every other model declares nothing,
//!   `null`), so a host may inject request extras anywhere else.

wit_bindgen::generate!({
    path: "../../../../south-provider-api/wit/task-adapter-v2.wit",
    world: "task-adapter-v2",
});

use exports::token_station::task_adapter::task_adapter::{
    AdapterHealth, AdapterMetadata, Guest, HealthStatus,
};
use serde_json::{json, Value};

const NAME: &str = "t09-canary-task";
const VERSION: &str = "1.0.0";
const API_VERSION: &str = "task-adapter-v2";

/// The locator route, relative to `base_url`. Recovery reads it back from the
/// saved locator, so observation never re-derives it from the model name.
const ROUTE: &str = "t09/render-jobs";
const DEFAULT_SECONDS: f64 = 4.0;
const MAX_REELS: usize = 16;

/// The upstream model that reports a token estimate, and its rate.
const TOKEN_RATE_MODEL: &str = "token-rate";
const TOKEN_RATE: i64 = 1_000;
/// The upstream model that declares immutable body paths, and the paths: the
/// fields the estimate is derived from.
const PINNED_BODY_MODEL: &str = "pinned-body";
const PINNED_BODY_PATHS: [&str; 2] = ["t09_job.reel_seconds", "t09_job.reel_model"];

struct T09Canary;

fn error_envelope(code: &str, http_status: u16, message: &str) -> String {
    json!({ "code": code, "http_status": http_status, "message": message }).to_string()
}

fn parse_json(raw: &str, what: &str) -> Result<Value, String> {
    serde_json::from_str(raw)
        .map_err(|_| error_envelope("internal", 500, &format!("t09 canary: {what} is not JSON")))
}

/// `base_url` and the credential slot, as the host handed them over. Unknown
/// and flattened extension keys are tolerated on purpose.
fn endpoint(config: &Value) -> Result<(String, Option<String>), String> {
    let base =
        config.get("base_url").and_then(Value::as_str).filter(|base| !base.is_empty()).ok_or_else(
            || error_envelope("internal", 500, "t09 canary: provider config has no base_url"),
        )?;
    let slot = config.get("auth").and_then(Value::as_str).map(str::to_owned);
    Ok((base.trim_end_matches('/').to_owned(), slot))
}

/// The upstream model that switches every descriptor to header auth.
const HEADER_AUTH_MODEL: &str = "header-auth";
const HEADER_AUTH_NAME: &str = "x-goog-api-key";

/// The slot is copied from the config, never invented: the host authorizes the
/// descriptor against exactly the slot it granted.
fn auth(slot: &Option<String>, model: &str) -> Value {
    match slot {
        Some(secret) if model == HEADER_AUTH_MODEL => {
            json!({ "scheme": "header", "name": HEADER_AUTH_NAME, "secret": secret })
        }
        Some(secret) => json!({ "scheme": "bearer", "secret": secret }),
        None => Value::Null,
    }
}

fn descriptor(
    method: &str,
    url: String,
    body: Option<Value>,
    slot: &Option<String>,
    model: &str,
) -> Value {
    let mut out = json!({ "method": method, "url": url });
    if let Some(body) = body {
        out["headers"] = json!({ "content-type": "application/json" });
        out["body"] = body;
    }
    let auth = auth(slot, model);
    if !auth.is_null() {
        out["auth"] = auth;
    }
    out
}

/// One path segment: unreserved bytes pass, everything else is `%XX`. Empty,
/// `.` and `..` are refused rather than encoded — they would change the path.
fn path_segment(raw: &str) -> Option<String> {
    if raw.is_empty() || raw == "." || raw == ".." || raw.bytes().any(|b| b.is_ascii_control()) {
        return None;
    }
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    Some(out)
}

fn seconds(value: &Value) -> Option<f64> {
    let seconds = match value {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    (seconds.is_finite() && seconds >= 0.0).then_some(seconds)
}

fn response(parts: &str) -> Result<(u16, Option<Value>), String> {
    let parts = parse_json(parts, "response parts")?;
    let status = parts
        .get("status")
        .and_then(Value::as_u64)
        .and_then(|s| u16::try_from(s).ok())
        .ok_or_else(|| {
            error_envelope("internal", 500, "t09 canary: response parts have no status")
        })?;
    let body = parts.get("body").and_then(Value::as_str).and_then(|b| serde_json::from_str(b).ok());
    Ok((status, body))
}

/// A t09 job body (poll response, or a submit response that finished at once)
/// → task observation JSON.
fn observation_of(body: &Value) -> Value {
    match body.get("t09_state").and_then(Value::as_str) {
        Some("queued") => {
            json!({ "state": "progress", "running": false, "status_word": "queued" })
        }
        Some("brewing") => {
            json!({ "state": "progress", "running": true, "status_word": "brewing" })
        }
        Some("done") => {
            let reels: Vec<Value> = body
                .get("t09_reels")
                .and_then(Value::as_array)
                .map(|reels| {
                    reels
                        .iter()
                        .filter_map(|reel| {
                            let href = reel.get("href").and_then(Value::as_str)?;
                            let sealed = reel.get("sealed").and_then(Value::as_bool) == Some(true);
                            let role = reel.get("role").cloned().unwrap_or(Value::Null);
                            Some((href, sealed, role))
                        })
                        .filter(|(href, _, _)| !href.is_empty())
                        .map(|(href, sealed, role)| json!({ "url": href, "id": null, "duration": null, "fetch_with_credential": sealed, "role": role }))
                        .collect()
                })
                .unwrap_or_default();
            if reels.is_empty() || reels.len() > MAX_REELS {
                return json!({ "state": "unknown", "reason": "t09 job done without usable reels" });
            }
            // Absent and zero are different facts: a missing meter stays null.
            let billed = body.get("t09_billed_seconds").and_then(seconds);
            let tokens = body.get("t09_billed_tokens").and_then(Value::as_i64).filter(|t| *t >= 0);
            let outputs = body.get("t09_billed_outputs").and_then(Value::as_u64);
            json!({
                "state": "succeeded",
                "artifacts": { "kind": "urls", "items": reels },
                "usage": { "seconds": billed, "milliunits": null, "tokens": tokens, "outputs": outputs },
            })
        }
        Some("spoiled") => json!({
            "state": "failed",
            "kind": "failed",
            "code": body.pointer("/t09_fault/code").and_then(Value::as_str),
            "message": body.pointer("/t09_fault/reason").and_then(Value::as_str),
        }),
        _ => json!({ "state": "unknown", "reason": "unrecognized t09_state" }),
    }
}

impl Guest for T09Canary {
    fn metadata() -> AdapterMetadata {
        AdapterMetadata {
            name: NAME.to_owned(),
            version: VERSION.to_owned(),
            api_version: API_VERSION.to_owned(),
        }
    }

    fn healthcheck() -> AdapterHealth {
        AdapterHealth { status: HealthStatus::Ready, detail: None }
    }

    fn build_submit_request(
        config: String,
        request: String,
        minted: String,
    ) -> Result<String, String> {
        let (base, slot) = endpoint(&parse_json(&config, "provider config")?)?;
        let request = parse_json(&request, "task request")?;
        let minted = parse_json(&minted, "host-minted values")?;

        let prompt = request
            .get("prompt")
            .and_then(Value::as_str)
            .filter(|p| !p.trim().is_empty())
            .ok_or_else(|| {
                error_envelope("invalid_request", 400, "t09 canary: prompt is required")
            })?;
        let model = request.get("model").and_then(Value::as_str).unwrap_or_default();
        let reel_seconds = match request.get("duration").filter(|d| !d.is_null()) {
            Some(raw) => seconds(raw).ok_or_else(|| {
                error_envelope(
                    "invalid_request",
                    400,
                    "t09 canary: duration must be non-negative seconds",
                )
            })?,
            None => DEFAULT_SECONDS,
        };
        let requested_outputs = match request.get("n").filter(|n| !n.is_null()) {
            Some(raw) => {
                let n = raw.as_u64().filter(|n| (1..=MAX_REELS as u64).contains(n)).ok_or_else(
                    || {
                        error_envelope(
                            "invalid_request",
                            400,
                            "t09 canary: n must be an integer between 1 and 16",
                        )
                    },
                )?;
                json!(n)
            }
            None => Value::Null,
        };
        let still = match request.get("image").filter(|i| !i.is_null()) {
            Some(Value::String(image)) => Some(image.clone()),
            Some(_) => {
                return Err(error_envelope(
                    "invalid_request",
                    400,
                    "t09 canary: image must be a string",
                ))
            }
            None => None,
        };
        let ticket = minted.get("task_id").and_then(Value::as_str).ok_or_else(|| {
            error_envelope("internal", 500, "t09 canary: host-minted task_id missing")
        })?;

        // Rogue modes (see the module header): the estimate a host must refuse.
        let (requested_seconds, milliunits_per_second) = match model {
            "rogue-milliunits" => (json!(reel_seconds), json!(1000)),
            "rogue-no-seconds" => (Value::Null, Value::Null),
            _ => (json!(reel_seconds), Value::Null),
        };

        let tokens_per_second =
            if model == TOKEN_RATE_MODEL { json!(TOKEN_RATE) } else { Value::Null };
        let immutable_body_paths =
            if model == PINNED_BODY_MODEL { json!(PINNED_BODY_PATHS) } else { Value::Null };

        let mut body = json!({
            "t09_job": {
                "reel_model": model,
                "storyboard": prompt,
                "reel_seconds": reel_seconds,
                "ticket": ticket,
            }
        });
        if let Some(still) = still {
            body["t09_job"]["still"] = json!(still);
        }
        Ok(json!({
            "descriptor": descriptor("POST", format!("{base}/{ROUTE}"), Some(body), &slot, model),
            "locator": { "schema_version": 1, "route": ROUTE },
            // Every key is written, null included: the host's decoder requires every
            // estimate key to be present (task contract 6 adds the token rate and the
            // requested output count).
            "request_estimate": {
                "requested_seconds": requested_seconds,
                "milliunits_per_second": milliunits_per_second,
                "resolution": null,
                "input_image_count": null,
                "tokens_per_second": tokens_per_second,
                "requested_outputs": requested_outputs,
            },
            // Task contract 6: null (no statement) except in the `pinned-body` mode.
            "immutable_body_paths": immutable_body_paths,
        })
        .to_string())
    }

    fn parse_submit_response(parts: String) -> Result<String, String> {
        let (status, body) = response(&parts)?;
        let outcome = match (status, body) {
            (200..=299, Some(body)) => {
                match body.pointer("/t09_ticket/handle").and_then(Value::as_str) {
                    Some(handle) if !handle.is_empty() => {
                        json!({ "outcome": "accepted", "upstream_task_id": handle })
                    }
                    // Finished at submit (see "Capability modes"): terminal
                    // observations only — anything else stays unknown.
                    _ => match observation_of(&body) {
                        observation
                            if matches!(
                                observation.get("state").and_then(Value::as_str),
                                Some("succeeded" | "failed")
                            ) =>
                        {
                            json!({ "outcome": "accepted-terminal", "observation": observation })
                        }
                        _ => json!({ "outcome": "unknown" }),
                    },
                }
            }
            // A 4xx that names this wire's own fault is a definite refusal;
            // anything else could have been accepted, so it stays unknown.
            (400..=499, Some(body)) if body.get("t09_fault").is_some() => {
                let reason =
                    body.pointer("/t09_fault/reason").and_then(Value::as_str).unwrap_or("refused");
                json!({
                    "outcome": "rejected",
                    "error": { "code": "invalid_request", "http_status": status, "message": format!("t09 upstream refused the job: {reason}") },
                })
            }
            _ => json!({ "outcome": "unknown" }),
        };
        Ok(outcome.to_string())
    }

    fn build_observe_request(
        config: String,
        upstream_model: String,
        upstream_task_id: String,
        locator: String,
    ) -> Result<String, String> {
        let (base, slot) = endpoint(&parse_json(&config, "provider config")?)?;
        let locator = parse_json(&locator, "locator")?;
        let route = locator
            .get("route")
            .and_then(Value::as_str)
            .ok_or_else(|| error_envelope("internal", 500, "t09 canary: locator has no route"))?;
        let id = path_segment(&upstream_task_id).ok_or_else(|| {
            error_envelope("internal", 500, "t09 canary: upstream task id is not a path segment")
        })?;
        Ok(descriptor("GET", format!("{base}/{route}/{id}"), None, &slot, &upstream_model)
            .to_string())
    }

    fn parse_observation(parts: String) -> Result<String, String> {
        let (status, body) = response(&parts)?;
        let body = match (status, body) {
            (200..=299, Some(body)) => body,
            _ => {
                return Ok(
                    json!({ "state": "unknown", "reason": format!("http {status}") }).to_string()
                )
            }
        };
        Ok(observation_of(&body).to_string())
    }

    fn build_artifact_request(
        _config: String,
        _locator: String,
        _observation: String,
    ) -> Result<String, String> {
        // The reels arrive in the observation; this wire never fetches twice.
        Ok("null".to_owned())
    }

    fn render_success(
        observation: String,
        _fetched: Option<String>,
        context: String,
    ) -> Result<String, String> {
        let observation = parse_json(&observation, "observation")?;
        let context = parse_json(&context, "render context")?;
        let mut data: Vec<Value> = Vec::new();
        for item in observation.pointer("/artifacts/items").and_then(Value::as_array).into_iter().flatten() {
            let Some(url) = item.get("url") else { continue };
            // Contract 7: a last frame decorates the reel before it instead of becoming a reel.
            if item.get("role").and_then(Value::as_str) == Some("last_frame") {
                let Some(reel) = data.last_mut() else {
                    return Err(error_envelope("internal", 500, "t09 canary: last frame before any reel"));
                };
                reel["last_frame_url"] = url.clone();
            } else {
                data.push(json!({ "url": url }));
            }
        }
        if data.is_empty() {
            return Err(error_envelope("internal", 500, "t09 canary: nothing to render"));
        }
        let task_id = context
            .get("upstream_task_id")
            .filter(|v| !v.is_null())
            .or_else(|| context.get("task_id"));
        Ok(json!({
            "created": context.get("created"),
            "model": context.get("model"),
            "provider": context.get("provider"),
            "task_id": task_id,
            "data": data,
        })
        .to_string())
    }

    fn map_terminal_failure(observation: String) -> Result<String, String> {
        let observation = parse_json(&observation, "observation")?;
        let reason =
            observation.get("message").and_then(Value::as_str).unwrap_or("no reason given");
        Ok(error_envelope(
            "upstream_unavailable",
            502,
            &format!("t09 render job spoiled: {reason}"),
        ))
    }
}

export!(T09Canary);
