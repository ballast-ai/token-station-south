//! The native reference implementation of the `image-openai-compatible` component.
//!
//! The host's OpenAI-compatible image fallback as a package (design record
//! `docs/design/2026-10-16-image-openai-compatible.md`, step 1; image record §13 row "OpenAI-compatible
//! fallback (DI4)", Q12). Transcribed from token-station-server's native fallback arm: the strict `n`
//! of `handler/images/handlers.rs`, `openai_compat_prepare` / `openai_compat_dispatch` in
//! `handler/images/openai_compat.rs`, the generic multipart edit pass-through of `handlers.rs`, the
//! owned sets `OPENAI_COMPAT_IMAGE_OWNED` and `MULTIPART_EDIT_OWNED` of `handler/images/execute.rs`,
//! `image_size_hint` / `image_size_long_edge` of `engine/token_counter/media.rs`,
//! `ImageTokenUsage::from_openai_usage` of the protocol crate, and the `AzureOpenai` and `GlmCoding`
//! arms of `engine/upstream.rs`:
//!
//! - **Families** (OCI-Q1 (b)): `openai-compatible` (`bearer`), and the two Azure `OpenAI` URL
//!   forms of `speech-openai-compatible` 1.1, `azure-openai-v1` and `azure-openai-dated`
//!   (`header_secret` `api-key`), from step 1; the GLM Coding Plan, `glm-coding` (`bearer`, and the
//!   manifest's `user_agent` `opencode/1.15.6`, which the host sends through its controlled
//!   user-agent slot), from step 2.
//! - **Path**: `openai-compatible` and `azure-openai-v1` use the API-root rule of
//!   `speech-openai-compatible` (`images/generations` / `images/edits` below the binding's
//!   `base_url` when it has a path, `v1/…` when it is an origin); `azure-openai-dated` posts to
//!   `openai/deployments/{deployment}/images/…` with the sanctioned `api-version` query, the
//!   deployment being the upstream model as one escaped segment and the version the config value
//!   `api_version` (default `2024-10-21`); `glm-coding` posts to `api/coding/paas/v4/images/…`
//!   below the row endpoint whatever its path, as native's `GlmCoding` URL arm does.
//! - **Generation** (JSON): the client's body with `model` set to the upstream id; every other
//!   member forwarded as sent, an elided string as an `as_is` reference. `n` as the handler reads
//!   it (absent or `null` 1, an integer or integral float, 1..=10).
//! - **Edit** (multipart): every part forwarded in order, file parts `as_is`, `model` set to the
//!   upstream id (appended when absent); `n` a decimal 1..=10 and `stream` `true` / `1` refused with
//!   the native texts, before admission.
//! - **Facts**: `requested_outputs` the `n`; generation inputs 0 (native counts none on these
//!   types); edit inputs the parts named `image` / `image[]` and `mask`; `size` from the native size
//!   hint (`WxH`, `W*H`, `W×H`, `Nk`); no tier, no bounds.
//! - **Response**: each `data` entry's non-empty `b64_json` is an `inline` base64 artifact
//!   (`image/png`, or the body's `output_format`), else its non-empty `url` a `url` artifact; any
//!   other 2xx is `unknown`. `tokens` from `usage` as native reads it, plus `total_input` from the
//!   top-level `input_tokens` (OCI-Q3, D11) and integral floats accepted (D9); the declared bucket is
//!   `total_output` (OCI-Q4).
//! - **Render**: the upstream body as the client received it natively, each image replaced by an
//!   artifact marker in the form the upstream answered in.

use serde_json::{Map, Value, json};
use south_contracts::JsonPointerV1;
use south_contracts::image::{
    ArtifactEncodingV1, ImageArtifactV1, ImageCallContextV1, ImageFactsV1, ImageInputRoleV1,
    ImageInputsV1, ImageMeteringV1, ImageModelCapabilitiesV1, ImageOperationV1,
    ImageRenderContextV1, ImageResponseFormatV1, ImageSizeV1, ImageTierV1, ImageTokenUsageV1,
    InputRoleKeyV1, MAX_IMAGE_EDGE, MAX_IMAGE_INPUTS_PER_ROLE, MeteringFormV1, PreparedImageCallV1,
    TokenBucketV1,
};
use south_contracts::media::{
    MediaLimitsV1, MediaPartV1, MediaRequestDescriptorV1, MediaTransformV1, PathPatternV1,
    ResponseBodyFormV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorEnvelope, ProviderConfig};

use crate::image_shared::{
    bearer, capability, capped, code_message, internal, invalid, is_blob, non_2xx,
    non_empty_inline, status_code, view_status, with_refs,
};
use crate::reference::{azure_openai_dated, glm_coding};
use crate::{ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1};

/// The package name this reference is published as.
pub const NAME: &str = "image-openai-compatible";
/// The package version.
pub const VERSION: &str = "1.0.7";
/// The world this reference exports.
pub const WORLD: &str = "image-adapter-v1";
/// The family, the provider, embeddings and speech worlds' name for the same wire shape (OCI-Q2).
pub const OPENAI_COMPATIBLE: &str = "openai-compatible";
/// Azure `OpenAI`'s GA v1 surface, the provider, embeddings and speech worlds' name.
pub const AZURE_OPENAI_V1: &str = "azure-openai-v1";
/// Azure `OpenAI`'s dated, deployment-addressed surface, the provider and speech worlds' name.
pub const AZURE_OPENAI_DATED: &str = azure_openai_dated::FAMILY;
/// The header Azure `OpenAI` reads the resource key from (native `apply_provider_auth`).
pub const AZURE_SECRET_HEADER: &str = "api-key";
/// The Z.ai GLM Coding Plan, the provider world's name (`provider-openai-compatible`'s family).
pub const GLM_CODING: &str = glm_coding::FAMILY;
/// The GLM Coding Plan's API root below the row endpoint.
///
/// The endpoint is an origin such as `https://api.z.ai`; native `build_upstream_url`'s `GlmCoding`
/// arm appends this root for every path that is not `/v1/messages`.
pub const GLM_CODING_API_ROOT: &str = "api/coding/paas/v4";

const GENERATIONS: &str = "images/generations";
const EDITS: &str = "images/edits";
/// The native `OPENAI_COMPAT_IMAGE_OWNED`: request extras may not set these on a generation.
pub const GENERATION_IMMUTABLE: [&str; 7] =
    ["model", "n", "size", "image_size", "quality", "resolution", "response_format"];
/// The native `MULTIPART_EDIT_OWNED`: request extras may neither add, remove nor rewrite these
/// parts of an edit (`image` also covers `image[]`).
pub const EDIT_IMMUTABLE: [&str; 9] =
    ["model", "n", "size", "quality", "width", "height", "image", "mask", "response_format"];
/// Where a reference image of a generation can be (OCI-Q5): elided whatever its length, so a data
/// URL never enters the sandbox. The bytes still pass `as_is`.
const REQUEST_ELISION_PATHS: [&str; 2] = ["/image", "/image/*"];
/// The most images `n` asks for, on both surfaces (the native handlers' range).
const MAX_N: u32 = 10;
/// The media type of a `b64_json` image when the body names no `output_format` (OCI-Q6).
const DEFAULT_INLINE_MEDIA_TYPE: &str = "image/png";
/// The budget of the response skeleton carried in `extras` (the outcome frame holds 64 KiB).
const SKELETON_BUDGET: usize = 48 * 1024;
/// The native multipart refusal of `stream`.
const STREAM_REFUSAL: &str = "streaming /v1/images/edits (stream=true) is not yet supported by \
     this gateway; re-submit without `stream` or with `stream=false`";

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAiCompatibleImageReferenceV1;

/// The family a configuration names, when this package serves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    OpenAiCompatible,
    AzureV1,
    AzureDated,
    GlmCoding,
}

impl Family {
    fn of(config: &ProviderConfig) -> ComponentResultV1<Self> {
        match config.provider.as_str() {
            OPENAI_COMPATIBLE => Ok(Self::OpenAiCompatible),
            AZURE_OPENAI_V1 => Ok(Self::AzureV1),
            AZURE_OPENAI_DATED => Ok(Self::AzureDated),
            GLM_CODING => Ok(Self::GlmCoding),
            _ => Err(capability(format!(
                "this package serves the {OPENAI_COMPATIBLE}, {AZURE_OPENAI_V1}, \
                 {AZURE_OPENAI_DATED} and {GLM_CODING} families"
            ))),
        }
    }
}

/// Where a call goes and how it is authenticated.
struct Route {
    path: String,
    query: Option<Value>,
    auth: Option<Value>,
}

/// The path of `endpoint` below the binding's `base_url`: its API root is its own path, or `v1`
/// when it is an origin (`speech-openai-compatible`'s and the embeddings package's rule).
fn api_path(config: &ProviderConfig, endpoint: &str) -> String {
    let base = config.base_url.as_str();
    let rest = base.split_once("://").map_or(base.as_str(), |(_, rest)| rest);
    if rest.contains('/') { endpoint.to_owned() } else { format!("v1/{endpoint}") }
}

/// `{"arm": "header_secret", "header": "api-key", "slot": …}` on the configured slot.
fn azure_key(config: &ProviderConfig) -> Option<Value> {
    config.auth.as_ref().map(|slot| {
        json!({ "arm": "header_secret", "header": AZURE_SECRET_HEADER, "slot": slot.as_str() })
    })
}

/// The dated version: the config value, else native's default; anything not dated is a
/// configuration error (the provider and speech worlds' `azure-openai-dated` rule).
fn dated_version(config: &ProviderConfig) -> ComponentResultV1<&str> {
    let version = config
        .declared
        .get(azure_openai_dated::API_VERSION_KEY)
        .unwrap_or(azure_openai_dated::DEFAULT_API_VERSION);
    if crate::reference::is_dated_api_version(version) {
        Ok(version)
    } else {
        Err(internal(format!(
            "the `{}` configuration value is not a dated API version",
            azure_openai_dated::API_VERSION_KEY
        )))
    }
}

/// The route of `endpoint` (`images/generations` or `images/edits`) for the family. The dated
/// family's configuration and deployment are checked here, before any request refusal.
fn route(
    config: &ProviderConfig,
    family: Family,
    endpoint: &str,
    upstream_model: &str,
) -> ComponentResultV1<Route> {
    Ok(match family {
        Family::OpenAiCompatible => {
            Route { path: api_path(config, endpoint), query: None, auth: bearer(config) }
        }
        Family::AzureV1 => {
            Route { path: api_path(config, endpoint), query: None, auth: azure_key(config) }
        }
        Family::AzureDated => {
            let version = dated_version(config)?;
            if upstream_model.is_empty() {
                return Err(capability(
                    "Azure OpenAI addresses the deployment in the URL path, so a request without \
                     a model has no deployment to send to",
                ));
            }
            Route {
                path: format!(
                    "openai/deployments/{}/{endpoint}",
                    crate::url_segment::encode(upstream_model)
                ),
                query: Some(json!([{ "name": "api-version", "value": version }])),
                auth: azure_key(config),
            }
        }
        // Native appends the Coding Plan root to the trimmed endpoint whatever its path, and
        // presents `api_key` only (no `oauth_token` fallback, unlike `glm`). The user-agent is
        // not the descriptor's to carry (`user-agent` is reserved): the manifest declares it.
        Family::GlmCoding => Route {
            path: format!("{GLM_CODING_API_ROOT}/{endpoint}"),
            query: None,
            auth: bearer(config),
        },
    })
}

/// The host's required metering forms, when this package can report them all: `tokens` and
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
        return Err(capability(format!(
            "OpenAI-compatible image models cannot report {form:?} metering"
        )));
    }
    Ok(context.metering_required.clone())
}

/// A request value as a refusal echoes it: as the native handler prints it (`serde_json`'s
/// display), except that a host-elided string is named rather than printed.
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

/// The native `coerce_image_count`: an integer or a whole non-negative float.
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

/// The generation handler's strict `n`: absent or `null` is 1; a count in `1..=10`.
fn generation_n(body: &Map<String, Value>) -> ComponentResultV1<u32> {
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
    if (1..=MAX_N).contains(&n) {
        Ok(n)
    } else {
        Err(invalid(format!("'n' must be in 1..=10 for /v1/images/generations, got {n}")))
    }
}

/// `image_size_long_edge` as dimensions: `Nk` is `N×1024` square (the long edge native prices
/// by), `WxH`, `W*H` or `W×H` (any case, spaces trimmed) as written. `None` when unparseable or
/// outside the contract's 1 to [`MAX_IMAGE_EDGE`] pixels.
fn size_of(hint: &str) -> Option<ImageSizeV1> {
    let text = hint.trim().to_ascii_lowercase();
    let (width, height) =
        if let Some(k) = text.strip_suffix('k').and_then(|k| k.trim().parse::<u32>().ok()) {
            let edge = k.saturating_mul(1024);
            (edge, edge)
        } else {
            let parts: Vec<&str> = text.split(['x', '*', '×']).collect();
            let [width, height] = parts.as_slice() else { return None };
            (width.trim().parse::<u32>().ok()?, height.trim().parse::<u32>().ok()?)
        };
    let edge = 1..=MAX_IMAGE_EDGE;
    (edge.contains(&width) && edge.contains(&height)).then_some(ImageSizeV1 { width, height })
}

/// `image_size_hint`: `size`, else `image_size`, read as a string. A `size` that is present but
/// not a string (or `null`) gives no hint and does not fall back.
fn generation_size(body: &Map<String, Value>) -> Option<ImageSizeV1> {
    body.get("size").or_else(|| body.get("image_size")).and_then(Value::as_str).and_then(size_of)
}

fn facts(
    operation: ImageOperationV1,
    inputs: ImageInputsV1,
    requested_outputs: u32,
    size: Option<ImageSizeV1>,
    metering_forms: Vec<MeteringFormV1>,
) -> ImageFactsV1 {
    ImageFactsV1 {
        operation,
        inputs,
        requested_outputs,
        size,
        tier: ImageTierV1::default(),
        tier_candidates: None,
        metering_forms,
        bounds: None,
    }
}

fn paths(patterns: &[&str]) -> ComponentResultV1<Vec<PathPatternV1>> {
    patterns
        .iter()
        .map(|pattern| PathPatternV1::parse(pattern).map_err(|_| internal("invalid path pattern")))
        .collect()
}

fn descriptor(route: Route, body: Value) -> ComponentResultV1<MediaRequestDescriptorV1> {
    let mut descriptor = Map::new();
    descriptor.insert("method".to_owned(), json!("POST"));
    descriptor.insert("path".to_owned(), json!(route.path));
    if let Some(query) = route.query {
        descriptor.insert("query".to_owned(), query);
    }
    if let Some(auth) = route.auth {
        descriptor.insert("auth".to_owned(), auth);
    }
    descriptor.insert("body".to_owned(), body);
    MediaRequestDescriptorV1::parse(&Value::Object(descriptor).to_string(), &MediaLimitsV1::V1)
        .map_err(|error| internal(format!("built an invalid descriptor: {error}")))
}

fn prepared(
    facts: ImageFactsV1,
    descriptor: MediaRequestDescriptorV1,
    immutable: &[&str],
    forms: &[MeteringFormV1],
) -> ComponentResultV1<PreparedImageCallV1> {
    Ok(PreparedImageCallV1 {
        facts,
        descriptor,
        repeat: 1,
        response_body_form: ResponseBodyFormV1::Json,
        response_elision_paths: paths(&["/data/*/b64_json"])?,
        immutable_body_paths: immutable.iter().map(|path| (*path).to_owned()).collect(),
        state: json!({ "metering_required": forms }).to_string(),
    })
}

/// The first text part named `name`, trimmed, as the native field reader takes it.
fn field<'a>(parts: &'a [Value], name: &str) -> Option<&'a str> {
    parts
        .iter()
        .find(|part| part["kind"] == "text" && part["name"] == name)
        .and_then(|part| part["value"].as_str())
        .map(str::trim)
}

/// The multipart `n`: a decimal `u32` in `1..=10`, 1 when absent (native texts).
fn edit_n(parts: &[Value]) -> ComponentResultV1<u32> {
    let Some(raw) = field(parts, "n") else { return Ok(1) };
    let n = raw.parse::<u32>().map_err(|_| {
        invalid(format!("multipart `n` field value '{raw}' must be a non-negative integer"))
    })?;
    if (1..=MAX_N).contains(&n) {
        Ok(n)
    } else {
        Err(invalid(format!("multipart `n` field value {n} is outside the supported range 1..=10")))
    }
}

/// A count in a `usage` block: a non-negative integer, or an integral float such as `4000.0`
/// (D9); anything else is not a count (D10). `None` when absent or `null`.
fn count(value: &Value) -> Result<Option<u64>, ()> {
    match value {
        Value::Null => Ok(None),
        Value::Number(_) => json_count(value).map(Some).ok_or(()),
        _ => Err(()),
    }
}

/// `ImageTokenUsage::from_openai_usage` as buckets: text and image input from
/// `input_tokens_details`; cached from `input_tokens_details.cached_tokens`, else the top-level
/// `cached_tokens`; output from `output_tokens`, else the sum of `output_tokens_details`' text and
/// image counts; and the top-level `input_tokens` as `total_input` (OCI-Q3, D11).
fn token_usage(usage: &Value) -> Result<ImageTokenUsageV1, ()> {
    let details = &usage["input_tokens_details"];
    let cached = match count(&details["cached_tokens"])? {
        Some(cached) => Some(cached),
        None => count(&usage["cached_tokens"])?,
    };
    let output = if let Some(output) = count(&usage["output_tokens"])? {
        Some(output)
    } else {
        let output_details = &usage["output_tokens_details"];
        match (count(&output_details["text_tokens"])?, count(&output_details["image_tokens"])?) {
            (None, None) => None,
            (text, image) => Some(text.unwrap_or(0).checked_add(image.unwrap_or(0)).ok_or(())?),
        }
    };
    Ok(ImageTokenUsageV1 {
        text_input: count(&details["text_tokens"])?,
        image_input: count(&details["image_tokens"])?,
        cached_input: cached,
        total_input: count(&usage["input_tokens"])?,
        total_output: output,
        ..ImageTokenUsageV1::default()
    })
}

/// The upstream's error body as an envelope: the code follows the status; the message is
/// `error.message`, else `error` (a string), else `message`, else `detail` (U5: the shapes are
/// unverified).
fn error_envelope(status: u16, body: &Value) -> ErrorEnvelope {
    let code = status_code(status);
    let message = [&body["error"]["message"], &body["error"], &body["message"], &body["detail"]]
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

/// The client body the render fills (`image-xai`'s skeleton): the upstream body with each image
/// `null` and every elided string `null`. Over the budget, the revised prompts are dropped first,
/// then everything but `created`, `usage` and one empty object per image.
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

impl OpenAiCompatibleImageReferenceV1 {
    fn prepare_generation(
        body: &Map<String, Value>,
        route: Route,
        upstream_model: &str,
        forms: &[MeteringFormV1],
    ) -> ComponentResultV1<PreparedImageCallV1> {
        let n = generation_n(body)?;
        let mut template = body.clone();
        template.insert("model".to_owned(), json!(upstream_model));
        let template = with_refs(&Value::Object(template));
        let descriptor = descriptor(route, json!({ "json": { "template": template } }))?;
        prepared(
            facts(
                ImageOperationV1::Generate,
                ImageInputsV1::default(),
                n,
                generation_size(body),
                forms.to_vec(),
            ),
            descriptor,
            &GENERATION_IMMUTABLE,
            forms,
        )
    }

    fn prepare_edit(
        parts: &[Value],
        route: Route,
        upstream_model: &str,
        forms: &[MeteringFormV1],
    ) -> ComponentResultV1<PreparedImageCallV1> {
        if parts.iter().any(|part| part["kind"] == "text" && part.get("blob").is_some()) {
            return Err(invalid("a text field is longer than the gateway accepts"));
        }
        let n = edit_n(parts)?;
        let size = field(parts, "size").and_then(size_of);
        if field(parts, "stream")
            .is_some_and(|value| value.eq_ignore_ascii_case("true") || value == "1")
        {
            return Err(invalid(STREAM_REFUSAL));
        }
        let mut inputs = ImageInputsV1::default();
        let mut out: Vec<MediaPartV1> = Vec::with_capacity(parts.len() + 1);
        let mut model_set = false;
        for part in parts {
            let name = part["name"].as_str().ok_or_else(|| internal("a part has no name"))?;
            match name {
                "image" | "image[]" => inputs.input_image = inputs.input_image.saturating_add(1),
                "mask" => inputs.mask = inputs.mask.saturating_add(1),
                _ => {}
            }
            if part["kind"] == "file" {
                out.push(MediaPartV1::File {
                    name: name.to_owned(),
                    blob: serde_json::from_value(part["blob"].clone())
                        .map_err(|_| internal("a file part has no blob"))?,
                    transform: MediaTransformV1::AsIs,
                    filename: part["filename"].as_str().map(str::to_owned),
                    media_type: part["media_type"].as_str().map(str::to_owned),
                });
            } else {
                let value = if name == "model" {
                    model_set = true;
                    upstream_model.to_owned()
                } else {
                    part["value"].as_str().unwrap_or_default().to_owned()
                };
                out.push(MediaPartV1::Text { name: name.to_owned(), value });
            }
        }
        if !model_set {
            out.push(MediaPartV1::Text {
                name: "model".to_owned(),
                value: upstream_model.to_owned(),
            });
        }
        for (role, count) in [("input images", inputs.input_image), ("masks", inputs.mask)] {
            if count > MAX_IMAGE_INPUTS_PER_ROLE {
                return Err(invalid(format!(
                    "an image edit accepts at most {MAX_IMAGE_INPUTS_PER_ROLE} {role} (got {count})"
                )));
            }
        }
        let parts = serde_json::to_value(&out).map_err(|error| internal(error.to_string()))?;
        let descriptor = descriptor(route, json!({ "multipart": { "parts": parts } }))?;
        prepared(
            facts(ImageOperationV1::Edit, inputs, n, size, forms.to_vec()),
            descriptor,
            &EDIT_IMMUTABLE,
            forms,
        )
    }
}

impl ImageComponentV1 for OpenAiCompatibleImageReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: NAME.to_owned(),
            version: VERSION.to_owned(),
            api_version: WORLD.to_owned(),
        }
    }

    /// Every configured model (the fallback serves any id, as `image-vertex` V7 declares the ids
    /// the host passes): `generate` and `edit`, `image` / `image[]` as `input_image` and `mask` as
    /// `mask`, `url` and `b64_json`, `tokens` with the `total_output` bucket and `images`, no tier
    /// dimension, the reference image strings elided.
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
        Family::of(config)?;
        let elision = paths(&REQUEST_ELISION_PATHS)?;
        Ok(config
            .models
            .iter()
            .map(|model| ImageModelCapabilitiesV1 {
                model: model.model.clone(),
                operations: vec![ImageOperationV1::Generate, ImageOperationV1::Edit],
                input_roles: vec![
                    InputRoleKeyV1 { key: "image".to_owned(), role: ImageInputRoleV1::InputImage },
                    InputRoleKeyV1 {
                        key: "image[]".to_owned(),
                        role: ImageInputRoleV1::InputImage,
                    },
                    InputRoleKeyV1 { key: "mask".to_owned(), role: ImageInputRoleV1::Mask },
                ],
                response_formats: vec![ImageResponseFormatV1::Url, ImageResponseFormatV1::B64Json],
                metering_forms: vec![MeteringFormV1::Tokens, MeteringFormV1::Images],
                token_buckets: vec![TokenBucketV1::TotalOutput],
                tier_dimensions: Vec::new(),
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
        let family = Family::of(config)?;
        let upstream_model = context.upstream_model.as_str();
        // The configuration first (the dated family's version and deployment), then the host's
        // metering requirement, then the request: `speech-openai-compatible`'s order.
        let parts = request.pointer("/multipart/parts").and_then(Value::as_array);
        let endpoint = if parts.is_some() { EDITS } else { GENERATIONS };
        let route = route(config, family, endpoint, upstream_model)?;
        let forms = required_forms(context)?;
        if let Some(parts) = parts {
            return Self::prepare_edit(parts, route, upstream_model, &forms);
        }
        match request.get("json") {
            Some(Value::Object(body)) => {
                Self::prepare_generation(body, route, upstream_model, &forms)
            }
            Some(_) => Err(invalid("JSON body must be an object")),
            None => Err(invalid("the request is neither JSON nor multipart")),
        }
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
        let Some(entries) = body["data"].as_array().filter(|entries| !entries.is_empty()) else {
            return Ok(unknown("a 2xx without images"));
        };
        let media_type = match body["output_format"].as_str() {
            Some("jpeg" | "jpg") => "image/jpeg",
            Some("webp") => "image/webp",
            _ => DEFAULT_INLINE_MEDIA_TYPE,
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
                        media_type: media_type.to_owned(),
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
        let tokens_required = state["metering_required"]
            .as_array()
            .is_some_and(|forms| forms.iter().any(|form| form == "tokens"));
        let tokens = match token_usage(&body["usage"]) {
            Ok(tokens) => Some(tokens),
            Err(()) if tokens_required => return Ok(unknown("a malformed usage count")),
            Err(()) => None,
        };
        if tokens_required && tokens.is_none_or(|tokens| tokens.total_output.is_none()) {
            return Ok(unknown("a declared token bucket is missing"));
        }
        Ok(ImageOutcomeV1::Succeeded {
            artifacts,
            metering: ImageMeteringV1 {
                tokens: tokens.filter(|tokens| *tokens != ImageTokenUsageV1::default()),
                images_reported: None,
                credits: None,
                upstream_cost: None,
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
            return Err(internal(
                "an OpenAI-compatible image call renders exactly one succeeded round",
            ));
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
