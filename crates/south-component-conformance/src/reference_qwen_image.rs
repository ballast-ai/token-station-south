//! The native reference implementation of the `image-qwen` component.
//!
//! Bailian's synchronous Qwen-Image generation through `DashScope` multimodal generation (image
//! world step I3-1, design record `docs/design/2026-10-12-image-i3-1-minimax-and-qwen.md` §3; image
//! record §13, Bailian Qwen-Image row).
//!
//! Transcribed from token-station-server's native synchronous Bailian arm as the record summarises
//! it (`handler/images/bailian.rs`, `BAILIAN_IMAGE_OWNED` in `images/execute.rs`), on the request
//! pieces the Wan task package already carries (the crate-private module `bailian_image`, shared rather than copied,
//! which also holds the `DashScope` error reader `speech-qwen` reads too):
//!
//! - **Request**: `POST {base_url}/api/v1/services/aigc/multimodal-generation/generation`, JSON, no
//!   `X-DashScope-Async`: `{"model", "input": {"messages": [{"role": "user", "content": [{"image"}…,
//!   {"text": prompt}]}]}, "parameters": {…}}`, the image parts from the client's `image_url` (a
//!   string or an array of strings) before the text part. `n` only when sent; `size` only when
//!   sent, `WxH` → `W*H` (any other value as sent); `watermark`, `bbox_list`, `seed`,
//!   `prompt_extend` and `negative_prompt` only when sent. The edit models (`qwen-image-edit*`)
//!   edit through this same generation request. Auth is `bearer` on the configured slot.
//! - **Refusals before dispatch** (`invalid_request`): `prompt` missing or not a string; `n` not a
//!   whole number from 1 to 6; `image_url` of the wrong type.
//! - **Facts**: `requested_outputs` is `n` (1 when absent), `inputs.input_image` the number of
//!   `image_url` entries, and `size` only when the client sent one (`WxH` or `W*H`): the host
//!   prices tiers from the requested size, and the upstream's default would move an unsized
//!   request into another tier.
//! - **Response**: a non-2xx maps `DashScope`'s `code` / `message`, `rejected` for a 4xx other than
//!   408 and `unknown` otherwise; a 2xx carrying a `code` failed in the body and is `unknown` (the
//!   native arm does not detect it, record §6 item 4). Every non-empty
//!   `output.choices[].message.content[].image`, flattened across choices, is a `url` artifact
//!   (record Q15); none is `unknown`. `images_reported` is `usage.image_count` (evidence, settled by
//!   the host under Q9's rule, IM-Q1).
//! - **Render**: `{"created", "data": [{"url", "revised_prompt": null}]}`.
//!
//! The request's `image_url` entries (data URLs, usually) are elided: their bytes never enter the
//! sandbox.

use serde_json::{Map, Value, json};
use south_contracts::JsonPointerV1;
use south_contracts::image::{
    ImageArtifactV1, ImageCallContextV1, ImageFactsV1, ImageInputRoleV1, ImageInputsV1,
    ImageMeteringV1, ImageModelCapabilitiesV1, ImageOperationV1, ImageRenderContextV1,
    ImageResponseFormatV1, ImageSizeV1, ImageTierV1, InputRoleKeyV1, MAX_IMAGE_EDGE,
    MAX_IMAGE_INPUTS_PER_ROLE, MeteringFormV1, PreparedImageCallV1,
};
use south_contracts::media::{
    MediaLimitsV1, MediaRequestDescriptorV1, PathPatternV1, ResponseBodyFormV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

use crate::bailian_image::{
    PASSTHROUGH, dashscope_error, image_url_parts, result_images, wan_size, whole_count,
};
use crate::image_shared::{
    bearer, images_only, internal, invalid, is_blob, non_2xx, requested, serves, status_code,
    url_entry, view_status, with_refs,
};
use crate::{ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1};

/// The package name this reference is published as.
pub const NAME: &str = "image-qwen";
/// The package version.
pub const VERSION: &str = "1.0.16";
/// The world this reference exports.
pub const WORLD: &str = "image-adapter-v1";
/// The family, as a host's image rows name it.
pub const BAILIAN_QWEN_IMAGE: &str = "bailian-qwen-image";

const UPSTREAM: &str = "Bailian Qwen-Image";
const GENERATION_PATH: &str = "api/v1/services/aigc/multimodal-generation/generation";
/// The native arm's `BAILIAN_IMAGE_OWNED`: request extras may not set these.
const IMMUTABLE_BODY_PATHS: [&str; 4] = ["model", "input", "parameters.n", "parameters.size"];
const MAX_IMAGES: u64 = 6;
/// Request-side elision: the input images, which are usually data URLs.
const REQUEST_ELISION_PATHS: [&str; 2] = ["/image_url", "/image_url/*"];

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct QwenImageReferenceV1;

/// The pixel size a client's `WxH` or `W*H` names, when it names one within the fact's bounds.
fn size_fact(size: &str) -> Option<ImageSizeV1> {
    let (width, height) = size.trim().split_once(['x', 'X', '*'])?;
    let edge = |text: &str| {
        let text = text.trim();
        (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| text.parse::<u32>().ok())
            .flatten()
            .filter(|edge| (1..=MAX_IMAGE_EDGE).contains(edge))
    };
    Some(ImageSizeV1 { width: edge(width)?, height: edge(height)? })
}

/// Whether a 2xx body names a `DashScope` error code.
fn carries_code(body: &Value) -> bool {
    match &body["code"] {
        Value::Null => false,
        Value::String(code) => !code.trim().is_empty(),
        _ => true,
    }
}

impl ImageComponentV1 for QwenImageReferenceV1 {
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
        serves(config, BAILIAN_QWEN_IMAGE)?;
        let request_elision_paths = REQUEST_ELISION_PATHS
            .iter()
            .map(|path| PathPatternV1::parse(path).map_err(|_| internal("invalid elision path")))
            .collect::<ComponentResultV1<Vec<_>>>()?;
        Ok(config
            .models
            .iter()
            .map(|model| ImageModelCapabilitiesV1 {
                model: model.model.clone(),
                operations: vec![ImageOperationV1::Generate],
                input_roles: vec![InputRoleKeyV1 {
                    key: "image_url".to_owned(),
                    role: ImageInputRoleV1::InputImage,
                }],
                response_formats: vec![ImageResponseFormatV1::Url],
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
        serves(config, BAILIAN_QWEN_IMAGE)?;
        let forms = images_only(context, UPSTREAM)?;
        let body = request.get("json").and_then(Value::as_object).ok_or_else(|| {
            invalid("Bailian Qwen-Image models only generate from a JSON request")
        })?;
        let prompt = match body.get("prompt") {
            Some(value) if is_blob(value) => {
                return Err(invalid("'prompt' is longer than the gateway accepts"));
            }
            Some(Value::String(prompt)) => prompt,
            _ => return Err(invalid("Missing 'prompt' field")),
        };
        let n = requested(body.get("n"), MAX_IMAGES, UPSTREAM)?;
        let mut parts = image_url_parts(body.get("image_url"), url_entry)?;
        let input_images = u32::try_from(parts.len()).unwrap_or(u32::MAX);
        if input_images > MAX_IMAGE_INPUTS_PER_ROLE {
            return Err(invalid(format!(
                "at most {MAX_IMAGE_INPUTS_PER_ROLE} input images are accepted (got {input_images})"
            )));
        }
        parts.push(json!({ "text": prompt }));
        let mut parameters = Map::new();
        if body.contains_key("n") {
            parameters.insert("n".to_owned(), json!(n));
        }
        let size = body.get("size");
        if let Some(size) = size {
            parameters.insert(
                "size".to_owned(),
                size.as_str().map_or_else(|| with_refs(size), |text| json!(wan_size(text))),
            );
        }
        for key in PASSTHROUGH {
            if let Some(value) = body.get(key) {
                parameters.insert(key.to_owned(), with_refs(value));
            }
        }
        let template = json!({
            "model": context.upstream_model,
            "input": { "messages": [{ "role": "user", "content": parts }] },
            "parameters": parameters,
        });
        let mut descriptor = Map::new();
        descriptor.insert("method".to_owned(), json!("POST"));
        descriptor.insert("path".to_owned(), json!(GENERATION_PATH));
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
                inputs: ImageInputsV1 { input_image: input_images, ..ImageInputsV1::default() },
                requested_outputs: u32::try_from(n).unwrap_or(1),
                size: size.and_then(Value::as_str).and_then(size_fact),
                tier: ImageTierV1::default(),
                tier_candidates: None,
                metering_forms: forms,
                bounds: None,
            },
            descriptor,
            repeat: 1,
            response_body_form: ResponseBodyFormV1::Json,
            response_elision_paths: Vec::new(),
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
            return Ok(non_2xx(status, dashscope_error(status, status_code(status), &body)));
        }
        if carries_code(&body) {
            return Ok(ImageOutcomeV1::Unknown {
                reason: "a 2xx carrying a DashScope error code".to_owned(),
                error: Some(dashscope_error(502, ErrorCode::ProviderProtocolError, &body)),
            });
        }
        let artifacts = result_images(&body)
            .into_iter()
            .map(|(pointer, _)| {
                JsonPointerV1::parse(&pointer)
                    .map(|pointer| ImageArtifactV1::Url { pointer, media_type: None })
                    .map_err(|_| internal("invalid artifact pointer"))
            })
            .collect::<ComponentResultV1<Vec<_>>>()?;
        if artifacts.is_empty() {
            return Ok(ImageOutcomeV1::Unknown {
                reason: "a 2xx without an image".to_owned(),
                error: None,
            });
        }
        let images_reported = body
            .pointer("/usage/image_count")
            .and_then(whole_count)
            .and_then(|count| u32::try_from(count).ok());
        Ok(ImageOutcomeV1::Succeeded {
            artifacts,
            metering: ImageMeteringV1 {
                tokens: None,
                images_reported,
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
            return Err(internal("Bailian Qwen-Image renders exactly one succeeded round"));
        };
        let data: Vec<Value> = (0..artifacts.len())
            .map(|index| {
                json!({
                    "url": { "$south.artifact": { "index": index, "as": "url" } },
                    "revised_prompt": null,
                })
            })
            .collect();
        Ok(ImageRenderedV1 {
            template: json!({ "created": context.created, "data": data }).to_string(),
        })
    }
}
