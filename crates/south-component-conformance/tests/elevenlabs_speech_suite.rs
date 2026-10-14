//! `south.speech-component.v1` against the `speech-elevenlabs` native reference and its shipped pack,
//! and the behaviours of design record `2026-10-14-speech-v3-1-tts.md` §5, §6 and SV-Q4 pinned by
//! name.

use serde_json::{Value, json};
use south_component_conformance::reference_elevenlabs_speech::{
    ElevenLabsSpeechReferenceV1, declared_query_parameters,
};
use south_component_conformance::{
    CheckV1, ComponentResultV1, DeclaredInstancesV1, SpeechComponentV1, SpeechOutcomeV1,
    run_speech_component_suite_v1,
};
use south_contracts::media::{MediaDescriptorErrorV1, MediaLimitsV1, MediaRequestDescriptorV1};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechContractErrorV1, SpeechModelCapabilitiesV1,
    parse_prepared_speech_call_v1, parse_prepared_speech_call_with_declared_v1,
};
use south_contracts::{DeclaredQueryParameterV1, QueryValueSyntaxV1};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

#[path = "support/speech_v3_1.rs"]
mod support;

const PACKAGE: &str = "speech-elevenlabs";
const REFERENCE: ElevenLabsSpeechReferenceV1 = ElevenLabsSpeechReferenceV1;

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "elevenlabs-speech",
        "base_url": "https://api.elevenlabs.io",
        "auth": "provider_api_key",
    }))
    .unwrap()
}

fn context() -> SpeechCallContextV1 {
    serde_json::from_value(json!({
        "upstream_model": "eleven_multilingual_v2",
        "operation": "synthesize",
        "metering_required": ["characters"],
    }))
    .unwrap()
}

fn prepare(voice: &Value) -> ComponentResultV1<PreparedSpeechCallV1> {
    REFERENCE.prepare(&config(), &json!({ "json": { "input": "hi", "voice": voice } }), &context())
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
    support::assert_rows_ran(&report, &support::APPLICABLE_ROWS);
}

/// Record §6: the reference declares exactly what its manifest declares, so the host's
/// declarations (built from the manifest) and the reference's own are one set.
#[test]
fn the_reference_declares_what_the_manifest_declares() {
    let instances = DeclaredInstancesV1::from_manifest(&support::manifest(PACKAGE)).unwrap();
    assert_eq!(instances.query_parameters(), declared_query_parameters().unwrap().as_slice());
}

/// Record §6: the prepared call carries the declared parameter; a host parses it only with the
/// package's declarations, and the descriptor needs `contracts.media` 3.
#[test]
fn the_prepared_query_needs_the_declaration_and_media_3() {
    let prepared = prepare(&json!("alloy")).unwrap();
    assert_eq!(
        prepared.descriptor.query().map(south_contracts::QueryStringV1::as_str),
        Some("output_format=mp3_44100_128")
    );
    assert_eq!(prepared.descriptor.path().as_str(), "v1/text-to-speech/9BWtsMINqrJLrRacOk9x");
    assert_eq!(prepared.descriptor.minimum_contract_version(), 3);
    let text = prepared.to_json();
    assert_eq!(
        parse_prepared_speech_call_v1(&text, &MediaLimitsV1::V1),
        Err(SpeechContractErrorV1::Descriptor(MediaDescriptorErrorV1::UnsanctionedQuery))
    );
    let declared = declared_query_parameters().unwrap();
    assert_eq!(
        parse_prepared_speech_call_with_declared_v1(&text, &MediaLimitsV1::V1, &declared),
        Ok(prepared)
    );
}

/// SV-Q4: a resolved voice id outside `[A-Za-z0-9._~-]{1,128}`, or a dot segment, is refused before
/// dispatch with `invalid_request`; every admitted id is one path segment.
#[test]
fn voice_ids_outside_the_grammar_are_refused() {
    let (too_long, longest) = ("x".repeat(129), "x".repeat(128));
    for bad in ["a/b", "a?b", "a#b", "a&b", "a%2Fb", "a b", ".", "..", "\u{e9}", too_long.as_str()]
    {
        let error = prepare(&json!(bad)).expect_err(bad);
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{bad}");
        assert!(error.message.contains("voice_id"), "{bad}: {}", error.message);
    }
    for good in ["...", "a.b", "A-z_0~9", longest.as_str(), "pNInz6obpgDQGcFmaJgB"] {
        let prepared = prepare(&json!(good)).expect(good);
        assert_eq!(prepared.descriptor.path().as_str(), format!("v1/text-to-speech/{good}"));
    }
}

/// A component that writes a query name its package does not declare.
struct UndeclaredQueryName;

impl SpeechComponentV1 for UndeclaredQueryName {
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
        let prepared = REFERENCE.prepare(config, request, context)?;
        let mut descriptor: Value = serde_json::from_str(&prepared.descriptor.to_json()).unwrap();
        descriptor["query"][0]["name"] = json!("format");
        let elsewhere = DeclaredQueryParameterV1::try_new("format", QueryValueSyntaxV1::Token)
            .expect("a valid declaration");
        Ok(PreparedSpeechCallV1 {
            descriptor: MediaRequestDescriptorV1::parse_with_declared(
                &descriptor.to_string(),
                &MediaLimitsV1::V1,
                &[elsewhere],
            )
            .unwrap(),
            ..prepared
        })
    }
    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        REFERENCE.parse_response(state, response)
    }
}

/// Record §6 conformance: a mutation that writes an undeclared name turns the pack red, on the row
/// that judges what a host admits.
#[test]
fn an_undeclared_query_name_turns_the_pack_red() {
    let manifest = support::manifest(PACKAGE);
    let report =
        run_speech_component_suite_v1(&UndeclaredQueryName, &support::pack(&manifest), &manifest);
    assert!(
        report.failures().any(|outcome| outcome.check == CheckV1::EndpointConfinement),
        "{report}"
    );
}

#[test]
fn every_break_turns_its_row_red() {
    let manifest = support::manifest(PACKAGE);
    support::assert_every_break_bites(&REFERENCE, &support::pack(&manifest), &manifest);
}
