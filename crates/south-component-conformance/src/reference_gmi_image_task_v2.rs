//! Pure `GMI Cloud` media image translation (request-queue API).
//!
//! Transcribed from the host's native GMI arm (token-station-server `handler/images/gmi.rs`
//! `build_gmi_submit_body` / `gmi_reference_images` / `gmi_reference_image_limit` /
//! `gmi_media_urls` / `gmi_media_result_to_openai`, `images/durable.rs::gmi_run` /
//! `parse_gmi_create`, `images/observe.rs::normalize_gmi`). Credentials, pricing, persistence and
//! time remain host responsibilities.
//!
//! - **Finish at submit**: GMI sometimes answers the submission with the finished media URLs; that
//!   response is `accepted-terminal` (task contract 5 already had the outcome; the host settles it
//!   directly since token-station-server P13 C6).
//! - **Organization header**: the native arm sends `X-Organization-ID` from the credential's
//!   non-secret account id. A component never sees credential rows, so the host passes that value
//!   as the provider-config extension `organization_id`; when present it becomes the header.
//! - **Counts**: GMI prices per request, not per image, and reports no usage. `n` is still the
//!   requested output count and the delivered URL count is `usage.outputs` — pricing policy is the
//!   host's.
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

const QUEUE: &str = "api/v1/ie/requestqueue/apikey/requests";
/// Request-body paths the host must not rewrite (contract 6): what decides how many images of
/// which size and quality the request produces.
const IMMUTABLE_BODY_PATHS: [&str; 4] = ["model", "payload.n", "payload.size", "payload.quality"];
const PASSTHROUGH: [&str; 4] = ["size", "quality", "output_format", "response_format"];

/// Managed GMI media image dialect; no credentials, clock, price or persistence.
#[derive(Debug, Default, Clone, Copy)]
pub struct GmiImageTaskComponentV2;

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
/// Reference-image cap per upstream model, only where the official docs state one; `None` means
/// the model takes no reference images.
fn reference_limit(model: &str) -> Option<usize> {
    if model.starts_with("seedream-4-0") {
        Some(10)
    } else if model.starts_with("seedream-5.0-lite") || model.starts_with("seedream-5-0-lite") {
        Some(14)
    } else {
        None
    }
}
fn reference_images(body: &Value) -> ComponentResultV1<Vec<String>> {
    let (key, value) = match (body.get("image"), body.get("image_url")) {
        (Some(v), _) if !v.is_null() => ("image", v),
        (_, Some(v)) if !v.is_null() => ("image_url", v),
        _ => return Ok(Vec::new()),
    };
    match value {
        Value::String(url) => Ok(vec![url.clone()]),
        Value::Array(items) => items
            .iter()
            .map(|item| {
                item.as_str().map(str::to_owned).ok_or_else(|| {
                    invalid(&format!("'{key}' array entries must be image URL strings"))
                })
            })
            .collect(),
        other => Err(invalid(&format!(
            "'{key}' must be a URL string or an array of URL strings (got {other})"
        ))),
    }
}
/// `media_urls` at the top level or under `outcome`, as strings or `{url}` objects.
fn media_urls(body: &Value) -> Vec<&str> {
    body.get("media_urls")
        .or_else(|| body.pointer("/outcome/media_urls"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().or_else(|| field(item, "url")))
                .filter(|url| !url.is_empty())
                .collect()
        })
        .unwrap_or_default()
}
fn succeeded(urls: &[&str]) -> TaskObservationV2 {
    let artifacts: Result<Vec<_>, _> = urls
        .iter()
        .map(|url| TaskArtifactV2::new(url, TaskScalarV2::Null, TaskScalarV2::Null))
        .collect();
    let count = i64::try_from(urls.len()).unwrap_or(i64::MAX);
    match (
        artifacts.and_then(TaskArtifactRefV2::urls),
        TaskUsageFactsV2::new(None, None, None).and_then(|u| u.with_outputs(Some(count))),
    ) {
        (Ok(artifacts), Ok(usage)) => TaskObservationV2::Succeeded { artifacts, usage },
        _ => TaskObservationV2::Unknown { reason: "gmi media exceeds the artifact bounds".into() },
    }
}
/// The poll reading, shared with the submit path that may already carry the media.
fn observe(body: &Value) -> TaskObservationV2 {
    let urls = media_urls(body);
    if !urls.is_empty() {
        return succeeded(&urls);
    }
    let status = field(body, "status").unwrap_or("");
    match status {
        "success" | "finished" => TaskObservationV2::Failed {
            kind: TaskFailureKindV1::Failed,
            code: Some("empty_result".into()),
            message: Some(format!("gmi reported '{status}' but delivered no media urls")),
        },
        "failed" | "cancelled" | "canceled" | "error" => TaskObservationV2::Failed {
            kind: TaskFailureKindV1::Failed,
            code: Some(status.into()),
            message: bounded(field(body, "message").or_else(|| field(body, "error"))),
        },
        "" => TaskObservationV2::Progress { running: true, status_word: "unreported".into() },
        other => bounded(Some(other)).map_or_else(
            || TaskObservationV2::Unknown { reason: "gmi status word exceeds its bound".into() },
            |word| TaskObservationV2::Progress { running: true, status_word: word },
        ),
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
fn request(
    config: &ProviderConfig,
    method: HttpMethod,
    path: &str,
    json_body: bool,
) -> ComponentResultV1<HttpRequestDescriptor> {
    let mut descriptor = HttpRequestDescriptor::new(
        method,
        format!("{}/{path}", config.base_url.as_str().trim_end_matches('/')),
    );
    descriptor.auth = config.auth.clone().map(Auth::bearer);
    let organization = config
        .extensions
        .get("organization_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());
    let mut headers: Vec<(&str, &str)> = Vec::new();
    if json_body {
        headers.push(("content-type", "application/json"));
    }
    if let Some(organization) = organization {
        headers.push(("x-organization-id", organization));
    }
    descriptor.headers = SafeHeaders::try_new(headers)
        .map_err(|_| invalid("invalid GMI organization id extension"))?;
    Ok(descriptor)
}
fn checked_locator(locator: &TaskLocatorV2) -> ComponentResultV1<()> {
    if locator.route() == QUEUE { Ok(()) } else { Err(invalid("unsupported GMI locator")) }
}

impl TaskComponentV2 for GmiImageTaskComponentV2 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "task-gmi-image-v2".into(),
            version: "0.35.0".into(),
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
            .ok_or_else(|| invalid("missing GMI model"))?;
        if field(body, "response_format") == Some("b64_json") {
            return Err(invalid(
                "GMI media image generation returns URLs only; response_format='b64_json' is not supported",
            ));
        }
        let prompt = match body.get("prompt") {
            None => return Err(invalid("Missing 'prompt' field")),
            Some(value) => value
                .as_str()
                .ok_or_else(|| invalid(&format!("'prompt' must be a string (got {value})")))?,
        };
        let requested = match body.get("n").filter(|v| !v.is_null()) {
            None => 1,
            Some(raw) => raw
                .as_u64()
                .filter(|n| *n >= 1 && u32::try_from(*n).is_ok())
                .ok_or_else(|| invalid("'n' must be a positive whole number of images"))?,
        };
        let mut payload = Map::new();
        payload.insert("prompt".into(), json!(prompt));
        for key in PASSTHROUGH {
            if let Some(value) = body.get(key) {
                payload.insert(key.into(), value.clone());
            }
        }
        payload.insert("n".into(), json!(requested));
        let references = reference_images(body)?;
        if !references.is_empty() {
            let limit = reference_limit(model).ok_or_else(|| {
                invalid(&format!(
                    "upstream model '{model}' documents no reference-image input on GMI"
                ))
            })?;
            if references.len() > limit {
                return Err(invalid(&format!(
                    "upstream model '{model}' accepts at most {limit} reference images, got {}",
                    references.len()
                )));
            }
            payload.insert("image".into(), json!(references));
        }
        let request_estimate = TaskRequestEstimateV2::new(None, None)
            .and_then(|e| {
                e.with_input_facts(None, Some(u32::try_from(references.len()).unwrap_or(u32::MAX)))
            })
            .and_then(|e| e.with_output_facts(None, i64::try_from(requested).ok()))
            .map_err(|_| invalid("invalid GMI request estimate"))?;
        let mut descriptor = request(config, HttpMethod::Post, QUEUE, true)?;
        descriptor.body = Some(json!({"model": model, "payload": payload}));
        Ok(PreparedTaskV2 {
            descriptor,
            locator: TaskLocatorV2::new(1, QUEUE).map_err(|_| protocol("invalid GMI locator"))?,
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
                .and_then(|body| bounded(field(&body, "message").or_else(|| field(&body, "error"))))
                .unwrap_or_else(|| "GMI submission rejected".into());
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
        if !media_urls(&body).is_empty() {
            return Ok(match observe(&body) {
                done @ TaskObservationV2::Succeeded { .. } => {
                    SubmitOutcomeV2::AcceptedTerminal(done)
                }
                _ => SubmitOutcomeV2::Unknown,
            });
        }
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
            return Err(invalid("invalid GMI request identifier"));
        }
        request(config, HttpMethod::Get, &format!("{QUEUE}/{}", encode_segment(id)), false)
    }
    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV2> {
        if !(200..300).contains(&parts.status) {
            return Ok(TaskObservationV2::Unknown {
                reason: "GMI observation HTTP failure".into(),
            });
        }
        let Ok(body) = serde_json::from_str::<Value>(&parts.body) else {
            return Ok(TaskObservationV2::Unknown {
                reason: "invalid GMI observation JSON".into(),
            });
        };
        Ok(observe(&body))
    }
    fn build_artifact_request(
        &self,
        _: &ProviderConfig,
        locator: &TaskLocatorV2,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        checked_locator(locator)?;
        observation.validate().map_err(|_| protocol("invalid GMI observation"))?;
        Ok(None)
    }
    fn render_success(
        &self,
        observation: &TaskObservationV2,
        _: Option<&HttpResponseParts>,
        context: &TaskRenderContextV2,
    ) -> ComponentResultV1<Value> {
        observation.validate().map_err(|_| protocol("invalid GMI observation"))?;
        let TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::Urls(items), .. } =
            observation
        else {
            return Err(protocol("GMI render requires direct artifacts"));
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
        observation.validate().map_err(|_| protocol("invalid GMI observation"))?;
        let TaskObservationV2::Failed { code, message, .. } = observation else {
            return Err(protocol("GMI failure mapping requires failed observation"));
        };
        let detail = message.as_deref().unwrap_or("upstream task failed");
        let message =
            code.as_deref().map_or_else(|| detail.to_owned(), |code| format!("{code}: {detail}"));
        Ok(ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message))
    }
}
