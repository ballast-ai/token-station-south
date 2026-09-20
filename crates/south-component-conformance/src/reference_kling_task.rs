//! The native reference implementation of the official Kling task component.
//!
//! Design record: `docs/design/2026-09-19-task-world-fit-survey.md` for the
//! signatures, `2026-09-19-task-vocabulary-fit-survey.md` for the types.
//!
//! Same shape as the three provider references: gate ② is frozen against this
//! implementation, a `wasm32-wasip2` build of the same logic is what ships,
//! and the sandbox parity test proves the two agree.
//!
//! # Provenance of the wire knowledge
//!
//! **Not captured from the upstream.** Every wire fact below is transcribed
//! from the adopting host's production implementation — its `normalize_kling`,
//! `parse_kling_create` and `KlingDispatch`, together with the frozen word
//! table its own tests pin. That code has served real traffic and carries the
//! marks of incidents (the two defensive arms in
//! [`parse_observation`](TaskComponentV1::parse_observation) are
//! shapes it was taught by failure), so it is second-hand but *evidenced*
//! knowledge rather than a guess.
//!
//! It is still second-hand. Where a reader needs certainty about what Kling
//! actually sends, the upstream's own documentation and a capture are the
//! authority, not this file. The fixture pack carries the same note: a case
//! here proves the component agrees with the host, and the host's agreement
//! with Kling rests on production traffic rather than on a recording.
//!
//! # The dialect
//!
//! - **Three creation paths, chosen by model and by whether an image was
//!   given.** `text2video` / `image2video` / motion control are different
//!   endpoints on the same provider, and the status query is always
//!   `GET {create_path}/{task_id}` — there is no generic `/v1/videos/{id}`
//!   route. This is why `build-observe-request` takes the model.
//! - **The gateway's task id rides in the body** as `external_task_id`, which
//!   is what makes a resubmission idempotent upstream.
//! - **Status words are `submitted` / `processing` / `succeed` / `failed`.**
//!   Note `succeed`, not `succeeded`.
//! - **The meter is `final_unit_deduction`**, a decimal string of the
//!   provider's own billing unit, reported only on success.

use serde_json::{Map, Value, json};
use south_contracts::{HostMintedValuesV1, TaskArtifactRefV1, TaskMeterV1, TaskObservationV1};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{
    Auth, ErrorCode, ErrorEnvelope, HttpMethod, HttpRequestDescriptor, HttpResponseParts,
    ProviderConfig, SafeHeaders,
};

use crate::component::{ComponentResultV1, SubmitOutcomeV1, TaskComponentV1};

/// The world this component speaks.
const TASK_WORLD: &str = "task-adapter-v1";

/// The reference component. Stateless: a task's state lives on the host's row.
#[derive(Debug, Default, Clone, Copy)]
pub struct KlingTaskReferenceV1;

fn internal(detail: impl std::fmt::Display) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string())
}

fn invalid(detail: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, detail)
}

/// Reads the body as JSON. A body that is not JSON is the component's own
/// failure to process, not a statement about the task.
fn body_json(parts: &HttpResponseParts) -> ComponentResultV1<Value> {
    serde_json::from_str(&parts.body)
        .map_err(|source| internal(format!("kling response body is not json: {source}")))
}

/// Kling nests the payload under `data` on some surfaces and not others.
fn payload(body: &Value) -> &Value {
    body.get("data").unwrap_or(body)
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// Seconds arrive as a number or as a decimal string (`"5"`).
fn lenient_secs(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
}

/// `final_unit_deduction` is a decimal string of the provider's billing unit;
/// the contract carries thousandths of it.
/// Far above any real deduction, and a literal rather than `i64::MAX as f64`
/// (which is itself a lossy cast).
const MAX_MILLIUNITS: f64 = 1e15;

#[expect(
    clippy::cast_possible_truncation,
    reason = "bounded above just before the cast; a non-finite or negative value returns None"
)]
fn final_milliunits(payload: &Value) -> Option<i64> {
    let raw = payload.get("final_unit_deduction")?;
    let units = raw.as_f64().or_else(|| raw.as_str().and_then(|s| s.trim().parse().ok()))?;
    if !units.is_finite() || units < 0.0 {
        return None;
    }
    let milli = (units * 1000.0).round();
    // An upstream reporting something absurd is a shape we decline to carry,
    // not a number we silently truncate: the meter reaches the host's
    // settlement, so a wrapped value would be a wrong charge. The bound is a
    // literal rather than `i64::MAX as f64`, which is itself lossy — and it is
    // far above any real deduction, so the cast below cannot truncate.
    if milli > MAX_MILLIUNITS {
        return None;
    }
    Some(milli as i64)
}

/// Which creation path this task used.
///
/// The model names the family; the presence of an input image picks between
/// the two paths that family serves. Motion control is its own model and its
/// own path.
fn create_path(upstream_model: &str, has_image: bool) -> &'static str {
    if upstream_model.contains("motion-control") {
        "/v1/videos/motion-control"
    } else if has_image {
        "/v1/videos/image2video"
    } else {
        "/v1/videos/text2video"
    }
}

fn base(config: &ProviderConfig) -> String {
    config.base_url.as_str().trim_end_matches('/').to_owned()
}

impl TaskComponentV1 for KlingTaskReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "task-kling".to_owned(),
            version: "1.0.1".to_owned(),
            api_version: TASK_WORLD.to_owned(),
        }
    }

    fn build_submit_request(
        &self,
        config: &ProviderConfig,
        request: &Value,
        minted: &HostMintedValuesV1,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        let model = request
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("a kling task request must name a model"))?;
        let prompt = request.get("prompt").and_then(Value::as_str);
        let image =
            request.get("image").or_else(|| request.get("image_tail")).and_then(Value::as_str);
        if prompt.is_none() && image.is_none() {
            return Err(invalid("a kling task request must carry a prompt or an image"));
        }

        let mut body = Map::new();
        body.insert("model_name".to_owned(), json!(model));
        if let Some(prompt) = prompt {
            body.insert("prompt".to_owned(), json!(prompt));
        }
        if let Some(image) = image {
            body.insert("image".to_owned(), json!(image));
        }
        for passthrough in ["duration", "mode", "aspect_ratio", "cfg_scale", "negative_prompt"] {
            if let Some(value) = request.get(passthrough) {
                body.insert(passthrough.to_owned(), value.clone());
            }
        }
        // The host's id, placed where this dialect reads it. Never invented:
        // it is the upstream idempotency anchor, and a component minting its
        // own would make a resubmission bill twice.
        body.insert("external_task_id".to_owned(), json!(minted.task_id()));

        let path = create_path(model, image.is_some());
        let mut descriptor =
            HttpRequestDescriptor::new(HttpMethod::Post, format!("{}{path}", base(config)));
        descriptor.headers =
            SafeHeaders::try_new([("content-type", "application/json")]).map_err(internal)?;
        descriptor.body = Some(Value::Object(body));
        descriptor.auth = config.auth.clone().map(Auth::bearer);
        Ok(descriptor)
    }

    fn parse_submit_response(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<SubmitOutcomeV1> {
        let body = body_json(parts)?;
        let data = payload(&body);

        // A non-zero `code` on a 2xx is a business-layer refusal: the call was
        // taken and the work declined, so nothing is running.
        if let Some(code) = body.get("code").and_then(Value::as_i64)
            && code != 0
        {
            let message = str_field(&body, "message")
                .unwrap_or_else(|| format!("kling refused the submission (code {code})"));
            return Ok(SubmitOutcomeV1::Rejected(ErrorEnvelope::new(
                ErrorCode::InvalidRequest,
                400,
                message,
            )));
        }

        match str_field(data, "task_id") {
            Some(id) if !id.is_empty() => Ok(SubmitOutcomeV1::Accepted(id)),
            // No id and no refusal: the upstream may have taken the work.
            // Never an error — the host keeps the reservation and reconciles.
            _ => Ok(SubmitOutcomeV1::Unknown),
        }
    }

    fn build_observe_request(
        &self,
        config: &ProviderConfig,
        upstream_model: &str,
        upstream_task_id: &str,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        if upstream_task_id.is_empty() {
            return Err(invalid("cannot observe a kling task without its upstream id"));
        }
        // V1 lacks the submitted image/context fact. This heuristic cannot
        // recover an ordinary model submitted with an image. A host must not
        // migrate that path until a versioned context contract is available.
        let path = create_path(upstream_model, false);
        let image_path = create_path(upstream_model, true);
        // There is no evidence that both query routes accept the same id.
        let path = if upstream_model.contains("i2v") { image_path } else { path };
        let mut descriptor = HttpRequestDescriptor::new(
            HttpMethod::Get,
            format!("{}{path}/{upstream_task_id}", base(config)),
        );
        descriptor.auth = config.auth.clone().map(Auth::bearer);
        Ok(descriptor)
    }

    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV1> {
        // A failed *query* is not a failed *task* (D3 rule 2). 429, 5xx, 404
        // and 401 all mean the observation did not happen.
        if !(200..300).contains(&parts.status) {
            return Ok(TaskObservationV1::Unknown {
                reason: format!("kling query http {}", parts.status),
            });
        }
        let body = body_json(parts)?;
        let data = payload(&body);

        let Some(status) = str_field(data, "task_status") else {
            return Ok(TaskObservationV1::Unknown {
                reason: "kling poll body has no task_status".to_owned(),
            });
        };

        match status.as_str() {
            "submitted" | "processing" => Ok(TaskObservationV1::Running { status_word: status }),
            "succeed" => {
                let videos = data
                    .get("task_result")
                    .and_then(|result| result.get("videos"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let urls: Vec<String> =
                    videos.iter().filter_map(|video| str_field(video, "url")).collect();
                if urls.is_empty() {
                    // Success with nothing to deliver is not a success we can
                    // render. Unknown re-queries rather than fabricating one.
                    return Ok(TaskObservationV1::Unknown {
                        reason: "kling succeed without task_result.videos[].url".to_owned(),
                    });
                }
                // The meter the upstream reported, preferring its exact
                // deduction over the derived duration.
                let meter = final_milliunits(data).map(TaskMeterV1::Milliunits).or_else(|| {
                    videos
                        .first()
                        .and_then(|video| video.get("duration"))
                        .and_then(lenient_secs)
                        .map(TaskMeterV1::Seconds)
                });
                let artifact = TaskArtifactRefV1::urls(urls)
                    .map_err(|source| internal(format!("kling artifact refused: {source}")))?;
                Ok(TaskObservationV1::Succeeded { artifact, meter })
            }
            "failed" => Ok(TaskObservationV1::Failed {
                // Kling states no expiry on the wire, so this is never
                // `ProviderExpired` (D3 rule: expiry needs an explicit
                // statement, and measured across six families only two make
                // one).
                kind: south_contracts::TaskFailureKindV1::Failed,
                code: None,
                message: str_field(data, "task_status_msg"),
            }),
            other => Ok(TaskObservationV1::Unknown {
                reason: format!("kling unrecognized task_status '{other}'"),
            }),
        }
    }

    fn build_artifact_request(
        &self,
        _config: &ProviderConfig,
        _observation: &TaskObservationV1,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        // Kling's terminal observation carries the URLs directly. `None` means
        // "already have it", not "not supported".
        Ok(None)
    }

    fn render_success(
        &self,
        observation: &TaskObservationV1,
        _fetched: Option<&HttpResponseParts>,
        minted: &HostMintedValuesV1,
    ) -> ComponentResultV1<Value> {
        let TaskObservationV1::Succeeded { artifact, .. } = observation else {
            return Err(internal("render_success called on a non-terminal observation"));
        };
        let TaskArtifactRefV1::Urls(urls) = artifact else {
            return Err(internal("kling always reports artifact urls"));
        };
        // Artifacts are addressed by the host's own relative path, built from
        // the id the host minted. The component places it and never invents
        // it — the same rule the submit side carries.
        let data: Vec<Value> = urls
            .iter()
            .enumerate()
            .map(|(index, url)| {
                json!({
                    "url": url,
                    "artifact": format!("/v1/video/tasks/{}/artifacts/art_{index}", minted.task_id()),
                })
            })
            .collect();
        Ok(json!({ "data": data }))
    }

    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV1,
    ) -> ComponentResultV1<ErrorEnvelope> {
        let TaskObservationV1::Failed { kind, code, message } = observation else {
            return Err(internal("map_terminal_failure called on a non-failure"));
        };
        let detail = message.clone().unwrap_or_else(|| "kling task failed".to_owned());
        let detail = match code {
            Some(code) => format!("{detail} ({code})"),
            None => detail,
        };
        // Whether to try another upstream is the host's decision, read off the
        // kind. An unclassifiable failure is `Internal`, never an invented code.
        Ok(match kind {
            south_contracts::TaskFailureKindV1::ProviderExpired => {
                ErrorEnvelope::new(ErrorCode::Internal, 500, detail)
            }
            _ => ErrorEnvelope::new(ErrorCode::Internal, 500, detail),
        })
    }
}
