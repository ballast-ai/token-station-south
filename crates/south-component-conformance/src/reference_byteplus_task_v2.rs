//! Pure `BytePlus` `ModelArk` Seedance video translation.
//!
//! Transcribed from the host's native `BytePlus` dialect (token-station-server
//! `handler/video/byteplus.rs`, `durable.rs::byteplus_run` / `byteplus_billing_tokens`,
//! `observe.rs::normalize_byteplus`). Credentials, pricing, persistence and time remain host
//! responsibilities.
//!
//! The token rate is the one fact that moves here from the host (task contract 6): Seedance
//! meters `width × height × 24 / 1024` tokens per second of output at the requested resolution,
//! and before this component the host held a second copy of that formula to reserve. The
//! component now states the rate; the host multiplies its own time and price.
//!
//! Differences from the host-native arm:
//! - Reference images are capped at 30 (the most any Seedance model accepts); a host that knows a
//!   model's lower cap enforces it before calling here, as token-station-server's capability
//!   pre-check already does.
//! - The upstream's `content.last_frame_url` (returned when the request asked for
//!   `return_last_frame`) is reported as a second artifact with the `last_frame` role (task
//!   contract 7) and rendered into `data[0].last_frame_url`, where the native blocking body puts
//!   it. It is passed through as the upstream spelled it — not proxied, not stored and not
//!   credential-gated, like the video URL itself; the host counts, delivers and stores only the
//!   video. An absent or empty value means no frame; one the contract cannot carry (over its
//!   byte bound) makes the observation unknown rather than silently dropping the frame.
use crate::{ComponentResultV1, PreparedTaskV2, SubmitOutcomeV2, TaskComponentV2};
use serde_json::{Value, json};
use south_contracts::{
    HostMintedValuesV1, TaskArtifactRefV2, TaskArtifactRoleV2, TaskArtifactV2, TaskFailureKindV1,
    TaskLocatorV2, TaskObservationV2, TaskRenderContextV2, TaskRequestEstimateV2, TaskScalarV2,
    TaskUsageFactsV2,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{
    Auth, ErrorCode, ErrorEnvelope, HttpMethod, HttpRequestDescriptor, HttpResponseParts,
    ProviderConfig, SafeHeaders,
};

const TASKS: &str = "api/v3/contents/generations/tasks";
/// The most reference images any Seedance model accepts (2.5 raised it to 30).
const MAX_REFERENCE_IMAGES: usize = 30;
const DEFAULT_SECONDS: i64 = 5;
const DEFAULT_SECONDS_F64: f64 = 5.0;
const DEFAULT_RESOLUTION: &str = "720p";
/// Request-body fields the host must not rewrite (contract 6): what the token rate and the
/// reservation are computed from.
const IMMUTABLE_BODY_PATHS: [&str; 3] = ["model", "resolution", "duration"];

/// Managed `BytePlus` Seedance video dialect; no credentials, clock, price or persistence.
#[derive(Debug, Default, Clone, Copy)]
pub struct BytePlusTaskComponentV2;

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
fn seconds(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
}
/// Seedance output pixel size per resolution tier, as the upstream meters it.
fn pixels(resolution: &str) -> (i64, i64) {
    match resolution.to_ascii_lowercase().as_str() {
        "480p" => (864, 496),
        "720p" => (1280, 720),
        "4k" | "2160p" => (3840, 2160),
        _ => (1920, 1088),
    }
}
/// `width × height × 24 frames ÷ 1024` tokens per second of output.
fn tokens_per_second(resolution: &str) -> i64 {
    let (width, height) = pixels(resolution);
    width * height * 24 / 1024
}
fn resolution_fact(resolution: &str) -> Option<String> {
    let upper = resolution.to_ascii_uppercase();
    (!upper.is_empty() && upper.len() <= 32 && upper.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then_some(upper)
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
    if locator.route() == TASKS { Ok(()) } else { Err(invalid("unsupported BytePlus locator")) }
}
fn unknown(reason: &str) -> TaskObservationV2 {
    TaskObservationV2::Unknown { reason: reason.into() }
}
fn image_part(url: &str, role: &str) -> Value {
    json!({"type": "image_url", "image_url": {"url": url}, "role": role})
}

impl TaskComponentV2 for BytePlusTaskComponentV2 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "task-byteplus-v2".into(),
            version: "0.36.3".into(),
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
            .ok_or_else(|| invalid("missing BytePlus model"))?;
        let prompt = field(body, "prompt").ok_or_else(|| invalid("Missing 'prompt' field"))?;
        let first_frame = field(body, "image").or_else(|| field(body, "image_url"));
        let mut content = vec![json!({"type": "text", "text": prompt})];
        let mut image_count = 0u32;
        if let Some(url) = first_frame {
            content.push(image_part(url, "first_frame"));
            image_count += 1;
        }
        if let Some(url) = field(body, "last_frame") {
            content.push(image_part(url, "last_frame"));
            image_count += 1;
        }
        if let Some(url) = field(body, "video").or_else(|| field(body, "video_url")) {
            content.push(json!({"type": "video_url", "video_url": {"url": url}, "role": "video"}));
        }
        if let Some(references) = body.get("reference_images").filter(|v| !v.is_null()) {
            let items = references.as_array().ok_or_else(|| {
                invalid("'reference_images' must be an array of image URL strings")
            })?;
            if items.len() > MAX_REFERENCE_IMAGES {
                return Err(invalid(
                    "'reference_images' accepts at most 30 entries; Seedance rejects more",
                ));
            }
            for item in items {
                let url = item
                    .as_str()
                    .ok_or_else(|| invalid("'reference_images' entries must be URL strings"))?;
                content.push(image_part(url, "reference_image"));
                image_count += 1;
            }
        }
        let mut upstream = json!({"model": model, "content": content});
        upstream["ratio"] = match field(body, "aspect_ratio").or_else(|| field(body, "ratio")) {
            Some(ratio) => json!(ratio),
            None if first_frame.is_some() => json!("adaptive"),
            None => json!("16:9"),
        };
        let duration = if let Some(raw) = body.get("duration").filter(|v| !v.is_null()) {
            let value = seconds(raw)
                .ok_or_else(|| invalid("BytePlus video duration must be positive seconds"))?;
            upstream["duration"] = raw.clone();
            value
        } else {
            // The native arm writes the default into the body, so it is a fact of the request.
            upstream["duration"] = json!(DEFAULT_SECONDS);
            DEFAULT_SECONDS_F64
        };
        let resolution = field(body, "resolution").unwrap_or(DEFAULT_RESOLUTION);
        upstream["resolution"] = json!(resolution);
        for key in
            ["seed", "generate_audio", "draft", "watermark", "camera_fixed", "return_last_frame"]
        {
            if let Some(value) = body.get(key) {
                upstream[key] = value.clone();
            }
        }
        if let Some(tier) = field(body, "service_tier") {
            upstream["service_tier"] = json!(tier);
        }
        let request_estimate = TaskRequestEstimateV2::new(Some(duration), None)
            .and_then(|e| {
                e.with_input_facts(resolution_fact(resolution).as_deref(), Some(image_count))
            })
            .and_then(|e| e.with_output_facts(Some(tokens_per_second(resolution)), Some(1)))
            .map_err(|_| invalid("invalid BytePlus request estimate"))?;
        let mut descriptor = request(config, HttpMethod::Post, TASKS);
        descriptor.headers = SafeHeaders::try_new([("content-type", "application/json")])
            .map_err(|_| protocol("invalid BytePlus headers"))?;
        descriptor.body = Some(upstream);
        Ok(PreparedTaskV2 {
            descriptor,
            locator: TaskLocatorV2::new(1, TASKS)
                .map_err(|_| protocol("invalid BytePlus locator"))?,
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
                .and_then(|body| bounded(body.get("error").and_then(|e| field(e, "message"))))
                .unwrap_or_else(|| "BytePlus submission rejected".into());
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
        Ok(field(&body, "id")
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
            return Err(invalid("invalid BytePlus task identifier"));
        }
        Ok(request(config, HttpMethod::Get, &format!("{TASKS}/{}", encode_segment(id))))
    }
    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV2> {
        if !(200..300).contains(&parts.status) {
            return Ok(unknown("BytePlus observation HTTP failure"));
        }
        let Ok(body) = serde_json::from_str::<Value>(&parts.body) else {
            return Ok(unknown("invalid BytePlus observation JSON"));
        };
        let error = |key: &str| bounded(body.get("error").and_then(|e| field(e, key)));
        Ok(match field(&body, "status") {
            None => unknown("byteplus poll body has no status"),
            Some(word @ ("queued" | "running")) => {
                TaskObservationV2::Progress { running: word == "running", status_word: word.into() }
            }
            Some("succeeded") => {
                let content = &body["content"];
                let Some(url) = field(content, "video_url") else {
                    return Ok(unknown("byteplus succeeded without content.video_url"));
                };
                let Ok(video) = TaskArtifactV2::new(url, TaskScalarV2::Null, TaskScalarV2::Null)
                else {
                    return Ok(unknown("invalid BytePlus artifact URL"));
                };
                let mut artifacts = vec![video];
                // Contract 7: the last frame rides beside the video, never as a second output.
                if let Some(frame) = field(content, "last_frame_url").filter(|f| !f.is_empty()) {
                    let Ok(frame) =
                        TaskArtifactV2::new(frame, TaskScalarV2::Null, TaskScalarV2::Null)
                    else {
                        return Ok(unknown("invalid BytePlus last frame URL"));
                    };
                    artifacts.push(frame.with_role(TaskArtifactRoleV2::LastFrame));
                }
                let tokens = body.get("usage").and_then(|u| u.get("completion_tokens"));
                let tokens = match tokens {
                    None | Some(Value::Null) => None,
                    Some(value) => match value.as_i64().filter(|t| *t >= 0) {
                        Some(tokens) => Some(tokens),
                        None => return Ok(unknown("invalid BytePlus completion_tokens")),
                    },
                };
                TaskObservationV2::Succeeded {
                    artifacts: TaskArtifactRefV2::urls(artifacts)
                        .map_err(|_| protocol("invalid BytePlus artifacts"))?,
                    usage: TaskUsageFactsV2::new(None, None, tokens)
                        .and_then(|usage| usage.with_outputs(Some(1)))
                        .map_err(|_| protocol("invalid BytePlus usage"))?,
                }
            }
            Some("failed") => TaskObservationV2::Failed {
                kind: TaskFailureKindV1::Failed,
                code: error("code"),
                message: error("message"),
            },
            Some("expired") => TaskObservationV2::Failed {
                kind: TaskFailureKindV1::ProviderExpired,
                code: error("code"),
                message: error("message"),
            },
            Some(_) => unknown("byteplus unrecognized status"),
        })
    }
    fn build_artifact_request(
        &self,
        _: &ProviderConfig,
        locator: &TaskLocatorV2,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        checked_locator(locator)?;
        observation.validate().map_err(|_| protocol("invalid BytePlus observation"))?;
        Ok(None)
    }
    fn render_success(
        &self,
        observation: &TaskObservationV2,
        _: Option<&HttpResponseParts>,
        context: &TaskRenderContextV2,
    ) -> ComponentResultV1<Value> {
        observation.validate().map_err(|_| protocol("invalid BytePlus observation"))?;
        let TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::Urls(items), .. } =
            observation
        else {
            return Err(protocol("BytePlus render requires direct artifacts"));
        };
        let id = context
            .upstream_task_id()
            .ok_or_else(|| protocol("BytePlus render requires an upstream task identifier"))?;
        // `data` lists the videos, as the native body does; a last frame is written onto the
        // video it follows (`data[0].last_frame_url` for Seedance's single output).
        let mut data = Vec::new();
        for item in items {
            match item.role() {
                TaskArtifactRoleV2::Primary => data.push(json!({"url": item.url()})),
                TaskArtifactRoleV2::LastFrame => {
                    let Some(video) = data.last_mut() else {
                        return Err(protocol("BytePlus last frame precedes its video"));
                    };
                    video["last_frame_url"] = json!(item.url());
                }
            }
        }
        Ok(json!({
            "created": context.created(),
            "model": context.model(),
            "provider": context.provider(),
            "task_id": id,
            "data": data,
        }))
    }
    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<ErrorEnvelope> {
        observation.validate().map_err(|_| protocol("invalid BytePlus observation"))?;
        let TaskObservationV2::Failed { kind, message, .. } = observation else {
            return Err(protocol("BytePlus failure mapping requires failed observation"));
        };
        let message = if *kind == TaskFailureKindV1::ProviderExpired {
            "BytePlus video generation task expired".to_owned()
        } else {
            format!(
                "BytePlus video generation failed: {}",
                message.as_deref().unwrap_or("Video generation failed")
            )
        };
        Ok(ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message))
    }
}
