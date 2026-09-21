//! `MiniMax` Hailuo v1 and H3 v2 translation, transcribed from the managed host.
//! No price card, credential value, clock or I/O belongs to this component.
use crate::{ComponentResultV1, PreparedTaskV2, SubmitOutcomeV2, TaskComponentV2};
use serde_json::{Value, json};
use south_contracts::{
    HostMintedValuesV1, QueryParameterV1, QueryStringV1, TaskArtifactRefV2, TaskArtifactV2,
    TaskFailureKindV1, TaskLocatorV2, TaskObservationV2, TaskRenderContextV2,
    TaskRequestEstimateV2, TaskScalarV2, TaskUsageFactsV2,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{
    Auth, ErrorCode, ErrorEnvelope, HttpMethod, HttpRequestDescriptor, HttpResponseParts,
    ProviderConfig, SafeHeaders,
};
const QUERY: &str = "v1/query/video_generation";
const QUERY_V2: &str = "v2/query/video_generation";
/// Pure Hailuo and H3 reference; the saved locator selects the recovery API family.
#[derive(Debug, Default, Clone, Copy)]
pub struct MiniMaxTaskReferenceV2;
fn invalid(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, message)
}
fn protocol(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message)
}
fn unknown(reason: &str) -> TaskObservationV2 {
    TaskObservationV2::Unknown { reason: reason.into() }
}
fn field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}
fn identifier(value: &Value) -> Option<String> {
    let id = match value {
        Value::String(id) => id.trim().to_owned(),
        Value::Number(n) => n.as_u64()?.to_string(),
        _ => return None,
    };
    (!id.is_empty()
        && !matches!(id.as_str(), "." | "..")
        && !id.starts_with('-')
        && id.len() <= south_contracts::MAX_ARTIFACT_REF_BYTES
        && !id.chars().any(char::is_control))
    .then_some(id)
}
fn body(parts: &HttpResponseParts) -> Option<Value> {
    (200..300).contains(&parts.status).then(|| serde_json::from_str(&parts.body).ok()).flatten()
}
fn base_code(v: &Value) -> Result<i64, ()> {
    match v.get("base_resp") {
        None | Some(Value::Null) => Ok(0),
        Some(base) => match base.get("status_code") {
            None | Some(Value::Null) => Ok(0),
            Some(code) => code.as_i64().ok_or(()),
        },
    }
}
fn checked_locator(locator: &TaskLocatorV2) -> ComponentResultV1<()> {
    if matches!(locator.route(), QUERY | QUERY_V2) {
        Ok(())
    } else {
        Err(invalid("unsupported MiniMax locator"))
    }
}
fn request(
    config: &ProviderConfig,
    method: HttpMethod,
    path: &str,
    query: Option<(QueryParameterV1, &str)>,
) -> ComponentResultV1<HttpRequestDescriptor> {
    let mut url = format!("{}/{path}", config.base_url.as_str().trim_end_matches('/'));
    let mut parameters = Vec::new();
    if let Some(query) = query {
        parameters.push(query);
    }
    if let Some(group) = config.extensions.get("group_id").filter(|v| !v.is_null()) {
        let group = group.as_str().ok_or_else(|| invalid("invalid MiniMax group_id"))?.trim();
        if !group.is_empty() {
            parameters.push((QueryParameterV1::GroupId, group));
        }
    }
    if !parameters.is_empty() {
        let query = QueryStringV1::try_from_iter(parameters)
            .map_err(|_| invalid("invalid MiniMax query declaration"))?;
        url.push('?');
        url.push_str(query.as_str());
    }
    let mut descriptor = HttpRequestDescriptor::new(method, url);
    descriptor.auth = config.auth.clone().map(Auth::bearer);
    Ok(descriptor)
}
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "the whole-number float is explicitly bounded before conversion"
)]
fn seconds(value: Option<&Value>) -> ComponentResultV1<i64> {
    let Some(value) = value else { return Ok(6) };
    let parsed = value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
        .or_else(|| {
            let f = value
                .as_f64()
                .or_else(|| value.as_str().and_then(|s| s.trim().parse::<f64>().ok()))?;
            (f.is_finite() && f.fract() == 0.0 && f >= 0.0 && f < (i64::MAX as f64))
                .then_some(f as i64)
        });
    parsed.filter(|v| *v > 0).ok_or_else(|| invalid("invalid MiniMax duration"))
}
fn rejection(code: i64) -> ErrorEnvelope {
    let (kind, status, message) = match code {
        1004 | 2049 => (ErrorCode::Auth, 401, "MiniMax credentials were rejected"),
        2013 => (ErrorCode::InvalidRequest, 400, "MiniMax request was rejected"),
        1026 => (ErrorCode::ContentPolicy, 400, "MiniMax content policy rejected the request"),
        1002 => (ErrorCode::RateLimit, 429, "MiniMax rate limit exceeded"),
        1008 | 2153 => (ErrorCode::UpstreamUnavailable, 503, "MiniMax account is unavailable"),
        _ => (ErrorCode::ProviderProtocolError, 502, "MiniMax operation was rejected"),
    };
    ErrorEnvelope::new(kind, status, message)
}
fn http_error(status: u16) -> ErrorEnvelope {
    let kind = match status {
        401 | 403 => ErrorCode::Auth,
        429 => ErrorCode::RateLimit,
        400..=499 => ErrorCode::InvalidRequest,
        500..=599 => ErrorCode::UpstreamUnavailable,
        _ => ErrorCode::ProviderProtocolError,
    };
    ErrorEnvelope::new(kind, status, "MiniMax upstream HTTP request failed")
}
fn encode_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
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
#[expect(
    clippy::cast_precision_loss,
    reason = "the host validates its offered duration tiers before pricing"
)]
fn request_estimate(
    duration: i64,
    resolution: &str,
    images: u32,
) -> ComponentResultV1<TaskRequestEstimateV2> {
    TaskRequestEstimateV2::new(Some(duration as f64), None)
        .and_then(|estimate| estimate.with_input_facts(Some(resolution), Some(images)))
        .map_err(|_| invalid("invalid MiniMax request estimate"))
}
fn prepare_h3(
    config: &ProviderConfig,
    input: &Value,
    model: &str,
    shape: &str,
) -> ComponentResultV1<PreparedTaskV2> {
    for key in [
        "video",
        "video_url",
        "reference_video",
        "reference_videos",
        "audio_url",
        "reference_audio",
    ] {
        if input.get(key).is_some_and(|value| !value.is_null()) {
            return Err(invalid("MiniMax H3 input video and audio are not supported"));
        }
    }
    let prompt = field(input, "prompt")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid("MiniMax H3 requires a prompt"))?;
    let first = input
        .get("image")
        .or_else(|| input.get("image_url"))
        .or_else(|| input.get("first_frame_image"))
        .and_then(Value::as_str);
    let last = field(input, "last_frame");
    if last.is_some() && first.is_none() {
        return Err(invalid("MiniMax H3 last frame requires a first frame"));
    }
    let refs = match input.get("reference_images").filter(|value| !value.is_null()) {
        None => Vec::new(),
        Some(value) => {
            let values = value.as_array().filter(|values| values.len() <= 9).ok_or_else(|| {
                invalid("MiniMax H3 reference images require at most nine URL strings")
            })?;
            values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| invalid("MiniMax H3 reference image must be a URL string"))
                })
                .collect::<ComponentResultV1<Vec<_>>>()?
        }
    };
    if !refs.is_empty() && (first.is_some() || shape == "MiniMax-H3-Max") {
        return Err(invalid("MiniMax H3 reference image mode is incompatible with this request"));
    }
    let resolution = field(input, "resolution")
        .unwrap_or("768P")
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let resolution = match resolution.as_str() {
        "480" | "480p" => "480P",
        "768" | "768p" => "768P",
        "2k" => "2K",
        _ => return Err(invalid("unsupported MiniMax H3 resolution")),
    };
    let duration = match input.get("duration").filter(|value| !value.is_null()) {
        None => 5,
        Some(value) => seconds(Some(value))?,
    };
    let ratio = input
        .get("aspect_ratio")
        .or_else(|| input.get("ratio"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("16:9");
    if ratio.eq_ignore_ascii_case("adaptive") {
        return Err(invalid("MiniMax H3 does not support adaptive ratio"));
    }
    let mut content = vec![json!({"type":"text", "text":prompt})];
    let mut images = 0_u32;
    for (url, role) in first
        .into_iter()
        .map(|url| (url, "first_frame"))
        .chain(last.into_iter().map(|url| (url, "last_frame")))
        .chain(refs.into_iter().map(|url| (url, "reference_image")))
    {
        content.push(json!({"type":"image_url","image_url":{"url":url},"role":role}));
        images += 1;
    }
    let mut descriptor = request(config, HttpMethod::Post, "v2/video_generation", None)?;
    descriptor.body = Some(
        json!({"model":model,"content":content,"duration":duration,"resolution":resolution,"ratio":ratio}),
    );
    descriptor.headers = SafeHeaders::try_new([("content-type", "application/json")])
        .map_err(|_| protocol("invalid MiniMax headers"))?;
    Ok(PreparedTaskV2 {
        descriptor,
        locator: TaskLocatorV2::new(1, QUERY_V2)
            .map_err(|_| protocol("invalid MiniMax locator"))?,
        request_estimate: request_estimate(duration, resolution, images)?,
    })
}
fn observe_h3(task: &Value) -> TaskObservationV2 {
    match field(task, "status") {
        Some(word @ ("queued" | "running")) => {
            TaskObservationV2::Progress { running: word == "running", status_word: word.into() }
        }
        Some("succeeded") => {
            let Some(url) = task.get("content").and_then(|content| field(content, "url")) else {
                return unknown("MiniMax H3 success has no artifact URL");
            };
            let seconds = match task
                .get("usage")
                .and_then(|usage| usage.get("output_seconds"))
                .filter(|value| !value.is_null())
            {
                None => None,
                Some(value) => match value
                    .as_f64()
                    .or_else(|| value.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
                {
                    Some(value) if value.is_finite() && value >= 0.0 => Some(value),
                    _ => return unknown("MiniMax H3 usage is invalid"),
                },
            };
            let Ok(usage) = TaskUsageFactsV2::new(seconds, None, None) else {
                return unknown("MiniMax H3 usage is invalid");
            };
            let Ok(artifact) = TaskArtifactV2::new(url, TaskScalarV2::Null, TaskScalarV2::Null)
            else {
                return unknown("MiniMax H3 artifact is invalid");
            };
            TaskObservationV2::Succeeded {
                artifacts: TaskArtifactRefV2::Urls(vec![artifact]),
                usage,
            }
        }
        Some(word @ ("failed" | "cancelled")) => {
            let code = task.get("error").and_then(|error| error.get("code")).and_then(|value| {
                value.as_str().map(str::to_owned).or_else(|| value.as_i64().map(|n| n.to_string()))
            });
            let observation = TaskObservationV2::Failed {
                kind: if word == "cancelled" {
                    TaskFailureKindV1::Cancelled
                } else {
                    TaskFailureKindV1::Failed
                },
                code,
                message: Some("MiniMax video generation failed".into()),
            };
            if observation.validate().is_ok() {
                observation
            } else {
                unknown("MiniMax H3 failure is invalid")
            }
        }
        _ => unknown("MiniMax H3 task status is unrecognized"),
    }
}
fn render_url(url: &str, context: &TaskRenderContextV2) -> ComponentResultV1<Value> {
    TaskArtifactV2::new(url, TaskScalarV2::Null, TaskScalarV2::Null)
        .map_err(|_| protocol("MiniMax artifact URL is invalid"))?;
    let id = context
        .upstream_task_id()
        .ok_or_else(|| protocol("MiniMax rendering requires upstream task id"))?;
    Ok(
        json!({"created":context.created(),"model":context.model(),"provider":context.provider(),"task_id":id,"data":[{"url":url}]}),
    )
}
impl TaskComponentV2 for MiniMaxTaskReferenceV2 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "task-minimax-v2".into(),
            version: "0.31.0".into(),
            api_version: "task-adapter-v2".into(),
        }
    }
    fn build_submit_request(
        &self,
        config: &ProviderConfig,
        input: &Value,
        _minted: &HostMintedValuesV1,
    ) -> ComponentResultV1<PreparedTaskV2> {
        let model = field(input, "model").ok_or_else(|| invalid("missing MiniMax model"))?;
        let shape = match config.extensions.get("upstream_model_family").filter(|v| !v.is_null()) {
            None => model,
            Some(v) => v
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| invalid("invalid MiniMax upstream_model_family"))?,
        };
        if matches!(shape, "MiniMax-H3" | "MiniMax-H3-Max") {
            return prepare_h3(config, input, model, shape);
        }
        if !matches!(shape, "MiniMax-Hailuo-02" | "MiniMax-Hailuo-2.3" | "MiniMax-Hailuo-2.3-Fast")
        {
            return Err(invalid("unsupported MiniMax v1 model"));
        }
        let prompt = field(input, "prompt");
        let image = input
            .get("image")
            .or_else(|| input.get("image_url"))
            .or_else(|| input.get("first_frame_image"))
            .and_then(Value::as_str);
        let last =
            input.get("last_frame").or_else(|| input.get("last_frame_url")).and_then(Value::as_str);
        if prompt.is_none() && image.is_none() && last.is_none() {
            return Err(invalid("missing MiniMax prompt or image"));
        }
        if last.is_some() && shape != "MiniMax-Hailuo-02" {
            return Err(invalid("last frame requires MiniMax-Hailuo-02"));
        }
        let resolution = field(input, "resolution")
            .unwrap_or("768P")
            .chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect::<String>();
        let resolution = match resolution.as_str() {
            "512" | "512p" => "512P",
            "768" | "768p" => "768P",
            "1080" | "1080p" => "1080P",
            _ => return Err(invalid("unsupported MiniMax resolution")),
        };
        if last.is_some() && resolution == "512P" {
            return Err(invalid("last frame does not support 512P"));
        }
        let duration = seconds(input.get("duration"))?;
        let mut wire = json!({"model":model,"duration":duration,"resolution":resolution});
        for (key, value) in
            [("prompt", prompt), ("first_frame_image", image), ("last_frame_image", last)]
        {
            if let Some(value) = value {
                wire[key] = json!(value);
            }
        }
        for key in ["prompt_optimizer", "fast_pretreatment"] {
            if let Some(value) = input.get(key) {
                wire[key] = value.clone();
            }
        }
        let mut descriptor = request(config, HttpMethod::Post, "v1/video_generation", None)?;
        descriptor.body = Some(wire);
        descriptor.headers = SafeHeaders::try_new([("content-type", "application/json")])
            .map_err(|_| protocol("invalid MiniMax headers"))?;
        Ok(PreparedTaskV2 {
            descriptor,
            locator: TaskLocatorV2::new(1, QUERY)
                .map_err(|_| protocol("invalid MiniMax locator"))?,
            request_estimate: request_estimate(
                duration,
                resolution,
                u32::from(image.is_some()) + u32::from(last.is_some()),
            )?,
        })
    }
    fn parse_submit_response(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<SubmitOutcomeV2> {
        if (400..500).contains(&parts.status) {
            return Ok(SubmitOutcomeV2::Rejected(http_error(parts.status)));
        }
        let Some(value) = body(parts) else { return Ok(SubmitOutcomeV2::Unknown) };
        match base_code(&value) {
            Ok(0) => {}
            Ok(code) => {
                if value.get("task_id").and_then(identifier).is_some() {
                    return Ok(SubmitOutcomeV2::Unknown);
                }
                return Ok(SubmitOutcomeV2::Rejected(rejection(code)));
            }
            Err(()) => return Ok(SubmitOutcomeV2::Unknown),
        }
        Ok(value
            .get("task_id")
            .and_then(identifier)
            .map_or(SubmitOutcomeV2::Unknown, SubmitOutcomeV2::Accepted))
    }
    fn build_observe_request(
        &self,
        config: &ProviderConfig,
        _model: &str,
        id: &str,
        locator: &TaskLocatorV2,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        checked_locator(locator)?;
        if locator.route() == QUERY_V2 {
            if id.is_empty()
                || matches!(id, "." | "..")
                || id.len() > south_contracts::MAX_ARTIFACT_REF_BYTES
                || id.chars().any(char::is_control)
            {
                return Err(invalid("invalid MiniMax H3 task id"));
            }
            return request(
                config,
                HttpMethod::Get,
                &format!("{QUERY_V2}/{}", encode_segment(id)),
                None,
            );
        }
        QueryStringV1::try_from_iter([(QueryParameterV1::TaskId, id)])
            .map_err(|_| invalid("unsupported MiniMax task id query"))?;
        request(config, HttpMethod::Get, QUERY, Some((QueryParameterV1::TaskId, id)))
    }
    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV2> {
        let Some(value) = body(parts) else {
            return Ok(unknown("MiniMax query is not a successful JSON response"));
        };
        if let Some(task) = value.get("task") {
            return Ok(observe_h3(task));
        }
        if base_code(&value) != Ok(0) {
            return Ok(unknown("MiniMax query base response is invalid or nonzero"));
        }
        Ok(match field(&value, "status") {
            Some(word @ ("Preparing" | "Queueing" | "Processing")) => TaskObservationV2::Progress {
                running: word == "Processing",
                status_word: word.into(),
            },
            Some("Success") => value
                .get("file_id")
                .and_then(identifier)
                .and_then(|id| TaskArtifactRefV2::file_id(&id).ok())
                .map_or_else(
                    || unknown("MiniMax success has invalid file id"),
                    |artifacts| TaskObservationV2::Succeeded {
                        artifacts,
                        usage: TaskUsageFactsV2::default(),
                    },
                ),
            Some("Fail") => TaskObservationV2::Failed {
                kind: TaskFailureKindV1::Failed,
                code: None,
                message: Some("MiniMax video generation failed".into()),
            },
            _ => unknown("MiniMax task status is unrecognized"),
        })
    }
    fn build_artifact_request(
        &self,
        config: &ProviderConfig,
        locator: &TaskLocatorV2,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        checked_locator(locator)?;
        observation.validate().map_err(|_| protocol("invalid MiniMax observation"))?;
        let TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::FileId(id), .. } =
            observation
        else {
            return Ok(None);
        };
        if locator.route() != QUERY {
            return Err(protocol("MiniMax H3 does not use file artifacts"));
        }
        QueryStringV1::try_from_iter([(QueryParameterV1::FileId, id.as_str())])
            .map_err(|_| invalid("unsupported MiniMax file id query"))?;
        Ok(Some(request(
            config,
            HttpMethod::Get,
            "v1/files/retrieve",
            Some((QueryParameterV1::FileId, id)),
        )?))
    }
    fn render_success(
        &self,
        observation: &TaskObservationV2,
        fetched: Option<&HttpResponseParts>,
        context: &TaskRenderContextV2,
    ) -> ComponentResultV1<Value> {
        observation.validate().map_err(|_| protocol("invalid MiniMax observation"))?;
        if let TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::Urls(items), .. } =
            observation
        {
            if items.len() != 1 {
                return Err(protocol("MiniMax H3 requires one direct artifact"));
            }
            return render_url(items[0].url(), context);
        }
        if !matches!(
            observation,
            TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::FileId(_), .. }
        ) {
            return Err(protocol("MiniMax rendering requires successful file artifact"));
        }
        if let Some(parts) = fetched
            && !(200..300).contains(&parts.status)
        {
            return Err(http_error(parts.status));
        }
        let value = fetched
            .and_then(body)
            .ok_or_else(|| protocol("MiniMax rendering requires successful file retrieval"))?;
        match base_code(&value) {
            Ok(0) => {}
            Ok(code) => return Err(rejection(code)),
            Err(()) => return Err(protocol("MiniMax file retrieval has invalid base response")),
        }
        let url = value
            .get("file")
            .and_then(|v| field(v, "download_url"))
            .ok_or_else(|| protocol("MiniMax file retrieval has no download URL"))?;
        render_url(url, context)
    }
    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<ErrorEnvelope> {
        observation.validate().map_err(|_| protocol("invalid MiniMax observation"))?;
        if !matches!(observation, TaskObservationV2::Failed { .. }) {
            return Err(protocol("MiniMax failure mapping requires failed observation"));
        }
        if let TaskObservationV2::Failed { code: Some(code), .. } = observation {
            return Ok(code
                .parse::<i64>()
                .map_or_else(|_| protocol("MiniMax video generation failed"), rejection));
        }
        Ok(protocol("MiniMax video generation failed"))
    }
}
