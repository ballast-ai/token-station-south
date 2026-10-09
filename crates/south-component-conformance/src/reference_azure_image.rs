//! The native reference implementation of the `image-azure` component: Azure AI Foundry's MAI
//! image API (image record §13, Azure row; §19 S-I-6).
//!
//! Transcribed from token-station-server's native Azure arm so a dual run agrees on the wire:
//! `azure_size_to_wh`, `validate_azure_image_caps`, `validate_azure_image_gen_request`,
//! `azure_prepare`, `azure_dispatch`, `azure_image_token_usage` and `azure_edits_translate_size`
//! in `gateway/src/modules/inference/handler/images/azure.rs`, the edit branch of
//! `handlers.rs`, the owned fields `AZURE_IMAGE_OWNED` in `execute.rs`, and the `AzureFoundry`
//! URL arm in `engine/upstream.rs`.
//!
//! - **Generation** is JSON: `{"model", "prompt", "width", "height"}` to `mai/v1/images/generations`.
//!   `size` (`WxH`, either case of `x`) becomes `width`/`height`, each at least 768 and their
//!   product at most 1,048,576; no `size`, `""` or `auto` is 1024×1024. A missing or non-string
//!   `prompt` is refused.
//! - **Edit** is multipart to `mai/v1/images/edits`: every part is forwarded in order, `model` is
//!   set to the routed upstream id, and `size` is translated as the native arm does — absent:
//!   nothing changes (Azure defaults from the source image); `""` or `auto`: `size` is dropped;
//!   `WxH`: `size`, `width` and `height` are dropped and `width`, `height` appended.
//! - **Caps**, both operations: one image (`n` absent or 1; a JSON `n` that is not a whole
//!   number reads as 1, as the native arm's coercion does) and `b64_json` only (absent or `""`
//!   is `b64_json`).
//! - **Auth** is `header_secret` `api-key` on the configured slot.
//! - **Response**: `{"data": [{"b64_json"}], "usage"?, …}`, passed to the client as is with the
//!   image bytes delivered by the host. Each `data[i].b64_json` is an `inline` base64 artifact,
//!   `image/png` unless the body's `output_format` says `jpeg` or `webp`.
//! - **Metering**: `tokens` or `images`, as the host's pricing decision requires. The declared
//!   buckets are `total_input` (`usage.input_tokens`) and `total_output` (`usage.output_tokens`),
//!   which every OpenAI-images-shaped usage block carries; the breakdown
//!   (`input_tokens_details.text_tokens`, `image_tokens`, `cached_tokens`, or a top-level
//!   `cached_tokens`) is reported when present and `null` when absent. The host folds a missing
//!   breakdown into text input, the native arm's rule (#184 R1), as pricing policy.
//!
//! Differences from the native arm, all deliberate (record §14): when `tokens` is required, a 2xx
//! whose usage lacks either declared bucket is `unknown`, where the native arm settled any
//! non-zero bucket (the partial-report gap of §9.1); a negative or fractional count is `unknown`,
//! where the native arm clamped or truncated it (an integral float such as `4000.0` is read as
//! `4000`, as there); a prompt above the 1 MiB fallback threshold is refused before admission; a
//! 4xx is `rejected` (no output, no charge) on every path, as the boundary record §6.4 rule makes
//! it after the dual run.

use serde_json::{Map, Value, json};
use south_contracts::JsonPointerV1;
use south_contracts::image::{
    ArtifactEncodingV1, ImageArtifactV1, ImageCallContextV1, ImageFactsV1, ImageInputsV1,
    ImageMeteringV1, ImageModelCapabilitiesV1, ImageOperationV1, ImageRenderContextV1,
    ImageResponseFormatV1, ImageSizeV1, ImageTierV1, ImageTokenUsageV1, InputRoleKeyV1,
    MeteringFormV1, PreparedImageCallV1, TokenBucketV1,
};
use south_contracts::media::{
    MediaLimitsV1, MediaPartV1, MediaRequestDescriptorV1, PathPatternV1, ResponseBodyFormV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

use crate::{ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1};

/// The package name and version this reference is published as.
pub const NAME: &str = "image-azure";
/// The package version.
pub const VERSION: &str = "1.0.0";
/// The world this reference exports.
pub const WORLD: &str = "image-adapter-v1";
/// The family, as a host's image rows name it.
pub const AZURE_MAI: &str = "azure-mai";

const GENERATIONS_PATH: &str = "mai/v1/images/generations";
const EDITS_PATH: &str = "mai/v1/images/edits";
/// The native arm's `AZURE_IMAGE_OWNED`: request extras may not set these.
const IMMUTABLE_BODY_PATHS: [&str; 5] = ["model", "width", "height", "n", "response_format"];
const MIN_EDGE: u32 = 768;
const MAX_PIXELS: u64 = 1_048_576;
const DEFAULT_EDGE: u32 = 1024;
const BLOB_KEY: &str = "$south.blob";

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct AzureImageReferenceV1;

fn invalid(message: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, message)
}

fn capability(message: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Capability, 400, message)
}

fn internal(message: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, message)
}

fn is_blob(value: &Value) -> bool {
    value.get(BLOB_KEY).is_some()
}

/// `azure_size_to_wh`: `None`, `""` and `auto` are 1024×1024.
fn size_to_wh(size: Option<&str>) -> ComponentResultV1<(u32, u32)> {
    let (width, height) = match size {
        None | Some("" | "auto") => (DEFAULT_EDGE, DEFAULT_EDGE),
        Some(text) => {
            let (w, h) = text.split_once(['x', 'X']).ok_or_else(|| {
                invalid(format!("invalid size '{text}', expected WxH (e.g. 1024x1024)"))
            })?;
            let axis = |part: &str, name: &str| {
                part.trim()
                    .parse::<u32>()
                    .map_err(|_| invalid(format!("invalid {name} in size '{text}'")))
            };
            (axis(w, "width")?, axis(h, "height")?)
        }
    };
    if width < MIN_EDGE || height < MIN_EDGE {
        return Err(invalid(format!(
            "Azure MAI image requires width and height ≥ 768 (got {width}x{height})"
        )));
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(invalid(format!(
            "Azure MAI image requires width*height ≤ 1048576 (got {width}x{height})"
        )));
    }
    Ok((width, height))
}

/// `validate_azure_image_caps`.
fn check_caps(n: u64, response_format: Option<&str>) -> ComponentResultV1<()> {
    if n != 1 {
        return Err(invalid(format!(
            "'n' = {n} is not supported for Azure MAI image models; they generate a single image \
             per request (omit `n` or set it to 1)"
        )));
    }
    if let Some(format) =
        response_format.filter(|format| !format.is_empty() && *format != "b64_json")
    {
        return Err(invalid(format!(
            "response_format '{format}' is not supported for Azure MAI image models; they only \
             return b64_json"
        )));
    }
    Ok(())
}

/// The native coercion of a JSON `n`: an integer or a whole non-negative float; anything else
/// reads as absent (and therefore 1).
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

fn required_forms(context: &ImageCallContextV1) -> ComponentResultV1<Vec<MeteringFormV1>> {
    if let Some(form) = context
        .metering_required
        .iter()
        .find(|form| !matches!(form, MeteringFormV1::Tokens | MeteringFormV1::Images))
    {
        return Err(capability(format!("Azure MAI image models cannot report {form:?} metering")));
    }
    if context.metering_required.is_empty() {
        return Err(capability("the host requires no metering form"));
    }
    Ok(context.metering_required.clone())
}

fn auth_json(config: &ProviderConfig) -> String {
    config.auth.as_ref().map_or_else(String::new, |slot| {
        format!(
            r#","auth":{{"arm":"header_secret","header":"api-key","slot":{}}}"#,
            serde_json::to_string(slot.as_str()).unwrap_or_default()
        )
    })
}

fn facts(
    operation: ImageOperationV1,
    inputs: ImageInputsV1,
    size: Option<(u32, u32)>,
    metering_forms: Vec<MeteringFormV1>,
) -> ImageFactsV1 {
    ImageFactsV1 {
        operation,
        inputs,
        requested_outputs: 1,
        size: size.map(|(width, height)| ImageSizeV1 { width, height }),
        tier: ImageTierV1::default(),
        tier_candidates: None,
        metering_forms,
        bounds: None,
    }
}

fn prepared(
    facts: ImageFactsV1,
    descriptor: &str,
    immutable: &[&str],
    metering_required: &[MeteringFormV1],
) -> ComponentResultV1<PreparedImageCallV1> {
    let descriptor = MediaRequestDescriptorV1::parse(descriptor, &MediaLimitsV1::V1)
        .map_err(|error| internal(format!("built an invalid descriptor: {error}")))?;
    Ok(PreparedImageCallV1 {
        facts,
        descriptor,
        repeat: 1,
        response_body_form: ResponseBodyFormV1::Json,
        response_elision_paths: vec![
            PathPatternV1::parse("/data/*/b64_json")
                .map_err(|_| internal("invalid elision path"))?,
        ],
        immutable_body_paths: immutable.iter().map(|path| (*path).to_owned()).collect(),
        state: json!({ "metering_required": metering_required }).to_string(),
    })
}

impl AzureImageReferenceV1 {
    fn prepare_generation(
        config: &ProviderConfig,
        body: &Map<String, Value>,
        context: &ImageCallContextV1,
        forms: &[MeteringFormV1],
    ) -> ComponentResultV1<PreparedImageCallV1> {
        let prompt = match body.get("prompt") {
            Some(value) if is_blob(value) => {
                return Err(invalid("'prompt' is longer than the gateway accepts"));
            }
            Some(Value::String(prompt)) => prompt,
            _ => return Err(invalid("Missing 'prompt' field")),
        };
        let n = body.get("n").and_then(json_count).unwrap_or(1);
        check_caps(n, body.get("response_format").and_then(Value::as_str))?;
        let (width, height) = size_to_wh(body.get("size").and_then(Value::as_str))?;
        let template = json!({
            "model": context.upstream_model, "prompt": prompt, "width": width, "height": height,
        });
        let descriptor = format!(
            r#"{{"method":"POST","path":"{GENERATIONS_PATH}"{},"body":{{"json":{{"template":{template}}}}}}}"#,
            auth_json(config)
        );
        prepared(
            facts(
                ImageOperationV1::Generate,
                ImageInputsV1::default(),
                Some((width, height)),
                forms.to_vec(),
            ),
            &descriptor,
            &IMMUTABLE_BODY_PATHS,
            forms,
        )
    }

    fn prepare_edit(
        config: &ProviderConfig,
        parts: &[Value],
        context: &ImageCallContextV1,
        forms: &[MeteringFormV1],
    ) -> ComponentResultV1<PreparedImageCallV1> {
        let text =
            |name: &str| parts.iter().find(|part| part["kind"] == "text" && part["name"] == name);
        if parts.iter().any(|part| part["kind"] == "text" && part.get("blob").is_some()) {
            return Err(invalid("a text field is longer than the gateway accepts"));
        }
        let field = |name: &str| text(name).and_then(|part| part["value"].as_str());
        let n = match field("n").map(str::trim) {
            None | Some("") => 1,
            Some(value) => value.parse::<u64>().map_err(|_| invalid("invalid 'n'"))?,
        };
        check_caps(n, field("response_format").map(str::trim))?;
        let size = field("size").map(str::trim);
        let translated = match size {
            None | Some("" | "auto") => None,
            Some(_) => Some(size_to_wh(size)?),
        };
        let dropped: &[&str] = match size {
            None => &[],
            Some("" | "auto") => &["size"],
            Some(_) => &["size", "width", "height"],
        };
        let mut inputs = ImageInputsV1::default();
        let mut out: Vec<MediaPartV1> = Vec::with_capacity(parts.len() + 2);
        let mut model_set = false;
        for part in parts {
            let name = part["name"].as_str().ok_or_else(|| internal("a part has no name"))?;
            if dropped.contains(&name) {
                continue;
            }
            if part["kind"] == "file" {
                match name {
                    "image" | "image[]" => inputs.input_image += 1,
                    "mask" => inputs.mask += 1,
                    _ => {}
                }
                out.push(MediaPartV1::File {
                    name: name.to_owned(),
                    blob: serde_json::from_value(part["blob"].clone())
                        .map_err(|_| internal("a file part has no blob"))?,
                    transform: south_contracts::media::MediaTransformV1::AsIs,
                    filename: part["filename"].as_str().map(str::to_owned),
                    media_type: part["media_type"].as_str().map(str::to_owned),
                });
            } else {
                let value = if name == "model" {
                    model_set = true;
                    context.upstream_model.clone()
                } else {
                    part["value"].as_str().unwrap_or_default().to_owned()
                };
                out.push(MediaPartV1::Text { name: name.to_owned(), value });
            }
        }
        if !model_set {
            out.push(MediaPartV1::Text {
                name: "model".to_owned(),
                value: context.upstream_model.clone(),
            });
        }
        if let Some((width, height)) = translated {
            out.push(MediaPartV1::Text { name: "width".to_owned(), value: width.to_string() });
            out.push(MediaPartV1::Text { name: "height".to_owned(), value: height.to_string() });
        }
        let parts_json =
            serde_json::to_string(&out).map_err(|error| internal(error.to_string()))?;
        let descriptor = format!(
            r#"{{"method":"POST","path":"{EDITS_PATH}"{},"body":{{"multipart":{{"parts":{parts_json}}}}}}}"#,
            auth_json(config)
        );
        prepared(
            facts(ImageOperationV1::Edit, inputs, translated, forms.to_vec()),
            &descriptor,
            &[],
            forms,
        )
    }
}

/// A non-negative integer count, or an integral float such as `4000.0`; anything else is not a
/// count. `None` when absent.
fn count(value: &Value) -> Result<Option<u64>, ()> {
    match value {
        Value::Null => Ok(None),
        Value::Number(number) => {
            if let Some(count) = number.as_u64() {
                return Ok(Some(count));
            }
            let float = number.as_f64().ok_or(())?;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            if float.is_finite() && float >= 0.0 && float.fract() == 0.0 && float < u64::MAX as f64
            {
                Ok(Some(float as u64))
            } else {
                Err(())
            }
        }
        _ => Err(()),
    }
}

fn token_usage(usage: &Value) -> Result<ImageTokenUsageV1, ()> {
    let details = &usage["input_tokens_details"];
    let cached = match count(&details["cached_tokens"])? {
        Some(cached) => Some(cached),
        None => count(&usage["cached_tokens"])?,
    };
    Ok(ImageTokenUsageV1 {
        text_input: count(&details["text_tokens"])?,
        image_input: count(&details["image_tokens"])?,
        cached_input: cached,
        total_input: count(&usage["input_tokens"])?,
        total_output: count(&usage["output_tokens"])?,
        ..ImageTokenUsageV1::default()
    })
}

fn unknown(reason: &str) -> ImageOutcomeV1 {
    ImageOutcomeV1::Unknown { reason: reason.to_owned(), error: None }
}

fn error_envelope(status: u16, body: &Value) -> ErrorEnvelope {
    let provider_code = body["error"]["code"].as_str().unwrap_or_default().to_ascii_lowercase();
    let code =
        if provider_code.contains("content_policy") || provider_code.contains("contentfilter") {
            ErrorCode::ContentPolicy
        } else {
            match status {
                401 | 403 => ErrorCode::Auth,
                402 => ErrorCode::PaymentRequired,
                408 => ErrorCode::Timeout,
                429 => ErrorCode::RateLimit,
                500..=599 => ErrorCode::UpstreamUnavailable,
                _ => ErrorCode::InvalidRequest,
            }
        };
    let message = match code {
        ErrorCode::Auth => "the upstream rejected the credential",
        ErrorCode::PaymentRequired => {
            "the upstream requires payment or the account is out of funds"
        }
        ErrorCode::RateLimit => "the upstream rate limited this request",
        ErrorCode::ContentPolicy => "the upstream refused on content-policy grounds",
        ErrorCode::Timeout => "the upstream did not answer in time",
        ErrorCode::UpstreamUnavailable => "the upstream is unavailable",
        _ => "the upstream refused the request as malformed",
    };
    let mut envelope = ErrorEnvelope::new(code, status, message);
    envelope.provider_message = body["error"]["message"]
        .as_str()
        .filter(|message| message.chars().count() <= 256)
        .map(str::to_owned);
    envelope
}

impl ImageComponentV1 for AzureImageReferenceV1 {
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
        if config.provider != AZURE_MAI {
            return Err(capability(format!("this package serves the {AZURE_MAI} family")));
        }
        Ok(config
            .models
            .iter()
            .map(|model| ImageModelCapabilitiesV1 {
                model: model.model.clone(),
                operations: vec![ImageOperationV1::Generate, ImageOperationV1::Edit],
                input_roles: vec![
                    InputRoleKeyV1 {
                        key: "image".to_owned(),
                        role: south_contracts::image::ImageInputRoleV1::InputImage,
                    },
                    InputRoleKeyV1 {
                        key: "image[]".to_owned(),
                        role: south_contracts::image::ImageInputRoleV1::InputImage,
                    },
                    InputRoleKeyV1 {
                        key: "mask".to_owned(),
                        role: south_contracts::image::ImageInputRoleV1::Mask,
                    },
                ],
                response_formats: vec![ImageResponseFormatV1::B64Json],
                metering_forms: vec![MeteringFormV1::Tokens, MeteringFormV1::Images],
                token_buckets: vec![TokenBucketV1::TotalInput, TokenBucketV1::TotalOutput],
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
        if config.provider != AZURE_MAI {
            return Err(capability(format!("this package serves the {AZURE_MAI} family")));
        }
        let forms = required_forms(context)?;
        if let Some(body) = request.get("json").and_then(Value::as_object) {
            return Self::prepare_generation(config, body, context, &forms);
        }
        if let Some(parts) = request.pointer("/multipart/parts").and_then(Value::as_array) {
            return Self::prepare_edit(config, parts, context, &forms);
        }
        Err(invalid("the request is neither JSON nor multipart"))
    }

    fn parse_response(&self, state: &Value, response: &Value) -> ComponentResultV1<ImageOutcomeV1> {
        let status =
            response["status"].as_u64().and_then(|status| u16::try_from(status).ok()).unwrap_or(0);
        let body = response.pointer("/body/json").cloned().unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            let error = error_envelope(status, &body);
            return Ok(if (400..500).contains(&status) && status != 408 {
                ImageOutcomeV1::Rejected { error }
            } else {
                ImageOutcomeV1::Unknown {
                    reason: format!("upstream status {status}"),
                    error: Some(error),
                }
            });
        }
        let Some(data) = body["data"].as_array().filter(|data| !data.is_empty()) else {
            return Ok(unknown("a 2xx without data"));
        };
        if !data.iter().all(|item| is_blob(&item["b64_json"])) {
            return Ok(unknown("a data item without b64_json"));
        }
        let media_type = match body["output_format"].as_str() {
            Some("jpeg" | "jpg") => "image/jpeg",
            Some("webp") => "image/webp",
            _ => "image/png",
        };
        let artifacts = (0..data.len())
            .map(|index| {
                JsonPointerV1::parse(&format!("/data/{index}/b64_json"))
                    .map(|pointer| ImageArtifactV1::Inline {
                        pointer,
                        encoding: ArtifactEncodingV1::Base64,
                        media_type: media_type.to_owned(),
                    })
                    .map_err(|_| internal("invalid artifact pointer"))
            })
            .collect::<ComponentResultV1<Vec<_>>>()?;
        let tokens_required = state["metering_required"]
            .as_array()
            .is_some_and(|forms| forms.iter().any(|form| form == "tokens"));
        let Ok(tokens) = token_usage(&body["usage"]) else {
            return Ok(unknown("a malformed usage count"));
        };
        if tokens_required && (tokens.total_input.is_none() || tokens.total_output.is_none()) {
            return Ok(unknown("a declared token bucket is missing"));
        }
        let reported = tokens != ImageTokenUsageV1::default();
        // The skeleton the client body is rendered from: the response with each image replaced by
        // `null` (its `$south.blob` placeholder never leaves the sandbox).
        let mut skeleton = body;
        if let Some(items) = skeleton["data"].as_array_mut() {
            for item in items {
                item["b64_json"] = Value::Null;
            }
        }
        Ok(ImageOutcomeV1::Succeeded {
            artifacts,
            metering: ImageMeteringV1 {
                tokens: reported.then_some(tokens),
                images_reported: None,
                credits: None,
                upstream_cost: None,
            },
            extras: json!({ "body": skeleton }),
        })
    }

    fn render(
        &self,
        _state: &Value,
        outcomes: &[ImageOutcomeV1],
        _context: &ImageRenderContextV1,
    ) -> ComponentResultV1<ImageRenderedV1> {
        let [ImageOutcomeV1::Succeeded { artifacts, extras, .. }] = outcomes else {
            return Err(internal("Azure MAI renders exactly one succeeded round"));
        };
        let mut body = extras["body"].clone();
        let items =
            body["data"].as_array_mut().ok_or_else(|| internal("the skeleton has no data"))?;
        if items.len() != artifacts.len() {
            return Err(internal("the skeleton and the artifacts disagree"));
        }
        for (index, item) in items.iter_mut().enumerate() {
            item["b64_json"] = json!({ "$south.artifact": { "index": index, "as": "b64_json" } });
        }
        Ok(ImageRenderedV1 { template: body.to_string() })
    }
}
