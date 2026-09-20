//! Candidate Kling task-v2 translation, transcribed from managed host behavior.
//!
//! Explicit operation and persisted route replace model-name heuristics. Host
//! policy still owns mode eligibility, pricing, reservation estimates and result
//! delivery. The stricter artifact and usage checks are candidate boundaries,
//! not a claim that every malformed legacy response behaves identically.
use crate::{ComponentResultV1, PreparedTaskV2, SubmitOutcomeV2, TaskComponentV2, task_v2_json};
use serde_json::{Map, Value, json};
use south_contracts::{
    HostMintedValuesV1, MAX_ARTIFACT_REF_BYTES, TaskArtifactRefV2, TaskArtifactV2,
    TaskFailureKindV1, TaskLocatorV2, TaskObservationV2, TaskRenderContextV2, TaskUsageFactsV2,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{
    Auth, ErrorCode, ErrorEnvelope, HttpMethod, HttpRequestDescriptor, HttpResponseParts,
    ProviderConfig, SafeHeaders,
};

// Candidate compatibility bound retained from the v1 component.
const MAX_MILLIUNITS: f64 = 1e15;

const TEXT: &str = "v1/videos/text2video";
const IMAGE: &str = "v1/videos/image2video";
const OMNI: &str = "v1/videos/omni-video";
const MOTION: &str = "v1/videos/motion-control";
const MULTI_PROMPT_REQUIRED: &str =
    "multi_shot=true with shot_type=customize requires 'multi_prompt'";

/// Stateless reference for the candidate task-adapter-v2 world.
#[derive(Debug, Default, Clone, Copy)]
pub struct KlingTaskReferenceV2;

fn invalid(message: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, message)
}
fn internal(message: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, message)
}
fn unknown(reason: impl Into<String>) -> TaskObservationV2 {
    TaskObservationV2::Unknown { reason: reason.into() }
}
fn field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}
fn customize(request: &Value) -> bool {
    request.get("multi_shot").and_then(Value::as_bool) == Some(true)
        && field(request, "shot_type") == Some("customize")
}
fn copy_fields(body: &mut Map<String, Value>, request: &Value, fields: &[&str]) {
    for key in fields {
        if let Some(value) = request.get(key) {
            body.insert((*key).to_owned(), value.clone());
        }
    }
}
fn duration(body: &mut Map<String, Value>, request: &Value) {
    body.insert(
        "duration".to_owned(),
        request.get("duration").cloned().unwrap_or_else(|| json!("5")),
    );
}
fn text_body(
    request: &Value,
    body: &mut Map<String, Value>,
    minted: &HostMintedValuesV1,
) -> ComponentResultV1<&'static str> {
    match field(request, "prompt") {
        Some(prompt) => {
            body.insert("prompt".into(), json!(prompt));
        }
        None if customize(request) => {}
        None => return Err(invalid("Missing 'prompt' field")),
    }
    if customize(request) && request.get("multi_prompt").is_none() {
        return Err(invalid(MULTI_PROMPT_REQUIRED));
    }
    copy_fields(
        body,
        request,
        &[
            "negative_prompt",
            "sound",
            "multi_shot",
            "shot_type",
            "multi_prompt",
            "image",
            "image_tail",
            "voice_list",
            "camera_control",
            "static_mask",
            "dynamic_masks",
            "cfg_scale",
            "aspect_ratio",
        ],
    );
    duration(body, request);
    if let Some(mode) = field(request, "mode") {
        body.insert("mode".into(), json!(mode));
    }
    body.insert("external_task_id".into(), json!(minted.task_id()));
    Ok(if field(request, "image").is_some() || field(request, "image_tail").is_some() {
        IMAGE
    } else {
        TEXT
    })
}
fn omni_body(request: &Value, body: &mut Map<String, Value>) -> ComponentResultV1<&'static str> {
    let videos = request.get("video_list").and_then(Value::as_array);
    if videos.is_some_and(|videos| !videos.is_empty()) && field(request, "sound") == Some("on") {
        return Err(invalid(
            "Kling omni-video: 'sound' must be \"off\" when 'video_list' is present (official spec — reference/edited videos cannot add generated audio)",
        ));
    }
    if customize(request) && request.get("multi_prompt").is_none() {
        return Err(invalid(MULTI_PROMPT_REQUIRED));
    }
    if !customize(request) && field(request, "prompt").is_none_or(str::is_empty) {
        return Err(invalid("Missing 'prompt' field"));
    }
    let mode = field(request, "mode")
        .filter(|mode| !mode.is_empty())
        .ok_or_else(|| invalid("omni requires a resolved mode"))?;
    copy_fields(
        body,
        request,
        &[
            "prompt",
            "multi_shot",
            "shot_type",
            "multi_prompt",
            "image_list",
            "element_list",
            "video_list",
            "sound",
        ],
    );
    let base_edit = videos.is_some_and(|videos| {
        videos.iter().any(|video| field(video, "refer_type").unwrap_or("base") == "base")
    });
    if !base_edit {
        duration(body, request);
        copy_fields(body, request, &["aspect_ratio"]);
    }
    body.insert("mode".into(), json!(mode));
    Ok(OMNI)
}
fn motion_body(request: &Value, body: &mut Map<String, Value>) -> ComponentResultV1<&'static str> {
    let image = field(request,"image_url").or_else(|| field(request,"image")).ok_or_else(|| invalid("Missing 'image_url' (character/subject reference image; required for kling-v3-motion-control)"))?;
    let video = field(request,"video_url").ok_or_else(|| invalid("Missing 'video_url' (motion reference clip, 3-30s; required for kling-v3-motion-control)"))?;
    let orientation = field(request,"character_orientation").ok_or_else(|| invalid("Missing 'character_orientation' (\"image\" or \"video\"; required for kling-v3-motion-control)"))?;
    body.insert("image_url".into(), json!(image));
    body.insert("video_url".into(), json!(video));
    body.insert("character_orientation".into(), json!(orientation));
    body.insert("mode".into(), json!("pro"));
    copy_fields(body, request, &["prompt", "element_list", "keep_original_sound"]);
    Ok(MOTION)
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ARTIFACT_REF_BYTES
        && !matches!(id, "." | "..")
        && !id.chars().any(char::is_control)
}
/// Encodes an original identifier as exactly one path segment. No persisted
/// identifier is rewritten, and dot segments are rejected before this step.
fn id_segment(id: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(id.len());
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    encoded
}
fn route_url(config: &ProviderConfig, route: &str) -> String {
    format!("{}/{route}", config.base_url.as_str().trim_end_matches('/'))
}
/// Missing/null is absent. A present invalid fact cannot silently become absent.
fn numeric_fact(value: Option<&Value>) -> Result<Option<f64>, ()> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let number = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
        .ok_or(())?;
    if number.is_finite() && number >= 0.0 { Ok(Some(number)) } else { Err(()) }
}
#[expect(
    clippy::cast_possible_truncation,
    reason = "the rounded nonnegative value is bounded below i64::MAX before conversion"
)]
fn milliunits(value: Option<&Value>) -> Result<Option<i64>, ()> {
    let Some(units) = numeric_fact(value)? else {
        return Ok(None);
    };
    let milli = (units * 1000.0).round();
    if milli > MAX_MILLIUNITS {
        return Err(());
    }
    Ok(Some(milli as i64))
}
fn succeeded(data: &Value) -> TaskObservationV2 {
    let Some(videos) =
        data.get("task_result").and_then(|value| value.get("videos")).and_then(Value::as_array)
    else {
        return unknown("kling success has invalid artifacts");
    };
    // Do not filter: dropping one entry changes the host's artifact indexes.
    let artifacts: Result<Vec<_>, ()> = videos
        .iter()
        .map(|video| {
            let url = field(video, "url").ok_or(())?;
            let id = task_v2_json::scalar_from_json(video.get("id").unwrap_or(&Value::Null))
                .map_err(|_| ())?;
            let duration =
                task_v2_json::scalar_from_json(video.get("duration").unwrap_or(&Value::Null))
                    .map_err(|_| ())?;
            TaskArtifactV2::new(url, id, duration).map_err(|_| ())
        })
        .collect();
    let Ok(artifacts) = artifacts.and_then(|items| TaskArtifactRefV2::urls(items).map_err(|_| ()))
    else {
        return unknown("kling success has invalid artifacts");
    };
    let seconds = numeric_fact(videos.first().and_then(|video| video.get("duration")));
    let units = milliunits(data.get("final_unit_deduction"));
    let (Ok(seconds), Ok(units)) = (seconds, units) else {
        return unknown("kling success has invalid usage");
    };
    let Ok(usage) = TaskUsageFactsV2::new(seconds, units, None) else {
        return unknown("kling success has invalid usage");
    };
    TaskObservationV2::Succeeded { artifacts, usage }
}
// Protocol resource-pack units per second, not a host monetary price. This
// managed request basis deliberately tests field presence, including null/[];
// submit validation separately tests whether the reference list is nonempty.
fn request_unit_rate(model: &str, route: &str, body: &Value) -> Option<i64> {
    let mode = field(body, "mode").unwrap_or("std");
    let sound = field(body, "sound") == Some("on");
    let reference = body.get("video_list").is_some();
    match (model, route == MOTION) {
        ("kling-v3", true) => match mode {
            "std" => Some(900),
            "pro" => Some(1200),
            _ => None,
        },
        ("kling-v3", false) if !reference => match (mode, sound) {
            ("std", false) => Some(600),
            ("std", true) => Some(900),
            ("pro", false) => Some(800),
            ("pro", true) => Some(1200),
            ("4k", _) => Some(3000),
            _ => None,
        },
        ("kling-v3-omni", false) => match (mode, sound, reference) {
            ("std", false, false) => Some(600),
            ("std", true, false) | ("pro", false, false) => Some(800),
            ("std", false, true) => Some(900),
            ("pro", true, false) => Some(1000),
            ("pro", false, true) => Some(1200),
            ("4k", _, false) => Some(3000),
            _ => None,
        },
        ("kling-video-o1", false) if !sound => match (mode, reference) {
            ("std", false) => Some(600),
            ("std", true) => Some(900),
            ("pro", false) => Some(800),
            ("pro", true) => Some(1200),
            _ => None,
        },
        _ => None,
    }
}
impl TaskComponentV2 for KlingTaskReferenceV2 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "task-kling-v2".into(),
            version: "0.31.0".into(),
            api_version: "task-adapter-v2".into(),
        }
    }
    fn build_submit_request(
        &self,
        config: &ProviderConfig,
        request: &Value,
        minted: &HostMintedValuesV1,
    ) -> ComponentResultV1<PreparedTaskV2> {
        let model = field(request, "model")
            .ok_or_else(|| invalid("a kling task request must name a model"))?;
        let operation = field(request, "operation")
            .ok_or_else(|| invalid("a kling task request must name an operation"))?;
        let mut body = Map::new();
        body.insert("model_name".into(), json!(model));
        let route = match operation {
            "text-or-image" => text_body(request, &mut body, minted)?,
            "omni" => omni_body(request, &mut body)?,
            "motion-control" => motion_body(request, &mut body)?,
            _ => return Err(invalid("unsupported kling task operation")),
        };
        let mut descriptor = HttpRequestDescriptor::new(HttpMethod::Post, route_url(config, route));
        descriptor.headers = SafeHeaders::try_new([("content-type", "application/json")])
            .map_err(|_| internal("invalid kling request headers"))?;
        let body = Value::Object(body);
        let requested_seconds = numeric_fact(body.get("duration"))
            .map_err(|()| invalid("invalid kling request duration"))?;
        let request_estimate = south_contracts::TaskRequestEstimateV2::new(
            requested_seconds,
            request_unit_rate(model, route, &body),
        )
        .map_err(|_| invalid("invalid kling request estimate"))?;
        descriptor.body = Some(body);
        descriptor.auth = config.auth.clone().map(Auth::bearer);
        let locator =
            TaskLocatorV2::new(1, route).map_err(|_| internal("invalid kling task locator"))?;
        Ok(PreparedTaskV2 { descriptor, locator, request_estimate })
    }
    fn parse_submit_response(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<SubmitOutcomeV2> {
        if !(200..300).contains(&parts.status) {
            return Ok(SubmitOutcomeV2::Unknown);
        }
        let Ok(body) = serde_json::from_str::<Value>(&parts.body) else {
            return Ok(SubmitOutcomeV2::Unknown);
        };
        let id = body
            .get("data")
            .and_then(|data| field(data, "task_id"))
            .filter(|id| valid_id(id))
            .or_else(|| field(&body, "task_id").filter(|id| valid_id(id)));
        Ok(id.map_or(SubmitOutcomeV2::Unknown, |id| SubmitOutcomeV2::Accepted(id.to_owned())))
    }
    fn build_observe_request(
        &self,
        config: &ProviderConfig,
        _upstream_model: &str,
        upstream_task_id: &str,
        locator: &TaskLocatorV2,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        if !matches!(locator.route(), TEXT | IMAGE | OMNI | MOTION) {
            return Err(invalid("unsupported kling task locator"));
        }
        if !valid_id(upstream_task_id) {
            return Err(invalid("invalid kling upstream task id"));
        }
        let mut descriptor = HttpRequestDescriptor::new(
            HttpMethod::Get,
            format!("{}/{}", route_url(config, locator.route()), id_segment(upstream_task_id)),
        );
        descriptor.auth = config.auth.clone().map(Auth::bearer);
        Ok(descriptor)
    }
    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV2> {
        if !(200..300).contains(&parts.status) {
            return Ok(unknown(format!("kling query http {}", parts.status)));
        }
        let Ok(body) = serde_json::from_str::<Value>(&parts.body) else {
            return Ok(unknown("kling poll body is not json"));
        };
        let data = body.get("data").unwrap_or(&body);
        let observation = match field(data, "task_status") {
            Some(word @ ("submitted" | "processing")) => TaskObservationV2::Progress {
                running: word == "processing",
                status_word: word.to_owned(),
            },
            Some("succeed") => succeeded(data),
            Some("failed") => TaskObservationV2::Failed {
                kind: TaskFailureKindV1::Failed,
                code: None,
                message: field(data, "task_status_msg").map(str::to_owned),
            },
            _ => unknown("kling task status is unrecognized"),
        };
        Ok(if observation.validate().is_ok() {
            observation
        } else {
            unknown("kling observation exceeds its bounds")
        })
    }
    fn build_artifact_request(
        &self,
        _config: &ProviderConfig,
        _locator: &TaskLocatorV2,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        observation.validate().map_err(|_| internal("invalid kling observation"))?;
        Ok(None)
    }
    fn render_success(
        &self,
        observation: &TaskObservationV2,
        _fetched: Option<&HttpResponseParts>,
        context: &TaskRenderContextV2,
    ) -> ComponentResultV1<Value> {
        observation.validate().map_err(|_| internal("invalid kling observation"))?;
        let TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::Urls(artifacts), .. } =
            observation
        else {
            return Err(internal("kling rendering requires successful direct artifacts"));
        };
        let upstream_id = context
            .upstream_task_id()
            .ok_or_else(|| internal("kling rendering requires the upstream task id"))?;
        let data:Result<Vec<_>,_>=artifacts.iter().map(|artifact|Ok(json!({"url":artifact.url(),"id":task_v2_json::scalar_json(artifact.id())?,"duration":task_v2_json::scalar_json(artifact.duration())?}))).collect::<Result<_,String>>();
        Ok(
            json!({"created":context.created(),"model":context.model(),"provider":context.provider(),"task_id":upstream_id,"data":data.map_err(|_|internal("invalid kling artifact scalar"))?}),
        )
    }
    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<ErrorEnvelope> {
        observation.validate().map_err(|_| internal("invalid kling observation"))?;
        let TaskObservationV2::Failed { message, .. } = observation else {
            return Err(internal("kling failure mapping requires a failed observation"));
        };
        Ok(internal(format!(
            "Kling video generation failed: {}",
            message.as_deref().unwrap_or("Video generation failed")
        )))
    }
}
