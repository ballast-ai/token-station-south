//! What the suite tests of the four text-to-speech packages of speech world batch V3-1 share
//! (design record `docs/design/2026-10-14-speech-v3-1-tts.md` §12): the shipped manifest and pack,
//! the rows each run must show, and the component mutations every pack must turn red.

#![allow(dead_code, reason = "each test binary mounting this module uses a different subset")]

use std::path::Path;

use serde_json::Value;
use south_component_conformance::speech_fixture::SpeechFixturePackV1;
use south_component_conformance::{
    CheckV1, ComponentResultV1, ReportV1, SpeechComponentV1, SpeechDeliveryV1, SpeechOutcomeV1,
    run_speech_component_suite_v1,
};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechArtifactFormV1, SpeechArtifactV1, SpeechCallContextV1,
    SpeechCharacterMeterV1, SpeechCharacterSourceV1, SpeechMeteringV1, SpeechModelCapabilitiesV1,
    SpeechOperationV1,
};
use south_provider_api::{ComponentManifestV1, ComponentMetadataV1};
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

/// The shipped manifest of `components/<package>`.
pub fn manifest(package: &str) -> ComponentManifestV1 {
    serde_json::from_str(
        &std::fs::read_to_string(
            repo_root().join("components").join(package).join("manifest.json"),
        )
        .expect("the shipped manifest reads"),
    )
    .expect("the shipped manifest parses")
}

/// The pack the manifest names.
pub fn pack(manifest: &ComponentManifestV1) -> SpeechFixturePackV1 {
    SpeechFixturePackV1::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(&manifest.conformance.fixtures),
    )
    .expect("the shipped pack loads")
}

/// Every failed row, one line each.
pub fn failures(report: &ReportV1) -> Vec<String> {
    report
        .failures()
        .map(|outcome| format!("{} {}: {}", outcome.check.as_str(), outcome.case, outcome.detail()))
        .collect()
}

/// The rows every V3-1 pack runs: the image world's carried-over rows and the speech rows that
/// apply to a synthesis-only package (record §12; `ssml_escaping`, `second_hop_artifact`,
/// `client_format_rendering` and `absent_duration_is_null_or_unknown` apply to no V3-1 package).
pub const APPLICABLE_ROWS: [CheckV1; 14] = [
    CheckV1::Coverage,
    CheckV1::FixtureMatch,
    CheckV1::Determinism,
    CheckV1::UnknownFieldTolerance,
    CheckV1::UndeclaredValuesIgnored,
    CheckV1::DescriptorAuthWithinManifest,
    CheckV1::EndpointConfinement,
    CheckV1::TerminalOnlyFromTheWire,
    CheckV1::AuthErrorsAreNotRetriable,
    CheckV1::PreDispatchRefusal,
    CheckV1::ReferenceIntegrity,
    CheckV1::MeteringSample,
    CheckV1::UndeclaredOperationRefused,
    CheckV1::EmptyAudioIsUnknown,
];

/// Asserts each row ran at least once, so a pass is not vacuous.
pub fn assert_rows_ran(report: &ReportV1, rows: &[CheckV1]) {
    for check in rows {
        assert!(
            report.outcomes().iter().any(|outcome| outcome.check == *check),
            "{check} never ran"
        );
    }
}

/// A way to break a component, applied around its reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Break {
    /// A pre-dispatch refusal reported as an internal error.
    InternalRefusal,
    /// Any operation prepared as `synthesize`.
    IgnoreOperation,
    /// A 2xx without audio delivered as an empty body.
    SucceedWithoutAudio,
    /// A 401 or 403 mapped to a retriable error.
    RetriableAuth,
    /// A non-2xx answered as a success.
    SucceedOnError,
    /// Characters reported as the upstream's.
    UpstreamCharacters,
}

/// Every break and the row it must turn red on any V3-1 pack.
pub const EVERY_BREAK: [(Break, CheckV1); 6] = [
    (Break::InternalRefusal, CheckV1::PreDispatchRefusal),
    (Break::IgnoreOperation, CheckV1::UndeclaredOperationRefused),
    (Break::SucceedWithoutAudio, CheckV1::EmptyAudioIsUnknown),
    (Break::RetriableAuth, CheckV1::AuthErrorsAreNotRetriable),
    (Break::SucceedOnError, CheckV1::TerminalOnlyFromTheWire),
    (Break::UpstreamCharacters, CheckV1::MeteringSample),
];

/// A reference with one break applied.
pub struct Broken<'a> {
    pub reference: &'a dyn SpeechComponentV1,
    pub broken: Break,
}

fn body_audio(characters: u64) -> SpeechOutcomeV1 {
    SpeechOutcomeV1::Succeeded {
        delivery: SpeechDeliveryV1::Audio(SpeechArtifactV1 {
            form: SpeechArtifactFormV1::Body,
            media_type: "audio/mpeg".to_owned(),
            container: None,
        }),
        metering: SpeechMeteringV1 {
            characters: Some(SpeechCharacterMeterV1 {
                value: characters,
                source: SpeechCharacterSourceV1::Request,
            }),
            ..SpeechMeteringV1::default()
        },
    }
}

impl SpeechComponentV1 for Broken<'_> {
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
            Break::InternalRefusal => self
                .reference
                .prepare(config, request, context)
                .map_err(|_| ErrorEnvelope::new(ErrorCode::Internal, 500, "refused")),
            Break::IgnoreOperation => {
                let context = SpeechCallContextV1 {
                    operation: SpeechOperationV1::Synthesize,
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
        let characters = state["characters"].as_u64().unwrap_or(1);
        Ok(match (self.broken, outcome) {
            (Break::SucceedWithoutAudio, SpeechOutcomeV1::Unknown { reason, .. })
                if reason.contains("without audio") =>
            {
                body_audio(characters)
            }
            (Break::RetriableAuth, SpeechOutcomeV1::Rejected { mut error })
                if error.code == ErrorCode::Auth =>
            {
                error.code = ErrorCode::UpstreamUnavailable;
                SpeechOutcomeV1::Rejected { error }
            }
            (
                Break::SucceedOnError,
                SpeechOutcomeV1::Rejected { .. } | SpeechOutcomeV1::Unknown { error: Some(_), .. },
            ) => body_audio(characters),
            (Break::UpstreamCharacters, SpeechOutcomeV1::Succeeded { delivery, mut metering }) => {
                if let Some(characters) = &mut metering.characters {
                    characters.source = SpeechCharacterSourceV1::Upstream;
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
    broken: Break,
    pack: &SpeechFixturePackV1,
    manifest: &ComponentManifestV1,
) -> Vec<CheckV1> {
    run_speech_component_suite_v1(&Broken { reference, broken }, pack, manifest)
        .failures()
        .map(|outcome| outcome.check)
        .collect()
}

/// Every break of [`EVERY_BREAK`] turns its row red on this pack.
pub fn assert_every_break_bites(
    reference: &dyn SpeechComponentV1,
    pack: &SpeechFixturePackV1,
    manifest: &ComponentManifestV1,
) {
    for (broken, check) in EVERY_BREAK {
        let failing = failing_checks(reference, broken, pack, manifest);
        assert!(failing.contains(&check), "{broken:?}: {check:?} did not fail: {failing:?}");
    }
}
