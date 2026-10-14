//! `south.speech-component.v1` against the `speech-azure` native reference and its shipped pack, and
//! the behaviours of design record `2026-10-14-speech-v3-3-asr.md` §5 pinned by name: native's
//! `definition` bytes, the sanctioned `api-version` query on media contract 1, and the subtitle and
//! streaming refusals.

use serde_json::{Value, json};
use south_component_conformance::reference_azure_speech::{AzureSpeechReferenceV1, definition};
use south_component_conformance::{CheckV1, SpeechComponentV1, run_speech_component_suite_v1};
use south_contracts::QueryStringV1;
use south_contracts::media::{MediaBodyV1, MediaPartV1};
use south_contracts::speech::SpeechCallContextV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

#[path = "support/speech_v3_3.rs"]
mod asr;
#[path = "support/speech_v3_1.rs"]
mod support;

const PACKAGE: &str = "speech-azure";
const REFERENCE: AzureSpeechReferenceV1 = AzureSpeechReferenceV1;

/// The rows a transcription-only package runs: the image world's carried-over rows and the
/// recognition rows (`client_format_rendering` judges per case and reports only a failure, since
/// no Azure row carries `speech.verbose_json`; the `TextAsJson` break proves it bites).
const ROWS: [CheckV1; 14] = [
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
    CheckV1::AbsentDurationIsNullOrUnknown,
];

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "azure-speech",
        "base_url": "https://eastus.cognitiveservices.azure.com",
        "auth": "provider_api_key",
        "models": [{ "model": "mai-transcribe-1.5" }],
    }))
    .unwrap()
}

fn context(model: &str) -> SpeechCallContextV1 {
    serde_json::from_value(json!({
        "upstream_model": model,
        "operation": "transcribe",
        "metering_required": ["seconds"],
    }))
    .unwrap()
}

fn request(extra: &[(&str, &str)]) -> Value {
    let mut parts = vec![
        json!({ "kind": "file", "name": "file", "blob": "b0", "filename": "a.wav", "bytes": 9 }),
    ];
    for (name, value) in extra {
        parts.push(json!({ "kind": "text", "name": name, "value": value }));
    }
    json!({ "multipart": { "parts": parts } })
}

#[test]
fn the_manifest_passes_gate_one() {
    assert_eq!(support::manifest(PACKAGE).validate(), Ok(()));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let manifest = support::manifest(PACKAGE);
    let report = run_speech_component_suite_v1(&REFERENCE, &support::pack(&manifest), &manifest);
    let failures = support::failures(&report);
    assert!(failures.is_empty(), "{failures:#?}");
    support::assert_rows_ran(&report, &ROWS);
    for (check, row) in [
        (CheckV1::MeteringSample, "speech-v1.metering.seconds"),
        (CheckV1::AbsentDurationIsNullOrUnknown, "speech-v1.absent.seconds"),
        (CheckV1::UndeclaredOperationRefused, "speech-v1.undeclared-operation"),
    ] {
        assert!(asr::passed_row(&report, check, row), "{row} did not pass");
    }
    // A transcription-only package has no synthesis rows.
    assert!(
        !report.outcomes().iter().any(|outcome| outcome.check == CheckV1::EmptyAudioIsUnknown),
        "{report}"
    );
}

#[test]
fn every_recognition_break_turns_its_row_red() {
    let manifest = support::manifest(PACKAGE);
    asr::assert_every_asr_break_bites(&REFERENCE, &support::pack(&manifest), &manifest);
}

/// Record §5: the `definition` part carries native's bytes (`multipart.rs:315-318`, built with
/// `serde_json`), whatever the upstream id holds; the dual run compares part values as strings.
#[test]
fn the_definition_part_is_natives_bytes() {
    for id in [
        "mai-transcribe-1.5",
        "a\"b",
        "a\\b",
        "line\nbreak\ttab\u{1}",
        "</script>",
        "\u{c548}\u{b155}",
        "",
    ] {
        let native = json!({ "enhancedMode": { "enabled": true, "model": id } }).to_string();
        assert_eq!(definition(id), native, "{id:?}");
        let prepared = REFERENCE.prepare(&config(), &request(&[]), &context(id)).unwrap();
        let MediaBodyV1::Multipart(parts) = prepared.descriptor.body() else { panic!("multipart") };
        let Some(MediaPartV1::Text { name, value }) = parts.last() else { panic!("a text part") };
        assert_eq!((name.as_str(), value.as_str()), ("definition", native.as_str()), "{id:?}");
    }
}

/// Record §5: the `api-version` query is a sanctioned parameter, so the package needs only media
/// contract 1 and declares no query parameter.
#[test]
fn the_api_version_query_is_sanctioned() {
    let prepared =
        REFERENCE.prepare(&config(), &request(&[]), &context("mai-transcribe-1.5")).unwrap();
    assert_eq!(
        prepared.descriptor.query().map(QueryStringV1::as_str),
        Some("api-version=2025-10-15")
    );
    assert_eq!(prepared.descriptor.minimum_contract_version(), 1);
    assert!(support::manifest(PACKAGE).query_parameters.is_empty());
}

/// Record §5 and §1: subtitles are refused before dispatch with native's text whatever the padding,
/// and a streaming request before the format is read.
#[test]
fn subtitles_and_streaming_are_refused_before_dispatch() {
    for format in ["srt", "vtt", " srt", "vtt\t", "\nsrt\n"] {
        let error = REFERENCE
            .prepare(&config(), &request(&[("response_format", format)]), &context("m"))
            .expect_err(format);
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{format:?}");
        assert!(
            error
                .message
                .starts_with(&format!("response_format '{}' is not supported", format.trim())),
            "{format:?}: {}",
            error.message
        );
    }
    for stream in ["true", "TRUE", " true ", "1"] {
        let error = REFERENCE
            .prepare(
                &config(),
                &request(&[("stream", stream), ("response_format", "srt")]),
                &context("m"),
            )
            .expect_err(stream);
        assert!(
            error.message.starts_with("streaming transcription is not supported"),
            "{stream:?}"
        );
    }
    for stream in ["false", "0", "yes"] {
        assert!(
            REFERENCE.prepare(&config(), &request(&[("stream", stream)]), &context("m")).is_ok(),
            "{stream:?} is forwarded"
        );
    }
}
