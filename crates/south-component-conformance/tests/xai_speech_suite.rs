//! `south.speech-component.v1` against the `speech-xai` native reference and its shipped pack, and
//! the behaviours of design record `2026-10-14-speech-v3-1-tts.md` §3 pinned by name.

use serde_json::{Value, json};
use south_component_conformance::reference_xai_speech::XaiSpeechReferenceV1;
use south_component_conformance::{SpeechComponentV1, run_speech_component_suite_v1};
use south_contracts::media::MediaBodyV1;
use south_contracts::speech::SpeechCallContextV1;
use token_station_protocol::ProviderConfig;

#[path = "support/speech_v3_1.rs"]
mod support;

const PACKAGE: &str = "speech-xai";
const REFERENCE: XaiSpeechReferenceV1 = XaiSpeechReferenceV1;

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "xai-speech",
        "base_url": "https://api.x.ai",
        "auth": "provider_api_key",
        "models": [{ "model": "grok-tts" }],
    }))
    .unwrap()
}

fn context() -> SpeechCallContextV1 {
    serde_json::from_value(json!({
        "upstream_model": "grok-tts",
        "operation": "synthesize",
        "metering_required": ["characters"],
    }))
    .unwrap()
}

/// The upstream body `prepare` builds for this client body.
fn upstream_body(request: &Value) -> Value {
    let prepared = REFERENCE.prepare(&config(), &json!({ "json": request }), &context()).unwrap();
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

/// Record §3: a client `output_format` wins over `response_format`, forwarded whatever its JSON
/// type (record §9 item 7, reproduced).
#[test]
fn a_client_output_format_is_forwarded_verbatim() {
    for value in [json!(null), json!("pcm"), json!({ "codec": "mp3", "bit_rate": 64000 })] {
        let body = upstream_body(&json!({
            "input": "hi", "response_format": "flac", "output_format": value.clone(),
        }));
        assert_eq!(body["output_format"], value);
        assert!(body.as_object().unwrap().contains_key("output_format"));
    }
    let body = upstream_body(&json!({ "input": "hi", "response_format": "mp3" }));
    assert!(!body.as_object().unwrap().contains_key("output_format"), "mp3 is xAI's default");
}

/// Record §3: the native voice table, lower-cased match, anything else verbatim.
#[test]
fn voices_follow_the_native_table() {
    for (voice, expected) in [
        (json!("ALLOY"), "eve"),
        (json!("verse"), "sal"),
        (json!("Echo"), "rex"),
        (json!("ara"), "ara"),
        (json!("Custom"), "Custom"),
        (json!(""), ""),
        (json!(null), "eve"),
        (json!(1), "eve"),
    ] {
        let body = upstream_body(&json!({ "input": "hi", "voice": voice }));
        assert_eq!(body["voice_id"], expected, "{voice}");
    }
}

#[test]
fn every_break_turns_its_row_red() {
    let manifest = support::manifest(PACKAGE);
    support::assert_every_break_bites(&REFERENCE, &support::pack(&manifest), &manifest);
}
