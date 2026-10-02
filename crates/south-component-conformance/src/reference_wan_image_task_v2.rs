//! Pure Bailian Wan 2.7 image translation (asynchronous `DashScope` image generation).
//!
//! Transcribed from the host's native Wan image arm (token-station-server `handler/images/bailian.rs`
//! `build_bailian_image_body` / `extract_bailian_image_urls` / `trusted_image_count` /
//! `bailian_image_result_to_openai`, `images/durable.rs::wan_run` / `parse_wan_create`,
//! `images/observe.rs::normalize_bailian_wan`). Credentials, pricing, persistence and time remain
//! host responsibilities.
//!
//! The first image task component. What task contract 6 gives it:
//! - `requested_outputs` is the request's `n` (Wan caps it at 1–4), so a host prices a reservation
//!   per image without reading the request body itself;
//! - `usage.outputs` is the **billed** count, not the delivered one: the upstream's positive
//!   `usage.image_count`, capped at delivered + 1, else the number of delivered URLs — the host's
//!   existing "trust the upstream count" contract, moved here with the parser that reads it.
//!
//! `SUCCEEDED` with no usable image is a terminal failure (`empty_result`), not `unknown`: the
//! upstream has said its last word, and `unknown` would only park the reservation for review.
use crate::{ComponentResultV1, PreparedTaskV2, SubmitOutcomeV2, TaskComponentV2};
use serde_json::{Map, Value, json};
use south_contracts::{
    HostMintedValuesV1, TaskArtifactRefV2, TaskArtifactV2, TaskFailureKindV1, TaskLocatorV2,
    TaskObservationV2, TaskRenderContextV2, TaskRequestEstimateV2, TaskScalarV2, TaskUsageFactsV2,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{
    Auth, ErrorCode, ErrorEnvelope, HttpMethod, HttpRequestDescriptor, HttpResponseParts,
    ProviderConfig, SafeHeaders,
};

const QUERY: &str = "api/v1/tasks";
const SUBMIT: &str = "api/v1/services/aigc/image-generation/generation";
/// Wan 2.7 image generates at most four images per request.
const MAX_IMAGES: u64 = 4;
/// Request-body paths the host must not rewrite (contract 6): the prompt and source images, and
/// the count and size the reservation reads.
const IMMUTABLE_BODY_PATHS: [&str; 4] = ["model", "input", "parameters.n", "parameters.size"];
const PASSTHROUGH: [&str; 5] =
    ["watermark", "bbox_list", "seed", "prompt_extend", "negative_prompt"];

/// Managed Bailian Wan image dialect; no credentials, clock, price or persistence.
#[derive(Debug, Default, Clone, Copy)]
pub struct WanImageTaskComponentV2;

fn invalid(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, message)
}
fn protocol(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message)
}
fn field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}
fn bounded(text: Option<&str>) -> Option<String> {
    text.filter(|t| t.len() <= south_contracts::MAX_ARTIFACT_REF_BYTES).map(str::to_owned)
}
/// A whole, non-negative image count; `2.0` counts as two (some JSON producers write it so).
fn whole_count(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|f| f.is_finite() && *f >= 0.0 && f.fract() == 0.0 && *f < 1e15)
            .and_then(|f| format!("{f:.0}").parse().ok())
    })
}
/// `OpenAI` `WxH` → Wan `W*H`; named tiers and anything else pass through.
fn wan_size(size: &str) -> String {
    let trimmed = size.trim();
    let lower = trimmed.to_ascii_lowercase();
    if let Some((w, h)) = lower.split_once('x') {
        let (w, h) = (w.trim(), h.trim());
        if !w.is_empty()
            && !h.is_empty()
            && w.bytes().all(|b| b.is_ascii_digit())
            && h.bytes().all(|b| b.is_ascii_digit())
        {
            return format!("{w}*{h}");
        }
    }
    trimmed.to_owned()
}
fn identifier(id: &str) -> bool {
    !id.is_empty()
        && id.trim() == id
        && !matches!(id, "." | "..")
        && id.len() <= south_contracts::MAX_TASK_ID_BYTES
        && !id.chars().any(char::is_control)
}
fn encode_segment(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}
fn request(config: &ProviderConfig, method: HttpMethod, path: &str) -> HttpRequestDescriptor {
    let mut descriptor = HttpRequestDescriptor::new(
        method,
        format!("{}/{path}", config.base_url.as_str().trim_end_matches('/')),
    );
    descriptor.auth = config.auth.clone().map(Auth::bearer);
    descriptor
}
fn checked_locator(locator: &TaskLocatorV2) -> ComponentResultV1<()> {
    if locator.route() == QUERY { Ok(()) } else { Err(invalid("unsupported Wan image locator")) }
}
fn unknown(reason: &str) -> TaskObservationV2 {
    TaskObservationV2::Unknown { reason: reason.into() }
}
/// Every non-empty `output.choices[].message.content[].image`, flattened across choices.
fn image_urls(body: &Value) -> Vec<&str> {
    body.pointer("/output/choices")
        .and_then(Value::as_array)
        .map(|choices| {
            choices
                .iter()
                .filter_map(|c| c.pointer("/message/content").and_then(Value::as_array))
                .flatten()
                .filter_map(|part| field(part, "image"))
                .filter(|url| !url.is_empty())
                .collect()
        })
        .unwrap_or_default()
}
/// The billed count: a positive reported `usage.image_count` at most one above what was
/// delivered, else the delivered count.
fn billed_count(body: &Value, delivered: usize) -> i64 {
    let delivered = i64::try_from(delivered).unwrap_or(i64::MAX);
    body.pointer("/usage/image_count")
        .and_then(Value::as_u64)
        .and_then(|n| i64::try_from(n).ok())
        .filter(|n| *n > 0 && *n <= delivered.saturating_add(1))
        .unwrap_or(delivered)
}

impl TaskComponentV2 for WanImageTaskComponentV2 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "task-wan-image-v2".into(),
            version: "0.35.2".into(),
            api_version: "task-adapter-v2".into(),
        }
    }
    fn build_submit_request(
        &self,
        config: &ProviderConfig,
        body: &Value,
        _: &HostMintedValuesV1,
    ) -> ComponentResultV1<PreparedTaskV2> {
        let model = field(body, "model")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| invalid("missing Wan image model"))?;
        let requested = match body.get("n") {
            None => 1,
            Some(raw) => {
                let n = whole_count(raw)
                    .ok_or_else(|| invalid(&format!("'n' must be an integer (got {raw})")))?;
                if !(1..=MAX_IMAGES).contains(&n) {
                    return Err(invalid(&format!(
                        "'n' must be in 1..=4 for Bailian Wan 2.7 image models (got {n})"
                    )));
                }
                n
            }
        };
        let mut content = Vec::new();
        match body.get("image_url") {
            None | Some(Value::Null) => {}
            Some(Value::String(url)) => content.push(json!({"image": url})),
            Some(Value::Array(items)) => {
                for item in items {
                    let url = item
                        .as_str()
                        .ok_or_else(|| invalid("image_url array entries must be string URLs"))?;
                    content.push(json!({"image": url}));
                }
            }
            Some(other) => {
                return Err(invalid(&format!(
                    "image_url must be a string URL or an array of string URLs (got {other})"
                )));
            }
        }
        let image_count = u32::try_from(content.len()).unwrap_or(u32::MAX);
        content.push(json!({"text": field(body, "prompt").unwrap_or("")}));
        let mut parameters = Map::new();
        if body.get("n").is_some() {
            parameters.insert("n".into(), json!(requested));
        }
        let size = body.get("size");
        if let Some(size) = size {
            parameters.insert(
                "size".into(),
                size.as_str().map_or_else(|| size.clone(), |s| json!(wan_size(s))),
            );
        }
        for key in PASSTHROUGH {
            if let Some(value) = body.get(key) {
                parameters.insert(key.into(), value.clone());
            }
        }
        // Only a named tier (`1K` / `2K` / `4K`) is a pricing fact; a pixel size is not a tier.
        let resolution =
            size.and_then(Value::as_str).map(|s| s.trim().to_ascii_uppercase()).filter(|s| {
                s.len() >= 2
                    && s.len() <= 32
                    && s.ends_with('K')
                    && s[..s.len() - 1].bytes().all(|b| b.is_ascii_digit())
            });
        let request_estimate = TaskRequestEstimateV2::new(None, None)
            .and_then(|e| e.with_input_facts(resolution.as_deref(), Some(image_count)))
            .and_then(|e| e.with_output_facts(None, i64::try_from(requested).ok()))
            .map_err(|_| invalid("invalid Wan image request estimate"))?;
        let mut descriptor = request(config, HttpMethod::Post, SUBMIT);
        descriptor.headers = SafeHeaders::try_new([
            ("content-type", "application/json"),
            ("x-dashscope-async", "enable"),
        ])
        .map_err(|_| protocol("invalid Wan image headers"))?;
        descriptor.body = Some(json!({
            "model": model,
            "input": {"messages": [{"role": "user", "content": content}]},
            "parameters": parameters,
        }));
        Ok(PreparedTaskV2 {
            descriptor,
            locator: TaskLocatorV2::new(1, QUERY)
                .map_err(|_| protocol("invalid Wan image locator"))?,
            request_estimate,
            immutable_body_paths: Some(IMMUTABLE_BODY_PATHS.map(str::to_owned).to_vec()),
        })
    }
    fn parse_submit_response(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<SubmitOutcomeV2> {
        if (400..500).contains(&parts.status) {
            let message = serde_json::from_str::<Value>(&parts.body)
                .ok()
                .and_then(|body| bounded(field(&body, "message")))
                .unwrap_or_else(|| "Wan image submission rejected".into());
            return Ok(SubmitOutcomeV2::Rejected(ErrorEnvelope::new(
                ErrorCode::ProviderProtocolError,
                parts.status,
                message,
            )));
        }
        if !(200..300).contains(&parts.status) {
            return Ok(SubmitOutcomeV2::Unknown);
        }
        let Ok(body) = serde_json::from_str::<Value>(&parts.body) else {
            return Ok(SubmitOutcomeV2::Unknown);
        };
        Ok(body
            .pointer("/output/task_id")
            .and_then(Value::as_str)
            .filter(|id| identifier(id))
            .map_or(SubmitOutcomeV2::Unknown, |id| SubmitOutcomeV2::Accepted(id.into())))
    }
    fn build_observe_request(
        &self,
        config: &ProviderConfig,
        _: &str,
        id: &str,
        locator: &TaskLocatorV2,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        checked_locator(locator)?;
        if !identifier(id) {
            return Err(invalid("invalid Wan image task identifier"));
        }
        Ok(request(config, HttpMethod::Get, &format!("{QUERY}/{}", encode_segment(id))))
    }
    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV2> {
        if !(200..300).contains(&parts.status) {
            return Ok(unknown("Wan image observation HTTP failure"));
        }
        let Ok(body) = serde_json::from_str::<Value>(&parts.body) else {
            return Ok(unknown("invalid Wan image observation JSON"));
        };
        let output = &body["output"];
        let failed = |kind| TaskObservationV2::Failed {
            kind,
            code: bounded(field(output, "code")),
            message: bounded(field(output, "message")),
        };
        Ok(match field(output, "task_status") {
            None => unknown("bailian poll body has no output.task_status"),
            Some(word @ ("PENDING" | "RUNNING")) => {
                TaskObservationV2::Progress { running: word == "RUNNING", status_word: word.into() }
            }
            Some("SUCCEEDED") => {
                let urls = image_urls(&body);
                if urls.is_empty() {
                    return Ok(TaskObservationV2::Failed {
                        kind: TaskFailureKindV1::Failed,
                        code: Some("empty_result".into()),
                        message: Some("bailian SUCCEEDED but delivered no image urls".into()),
                    });
                }
                let billed = billed_count(&body, urls.len());
                let artifacts: Result<Vec<_>, _> = urls
                    .iter()
                    .map(|url| TaskArtifactV2::new(url, TaskScalarV2::Null, TaskScalarV2::Null))
                    .collect();
                let (Ok(artifacts), Ok(usage)) = (
                    artifacts.and_then(TaskArtifactRefV2::urls),
                    TaskUsageFactsV2::new(None, None, None)
                        .and_then(|u| u.with_outputs(Some(billed))),
                ) else {
                    return Ok(unknown("Wan image result exceeds the artifact bounds"));
                };
                TaskObservationV2::Succeeded { artifacts, usage }
            }
            Some("CANCELED") => failed(TaskFailureKindV1::Cancelled),
            Some("FAILED" | "UNKNOWN") => failed(TaskFailureKindV1::Failed),
            Some(_) => unknown("bailian unrecognized task_status"),
        })
    }
    fn build_artifact_request(
        &self,
        _: &ProviderConfig,
        locator: &TaskLocatorV2,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        checked_locator(locator)?;
        observation.validate().map_err(|_| protocol("invalid Wan image observation"))?;
        Ok(None)
    }
    fn render_success(
        &self,
        observation: &TaskObservationV2,
        _: Option<&HttpResponseParts>,
        context: &TaskRenderContextV2,
    ) -> ComponentResultV1<Value> {
        observation.validate().map_err(|_| protocol("invalid Wan image observation"))?;
        let TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::Urls(items), .. } =
            observation
        else {
            return Err(protocol("Wan image render requires direct artifacts"));
        };
        Ok(json!({
            "created": context.created(),
            "data": items
                .iter()
                .map(|item| json!({"url": item.url(), "revised_prompt": null}))
                .collect::<Vec<_>>(),
        }))
    }
    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<ErrorEnvelope> {
        observation.validate().map_err(|_| protocol("invalid Wan image observation"))?;
        let TaskObservationV2::Failed { code, message, .. } = observation else {
            return Err(protocol("Wan image failure mapping requires failed observation"));
        };
        let detail = message.as_deref().unwrap_or("upstream task failed");
        let message =
            code.as_deref().map_or_else(|| detail.to_owned(), |code| format!("{code}: {detail}"));
        Ok(ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message))
    }
}
