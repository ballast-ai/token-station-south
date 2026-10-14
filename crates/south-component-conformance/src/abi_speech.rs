//! Guest-side speech-v1 JSON boundary, using the shared validated codecs (speech record §7).
//!
//! A guest crate exports the `speech-adapter-v1` world by forwarding each export here with its
//! native component, so the guest and the host-side seam agree byte for byte. The prepared call is
//! written with `PreparedSpeechCallV1::to_json`, the model list as plain JSON and an outcome
//! through [`crate::speech_json`]. Every error is a JSON `ErrorEnvelope`.

use serde_json::Value;
use south_contracts::speech::SpeechCallContextV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope};

use crate::{SpeechComponentV1, speech_json as wire};

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

/// `ProviderConfig` -> `list<SpeechModelCapabilitiesV1>`.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn model_capabilities_json(
    component: &dyn SpeechComponentV1,
    config: &str,
) -> Result<String, String> {
    let models = component.model_capabilities(&parse(config)?).map_err(|error| fail(&error))?;
    serde_json::to_string(&models).map_err(internal)
}

/// `ProviderConfig`, `MediaRequestViewV1`, `SpeechCallContextV1` -> `PreparedSpeechCallV1`.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure; a component refusal is
/// the pre-dispatch refusal the world defines.
pub fn prepare_json(
    component: &dyn SpeechComponentV1,
    config: &str,
    request: &str,
    context: &str,
) -> Result<String, String> {
    let request: Value = parse(request)?;
    let context: SpeechCallContextV1 = parse(context)?;
    let prepared =
        component.prepare(&parse(config)?, &request, &context).map_err(|error| fail(&error))?;
    Ok(prepared.to_json())
}

/// The prepared state and one round's `MediaResponseViewV1` -> `SpeechOutcomeV1`.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn parse_response_json(
    component: &dyn SpeechComponentV1,
    state: &str,
    response: &str,
) -> Result<String, String> {
    let state: Value = parse(state)?;
    let response: Value = parse(response)?;
    let outcome = component.parse_response(&state, &response).map_err(|error| fail(&error))?;
    wire::speech_outcome_json(&outcome).map_err(internal)
}
