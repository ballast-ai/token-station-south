//! The native reference implementation of the `image-reve` component.
//!
//! The Reve bridge (image world step I3-6, design record `docs/design/2026-10-13-image-i3-6-reve.md`;
//! image record §13, Reve row). Transcribed from token-station-server's native Reve arm
//! (`handler/reve.rs`: `size_to_reve_aspect_ratio`, `validate_reve_generation_request`,
//! `validate_reve_output_format`, `build_reve_create_body`, `build_reve_edit_body`,
//! `build_reve_remix_body`, `reve_extras_from_multipart`, `generations`, `edits`,
//! `finish_reve_openai_surface`, `reve_json_to_openai_envelope`, `reve_content_violation`,
//! `reve_credits_from_json`; `REVE_IMAGE_OWNED` in `images/execute.rs`). Statements about Reve's
//! own API are the native arm's, unverified against a capture since Reve's 2026-08-14 API change
//! (record §0.2).
//!
//! - **Scope** (RV-Q1): both OpenAI-surface bridges. A JSON request is a generation, sent to
//!   `v1/image/create`; a multipart request is an edit, sent to `v1/image/edit` (exactly one image)
//!   or `v1/image/remix` (one to six). The native routes `/v1/image/*` stay native.
//! - **Endpoint** (RV-Q2 (c)): the routed row pins `endpoint` (`create`, `edit` or `remix`) through
//!   the image route's `request.set`, which the host writes into the JSON view as a top-level member
//!   and into the multipart view as a text part. A generation without a pin is a create; an edit
//!   without a pin is refused, since the package cannot tell edit from remix (native reads the
//!   gateway model name, which a package never sees).
//! - **Request** (§3, §4): JSON with `accept: application/json` and `bearer` on the configured
//!   slot. The body keys and every refusal follow the native order and texts; images are inlined
//!   as base64 through `$south.ref` references. An elided field the package reads is refused as
//!   longer than the gateway accepts (P7).
//! - **Facts**: the operation, the input count by role (edit `input_image`, remix
//!   `reference_image`), one output, no size (Reve receives a ratio), no tier, `credits` only, no
//!   bounds (RV-Q3), `repeat` 1, `json`, the response elided at `/image`.
//! - **Response** (§5): the meter `credits_used` (a JSON number, at most six decimal places, not
//!   negative) first; without it the round is `unknown`. `content_violation: true` is
//!   `charged_failure` with a content-policy 400 and the credits; otherwise a non-empty `image` is one
//!   inline PNG artifact, and anything else is `unknown`.
//! - **Render**: the native envelope with its flat metadata, without `credits_remaining` (RV-Q5 (b)).

use serde_json::{Map, Value, json};
use south_contracts::JsonPointerV1;
use south_contracts::image::{
    ArtifactEncodingV1, CreditsV1, ImageArtifactV1, ImageCallContextV1, ImageFactsV1,
    ImageInputsV1, ImageMeteringV1, ImageModelCapabilitiesV1, ImageOperationV1,
    ImageRenderContextV1, ImageResponseFormatV1, ImageTierV1, MeteringFormV1, PreparedImageCallV1,
};
use south_contracts::media::{
    MediaLimitsV1, MediaRequestDescriptorV1, PathPatternV1, ResponseBodyFormV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

use crate::base64::{Alphabet, encode};
use crate::image_shared::{
    bearer, capability, capped, code_message, internal, invalid, is_blob, non_2xx,
    non_empty_inline, serves, status_code, view_status,
};
use crate::{ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1};

/// The package name this reference is published as.
pub const NAME: &str = "image-reve";
/// The package version.
pub const VERSION: &str = "1.0.10";
/// The world this reference exports.
pub const WORLD: &str = "image-adapter-v1";
/// The family, as a host's image rows name it (RV-Q6).
pub const REVE_IMAGE: &str = "reve-image";
/// The request key the image route pins (RV-Q2 (c)).
pub const ENDPOINT_PIN: &str = "endpoint";

const REF_KEY: &str = "$south.ref";
/// Where the image sits in a JSON-mode answer; elided, so its bytes never enter the sandbox.
const RESPONSE_ELISION_PATH: &str = "/image";
/// The native `REVE_IMAGE_OWNED`: operator request extras may not set these, since each can raise
/// the credit meter above the reservation.
const IMMUTABLE_BODY_PATHS: [&str; 3] = ["version", "test_time_scaling", "postprocessing"];
/// The optional Reve fields the generation bridge copies as sent (`REVE_PASSTHROUGH_FIELDS`).
const PASSTHROUGH_FIELDS: [&str; 3] = ["test_time_scaling", "postprocessing", "breadcrumb"];
/// Reve's seven aspect ratios in the native tie-break order (the first wins on equal distance).
const ASPECT_RATIOS: [(&str, f64); 7] = [
    ("16:9", 16.0 / 9.0),
    ("9:16", 9.0 / 16.0),
    ("3:2", 3.0 / 2.0),
    ("2:3", 2.0 / 3.0),
    ("4:3", 4.0 / 3.0),
    ("3:4", 3.0 / 4.0),
    ("1:1", 1.0),
];
/// The most images a remix takes.
const MAX_REMIX_IMAGES: usize = 6;
/// The native answer to a content violation, which Reve still metered (`reve_violation_error_body`).
const VIOLATION: &str = "Reve flagged this request as a content policy violation; the upstream \
                         still metered credits for the attempt and the gateway billed them.";

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReveImageReferenceV1;

/// The upstream endpoint a call goes to (native `ReveFamily`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endpoint {
    Create,
    Edit,
    Remix,
}

impl Endpoint {
    const fn path(self) -> &'static str {
        match self {
            Self::Create => "v1/image/create",
            Self::Edit => "v1/image/edit",
            Self::Remix => "v1/image/remix",
        }
    }

    fn of_word(word: &str) -> Option<Self> {
        match word {
            "create" => Some(Self::Create),
            "edit" => Some(Self::Edit),
            "remix" => Some(Self::Remix),
            _ => None,
        }
    }
}

/// The host's required metering forms, when this package can report them all: `credits` only.
fn credits_only(context: &ImageCallContextV1) -> ComponentResultV1<Vec<MeteringFormV1>> {
    if context.metering_required.is_empty() {
        return Err(capability("the host requires no metering form"));
    }
    if let Some(form) =
        context.metering_required.iter().find(|form| **form != MeteringFormV1::Credits)
    {
        return Err(capability(format!("Reve image models cannot report {form:?} metering")));
    }
    Ok(context.metering_required.clone())
}

fn elided(field: &str) -> ErrorEnvelope {
    invalid(format!("'{field}' is longer than the gateway accepts"))
}

/// Whether `value` holds a host placeholder anywhere.
fn holds_blob(value: &Value) -> bool {
    match value {
        Value::Object(map) => is_blob(value) || map.values().any(holds_blob),
        Value::Array(items) => items.iter().any(holds_blob),
        _ => false,
    }
}

/// The pinned endpoint's word, judged; `None` when the request carries no pin (absent or `null`).
fn pinned(word: Option<&Value>) -> ComponentResultV1<Option<Endpoint>> {
    let Some(value) = word.filter(|value| !value.is_null()) else { return Ok(None) };
    value.as_str().and_then(Endpoint::of_word).map(Some).ok_or_else(|| {
        capability(format!(
            "the route pins an unknown Reve endpoint; '{ENDPOINT_PIN}' must be 'create', 'edit' or \
             'remix'"
        ))
    })
}

/// `size_to_reve_aspect_ratio`: `auto` or empty is none; else `WxH` split at the first `x` or `X`,
/// each side a `u32` (a leading `+` parses), both positive, snapped to the nearest ratio.
fn aspect_ratio_of_size(size: &str) -> ComponentResultV1<Option<&'static str>> {
    let size = size.trim();
    if size.is_empty() || size.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }
    let (width, height) = size.split_once(['x', 'X']).ok_or_else(|| {
        invalid(format!("invalid size '{size}', expected WxH (e.g. 1024x1024) or 'auto'"))
    })?;
    let axis = |part: &str, name: &str| {
        part.trim().parse::<u32>().map_err(|_| invalid(format!("invalid {name} in size '{size}'")))
    };
    let (width, height) = (axis(width, "width")?, axis(height, "height")?);
    if width == 0 || height == 0 {
        return Err(invalid(format!("size '{size}' must have positive width and height")));
    }
    let ratio = f64::from(width) / f64::from(height);
    let mut best = ASPECT_RATIOS[0];
    for candidate in ASPECT_RATIOS {
        if (candidate.1 - ratio).abs() < (best.1 - ratio).abs() {
            best = candidate;
        }
    }
    Ok(Some(best.0))
}

/// `validate_reve_output_format`: the client surface answers PNG only.
fn check_output_format(output_format: Option<&str>) -> ComponentResultV1<()> {
    match output_format {
        None => Ok(()),
        Some(format) if format.eq_ignore_ascii_case("png") => Ok(()),
        Some(format) => Err(invalid(format!(
            "output_format '{format}' is not supported on the OpenAI surface for Reve (JSON mode \
             always returns PNG b64_json); use the native /v1/image/* endpoints with an `Accept: \
             image/{format}` header for binary formats"
        ))),
    }
}

/// A request value as a refusal echoes it, never printing a placeholder.
fn shown(value: &Value) -> String {
    if holds_blob(value) {
        "a value longer than the gateway accepts".to_owned()
    } else {
        value.to_string()
    }
}

/// The host handler's coercion of `n`: an integer or a whole non-negative float.
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

/// The descriptor of a JSON POST with `accept: application/json`.
fn descriptor(
    config: &ProviderConfig,
    endpoint: Endpoint,
    template: &Value,
) -> ComponentResultV1<MediaRequestDescriptorV1> {
    let mut descriptor = Map::new();
    descriptor.insert("method".to_owned(), json!("POST"));
    descriptor.insert("path".to_owned(), json!(endpoint.path()));
    descriptor
        .insert("headers".to_owned(), json!([{ "name": "accept", "value": "application/json" }]));
    if let Some(auth) = bearer(config) {
        descriptor.insert("auth".to_owned(), auth);
    }
    descriptor.insert("body".to_owned(), json!({ "json": { "template": template } }));
    MediaRequestDescriptorV1::parse(&Value::Object(descriptor).to_string(), &MediaLimitsV1::V1)
        .map_err(|error| internal(format!("built an invalid descriptor: {error}")))
}

fn prepared(
    operation: ImageOperationV1,
    inputs: ImageInputsV1,
    forms: Vec<MeteringFormV1>,
    descriptor: MediaRequestDescriptorV1,
) -> ComponentResultV1<PreparedImageCallV1> {
    Ok(PreparedImageCallV1 {
        facts: ImageFactsV1 {
            operation,
            inputs,
            requested_outputs: 1,
            // Reve receives an aspect ratio, never a size (P5).
            size: None,
            tier: ImageTierV1::default(),
            tier_candidates: None,
            metering_forms: forms,
            // RV-Q3: Reve's per-call credit table is itself unverified; the host bounds credits.
            bounds: None,
        },
        descriptor,
        repeat: 1,
        response_body_form: ResponseBodyFormV1::Json,
        response_elision_paths: vec![
            PathPatternV1::parse(RESPONSE_ELISION_PATH)
                .map_err(|_| internal("invalid elision path"))?,
        ],
        immutable_body_paths: IMMUTABLE_BODY_PATHS.iter().map(|path| (*path).to_owned()).collect(),
        state: "{}".to_owned(),
    })
}

/// `generations`: the pin, then `validate_reve_generation_request` (`n`, `stream`,
/// `output_format`), then `build_reve_create_body` (`prompt`, `aspect_ratio` or `size`, `version`,
/// the pass-through fields).
fn prepare_generation(
    config: &ProviderConfig,
    body: &Map<String, Value>,
    context: &ImageCallContextV1,
    forms: Vec<MeteringFormV1>,
) -> ComponentResultV1<PreparedImageCallV1> {
    match pinned(body.get(ENDPOINT_PIN))? {
        None | Some(Endpoint::Create) => {}
        Some(Endpoint::Edit | Endpoint::Remix) => {
            return Err(invalid(
                "this model is a Reve edit/remix model; /v1/images/generations only accepts Reve \
                 create models. Use /v1/images/edits or the native /v1/image/edit | \
                 /v1/image/remix endpoints.",
            ));
        }
    }
    let n = match body.get("n") {
        None | Some(Value::Null) => 1,
        Some(value) => json_count(value).ok_or_else(|| {
            invalid(format!("'n' must be a non-negative integer (got {})", shown(value)))
        })?,
    };
    if n != 1 {
        return Err(invalid(format!(
            "Reve generates exactly one image per request; 'n' must be 1 or absent (got {n})"
        )));
    }
    if body.get("stream").and_then(Value::as_bool) == Some(true) {
        return Err(invalid(
            "streaming is not supported for Reve image generation; re-submit without `stream`",
        ));
    }
    match body.get("output_format") {
        Some(value) if is_blob(value) => return Err(elided("output_format")),
        value => check_output_format(value.and_then(Value::as_str))?,
    }

    let prompt = match body.get("prompt") {
        Some(value) if is_blob(value) => return Err(elided("prompt")),
        Some(Value::String(prompt)) if !prompt.trim().is_empty() => prompt,
        _ => return Err(invalid("Missing 'prompt' field")),
    };
    let mut template = Map::new();
    template.insert("prompt".to_owned(), json!(prompt));
    // An explicit `aspect_ratio` string wins and is forwarded unvalidated; `size` is then not read.
    match body.get("aspect_ratio") {
        Some(value) if is_blob(value) => return Err(elided("aspect_ratio")),
        Some(Value::String(ratio)) => {
            template.insert("aspect_ratio".to_owned(), json!(ratio));
        }
        _ => match body.get("size") {
            Some(value) if is_blob(value) => return Err(elided("size")),
            Some(Value::String(size)) => {
                if let Some(ratio) = aspect_ratio_of_size(size)? {
                    template.insert("aspect_ratio".to_owned(), json!(ratio));
                }
            }
            // A non-string `size` is ignored, as native `as_str`.
            _ => {}
        },
    }
    let version = match body.get("version") {
        Some(value) if is_blob(value) => return Err(elided("version")),
        Some(Value::String(version)) => version.as_str(),
        _ => context.upstream_model.as_str(),
    };
    template.insert("version".to_owned(), json!(version));
    for field in PASSTHROUGH_FIELDS {
        match body.get(field) {
            None | Some(Value::Null) => {}
            Some(value) if holds_blob(value) => return Err(elided(field)),
            Some(value) => {
                template.insert(field.to_owned(), value.clone());
            }
        }
    }
    let descriptor = descriptor(config, Endpoint::Create, &Value::Object(template))?;
    prepared(ImageOperationV1::Generate, ImageInputsV1::default(), forms, descriptor)
}

/// The multipart view's parts, read the way the native field finder reads them: the first part of
/// a name, file parts having no text the package can see.
struct Parts<'a>(&'a [Value]);

impl<'a> Parts<'a> {
    fn named<'n>(&self, name: &'n str) -> impl Iterator<Item = &'a Value> + use<'a, 'n> {
        let parts = self.0;
        parts.iter().filter(move |part| part["name"] == name)
    }

    /// The first part named `name`, as trimmed text when it is a text part; an elided value is
    /// refused (P7), a file part reads as absent.
    fn text(&self, name: &str) -> ComponentResultV1<Option<&'a str>> {
        let Some(part) = self.named(name).next() else { return Ok(None) };
        if part["kind"] != "text" {
            return Ok(None);
        }
        if part.get("blob").is_some() {
            return Err(elided(name));
        }
        Ok(part["value"].as_str().map(str::trim))
    }
}

/// One image part as a template value: a file part through a `base64` reference, a text part's
/// value encoded here (native encodes whatever bytes the part holds).
fn image_value(part: &Value) -> ComponentResultV1<Value> {
    if part["kind"] == "file" {
        let blob = part["blob"].as_str().ok_or_else(|| internal("a file part has no blob"))?;
        return Ok(json!({ REF_KEY: { "blob": blob, "transform": "base64" } }));
    }
    let Some(value) = part["value"].as_str() else { return Err(elided("image")) };
    let encoded = encode(value.as_bytes(), Alphabet::Standard, true);
    if encoded.len() > MediaLimitsV1::V1.inline_string_bytes {
        return Err(elided("image"));
    }
    Ok(json!(encoded))
}

/// `reve_extras_from_multipart`: `test_time_scaling` (a number, sent as a JSON float),
/// `postprocessing` (a JSON array), `breadcrumb` (free text); each empty field is absent.
fn multipart_extras(parts: &Parts<'_>) -> ComponentResultV1<Vec<(&'static str, Value)>> {
    let mut extras = Vec::new();
    if let Some(raw) = parts.text("test_time_scaling")?.filter(|raw| !raw.is_empty()) {
        let number: f64 = raw.parse().map_err(|_| {
            invalid(format!("multipart `test_time_scaling` field value '{raw}' must be a number"))
        })?;
        // `json!` of a non-finite float is `null`, as native.
        extras.push(("test_time_scaling", json!(number)));
    }
    if let Some(raw) = parts.text("postprocessing")?.filter(|raw| !raw.is_empty()) {
        let value: Value = serde_json::from_str(raw).map_err(|_| {
            invalid(
                "multipart `postprocessing` field must be a JSON array (e.g. [{\"process\": \
                 \"upscale\", \"upscale_factor\": 2}])",
            )
        })?;
        if !value.is_array() {
            return Err(invalid("multipart `postprocessing` field must be a JSON array"));
        }
        extras.push(("postprocessing", value));
    }
    if let Some(raw) = parts.text("breadcrumb")?.filter(|raw| !raw.is_empty()) {
        extras.push(("breadcrumb", json!(raw)));
    }
    Ok(extras)
}

/// The endpoint an edit's pin names: `edit` or `remix` (RV-Q2 (c)); fail closed otherwise.
fn edit_endpoint(parts: &Parts<'_>) -> ComponentResultV1<Endpoint> {
    let pins: Vec<&Value> = parts.named(ENDPOINT_PIN).collect();
    let endpoint = match pins.as_slice() {
        [] => None,
        [pin] if pin["kind"] == "text" => pinned(pin.get("value"))?,
        _ => {
            return Err(capability(format!(
                "the route pins the Reve endpoint as one text part '{ENDPOINT_PIN}'"
            )));
        }
    };
    match endpoint {
        Some(endpoint @ (Endpoint::Edit | Endpoint::Remix)) => Ok(endpoint),
        Some(Endpoint::Create) => Err(invalid(
            "this model is a Reve create model; /v1/images/edits only accepts Reve edit or \
                 remix models.",
        )),
        None => Err(capability(format!(
            "the route pins no Reve endpoint; a Reve edit route must pin '{ENDPOINT_PIN}' to \
                 'edit' or 'remix'"
        ))),
    }
}

/// `edits`: the pin, then `mask`, `n`, `stream`, `output_format`, `prompt`, `size`, `version`, the
/// extras and the image count, in the native order.
fn prepare_edit(
    config: &ProviderConfig,
    parts: &[Value],
    context: &ImageCallContextV1,
    forms: Vec<MeteringFormV1>,
) -> ComponentResultV1<PreparedImageCallV1> {
    let parts = Parts(parts);
    let endpoint = edit_endpoint(&parts)?;
    if parts.named("mask").next().is_some() {
        return Err(invalid(
            "`mask` is not supported for Reve edits — Reve conditions on the edit instruction \
             rather than a separate mask",
        ));
    }
    if let Some(raw) = parts.text("n")?.filter(|raw| !raw.is_empty() && *raw != "1") {
        return Err(invalid(format!(
            "Reve generates exactly one image per request; multipart `n` must be 1 or absent (got \
             '{raw}')"
        )));
    }
    if parts.text("stream")?.is_some_and(|value| value.eq_ignore_ascii_case("true") || value == "1")
    {
        return Err(invalid("streaming /v1/images/edits (stream=true) is not supported for Reve"));
    }
    // An empty `output_format` part is refused, as native (`Some("")` is not `png`).
    check_output_format(parts.text("output_format")?)?;
    let prompt = parts
        .text("prompt")?
        .filter(|prompt| !prompt.is_empty())
        .ok_or_else(|| invalid("Missing `prompt` field in multipart body"))?;
    let aspect_ratio = match parts.text("size")? {
        Some(size) if !size.is_empty() => aspect_ratio_of_size(size)?,
        _ => None,
    };
    let version = parts
        .text("version")?
        .filter(|version| !version.is_empty())
        .unwrap_or(context.upstream_model.as_str());
    let extras = multipart_extras(&parts)?;

    let images: Vec<&Value> = parts
        .0
        .iter()
        .filter(|part| part["name"] == "image" || part["name"] == "image[]")
        .collect();
    let mut template = Map::new();
    let inputs = match endpoint {
        Endpoint::Edit => {
            let [image] = images.as_slice() else {
                return Err(invalid(format!(
                    "Reve edit takes exactly one `image` part (got {}); for multi-image \
                     composition use a remix SKU (e.g. reve/reve-remix)",
                    images.len()
                )));
            };
            template.insert("edit_instruction".to_owned(), json!(prompt));
            template.insert("reference_image".to_owned(), image_value(image)?);
            ImageInputsV1 { input_image: 1, ..ImageInputsV1::default() }
        }
        Endpoint::Remix | Endpoint::Create => {
            if images.is_empty() || images.len() > MAX_REMIX_IMAGES {
                return Err(invalid(format!(
                    "Reve remix takes between 1 and 6 `image` parts (got {})",
                    images.len()
                )));
            }
            let references =
                images.iter().map(|image| image_value(image)).collect::<Result<Vec<_>, _>>()?;
            template.insert("prompt".to_owned(), json!(prompt));
            template.insert("reference_images".to_owned(), Value::Array(references));
            ImageInputsV1 {
                reference_image: u32::try_from(images.len()).unwrap_or(u32::MAX),
                ..ImageInputsV1::default()
            }
        }
    };
    if let Some(ratio) = aspect_ratio {
        template.insert("aspect_ratio".to_owned(), json!(ratio));
    }
    template.insert("version".to_owned(), json!(version));
    for (field, value) in extras {
        template.insert(field.to_owned(), value);
    }
    let descriptor = descriptor(config, endpoint, &Value::Object(template))?;
    prepared(ImageOperationV1::Edit, inputs, forms, descriptor)
}

/// Reve's error body as an envelope: the code follows the status; the message is the body's
/// `error`, `message` or `detail` string, or `error.message` (Reve's error shape is unverified).
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

/// The digits of a JSON number's text without an exponent; `None` for a negative number.
fn plain_decimal(text: &str) -> Option<String> {
    if text.starts_with('-') {
        return None;
    }
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
        None => (text, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let point = i64::try_from(whole.len()).ok()? + i64::from(exponent);
    let length = i64::try_from(digits.len()).ok()?;
    // Far outside what a credit count can be: not a decimal the contract can carry.
    if !(-64..=64).contains(&point) {
        return None;
    }
    let (whole, fraction) = if point <= 0 {
        let zeros = usize::try_from(-point).ok()?;
        ("0".to_owned(), format!("{}{digits}", "0".repeat(zeros)))
    } else if point >= length {
        let zeros = usize::try_from(point - length).ok()?;
        (format!("{digits}{}", "0".repeat(zeros)), String::new())
    } else {
        let split = usize::try_from(point).ok()?;
        (digits[..split].to_owned(), digits[split..].to_owned())
    };
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let fraction = fraction.trim_end_matches('0');
    Some(if fraction.is_empty() { whole.to_owned() } else { format!("{whole}.{fraction}") })
}

/// `reve_credits_from_json`, narrowed to what the `credits` fact carries: a JSON number (a string
/// such as `"18"` is not, as native `as_f64`), not negative, with at most six decimal places.
fn credits(value: &Value) -> Option<CreditsV1> {
    let Value::Number(number) = value else { return None };
    CreditsV1::parse(&plain_decimal(&number.to_string())?).ok()
}

/// A response value copied into `extras`, or `null` when it holds a placeholder.
fn copied(value: Option<&Value>) -> Value {
    value.filter(|value| !holds_blob(value)).cloned().unwrap_or(Value::Null)
}

fn unknown(reason: &str) -> ImageOutcomeV1 {
    ImageOutcomeV1::Unknown { reason: reason.to_owned(), error: None }
}

impl ImageComponentV1 for ReveImageReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: NAME.to_owned(),
            version: VERSION.to_owned(),
            api_version: WORLD.to_owned(),
        }
    }

    /// Decision 1: every configured model (Reve versions are an open set), `generate` and `edit`,
    /// no input role (the role follows the pinned endpoint, reported per call in `inputs`),
    /// `b64_json` only, `credits` only, nothing elided on the request side.
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
        serves(config, REVE_IMAGE)?;
        Ok(config
            .models
            .iter()
            .map(|model| ImageModelCapabilitiesV1 {
                model: model.model.clone(),
                operations: vec![ImageOperationV1::Generate, ImageOperationV1::Edit],
                input_roles: Vec::new(),
                response_formats: vec![ImageResponseFormatV1::B64Json],
                metering_forms: vec![MeteringFormV1::Credits],
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
        serves(config, REVE_IMAGE)?;
        let forms = credits_only(context)?;
        if let Some(body) = request.get("json").and_then(Value::as_object) {
            return prepare_generation(config, body, context, forms);
        }
        if let Some(parts) = request.pointer("/multipart/parts").and_then(Value::as_array) {
            return prepare_edit(config, parts, context, forms);
        }
        Err(invalid("the request is neither JSON nor multipart"))
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
        // The meter first, whatever else the answer says: native fails without it.
        let Some(credits) = credits(&body["credits_used"]) else {
            return Ok(unknown("a 2xx without a usable credit meter"));
        };
        let metering = ImageMeteringV1 {
            tokens: None,
            images_reported: None,
            credits: Some(credits),
            upstream_cost: None,
        };
        // A violation wins over an image (decision 4).
        if body["content_violation"] == Value::Bool(true) {
            return Ok(ImageOutcomeV1::ChargedFailure {
                error: ErrorEnvelope::new(ErrorCode::ContentPolicy, 400, VIOLATION),
                metering,
            });
        }
        if !non_empty_inline(&body["image"]) {
            return Ok(unknown("a 2xx without an image"));
        }
        Ok(ImageOutcomeV1::Succeeded {
            artifacts: vec![ImageArtifactV1::Inline {
                pointer: JsonPointerV1::parse(RESPONSE_ELISION_PATH)
                    .map_err(|_| internal("invalid artifact pointer"))?,
                encoding: ArtifactEncodingV1::Base64,
                media_type: "image/png".to_owned(),
            }],
            metering,
            // RV-Q5 (b): `credits_remaining` (the operator's upstream balance) is not carried.
            extras: json!({
                "request_id": copied(body.get("request_id")),
                "version": copied(body.get("version")),
                "content_violation": body
                    .get("content_violation")
                    .map_or(Value::Bool(false), |value| copied(Some(value))),
                "credits_used": copied(body.get("credits_used")),
            }),
        })
    }

    fn render(
        &self,
        _state: &Value,
        outcomes: &[ImageOutcomeV1],
        context: &ImageRenderContextV1,
    ) -> ComponentResultV1<ImageRenderedV1> {
        let [ImageOutcomeV1::Succeeded { artifacts, extras, .. }] = outcomes else {
            return Err(internal("Reve renders exactly one succeeded round"));
        };
        if artifacts.len() != 1 {
            return Err(internal("Reve renders exactly one image"));
        }
        let field = |key: &str| extras.get(key).cloned().unwrap_or(Value::Null);
        Ok(ImageRenderedV1 {
            template: json!({
                "created": context.created,
                "data": [{ "b64_json": { "$south.artifact": { "index": 0, "as": "b64_json" } } }],
                "request_id": field("request_id"),
                "version": field("version"),
                "content_violation": field("content_violation"),
                "credits_used": field("credits_used"),
            })
            .to_string(),
        })
    }
}
