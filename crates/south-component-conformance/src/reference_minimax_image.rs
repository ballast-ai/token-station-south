//! The native reference implementation of the `image-minimax` component.
//!
//! `MiniMax`'s synchronous image generation (image world step I3-1, design record
//! `docs/design/2026-10-12-image-i3-1-minimax-and-qwen.md` §2; image record §13, `MiniMax` row).
//!
//! Transcribed from token-station-server's native `MiniMax` image arm as the record summarises it
//! (`handler/images/minimax.rs`, the `GroupId` arm of `engine/upstream.rs`, the status table of
//! `minimax_status.rs` and `MINIMAX_IMAGE_OWNED` in `images/execute.rs`):
//!
//! - **Request**: `POST {base_url}/v1/image_generation`, JSON: `model` (the routed upstream id),
//!   `prompt`; `n` only when the client sent it, as an integer; `aspect_ratio`, `width`, `height`,
//!   `seed` and `prompt_optimizer` copied when present; `response_format` with `b64_json` mapped to
//!   `base64` (any other value as sent); `subject_reference` as sent, else the client's `image_url`
//!   (a string or an array of strings) wrapped as `[{"type": "character", "image_file": …}]`. Every
//!   other field, `size` included, is dropped. `?GroupId=` is appended only when the family's config
//!   key `group_id` is set (IM-Q3). Auth is `bearer` on the configured slot.
//! - **Refusals before dispatch** (`invalid_request`): `prompt` missing or not a string, or longer
//!   than 1500 characters; `n` not a whole number from 1 to 9; `image_url` of the wrong type.
//! - **Response**: a 2xx whose `base_resp.status_code` is not `0` failed in the body (record §7).
//!   Its envelope carries the host's status for the code (1004 / 2049 → 401, 1026 / 2013 → 400,
//!   1002 → 429, 1008 / 2153 → 503, else 502; the token-station host answers 502 while the funds
//!   are parked, IM-Q6 as re-ruled); the codes that mean "refused before output"
//!   are `rejected`, and 1026 (an output violation, possibly charged; IM-Q4) and any code the table
//!   does not name are `unknown`. A missing `base_resp` is success. Each non-empty
//!   `data.image_urls[i]` is a `url` artifact and each non-empty `data.image_base64[i]` an inline
//!   base64 `image/png` artifact (IM-Q5), URLs first; none is `unknown`. `images_reported` is
//!   `metadata.success_count` (evidence: the host settles it under Q9's rule, IM-Q1).
//! - **Render**: `{"created", "data": [{"url" | "b64_json", "revised_prompt": null}]}`.
//!
//! The request's `image_url` and `subject_reference[*].image_file` (data URLs, usually) and the
//! response's `image_base64` entries are elided: their bytes never enter the sandbox.

use serde_json::{Map, Value, json};
use south_contracts::JsonPointerV1;
use south_contracts::image::{
    ArtifactEncodingV1, ImageArtifactV1, ImageCallContextV1, ImageFactsV1, ImageInputRoleV1,
    ImageInputsV1, ImageMeteringV1, ImageModelCapabilitiesV1, ImageOperationV1,
    ImageRenderContextV1, ImageResponseFormatV1, ImageTierV1, InputRoleKeyV1,
    MAX_IMAGE_INPUTS_PER_ROLE, MeteringFormV1, PreparedImageCallV1,
};
use south_contracts::media::{
    MediaLimitsV1, MediaRequestDescriptorV1, PathPatternV1, ResponseBodyFormV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

use crate::image_shared::{
    bearer, capped, code_message, images_only, internal, invalid, is_blob, non_2xx,
    non_empty_inline, reported_count, requested, serves, status_code, url_entry, view_status,
    with_refs,
};
use crate::{ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1};

/// The package name this reference is published as.
pub const NAME: &str = "image-minimax";
/// The package version.
pub const VERSION: &str = "1.0.1";
/// The world this reference exports.
pub const WORLD: &str = "image-adapter-v1";
/// The family, as a host's image rows name it.
pub const MINIMAX_IMAGE: &str = "minimax-image";
/// The family's one config key: the account's group id, sent as `?GroupId=` when set (IM-Q3).
pub const GROUP_ID: &str = "group_id";

const UPSTREAM: &str = "MiniMax";
const GENERATION_PATH: &str = "v1/image_generation";
/// The native arm's `MINIMAX_IMAGE_OWNED`: request extras may not set these.
const IMMUTABLE_BODY_PATHS: [&str; 6] =
    ["model", "n", "width", "height", "aspect_ratio", "response_format"];
/// Fields copied to the upstream body exactly as the client sent them.
const COPIED: [&str; 5] = ["aspect_ratio", "width", "height", "seed", "prompt_optimizer"];
const MAX_PROMPT_CHARS: usize = 1500;
const MAX_IMAGES: u64 = 9;
/// Request-side elision: the reference images, which are usually data URLs.
const REQUEST_ELISION_PATHS: [&str; 3] =
    ["/subject_reference/*/image_file", "/image_url", "/image_url/*"];
/// The `base_resp` codes that mean the upstream refused before producing anything.
const REFUSED_BEFORE_OUTPUT: [i64; 6] = [1004, 2049, 2013, 1002, 1008, 2153];

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct MiniMaxImageReferenceV1;

fn paths(patterns: &[&str]) -> ComponentResultV1<Vec<PathPatternV1>> {
    patterns
        .iter()
        .map(|pattern| PathPatternV1::parse(pattern).map_err(|_| internal("invalid elision path")))
        .collect()
}

/// The prompt, or the refusal the native arm answers.
fn prompt(body: &Map<String, Value>) -> ComponentResultV1<&str> {
    match body.get("prompt") {
        Some(value) if is_blob(value) => {
            Err(invalid("'prompt' is longer than the gateway accepts"))
        }
        Some(Value::String(prompt)) => {
            let chars = prompt.chars().count();
            if chars > MAX_PROMPT_CHARS {
                Err(invalid(format!(
                    "'prompt' must be at most {MAX_PROMPT_CHARS} characters for MiniMax image \
                     models (got {chars})"
                )))
            } else {
                Ok(prompt)
            }
        }
        _ => Err(invalid("Missing 'prompt' field")),
    }
}

/// The client's `image_url` as template values, in order: one string, or each string of an array.
fn image_urls(body: &Map<String, Value>) -> ComponentResultV1<Option<(Value, u32)>> {
    match body.get("image_url") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => {
            let values = items
                .iter()
                .map(|item| {
                    url_entry(item)
                        .ok_or_else(|| invalid("image_url array entries must be string URLs"))
                })
                .collect::<ComponentResultV1<Vec<_>>>()?;
            let count = u32::try_from(values.len()).unwrap_or(u32::MAX);
            Ok(Some((Value::Array(values), count)))
        }
        Some(other) => url_entry(other).map(|value| Some((value, 1))).ok_or_else(|| {
            invalid(format!(
                "image_url must be a string URL or an array of string URLs (got {other})"
            ))
        }),
    }
}

/// The host's status for a `base_resp` code (`minimax_status.rs`), as an envelope.
fn body_failure(code: i64, base_resp: &Value) -> ErrorEnvelope {
    let (error_code, status) = match code {
        1004 | 2049 => (ErrorCode::Auth, 401),
        1026 => (ErrorCode::ContentPolicy, 400),
        2013 => (ErrorCode::InvalidRequest, 400),
        1002 => (ErrorCode::RateLimit, 429),
        1008 | 2153 => (ErrorCode::UpstreamUnavailable, 503),
        _ => (ErrorCode::ProviderProtocolError, 502),
    };
    let mut envelope = ErrorEnvelope::new(error_code, status, code_message(error_code));
    envelope.provider_message = base_resp["status_msg"].as_str().and_then(capped);
    envelope
}

fn pointer(text: &str) -> ComponentResultV1<JsonPointerV1> {
    JsonPointerV1::parse(text).map_err(|_| internal("invalid artifact pointer"))
}

impl ImageComponentV1 for MiniMaxImageReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: NAME.to_owned(),
            version: VERSION.to_owned(),
            api_version: WORLD.to_owned(),
        }
    }

    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
        serves(config, MINIMAX_IMAGE)?;
        let request_elision_paths = paths(&REQUEST_ELISION_PATHS)?;
        Ok(config
            .models
            .iter()
            .map(|model| ImageModelCapabilitiesV1 {
                model: model.model.clone(),
                operations: vec![ImageOperationV1::Generate],
                input_roles: vec![
                    InputRoleKeyV1 {
                        key: "subject_reference".to_owned(),
                        role: ImageInputRoleV1::ReferenceImage,
                    },
                    InputRoleKeyV1 {
                        key: "image_url".to_owned(),
                        role: ImageInputRoleV1::ReferenceImage,
                    },
                ],
                response_formats: vec![ImageResponseFormatV1::B64Json, ImageResponseFormatV1::Url],
                metering_forms: vec![MeteringFormV1::Images],
                token_buckets: Vec::new(),
                tier_dimensions: Vec::new(),
                repeat: false,
                request_elision_paths: request_elision_paths.clone(),
            })
            .collect())
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &ImageCallContextV1,
    ) -> ComponentResultV1<PreparedImageCallV1> {
        serves(config, MINIMAX_IMAGE)?;
        let forms = images_only(context, UPSTREAM)?;
        let body = request
            .get("json")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("MiniMax image models only generate from a JSON request"))?;
        let prompt = prompt(body)?;
        let n = requested(body.get("n"), MAX_IMAGES, UPSTREAM)?;
        let image_url = image_urls(body)?;

        let mut template = Map::new();
        template.insert("model".to_owned(), json!(context.upstream_model));
        template.insert("prompt".to_owned(), json!(prompt));
        if body.contains_key("n") {
            template.insert("n".to_owned(), json!(n));
        }
        for key in COPIED {
            if let Some(value) = body.get(key) {
                template.insert(key.to_owned(), with_refs(value));
            }
        }
        if let Some(format) = body.get("response_format") {
            let mapped = match format.as_str() {
                Some("b64_json") => json!("base64"),
                _ => with_refs(format),
            };
            template.insert("response_format".to_owned(), mapped);
        }
        let subject = body.get("subject_reference").filter(|subject| !subject.is_null());
        let references = if let Some(subject) = subject {
            template.insert("subject_reference".to_owned(), with_refs(subject));
            match subject {
                Value::Array(items) => u32::try_from(items.len()).unwrap_or(u32::MAX),
                _ => 1,
            }
        } else if let Some((image_file, count)) = image_url {
            template.insert(
                "subject_reference".to_owned(),
                json!([{ "type": "character", "image_file": image_file }]),
            );
            count
        } else {
            0
        };
        if references > MAX_IMAGE_INPUTS_PER_ROLE {
            return Err(invalid(format!(
                "at most {MAX_IMAGE_INPUTS_PER_ROLE} reference images are accepted (got {references})"
            )));
        }

        let mut descriptor = Map::new();
        descriptor.insert("method".to_owned(), json!("POST"));
        descriptor.insert("path".to_owned(), json!(GENERATION_PATH));
        if let Some(group) = config.declared.get(GROUP_ID) {
            descriptor.insert("query".to_owned(), json!([{ "name": "GroupId", "value": group }]));
        }
        if let Some(auth) = bearer(config) {
            descriptor.insert("auth".to_owned(), auth);
        }
        descriptor.insert("body".to_owned(), json!({ "json": { "template": template } }));
        let descriptor = MediaRequestDescriptorV1::parse(
            &Value::Object(descriptor).to_string(),
            &MediaLimitsV1::V1,
        )
        .map_err(|error| internal(format!("built an invalid descriptor: {error}")))?;
        Ok(PreparedImageCallV1 {
            facts: ImageFactsV1 {
                operation: ImageOperationV1::Generate,
                inputs: ImageInputsV1 { reference_image: references, ..ImageInputsV1::default() },
                requested_outputs: u32::try_from(n).unwrap_or(1),
                size: None,
                tier: ImageTierV1::default(),
                tier_candidates: None,
                metering_forms: forms,
                bounds: None,
            },
            descriptor,
            repeat: 1,
            response_body_form: ResponseBodyFormV1::Json,
            response_elision_paths: paths(&["/data/image_base64/*"])?,
            immutable_body_paths: IMMUTABLE_BODY_PATHS
                .iter()
                .map(|path| (*path).to_owned())
                .collect(),
            state: "{}".to_owned(),
        })
    }

    fn parse_response(
        &self,
        _state: &Value,
        response: &Value,
    ) -> ComponentResultV1<ImageOutcomeV1> {
        let status = view_status(response);
        let body = response.pointer("/body/json").cloned().unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            let code = status_code(status);
            let mut error = ErrorEnvelope::new(code, status, code_message(code));
            error.provider_message = body["base_resp"]["status_msg"].as_str().and_then(capped);
            return Ok(non_2xx(status, error));
        }
        if !body.is_object() {
            return Ok(ImageOutcomeV1::Unknown {
                reason: "a 2xx without a JSON object body".to_owned(),
                error: None,
            });
        }
        let base_resp = &body["base_resp"];
        let code = match &base_resp["status_code"] {
            Value::Null => 0,
            value => match value.as_i64() {
                Some(code) => code,
                None => {
                    return Ok(ImageOutcomeV1::Unknown {
                        reason: "a malformed base_resp.status_code".to_owned(),
                        error: None,
                    });
                }
            },
        };
        if code != 0 {
            let error = body_failure(code, base_resp);
            return Ok(if REFUSED_BEFORE_OUTPUT.contains(&code) {
                ImageOutcomeV1::Rejected { error }
            } else {
                ImageOutcomeV1::Unknown {
                    reason: format!("MiniMax base_resp status code {code}"),
                    error: Some(error),
                }
            });
        }
        let mut artifacts = Vec::new();
        if let Some(urls) = body.pointer("/data/image_urls").and_then(Value::as_array) {
            for (index, url) in urls.iter().enumerate() {
                if url.as_str().is_some_and(|url| !url.is_empty()) {
                    artifacts.push(ImageArtifactV1::Url {
                        pointer: pointer(&format!("/data/image_urls/{index}"))?,
                        media_type: None,
                    });
                }
            }
        }
        if let Some(images) = body.pointer("/data/image_base64").and_then(Value::as_array) {
            for (index, image) in images.iter().enumerate() {
                if non_empty_inline(image) {
                    artifacts.push(ImageArtifactV1::Inline {
                        pointer: pointer(&format!("/data/image_base64/{index}"))?,
                        encoding: ArtifactEncodingV1::Base64,
                        media_type: "image/png".to_owned(),
                    });
                }
            }
        }
        if artifacts.is_empty() {
            return Ok(ImageOutcomeV1::Unknown {
                reason: "a 2xx without an image".to_owned(),
                error: None,
            });
        }
        Ok(ImageOutcomeV1::Succeeded {
            artifacts,
            metering: ImageMeteringV1 {
                tokens: None,
                images_reported: reported_count(&body["metadata"]["success_count"]),
                credits: None,
                upstream_cost: None,
            },
            extras: Value::Null,
        })
    }

    fn render(
        &self,
        _state: &Value,
        outcomes: &[ImageOutcomeV1],
        context: &ImageRenderContextV1,
    ) -> ComponentResultV1<ImageRenderedV1> {
        let [ImageOutcomeV1::Succeeded { artifacts, .. }] = outcomes else {
            return Err(internal("MiniMax renders exactly one succeeded round"));
        };
        let data: Vec<Value> = artifacts
            .iter()
            .enumerate()
            .map(|(index, artifact)| {
                let delivery = if matches!(artifact, ImageArtifactV1::Url { .. }) {
                    "url"
                } else {
                    "b64_json"
                };
                json!({
                    delivery: { "$south.artifact": { "index": index, "as": delivery } },
                    "revised_prompt": null,
                })
            })
            .collect();
        Ok(ImageRenderedV1 {
            template: json!({ "created": context.created, "data": data }).to_string(),
        })
    }
}
