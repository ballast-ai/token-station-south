//! Pure managed Bailian video translation, transcribed from the host's video dialect.
//! Credentials, pricing, persistence and time remain host responsibilities.
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

const QUERY: &str = "api/v1/tasks";
/// Managed video dialect; no credentials, clock, price or persistence.
#[derive(Debug, Default, Clone, Copy)]
pub struct BailianTaskComponentV2;
fn invalid(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, message)
}
fn protocol(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message)
}
fn field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}
fn bounded_field(value: &Value, key: &str) -> Option<String> {
    field(value, key)
        .filter(|s| s.len() <= south_contracts::MAX_ARTIFACT_REF_BYTES)
        .map(str::to_owned)
}
fn seconds(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
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
    if locator.route() == QUERY { Ok(()) } else { Err(invalid("unsupported Bailian locator")) }
}
fn unknown(reason: &str) -> TaskObservationV2 {
    TaskObservationV2::Unknown { reason: reason.into() }
}
fn resolution_hint(value: &str) -> Option<String> {
    let normalized: String =
        value.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_uppercase).collect();
    let dims: Vec<&str> = normalized.split(['X', '*']).collect();
    let dimensions = if let [w, h] = dims.as_slice() {
        w.parse::<u32>().ok().zip(h.parse::<u32>().ok())
    } else {
        None
    };
    if matches!(normalized.as_str(), "1080" | "1080P") || dimensions == Some((1920, 1080)) {
        return Some("1080P".into());
    }
    if matches!(normalized.as_str(), "480" | "480P") || dimensions.is_some_and(|(_, h)| h == 480) {
        return Some("480P".into());
    }
    (!normalized.is_empty()
        && normalized.len() <= 32
        && normalized.bytes().all(|b| b.is_ascii_alphanumeric()))
    .then_some(normalized)
}

impl TaskComponentV2 for BailianTaskComponentV2 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "task-bailian-v2".into(),
            version: "0.31.0".into(),
            api_version: "task-adapter-v2".into(),
        }
    }
    #[expect(
        clippy::too_many_lines,
        reason = "the four existing video input shapes share one request construction boundary"
    )]
    fn build_submit_request(
        &self,
        config: &ProviderConfig,
        body: &Value,
        _: &HostMintedValuesV1,
    ) -> ComponentResultV1<PreparedTaskV2> {
        let model = field(body, "model")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| invalid("missing Bailian model"))?;
        let shape =
            config.extensions.get("upstream_model_family").and_then(Value::as_str).unwrap_or(model);
        let prompt = field(body, "prompt").ok_or_else(|| invalid("missing Bailian prompt"))?;
        let kf = shape.contains("kf2v");
        let r2v = shape.starts_with("happyhorse") && shape.contains("r2v");
        let image = field(body, "image");
        let mut input = json!({"prompt":prompt});
        let mut parameters = json!({});
        let mut image_count = u32::from(image.is_some());
        if kf {
            input["first_frame_url"] =
                json!(image.ok_or_else(|| invalid("Bailian first frame is required"))?);
            if let Some(last) = field(body, "last_frame") {
                input["last_frame_url"] = json!(last);
                image_count += 1;
            }
            if let Some(negative) = field(body, "negative_prompt") {
                input["negative_prompt"] = json!(negative);
            }
            if let Some(duration) = body.get("duration").filter(|v| !v.is_null())
                && seconds(duration) != Some(5.0)
            {
                return Err(invalid("Bailian first-last-frame duration must be five seconds"));
            }
            for key in ["resolution", "prompt_extend", "watermark", "seed"] {
                if let Some(value) = body.get(key).filter(|v| !v.is_null()) {
                    parameters[key] = value.clone();
                }
            }
        } else {
            if r2v {
                if body.get("image").is_some_and(|v| !v.is_null()) {
                    return Err(invalid(
                        "Bailian reference video requires reference_images, not a first frame",
                    ));
                }
                let references = body
                    .get("reference_images")
                    .and_then(Value::as_array)
                    .filter(|items| !items.is_empty())
                    .ok_or_else(|| invalid("'reference_images' (1–9 image URLs) is required"))?;
                if references.len() > 9 {
                    return Err(invalid("reference_images accepts at most 9 entries"));
                }
                let urls: Option<Vec<&str>> = references.iter().map(Value::as_str).collect();
                let urls =
                    urls.ok_or_else(|| invalid("Bailian reference images must be strings"))?;
                image_count = u32::try_from(urls.len())
                    .map_err(|_| invalid("too many Bailian reference images"))?;
                input["media"] = json!(
                    urls.into_iter()
                        .map(|url| json!({"type":"reference_image","url":url}))
                        .collect::<Vec<_>>()
                );
                if let Some(ratio) =
                    body.get("aspect_ratio").or_else(|| body.get("ratio")).and_then(Value::as_str)
                {
                    parameters["ratio"] = json!(ratio);
                }
            } else if let Some(image) = image {
                if shape.starts_with("happyhorse") {
                    input["media"] = json!([{"type":"first_frame","url":image}]);
                } else {
                    input["img_url"] = json!(image);
                }
            }
            if let Some(audio) = field(body, "audio_url") {
                input["audio_url"] = json!(audio);
            }
            for key in ["size", "resolution"] {
                if let Some(value) = field(body, key) {
                    parameters[key] = json!(value);
                }
            }
            for key in ["duration", "prompt_extend", "watermark", "seed"] {
                if let Some(value) = body.get(key) {
                    parameters[key] = value.clone();
                }
            }
        }
        let duration = if kf { 5.0 } else { body.get("duration").and_then(seconds).unwrap_or(5.0) };
        if duration <= 0.0 || duration > f64::from(u32::MAX) {
            return Err(invalid("invalid Bailian video duration"));
        }
        let resolution =
            field(body, "resolution").or_else(|| field(body, "size")).and_then(resolution_hint);
        let request_estimate = TaskRequestEstimateV2::new(Some(duration), None)
            .and_then(|e| e.with_input_facts(resolution.as_deref(), Some(image_count)))
            .map_err(|_| invalid("invalid Bailian request estimate"))?;
        let path = if kf {
            "api/v1/services/aigc/image2video/video-synthesis"
        } else {
            "api/v1/services/aigc/video-generation/video-synthesis"
        };
        let mut descriptor = request(config, HttpMethod::Post, path);
        descriptor.headers = SafeHeaders::try_new([
            ("content-type", "application/json"),
            ("x-dashscope-async", "enable"),
        ])
        .map_err(|_| protocol("invalid Bailian headers"))?;
        descriptor.body = Some(json!({"model":model,"input":input,"parameters":parameters}));
        Ok(PreparedTaskV2 {
            descriptor,
            locator: TaskLocatorV2::new(1, QUERY)
                .map_err(|_| protocol("invalid Bailian locator"))?,
            request_estimate,
        })
    }
    fn parse_submit_response(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<SubmitOutcomeV2> {
        if (400..500).contains(&parts.status) {
            return Ok(SubmitOutcomeV2::Rejected(ErrorEnvelope::new(
                ErrorCode::ProviderProtocolError,
                parts.status,
                "Bailian submission rejected",
            )));
        }
        if !(200..300).contains(&parts.status) {
            return Ok(SubmitOutcomeV2::Unknown);
        }
        let Ok(body) = serde_json::from_str::<Value>(&parts.body) else {
            return Ok(SubmitOutcomeV2::Unknown);
        };
        let id = body.get("output").and_then(|v| field(v, "task_id")).filter(|id| identifier(id));
        if body.get("code").is_some_and(|v| !v.is_null()) {
            return Ok(if id.is_some() {
                SubmitOutcomeV2::Unknown
            } else {
                SubmitOutcomeV2::Rejected(invalid("Bailian submission rejected"))
            });
        }
        Ok(id.map_or(SubmitOutcomeV2::Unknown, |id| SubmitOutcomeV2::Accepted(id.into())))
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
            return Err(invalid("invalid Bailian task identifier"));
        }
        Ok(request(config, HttpMethod::Get, &format!("{QUERY}/{}", encode_segment(id))))
    }
    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV2> {
        if !(200..300).contains(&parts.status) {
            return Ok(unknown("Bailian observation HTTP failure"));
        }
        let Ok(body) = serde_json::from_str::<Value>(&parts.body) else {
            return Ok(unknown("invalid Bailian observation JSON"));
        };
        let output = &body["output"];
        Ok(match field(output, "task_status") {
            Some(word @ ("PENDING" | "RUNNING")) => {
                TaskObservationV2::Progress { running: word == "RUNNING", status_word: word.into() }
            }
            Some("SUCCEEDED") => {
                let Some(url) = field(output, "video_url") else {
                    return Ok(unknown("Bailian success has no video URL"));
                };
                let Ok(artifact) = TaskArtifactV2::new(url, TaskScalarV2::Null, TaskScalarV2::Null)
                else {
                    return Ok(unknown("invalid Bailian artifact URL"));
                };
                let usage = &body["usage"];
                let seconds = usage
                    .get("duration")
                    .and_then(seconds)
                    .or_else(|| usage.get("video_duration").and_then(seconds));
                TaskObservationV2::Succeeded {
                    artifacts: TaskArtifactRefV2::urls(vec![artifact])
                        .map_err(|_| protocol("invalid Bailian artifacts"))?,
                    usage: TaskUsageFactsV2::new(seconds, None, None)
                        .map_err(|_| protocol("invalid Bailian usage"))?,
                }
            }
            Some(word @ ("FAILED" | "CANCELED")) => TaskObservationV2::Failed {
                kind: if word == "CANCELED" {
                    TaskFailureKindV1::Cancelled
                } else {
                    TaskFailureKindV1::Failed
                },
                code: bounded_field(output, "code"),
                message: bounded_field(output, "message"),
            },
            _ => unknown("Bailian task observation is unavailable"),
        })
    }
    fn build_artifact_request(
        &self,
        _: &ProviderConfig,
        locator: &TaskLocatorV2,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        checked_locator(locator)?;
        observation.validate().map_err(|_| protocol("invalid Bailian observation"))?;
        Ok(None)
    }
    fn render_success(
        &self,
        observation: &TaskObservationV2,
        _: Option<&HttpResponseParts>,
        context: &TaskRenderContextV2,
    ) -> ComponentResultV1<Value> {
        observation.validate().map_err(|_| protocol("invalid Bailian observation"))?;
        let TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::Urls(items), .. } =
            observation
        else {
            return Err(protocol("Bailian render requires direct artifacts"));
        };
        let id = context
            .upstream_task_id()
            .ok_or_else(|| protocol("Bailian render requires an upstream task identifier"))?;
        Ok(
            json!({"created":context.created(),"model":context.model(),"provider":context.provider(),"task_id":id,"data":items.iter().map(|item| json!({"url":item.url()})).collect::<Vec<_>>() }),
        )
    }
    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<ErrorEnvelope> {
        observation.validate().map_err(|_| protocol("invalid Bailian observation"))?;
        let TaskObservationV2::Failed { kind, code, message } = observation else {
            return Err(protocol("Bailian failure mapping requires failed observation"));
        };
        let status = if *kind == TaskFailureKindV1::Cancelled { "CANCELED" } else { "FAILED" };
        let detail = match (code, message) {
            (Some(c), Some(m)) => format!("{m} ({c})"),
            (Some(c), None) => c.clone(),
            (None, Some(m)) => m.clone(),
            (None, None) => "generation failed".into(),
        };
        Ok(ErrorEnvelope::new(
            ErrorCode::ProviderProtocolError,
            400,
            format!("Bailian video generation {status}: {detail}"),
        ))
    }
}
