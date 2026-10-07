//! Pure xAI Grok Imagine video translation.
//!
//! Transcribed from the host's native xAI dialect (token-station-server `handler/video/xai.rs`,
//! `durable.rs::xai_run`, `observe.rs::normalize_xai`). Credentials, pricing, persistence, time
//! and remote-image fetching remain host responsibilities.
//!
//! Differences from the host-native arm, all deliberate:
//! - Input images are forwarded as given (`{"url": …}`). xAI accepts public URLs and data URIs
//!   alike; a host that wants to inline them configures its own pre-fetch before calling here.
//! - An out-of-range duration is refused here (xAI accepts 1–15 s) instead of being sent for the
//!   upstream to reject after the host has reserved for it.
//! - A request without `duration` reports no requested seconds: the upstream picks the length, and
//!   the component does not invent one. The host decides what to reserve (its default-duration
//!   policy), exactly as for any component that cannot know the length up front.
use crate::{ComponentResultV1, PreparedTaskV2, SubmitOutcomeV2, TaskComponentV2};
use serde_json::{Value, json};
use south_contracts::{
    HostMintedValuesV1, TaskArtifactRefV2, TaskArtifactV2, TaskFailureKindV1, TaskLocatorV2,
    TaskObservationV2, TaskRenderContextV2, TaskRequestEstimateV2, TaskScalarV2, TaskUsageFactsV2,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{
    Auth, ErrorCode, ErrorEnvelope, HttpMethod, HttpRequestDescriptor, HttpResponseParts,
    ProviderConfig, SafeHeaders,
};

const QUERY: &str = "v1/videos";
const SUBMIT: &str = "v1/videos/generations";
/// "A maximum of 7 reference images can be provided per request." (docs.x.ai reference-to-video)
const MAX_REFERENCE_IMAGES: usize = 7;
/// xAI video duration bounds in seconds.
const MIN_SECONDS: f64 = 1.0;
const MAX_SECONDS: f64 = 15.0;
/// Request-body fields the host must not rewrite (contract 6): the upstream model and the
/// duration the reservation was computed from.
const IMMUTABLE_BODY_PATHS: [&str; 2] = ["model", "duration"];

/// Managed xAI video dialect; no credentials, clock, price or persistence.
#[derive(Debug, Default, Clone, Copy)]
pub struct XaiTaskComponentV2;

fn invalid(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, message)
}
fn protocol(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message)
}
fn field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}
fn bounded(text: &str) -> Option<String> {
    (text.len() <= south_contracts::MAX_ARTIFACT_REF_BYTES).then(|| text.to_owned())
}
fn seconds(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
}
fn scalar(value: Option<&Value>) -> TaskScalarV2 {
    match value {
        Some(Value::String(s)) if s.len() <= south_contracts::MAX_ARTIFACT_REF_BYTES => {
            TaskScalarV2::String(s.clone())
        }
        Some(Value::Number(n)) => n.as_u64().map_or_else(
            || {
                n.as_i64().map_or_else(
                    || n.as_f64().map_or(TaskScalarV2::Null, TaskScalarV2::Float),
                    TaskScalarV2::Signed,
                )
            },
            TaskScalarV2::Unsigned,
        ),
        _ => TaskScalarV2::Null,
    }
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
    if locator.route() == QUERY { Ok(()) } else { Err(invalid("unsupported xAI locator")) }
}
fn unknown(reason: &str) -> TaskObservationV2 {
    TaskObservationV2::Unknown { reason: reason.into() }
}

impl TaskComponentV2 for XaiTaskComponentV2 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "task-xai-v2".into(),
            version: "0.35.5".into(),
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
            .ok_or_else(|| invalid("missing xAI model"))?;
        let prompt = field(body, "prompt").ok_or_else(|| invalid("Missing 'prompt' field"))?;
        let mut upstream = json!({"model": model, "prompt": prompt});
        let duration = match body.get("duration").filter(|v| !v.is_null()) {
            Some(raw) => {
                let value = seconds(raw)
                    .filter(|s| (MIN_SECONDS..=MAX_SECONDS).contains(s))
                    .ok_or_else(|| invalid("xAI video duration must be 1 to 15 seconds"))?;
                upstream["duration"] = raw.clone();
                Some(value)
            }
            None => None,
        };
        for key in ["aspect_ratio", "resolution"] {
            if let Some(value) = field(body, key) {
                upstream[key] = json!(value);
            }
        }
        let mut image_count = 0u32;
        if let Some(url) = field(body, "last_frame").or_else(|| field(body, "last_frame_url")) {
            upstream["last_frame"] = json!({"url": url});
            image_count += 1;
        }
        if let Some(references) = body.get("reference_images").filter(|v| !v.is_null()) {
            let items = references.as_array().ok_or_else(|| {
                invalid("'reference_images' must be an array of image URL strings")
            })?;
            if items.len() > MAX_REFERENCE_IMAGES {
                return Err(invalid(
                    "'reference_images' accepts at most 7 entries; xAI rejects more",
                ));
            }
            let urls: Option<Vec<&str>> = items.iter().map(Value::as_str).collect();
            let urls =
                urls.ok_or_else(|| invalid("'reference_images' entries must be URL strings"))?;
            if !urls.is_empty() {
                upstream["reference_images"] =
                    json!(urls.iter().map(|url| json!({"url": url})).collect::<Vec<_>>());
                image_count += u32::try_from(urls.len()).unwrap_or(u32::MAX);
            }
        }
        if let Some(url) = field(body, "image_url").or_else(|| field(body, "image")) {
            upstream["image"] = json!({"url": url});
            image_count += 1;
        }
        if let Some(url) = field(body, "video_url") {
            upstream["video_url"] = json!(url);
        }
        let request_estimate = TaskRequestEstimateV2::new(duration, None)
            .and_then(|e| e.with_input_facts(None, Some(image_count)))
            .and_then(|e| e.with_output_facts(None, Some(1)))
            .map_err(|_| invalid("invalid xAI request estimate"))?;
        let mut descriptor = request(config, HttpMethod::Post, SUBMIT);
        descriptor.headers = SafeHeaders::try_new([("content-type", "application/json")])
            .map_err(|_| protocol("invalid xAI headers"))?;
        descriptor.body = Some(upstream);
        Ok(PreparedTaskV2 {
            descriptor,
            locator: TaskLocatorV2::new(1, QUERY).map_err(|_| protocol("invalid xAI locator"))?,
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
                .and_then(|body| field(&body, "error").and_then(bounded))
                .unwrap_or_else(|| "xAI submission rejected".into());
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
        Ok(field(&body, "request_id")
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
            return Err(invalid("invalid xAI request identifier"));
        }
        Ok(request(config, HttpMethod::Get, &format!("{QUERY}/{}", encode_segment(id))))
    }
    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV2> {
        if !(200..300).contains(&parts.status) {
            return Ok(unknown("xAI observation HTTP failure"));
        }
        let Ok(body) = serde_json::from_str::<Value>(&parts.body) else {
            return Ok(unknown("invalid xAI observation JSON"));
        };
        Ok(match field(&body, "status") {
            None => unknown("xai poll body has no status"),
            Some("pending") => {
                TaskObservationV2::Progress { running: false, status_word: "pending".into() }
            }
            Some("done") => {
                let video = &body["video"];
                let Some(url) = field(video, "url") else {
                    return Ok(unknown("xai done without video.url"));
                };
                let Ok(artifact) =
                    TaskArtifactV2::new(url, TaskScalarV2::Null, scalar(video.get("duration")))
                else {
                    return Ok(unknown("invalid xAI artifact URL"));
                };
                TaskObservationV2::Succeeded {
                    artifacts: TaskArtifactRefV2::urls(vec![artifact])
                        .map_err(|_| protocol("invalid xAI artifacts"))?,
                    usage: TaskUsageFactsV2::new(
                        video.get("duration").and_then(seconds),
                        None,
                        None,
                    )
                    .and_then(|usage| usage.with_outputs(Some(1)))
                    .map_err(|_| protocol("invalid xAI usage"))?,
                }
            }
            Some("failed") => TaskObservationV2::Failed {
                kind: TaskFailureKindV1::Failed,
                code: None,
                message: field(&body, "error").and_then(bounded),
            },
            Some("expired") => TaskObservationV2::Failed {
                kind: TaskFailureKindV1::ProviderExpired,
                code: None,
                message: None,
            },
            Some(_) => unknown("xai unrecognized status"),
        })
    }
    fn build_artifact_request(
        &self,
        _: &ProviderConfig,
        locator: &TaskLocatorV2,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        checked_locator(locator)?;
        observation.validate().map_err(|_| protocol("invalid xAI observation"))?;
        Ok(None)
    }
    fn render_success(
        &self,
        observation: &TaskObservationV2,
        _: Option<&HttpResponseParts>,
        context: &TaskRenderContextV2,
    ) -> ComponentResultV1<Value> {
        observation.validate().map_err(|_| protocol("invalid xAI observation"))?;
        let TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::Urls(items), .. } =
            observation
        else {
            return Err(protocol("xAI render requires direct artifacts"));
        };
        let id = context
            .upstream_task_id()
            .ok_or_else(|| protocol("xAI render requires an upstream request identifier"))?;
        let data = items
            .iter()
            .map(|item| {
                let duration = match item.duration() {
                    TaskScalarV2::Null => Value::Null,
                    TaskScalarV2::String(s) => json!(s),
                    TaskScalarV2::Signed(n) => json!(n),
                    TaskScalarV2::Unsigned(n) => json!(n),
                    TaskScalarV2::Float(n) => json!(n),
                };
                json!({"url": item.url(), "duration": duration})
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "created": context.created(),
            "model": context.model(),
            "provider": context.provider(),
            "request_id": id,
            "data": data,
        }))
    }
    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<ErrorEnvelope> {
        observation.validate().map_err(|_| protocol("invalid xAI observation"))?;
        let TaskObservationV2::Failed { kind, .. } = observation else {
            return Err(protocol("xAI failure mapping requires failed observation"));
        };
        let message = if *kind == TaskFailureKindV1::ProviderExpired {
            "xAI video generation expired"
        } else {
            "xAI video generation failed"
        };
        Ok(ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message))
    }
}
