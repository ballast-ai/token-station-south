//! `south.speech-component.v1` against its synthetic native reference and fixture pack (speech
//! record §11, §18 S-S-3).
//!
//! The reference passes every row, and each row the suite requires fails on a component that
//! breaks it (the `mutations` module), so a passing report means each row was checked, not
//! skipped.

use std::path::Path;

use south_component_conformance::reference_synthetic_speech::SyntheticSpeechReferenceV1;
use south_component_conformance::run_speech_component_suite_v1;
use south_component_conformance::speech_fixture::SpeechFixturePackV1;
use south_provider_api::ComponentManifestV1;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("tests/guests/synthetic-speech/manifest.json"))
            .unwrap(),
    )
    .unwrap()
}

fn pack() -> SpeechFixturePackV1 {
    SpeechFixturePackV1::load(&root().join(manifest().conformance.fixtures)).unwrap()
}

#[test]
fn the_manifest_passes_gate_one() {
    assert_eq!(manifest().validate(), Ok(()));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let report = run_speech_component_suite_v1(&SyntheticSpeechReferenceV1, &pack(), &manifest());
    let failures: Vec<String> = report
        .failures()
        .map(|outcome| format!("{} {}: {}", outcome.check.as_str(), outcome.case, outcome.detail()))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every speech row ran at least once on the reference's pack, so a pass is not vacuous.
#[test]
fn every_speech_row_ran() {
    use south_component_conformance::CheckV1;
    let report = run_speech_component_suite_v1(&SyntheticSpeechReferenceV1, &pack(), &manifest());
    for check in [
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
        CheckV1::MissingMeterIsNotZero,
        CheckV1::AbsentDurationIsNullOrUnknown,
        CheckV1::ClientFormatRendering,
        CheckV1::UndeclaredOperationRefused,
        CheckV1::SsmlEscaping,
        CheckV1::SecondHopArtifact,
        CheckV1::EmptyAudioIsUnknown,
    ] {
        assert!(
            report.outcomes().iter().any(|outcome| outcome.check == check),
            "{check} never ran"
        );
    }
}

/// A pack without the cases a row requires fails that row by name: the rows are required, not
/// satisfied by silence.
#[test]
fn a_pack_without_the_required_cases_fails_the_required_rows() {
    use south_component_conformance::CheckV1;
    use south_component_conformance::speech_fixture::SpeechFamilyV1;
    let full = pack();
    let without = |predicate: &dyn Fn(&str) -> bool| {
        SpeechFixturePackV1::from_cases(
            full.cases().iter().filter(|case| !predicate(&case.name)).cloned().collect(),
        )
    };
    for (dropped, check, row) in [
        (
            &(|name: &str| name.contains("ssml-escaping") || name == "speech-v1.response.ssml")
                as &dyn Fn(&str) -> bool,
            CheckV1::SsmlEscaping,
            "speech-v1.ssml.special-characters",
        ),
        (
            &|name: &str| name.ends_with("-missing-duration") || name.ends_with("no-duration"),
            CheckV1::AbsentDurationIsNullOrUnknown,
            "speech-v1.absent.seconds",
        ),
        (
            &|name: &str| name == "speech-v1.response.url",
            CheckV1::SecondHopArtifact,
            "speech-v1.second-hop",
        ),
        (
            &|name: &str| name == "speech-v1.response.sse-no-deltas",
            CheckV1::EmptyAudioIsUnknown,
            "speech-v1.empty-audio.sse",
        ),
        (
            &|name: &str| name == "speech-v1.response.sse-missing-input-tokens",
            CheckV1::MissingMeterIsNotZero,
            "speech-v1.missing.tokens.input",
        ),
        (
            &|name: &str| name == "speech-v1.response.verbose-vtt",
            CheckV1::ClientFormatRendering,
            "speech-v1.client-formats",
        ),
        (
            &|name: &str| {
                name.contains("inline-wav-request-characters")
                    || name.contains("response.stream-binary")
                    || name == "speech-v1.response.ssml"
            },
            CheckV1::MeteringSample,
            "speech-v1.metering.characters.request",
        ),
    ] {
        let report = run_speech_component_suite_v1(
            &SyntheticSpeechReferenceV1,
            &without(dropped),
            &manifest(),
        );
        assert!(
            report.failures().any(|outcome| outcome.check == check && outcome.case == row),
            "{row} did not fail: {report}"
        );
    }
    let only_capabilities = SpeechFixturePackV1::from_cases(
        full.cases()
            .iter()
            .filter(|case| case.family == SpeechFamilyV1::Capabilities)
            .cloned()
            .collect(),
    );
    let report =
        run_speech_component_suite_v1(&SyntheticSpeechReferenceV1, &only_capabilities, &manifest());
    for check in
        [CheckV1::Coverage, CheckV1::UndeclaredOperationRefused, CheckV1::PreDispatchRefusal]
    {
        assert!(report.failures().any(|outcome| outcome.check == check), "{check} did not fail");
    }
}

mod mutations {
    //! Each required row fails on a component that breaks it.

    use super::{manifest, pack};
    use serde_json::Value;
    use south_component_conformance::reference_synthetic_speech::SyntheticSpeechReferenceV1;
    use south_component_conformance::{
        CheckV1, ComponentResultV1, SpeechComponentV1, SpeechDeliveryV1, SpeechOutcomeV1,
        run_speech_component_suite_v1,
    };
    use south_contracts::JsonPointerV1;
    use south_contracts::media::{MediaBodyV1, MediaLimitsV1, MediaRequestDescriptorV1};
    use south_contracts::speech::{
        PreparedSpeechCallV1, SpeechArtifactFormV1, SpeechArtifactV1, SpeechCallContextV1,
        SpeechCharacterMeterV1, SpeechCharacterSourceV1, SpeechMeteringV1,
        SpeechModelCapabilitiesV1, SpeechOperationV1, SpeechSecondsBasisV1, SpeechSecondsMeterV1,
        SpeechTokensV1, TranscriptDeliveryV1,
    };
    use south_provider_api::ComponentMetadataV1;
    use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

    const REFERENCE: SyntheticSpeechReferenceV1 = SyntheticSpeechReferenceV1;

    #[derive(Clone, Copy, Debug)]
    enum Break {
        /// SSML assembled without escaping the input or the voice.
        UnescapedSsml,
        /// Subtitles asked of the upstream in their own format, so they carry no duration.
        SubtitlesWithoutDuration,
        /// A missing duration billed as the host's 60 seconds, by the component.
        FallbackDuration,
        /// Asked to translate, the component transcribes.
        TranscribeForTranslate,
        /// A `url` pointer that lands on no string.
        UrlAtTheWrongPointer,
        /// A 2xx without audio delivered as an empty body.
        SucceedWithoutAudio,
        /// Characters always reported as upstream, even when the upstream gave none.
        AlwaysUpstreamCharacters,
        /// Missing token counts read as zero.
        ZeroForMissingTokens,
        /// `segments` pointers at a member that is not elided.
        UnelidedSegments,
        /// A view placeholder copied into a transcript.
        BlobInTranscript,
        /// A prepare refusal as an internal error.
        InternalRefusal,
        /// A 401 mapped to a retriable error.
        RetriableAuth,
        /// A non-2xx answered as a success.
        SucceedOnError,
    }

    struct Broken(Break);

    fn unescaped(prepared: PreparedSpeechCallV1, request: &Value) -> PreparedSpeechCallV1 {
        let MediaBodyV1::Text(_) = prepared.descriptor.body() else { return prepared };
        let input = request.pointer("/json/input").and_then(Value::as_str).unwrap_or_default();
        let voice = request.pointer("/json/voice").and_then(Value::as_str).unwrap_or_default();
        let ssml = format!(
            "<speak version='1.0' xml:lang='en-US'><voice name='{voice}'>{input}</voice></speak>"
        );
        let mut descriptor: Value = serde_json::from_str(&prepared.descriptor.to_json()).unwrap();
        descriptor["body"]["text"]["text"] = Value::String(ssml);
        PreparedSpeechCallV1 {
            descriptor: MediaRequestDescriptorV1::parse(
                &descriptor.to_string(),
                &MediaLimitsV1::V1,
            )
            .unwrap(),
            ..prepared
        }
    }

    fn with_seconds(outcome: SpeechOutcomeV1, seconds: Option<f64>) -> SpeechOutcomeV1 {
        match outcome {
            SpeechOutcomeV1::Succeeded { delivery, metering } => SpeechOutcomeV1::Succeeded {
                delivery,
                metering: SpeechMeteringV1 {
                    seconds: seconds.map(|value| SpeechSecondsMeterV1 {
                        value,
                        basis: SpeechSecondsBasisV1::Reported,
                    }),
                    ..metering
                },
            },
            other => other,
        }
    }

    fn body_artifact() -> SpeechDeliveryV1 {
        SpeechDeliveryV1::Audio(SpeechArtifactV1 {
            form: SpeechArtifactFormV1::Body,
            media_type: "audio/mpeg".to_owned(),
            container: None,
        })
    }

    impl SpeechComponentV1 for Broken {
        fn metadata(&self) -> ComponentMetadataV1 {
            REFERENCE.metadata()
        }
        fn model_capabilities(
            &self,
            config: &ProviderConfig,
        ) -> ComponentResultV1<Vec<SpeechModelCapabilitiesV1>> {
            REFERENCE.model_capabilities(config)
        }
        fn prepare(
            &self,
            config: &ProviderConfig,
            request: &Value,
            context: &SpeechCallContextV1,
        ) -> ComponentResultV1<PreparedSpeechCallV1> {
            match self.0 {
                Break::TranscribeForTranslate
                    if context.operation == SpeechOperationV1::Translate =>
                {
                    let context = SpeechCallContextV1 {
                        operation: SpeechOperationV1::Transcribe,
                        ..context.clone()
                    };
                    REFERENCE.prepare(config, request, &context)
                }
                Break::UnescapedSsml => REFERENCE
                    .prepare(config, request, context)
                    .map(|prepared| unescaped(prepared, request)),
                Break::InternalRefusal => REFERENCE
                    .prepare(config, request, context)
                    .map_err(|_| ErrorEnvelope::new(ErrorCode::Internal, 500, "refused")),
                _ => REFERENCE.prepare(config, request, context),
            }
        }
        #[allow(clippy::too_many_lines, reason = "one arm per mutation")]
        fn parse_response(
            &self,
            state: &Value,
            response: &Value,
        ) -> ComponentResultV1<SpeechOutcomeV1> {
            let outcome = REFERENCE.parse_response(state, response)?;
            Ok(match (self.0, outcome) {
                (
                    Break::SubtitlesWithoutDuration,
                    outcome @ SpeechOutcomeV1::Succeeded {
                        delivery: SpeechDeliveryV1::Transcript(TranscriptDeliveryV1::Text { .. }),
                        ..
                    },
                ) if state["client_format"] == "srt" => with_seconds(outcome, None),
                (
                    Break::FallbackDuration,
                    outcome @ SpeechOutcomeV1::Succeeded {
                        delivery: SpeechDeliveryV1::Transcript(_),
                        metering: SpeechMeteringV1 { seconds: None, .. },
                    },
                ) => with_seconds(outcome, Some(60.0)),
                (
                    Break::UrlAtTheWrongPointer,
                    SpeechOutcomeV1::Succeeded {
                        delivery: SpeechDeliveryV1::Audio(artifact),
                        metering,
                    },
                ) if matches!(artifact.form, SpeechArtifactFormV1::Url { .. }) => {
                    SpeechOutcomeV1::Succeeded {
                        delivery: SpeechDeliveryV1::Audio(SpeechArtifactV1 {
                            form: SpeechArtifactFormV1::Url {
                                pointer: JsonPointerV1::parse("/usage/characters").unwrap(),
                            },
                            ..artifact
                        }),
                        metering,
                    }
                }
                (Break::SucceedWithoutAudio, SpeechOutcomeV1::Unknown { reason, .. })
                    if reason.contains("without audio") =>
                {
                    SpeechOutcomeV1::Succeeded {
                        delivery: body_artifact(),
                        metering: SpeechMeteringV1 {
                            characters: Some(SpeechCharacterMeterV1 {
                                value: 13,
                                source: SpeechCharacterSourceV1::Request,
                            }),
                            ..SpeechMeteringV1::default()
                        },
                    }
                }
                (
                    Break::AlwaysUpstreamCharacters,
                    SpeechOutcomeV1::Succeeded { delivery, mut metering },
                ) => {
                    if let Some(characters) = &mut metering.characters {
                        characters.source = SpeechCharacterSourceV1::Upstream;
                    }
                    SpeechOutcomeV1::Succeeded { delivery, metering }
                }
                (Break::ZeroForMissingTokens, SpeechOutcomeV1::Unknown { reason, .. })
                    if reason.contains("usage") || reason.contains("token count is missing") =>
                {
                    SpeechOutcomeV1::Succeeded {
                        delivery: body_artifact(),
                        metering: SpeechMeteringV1 {
                            tokens: Some(SpeechTokensV1 {
                                input: Some(0),
                                output: Some(0),
                                ..SpeechTokensV1::default()
                            }),
                            ..SpeechMeteringV1::default()
                        },
                    }
                }
                (
                    Break::UnelidedSegments,
                    SpeechOutcomeV1::Succeeded {
                        delivery: SpeechDeliveryV1::Audio(mut artifact),
                        metering,
                    },
                ) => {
                    if let SpeechArtifactFormV1::Segments { items } = &mut artifact.form {
                        for (index, item) in items.iter_mut().enumerate() {
                            item.pointer =
                                JsonPointerV1::parse(&format!("/{index}/data/index")).unwrap();
                        }
                    }
                    SpeechOutcomeV1::Succeeded {
                        delivery: SpeechDeliveryV1::Audio(artifact),
                        metering,
                    }
                }
                (
                    Break::BlobInTranscript,
                    SpeechOutcomeV1::Succeeded {
                        delivery: SpeechDeliveryV1::Transcript(TranscriptDeliveryV1::Json { .. }),
                        metering,
                    },
                ) => SpeechOutcomeV1::Succeeded {
                    delivery: SpeechDeliveryV1::Transcript(TranscriptDeliveryV1::Json {
                        template: r#"{"text":{"$south.blob":{"id":"b0","bytes":1,"head":"x"}}}"#
                            .to_owned(),
                    }),
                    metering,
                },
                (Break::RetriableAuth, SpeechOutcomeV1::Rejected { mut error })
                    if error.code == ErrorCode::Auth =>
                {
                    error.code = ErrorCode::UpstreamUnavailable;
                    SpeechOutcomeV1::Rejected { error }
                }
                (
                    Break::SucceedOnError,
                    SpeechOutcomeV1::Rejected { .. }
                    | SpeechOutcomeV1::Unknown { error: Some(_), .. },
                ) => SpeechOutcomeV1::Succeeded {
                    delivery: body_artifact(),
                    metering: SpeechMeteringV1 {
                        characters: Some(SpeechCharacterMeterV1 {
                            value: 13,
                            source: SpeechCharacterSourceV1::Request,
                        }),
                        ..SpeechMeteringV1::default()
                    },
                },
                (_, outcome) => outcome,
            })
        }
    }

    fn failing_checks(broken: Break) -> Vec<CheckV1> {
        let report = run_speech_component_suite_v1(&Broken(broken), &pack(), &manifest());
        report.failures().map(|outcome| outcome.check).collect()
    }

    #[test]
    fn each_row_bites() {
        for (broken, check) in [
            (Break::UnescapedSsml, CheckV1::SsmlEscaping),
            (Break::SubtitlesWithoutDuration, CheckV1::ClientFormatRendering),
            (Break::FallbackDuration, CheckV1::AbsentDurationIsNullOrUnknown),
            (Break::TranscribeForTranslate, CheckV1::UndeclaredOperationRefused),
            (Break::UrlAtTheWrongPointer, CheckV1::SecondHopArtifact),
            (Break::SucceedWithoutAudio, CheckV1::EmptyAudioIsUnknown),
            (Break::AlwaysUpstreamCharacters, CheckV1::MeteringSample),
            (Break::ZeroForMissingTokens, CheckV1::MissingMeterIsNotZero),
            (Break::UnelidedSegments, CheckV1::ReferenceIntegrity),
            (Break::BlobInTranscript, CheckV1::ReferenceIntegrity),
            (Break::InternalRefusal, CheckV1::PreDispatchRefusal),
            (Break::RetriableAuth, CheckV1::AuthErrorsAreNotRetriable),
            (Break::SucceedOnError, CheckV1::TerminalOnlyFromTheWire),
        ] {
            let failing = failing_checks(broken);
            assert!(failing.contains(&check), "{broken:?}: {check:?} did not fail: {failing:?}");
        }
    }
}
