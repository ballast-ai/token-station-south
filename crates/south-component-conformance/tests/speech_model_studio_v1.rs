//! The Model Studio speech families (design record
//! `docs/design/2026-11-10-speech-bailian-synthesizer-and-minimax.md`): `bailian-speech-synthesizer`
//! in `speech-qwen` 1.3.0 (Qwen-Audio-TTS and `CosyVoice` through the HTTP `SpeechSynthesizer` API) and
//! `bailian-minimax-speech` in `speech-minimax` 1.2.0 (`MiniMax` hosted by Model Studio).
//!
//! Neither family has a native arm, so nothing here is transcribed from a host: the values pinned
//! are the record's, written out. Each package's gate ② pack carries the families' cases beside its
//! first family's; the suites in `qwen_speech_suite.rs` and `minimax_speech_suite.rs` run them.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use south_component_conformance::SpeechComponentV1;
use south_component_conformance::reference_minimax_speech::{
    BAILIAN_MINIMAX_SPEECH, MINIMAX_SPEECH, MiniMaxSpeechReferenceV1,
};
use south_component_conformance::reference_qwen_speech::{
    BAILIAN_QWEN_SPEECH, BAILIAN_SPEECH_SYNTHESIZER, QwenSpeechReferenceV1,
};
use south_contracts::media::MediaBodyV1;
use south_contracts::speech::SpeechCallContextV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

#[path = "support/speech_v3_1.rs"]
mod support;

const ENDPOINT: &str = "https://dashscope.aliyuncs.com";
/// Record §2.1: the character-billed ids on the `CosyVoice` and Qwen-Audio-TTS pages.
const SYNTHESIZER_IDS: [&str; 8] = [
    "cosyvoice-v2",
    "cosyvoice-v3-flash",
    "cosyvoice-v3-plus",
    "cosyvoice-v3.5-flash",
    "cosyvoice-v3.5-plus",
    "qwen-audio-3.0-tts-flash",
    "qwen-audio-3.0-tts-plus",
    "qwen-audio-3.1-tts-plus",
];
/// The `OpenAI` voice names, which name no `SpeechSynthesizer` voice (SV-Q14).
const OPENAI_VOICES: [&str; 11] = [
    "alloy", "echo", "fable", "onyx", "nova", "shimmer", "ash", "sage", "coral", "ballad", "verse",
];

fn raw_manifest(package: &str) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../components")
                .join(package)
                .join("manifest.json"),
        )
        .expect("the manifest reads"),
    )
    .expect("the manifest parses")
}

fn config(family: &str, declared: Option<Value>) -> ProviderConfig {
    let mut config = json!({
        "provider": family,
        "base_url": ENDPOINT,
        "auth": "provider_api_key",
        "models": [{ "model": "cosyvoice-v3.5-plus" }, { "model": "MiniMax/speech-2.8-hd" }],
    });
    if let Some(declared) = declared {
        config["declared"] = declared;
    }
    serde_json::from_value(config).expect("the configuration parses")
}

fn context(model: &str) -> SpeechCallContextV1 {
    serde_json::from_value(json!({
        "upstream_model": model,
        "operation": "synthesize",
        "metering_required": ["characters"],
    }))
    .expect("the context parses")
}

fn synthesize(voice: &Value) -> Result<Value, ErrorCode> {
    QwenSpeechReferenceV1
        .prepare(
            &config(BAILIAN_SPEECH_SYNTHESIZER, None),
            &json!({ "json": { "input": "hello", "voice": voice } }),
            &context("cosyvoice-v3.5-plus"),
        )
        .map(|prepared| {
            let MediaBodyV1::Json(template) = prepared.descriptor.body() else {
                panic!("a JSON body")
            };
            serde_json::from_str(template).expect("the template parses")
        })
        .map_err(|error| error.code)
}

/// Record §1: each package gains one family after its own, the minor that says so, and the served
/// models of §2.1 and §3.1. `qwen-audio-3.1-tts-flash`, which bills tokens, is not served (SV-Q13).
/// The hosted `MiniMax` family declares no configuration key: no `?GroupId=` (§3.2).
#[test]
fn the_model_studio_speech_families_join_their_packages() {
    let qwen = raw_manifest("speech-qwen");
    assert_eq!(qwen["version"], "1.3.0");
    assert_eq!(QwenSpeechReferenceV1.metadata().version, "1.3.0");
    assert_eq!(qwen["providers"], json!([BAILIAN_QWEN_SPEECH, BAILIAN_SPEECH_SYNTHESIZER]));
    assert_eq!(
        qwen["served_models"][BAILIAN_SPEECH_SYNTHESIZER],
        json!({ "ids": SYNTHESIZER_IDS })
    );
    assert_eq!(
        qwen["served_models"][BAILIAN_QWEN_SPEECH],
        json!({ "prefixes": ["qwen-tts", "qwen3-asr-", "qwen3-tts-"] }),
        "the first family's models are unchanged"
    );
    assert!(!SYNTHESIZER_IDS.contains(&"qwen-audio-3.1-tts-flash"));

    let minimax = raw_manifest("speech-minimax");
    assert_eq!(minimax["version"], "1.2.0");
    assert_eq!(MiniMaxSpeechReferenceV1.metadata().version, "1.2.0");
    assert_eq!(minimax["providers"], json!([MINIMAX_SPEECH, BAILIAN_MINIMAX_SPEECH]));
    assert_eq!(
        minimax["served_models"][BAILIAN_MINIMAX_SPEECH],
        json!({ "prefixes": ["MiniMax/speech-"] })
    );
    assert_eq!(minimax["served_models"][MINIMAX_SPEECH], json!({ "prefixes": ["speech-"] }));
    assert_eq!(minimax["config_schema"].get(BAILIAN_MINIMAX_SPEECH), None);
    assert!(minimax["config_schema"][MINIMAX_SPEECH].get("group_id").is_some());

    for package in ["speech-qwen", "speech-minimax"] {
        assert_eq!(support::manifest(package).validate(), Ok(()), "{package}");
    }
}

/// Record §4: each pack carries the new family's cases, and each family's capabilities are
/// `synthesize` billed by characters.
#[test]
fn each_pack_carries_its_new_family() {
    for (package, prefix, family) in [
        ("speech-qwen", "synthesizer-", BAILIAN_SPEECH_SYNTHESIZER),
        ("speech-minimax", "bailian-", BAILIAN_MINIMAX_SPEECH),
    ] {
        let pack = support::pack(&support::manifest(package));
        let families: BTreeSet<&str> = ["capabilities", "prepare", "response"]
            .into_iter()
            .filter(|kind| {
                pack.cases().iter().any(|case| {
                    case.name.starts_with(&format!("speech-v1.{kind}.{prefix}"))
                        && case
                            .input
                            .pointer("/provider_config/provider")
                            .is_none_or(|provider| provider == family)
                })
            })
            .collect();
        assert_eq!(families, BTreeSet::from(["capabilities", "prepare", "response"]), "{package}");
    }
    let qwen = QwenSpeechReferenceV1
        .model_capabilities(&config(BAILIAN_SPEECH_SYNTHESIZER, None))
        .expect("served");
    let minimax = MiniMaxSpeechReferenceV1
        .model_capabilities(&config(BAILIAN_MINIMAX_SPEECH, None))
        .expect("served");
    for models in [qwen, minimax] {
        let models = serde_json::to_value(models).unwrap();
        for model in models.as_array().unwrap() {
            assert_eq!(model["operations"], json!(["synthesize"]));
            assert_eq!(model["metering_forms"], json!(["characters"]));
        }
    }
}

/// SV-Q14: no default voice. Absent, `null`, a non-string, `""` and every `OpenAI` name in any case
/// are refused before admission; any other string is sent verbatim.
#[test]
fn the_synthesizer_family_has_no_default_voice() {
    for voice in [Value::Null, json!(""), json!(3), json!({ "id": "x" })] {
        assert_eq!(synthesize(&voice), Err(ErrorCode::InvalidRequest), "{voice}");
    }
    for name in OPENAI_VOICES {
        for spelled in [name.to_owned(), name.to_uppercase()] {
            assert_eq!(synthesize(&json!(spelled)), Err(ErrorCode::InvalidRequest), "{spelled}");
        }
    }
    for voice in ["longanyang", "Cherry", " ", "alloy2"] {
        assert_eq!(synthesize(&json!(voice)).expect("sent")["input"]["voice"], voice);
    }
}

/// Record §3.2: the hosted family sends no `?GroupId=`, even from a configuration that carries the
/// first family's key, and its body is `MiniMax`'s under `input`, without `stream`.
#[test]
fn the_hosted_minimax_family_sends_no_group_id_and_no_stream() {
    let prepared = MiniMaxSpeechReferenceV1
        .prepare(
            &config(BAILIAN_MINIMAX_SPEECH, Some(json!({ "group_id": "1234567890" }))),
            &json!({ "json": { "input": "hello", "stream": true } }),
            &context("MiniMax/speech-2.8-hd"),
        )
        .expect("prepared");
    let wire: Value = serde_json::from_str(&prepared.to_json()).expect("the call parses");
    assert_eq!(wire["descriptor"]["path"], "api/v1/services/aigc/multimodal-generation/generation");
    assert_eq!(wire["descriptor"].get("query"), None);
    let template = &wire["descriptor"]["body"]["json"]["template"];
    assert_eq!(template.as_object().unwrap().keys().collect::<Vec<_>>(), ["input", "model"]);
    assert_eq!(template["input"].get("stream"), None);
    assert_eq!(template["input"]["output_format"], "hex");
}

/// Each package refuses a configuration naming any other family, naming both of its own.
#[test]
fn each_package_names_both_families_when_refusing_another() {
    let qwen = QwenSpeechReferenceV1
        .model_capabilities(&config(BAILIAN_MINIMAX_SPEECH, None))
        .expect_err("not served");
    assert_eq!(qwen.code, ErrorCode::Capability);
    assert!(qwen.message.contains(BAILIAN_QWEN_SPEECH));
    assert!(qwen.message.contains(BAILIAN_SPEECH_SYNTHESIZER));
    let minimax = MiniMaxSpeechReferenceV1
        .model_capabilities(&config(BAILIAN_SPEECH_SYNTHESIZER, None))
        .expect_err("not served");
    assert_eq!(minimax.code, ErrorCode::Capability);
    assert!(minimax.message.contains(MINIMAX_SPEECH));
    assert!(minimax.message.contains(BAILIAN_MINIMAX_SPEECH));
}
