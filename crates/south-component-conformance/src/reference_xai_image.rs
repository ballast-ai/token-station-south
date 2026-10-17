//! The native reference implementation of the `image-xai` component.
//!
//! xAI Grok Imagine generation and edit (image world step I3-7 / S-I-7, design record
//! `docs/design/2026-10-15-image-i3-7-xai.md`; image record §8, §9.1, §9.4 and §13, xAI rows).
//!
//! Transcribed from token-station-server's native xAI arm (`handler/images/xai.rs`:
//! `is_xai_imagine_image_model`, `normalize_xai_image_generation_body`,
//! `normalize_xai_image_edit_body`, `xai_image_edit_input_count`, `xai_requested_quality`,
//! `parse_xai_resolution_hint`, `upstream_cost_ticks`; the candidate cards of
//! `engine/token_counter/media.rs`; the OpenAI-compatible pass-through of
//! `handler/images/openai_compat.rs`; the JSON edit branch of `handler/images/handlers.rs`;
//! `OPENAI_COMPAT_IMAGE_OWNED` and `XAI_IMAGE_EDIT_OWNED` in `handler/images/execute.rs`):
//!
//! - **Scope** (XI-Q1): the six Grok Imagine ids the native arm recognises, generation and edit,
//!   both JSON. A body carrying a non-null `image`, `images`, `image_url` or `image_urls` is an
//!   edit, any other a generation (XI-Q2); a multipart edit is refused with the native text.
//! - **Request**: the client's body is forwarded as the native arm forwards it (every field the
//!   arm does not touch passes through, a host-elided string as an `as_is` reference), with
//!   `model` set to the upstream id, `size` removed and `resolution` normalised to `1k` / `2k`
//!   from `resolution` or else `size`; on an edit, `image_url` / `image_urls` become the official
//!   `image` / `images`. `POST v1/images/generations` or `v1/images/edits`, `bearer` on the
//!   configured slot.
//! - **Facts**: `requested_outputs` the client's `n`; on an edit, the input image count;
//!   `tier.resolution` always (`1k` when the request names none); on `grok-imagine-image-2.0`,
//!   `tier.quality` when `quality` pins `low` or `medium`, otherwise `tier_candidates`
//!   `{low, medium}` with the default `low` on a generation and `medium` on an edit (H5 §8.2,
//!   X-Q1 to X-Q3); `images` metering only.
//! - **Response**: each `data` entry's `b64_json` is an `inline` base64 artifact (`image/jpeg`,
//!   XI-Q3) and otherwise its `url` a `url` artifact with no declared media type;
//!   `usage.cost_in_usd_ticks` (1 USD = 10^10 ticks) is reported as the evidence fact
//!   `upstream_cost` in decimal dollars, `null` when absent or not a non-negative integer. The
//!   host picks the served tier from it (record §9.4 item 4) and bills its own price list.
//! - **Render**: the upstream body as the client received it natively, each image replaced by an
//!   artifact marker delivered in the form the upstream answered in.

use serde_json::{Map, Value, json};
use south_contracts::JsonPointerV1;
use south_contracts::image::{
    ArtifactEncodingV1, ImageArtifactV1, ImageCallContextV1, ImageFactsV1, ImageInputRoleV1,
    ImageInputsV1, ImageMeteringV1, ImageModelCapabilitiesV1, ImageOperationV1,
    ImageRenderContextV1, ImageResponseFormatV1, ImageTierV1, InputRoleKeyV1,
    MAX_IMAGE_INPUTS_PER_ROLE, MeteringFormV1, PreparedImageCallV1, TierCandidatesV1,
    TierDimensionV1, TierWordV1, UpstreamCostV1, UpstreamCurrencyV1, UsdAmountV1,
};
use south_contracts::media::{
    MediaLimitsV1, MediaRequestDescriptorV1, PathPatternV1, ResponseBodyFormV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorEnvelope, ProviderConfig};

use crate::image_shared::{
    bearer, capped, code_message, images_only, internal, invalid, is_blob, non_2xx,
    non_empty_inline, serves, status_code, view_status, with_refs,
};
use crate::{ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1};

/// The package name this reference is published as.
pub const NAME: &str = "image-xai";
/// The package version.
pub const VERSION: &str = "1.0.7";
/// The world this reference exports.
pub const WORLD: &str = "image-adapter-v1";
/// The family, as a host's image rows name it.
pub const XAI_IMAGE: &str = "xai-image";

const UPSTREAM: &str = "xAI";
const GENERATIONS_PATH: &str = "v1/images/generations";
const EDITS_PATH: &str = "v1/images/edits";
/// `grok-imagine-image-2.0`: the only id with a `quality` dimension.
pub const V2: &str = "grok-imagine-image-2.0";
/// The standard card's id.
pub const STANDARD: &str = "grok-imagine-image";
/// The quality family (`is_xai_quality_image_model`).
pub const QUALITY_FAMILY: [&str; 4] = [
    "grok-imagine-image-quality",
    "grok-imagine-image-pro",
    "grok-imagine-image-quality-latest",
    "grok-imagine-image-quality-20260403",
];
/// The native `OPENAI_COMPAT_IMAGE_OWNED`: request extras may not set these on a generation.
const GENERATION_IMMUTABLE: [&str; 7] =
    ["model", "n", "size", "image_size", "quality", "resolution", "response_format"];
/// The native `XAI_IMAGE_EDIT_OWNED`: request extras may not set these on an edit.
const EDIT_IMMUTABLE: [&str; 6] = ["model", "n", "resolution", "quality", "image", "images"];
/// The request keys that hold source images, in the native reader's priority order.
const SOURCE_KEYS: [&str; 4] = ["images", "image", "image_url", "image_urls"];
/// Where a source image's string can be: elided whatever its length, so the base64 of a data URL
/// never enters the sandbox.
const REQUEST_ELISION_PATHS: [&str; 7] = [
    "/image",
    "/image/url",
    "/images/*",
    "/images/*/url",
    "/image_url",
    "/image_url/*",
    "/image_urls/*",
];
/// The most images `n` asks for, on both surfaces (the native handlers' range).
const MAX_N: u64 = 10;
/// xAI's charge unit: 1 USD = 10^10 ticks (`XAI_TICKS_PER_MICRODOLLAR` is 10,000).
const TICKS_PER_USD: u64 = 10_000_000_000;
/// The default `resolution` word: the native `xai_resolution` reads an absent one as 1K.
const DEFAULT_RESOLUTION: &str = "1k";
/// The media type of a `b64_json` image (XI-Q3; xAI's output format is unverified).
const INLINE_MEDIA_TYPE: &str = "image/jpeg";
/// The multipart refusal (`edits_unsupported_reason`, the `xai` row).
const MULTIPART_EDIT: &str = "/v1/images/edits for xAI grok-imagine models requires Content-Type: \
     application/json with an `image` object or `images` array; the OpenAI multipart \
     images.edit() shape is not supported by xAI.";
/// The budget of the response skeleton carried in `extras` (the outcome frame holds 64 KiB).
const SKELETON_BUDGET: usize = 48 * 1024;

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct XaiImageReferenceV1;

/// Whether the native arm recognises `model` (`is_xai_imagine_image_model`).
#[must_use]
pub fn serves_model(model: &str) -> bool {
    model == STANDARD || model == V2 || QUALITY_FAMILY.contains(&model)
}

/// A request value as a refusal echoes it: as the native arm prints it (`serde_json`'s display),
/// except that a value holding an elided string is named rather than printed.
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

/// The handlers' strict `n`: absent or `null` is 1; a count in `1..=10`.
fn requested_n(body: &Map<String, Value>, endpoint: &str) -> ComponentResultV1<u32> {
    let n = match body.get("n") {
        None | Some(Value::Null) => 1,
        Some(value) => {
            let n = json_count(value).ok_or_else(|| {
                invalid(format!("'n' must be a non-negative integer (got {})", shown(value)))
            })?;
            u32::try_from(n)
                .map_err(|_| invalid(format!("'n' value {n} is too large to fit in u32")))?
        }
    };
    if (1..=MAX_N).contains(&u64::from(n)) {
        Ok(n)
    } else {
        Err(invalid(format!("'n' must be in 1..=10 for {endpoint}, got {n}")))
    }
}

/// `parse_xai_resolution_hint`: trimmed, lower-cased, without spaces, underscores and hyphens,
/// `*` read as `x`.
fn resolution_hint(value: &str, field: &str) -> ComponentResultV1<&'static str> {
    let compact: String = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .map(|c| if c == '*' { 'x' } else { c })
        .collect();
    match compact.as_str() {
        "" | "1k" | "1024" | "1024x1024" => Ok("1k"),
        "2k" | "2048" | "2048x2048" => Ok("2k"),
        _ => Err(invalid(format!(
            "xAI `{field}` must be 1k/2k (or OpenAI-style 1024x1024/2048x2048), got '{value}'"
        ))),
    }
}

/// One of `resolution` / `size` as the normaliser reads it: `None` when absent or `null`.
fn hint_field(body: &Map<String, Value>, field: &str) -> ComponentResultV1<Option<&'static str>> {
    match body.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) if is_blob(value) => Err(elided(field)),
        Some(Value::String(text)) => resolution_hint(text, field).map(Some),
        Some(_) => Err(invalid(format!("xAI `{field}` must be a string"))),
    }
}

/// `normalize_xai_resolution_fields`: `resolution` wins over `size`; `size` is always removed;
/// a derived word replaces `resolution`. Returns the word billed (`xai_resolution`: absent is
/// 1K).
fn normalise_resolution(body: &mut Map<String, Value>) -> ComponentResultV1<&'static str> {
    let derived = match hint_field(body, "resolution")? {
        Some(word) => Some(word),
        None => hint_field(body, "size")?,
    };
    body.remove("size");
    if let Some(word) = derived {
        body.insert("resolution".to_owned(), json!(word));
    }
    Ok(derived.unwrap_or(DEFAULT_RESOLUTION))
}

/// A string a request may carry where the native arm wants one: a string, or a host-elided one.
fn is_string_like(value: &Value) -> bool {
    value.is_string() || is_blob(value)
}

/// `image_url_values_to_images`: each entry must be a string (or an elided one).
fn url_entries(urls: &[Value]) -> ComponentResultV1<Value> {
    urls.iter()
        .map(|url| {
            if is_string_like(url) {
                Ok(json!({ "type": "image_url", "url": url }))
            } else {
                Err(invalid("xAI image URL arrays must contain only string URLs/data URIs"))
            }
        })
        .collect::<ComponentResultV1<Vec<_>>>()
        .map(Value::Array)
}

/// `normalize_xai_image_edit_body` after the resolution step: the gateway aliases `image_url` /
/// `image_urls` become the official `image` / `images`. A key holding `null` counts as present
/// when it is removed, as the native `obj.remove(..).is_some()` does.
fn fold_aliases(body: &mut Map<String, Value>) -> ComponentResultV1<()> {
    let present =
        |body: &Map<String, Value>, key: &str| body.get(key).is_some_and(|v| !v.is_null());
    let official = present(body, "image") || present(body, "images");
    let image_url = body.remove("image_url");
    let image_urls = body.remove("image_urls");
    if official && (image_url.is_some() || image_urls.is_some()) {
        return Err(invalid(
            "xAI image edits accept only one source-image field: use `image`, `images`, \
             `image_url`, or `image_urls`, not multiple forms",
        ));
    }
    if official {
        return Ok(());
    }
    match (image_url, image_urls) {
        (Some(url), None) if is_string_like(&url) => {
            body.insert("image".to_owned(), json!({ "type": "image_url", "url": url }));
        }
        (Some(Value::Array(urls)), None) | (None, Some(Value::Array(urls))) => {
            body.insert("images".to_owned(), url_entries(&urls)?);
        }
        (Some(value), None) => {
            return Err(invalid(format!(
                "xAI `image_url` must be a string URL/data URI or an array of them (got {})",
                shown(&value)
            )));
        }
        (None, Some(value)) => {
            return Err(invalid(format!(
                "xAI `image_urls` must be an array of string URLs/data URIs (got {})",
                shown(&value)
            )));
        }
        (Some(_), Some(_)) => {
            return Err(invalid("xAI image edits accept only one of `image_url` or `image_urls`"));
        }
        (None, None) => {}
    }
    Ok(())
}

/// `xai_image_edit_input_count`, without the per-model maximum (catalog data, record §8 R-1 = A):
/// `image` counts one whatever it holds, `images` its length.
fn input_count(body: &Map<String, Value>) -> ComponentResultV1<u32> {
    let image = body.get("image").filter(|value| !value.is_null());
    let images = body.get("images").filter(|value| !value.is_null());
    let count = match (image, images) {
        (Some(_), Some(_)) => {
            return Err(invalid("xAI image edits accept either `image` or `images`, not both"));
        }
        (Some(_), None) => 1,
        (None, Some(images)) => {
            let images =
                images.as_array().ok_or_else(|| invalid("xAI `images` must be an array"))?;
            u32::try_from(images.len()).unwrap_or(u32::MAX)
        }
        (None, None) => {
            return Err(invalid("xAI image edits require an `image` object or `images` array"));
        }
    };
    if count == 0 {
        return Err(invalid("xAI image edits need at least 1 input image, got 0"));
    }
    if count > MAX_IMAGE_INPUTS_PER_ROLE {
        return Err(invalid(format!(
            "xAI image edits accept at most {MAX_IMAGE_INPUTS_PER_ROLE} input images (got {count})"
        )));
    }
    Ok(count)
}

/// What `quality` asks of `grok-imagine-image-2.0` (`xai_requested_quality`): a pinned tier, or
/// `None` for `auto` / absent (the upstream chooses). Other ids have no such dimension.
fn requested_quality(
    upstream_model: &str,
    body: &Map<String, Value>,
) -> ComponentResultV1<Option<&'static str>> {
    if upstream_model != V2 {
        return Ok(None);
    }
    let value = match body.get("quality") {
        None | Some(Value::Null) => return Ok(None),
        Some(value) if is_blob(value) => return Err(elided("quality")),
        Some(value) => value,
    };
    match value.as_str().map(|text| text.trim().to_ascii_lowercase()).as_deref() {
        Some("auto") => Ok(None),
        Some("low") => Ok(Some("low")),
        Some("medium") => Ok(Some("medium")),
        _ => Err(invalid(format!(
            "xAI `quality` for grok-imagine-image-2.0 must be low, medium or auto (the price \
             card has no other tier), got {}",
            shown(value)
        ))),
    }
}

fn word(text: &str) -> ComponentResultV1<TierWordV1> {
    TierWordV1::parse(text).map_err(|_| internal(format!("`{text}` is not a tier word")))
}

fn tier(resolution: &str, quality: Option<&str>) -> ComponentResultV1<ImageTierV1> {
    Ok(ImageTierV1 { resolution: Some(word(resolution)?), quality: quality.map(word).transpose()? })
}

/// The tier facts of H5 §8.2 (X-Q1 to X-Q3, lv 2026-10-14): the resolution always; on 2.0 the
/// quality when pinned, else the candidates `low`, `medium` (native order) with the documented
/// default of the surface.
fn tier_facts(
    resolution: &str,
    quality: Option<&str>,
    upstream_model: &str,
    operation: ImageOperationV1,
) -> ComponentResultV1<(ImageTierV1, Option<TierCandidatesV1>)> {
    if upstream_model != V2 || quality.is_some() {
        return Ok((tier(resolution, quality)?, None));
    }
    let default = match operation {
        ImageOperationV1::Generate => "low",
        ImageOperationV1::Edit => "medium",
    };
    Ok((
        tier(resolution, None)?,
        Some(TierCandidatesV1 {
            candidates: vec![tier(resolution, Some("low"))?, tier(resolution, Some("medium"))?],
            default: tier(resolution, Some(default))?,
        }),
    ))
}

fn paths(patterns: &[&str]) -> ComponentResultV1<Vec<PathPatternV1>> {
    patterns
        .iter()
        .map(|pattern| PathPatternV1::parse(pattern).map_err(|_| internal("invalid path pattern")))
        .collect()
}

/// `usage.cost_in_usd_ticks` as decimal dollars (record §9.1): the native reader takes a JSON
/// integer that fits an `i64`; a negative one is no quote a list price can equal, so it is
/// reported as absent.
#[must_use]
pub fn upstream_cost(body: &Value) -> Option<UpstreamCostV1> {
    let ticks = body.pointer("/usage/cost_in_usd_ticks")?.as_i64()?;
    let ticks = u64::try_from(ticks).ok()?;
    let (whole, fraction) = (ticks / TICKS_PER_USD, ticks % TICKS_PER_USD);
    let amount = if fraction == 0 {
        whole.to_string()
    } else {
        let digits = format!("{fraction:010}");
        format!("{whole}.{}", digits.trim_end_matches('0'))
    };
    Some(UpstreamCostV1 {
        currency: UpstreamCurrencyV1::Usd,
        amount: UsdAmountV1::parse(&amount).ok()?,
    })
}

/// xAI's error body `{"code", "error"}` as an envelope: the code follows the status; the message is
/// the body's `error` string, `error.message` or `message`.
fn error_envelope(status: u16, body: &Value) -> ErrorEnvelope {
    let code = status_code(status);
    let message = [&body["error"], &body["error"]["message"], &body["message"]]
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

/// `value` with every host placeholder replaced by `null`: a placeholder never reaches a template.
fn without_placeholders(value: &Value) -> Value {
    match value {
        _ if is_blob(value) => Value::Null,
        Value::Object(map) => Value::Object(
            map.iter().map(|(key, item)| (key.clone(), without_placeholders(item))).collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_placeholders).collect()),
        other => other.clone(),
    }
}

/// The client body the render fills: the upstream body with each image `null` and every elided
/// string `null`. Over the budget, the rewritten prompts are dropped first, then everything but
/// `created`, `usage` and one empty object per image.
fn skeleton(body: &Value, fields: &[&str]) -> Value {
    let mut skeleton = without_placeholders(body);
    if let Some(entries) = skeleton["data"].as_array_mut() {
        for (entry, field) in entries.iter_mut().zip(fields) {
            if let Some(entry) = entry.as_object_mut() {
                entry.insert((*field).to_owned(), Value::Null);
            }
        }
    }
    if skeleton.to_string().len() <= SKELETON_BUDGET {
        return skeleton;
    }
    if let Some(entries) = skeleton["data"].as_array_mut() {
        for entry in entries.iter_mut().filter(|entry| entry.get("revised_prompt").is_some()) {
            entry["revised_prompt"] = Value::Null;
        }
    }
    if skeleton.to_string().len() <= SKELETON_BUDGET {
        return skeleton;
    }
    let mut minimal = Map::new();
    for key in ["created", "usage"] {
        if let Some(value) = skeleton.get(key) {
            minimal.insert(key.to_owned(), value.clone());
        }
    }
    minimal.insert("data".to_owned(), Value::Array(vec![json!({}); fields.len()]));
    let minimal = Value::Object(minimal);
    if minimal.to_string().len() <= SKELETON_BUDGET {
        minimal
    } else {
        json!({ "data": vec![json!({}); fields.len()] })
    }
}

impl XaiImageReferenceV1 {
    fn descriptor(
        config: &ProviderConfig,
        path: &str,
        template: Map<String, Value>,
    ) -> ComponentResultV1<MediaRequestDescriptorV1> {
        let mut descriptor = Map::new();
        descriptor.insert("method".to_owned(), json!("POST"));
        descriptor.insert("path".to_owned(), json!(path));
        if let Some(auth) = bearer(config) {
            descriptor.insert("auth".to_owned(), auth);
        }
        descriptor
            .insert("body".to_owned(), json!({ "json": { "template": Value::Object(template) } }));
        MediaRequestDescriptorV1::parse(&Value::Object(descriptor).to_string(), &MediaLimitsV1::V1)
            .map_err(|error| internal(format!("built an invalid descriptor: {error}")))
    }
}

impl ImageComponentV1 for XaiImageReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: NAME.to_owned(),
            version: VERSION.to_owned(),
            api_version: WORLD.to_owned(),
        }
    }

    /// Each configured model among the six ids: `generate` and `edit`, the four source-image keys
    /// as `input_image`, `url` and `b64_json`, `images` only, the `resolution` dimension (and
    /// `quality` on 2.0), the source-image strings elided.
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
        serves(config, XAI_IMAGE)?;
        let elision = paths(&REQUEST_ELISION_PATHS)?;
        Ok(config
            .models
            .iter()
            .filter(|model| serves_model(&model.model))
            .map(|model| ImageModelCapabilitiesV1 {
                model: model.model.clone(),
                operations: vec![ImageOperationV1::Generate, ImageOperationV1::Edit],
                input_roles: SOURCE_KEYS
                    .iter()
                    .map(|key| InputRoleKeyV1 {
                        key: (*key).to_owned(),
                        role: ImageInputRoleV1::InputImage,
                    })
                    .collect(),
                response_formats: vec![ImageResponseFormatV1::Url, ImageResponseFormatV1::B64Json],
                metering_forms: vec![MeteringFormV1::Images],
                token_buckets: Vec::new(),
                tier_dimensions: if model.model == V2 {
                    vec![TierDimensionV1::Resolution, TierDimensionV1::Quality]
                } else {
                    vec![TierDimensionV1::Resolution]
                },
                repeat: false,
                request_elision_paths: elision.clone(),
            })
            .collect())
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &ImageCallContextV1,
    ) -> ComponentResultV1<PreparedImageCallV1> {
        serves(config, XAI_IMAGE)?;
        let forms = images_only(context, UPSTREAM)?;
        let Some(body) = request.get("json").and_then(Value::as_object) else {
            return Err(if request.get("multipart").is_some() {
                invalid(MULTIPART_EDIT)
            } else {
                invalid("JSON body must be an object")
            });
        };
        let upstream_model = context.upstream_model.as_str();
        if !serves_model(upstream_model) {
            return Err(invalid(format!(
                "upstream model '{upstream_model}' is not an xAI Grok Imagine image model"
            )));
        }
        // XI-Q2: a body with a source image is an edit.
        let operation =
            if SOURCE_KEYS.iter().any(|key| body.get(*key).is_some_and(|value| !value.is_null())) {
                ImageOperationV1::Edit
            } else {
                ImageOperationV1::Generate
            };
        let (endpoint, path, immutable): (&str, &str, &[&str]) = match operation {
            ImageOperationV1::Generate => {
                ("/v1/images/generations", GENERATIONS_PATH, &GENERATION_IMMUTABLE)
            }
            ImageOperationV1::Edit => ("xAI /v1/images/edits", EDITS_PATH, &EDIT_IMMUTABLE),
        };
        // The native order: `n`, the resolution, then (edit) the source images and their count,
        // then the quality.
        let n = requested_n(body, endpoint)?;
        let mut template = body.clone();
        let resolution = normalise_resolution(&mut template)?;
        let mut inputs = ImageInputsV1::default();
        if operation == ImageOperationV1::Edit {
            fold_aliases(&mut template)?;
            inputs.input_image = input_count(&template)?;
        }
        let quality = requested_quality(upstream_model, &template)?;
        let (tier, tier_candidates) = tier_facts(resolution, quality, upstream_model, operation)?;
        template.insert("model".to_owned(), json!(upstream_model));
        let Value::Object(template) = with_refs(&Value::Object(template)) else {
            return Err(internal("the request body is not an object"));
        };
        Ok(PreparedImageCallV1 {
            facts: ImageFactsV1 {
                operation,
                inputs,
                requested_outputs: n,
                // `size` is never forwarded: the resolution word is the fact.
                size: None,
                tier,
                tier_candidates,
                metering_forms: forms,
                bounds: None,
            },
            descriptor: Self::descriptor(config, path, template)?,
            repeat: 1,
            response_body_form: ResponseBodyFormV1::Json,
            response_elision_paths: paths(&["/data/*/b64_json"])?,
            immutable_body_paths: immutable.iter().map(|path| (*path).to_owned()).collect(),
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
            return Ok(non_2xx(status, error_envelope(status, &body)));
        }
        if !body.is_object() {
            return Ok(unknown("a 2xx without a JSON object body"));
        }
        let Some(entries) = body["data"].as_array().filter(|entries| !entries.is_empty()) else {
            return Ok(unknown("a 2xx without images"));
        };
        let mut artifacts = Vec::with_capacity(entries.len());
        let mut fields = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let (field, artifact) = if non_empty_inline(&entry["b64_json"]) {
                let pointer = JsonPointerV1::parse(&format!("/data/{index}/b64_json"))
                    .map_err(|_| internal("invalid artifact pointer"))?;
                (
                    "b64_json",
                    ImageArtifactV1::Inline {
                        pointer,
                        encoding: ArtifactEncodingV1::Base64,
                        media_type: INLINE_MEDIA_TYPE.to_owned(),
                    },
                )
            } else if non_empty_inline(&entry["url"]) {
                let pointer = JsonPointerV1::parse(&format!("/data/{index}/url"))
                    .map_err(|_| internal("invalid artifact pointer"))?;
                ("url", ImageArtifactV1::Url { pointer, media_type: None })
            } else {
                return Ok(unknown("a data entry without an image"));
            };
            artifacts.push(artifact);
            fields.push(field);
        }
        Ok(ImageOutcomeV1::Succeeded {
            artifacts,
            metering: ImageMeteringV1 {
                tokens: None,
                images_reported: None,
                credits: None,
                upstream_cost: upstream_cost(&body),
            },
            extras: json!({ "body": skeleton(&body, &fields) }),
        })
    }

    fn render(
        &self,
        _state: &Value,
        outcomes: &[ImageOutcomeV1],
        _context: &ImageRenderContextV1,
    ) -> ComponentResultV1<ImageRenderedV1> {
        let [ImageOutcomeV1::Succeeded { artifacts, extras, .. }] = outcomes else {
            return Err(internal("xAI renders exactly one succeeded round"));
        };
        let mut body = extras["body"].clone();
        let entries =
            body["data"].as_array_mut().ok_or_else(|| internal("the skeleton has no data"))?;
        if entries.len() != artifacts.len() {
            return Err(internal("the skeleton and the artifacts disagree"));
        }
        for (index, (entry, artifact)) in entries.iter_mut().zip(artifacts).enumerate() {
            let delivery = match artifact {
                ImageArtifactV1::Url { .. } => "url",
                _ => "b64_json",
            };
            entry
                .as_object_mut()
                .ok_or_else(|| internal("a skeleton entry is not an object"))?
                .insert(
                    delivery.to_owned(),
                    json!({ "$south.artifact": { "index": index, "as": delivery } }),
                );
        }
        Ok(ImageRenderedV1 { template: body.to_string() })
    }
}
