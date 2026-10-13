//! The `:generateContent` image request and response pieces Google's two image families share:
//! the Gemini API (`image-gemini`, image world step I3-2, design record
//! `docs/design/2026-10-13-image-i3-2-gemini.md`) and, in step I3-3, Vertex AI.
//!
//! The host's native arm already shares everything but the URL and the auth between the two
//! families (record §7), so the body builders, the response reader (artifacts, finish reasons,
//! usage), the render and the refusal texts live here, and a family's reference passes in only
//! its path, its auth arm and whether it edits. The texts are the native arm's ("Gemini Nano
//! Banana …"), which names both families the same way.

use serde_json::{Map, Value, json};
use south_contracts::JsonPointerV1;
use south_contracts::image::{
    ArtifactEncodingV1, ImageArtifactV1, ImageCallContextV1, ImageFactsV1, ImageInputRoleV1,
    ImageInputsV1, ImageMeteringV1, ImageModelCapabilitiesV1, ImageOperationV1,
    ImageRenderContextV1, ImageResponseFormatV1, ImageTierV1, ImageTokenUsageV1, InputRoleKeyV1,
    MeteringFormV1, PreparedImageCallV1, TierDimensionV1, TierWordV1, TokenBucketV1,
};
use south_contracts::media::{
    MediaLimitsV1, MediaRequestDescriptorV1, PathPatternV1, ResponseBodyFormV1, is_media_type,
};
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

use crate::image_shared::{
    capability, capped, code_message, internal, invalid, is_blob, non_2xx, non_empty_inline,
    status_code, view_status,
};
use crate::{ComponentResultV1, ImageOutcomeV1, ImageRenderedV1};

const BLOB_KEY: &str = "$south.blob";
const REF_KEY: &str = "$south.ref";
/// The most images one call may ask for (`n`), one upstream call each (`repeat`).
pub const MAX_IMAGES: u64 = 10;
/// The host's `GEMINI_IMAGE_OWNED`: request extras may not set these.
const IMMUTABLE_BODY_PATHS: [&str; 4] = [
    "contents",
    "generationConfig.imageConfig",
    "generationConfig.candidateCount",
    "generationConfig.responseModalities",
];
/// Request-side elision: the reference image, a data URL whose bytes never enter the sandbox.
const REQUEST_ELISION_PATHS: [&str; 1] = ["/image_url"];
/// Response-side elision: every image the upstream returns.
const RESPONSE_ELISION_PATH: &str = "/candidates/*/content/parts/*/inlineData/data";
/// The finish reasons that mean a policy or safety stop (GI-Q5). Unverified against a capture;
/// a candidate ending for any of them without an image is `unknown` with a `content_policy`
/// envelope, any other image-less 2xx is `unknown` without one.
const POLICY_FINISH_REASONS: [&str; 9] = [
    "SAFETY",
    "RECITATION",
    "BLOCKLIST",
    "PROHIBITED_CONTENT",
    "SPII",
    "IMAGE_SAFETY",
    "IMAGE_PROHIBITED_CONTENT",
    "IMAGE_RECITATION",
    "NO_IMAGE",
];
/// The default media type of an image whose type the upstream or the client leaves unsaid.
const DEFAULT_IMAGE_TYPE: &str = "image/png";

/// Where and how one family's request goes; everything else is shared.
pub struct GeminiRouteV1 {
    /// The path relative to the family's base URL.
    pub path: String,
    /// The descriptor's `auth` member, when the family has a credential slot.
    pub auth: Option<Value>,
    /// Whether the family serves `/v1/images/edits`.
    pub edit: bool,
}

/// The family check every function starts with.
pub fn serves(config: &ProviderConfig, family: &str) -> ComponentResultV1<()> {
    crate::image_shared::serves(config, family)
}

/// `model-capabilities` (record §9 decision 1): every configured model, `generate` (and `edit`
/// when the family edits), one reference image under `image_url` (generation) or `image` (edit),
/// `b64_json` only, `tokens` and `images` with the two declared token buckets, the `resolution`
/// tier dimension, `repeat`, and `/image_url` elided.
pub fn model_capabilities(
    config: &ProviderConfig,
    edit: bool,
) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
    let request_elision_paths = paths(&REQUEST_ELISION_PATHS)?;
    let mut operations = vec![ImageOperationV1::Generate];
    let mut input_roles = vec![InputRoleKeyV1 {
        key: "image_url".to_owned(),
        role: ImageInputRoleV1::ReferenceImage,
    }];
    if edit {
        operations.push(ImageOperationV1::Edit);
        input_roles.push(InputRoleKeyV1 {
            key: "image".to_owned(),
            role: ImageInputRoleV1::ReferenceImage,
        });
    }
    Ok(config
        .models
        .iter()
        .map(|model| ImageModelCapabilitiesV1 {
            model: model.model.clone(),
            operations: operations.clone(),
            input_roles: input_roles.clone(),
            response_formats: vec![ImageResponseFormatV1::B64Json],
            metering_forms: vec![MeteringFormV1::Tokens, MeteringFormV1::Images],
            token_buckets: vec![TokenBucketV1::TotalInput, TokenBucketV1::TotalOutput],
            tier_dimensions: vec![TierDimensionV1::Resolution],
            repeat: true,
            request_elision_paths: request_elision_paths.clone(),
        })
        .collect())
}

fn paths(patterns: &[&str]) -> ComponentResultV1<Vec<PathPatternV1>> {
    patterns
        .iter()
        .map(|pattern| PathPatternV1::parse(pattern).map_err(|_| internal("invalid elision path")))
        .collect()
}

/// The host's required metering forms, when this family can report them all: `tokens` and
/// `images`.
fn required_forms(context: &ImageCallContextV1) -> ComponentResultV1<Vec<MeteringFormV1>> {
    if context.metering_required.is_empty() {
        return Err(capability("the host requires no metering form"));
    }
    if let Some(form) = context
        .metering_required
        .iter()
        .find(|form| !matches!(form, MeteringFormV1::Tokens | MeteringFormV1::Images))
    {
        return Err(capability(format!("Gemini image models cannot report {form:?} metering")));
    }
    Ok(context.metering_required.clone())
}

/// `generationConfig` with the optional `imageConfig` (native `gemini_image_config`): each value
/// trimmed and sent when non-empty, the resolution upper-cased (Google refuses `1k`).
fn generation_config(aspect_ratio: Option<&str>, resolution: Option<&str>) -> Value {
    let mut image_config = Map::new();
    if let Some(ratio) = aspect_ratio.map(str::trim).filter(|ratio| !ratio.is_empty()) {
        image_config.insert("aspectRatio".to_owned(), json!(ratio));
    }
    if let Some(size) = upper_resolution(resolution) {
        image_config.insert("imageSize".to_owned(), json!(size));
    }
    let mut config = json!({ "responseModalities": ["IMAGE"] });
    if !image_config.is_empty() {
        config["imageConfig"] = Value::Object(image_config);
    }
    config
}

fn upper_resolution(resolution: Option<&str>) -> Option<String> {
    resolution.map(str::trim).filter(|size| !size.is_empty()).map(str::to_ascii_uppercase)
}

/// The tier fact (GI-Q3): the upper-cased resolution, only when the client sent a non-empty one and
/// the host prices this call per image. On such a call a resolution outside the tier grammar is
/// refused: it could be priced by no tier row, and the native per-image guard refuses any
/// resolution its row does not price.
fn tier(resolution: Option<&str>, forms: &[MeteringFormV1]) -> ComponentResultV1<ImageTierV1> {
    let Some(word) =
        upper_resolution(resolution).filter(|_| forms.contains(&MeteringFormV1::Images))
    else {
        return Ok(ImageTierV1::default());
    };
    let word = TierWordV1::parse(&word).map_err(|_| {
        invalid(format!("'resolution' {word:?} is not a resolution the gateway can price"))
    })?;
    Ok(ImageTierV1 { resolution: Some(word), quality: None })
}

/// The `inlineData` part of the client's `image_url`, as the native `parse_data_url` reads it:
/// a `data:<mime>;base64,<payload>` string. A host-elided value (always, since `/image_url` is
/// declared) is read from its 64-byte head and sent as a `data_url_payload` reference, so the
/// host writes the payload and checks its base64 (GI-Q1).
fn reference_part(value: &Value) -> ComponentResultV1<Value> {
    let (text, blob) = if let Value::String(url) = value {
        (url.as_str(), None)
    } else if let Some(blob) = value.get(BLOB_KEY) {
        (
            blob["head"].as_str().unwrap_or_default(),
            Some(blob["id"].as_str().ok_or_else(|| internal("a placeholder has no id"))?),
        )
    } else {
        // The native text echoes the value; this one names its type, so an array of data URLs is
        // not copied into the error.
        let kind = match value {
            Value::Array(_) => "array",
            Value::Object(_) => "object",
            Value::Number(_) => "number",
            _ => "boolean",
        };
        return Err(invalid(format!(
            "Gemini Nano Banana `image_url` must be a single base64 data URL string \
             (data:<mime>;base64,<bytes>); got non-string {kind} — multi-source compose (e.g. \
             `image_url: [url1, url2]`) is not supported in this release"
        )));
    };
    let rest = text.strip_prefix("data:").ok_or_else(|| {
        let preview: String = text.chars().take(60).collect();
        invalid(format!(
            "Gemini Nano Banana `image_url` must be a base64 data URL \
             (data:<mime>;base64,<bytes>); got '{preview}'"
        ))
    })?;
    let (mime, payload) = rest.split_once(";base64,").ok_or_else(|| {
        invalid(
            "Gemini Nano Banana `image_url` data URL must use base64 encoding \
             (data:<mime>;base64,<bytes>)",
        )
    })?;
    if mime.is_empty() {
        return Err(invalid(
            "Gemini Nano Banana `image_url` data URL is missing the <mime> portion",
        ));
    }
    let data = if let Some(id) = blob {
        json!({ REF_KEY: { "blob": id, "transform": "data_url_payload" } })
    } else if is_base64_syntax(payload) {
        json!(payload)
    } else {
        return Err(invalid(
            "Gemini Nano Banana `image_url` data URL payload is not valid base64 (must use the \
             standard alphabet A-Z/a-z/0-9/+/, length a multiple of 4, 0-2 trailing '=' padding \
             chars)",
        ));
    };
    Ok(json!({ "inlineData": { "mimeType": mime, "data": data } }))
}

/// The native syntax-only base64 check, for a data URL the host did not elide.
fn is_base64_syntax(text: &str) -> bool {
    if text.is_empty() || !text.len().is_multiple_of(4) {
        return false;
    }
    let bytes = text.as_bytes();
    let padding = bytes.iter().rev().take_while(|byte| **byte == b'=').count();
    padding <= 2
        && bytes[..bytes.len() - padding]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
}

/// A string-or-absent field of the JSON request (`aspect_ratio`, `resolution`).
fn optional_text<'a>(
    body: &'a Map<String, Value>,
    key: &str,
) -> ComponentResultV1<Option<&'a str>> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(value) if is_blob(value) => {
            Err(invalid(format!("'{key}' is longer than the gateway accepts")))
        }
        Some(other) => {
            Err(invalid(format!("Gemini Nano Banana `{key}` must be a string (got {other})")))
        }
    }
}

/// The range check every `n` passes (native `validate_nano_banana_n`).
fn checked_n(n: u64) -> ComponentResultV1<u8> {
    if (1..=MAX_IMAGES).contains(&n) {
        u8::try_from(n).map_err(|_| internal("n fits a u8"))
    } else {
        Err(invalid(format!(
            "`n` must be in 1..={MAX_IMAGES} for Gemini Nano Banana models, got {n}"
        )))
    }
}

/// `prepare`: a JSON request is a generation (§2), a multipart request an edit (§3).
pub fn prepare(
    request: &Value,
    context: &ImageCallContextV1,
    route: GeminiRouteV1,
) -> ComponentResultV1<PreparedImageCallV1> {
    let forms = required_forms(context)?;
    if let Some(body) = request.get("json").and_then(Value::as_object) {
        return prepare_generation(body, &forms, route);
    }
    if let Some(parts) = request.pointer("/multipart/parts").and_then(Value::as_array) {
        if !route.edit {
            return Err(invalid("this family does not edit images"));
        }
        return prepare_edit(parts, &forms, route);
    }
    Err(invalid("the request is neither JSON nor multipart"))
}

fn prepare_generation(
    body: &Map<String, Value>,
    forms: &[MeteringFormV1],
    route: GeminiRouteV1,
) -> ComponentResultV1<PreparedImageCallV1> {
    let n = match body.get("n") {
        None | Some(Value::Null) => 1,
        Some(raw) => crate::bailian_image::whole_count(raw)
            .ok_or_else(|| invalid(format!("'n' must be a non-negative integer (got {raw})")))?,
    };
    let n = checked_n(n)?;
    let prompt = match body.get("prompt") {
        Some(value) if is_blob(value) => {
            return Err(invalid("'prompt' is longer than the gateway accepts"));
        }
        Some(Value::String(prompt)) => prompt,
        _ => return Err(invalid("Missing 'prompt' field")),
    };
    let mut parts = Vec::with_capacity(2);
    let mut references = 0;
    match body.get("image_url") {
        None | Some(Value::Null) => {}
        Some(value) => {
            parts.push(reference_part(value)?);
            references = 1;
        }
    }
    parts.push(json!({ "text": prompt }));
    let aspect_ratio = optional_text(body, "aspect_ratio")?;
    let resolution = optional_text(body, "resolution")?;
    let template = json!({
        "contents": [{ "role": "user", "parts": parts }],
        "generationConfig": generation_config(aspect_ratio, resolution),
    });
    prepared(
        ImageOperationV1::Generate,
        references,
        n,
        tier(resolution, forms)?,
        forms,
        &template,
        route,
    )
}

fn prepare_edit(
    parts: &[Value],
    forms: &[MeteringFormV1],
    route: GeminiRouteV1,
) -> ComponentResultV1<PreparedImageCallV1> {
    if parts.iter().any(|part| part["kind"] == "text" && part.get("blob").is_some()) {
        return Err(invalid("a text field is longer than the gateway accepts"));
    }
    let named = |name: &str| parts.iter().find(|part| part["name"] == name);
    if named("mask").is_some() {
        return Err(invalid(
            "`mask` is not supported for Gemini Nano Banana edits — Gemini conditions on the \
             input image directly via the prompt rather than a separate mask.",
        ));
    }
    let text = |name: &str| {
        named(name).filter(|part| part["kind"] == "text").and_then(|part| part["value"].as_str())
    };
    let stream = text("stream").map(str::trim);
    if stream.is_some_and(|value| value.eq_ignore_ascii_case("true") || value == "1") {
        return Err(invalid(
            "streaming /v1/images/edits (stream=true) is not supported for Gemini Nano Banana",
        ));
    }
    let n = match text("n").map(str::trim) {
        None | Some("") => 1,
        Some(raw) => raw.parse::<u32>().map(u64::from).map_err(|_| {
            invalid(format!("multipart `n` field value '{raw}' must be a non-negative integer"))
        })?,
    };
    let n = checked_n(n)?;
    let prompt = text("prompt")
        .map(str::trim)
        .ok_or_else(|| invalid("Missing `prompt` field in multipart body"))?;
    let images: Vec<&Value> =
        parts.iter().filter(|part| part["kind"] == "file" && part["name"] == "image").collect();
    let Some(image) = images.first() else {
        return Err(invalid(
            "Missing `image` file part in multipart body — /v1/images/edits requires a source \
             image to edit",
        ));
    };
    let blob = image["blob"].as_str().ok_or_else(|| internal("a file part has no blob"))?;
    // GI-Q6: the part's own media type when it names an image, else `image/png` (the native arm
    // sniffs bytes the component never sees).
    let mime = image["media_type"]
        .as_str()
        .filter(|media_type| {
            media_type.get(..6).is_some_and(|head| head.eq_ignore_ascii_case("image/"))
                && is_media_type(media_type)
        })
        .unwrap_or(DEFAULT_IMAGE_TYPE);
    let aspect_ratio = text("aspect_ratio");
    let resolution = text("resolution");
    let template = json!({
        "contents": [{
            "role": "user",
            "parts": [
                { "inlineData": {
                    "mimeType": mime,
                    "data": { REF_KEY: { "blob": blob, "transform": "base64" } },
                } },
                { "text": prompt },
            ],
        }],
        "generationConfig": generation_config(aspect_ratio, resolution),
    });
    prepared(
        ImageOperationV1::Edit,
        u32::try_from(images.len()).unwrap_or(u32::MAX),
        n,
        tier(resolution, forms)?,
        forms,
        &template,
        route,
    )
}

fn prepared(
    operation: ImageOperationV1,
    references: u32,
    repeat: u8,
    tier: ImageTierV1,
    forms: &[MeteringFormV1],
    template: &Value,
    route: GeminiRouteV1,
) -> ComponentResultV1<PreparedImageCallV1> {
    let mut descriptor = Map::new();
    descriptor.insert("method".to_owned(), json!("POST"));
    descriptor.insert("path".to_owned(), json!(route.path));
    if let Some(auth) = route.auth {
        descriptor.insert("auth".to_owned(), auth);
    }
    descriptor.insert("body".to_owned(), json!({ "json": { "template": template } }));
    let descriptor =
        MediaRequestDescriptorV1::parse(&Value::Object(descriptor).to_string(), &MediaLimitsV1::V1)
            .map_err(|error| internal(format!("built an invalid descriptor: {error}")))?;
    Ok(PreparedImageCallV1 {
        facts: ImageFactsV1 {
            operation,
            inputs: ImageInputsV1 { reference_image: references, ..ImageInputsV1::default() },
            // One image per upstream call, `repeat` calls (record §7).
            requested_outputs: u32::from(repeat),
            size: None,
            tier,
            tier_candidates: None,
            metering_forms: forms.to_vec(),
            bounds: None,
        },
        descriptor,
        repeat,
        response_body_form: ResponseBodyFormV1::Json,
        response_elision_paths: paths(&[RESPONSE_ELISION_PATH])?,
        immutable_body_paths: IMMUTABLE_BODY_PATHS.iter().map(|path| (*path).to_owned()).collect(),
        state: json!({ "metering_required": forms }).to_string(),
    })
}

fn unknown(reason: &str) -> ImageOutcomeV1 {
    ImageOutcomeV1::Unknown { reason: reason.to_owned(), error: None }
}

fn pointer(text: &str) -> ComponentResultV1<JsonPointerV1> {
    JsonPointerV1::parse(text).map_err(|_| internal("invalid artifact pointer"))
}

/// A token count: a non-negative integer, or an integral float such as `4000.0`; anything else is
/// not a count. `None` when absent.
fn count(value: &Value) -> Result<Option<u64>, ()> {
    match value {
        Value::Null => Ok(None),
        Value::Number(_) => crate::bailian_image::whole_count(value).map(Some).ok_or(()),
        _ => Err(()),
    }
}

/// The `TEXT` and `IMAGE` entries of a `*TokensDetails` list, reported only when they add up to
/// the total they split (GI-Q4); an entry without `tokenCount` is a proto3 zero.
fn modality_split(details: &Value, total: Option<u64>) -> Result<(Option<u64>, Option<u64>), ()> {
    let Some(entries) = details.as_array() else {
        return Ok((None, None));
    };
    let (mut text, mut image, mut other) = (None::<u64>, None::<u64>, false);
    for entry in entries {
        let tokens = count(&entry["tokenCount"])?.unwrap_or(0);
        let slot = match entry["modality"].as_str() {
            Some("TEXT") => &mut text,
            Some("IMAGE") => &mut image,
            _ => {
                other |= tokens > 0;
                continue;
            }
        };
        *slot = Some(slot.unwrap_or(0).checked_add(tokens).ok_or(())?);
    }
    let split = text.unwrap_or(0).checked_add(image.unwrap_or(0)).ok_or(())?;
    if other || (text.is_none() && image.is_none()) || Some(split) != total {
        return Ok((None, None));
    }
    Ok((text, image))
}

/// `usageMetadata` as the image token buckets (§4): the declared totals, the modality split when
/// it closes, and the cached prompt tokens. `thoughtsTokenCount` is not reported (GI-Q4).
fn token_usage(usage: &Value) -> Result<ImageTokenUsageV1, ()> {
    let total_input = count(&usage["promptTokenCount"])?;
    let total_output = count(&usage["candidatesTokenCount"])?;
    let (text_input, image_input) = modality_split(&usage["promptTokensDetails"], total_input)?;
    let (text_output, image_output) =
        modality_split(&usage["candidatesTokensDetails"], total_output)?;
    Ok(ImageTokenUsageV1 {
        text_input,
        image_input,
        cached_input: count(&usage["cachedContentTokenCount"])?,
        text_output,
        image_output,
        total_input,
        total_output,
        ..ImageTokenUsageV1::default()
    })
}

/// Google's `{"error": {"code", "message", "status"}}` on a non-2xx, by HTTP status.
fn error_envelope(status: u16, body: &Value) -> ErrorEnvelope {
    let code = status_code(status);
    let mut envelope = ErrorEnvelope::new(code, status, code_message(code));
    envelope.provider_message = body["error"]["message"].as_str().and_then(capped);
    envelope
}

fn policy_envelope(detail: Option<&str>) -> ErrorEnvelope {
    let mut envelope =
        ErrorEnvelope::new(ErrorCode::ContentPolicy, 400, code_message(ErrorCode::ContentPolicy));
    envelope.provider_message = detail.and_then(capped);
    envelope
}

/// `parse-response` for one round (§4).
pub fn parse_response(state: &Value, response: &Value) -> ComponentResultV1<ImageOutcomeV1> {
    let status = view_status(response);
    let body = response.pointer("/body/json").cloned().unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Ok(non_2xx(status, error_envelope(status, &body)));
    }
    if !body.is_object() {
        return Ok(unknown("a 2xx without a JSON object body"));
    }
    let empty = Vec::new();
    let candidates = body["candidates"].as_array().unwrap_or(&empty);
    let mut artifacts = Vec::new();
    for (c, candidate) in candidates.iter().enumerate() {
        let Some(parts) = candidate.pointer("/content/parts").and_then(Value::as_array) else {
            continue;
        };
        for (p, part) in parts.iter().enumerate() {
            // GI-Q7: an interim image of a thinking model is not delivered.
            if part["thought"] == true || !non_empty_inline(&part["inlineData"]["data"]) {
                continue;
            }
            let media_type = part["inlineData"]["mimeType"]
                .as_str()
                .filter(|media_type| media_type.starts_with("image/") && is_media_type(media_type))
                .unwrap_or(DEFAULT_IMAGE_TYPE);
            artifacts.push(ImageArtifactV1::Inline {
                pointer: pointer(&format!("/candidates/{c}/content/parts/{p}/inlineData/data"))?,
                encoding: ArtifactEncodingV1::Base64,
                media_type: media_type.to_owned(),
            });
        }
    }
    if artifacts.is_empty() {
        // GI-Q5: a prompt blocked before generation is refused; a candidate stopped for a policy
        // reason may have been generated (and charged), so it is undecided.
        if let Some(reason) = body
            .pointer("/promptFeedback/blockReason")
            .and_then(Value::as_str)
            .filter(|r| !r.is_empty())
        {
            return Ok(ImageOutcomeV1::Rejected { error: policy_envelope(Some(reason)) });
        }
        if let Some(reason) = candidates
            .iter()
            .filter_map(|candidate| candidate["finishReason"].as_str())
            .find(|reason| POLICY_FINISH_REASONS.contains(reason))
        {
            return Ok(ImageOutcomeV1::Unknown {
                reason: format!("a candidate finished for {reason} without an image"),
                error: Some(policy_envelope(Some(reason))),
            });
        }
        return Ok(unknown("a 2xx without an image"));
    }
    let Ok(tokens) = token_usage(&body["usageMetadata"]) else {
        return Ok(unknown("a malformed usage count"));
    };
    let tokens_required = state["metering_required"]
        .as_array()
        .is_some_and(|forms| forms.iter().any(|form| form == "tokens"));
    if tokens_required && (tokens.total_input.is_none() || tokens.total_output.is_none()) {
        return Ok(unknown("a declared token bucket is missing"));
    }
    let reported = tokens != ImageTokenUsageV1::default();
    Ok(ImageOutcomeV1::Succeeded {
        artifacts,
        metering: ImageMeteringV1 {
            tokens: reported.then_some(tokens),
            // Gemini reports no image count; the host settles the delivered count.
            images_reported: None,
            credits: None,
            upstream_cost: None,
        },
        extras: Value::Null,
    })
}

/// `render` over every round (§4): `{"created", "data": [{"b64_json", "revised_prompt": null}]}`,
/// the artifact indices counted across rounds in order.
pub fn render(
    outcomes: &[ImageOutcomeV1],
    context: &ImageRenderContextV1,
) -> ComponentResultV1<ImageRenderedV1> {
    let mut data = Vec::new();
    for outcome in outcomes {
        let ImageOutcomeV1::Succeeded { artifacts, .. } = outcome else {
            return Err(internal("Gemini renders succeeded rounds only"));
        };
        for _ in artifacts {
            let index = data.len();
            data.push(json!({
                "b64_json": { "$south.artifact": { "index": index, "as": "b64_json" } },
                "revised_prompt": null,
            }));
        }
    }
    if data.is_empty() {
        return Err(internal("Gemini renders at least one image"));
    }
    Ok(ImageRenderedV1 {
        template: json!({ "created": context.created, "data": data }).to_string(),
    })
}
