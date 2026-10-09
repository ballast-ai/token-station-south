//! Guest-side image-v1 JSON boundary, using the shared validated codecs (image record §7, §9.2).
//!
//! A guest crate exports the `image-adapter-v1` world by forwarding each export here with its
//! native component, so the guest and the host-side seam agree byte for byte. The prepared call
//! is written with `PreparedImageCallV1::to_json`, the model list as plain JSON, an outcome and
//! the rendered frame through [`crate::image_json`]; `outcomes` arrives as a JSON array of outcome
//! frames. Every error is a JSON `ErrorEnvelope`.

use serde_json::Value;
use south_contracts::image::{ImageCallContextV1, ImageRenderContextV1};
use token_station_protocol::{ErrorCode, ErrorEnvelope};

use crate::{ImageComponentV1, image_json as wire};

fn fail(error: &ErrorEnvelope) -> String {
    serde_json::to_string(error).unwrap_or_else(|_| {
        r#"{"code":"internal","http_status":500,"message":"unserializable error"}"#.to_owned()
    })
}

fn internal(detail: impl std::fmt::Display) -> String {
    fail(&ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string()))
}

fn parse<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, String> {
    serde_json::from_str(raw).map_err(internal)
}

/// `ProviderConfig` -> `list<ImageModelCapabilitiesV1>`.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn model_capabilities_json(
    component: &dyn ImageComponentV1,
    config: &str,
) -> Result<String, String> {
    let models = component.model_capabilities(&parse(config)?).map_err(|error| fail(&error))?;
    serde_json::to_string(&models).map_err(internal)
}

/// `ProviderConfig`, `MediaRequestViewV1`, `ImageCallContextV1` -> `PreparedImageCallV1`.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure; a component refusal is
/// the pre-dispatch refusal the world defines.
pub fn prepare_json(
    component: &dyn ImageComponentV1,
    config: &str,
    request: &str,
    context: &str,
) -> Result<String, String> {
    let request: Value = parse(request)?;
    let context: ImageCallContextV1 = parse(context)?;
    let prepared =
        component.prepare(&parse(config)?, &request, &context).map_err(|error| fail(&error))?;
    Ok(prepared.to_json())
}

/// The prepared state and one round's `MediaResponseViewV1` -> `ImageOutcomeV1`.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn parse_response_json(
    component: &dyn ImageComponentV1,
    state: &str,
    response: &str,
) -> Result<String, String> {
    let state: Value = parse(state)?;
    let response: Value = parse(response)?;
    let outcome = component.parse_response(&state, &response).map_err(|error| fail(&error))?;
    Ok(wire::image_outcome_json(&outcome).map_err(internal)?.to_string())
}

/// The prepared state, the succeeded rounds' outcomes and `ImageRenderContextV1` ->
/// `ImageRenderedV1`.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn render_json(
    component: &dyn ImageComponentV1,
    state: &str,
    outcomes: &str,
    context: &str,
) -> Result<String, String> {
    let state: Value = parse(state)?;
    let outcomes = wire::parse_image_outcomes_json(outcomes).map_err(internal)?;
    let context: ImageRenderContextV1 = parse(context)?;
    let rendered = component.render(&state, &outcomes, &context).map_err(|error| fail(&error))?;
    Ok(wire::image_rendered_json(&rendered))
}
