//! The native reference implementation of the `image-stability` component.
//!
//! Stability AI's Stable Image generation (image world step I3-4, design record
//! `docs/design/2026-10-13-image-i3-4-stability.md`; image record §13, Stability row).
//!
//! Transcribed from token-station-server's native Stability arm as the record summarises it
//! (`handler/images/stability.rs`: `classify_stability_model`, `build_stability_generation_request`,
//! `stability_prepare`, `stability_image_result_to_openai`; `STABILITY_IMAGE_OWNED` in
//! `images/execute.rs`):
//!
//! - **Scope** (SI-Q1): the five upstream ids the native arm classifies, generation only.
//!   `stable-image-ultra` and `stable-image-core` post to `v2beta/stable-image/generate/{ultra,core}`;
//!   `sd3.5-large`, `sd3.5-large-turbo` and `sd3.5-medium` to `v2beta/stable-image/generate/sd3`
//!   with `model` and `mode` parts.
//! - **Request** (§3): a multipart body (no `content-type`; the host's encoder writes it), header
//!   `accept: application/json` (SI-Q2), `bearer` on the configured slot. Text parts in the native
//!   order: `prompt`, `output_format` (the client's, else `png`), `negative_prompt`, `seed`,
//!   `style_preset`; with a source image `strength` and (SD3) `mode` = `image-to-image`; without
//!   one the derived `aspect_ratio`; SD3 `model`, `mode` = `text-to-image` without a source image,
//!   and `cfg_scale`. The source image (`image_url` or `image`, a string or `{"url"}`, always
//!   elided) is the last part, `image`, through `from_data_url`, named `source.<ext>` and labelled
//!   with its normalised media type. Every other field is dropped.
//! - **Refusals before dispatch**: the native texts and the native order (§3), with the size of a
//!   source image judged from its payload length (decision 2).
//! - **Facts**: `generate`, `inputs.input_image` 1 with a source image, one output, no `size`, no
//!   tier, `images` only, `repeat` 1, `response_body_form` `json`, `/image` elided.
//! - **Response** (§4): `{"image", "finish_reason", "seed"}`. `SUCCESS` and `CONTENT_FILTERED`
//!   (SI-Q3) succeed with one inline base64 artifact typed from the `output_format` sent; a missing
//!   or empty image, a missing finish reason, any other finish reason or a body that is not a JSON
//!   object is `unknown`. `images_reported` is always `null`. A non-2xx carries Stability's
//!   `{"name", "errors"}` as the provider message; `content_moderation` is `content_policy`.
//! - **Render**: `{"created", "data": [{"b64_json", "revised_prompt": null}]}`.

use serde_json::{Map, Value, json};
use south_contracts::JsonPointerV1;
use south_contracts::image::{
    ArtifactEncodingV1, ImageArtifactV1, ImageCallContextV1, ImageFactsV1, ImageInputRoleV1,
    ImageInputsV1, ImageMeteringV1, ImageModelCapabilitiesV1, ImageOperationV1,
    ImageRenderContextV1, ImageResponseFormatV1, ImageTierV1, InputRoleKeyV1, MeteringFormV1,
    PreparedImageCallV1,
};
use south_contracts::media::{
    MediaLimitsV1, MediaPartV1, MediaRequestDescriptorV1, MediaTransformV1, PathPatternV1,
    ResponseBodyFormV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

use crate::image_shared::{
    bearer, capped, code_message, images_only, internal, invalid, is_blob, non_2xx,
    non_empty_inline, serves, status_code, view_status,
};
use crate::{ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1};

/// The package name this reference is published as.
pub const NAME: &str = "image-stability";
/// The package version.
pub const VERSION: &str = "1.0.19";
/// The world this reference exports.
pub const WORLD: &str = "image-adapter-v1";
/// The family, as a host's image rows name it.
pub const STABILITY_IMAGE: &str = "stability-image";

const UPSTREAM: &str = "Stability AI";
/// The upstream ids the native arm classifies (SI-Q1), in its order.
pub const MODELS: [&str; 5] =
    ["stable-image-ultra", "stable-image-core", "sd3.5-large", "sd3.5-large-turbo", "sd3.5-medium"];
/// The native arm's `STABILITY_IMAGE_OWNED`: request extras may not set these parts.
const IMMUTABLE_PART_NAMES: [&str; 3] = ["model", "aspect_ratio", "output_format"];
/// Request-side elision: the source image, a data URL whose bytes never enter the sandbox.
const REQUEST_ELISION_PATHS: [&str; 4] = ["/image_url", "/image_url/url", "/image", "/image/url"];
/// Response-side elision: the image.
const RESPONSE_ELISION_PATH: &str = "/image";
const ALLOWED_ASPECT_RATIOS: [&str; 9] =
    ["21:9", "16:9", "3:2", "5:4", "1:1", "4:5", "2:3", "9:16", "9:21"];
const STYLE_PRESETS: [&str; 17] = [
    "enhance",
    "anime",
    "photographic",
    "digital-art",
    "comic-book",
    "fantasy-art",
    "line-art",
    "analog-film",
    "neon-punk",
    "isometric",
    "low-poly",
    "origami",
    "modeling-compound",
    "cinematic",
    "3d-model",
    "pixel-art",
    "tile-texture",
];
/// The native decoded-size limit of a source image.
const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
/// The longest canonical base64 payload whose decoded size can be [`MAX_IMAGE_BYTES`]: the
/// component sees the payload's length, not its padding (decision 2).
const MAX_PAYLOAD_CHARS: u64 = MAX_IMAGE_BYTES.div_ceil(3) * 4;
const BLOB_KEY: &str = "$south.blob";
const DATA_PREFIX: &str = "data:";
const BASE64_MARKER: &str = ";base64,";

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct StabilityImageReferenceV1;

/// The three native endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ultra,
    Core,
    Sd3(&'static str),
}

impl Kind {
    /// `classify_stability_model`.
    fn of(upstream_model: &str) -> ComponentResultV1<Self> {
        match upstream_model {
            "stable-image-ultra" => Ok(Self::Ultra),
            "stable-image-core" => Ok(Self::Core),
            "sd3.5-large" => Ok(Self::Sd3("sd3.5-large")),
            "sd3.5-large-turbo" => Ok(Self::Sd3("sd3.5-large-turbo")),
            "sd3.5-medium" => Ok(Self::Sd3("sd3.5-medium")),
            other => Err(invalid(format!(
                "Model upstream id '{other}' is not a supported Stability Stable Image SKU; \
                 expected stable-image-ultra, stable-image-core, sd3.5-large, \
                 sd3.5-large-turbo, or sd3.5-medium"
            ))),
        }
    }

    const fn path(self) -> &'static str {
        match self {
            Self::Ultra => "v2beta/stable-image/generate/ultra",
            Self::Core => "v2beta/stable-image/generate/core",
            Self::Sd3(_) => "v2beta/stable-image/generate/sd3",
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Ultra => "Stable Image Ultra",
            Self::Core => "Stable Image Core",
            Self::Sd3(_) => "Stable Diffusion 3.5",
        }
    }
}

/// The client's source image: the blob to send and its normalised media type.
struct Source {
    blob: String,
    media_type: &'static str,
    extension: &'static str,
}

fn paths(patterns: &[&str]) -> ComponentResultV1<Vec<PathPatternV1>> {
    patterns
        .iter()
        .map(|pattern| PathPatternV1::parse(pattern).map_err(|_| internal("invalid elision path")))
        .collect()
}

/// Whether `value` holds a host placeholder anywhere.
fn holds_blob(value: &Value) -> bool {
    match value {
        Value::Object(map) => is_blob(value) || map.values().any(holds_blob),
        Value::Array(items) => items.iter().any(holds_blob),
        _ => false,
    }
}

/// A request value as a refusal echoes it: as the native arm prints it, except that a value
/// holding an elided string is named rather than printed (a placeholder never reaches a message).
fn shown(value: &Value) -> String {
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

/// `stability_prepare`'s count check: one image per request.
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
            "Stability AI generates exactly one image per request; `n` must be 1 (got {n})"
        )))
    }
}

/// `parse_stability_response_format`: `b64_json` (or absent) only.
fn check_response_format(body: &Map<String, Value>) -> ComponentResultV1<()> {
    match body.get("response_format") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(format)) if format == "b64_json" => Ok(()),
        Some(Value::String(format)) if format == "url" => Err(invalid(
            "Stability AI does not return hosted image URLs; omit `response_format` or set it to \
             `b64_json`",
        )),
        Some(value @ Value::Object(object)) if !is_blob(value) => {
            match object.get("type").and_then(Value::as_str) {
                None | Some("b64_json") => Ok(()),
                Some("url") => Err(invalid(
                    "Stability AI does not return hosted image URLs; omit `response_format` or set \
                     `response_format.type` to `b64_json`",
                )),
                Some(other) => Err(invalid(format!(
                    "`response_format.type` must be `b64_json` for Stability AI models (got \
                     '{other}')"
                ))),
            }
        }
        Some(other) => Err(invalid(format!(
            "`response_format` must be `b64_json` or {{\"type\":\"b64_json\"}} for Stability AI \
             models (got {})",
            shown(other)
        ))),
    }
}

/// The prompt: present, a string, not blank.
fn prompt(body: &Map<String, Value>) -> ComponentResultV1<&str> {
    let prompt = match body.get("prompt") {
        None => return Err(invalid("Missing 'prompt' field")),
        Some(value) if is_blob(value) => return Err(elided("prompt")),
        Some(Value::String(prompt)) => prompt,
        Some(other) => {
            return Err(invalid(format!("`prompt` must be a string (got {})", shown(other))));
        }
    };
    if prompt.trim().is_empty() {
        return Err(invalid("`prompt` must not be empty for Stability AI models"));
    }
    Ok(prompt)
}

/// `optional_string_field`: absent or `null` is `None`; an elided string is refused.
fn optional_string<'a>(
    body: &'a Map<String, Value>,
    field: &str,
) -> ComponentResultV1<Option<&'a str>> {
    match body.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(value) if is_blob(value) => Err(elided(field)),
        Some(other) => Err(invalid(format!("`{field}` must be a string (got {})", shown(other)))),
    }
}

/// `optional_number_field`: the value as `serde_json` renders it, within `min..=max` (decision 3).
fn optional_number(
    body: &Map<String, Value>,
    field: &str,
    min: f64,
    max: f64,
) -> ComponentResultV1<Option<String>> {
    let Some(value) = body.get(field).filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let number = value
        .as_f64()
        .ok_or_else(|| invalid(format!("`{field}` must be a number (got {})", shown(value))))?;
    if !number.is_finite() || number < min || number > max {
        return Err(invalid(format!(
            "`{field}` must be in {min}..={max} for Stability AI models (got {number})"
        )));
    }
    Ok(Some(value.to_string()))
}

/// `optional_seed_field`: a whole number from 0 to 4294967294.
fn optional_seed(body: &Map<String, Value>) -> ComponentResultV1<Option<String>> {
    let Some(value) = body.get("seed").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let seed = json_count(value)
        .ok_or_else(|| invalid(format!("`seed` must be an integer (got {})", shown(value))))?;
    if seed > 4_294_967_294 {
        return Err(invalid(format!(
            "`seed` must be in 0..=4294967294 for Stability AI models (got {seed})"
        )));
    }
    Ok(Some(seed.to_string()))
}

/// `optional_output_format`: `png` (the default), `jpeg` or `webp`.
fn output_format(body: &Map<String, Value>) -> ComponentResultV1<&'static str> {
    match body.get("output_format") {
        None | Some(Value::Null) => Ok("png"),
        Some(Value::String(format)) => match format.as_str() {
            "png" => Ok("png"),
            "jpeg" => Ok("jpeg"),
            "webp" => Ok("webp"),
            other => Err(invalid(format!(
                "`output_format` must be one of png, jpeg, webp for Stability AI models (got \
                 '{other}')"
            ))),
        },
        Some(value) if is_blob(value) => Err(elided("output_format")),
        Some(other) => {
            Err(invalid(format!("`output_format` must be a string (got {})", shown(other))))
        }
    }
}

/// `optional_style_preset`: one of the seventeen words.
fn style_preset(body: &Map<String, Value>) -> ComponentResultV1<Option<&str>> {
    let Some(style) = optional_string(body, "style_preset")? else { return Ok(None) };
    if STYLE_PRESETS.contains(&style) {
        Ok(Some(style))
    } else {
        Err(invalid(format!(
            "`style_preset` value '{style}' is not supported for Stability AI models; allowed \
             values: {}",
            STYLE_PRESETS.join(", ")
        )))
    }
}

/// `stability_aspect_ratio_from_request`: the client's `aspect_ratio` wins, else `size`.
fn aspect_ratio(body: &Map<String, Value>) -> ComponentResultV1<Option<String>> {
    if let Some(value) = body.get("aspect_ratio") {
        return match value {
            Value::Null => Ok(None),
            Value::String(ratio) if ALLOWED_ASPECT_RATIOS.contains(&ratio.as_str()) => {
                Ok(Some(ratio.clone()))
            }
            Value::String(ratio) => Err(invalid(format!(
                "`aspect_ratio` value '{ratio}' is not supported for Stability AI models; allowed \
                 values: {}",
                ALLOWED_ASPECT_RATIOS.join(", ")
            ))),
            _ if is_blob(value) => Err(elided("aspect_ratio")),
            other => {
                Err(invalid(format!("`aspect_ratio` must be a string (got {})", shown(other))))
            }
        };
    }
    let size = match body.get("size") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(size)) => size.trim(),
        Some(value) if is_blob(value) => return Err(elided("size")),
        Some(other) => {
            return Err(invalid(format!("`size` must be a string (got {})", shown(other))));
        }
    };
    if size.is_empty() || size.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }
    if ALLOWED_ASPECT_RATIOS.contains(&size) {
        return Ok(Some(size.to_owned()));
    }
    let (width, height) = positive_dimensions(size).ok_or_else(|| {
        invalid(format!(
            "`size` value '{size}' is not supported for Stability AI models; pass `auto`, a WxH \
             pair like `1024x1024`, or a native aspect ratio ({})",
            ALLOWED_ASPECT_RATIOS.join(", ")
        ))
    })?;
    Ok(Some(nearest_aspect_ratio(width, height).to_owned()))
}

/// `parse_positive_dimensions`: `WxH`, either case of `x`, each a positive `u32`.
fn positive_dimensions(size: &str) -> Option<(u32, u32)> {
    let lower = size.to_ascii_lowercase();
    let (width, height) = lower.split_once('x')?;
    let width = width.trim().parse::<u32>().ok().filter(|&n| n > 0)?;
    let height = height.trim().parse::<u32>().ok().filter(|&n| n > 0)?;
    Some((width, height))
}

/// `nearest_stability_aspect_ratio`: the allowed ratio nearest by log ratio, the first on a tie.
fn nearest_aspect_ratio(width: u32, height: u32) -> &'static str {
    let want = (f64::from(width) / f64::from(height)).ln();
    let distance = |ratio: &str| {
        let (w, h) = ratio.split_once(':').unwrap_or(("1", "1"));
        let w: f64 = w.parse().unwrap_or(1.0);
        let h: f64 = h.parse().unwrap_or(1.0);
        ((w / h).ln() - want).abs()
    };
    ALLOWED_ASPECT_RATIOS
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .copied()
        .unwrap_or("1:1")
}

/// `parse_optional_stability_source_image`: at most one of `image_url` and `image`, each a string
/// or `{"url": <string>}`.
fn source_image(body: &Map<String, Value>) -> ComponentResultV1<Option<Source>> {
    let mut found: Option<(&str, &Value)> = None;
    for field in ["image_url", "image"] {
        match body.get(field) {
            None | Some(Value::Null) => {}
            Some(value) => {
                if found.is_some() {
                    return Err(invalid(
                        "Pass only one Stability source image field (`image_url` or `image`), not \
                         both",
                    ));
                }
                found = Some((field, value));
            }
        }
    }
    let Some((field, value)) = found else { return Ok(None) };
    let url = match value {
        Value::String(_) => value,
        _ if is_blob(value) => value,
        Value::Object(object) => match object.get("url") {
            Some(url) if url.is_string() || is_blob(url) => url,
            _ => {
                return Err(invalid(format!(
                    "`{field}` object must contain a string `url` data URL for Stability AI"
                )));
            }
        },
        other => {
            return Err(invalid(format!(
                "`{field}` must be a base64 data URL string, or an object with a string `url` \
                 data URL (got {})",
                shown(other)
            )));
        }
    };
    data_url(url, field).map(Some)
}

/// `parse_stability_data_url`, read from what the component sees: an elided string's 64-byte
/// `head` (its prefix) and `bytes` (its length). Base64 syntax is the host's `from_data_url`
/// check; the size limit is judged from the payload length (decision 2).
fn data_url(value: &Value, field: &str) -> ComponentResultV1<Source> {
    let (text, length, blob) = if let Value::String(url) = value {
        (url.as_str(), u64::try_from(url.len()).unwrap_or(u64::MAX), None)
    } else {
        let placeholder = &value[BLOB_KEY];
        let id = placeholder["id"].as_str().ok_or_else(|| internal("a placeholder has no id"))?;
        (
            placeholder["head"].as_str().unwrap_or_default(),
            placeholder["bytes"].as_u64().ok_or_else(|| internal("a placeholder has no size"))?,
            Some(id),
        )
    };
    let rest = text.strip_prefix(DATA_PREFIX).ok_or_else(|| {
        let preview: String = text.chars().take(80).collect();
        invalid(format!(
            "Stability AI `{field}` only supports base64 data URLs in this release \
             (data:<mime>;base64,<bytes>); HTTP(S) fetching is not enabled. Got '{preview}'"
        ))
    })?;
    let (mime, _) = rest.split_once(BASE64_MARKER).ok_or_else(|| {
        invalid(format!(
            "Stability AI `{field}` data URL must use base64 encoding (data:<mime>;base64,<bytes>)"
        ))
    })?;
    let (media_type, extension) = match mime {
        "image/png" => ("image/png", "png"),
        "image/jpeg" | "image/jpg" => ("image/jpeg", "jpg"),
        "image/webp" => ("image/webp", "webp"),
        "" => {
            return Err(invalid(format!(
                "Stability AI `{field}` data URL is missing the <mime> portion"
            )));
        }
        other => {
            return Err(invalid(format!(
                "Stability AI `{field}` MIME type '{other}' is not supported; use image/png, \
                 image/jpeg, or image/webp"
            )));
        }
    };
    let prefix =
        u64::try_from(DATA_PREFIX.len() + mime.len() + BASE64_MARKER.len()).unwrap_or(u64::MAX);
    let payload = length.saturating_sub(prefix);
    if payload == 0 {
        return Err(invalid(format!("Stability AI `{field}` data URL payload is empty")));
    }
    if payload > MAX_PAYLOAD_CHARS {
        return Err(invalid(format!(
            "Stability AI `{field}` image is more than {MAX_IMAGE_BYTES} bytes after base64 \
             decode; maximum is {MAX_IMAGE_BYTES} bytes"
        )));
    }
    let blob = blob.ok_or_else(|| internal("the host did not elide a declared source image"))?;
    Ok(Source { blob: blob.to_owned(), media_type, extension })
}

/// The native `mode` table (`stability.rs`, `build_stability_generation_request`).
fn check_mode(kind: Kind, mode: Option<&str>, has_source: bool) -> ComponentResultV1<()> {
    match (kind, mode, has_source) {
        (_, None, _)
        | (Kind::Sd3(_) | Kind::Ultra | Kind::Core, Some("text-to-image"), false)
        | (Kind::Sd3(_) | Kind::Ultra, Some("image-to-image"), true) => Ok(()),
        (Kind::Sd3(_), Some("image-to-image"), false) => Err(invalid(
            "Stability SD3 image-to-image mode requires `image_url` or `image` as a base64 data URL",
        )),
        (Kind::Sd3(_), Some(other), _) => Err(invalid(format!(
            "`mode` must be `text-to-image` or `image-to-image` for Stability SD3 models (got \
             '{other}')"
        ))),
        (Kind::Ultra, Some(other), _) => Err(invalid(format!(
            "`mode` must be `text-to-image` or `image-to-image` for Stable Image Ultra (got \
             '{other}')"
        ))),
        (Kind::Core, Some(_), _) => Err(invalid(format!(
            "`mode` is only supported for Stability SD3 models, not {}",
            kind.name()
        ))),
    }
}

fn text(name: &str, value: impl Into<String>) -> MediaPartV1 {
    MediaPartV1::Text { name: name.to_owned(), value: value.into() }
}

/// The ordered text parts and the source image of one generation, or the native refusal.
fn build(
    kind: Kind,
    body: &Map<String, Value>,
) -> ComponentResultV1<(Vec<MediaPartV1>, Option<Source>, &'static str)> {
    check_response_format(body)?;
    let prompt = prompt(body)?;
    let source = source_image(body)?;
    if kind == Kind::Core && source.is_some() {
        return Err(invalid(
            "Stable Image Core does not accept a source image on this endpoint; use Stable Image \
             Ultra or an SD3.5 model for image-to-image",
        ));
    }
    check_mode(kind, optional_string(body, "mode")?, source.is_some())?;

    let format = output_format(body)?;
    let mut parts = vec![text("prompt", prompt), text("output_format", format)];
    if let Some(negative) = optional_string(body, "negative_prompt")? {
        parts.push(text("negative_prompt", negative));
    }
    if let Some(seed) = optional_seed(body)? {
        parts.push(text("seed", seed));
    }
    if let Some(style) = style_preset(body)? {
        parts.push(text("style_preset", style));
    }
    if source.is_some() {
        let strength = optional_number(body, "strength", 0.0, 1.0)?.ok_or_else(|| {
            invalid("Stability image-to-image requests require numeric `strength` in 0..=1")
        })?;
        parts.push(text("strength", strength));
        if let Kind::Sd3(_) = kind {
            if body.contains_key("size") || body.contains_key("aspect_ratio") {
                return Err(invalid(
                    "`size` / `aspect_ratio` are only valid for Stability SD3 text-to-image; \
                     image-to-image uses the source image's aspect ratio",
                ));
            }
            parts.push(text("mode", "image-to-image"));
        }
    } else if let Some(ratio) = aspect_ratio(body)? {
        parts.push(text("aspect_ratio", ratio));
    }
    if let Kind::Sd3(model) = kind {
        parts.push(text("model", model));
        if source.is_none() {
            parts.push(text("mode", "text-to-image"));
        }
        if let Some(scale) = optional_number(body, "cfg_scale", 1.0, 10.0)? {
            parts.push(text("cfg_scale", scale));
        }
    } else if body.contains_key("cfg_scale") {
        return Err(invalid("`cfg_scale` is only supported for Stability SD3 models"));
    }
    let media_type = match format {
        "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    };
    Ok((parts, source, media_type))
}

/// Stability's error body `{"id", "name", "errors": [...]}` as an envelope.
fn error_envelope(status: u16, body: &Value) -> ErrorEnvelope {
    let name = body["name"].as_str().map(str::trim).filter(|name| !name.is_empty());
    let code = if name == Some("content_moderation") {
        ErrorCode::ContentPolicy
    } else {
        status_code(status)
    };
    let errors: Vec<&str> = body["errors"]
        .as_array()
        .map(|errors| errors.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let joined = errors.join("; ");
    let message = match (name, joined.trim().is_empty()) {
        (Some(name), false) => format!("{name}: {joined}"),
        (Some(name), true) => name.to_owned(),
        (None, _) => joined,
    };
    let mut envelope = ErrorEnvelope::new(code, status, code_message(code));
    envelope.provider_message = capped(&message);
    envelope
}

fn unknown(reason: impl Into<String>) -> ImageOutcomeV1 {
    ImageOutcomeV1::Unknown { reason: reason.into(), error: None }
}

impl ImageComponentV1 for StabilityImageReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: NAME.to_owned(),
            version: VERSION.to_owned(),
            api_version: WORLD.to_owned(),
        }
    }

    /// Decision 1: each configured model among the five ids; `generate` only, the source image
    /// under `image_url` or `image` as `input_image`, `b64_json` only, `images` only.
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
        serves(config, STABILITY_IMAGE)?;
        let request_elision_paths = paths(&REQUEST_ELISION_PATHS)?;
        Ok(config
            .models
            .iter()
            .filter(|model| MODELS.contains(&model.model.as_str()))
            .map(|model| ImageModelCapabilitiesV1 {
                model: model.model.clone(),
                operations: vec![ImageOperationV1::Generate],
                input_roles: vec![
                    InputRoleKeyV1 {
                        key: "image_url".to_owned(),
                        role: ImageInputRoleV1::InputImage,
                    },
                    InputRoleKeyV1 { key: "image".to_owned(), role: ImageInputRoleV1::InputImage },
                ],
                response_formats: vec![ImageResponseFormatV1::B64Json],
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
        serves(config, STABILITY_IMAGE)?;
        let forms = images_only(context, UPSTREAM)?;
        let body = request.get("json").and_then(Value::as_object).ok_or_else(|| {
            invalid("Stability AI image models only generate from a JSON request")
        })?;
        // The native order: the count (`stability_prepare`), the model, then the body.
        check_n(body)?;
        let kind = Kind::of(&context.upstream_model)?;
        let (mut parts, source, media_type) = build(kind, body)?;
        let has_source = source.is_some();
        if let Some(source) = source {
            parts.push(MediaPartV1::File {
                name: "image".to_owned(),
                blob: serde_json::from_value(json!(source.blob))
                    .map_err(|_| internal("a placeholder id is not a blob id"))?,
                transform: MediaTransformV1::FromDataUrl,
                filename: Some(format!("source.{}", source.extension)),
                media_type: Some(source.media_type.to_owned()),
            });
        }

        let mut descriptor = Map::new();
        descriptor.insert("method".to_owned(), json!("POST"));
        descriptor.insert("path".to_owned(), json!(kind.path()));
        descriptor.insert(
            "headers".to_owned(),
            json!([{ "name": "accept", "value": "application/json" }]),
        );
        if let Some(auth) = bearer(config) {
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
                inputs: ImageInputsV1 {
                    input_image: u32::from(has_source),
                    ..ImageInputsV1::default()
                },
                requested_outputs: 1,
                size: None,
                tier: ImageTierV1::default(),
                tier_candidates: None,
                metering_forms: forms,
                bounds: None,
            },
            descriptor,
            repeat: 1,
            response_body_form: ResponseBodyFormV1::Json,
            response_elision_paths: paths(&[RESPONSE_ELISION_PATH])?,
            immutable_body_paths: IMMUTABLE_PART_NAMES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            state: json!({ "media_type": media_type }).to_string(),
        })
    }

    fn parse_response(&self, state: &Value, response: &Value) -> ComponentResultV1<ImageOutcomeV1> {
        let status = view_status(response);
        let body = response.pointer("/body/json").cloned().unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            return Ok(non_2xx(status, error_envelope(status, &body)));
        }
        if !body.is_object() {
            return Ok(unknown("a 2xx without a JSON object body"));
        }
        if !non_empty_inline(&body["image"]) {
            return Ok(unknown("a 2xx without an image"));
        }
        let Some(finish_reason) = body["finish_reason"].as_str() else {
            return Ok(unknown("a 2xx without a finish_reason"));
        };
        if !matches!(finish_reason, "SUCCESS" | "CONTENT_FILTERED") {
            let word: String = finish_reason.chars().take(64).collect();
            return Ok(unknown(format!("an unknown finish_reason '{word}'")));
        }
        let media_type = state["media_type"].as_str().unwrap_or("image/png").to_owned();
        Ok(ImageOutcomeV1::Succeeded {
            artifacts: vec![ImageArtifactV1::Inline {
                pointer: JsonPointerV1::parse(RESPONSE_ELISION_PATH)
                    .map_err(|_| internal("invalid artifact pointer"))?,
                encoding: ArtifactEncodingV1::Base64,
                media_type,
            }],
            metering: ImageMeteringV1 {
                tokens: None,
                images_reported: None,
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
            return Err(internal("Stability AI renders exactly one succeeded round"));
        };
        if artifacts.len() != 1 {
            return Err(internal("Stability AI renders exactly one image"));
        }
        Ok(ImageRenderedV1 {
            template: json!({
                "created": context.created,
                "data": [{
                    "b64_json": { "$south.artifact": { "index": 0, "as": "b64_json" } },
                    "revised_prompt": null,
                }],
            })
            .to_string(),
        })
    }
}
