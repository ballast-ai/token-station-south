//! Guest-side task-v2 JSON boundary, using the shared validated codecs.

use south_contracts::HostMintedValuesV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope};

use crate::{TaskComponentV2, task_v2_json as wire};

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

fn encode<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(internal)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MintedWire {
    task_id: String,
    #[serde(default)]
    callback_url: Option<String>,
}

/// Prepares a descriptor and the bounded recovery locator.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn build_submit_request_json(
    component: &dyn TaskComponentV2,
    config: &str,
    request: &str,
    minted: &str,
) -> Result<String, String> {
    let minted: MintedWire = parse(minted)?;
    let minted = HostMintedValuesV1::new(&minted.task_id, minted.callback_url.as_deref())
        .map_err(internal)?;
    let prepared = component
        .build_submit_request(&parse(config)?, &parse(request)?, &minted)
        .map_err(|error| fail(&error))?;
    Ok(wire::prepared_task_json(&prepared).map_err(internal)?.to_string())
}

/// Decodes a submit response through the shared outcome codec.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn parse_submit_response_json(
    component: &dyn TaskComponentV2,
    parts: &str,
) -> Result<String, String> {
    let outcome = component.parse_submit_response(&parse(parts)?).map_err(|error| fail(&error))?;
    Ok(wire::submit_outcome_json(&outcome).map_err(internal)?.to_string())
}

/// Builds a poll descriptor from the exact saved locator.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn build_observe_request_json(
    component: &dyn TaskComponentV2,
    config: &str,
    upstream_model: &str,
    upstream_task_id: &str,
    locator: &str,
) -> Result<String, String> {
    encode(
        &component
            .build_observe_request(
                &parse(config)?,
                upstream_model,
                upstream_task_id,
                &wire::parse_locator_json(locator).map_err(internal)?,
            )
            .map_err(|error| fail(&error))?,
    )
}

/// Decodes an observation through the shared validated codec.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn parse_observation_json(
    component: &dyn TaskComponentV2,
    parts: &str,
) -> Result<String, String> {
    let observation = component.parse_observation(&parse(parts)?).map_err(|error| fail(&error))?;
    Ok(wire::observation_json(&observation).map_err(internal)?.to_string())
}

/// Builds an optional artifact request; `null` means no fetch is needed.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn build_artifact_request_json(
    component: &dyn TaskComponentV2,
    config: &str,
    locator: &str,
    observation: &str,
) -> Result<String, String> {
    encode(
        &component
            .build_artifact_request(
                &parse(config)?,
                &wire::parse_locator_json(locator).map_err(internal)?,
                &wire::parse_observation_json(observation).map_err(internal)?,
            )
            .map_err(|error| fail(&error))?,
    )
}

/// Renders success using host-supplied time and identities.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn render_success_json(
    component: &dyn TaskComponentV2,
    observation: &str,
    fetched: Option<&str>,
    context: &str,
) -> Result<String, String> {
    let fetched = fetched.map(parse).transpose()?;
    encode(
        &component
            .render_success(
                &wire::parse_observation_json(observation).map_err(internal)?,
                fetched.as_ref(),
                &wire::parse_render_context_json(context).map_err(internal)?,
            )
            .map_err(|error| fail(&error))?,
    )
}

/// Maps a terminal failure into the closed error catalog.
///
/// # Errors
/// Returns a JSON error envelope for invalid input or component failure.
pub fn map_terminal_failure_json(
    component: &dyn TaskComponentV2,
    observation: &str,
) -> Result<String, String> {
    encode(
        &component
            .map_terminal_failure(&wire::parse_observation_json(observation).map_err(internal)?)
            .map_err(|error| fail(&error))?,
    )
}
