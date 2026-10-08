//! Pure Google Veo (Gemini API line) video translation.
//!
//! Transcribed from the host's native Veo arm (token-station-server `handler/video/gemini.rs`
//! `build_veo_submit_body` / `build_veo_instance`, `durable.rs::parse_veo_create` / `veo_run`,
//! `observe.rs::normalize_veo` and the Veo failure wording). Credentials, pricing, persistence,
//! time and remote-image fetching remain host responsibilities.
//!
//! What this family needs from task contract 6, and why it could not move before:
//! - **Header credential**: the Gemini API authenticates with `x-goog-api-key`, so every
//!   descriptor asks for the credential in that header, never as a bearer token.
//! - **Credential-gated artifacts**: a generated sample's `video.uri` downloads only with the
//!   same key, so every artifact is `fetch_with_credential`. The rendered body keeps the native
//!   minimal shape `{created, data: [{url}]}`; a host must replace those URLs with its own proxy
//!   paths before a client sees them (contract 6, D5) — the component cannot know the host's
//!   routes, and the upstream URI is useless to a client anyway.
//! - **Counts**: `sampleCount` is the requested output count; the delivered sample count is the
//!   reported `outputs`. Veo reports no duration, so settlement multiplies the requested seconds
//!   by delivered outputs on the host side.
//! - **Finish at submit**: a create response that is already a terminal operation (no `name`) is
//!   `accepted-terminal`.
//!
//! Differences from the host-native arm, deliberate:
//! - Input images must already be `data:` URIs (the host's pre-fetch inlines remote images);
//!   Veo only accepts inline bytes, and a component has no network. A plain URL is refused with
//!   a pointer to that configuration instead of being silently dropped.
//! - An invalid `durationSeconds` (not a positive integer) is refused here; the native arm drops
//!   it and lets the upstream default apply, which reserves for a length the caller never asked for.
//! - A request without a duration reports no requested seconds (the upstream default applies);
//!   the host's default-duration policy decides the reservation.
//! - Vertex operation names (`projects/…`) are refused: that line needs a minted service-account
//!   token and a region-derived endpoint, neither of which this component can express.
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

const QUERY: &str = "v1beta";
const KEY_HEADER: &str = "x-goog-api-key";
/// Veo 3.1 accepts at most three asset references.
const MAX_REFERENCE_IMAGES: usize = 3;
/// Request-body paths the host must not rewrite (contract 6): the prompt and images, and every
/// parameter the reservation and settlement read, plus the callback slot.
const IMMUTABLE_BODY_PATHS: [&str; 5] = [
    "instances",
    "parameters.durationSeconds",
    "parameters.sampleCount",
    "parameters.resolution",
    "webhookConfig",
];

/// Managed Veo video dialect on the Gemini API; no credentials, clock, price or persistence.
#[derive(Debug, Default, Clone, Copy)]
pub struct VeoTaskComponentV2;

fn invalid(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, message)
}
fn protocol(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message)
}
fn field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}
fn bounded(text: Option<String>) -> Option<String> {
    text.filter(|t| t.len() <= south_contracts::MAX_ARTIFACT_REF_BYTES)
}
/// An operation name the Gemini API polls under `v1beta/`: `models/<m>/operations/<id>` or
/// `operations/<id>`. Every segment is a plain identifier, so it is used as a path verbatim.
fn operation_name(name: &str) -> bool {
    let segment = |s: &str| {
        !s.is_empty()
            && !matches!(s, "." | "..")
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    name.len() <= south_contracts::MAX_TASK_ID_BYTES
        && !name.starts_with("projects/")
        && (name.starts_with("models/") || name.starts_with("operations/"))
        && name.split('/').all(segment)
}
fn request(config: &ProviderConfig, method: HttpMethod, path: &str) -> HttpRequestDescriptor {
    let mut descriptor = HttpRequestDescriptor::new(
        method,
        format!("{}/{path}", config.base_url.as_str().trim_end_matches('/')),
    );
    descriptor.auth =
        config.auth.clone().map(|secret| Auth::Header { name: KEY_HEADER.into(), secret });
    descriptor
}
fn checked_locator(locator: &TaskLocatorV2) -> ComponentResultV1<()> {
    if locator.route() == QUERY { Ok(()) } else { Err(invalid("unsupported Veo locator")) }
}
fn unknown(reason: &str) -> TaskObservationV2 {
    TaskObservationV2::Unknown { reason: reason.into() }
}
/// `data:<mime>;base64,<payload>` → Veo's flat `{bytesBase64Encoded, mimeType}` image part.
fn inline_image(key: &str, value: &str) -> ComponentResultV1<Value> {
    let parsed = value.strip_prefix("data:").and_then(|rest| rest.split_once(";base64,"));
    let Some((mime, payload)) = parsed.filter(|(m, p)| !m.is_empty() && !p.is_empty()) else {
        return Err(invalid(&format!(
            "Veo needs '{key}' as inline image bytes (a data: URI); configure the host to \
             pre-fetch this field for the model"
        )));
    };
    Ok(json!({"bytesBase64Encoded": payload, "mimeType": mime}))
}
/// The terminal-or-not reading of one operation body, shared by the create and poll paths.
fn observe_operation(body: &Value) -> TaskObservationV2 {
    if !body.get("done").and_then(Value::as_bool).unwrap_or(false) {
        return TaskObservationV2::Progress { running: true, status_word: "processing".into() };
    }
    if let Some(error) = body.get("error") {
        return TaskObservationV2::Failed {
            kind: TaskFailureKindV1::Failed,
            code: bounded(error.get("code").map(Value::to_string)),
            message: bounded(field(error, "message").map(str::to_owned)),
        };
    }
    let response = body
        .get("response")
        .or_else(|| body.get("result"))
        .or_else(|| body.get("predictions"))
        .unwrap_or(&Value::Null);
    let generated = response.get("generateVideoResponse");
    let artifacts: Vec<TaskArtifactV2> = generated
        .and_then(|r| r.get("generatedSamples"))
        .and_then(Value::as_array)
        .map(|samples| {
            samples
                .iter()
                .filter_map(|s| s.get("video").and_then(|v| field(v, "uri")))
                .filter(|uri| !uri.is_empty())
                .filter_map(|uri| {
                    TaskArtifactV2::new(uri, TaskScalarV2::Null, TaskScalarV2::Null)
                        .ok()
                        .map(TaskArtifactV2::with_bound_credential)
                })
                .collect()
        })
        .unwrap_or_default();
    if !artifacts.is_empty() {
        let count = i64::try_from(artifacts.len()).unwrap_or(i64::MAX);
        let (Ok(artifacts), Ok(usage)) = (
            TaskArtifactRefV2::urls(artifacts),
            TaskUsageFactsV2::new(None, None, None).and_then(|u| u.with_outputs(Some(count))),
        ) else {
            return unknown("veo samples exceed the artifact bounds");
        };
        return TaskObservationV2::Succeeded { artifacts, usage };
    }
    let filtered =
        generated.and_then(|r| r.get("raiMediaFilteredCount")).and_then(Value::as_i64).unwrap_or(0);
    if filtered > 0 {
        return TaskObservationV2::Failed {
            kind: TaskFailureKindV1::Failed,
            code: Some("rai_media_filtered".into()),
            message: bounded(
                generated.and_then(|r| r.get("raiMediaFilteredReasons")).map(Value::to_string),
            ),
        };
    }
    unknown("veo done without samples, error, or RAI filter count")
}

impl TaskComponentV2 for VeoTaskComponentV2 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "task-veo-v2".into(),
            version: "0.35.8".into(),
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
            .filter(|m| {
                !m.is_empty() && m.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
            .ok_or_else(|| invalid("missing or invalid Veo model"))?;
        let prompt = field(body, "prompt").ok_or_else(|| invalid("Missing 'prompt' field"))?;
        let mut instance = json!({"prompt": prompt});
        let mut image_count = 0u32;
        if let Some(image) = field(body, "image").or_else(|| field(body, "image_url")) {
            instance["image"] = inline_image("image", image)?;
            image_count += 1;
        }
        if let Some(last) = field(body, "last_frame").or_else(|| field(body, "last_frame_url")) {
            instance["lastFrame"] = inline_image("last_frame", last)?;
            image_count += 1;
        }
        if let Some(references) = body.get("reference_images").filter(|v| !v.is_null()) {
            let items = references.as_array().ok_or_else(|| {
                invalid("'reference_images' must be an array of image URL strings")
            })?;
            if items.len() > MAX_REFERENCE_IMAGES {
                return Err(invalid(
                    "'reference_images' accepts at most 3 entries (got more); Veo 3.1 rejects more",
                ));
            }
            let mut parts = Vec::with_capacity(items.len());
            for item in items {
                let value = item
                    .as_str()
                    .ok_or_else(|| invalid("'reference_images' entries must be image strings"))?;
                parts.push(json!({"image": inline_image("reference_images", value)?,
                                  "referenceType": "asset"}));
                image_count += 1;
            }
            if !parts.is_empty() {
                instance["referenceImages"] = json!(parts);
            }
        }
        let mut parameters = json!({});
        if let Some(ratio) = field(body, "aspect_ratio") {
            parameters["aspectRatio"] = json!(ratio);
        }
        let duration = match body.get("duration").filter(|v| !v.is_null()) {
            None => None,
            Some(raw) => {
                let seconds = raw
                    .as_u64()
                    .or_else(|| raw.as_str().and_then(|s| s.trim().parse().ok()))
                    .filter(|s| *s >= 1)
                    .ok_or_else(|| {
                        invalid("Veo durationSeconds must be a positive whole number of seconds")
                    })?;
                parameters["durationSeconds"] = json!(seconds);
                Some(seconds)
            }
        };
        let resolution = field(body, "resolution");
        if let Some(resolution) = resolution {
            parameters["resolution"] = json!(resolution);
        }
        let samples = match body.get("n").filter(|v| !v.is_null()) {
            None => 1,
            Some(raw) => raw
                .as_u64()
                .filter(|n| *n >= 1 && u32::try_from(*n).is_ok())
                .ok_or_else(|| invalid("'n' must be a positive whole number of videos"))?,
        };
        parameters["sampleCount"] = json!(samples);
        let resolution_fact = resolution.map(str::to_ascii_uppercase).filter(|r| {
            !r.is_empty() && r.len() <= 32 && r.bytes().all(|b| b.is_ascii_alphanumeric())
        });
        #[expect(clippy::cast_precision_loss, reason = "Veo durations are small whole seconds")]
        let requested_seconds = duration.map(|s| s as f64);
        let request_estimate = TaskRequestEstimateV2::new(requested_seconds, None)
            .and_then(|e| e.with_input_facts(resolution_fact.as_deref(), Some(image_count)))
            .and_then(|e| e.with_output_facts(None, i64::try_from(samples).ok()))
            .map_err(|_| invalid("invalid Veo request estimate"))?;
        let mut descriptor =
            request(config, HttpMethod::Post, &format!("v1beta/models/{model}:predictLongRunning"));
        descriptor.headers = SafeHeaders::try_new([("content-type", "application/json")])
            .map_err(|_| protocol("invalid Veo headers"))?;
        descriptor.body = Some(json!({"instances": [instance], "parameters": parameters}));
        Ok(PreparedTaskV2 {
            descriptor,
            locator: TaskLocatorV2::new(1, QUERY).map_err(|_| protocol("invalid Veo locator"))?,
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
                .and_then(|body| {
                    bounded(body.get("error").and_then(|e| field(e, "message")).map(str::to_owned))
                })
                .unwrap_or_else(|| "Veo submission rejected".into());
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
        if let Some(name) = field(&body, "name") {
            return Ok(if operation_name(name) {
                SubmitOutcomeV2::Accepted(name.into())
            } else {
                SubmitOutcomeV2::Unknown
            });
        }
        // No operation name: only a body that is already terminal can be settled from here.
        Ok(match observe_operation(&body) {
            terminal @ (TaskObservationV2::Succeeded { .. } | TaskObservationV2::Failed { .. }) => {
                SubmitOutcomeV2::AcceptedTerminal(terminal)
            }
            _ => SubmitOutcomeV2::Unknown,
        })
    }
    fn build_observe_request(
        &self,
        config: &ProviderConfig,
        _: &str,
        id: &str,
        locator: &TaskLocatorV2,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        checked_locator(locator)?;
        if !operation_name(id) {
            return Err(invalid("invalid Veo operation name"));
        }
        Ok(request(config, HttpMethod::Get, &format!("{QUERY}/{id}")))
    }
    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV2> {
        if !(200..300).contains(&parts.status) {
            return Ok(unknown("Veo observation HTTP failure"));
        }
        let Ok(body) = serde_json::from_str::<Value>(&parts.body) else {
            return Ok(unknown("invalid Veo observation JSON"));
        };
        Ok(observe_operation(&body))
    }
    fn build_artifact_request(
        &self,
        _: &ProviderConfig,
        locator: &TaskLocatorV2,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        checked_locator(locator)?;
        observation.validate().map_err(|_| protocol("invalid Veo observation"))?;
        Ok(None)
    }
    fn render_success(
        &self,
        observation: &TaskObservationV2,
        _: Option<&HttpResponseParts>,
        context: &TaskRenderContextV2,
    ) -> ComponentResultV1<Value> {
        observation.validate().map_err(|_| protocol("invalid Veo observation"))?;
        let TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::Urls(items), .. } =
            observation
        else {
            return Err(protocol("Veo render requires direct artifacts"));
        };
        Ok(json!({
            "created": context.created(),
            "data": items.iter().map(|item| json!({"url": item.url()})).collect::<Vec<_>>(),
        }))
    }
    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<ErrorEnvelope> {
        observation.validate().map_err(|_| protocol("invalid Veo observation"))?;
        let TaskObservationV2::Failed { message, .. } = observation else {
            return Err(protocol("Veo failure mapping requires failed observation"));
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
