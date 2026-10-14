//! The strict speech-v1 outcome codec shared by fixtures, ABI and sandbox (speech record §6, §7,
//! §9).
//!
//! One frame crosses the component boundary in composite form here: a round's outcome. The
//! prepared call and the model capabilities have their codecs in `south-contracts`
//! (`parse_prepared_speech_call_v1`, `parse_speech_model_capabilities_v1`). The decoder refuses
//! unknown fields and bounds the frame before parsing.
//!
//! Wire, one of:
//!
//! - `{"outcome": "succeeded", "artifact": <SpeechArtifactV1>, "metering": <SpeechMeteringV1>}`
//!   for synthesized audio;
//! - `{"outcome": "succeeded", "transcript": <TranscriptDeliveryV1>, "metering": …}` for a
//!   recognized transcript, whose `json` template is kept as its source text;
//! - `{"outcome": "rejected", "error": <ErrorEnvelope>}`;
//! - `{"outcome": "charged_failure", "error": …, "metering": …}`;
//! - `{"outcome": "unknown", "reason": "…", "error": <ErrorEnvelope> | null}`.

use serde::Deserialize;
use south_contracts::image::member_source_text;
use south_contracts::media::MediaLimitsV1;
use south_contracts::speech::{
    SpeechArtifactV1, SpeechMeteringV1, TranscriptDeliveryV1, parse_transcript_delivery_v1,
};
use token_station_protocol::ErrorEnvelope;

use crate::{SpeechDeliveryV1, SpeechOutcomeV1};

/// The largest outcome frame: a transcript template's strings are each at most the fallback
/// threshold (1 MiB), and the frame as a whole at most this, the image world's rendered-frame
/// bound.
pub const MAX_SPEECH_OUTCOME_JSON_BYTES: usize = 4 * 1024 * 1024;
/// The longest `unknown` reason.
pub const MAX_SPEECH_UNKNOWN_REASON_BYTES: usize = 1024;

#[derive(Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
enum OutcomeWire {
    Succeeded {
        #[serde(default)]
        artifact: Option<SpeechArtifactV1>,
        #[allow(dead_code)]
        #[serde(default)]
        transcript: Option<serde::de::IgnoredAny>,
        metering: SpeechMeteringV1,
    },
    Rejected {
        error: ErrorEnvelope,
    },
    ChargedFailure {
        error: ErrorEnvelope,
        metering: SpeechMeteringV1,
    },
    Unknown {
        reason: String,
        error: Option<ErrorEnvelope>,
    },
}

/// Decodes one round's outcome.
///
/// # Errors
/// Returns why the frame is refused: too large, not the strict shape, a `succeeded` without
/// exactly one of `artifact` and `transcript`, a transcript the contract refuses, or an oversized
/// reason.
pub fn parse_speech_outcome_json(input: &str) -> Result<SpeechOutcomeV1, String> {
    if input.len() > MAX_SPEECH_OUTCOME_JSON_BYTES {
        return Err("speech outcome JSON exceeds the boundary limit".into());
    }
    let wire: OutcomeWire =
        serde_json::from_str(input).map_err(|_| "invalid speech outcome JSON")?;
    Ok(match wire {
        OutcomeWire::Succeeded { artifact, transcript, metering } => {
            let delivery = match (artifact, transcript.is_some()) {
                (Some(artifact), false) => SpeechDeliveryV1::Audio(artifact),
                (None, true) => {
                    let text = member_source_text(input, "transcript")
                        .ok_or("speech outcome has no transcript")?;
                    SpeechDeliveryV1::Transcript(
                        parse_transcript_delivery_v1(&text, &MediaLimitsV1::V1)
                            .map_err(|error| error.to_string())?,
                    )
                }
                _ => {
                    return Err(
                        "a succeeded speech outcome carries one artifact or one transcript".into(),
                    );
                }
            };
            SpeechOutcomeV1::Succeeded { delivery, metering }
        }
        OutcomeWire::Rejected { error } => SpeechOutcomeV1::Rejected { error },
        OutcomeWire::ChargedFailure { error, metering } => {
            SpeechOutcomeV1::ChargedFailure { error, metering }
        }
        OutcomeWire::Unknown { reason, error } => {
            if reason.len() > MAX_SPEECH_UNKNOWN_REASON_BYTES {
                return Err("speech outcome reason exceeds its bound".into());
            }
            SpeechOutcomeV1::Unknown { reason, error }
        }
    })
}

fn json_text<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}

/// Encodes one round's outcome as compact JSON text, a transcript template as its source text.
///
/// # Errors
/// Returns why the outcome cannot be encoded, or that the frame exceeds the boundary limit.
pub fn speech_outcome_json(outcome: &SpeechOutcomeV1) -> Result<String, String> {
    let text = match outcome {
        SpeechOutcomeV1::Succeeded { delivery, metering } => {
            let delivery = match delivery {
                SpeechDeliveryV1::Audio(artifact) => {
                    format!("\"artifact\":{}", json_text(artifact)?)
                }
                SpeechDeliveryV1::Transcript(transcript) => {
                    format!("\"transcript\":{}", transcript.to_json())
                }
            };
            format!(r#"{{"outcome":"succeeded",{delivery},"metering":{}}}"#, json_text(metering)?)
        }
        SpeechOutcomeV1::Rejected { error } => {
            format!(r#"{{"outcome":"rejected","error":{}}}"#, json_text(error)?)
        }
        SpeechOutcomeV1::ChargedFailure { error, metering } => format!(
            r#"{{"outcome":"charged_failure","error":{},"metering":{}}}"#,
            json_text(error)?,
            json_text(metering)?
        ),
        SpeechOutcomeV1::Unknown { reason, error } => format!(
            r#"{{"outcome":"unknown","reason":{},"error":{}}}"#,
            json_text(reason)?,
            json_text(error)?
        ),
    };
    if text.len() > MAX_SPEECH_OUTCOME_JSON_BYTES {
        return Err("speech outcome JSON exceeds the boundary limit".into());
    }
    Ok(text)
}

/// The transcript delivery's media type as the client receives it, when it names one: `text`
/// and `passthrough` do, a `json` template is `application/json`.
#[must_use]
pub fn transcript_media_type(delivery: &TranscriptDeliveryV1) -> &str {
    match delivery {
        TranscriptDeliveryV1::Json { .. } => "application/json",
        TranscriptDeliveryV1::Text { media_type, .. }
        | TranscriptDeliveryV1::Passthrough { media_type } => media_type,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use token_station_protocol::ErrorCode;

    #[test]
    fn outcomes_round_trip() {
        for text in [
            r#"{"outcome":"succeeded","artifact":{"form":"segments","items":[{"pointer":"/0/data/delta","encoding":"base64"}],"media_type":"audio/mpeg"},"metering":{"characters":null,"seconds":null,"tokens":{"input":3,"output":9,"input_text":null,"input_audio":null}}}"#,
            r#"{"outcome":"succeeded","transcript":{"json":{"text":{"$south.ref":{"blob":"b0","transform":"as_is"}},"b":1.0}},"metering":{"characters":null,"seconds":{"value":2.5,"basis":"reported"},"tokens":null}}"#,
            r#"{"outcome":"succeeded","transcript":{"text":"hi","media_type":"text/plain"},"metering":{"characters":null,"seconds":null,"tokens":null}}"#,
            r#"{"outcome":"unknown","reason":"5xx","error":null}"#,
        ] {
            let outcome = parse_speech_outcome_json(text).expect("valid");
            assert_eq!(speech_outcome_json(&outcome).expect("encodes"), text);
        }
        let rejected = SpeechOutcomeV1::Rejected {
            error: ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, "bad voice".to_owned()),
        };
        let text = speech_outcome_json(&rejected).expect("encodes");
        assert_eq!(parse_speech_outcome_json(&text).expect("re-parses"), rejected);
    }

    #[test]
    fn malformed_outcomes_are_refused() {
        for bad in [
            r#"{"outcome":"succeeded","metering":{}}"#,
            r#"{"outcome":"succeeded","artifact":{"form":"body","media_type":"audio/mpeg"},"transcript":{"text":"x","media_type":"text/plain"},"metering":{}}"#,
            r#"{"outcome":"succeeded","artifact":{"form":"body","media_type":"audio/mpeg"},"metering":{},"x":1}"#,
            r#"{"outcome":"succeeded","transcript":{"json":{"$south.blob":{}}},"metering":{}}"#,
            r#"{"outcome":"succeeded","artifact":{"form":"body","media_type":"audio/mpeg"},"metering":{"seconds":{"value":-1,"basis":"reported"}}}"#,
            r#"{"outcome":"refunded"}"#,
        ] {
            assert!(parse_speech_outcome_json(bad).is_err(), "{bad}");
        }
        let long =
            format!(r#"{{"outcome":"unknown","reason":"{}","error":null}}"#, "x".repeat(1025));
        assert!(parse_speech_outcome_json(&long).is_err());
    }
}
