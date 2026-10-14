//! `south.speech-component.v1` against the `speech-mimo` native reference and its shipped pack, and
//! the behaviours of design record `2026-10-14-speech-v3-2-tts.md` §2 pinned by name.

use serde_json::{Value, json};
use south_component_conformance::reference_mimo_speech::MimoSpeechReferenceV1;
use south_component_conformance::{
    SpeechComponentV1, SpeechDeliveryV1, SpeechOutcomeV1, run_speech_component_suite_v1,
};
use south_contracts::media::MediaBodyV1;
use south_contracts::speech::{SpeechArtifactFormV1, SpeechCallContextV1, SpeechEncodingV1};
use token_station_protocol::ProviderConfig;

#[path = "support/speech_v3_1.rs"]
mod support;

const PACKAGE: &str = "speech-mimo";
const REFERENCE: MimoSpeechReferenceV1 = MimoSpeechReferenceV1;
const PRESET: &str = "mimo-v2.5-tts";
const CLONE: &str = "MiMo-V2.5-TTS-VoiceClone";
const DESIGN: &str = "MiMo-V2.5-TTS-VoiceDesign";

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "mimo-speech",
        "base_url": "https://api.xiaomimimo.com",
        "auth": "provider_api_key",
        "models": [{ "model": PRESET }, { "model": CLONE }, { "model": DESIGN }],
    }))
    .unwrap()
}

fn context(model: &str) -> SpeechCallContextV1 {
    serde_json::from_value(json!({
        "upstream_model": model,
        "operation": "synthesize",
        "metering_required": ["characters"],
    }))
    .unwrap()
}

/// The upstream body `prepare` builds for this client body on `model`.
fn upstream_body(model: &str, request: &Value) -> Value {
    let prepared =
        REFERENCE.prepare(&config(), &json!({ "json": request }), &context(model)).unwrap();
    let MediaBodyV1::Json(template) = prepared.descriptor.body() else { panic!("a JSON body") };
    serde_json::from_str(template).unwrap()
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

/// Record §2 (P9): the model class is read from the upstream id, lower-cased, so an id in another
/// case keeps its class: a clone voice that collides with an `OpenAI` name stays verbatim and a
/// design model carries no `audio.voice`.
#[test]
fn model_classes_are_read_from_the_upstream_id_in_any_case() {
    let body = upstream_body(CLONE, &json!({ "input": "hi", "voice": "Nova" }));
    assert_eq!(body["audio"], json!({ "format": "wav", "voice": "Nova" }));
    let body = upstream_body(PRESET, &json!({ "input": "hi", "voice": "Nova" }));
    assert_eq!(body["audio"], json!({ "format": "wav", "voice": "mimo_default" }));
    let body = upstream_body(DESIGN, &json!({ "input": "hi", "voice": "Nova" }));
    assert_eq!(body["audio"], json!({ "format": "wav" }));
    assert_eq!(body["messages"][0], json!({ "role": "user", "content": "Nova" }));
}

/// Record §2 and decision 3: a design model without a description is dispatched as native does,
/// with the assistant turn only.
#[test]
fn a_design_model_without_a_description_is_dispatched() {
    let body = upstream_body(DESIGN, &json!({ "input": "hi", "instructions": " " }));
    assert_eq!(body["messages"], json!([{ "role": "assistant", "content": "hi" }]));
}

/// Record §2: the audio is the first choice's base64 string, typed by the requested format; later
/// choices and the chat usage take no part.
#[test]
fn the_first_choice_is_a_base64_inline_artifact() {
    let prepared = REFERENCE
        .prepare(
            &config(),
            &json!({ "json": { "input": "hi", "response_format": "pcm16" } }),
            &context(PRESET),
        )
        .unwrap();
    let state: Value = serde_json::from_str(&prepared.state).unwrap();
    let view = json!({
        "status": 200,
        "headers": [],
        "body": { "json": { "choices": [
            { "message": { "audio": { "data": { "$south.blob": { "id": "b0", "bytes": 4, "head": "AAAA" } } } } },
            { "message": { "audio": { "data": { "$south.blob": { "id": "b1", "bytes": 4, "head": "BBBB" } } } } },
        ], "usage": { "prompt_tokens": 0, "completion_tokens": 0 } } },
    });
    let SpeechOutcomeV1::Succeeded { delivery: SpeechDeliveryV1::Audio(artifact), metering } =
        REFERENCE.parse_response(&state, &view).unwrap()
    else {
        panic!("a delivery")
    };
    let SpeechArtifactFormV1::Inline { pointer, encoding } = artifact.form else {
        panic!("an inline artifact")
    };
    assert_eq!(pointer.as_str(), "/choices/0/message/audio/data");
    assert_eq!(encoding, SpeechEncodingV1::Base64);
    assert_eq!(artifact.media_type, "audio/pcm");
    assert_eq!(metering.characters.map(|characters| characters.value), Some(2));
}

#[test]
fn every_break_turns_its_row_red() {
    let manifest = support::manifest(PACKAGE);
    support::assert_every_break_bites(&REFERENCE, &support::pack(&manifest), &manifest);
}
