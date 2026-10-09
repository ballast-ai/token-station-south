//! The strict image-v1 JSON codec shared by fixtures, ABI and sandbox (image record §7, §9.2).
//!
//! Two frames cross the component boundary in composite form here: a round's outcome and the
//! rendered template. The prepared call and the model capabilities have their codecs in
//! `south-contracts` (`parse_prepared_image_call_v1`, `parse_image_model_capabilities_v1`). Each
//! decoder refuses unknown fields and bounds the frame before parsing.

use serde::Deserialize;
use serde_json::{Value, json};
use south_contracts::image::{ImageArtifactV1, ImageMeteringV1};
use south_contracts::media::MediaLimitsV1;
use token_station_protocol::ErrorEnvelope;

use crate::{ImageOutcomeV1, ImageRenderedV1};

/// The largest outcome frame: ten artifacts, the metering facts, bounded extras and an error.
pub const MAX_IMAGE_OUTCOME_JSON_BYTES: usize = 64 * 1024;
/// The longest `unknown` reason.
pub const MAX_IMAGE_UNKNOWN_REASON_BYTES: usize = 1024;
/// The largest rendered frame: the template is a client body without its image bytes.
pub const MAX_IMAGE_RENDERED_JSON_BYTES: usize = 4 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
enum OutcomeWire {
    Succeeded { artifacts: Vec<ImageArtifactV1>, metering: ImageMeteringV1, extras: Value },
    Rejected { error: ErrorEnvelope },
    ChargedFailure { error: ErrorEnvelope, metering: ImageMeteringV1 },
    Unknown { reason: String, error: Option<ErrorEnvelope> },
}

/// Decodes one round's outcome.
///
/// # Errors
/// Returns why the frame is refused: too large, not the strict shape, an artifact media type
/// outside the grammar, oversized extras, or an oversized reason.
pub fn parse_image_outcome_json(input: &str) -> Result<ImageOutcomeV1, String> {
    if input.len() > MAX_IMAGE_OUTCOME_JSON_BYTES {
        return Err("image outcome JSON exceeds the boundary limit".into());
    }
    let wire: OutcomeWire =
        serde_json::from_str(input).map_err(|_| "invalid image outcome JSON")?;
    Ok(match wire {
        OutcomeWire::Succeeded { artifacts, metering, extras } => {
            for artifact in &artifacts {
                artifact.validate().map_err(|error| error.to_string())?;
            }
            if extras.to_string().len() > MediaLimitsV1::V1.state_bytes {
                return Err("image outcome extras exceed their bound".into());
            }
            ImageOutcomeV1::Succeeded { artifacts, metering, extras }
        }
        OutcomeWire::Rejected { error } => ImageOutcomeV1::Rejected { error },
        OutcomeWire::ChargedFailure { error, metering } => {
            ImageOutcomeV1::ChargedFailure { error, metering }
        }
        OutcomeWire::Unknown { reason, error } => {
            if reason.len() > MAX_IMAGE_UNKNOWN_REASON_BYTES {
                return Err("image outcome reason exceeds its bound".into());
            }
            ImageOutcomeV1::Unknown { reason, error }
        }
    })
}

/// Encodes one round's outcome.
///
/// # Errors
/// Returns why the outcome cannot be encoded.
pub fn image_outcome_json(outcome: &ImageOutcomeV1) -> Result<Value, String> {
    let value = match outcome {
        ImageOutcomeV1::Succeeded { artifacts, metering, extras } => json!({
            "outcome": "succeeded", "artifacts": artifacts, "metering": metering, "extras": extras,
        }),
        ImageOutcomeV1::Rejected { error } => json!({ "outcome": "rejected", "error": error }),
        ImageOutcomeV1::ChargedFailure { error, metering } => {
            json!({ "outcome": "charged_failure", "error": error, "metering": metering })
        }
        ImageOutcomeV1::Unknown { reason, error } => {
            json!({ "outcome": "unknown", "reason": reason, "error": error })
        }
    };
    if value.to_string().len() > MAX_IMAGE_OUTCOME_JSON_BYTES {
        return Err("image outcome JSON exceeds the boundary limit".into());
    }
    Ok(value)
}

/// Decodes the `outcomes` argument of `render`: a JSON array of outcome frames, at most one per
/// round the contract admits (`MediaLimitsV1::V1.repeat`).
///
/// # Errors
/// Returns why the array is refused: not an array, too many frames, or a refused frame.
pub fn parse_image_outcomes_json(input: &str) -> Result<Vec<ImageOutcomeV1>, String> {
    let frames: Vec<Value> =
        serde_json::from_str(input).map_err(|_| "invalid image outcomes JSON")?;
    if frames.len() > usize::from(MediaLimitsV1::V1.repeat) {
        return Err("image outcomes exceed the round bound".into());
    }
    frames.iter().map(|frame| parse_image_outcome_json(&frame.to_string())).collect()
}

/// Encodes the `outcomes` argument of `render`, each frame as [`image_outcome_json`] writes it.
///
/// # Errors
/// Returns why an outcome cannot be encoded, or that there are more than the round bound.
pub fn image_outcomes_json(outcomes: &[ImageOutcomeV1]) -> Result<String, String> {
    if outcomes.len() > usize::from(MediaLimitsV1::V1.repeat) {
        return Err("image outcomes exceed the round bound".into());
    }
    let frames = outcomes.iter().map(image_outcome_json).collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(frames).to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenderedWire {
    #[allow(dead_code)]
    template: serde::de::IgnoredAny,
}

/// Decodes a rendered frame `{"template": …}`, keeping the template's source text.
///
/// # Errors
/// Returns why the frame is refused.
pub fn parse_image_rendered_json(input: &str) -> Result<ImageRenderedV1, String> {
    if input.len() > MAX_IMAGE_RENDERED_JSON_BYTES {
        return Err("image rendered JSON exceeds the boundary limit".into());
    }
    let _: RenderedWire = serde_json::from_str(input).map_err(|_| "invalid image rendered JSON")?;
    let template = south_contracts::image::member_source_text(input, "template")
        .ok_or("image rendered frame has no template")?;
    Ok(ImageRenderedV1 { template })
}

/// Encodes a rendered frame, writing the template as its source text.
#[must_use]
pub fn image_rendered_json(rendered: &ImageRenderedV1) -> String {
    format!("{{\"template\":{}}}", rendered.template)
}

#[cfg(test)]
mod tests {
    use super::*;
    use token_station_protocol::ErrorCode;

    #[test]
    fn outcomes_round_trip() {
        let outcomes = [
            r#"{"outcome":"succeeded","artifacts":[{"form":"inline","pointer":"/data/0/b64_json","encoding":"base64","media_type":"image/png"}],"metering":{"tokens":{"total_input":10,"total_output":20},"images_reported":null,"credits":null,"upstream_cost":null},"extras":null}"#,
            r#"{"outcome":"unknown","reason":"5xx","error":null}"#,
        ];
        for text in outcomes {
            let outcome = parse_image_outcome_json(text).expect("valid");
            let again = image_outcome_json(&outcome).expect("encodes").to_string();
            assert_eq!(parse_image_outcome_json(&again).expect("re-parses"), outcome);
        }
        let rejected = ImageOutcomeV1::Rejected {
            error: ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, "bad prompt".to_owned()),
        };
        let text = image_outcome_json(&rejected).expect("encodes").to_string();
        assert_eq!(parse_image_outcome_json(&text).expect("re-parses"), rejected);
        assert!(
            parse_image_outcome_json(
                r#"{"outcome":"succeeded","artifacts":[],"metering":{},"extras":null,"x":1}"#
            )
            .is_err()
        );
        assert!(parse_image_outcome_json(r#"{"outcome":"refunded"}"#).is_err());
    }

    #[test]
    fn rendered_keeps_the_template_text() {
        let rendered = parse_image_rendered_json(
            r#"{"template":{"b":1.0,"a":[{"$south.artifact":{"index":0,"as":"b64_json"}}]}}"#,
        )
        .expect("valid");
        assert_eq!(
            rendered.template,
            r#"{"b":1.0,"a":[{"$south.artifact":{"index":0,"as":"b64_json"}}]}"#
        );
        assert_eq!(
            parse_image_rendered_json(&image_rendered_json(&rendered)).expect("re-parses"),
            rendered
        );
        assert!(parse_image_rendered_json(r#"{"template":{},"extra":1}"#).is_err());
    }
}
