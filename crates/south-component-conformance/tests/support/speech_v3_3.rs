//! What the suite tests of the recognition packages of speech world batch V3-3 share (design record
//! `docs/design/2026-10-14-speech-v3-3-asr.md` §10): the recognition rows each run must show, and
//! the component mutations every recognition pack must turn red.

#![allow(dead_code, reason = "each test binary mounting this module uses a different subset")]

use serde_json::Value;
use south_component_conformance::speech_fixture::SpeechFixturePackV1;
use south_component_conformance::{
    CheckV1, ComponentResultV1, SpeechComponentV1, SpeechDeliveryV1, SpeechOutcomeV1,
    run_speech_component_suite_v1,
};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechMeteringV1, SpeechModelCapabilitiesV1,
    SpeechOperationV1, SpeechSecondsBasisV1, SpeechSecondsMeterV1, TranscriptDeliveryV1,
};
use south_provider_api::{ComponentManifestV1, ComponentMetadataV1};
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

/// The recognition rows every V3-3 pack runs (record §10): the seconds metering rows, the missing
/// duration, the undeclared operation, and the client format's document kind.
pub const RECOGNITION_ROWS: [CheckV1; 4] = [
    CheckV1::MeteringSample,
    CheckV1::AbsentDurationIsNullOrUnknown,
    CheckV1::UndeclaredOperationRefused,
    CheckV1::ClientFormatRendering,
];

/// Whether the report carries the named pack-wide row and it passed.
pub fn passed_row(
    report: &south_component_conformance::ReportV1,
    check: CheckV1,
    name: &str,
) -> bool {
    report
        .outcomes()
        .iter()
        .any(|outcome| outcome.check == check && outcome.case == name && !outcome.is_failure())
}

/// A way to break a recognition component, applied around its reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsrBreak {
    /// A pre-dispatch refusal reported as an internal error.
    InternalRefusal,
    /// Any recognition operation, and synthesis, prepared as `transcribe`.
    TranscribeEverything,
    /// A 401 or 403 mapped to a retriable error.
    RetriableAuth,
    /// A non-2xx answered as a transcript.
    SucceedOnError,
    /// Every transcript delivered without its seconds.
    DropSeconds,
    /// A text document (`text`, `srt`, `vtt`) delivered as a JSON one.
    TextAsJson,
    /// A rendered text document settled with one more second than the JSON ones.
    SecondsByFormat,
}

/// Every break and the row it must turn red on any V3-3 pack.
pub const EVERY_ASR_BREAK: [(AsrBreak, CheckV1); 6] = [
    (AsrBreak::InternalRefusal, CheckV1::PreDispatchRefusal),
    (AsrBreak::TranscribeEverything, CheckV1::UndeclaredOperationRefused),
    (AsrBreak::RetriableAuth, CheckV1::AuthErrorsAreNotRetriable),
    (AsrBreak::SucceedOnError, CheckV1::TerminalOnlyFromTheWire),
    (AsrBreak::DropSeconds, CheckV1::MeteringSample),
    (AsrBreak::TextAsJson, CheckV1::ClientFormatRendering),
];

/// A reference with one break applied.
pub struct BrokenAsr<'a> {
    pub reference: &'a dyn SpeechComponentV1,
    pub broken: AsrBreak,
}

fn transcript(seconds: Option<f64>) -> SpeechOutcomeV1 {
    SpeechOutcomeV1::Succeeded {
        delivery: SpeechDeliveryV1::Transcript(TranscriptDeliveryV1::Passthrough {
            media_type: "application/json".to_owned(),
        }),
        metering: SpeechMeteringV1 {
            seconds: seconds
                .map(|value| SpeechSecondsMeterV1 { value, basis: SpeechSecondsBasisV1::Reported }),
            ..SpeechMeteringV1::default()
        },
    }
}

impl SpeechComponentV1 for BrokenAsr<'_> {
    fn metadata(&self) -> ComponentMetadataV1 {
        self.reference.metadata()
    }
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<SpeechModelCapabilitiesV1>> {
        self.reference.model_capabilities(config)
    }
    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        match self.broken {
            AsrBreak::InternalRefusal => self
                .reference
                .prepare(config, request, context)
                .map_err(|_| ErrorEnvelope::new(ErrorCode::Internal, 500, "refused")),
            AsrBreak::TranscribeEverything if request.get("multipart").is_some() => {
                let context = SpeechCallContextV1 {
                    operation: SpeechOperationV1::Transcribe,
                    ..context.clone()
                };
                self.reference.prepare(config, request, &context)
            }
            _ => self.reference.prepare(config, request, context),
        }
    }
    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        let outcome = self.reference.parse_response(state, response)?;
        if state.get("op").is_none() {
            return Ok(outcome);
        }
        Ok(match (self.broken, outcome) {
            (AsrBreak::RetriableAuth, SpeechOutcomeV1::Rejected { mut error })
                if error.code == ErrorCode::Auth =>
            {
                error.code = ErrorCode::UpstreamUnavailable;
                SpeechOutcomeV1::Rejected { error }
            }
            (
                AsrBreak::SucceedOnError,
                SpeechOutcomeV1::Rejected { .. } | SpeechOutcomeV1::Unknown { error: Some(_), .. },
            ) => transcript(Some(1.0)),
            (AsrBreak::DropSeconds, SpeechOutcomeV1::Succeeded { delivery, mut metering }) => {
                metering.seconds = None;
                SpeechOutcomeV1::Succeeded { delivery, metering }
            }
            (
                AsrBreak::TextAsJson,
                SpeechOutcomeV1::Succeeded {
                    delivery: SpeechDeliveryV1::Transcript(TranscriptDeliveryV1::Text { .. }),
                    metering,
                },
            ) => SpeechOutcomeV1::Succeeded {
                delivery: SpeechDeliveryV1::Transcript(TranscriptDeliveryV1::Json {
                    template: r#"{"text":"broken"}"#.to_owned(),
                }),
                metering,
            },
            (
                AsrBreak::SecondsByFormat,
                SpeechOutcomeV1::Succeeded {
                    delivery:
                        delivery @ SpeechDeliveryV1::Transcript(TranscriptDeliveryV1::Text { .. }),
                    mut metering,
                },
            ) => {
                if let Some(seconds) = &mut metering.seconds {
                    seconds.value += 1.0;
                }
                SpeechOutcomeV1::Succeeded { delivery, metering }
            }
            (_, outcome) => outcome,
        })
    }
}

/// The rows that fail when `reference` runs `pack` with `broken` applied.
pub fn failing_checks(
    reference: &dyn SpeechComponentV1,
    broken: AsrBreak,
    pack: &SpeechFixturePackV1,
    manifest: &ComponentManifestV1,
) -> Vec<CheckV1> {
    run_speech_component_suite_v1(&BrokenAsr { reference, broken }, pack, manifest)
        .failures()
        .map(|outcome| outcome.check)
        .collect()
}

/// Every break of [`EVERY_ASR_BREAK`] turns its row red on this pack.
pub fn assert_every_asr_break_bites(
    reference: &dyn SpeechComponentV1,
    pack: &SpeechFixturePackV1,
    manifest: &ComponentManifestV1,
) {
    for (broken, check) in EVERY_ASR_BREAK {
        let failing = failing_checks(reference, broken, pack, manifest);
        assert!(failing.contains(&check), "{broken:?}: {check:?} did not fail: {failing:?}");
    }
}
