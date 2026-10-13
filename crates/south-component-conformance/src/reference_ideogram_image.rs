//! The native reference implementation of the `image-ideogram` component.
//!
//! Ideogram 4.0 generation (image world step I3-5, design record
//! `docs/design/2026-10-13-image-i3-5-ideogram.md`; image record §13, Ideogram row).
//!
//! Transcribed from token-station-server's native Ideogram arm as the record summarises it
//! (`handler/images/ideogram.rs`: `ideogram_v4_speed`, `ideogram_map_size`,
//! `build_ideogram_v4_form`, `ideogram_prepare`, `ideogram_dispatch`,
//! `ideogram_image_result_to_openai`; `IDEOGRAM_IMAGE_OWNED` in `images/execute.rs`):
//!
//! - **Scope** (IG-Q1): the three v4 ids the native arm maps, generation only. The upstream id
//!   pins the `rendering_speed`: `ideogram-v4-turbo` `TURBO`, `ideogram-v4` `DEFAULT`,
//!   `ideogram-v4-quality` `QUALITY`.
//! - **Request** (§3): `POST v1/ideogram-v4/generate`, a multipart body of text parts only (no
//!   `content-type`; the host's encoder writes it), no `accept`, `header_secret` `api-key` on the
//!   configured slot. Parts in the native order: `text_prompt` (the client's `prompt` as sent),
//!   `rendering_speed`, and `resolution` only when `size` names one: an exact member of the
//!   38-entry v4 list, or the member nearest a `WxH` by log aspect ratio (the earlier member on a
//!   tie). Every other field is dropped.
//! - **Refusals before dispatch**: the native texts in the native order after the upstream id
//!   (`prompt`, `n`, `response_format`, `size`); an elided `prompt`, `response_format` or `size` is
//!   refused as longer than the gateway accepts.
//! - **Facts**: `generate`, no inputs, one output, `size` the resolution sent (IG-Q3), no tier,
//!   `images` only, `repeat` 1, `response_body_form` `json`, nothing elided.
//! - **Response** (§4): each `data` entry that is not `is_image_safe: false` and has a non-empty
//!   `url` is a `url` artifact with no declared media type (IG-Q2: the host sniffs the bytes);
//!   Ideogram's rewritten `prompt` of each delivered entry is carried in `extras`. A non-empty
//!   `data` with every entry filtered is `rejected` as a content-policy refusal; a missing, empty
//!   or non-array `data` and a body that is not a JSON object are `unknown`. `images_reported` is
//!   always `null`.
//! - **Render**: `{"created", "data": [{"url" | "b64_json", "revised_prompt"}]}`, the delivery the
//!   client asked for (`url` passes the upstream URL through; `b64_json` has the host fetch it).

use serde_json::{Map, Value, json};
use south_contracts::JsonPointerV1;
use south_contracts::image::{
    ImageArtifactV1, ImageCallContextV1, ImageFactsV1, ImageInputsV1, ImageMeteringV1,
    ImageModelCapabilitiesV1, ImageOperationV1, ImageRenderContextV1, ImageResponseFormatV1,
    ImageSizeV1, ImageTierV1, MeteringFormV1, PreparedImageCallV1,
};
use south_contracts::media::{
    MediaLimitsV1, MediaPartV1, MediaRequestDescriptorV1, ResponseBodyFormV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

use crate::image_shared::{
    capped, code_message, images_only, internal, invalid, is_blob, non_2xx, non_empty_inline,
    serves, status_code, view_status,
};
use crate::{ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1};

/// The package name this reference is published as.
pub const NAME: &str = "image-ideogram";
/// The package version.
pub const VERSION: &str = "1.0.2";
/// The world this reference exports.
pub const WORLD: &str = "image-adapter-v1";
/// The family, as a host's image rows name it.
pub const IDEOGRAM_IMAGE: &str = "ideogram-image";

const UPSTREAM: &str = "Ideogram";
const GENERATE_PATH: &str = "v1/ideogram-v4/generate";
/// The upstream ids the native arm maps (IG-Q1), in the record's order.
pub const MODELS: [&str; 3] = ["ideogram-v4-turbo", "ideogram-v4", "ideogram-v4-quality"];
/// The native arm's `IDEOGRAM_IMAGE_OWNED`: request extras may not set these parts.
/// `num_images` is a part the component never writes, so immutable means "stays absent".
const IMMUTABLE_PART_NAMES: [&str; 4] =
    ["text_prompt", "rendering_speed", "resolution", "num_images"];
/// The 38 resolutions `/v1/ideogram-v4/generate` accepts, in the native (spec) order.
const RESOLUTIONS: [&str; 38] = [
    "2048x2048",
    "1440x2880",
    "2880x1440",
    "1664x2496",
    "2496x1664",
    "1792x2240",
    "2240x1792",
    "1440x2560",
    "2560x1440",
    "1600x2560",
    "2560x1600",
    "1728x2304",
    "2304x1728",
    "1296x3168",
    "3168x1296",
    "1152x2944",
    "2944x1152",
    "1248x3328",
    "3328x1248",
    "1280x3072",
    "3072x1280",
    "1024x3072",
    "3072x1024",
    "1024x1024",
    "896x1120",
    "1120x896",
    "864x1152",
    "1152x864",
    "832x1248",
    "1248x832",
    "800x1280",
    "1280x800",
    "720x1280",
    "1280x720",
    "720x1440",
    "1440x720",
    "512x1536",
    "1536x512",
];
/// The answer to a 2xx whose every entry Ideogram filtered (record §9.2).
const ALL_FILTERED: &str = "Ideogram filtered all generated images as unsafe; nothing was billed";

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct IdeogramImageReferenceV1;

/// `ideogram_v4_speed`: the `rendering_speed` an upstream id pins.
fn rendering_speed(upstream_model: &str) -> ComponentResultV1<&'static str> {
    match upstream_model {
        "ideogram-v4" => Ok("DEFAULT"),
        "ideogram-v4-turbo" => Ok("TURBO"),
        "ideogram-v4-quality" => Ok("QUALITY"),
        other => Err(invalid(format!(
            "Model upstream id '{other}' is not an Ideogram v4 image SKU; expected \
             'ideogram-v4', 'ideogram-v4-turbo', or 'ideogram-v4-quality'"
        ))),
    }
}

/// A request value as a refusal echoes it: as the native arm prints it, except that a value
/// holding an elided string is named rather than printed (a placeholder never reaches a message).
fn shown(value: &Value) -> String {
    fn holds_blob(value: &Value) -> bool {
        match value {
            Value::Object(map) => is_blob(value) || map.values().any(holds_blob),
            Value::Array(items) => items.iter().any(holds_blob),
            _ => false,
        }
    }
    if !holds_blob(value) {
        return value.to_string();
    }
    match value {
        Value::Array(_) => "an array".to_owned(),
        _ if is_blob(value) => "a string longer than the gateway accepts".to_owned(),
        _ => "an object".to_owned(),
    }
}

fn elided(field: &str) -> ErrorEnvelope {
    invalid(format!("'{field}' is longer than the gateway accepts"))
}

/// The prompt: present, a string (`null` included in the refusal), not blank.
fn prompt(body: &Map<String, Value>) -> ComponentResultV1<&str> {
    let prompt = match body.get("prompt") {
        None => return Err(invalid("Missing 'prompt' field")),
        Some(value) if is_blob(value) => return Err(elided("prompt")),
        Some(Value::String(prompt)) => prompt,
        Some(other) => {
            return Err(invalid(format!("'prompt' must be a string (got {})", shown(other))));
        }
    };
    if prompt.trim().is_empty() {
        return Err(invalid("'prompt' must not be empty"));
    }
    Ok(prompt)
}

/// The native coercion of a count: an integer or a whole non-negative float.
fn json_count(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        let float = value.as_f64()?;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        (float.is_finite() && float >= 0.0 && float.fract() == 0.0 && float < u64::MAX as f64)
            .then_some(float as u64)
    })
}

/// v4 has no `num_images`: one image per request.
fn check_n(body: &Map<String, Value>) -> ComponentResultV1<()> {
    let n = match body.get("n") {
        None | Some(Value::Null) => 1,
        Some(value) => json_count(value).ok_or_else(|| {
            invalid(format!("'n' must be a non-negative integer (got {})", shown(value)))
        })?,
    };
    if n == 1 {
        Ok(())
    } else {
        Err(invalid(format!(
            "Ideogram v4 generates exactly one image per request; 'n' must be 1 (got {n})"
        )))
    }
}

/// The delivery the client asked for: `url` (the default) or `b64_json`.
fn response_format(body: &Map<String, Value>) -> ComponentResultV1<&'static str> {
    match body.get("response_format") {
        None | Some(Value::Null) => Ok("url"),
        Some(value) if is_blob(value) => Err(elided("response_format")),
        Some(Value::String(format)) if format == "url" => Ok("url"),
        Some(Value::String(format)) if format == "b64_json" => Ok("b64_json"),
        Some(Value::Object(object)) => match object.get("type") {
            Some(kind) if is_blob(kind) => Err(elided("response_format")),
            Some(Value::String(kind)) => match kind.as_str() {
                "url" => Ok("url"),
                "b64_json" => Ok("b64_json"),
                other => Err(invalid(format!(
                    "'response_format.type' must be 'url' or 'b64_json' for Ideogram models (got \
                     '{other}')"
                ))),
            },
            _ => Ok("url"),
        },
        Some(other) => Err(invalid(format!(
            "'response_format' must be 'url', 'b64_json', or {{\"type\": \"...\"}} for Ideogram \
             models (got {})",
            shown(other)
        ))),
    }
}

/// `ideogram_map_size` on the client's `size`: the v4 resolution to send, if any.
fn resolution(body: &Map<String, Value>) -> ComponentResultV1<Option<&'static str>> {
    let size = match body.get("size") {
        None | Some(Value::Null) => return Ok(None),
        Some(value) if is_blob(value) => return Err(elided("size")),
        Some(Value::String(size)) => size.trim(),
        Some(other) => {
            return Err(invalid(format!("'size' must be a string (got {})", shown(other))));
        }
    };
    if size.is_empty() || size.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }
    if let Some(member) = RESOLUTIONS.iter().find(|member| **member == size) {
        return Ok(Some(member));
    }
    let (width, height) = positive_dimensions(size).ok_or_else(|| {
        invalid(format!(
            "'size' value '{size}' is not supported for Ideogram models; pass 'auto', a WxH pair \
             like '1024x1024', or a native Ideogram v4 resolution (e.g. '2048x2048')"
        ))
    })?;
    Ok(Some(nearest_resolution(width, height)))
}

/// `WxH` split at the first lower-case `x` only (the native parser is case-sensitive), each side
/// a positive `u32` (Rust's parser accepts a leading `+`).
fn positive_dimensions(size: &str) -> Option<(u32, u32)> {
    let (width, height) = size.split_once('x')?;
    let width = width.trim().parse::<u32>().ok().filter(|&n| n > 0)?;
    let height = height.trim().parse::<u32>().ok().filter(|&n| n > 0)?;
    Some((width, height))
}

/// A member's width and height.
fn member_edges(member: &str) -> (u32, u32) {
    let (width, height) = member.split_once('x').unwrap_or(("1", "1"));
    (width.parse().unwrap_or(1), height.parse().unwrap_or(1))
}

/// The member nearest `width`×`height` by `|ln(rw/rh) − ln(w/h)|`, the earlier member on a tie.
fn nearest_resolution(width: u32, height: u32) -> &'static str {
    let want = (f64::from(width) / f64::from(height)).ln();
    let distance = |member: &str| {
        let (w, h) = member_edges(member);
        ((f64::from(w) / f64::from(h)).ln() - want).abs()
    };
    RESOLUTIONS
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .copied()
        .unwrap_or(RESOLUTIONS[0])
}

fn text(name: &str, value: impl Into<String>) -> MediaPartV1 {
    MediaPartV1::Text { name: name.to_owned(), value: value.into() }
}

/// `{"arm": "header_secret", "header": "api-key", "slot": …}` on the configured slot.
fn api_key(config: &ProviderConfig) -> Option<Value> {
    config
        .auth
        .as_ref()
        .map(|slot| json!({ "arm": "header_secret", "header": "api-key", "slot": slot.as_str() }))
}

/// Ideogram's error body as an envelope: the code follows the status; the message is the body's
/// `error`, `message` or `detail` string, or `error.message`.
fn error_envelope(status: u16, body: &Value) -> ErrorEnvelope {
    let code = status_code(status);
    let message = [&body["error"], &body["message"], &body["detail"], &body["error"]["message"]]
        .into_iter()
        .find_map(Value::as_str)
        .unwrap_or_default();
    let mut envelope = ErrorEnvelope::new(code, status, code_message(code));
    envelope.provider_message = capped(message);
    envelope
}

fn unknown(reason: &str) -> ImageOutcomeV1 {
    ImageOutcomeV1::Unknown { reason: reason.to_owned(), error: None }
}

impl ImageComponentV1 for IdeogramImageReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: NAME.to_owned(),
            version: VERSION.to_owned(),
            api_version: WORLD.to_owned(),
        }
    }

    /// Decision 1: each configured model among the three ids; `generate` only, no input roles,
    /// `url` and `b64_json`, `images` only, nothing elided.
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
        serves(config, IDEOGRAM_IMAGE)?;
        Ok(config
            .models
            .iter()
            .filter(|model| MODELS.contains(&model.model.as_str()))
            .map(|model| ImageModelCapabilitiesV1 {
                model: model.model.clone(),
                operations: vec![ImageOperationV1::Generate],
                input_roles: Vec::new(),
                response_formats: vec![ImageResponseFormatV1::Url, ImageResponseFormatV1::B64Json],
                metering_forms: vec![MeteringFormV1::Images],
                token_buckets: Vec::new(),
                tier_dimensions: Vec::new(),
                repeat: false,
                request_elision_paths: Vec::new(),
            })
            .collect())
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &ImageCallContextV1,
    ) -> ComponentResultV1<PreparedImageCallV1> {
        serves(config, IDEOGRAM_IMAGE)?;
        let forms = images_only(context, UPSTREAM)?;
        let body = request
            .get("json")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("Ideogram image models only generate from a JSON request"))?;
        // The native order: the upstream id, then `prompt`, `n`, `response_format`, `size`.
        let speed = rendering_speed(&context.upstream_model)?;
        let prompt = prompt(body)?;
        check_n(body)?;
        let format = response_format(body)?;
        let resolution = resolution(body)?;

        let mut parts = vec![text("text_prompt", prompt), text("rendering_speed", speed)];
        if let Some(resolution) = resolution {
            parts.push(text("resolution", resolution));
        }
        let mut descriptor = Map::new();
        descriptor.insert("method".to_owned(), json!("POST"));
        descriptor.insert("path".to_owned(), json!(GENERATE_PATH));
        if let Some(auth) = api_key(config) {
            descriptor.insert("auth".to_owned(), auth);
        }
        descriptor.insert("body".to_owned(), json!({ "multipart": { "parts": parts } }));
        let descriptor = MediaRequestDescriptorV1::parse(
            &Value::Object(descriptor).to_string(),
            &MediaLimitsV1::V1,
        )
        .map_err(|error| internal(format!("built an invalid descriptor: {error}")))?;
        Ok(PreparedImageCallV1 {
            facts: ImageFactsV1 {
                operation: ImageOperationV1::Generate,
                inputs: ImageInputsV1::default(),
                requested_outputs: 1,
                // IG-Q3: the resolution Ideogram renders, when one is sent.
                size: resolution.map(|member| {
                    let (width, height) = member_edges(member);
                    ImageSizeV1 { width, height }
                }),
                tier: ImageTierV1::default(),
                tier_candidates: None,
                metering_forms: forms,
                bounds: None,
            },
            descriptor,
            repeat: 1,
            response_body_form: ResponseBodyFormV1::Json,
            response_elision_paths: Vec::new(),
            immutable_body_paths: IMMUTABLE_PART_NAMES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            state: json!({ "format": format }).to_string(),
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
            return Ok(non_2xx(status, error_envelope(status, &body)));
        }
        if !body.is_object() {
            return Ok(unknown("a 2xx without a JSON object body"));
        }
        let Some(entries) = body["data"].as_array().filter(|entries| !entries.is_empty()) else {
            return Ok(unknown("a 2xx without images"));
        };
        let mut artifacts = Vec::new();
        let mut revised_prompts = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            // The explicit safety flag first, then Ideogram's per-image signal: no URL.
            if entry["is_image_safe"].as_bool() == Some(false) || !non_empty_inline(&entry["url"]) {
                continue;
            }
            artifacts.push(ImageArtifactV1::Url {
                pointer: JsonPointerV1::parse(&format!("/data/{index}/url"))
                    .map_err(|_| internal("invalid artifact pointer"))?,
                media_type: None,
            });
            revised_prompts.push(entry["prompt"].as_str().map_or(Value::Null, |text| json!(text)));
        }
        if artifacts.is_empty() {
            return Ok(ImageOutcomeV1::Rejected {
                error: ErrorEnvelope::new(ErrorCode::ContentPolicy, 422, ALL_FILTERED),
            });
        }
        Ok(ImageOutcomeV1::Succeeded {
            artifacts,
            metering: ImageMeteringV1 {
                tokens: None,
                images_reported: None,
                credits: None,
                upstream_cost: None,
            },
            extras: json!({ "revised_prompts": revised_prompts }),
        })
    }

    fn render(
        &self,
        state: &Value,
        outcomes: &[ImageOutcomeV1],
        context: &ImageRenderContextV1,
    ) -> ComponentResultV1<ImageRenderedV1> {
        let [ImageOutcomeV1::Succeeded { artifacts, extras, .. }] = outcomes else {
            return Err(internal("Ideogram renders exactly one succeeded round"));
        };
        let delivery = match state["format"].as_str() {
            Some("b64_json") => "b64_json",
            _ => "url",
        };
        let data: Vec<Value> = (0..artifacts.len())
            .map(|index| {
                json!({
                    delivery: { "$south.artifact": { "index": index, "as": delivery } },
                    "revised_prompt": extras["revised_prompts"].get(index).cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Ok(ImageRenderedV1 {
            template: json!({ "created": context.created, "data": data }).to_string(),
        })
    }
}
