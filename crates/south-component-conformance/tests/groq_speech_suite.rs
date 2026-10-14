//! `south.speech-component.v1` against the `speech-groq` native reference and its shipped pack, and
//! the behaviours of design record `2026-10-14-speech-v3-1-tts.md` §4 pinned by name.

use serde_json::{Value, json};
use south_component_conformance::reference_groq_speech::GroqSpeechReferenceV1;
use south_component_conformance::{SpeechComponentV1, run_speech_component_suite_v1};
use south_contracts::media::MediaBodyV1;
use south_contracts::speech::SpeechCallContextV1;
use token_station_protocol::ProviderConfig;

#[path = "support/speech_v3_1.rs"]
mod support;

const PACKAGE: &str = "speech-groq";
const REFERENCE: GroqSpeechReferenceV1 = GroqSpeechReferenceV1;

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "groq-speech",
        "base_url": "https://api.groq.com/openai",
        "auth": "provider_api_key",
    }))
    .unwrap()
}

/// The voice `prepare` sends for this client voice and upstream id.
fn voice(voice: &Value, upstream_model: &str) -> Value {
    let context: SpeechCallContextV1 = serde_json::from_value(json!({
        "upstream_model": upstream_model,
        "operation": "synthesize",
        "metering_required": ["characters"],
    }))
    .unwrap();
    let request = json!({ "json": { "input": "hi", "voice": voice } });
    let prepared = REFERENCE.prepare(&config(), &request, &context).unwrap();
    let MediaBodyV1::Json(template) = prepared.descriptor.body() else { panic!("a JSON body") };
    let body: Value = serde_json::from_str(template).unwrap();
    assert_eq!(body["model"], upstream_model);
    body["voice"].clone()
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

/// Record §4: the language set follows the upstream id (case-insensitive `arabic`), and the same
/// `OpenAI` name folds to a persona of that set.
#[test]
fn the_persona_set_follows_the_upstream_id() {
    for (model, name, expected) in [
        ("canopylabs/orpheus-v1-english", "alloy", "autumn"),
        ("canopylabs/orpheus-ARABIC-saudi", "alloy", "noura"),
        ("canopylabs/orpheus-v1-english", "coral", "diana"),
        ("canopylabs/orpheus-arabic-saudi", "coral", "lulwa"),
        ("canopylabs/orpheus-arabic-saudi", "Fahad", "fahad"),
        ("canopylabs/orpheus-v1-english", "fahad", "fahad"),
    ] {
        assert_eq!(voice(&json!(name), model), expected, "{model} {name}");
    }
    assert_eq!(voice(&json!(" "), "canopylabs/orpheus-arabic-saudi"), "noura");
    assert_eq!(voice(&json!(null), "canopylabs/orpheus-v1-english"), "autumn");
}

#[test]
fn every_break_turns_its_row_red() {
    let manifest = support::manifest(PACKAGE);
    support::assert_every_break_bites(&REFERENCE, &support::pack(&manifest), &manifest);
}
