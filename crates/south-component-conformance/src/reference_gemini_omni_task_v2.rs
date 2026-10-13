//! Pure Gemini Omni video translation over the Interactions API (task contract 8).
//!
//! Design record: `docs/design/2026-10-01-omni-task-component.md` (accepted 2026-10-13; provisional
//! items pending a real-account test). Two families, one wire:
//!
//! - **`vertex-interactions-video`** (sampled, O0-b and O0-c): `POST {base}/interactions` with
//!   `response_format.delivery = "inline"` and `background: true`, then `GET
//!   {base}/interactions/{id}` until `completed`. The base is the operator's `v1beta1` project
//!   endpoint; the credential is the minted service-account token, presented as a bearer. The clip
//!   arrives as standard base64 inside the JSON, which the host elides before this component sees
//!   it (`response_blob_paths`), so the component names it by pointer (an inline artifact).
//! - **`gemini-interactions-video`** (provisional: not sampled, transcribed from token-station-server's
//!   native Omni arm, `handler/video/gemini.rs` `build_omni_interaction_body` /
//!   `proxy_gemini_omni_video` / `extract_omni_video_data`): `POST {base}/v1beta/interactions` with
//!   `x-goog-api-key`, `response_format.delivery = "uri"`, no `background`, text-to-video only. The
//!   synchronous answer is already `completed`; its Files API `uri` is a credential-gated URL
//!   artifact the host fetches with the bound key, as Veo's are.
//!
//! Usage is reported as declared token buckets (contract 8): `total_input`, `total_output`,
//! `video_output` and `reasoning_output` on every success, `text_input` / `image_input` /
//! `cached_input` when the wire states them. A success that misses one, reports a modality outside
//! that set, or breaks the dialect's own sums is `unknown`, never a partial bill. Pricing,
//! persistence, time and remote-image fetching remain host responsibilities.
use crate::{ComponentResultV1, PreparedTaskV2, SubmitOutcomeV2, TaskComponentV2};
use serde_json::{Map, Value, json};
use south_contracts::image::ArtifactEncodingV1;
use south_contracts::media::BLOB_KEY;
use south_contracts::{
    HostMintedValuesV1, TaskArtifactRefV2, TaskArtifactV2, TaskInlineArtifactV2, TaskLocatorV2,
    TaskObservationV2, TaskRenderContextV2, TaskRequestEstimateV2, TaskScalarV2, TaskTokenBucketV2,
    TaskTokenBucketsV2, TaskUsageFactsV2,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{
    Auth, ErrorCode, ErrorEnvelope, HttpMethod, HttpRequestDescriptor, HttpResponseParts,
    ProviderConfig, SafeHeaders,
};

/// The sampled Vertex AI family.
pub const VERTEX_FAMILY: &str = "vertex-interactions-video";
/// The Gemini API family, transcribed from the host's native arm (provisional).
pub const GEMINI_FAMILY: &str = "gemini-interactions-video";
const VERTEX_ROUTE: &str = "interactions";
const GEMINI_ROUTE: &str = "v1beta/interactions";
const KEY_HEADER: &str = "x-goog-api-key";
/// Every Omni clip is 10 s; the upstream has no duration parameter (O0-b).
const CLIP_SECONDS: u64 = 10;
/// Video tokens per clip, identical in every sample (O0-b, O0-c): the component's ceiling, which
/// may only tighten the host's own bound (record §7.4).
const VIDEO_TOKENS_PER_CLIP: u64 = 57_920;
/// Image parts per request: twenty were accepted (O0-c); the documented limit is unknown (Q-L3),
/// so this is the measured floor, provisional.
const MAX_IMAGE_PARTS: usize = 20;
/// The aspect ratios Vertex accepts; anything else is a 400 before billing (O0-c).
const VERTEX_ASPECT_RATIOS: [&str; 2] = ["16:9", "9:16"];
/// Vertex reserves the whole response format and the answer-shape switches (record §5.2).
const VERTEX_IMMUTABLE: [&str; 5] = ["model", "input", "response_format", "background", "stream"];
/// The Gemini family keeps the native arm's reservation (`GEMINI_OMNI_VIDEO_OWNED`), so an
/// operator extra may still set `response_format.aspect_ratio`, plus the answer-shape switches.
const GEMINI_IMMUTABLE: [&str; 6] =
    ["model", "input", "response_format.type", "response_format.delivery", "background", "stream"];
/// Every visual input key the native arm refuses on the Gemini family.
const GEMINI_VISUAL_KEYS: [&str; 6] =
    ["image", "image_url", "first_frame", "last_frame", "last_frame_url", "reference_images"];

/// The Omni Interactions video dialect; no credentials, clock, price or persistence.
#[derive(Debug, Default, Clone, Copy)]
pub struct GeminiOmniTaskComponentV2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Vertex,
    Gemini,
}
impl Family {
    fn of(config: &ProviderConfig) -> ComponentResultV1<Self> {
        match config.provider.as_str() {
            VERTEX_FAMILY => Ok(Self::Vertex),
            GEMINI_FAMILY => Ok(Self::Gemini),
            _ => Err(invalid("unsupported Interactions video family")),
        }
    }
    const fn route(self) -> &'static str {
        match self {
            Self::Vertex => VERTEX_ROUTE,
            Self::Gemini => GEMINI_ROUTE,
        }
    }
}

fn invalid(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, message)
}
fn capability(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Capability, 400, message)
}
fn protocol(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message)
}
fn unknown(reason: &str) -> TaskObservationV2 {
    TaskObservationV2::Unknown { reason: reason.into() }
}
fn field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}
/// A present, non-null member; `null` reads as absent, as a JSON client sends it.
fn present<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|value| !value.is_null())
}
fn bounded(text: Option<String>) -> Option<String> {
    text.filter(|t| t.len() <= south_contracts::MAX_ARTIFACT_REF_BYTES)
}
/// Measured ids are 40 ASCII letters and digits; base64url characters are admitted too (record
/// §4.4). Anything else is not an id this component will put in a URL.
fn interaction_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= south_contracts::MAX_TASK_ID_BYTES
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
fn request(
    config: &ProviderConfig,
    family: Family,
    method: HttpMethod,
    path: &str,
) -> HttpRequestDescriptor {
    let mut descriptor = HttpRequestDescriptor::new(
        method,
        format!("{}/{path}", config.base_url.as_str().trim_end_matches('/')),
    );
    descriptor.auth = config.auth.clone().map(|secret| match family {
        Family::Vertex => Auth::Bearer { secret },
        Family::Gemini => Auth::Header { name: KEY_HEADER.into(), secret },
    });
    descriptor
}
/// `data:<mime>;base64,<payload>` → the Interactions image part.
fn image_part(key: &str, value: &Value) -> ComponentResultV1<Value> {
    let parsed = value
        .as_str()
        .and_then(|text| text.strip_prefix("data:"))
        .and_then(|rest| rest.split_once(";base64,"))
        .filter(|(mime, payload)| mime.starts_with("image/") && !payload.is_empty());
    let Some((mime, payload)) = parsed else {
        return Err(invalid(&format!(
            "Omni needs '{key}' as inline image bytes (a data: URI); configure the host to \
             pre-fetch this field for the model"
        )));
    };
    Ok(json!({"type": "image", "mime_type": mime, "data": payload}))
}
/// A whole number of seconds, as a JSON number or a numeric string.
fn whole(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

/// The Vertex request's `input`, refusals and image count (record §5).
fn vertex_input(body: &Value, prompt: &str) -> ComponentResultV1<(Value, u32)> {
    if ["last_frame", "last_frame_url", "video", "video_url"]
        .iter()
        .any(|key| present(body, key).is_some())
    {
        return Err(capability(
            "Omni accepts a first frame and reference images; last-frame and video inputs are \
             not supported",
        ));
    }
    if present(body, "duration").is_some_and(|raw| whole(raw) != Some(CLIP_SECONDS)) {
        return Err(capability("Omni always generates a 10-second clip; 'duration' must be 10"));
    }
    if present(body, "n").is_some_and(|raw| whole(raw) != Some(1)) {
        return Err(capability("Omni generates one clip per request; 'n' must be 1"));
    }
    let mut parts = Vec::new();
    if let Some((key, first)) = ["image", "image_url", "first_frame"]
        .into_iter()
        .find_map(|key| present(body, key).map(|value| (key, value)))
    {
        parts.push(image_part(key, first)?);
    }
    if let Some(references) = present(body, "reference_images") {
        let items = references
            .as_array()
            .ok_or_else(|| invalid("'reference_images' must be an array of image strings"))?;
        for item in items {
            parts.push(image_part("reference_images", item)?);
        }
    }
    if parts.len() > MAX_IMAGE_PARTS {
        return Err(invalid("Omni accepts at most 20 images per request"));
    }
    let count = u32::try_from(parts.len()).map_err(|_| invalid("too many images"))?;
    if parts.is_empty() {
        return Ok((json!(prompt), 0));
    }
    parts.insert(0, json!({"type": "text", "text": prompt}));
    Ok((Value::Array(parts), count))
}

/// The reported token buckets (record §7.1), or why the usage is not billable.
fn token_buckets(body: &Value) -> Result<TaskTokenBucketsV2, &'static str> {
    let usage = present(body, "usage").ok_or("interaction carries no usage")?;
    let count = |key: &str| {
        present(usage, key)
            .map(|value| value.as_u64().ok_or("a usage count is not a whole number"))
            .transpose()
    };
    let total_input = count("total_input_tokens")?.ok_or("usage misses total_input_tokens")?;
    let total_output = count("total_output_tokens")?.ok_or("usage misses total_output_tokens")?;
    let thoughts = count("total_thought_tokens")?.ok_or("usage misses total_thought_tokens")?;
    let mut entries = vec![
        (TaskTokenBucketV2::TotalInput, total_input),
        (TaskTokenBucketV2::TotalOutput, total_output),
        (TaskTokenBucketV2::ReasoningOutput, thoughts),
    ];
    let modalities = |key: &str, allowed: &[(&str, TaskTokenBucketV2)]| {
        let Some(items) = present(usage, key) else { return Ok(None) };
        let items = items.as_array().ok_or("a modality breakdown is not an array")?;
        let mut seen: Vec<(TaskTokenBucketV2, u64)> = Vec::new();
        for item in items {
            let modality = field(item, "modality").ok_or("a modality entry names no modality")?;
            let bucket = allowed
                .iter()
                .find(|(word, _)| *word == modality)
                .map(|(_, bucket)| *bucket)
                .ok_or("a modality is outside the declared buckets")?;
            let tokens = present(item, "tokens")
                .and_then(Value::as_u64)
                .ok_or("a modality entry carries no whole token count")?;
            if seen.iter().any(|(earlier, _)| *earlier == bucket) {
                return Err("a modality is reported twice");
            }
            seen.push((bucket, tokens));
        }
        Ok(Some(seen))
    };
    let input = modalities(
        "input_tokens_by_modality",
        &[("text", TaskTokenBucketV2::TextInput), ("image", TaskTokenBucketV2::ImageInput)],
    )?;
    let output =
        modalities("output_tokens_by_modality", &[("video", TaskTokenBucketV2::VideoOutput)])?
            .filter(|items| !items.is_empty())
            .ok_or("usage reports no video output tokens")?;
    let sum = |items: &[(TaskTokenBucketV2, u64)]| {
        items.iter().try_fold(0_u64, |total, (_, tokens)| total.checked_add(*tokens))
    };
    if let Some(input) = input.filter(|items| !items.is_empty()) {
        if sum(&input) != Some(total_input) {
            return Err("input modalities do not sum to total_input_tokens");
        }
        entries.extend(input);
    }
    if sum(&output) != Some(total_output) {
        return Err("output modalities do not sum to total_output_tokens");
    }
    entries.extend(output);
    if let Some(cached) = count("total_cached_tokens")? {
        if cached > total_input {
            return Err("cached tokens exceed total_input_tokens");
        }
        entries.push((TaskTokenBucketV2::CachedInput, cached));
    }
    if let Some(total) = count("total_tokens")?
        && total_input.checked_add(total_output).and_then(|sum| sum.checked_add(thoughts))
            != Some(total)
    {
        return Err("total_tokens is not input + output + thought tokens");
    }
    TaskTokenBucketsV2::new(entries).map_err(|_| "usage counts exceed the contract bounds")
}

/// One video part, as a URL artifact (`uri`) or an inline artifact (an elided `data`).
fn video_artifact(part: &Value, pointer: &str) -> Result<TaskArtifactRefV2, &'static str> {
    if let Some(uri) = field(part, "uri").filter(|uri| !uri.is_empty()) {
        let artifact = TaskArtifactV2::new(uri, TaskScalarV2::Null, TaskScalarV2::Null)
            .map_err(|_| "the video uri exceeds the artifact bounds")?
            .with_bound_credential();
        return TaskArtifactRefV2::urls(vec![artifact]).map_err(|_| "invalid video artifact");
    }
    let data = part.get("data").ok_or("the video part carries neither uri nor data")?;
    if !data.as_object().is_some_and(|node| node.len() == 1 && node.contains_key(BLOB_KEY)) {
        return Err("the inline video was not elided by the host");
    }
    let media_type = field(part, "mime_type")
        .filter(|mime| mime.starts_with("video/"))
        .ok_or("the inline video names no video media type")?;
    let artifact = TaskInlineArtifactV2::new(pointer, ArtifactEncodingV1::Base64, media_type)
        .map_err(|_| "invalid inline video artifact")?;
    TaskArtifactRefV2::inline(vec![artifact]).map_err(|_| "invalid inline video artifact")
}

/// The generated video: the convenience `output_video` first, as the native arm reads it, else
/// the one video part among the steps (input echoes are `user_input` steps and are skipped by type).
fn find_video(body: &Value) -> Result<TaskArtifactRefV2, &'static str> {
    // As the native arm: `output_video` counts only when it carries a non-empty `uri` or a `data`.
    if let Some(output) = present(body, "output_video").filter(|output| {
        field(output, "uri").is_some_and(|uri| !uri.is_empty()) || output.get("data").is_some()
    }) {
        return video_artifact(output, "/output_video/data");
    }
    let mut found = None;
    for (step_index, step) in
        body.get("steps").and_then(Value::as_array).into_iter().flatten().enumerate()
    {
        if field(step, "type") == Some("user_input") {
            continue;
        }
        let content = step.get("content").and_then(Value::as_array).into_iter().flatten();
        for (part_index, part) in content.enumerate() {
            if field(part, "type") != Some("video") {
                continue;
            }
            if found.is_some() {
                return Err("the interaction carries more than one video");
            }
            found = Some(video_artifact(
                part,
                &format!("/steps/{step_index}/content/{part_index}/data"),
            )?);
        }
    }
    found.ok_or("completed interaction carries no video")
}

/// The reading of one interaction body, shared by the submit and poll paths.
fn observe_interaction(body: &Value) -> TaskObservationV2 {
    match field(body, "status") {
        Some("in_progress") => {
            TaskObservationV2::Progress { running: true, status_word: "in_progress".into() }
        }
        Some("completed") => {
            let (artifacts, buckets) = match (find_video(body), token_buckets(body)) {
                (Ok(artifacts), Ok(buckets)) => (artifacts, buckets),
                (Err(reason), _) | (_, Err(reason)) => return unknown(reason),
            };
            TaskUsageFactsV2::new(None, None, None)
                .and_then(|usage| usage.with_outputs(Some(1)))
                .and_then(|usage| usage.with_token_buckets(Some(buckets)))
                .map_or_else(
                    |_| unknown("usage exceeds the contract bounds"),
                    |usage| TaskObservationV2::Succeeded { artifacts, usage },
                )
        }
        // Only `in_progress` and `completed` were observed; a failure or refusal word is not
        // known yet (Q-L2), so it is never turned into a terminal failure here.
        _ => unknown("interaction status is not recognized"),
    }
}

/// The upstream's own error message: a JSON `error` object, or the first SSE `event: error`
/// frame, which arrived under `application/json` in the samples (record §8.1).
fn error_message(body: &str) -> Option<String> {
    let message = |value: &Value| {
        value.get("error").and_then(|error| field(error, "message")).map(str::to_owned)
    };
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        return bounded(message(&value));
    }
    let normalized = body.replace("\r\n", "\n");
    for frame in normalized.split("\n\n") {
        let mut event = None;
        let mut data = String::new();
        for line in frame.lines() {
            if let Some(rest) = line.strip_prefix("event:") {
                event = Some(rest.trim());
            } else if let Some(rest) = line.strip_prefix("data:") {
                data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            }
        }
        if event == Some("error") {
            return serde_json::from_str::<Value>(&data)
                .ok()
                .and_then(|value| bounded(message(&value)));
        }
    }
    None
}

impl TaskComponentV2 for GeminiOmniTaskComponentV2 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "task-gemini-omni-v2".into(),
            version: "0.1.0".into(),
            api_version: "task-adapter-v2".into(),
        }
    }
    fn build_submit_request(
        &self,
        config: &ProviderConfig,
        body: &Value,
        _: &HostMintedValuesV1,
    ) -> ComponentResultV1<PreparedTaskV2> {
        let family = Family::of(config)?;
        let model = field(body, "model")
            .filter(|m| {
                !m.is_empty() && m.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
            .ok_or_else(|| invalid("missing or invalid Omni model"))?;
        let prompt = field(body, "prompt").ok_or_else(|| invalid("Missing 'prompt' field"))?;
        let aspect_ratio = field(body, "aspect_ratio");
        let mut response_format = Map::new();
        response_format.insert("type".into(), json!("video"));
        let (input, images, immutable) = match family {
            Family::Vertex => {
                if aspect_ratio.is_some_and(|ratio| !VERTEX_ASPECT_RATIOS.contains(&ratio)) {
                    return Err(capability("Omni accepts the aspect ratios 16:9 and 9:16 only"));
                }
                response_format.insert("delivery".into(), json!("inline"));
                let (input, images) = vertex_input(body, prompt)?;
                (input, images, VERTEX_IMMUTABLE.as_slice())
            }
            Family::Gemini => {
                if GEMINI_VISUAL_KEYS.iter().any(|key| body.get(*key).is_some()) {
                    return Err(invalid(
                        "image / frame / reference inputs are not supported for Omni on the \
                         Gemini API line (text-to-video only)",
                    ));
                }
                response_format.insert("delivery".into(), json!("uri"));
                (json!(prompt), 0, GEMINI_IMMUTABLE.as_slice())
            }
        };
        if let Some(ratio) = aspect_ratio {
            response_format.insert("aspect_ratio".into(), json!(ratio));
        }
        let mut upstream =
            json!({"model": model, "input": input, "response_format": response_format});
        if family == Family::Vertex {
            upstream["background"] = json!(true);
        }
        let ceilings =
            TaskTokenBucketsV2::new([(TaskTokenBucketV2::VideoOutput, VIDEO_TOKENS_PER_CLIP)])
                .map_err(|_| protocol("invalid Omni request estimate"))?;
        #[expect(clippy::cast_precision_loss, reason = "the clip length is ten whole seconds")]
        let seconds = CLIP_SECONDS as f64;
        let request_estimate = TaskRequestEstimateV2::new(Some(seconds), None)
            .and_then(|estimate| estimate.with_input_facts(None, Some(images)))
            .and_then(|estimate| estimate.with_output_facts(None, Some(1)))
            .map(|estimate| estimate.with_bucket_ceilings(Some(ceilings)))
            .map_err(|_| protocol("invalid Omni request estimate"))?;
        let mut descriptor = request(config, family, HttpMethod::Post, family.route());
        descriptor.headers = SafeHeaders::try_new([("content-type", "application/json")])
            .map_err(|_| protocol("invalid Omni headers"))?;
        descriptor.body = Some(upstream);
        Ok(PreparedTaskV2 {
            descriptor,
            locator: TaskLocatorV2::new(1, family.route())
                .map_err(|_| protocol("invalid Omni locator"))?,
            request_estimate,
            immutable_body_paths: Some(immutable.iter().map(|path| (*path).to_owned()).collect()),
        })
    }
    fn parse_submit_response(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<SubmitOutcomeV2> {
        if (400..500).contains(&parts.status) {
            let code = match parts.status {
                401 | 403 => ErrorCode::Auth,
                429 => ErrorCode::RateLimit,
                _ => ErrorCode::InvalidRequest,
            };
            let message = error_message(&parts.body)
                .unwrap_or_else(|| "Interactions request rejected".into());
            return Ok(SubmitOutcomeV2::Rejected(ErrorEnvelope::new(code, parts.status, message)));
        }
        if !(200..300).contains(&parts.status) {
            return Ok(SubmitOutcomeV2::Unknown);
        }
        let Ok(body) = serde_json::from_str::<Value>(&parts.body) else {
            return Ok(SubmitOutcomeV2::Unknown);
        };
        let id = field(&body, "id").filter(|id| interaction_id(id));
        if field(&body, "status") == Some("completed") {
            // A URL artifact can be settled from here (the Gemini family's synchronous answer);
            // inline bytes cannot (contract 8), so a stored interaction is polled for them.
            let observation = observe_interaction(&body);
            if matches!(
                observation,
                TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::Urls(_), .. }
            ) {
                return Ok(SubmitOutcomeV2::AcceptedTerminal(observation));
            }
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
        let family = Family::of(config)?;
        if locator.route() != family.route() {
            return Err(invalid("unsupported Omni locator"));
        }
        if !interaction_id(id) {
            return Err(invalid("invalid Omni interaction id"));
        }
        let path = format!("{}/{}", family.route(), crate::url_segment::encode(id));
        Ok(request(config, family, HttpMethod::Get, &path))
    }
    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV2> {
        if !(200..300).contains(&parts.status) {
            return Ok(unknown("Omni observation HTTP failure"));
        }
        let Ok(body) = serde_json::from_str::<Value>(&parts.body) else {
            return Ok(unknown("invalid Omni observation JSON"));
        };
        Ok(observe_interaction(&body))
    }
    fn build_artifact_request(
        &self,
        _: &ProviderConfig,
        locator: &TaskLocatorV2,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        if ![VERTEX_ROUTE, GEMINI_ROUTE].contains(&locator.route()) {
            return Err(invalid("unsupported Omni locator"));
        }
        observation.validate().map_err(|_| protocol("invalid Omni observation"))?;
        Ok(None)
    }
    fn render_success(
        &self,
        observation: &TaskObservationV2,
        _: Option<&HttpResponseParts>,
        context: &TaskRenderContextV2,
    ) -> ComponentResultV1<Value> {
        observation.validate().map_err(|_| protocol("invalid Omni observation"))?;
        let TaskObservationV2::Succeeded { artifacts, .. } = observation else {
            return Err(protocol("Omni render requires a succeeded observation"));
        };
        let data: Vec<Value> = match artifacts {
            // The host replaces each marker with its own artifact path (`art_i`).
            TaskArtifactRefV2::Inline(items) => {
                (0..items.iter().filter(|i| i.role().is_primary()).count())
                    .map(|index| json!({"url": {"$south.artifact": {"index": index}}}))
                    .collect()
            }
            // Credential-gated URLs: the host rewrites them to its proxy paths (contract 6).
            TaskArtifactRefV2::Urls(items) => items
                .iter()
                .filter(|item| item.role().is_primary())
                .map(|item| json!({"url": item.url()}))
                .collect(),
            TaskArtifactRefV2::FileId(_) | TaskArtifactRefV2::None => {
                return Err(protocol("Omni render requires a video artifact"));
            }
        };
        Ok(json!({"created": context.created(), "data": data}))
    }
    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<ErrorEnvelope> {
        observation.validate().map_err(|_| protocol("invalid Omni observation"))?;
        let TaskObservationV2::Failed { message, .. } = observation else {
            return Err(protocol("Omni failure mapping requires a failed observation"));
        };
        Ok(ErrorEnvelope::new(
            ErrorCode::ProviderProtocolError,
            502,
            format!(
                "Video generation failed: {}",
                message.as_deref().unwrap_or("Video generation failed")
            ),
        ))
    }
}
