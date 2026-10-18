//! The shipped `model_parameters`, `drops_unsigned_thinking` and `anthropic` request-header
//! declarations (the family model parameters record,
//! `docs/design/2026-10-18-family-model-parameters.md` §4, §6): each provider manifest declares
//! exactly the words its reference reads, as `declared_parameters::family` lists them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::json;
use south_component_conformance::declared_parameters::family;
use south_component_conformance::{ProviderComponentV1, reference::OpenAiCompatibleReferenceV1};
use south_provider_api::{ComponentManifestV1, ModelParametersV1};
use token_station_protocol::{ChatRequest, ProviderConfig};

fn shipped(package: &str) -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components")
        .join(package)
        .join("manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn declaration(words: &[&str], exclusive: &[&[&str]]) -> ModelParametersV1 {
    let owned = |list: &[&str]| list.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
    ModelParametersV1 {
        words: owned(words),
        exclusive: exclusive.iter().map(|group| owned(group)).collect(),
    }
}

fn every_family(manifest: &ComponentManifestV1, words: &[&str], exclusive: &[&[&str]]) {
    let expected: BTreeMap<String, ModelParametersV1> = manifest
        .providers
        .iter()
        .map(|name| (name.clone(), declaration(words, exclusive)))
        .collect();
    assert_eq!(manifest.model_parameters, expected, "{}", manifest.name);
}

#[test]
fn every_provider_package_declares_the_words_its_reference_reads() {
    let claude_exclusive: &[&[&str]] = &[&family::CLAUDE_EXCLUSIVE];
    every_family(&shipped("provider-anthropic"), &family::CLAUDE, claude_exclusive);
    every_family(&shipped("provider-anthropic-bedrock-invoke"), &family::CLAUDE, claude_exclusive);
    for package in ["provider-bedrock-converse", "provider-bedrock-converse-bearer"] {
        every_family(&shipped(package), &family::BEDROCK_CONVERSE, claude_exclusive);
    }
    every_family(&shipped("provider-openai-responses"), &family::OPENAI_RESPONSES, &[]);
    every_family(&shipped("provider-kiro"), &family::KIRO, &[]);

    let compatible = shipped("provider-openai-compatible");
    for (name, declared) in &compatible.model_parameters {
        let words: &[&str] = if name == "openai-compatible" {
            &family::OPENAI_COMPATIBLE
        } else {
            &family::OPENAI_COMPATIBLE_OTHER
        };
        assert_eq!(declared, &declaration(words, &[]), "{name}");
    }
    assert_eq!(compatible.model_parameters.len(), compatible.providers.len());

    // The Gemini and Vertex text wires read the forced-tool refusal's word alone (lv, 2026-10-18).
    for package in ["provider-gemini", "provider-vertex"] {
        every_family(&shipped(package), &family::GEMINI_TEXT, &[]);
    }
    for package in [
        "provider-anthropic",
        "provider-anthropic-bedrock-invoke",
        "provider-bedrock-converse",
        "provider-bedrock-converse-bearer",
        "provider-openai-compatible",
        "provider-openai-responses",
        "provider-kiro",
        "provider-gemini",
        "provider-vertex",
    ] {
        assert_eq!(shipped(package).validate(), Ok(()), "{package}");
    }
}

/// MP-Q1 and MP-Q3 (ruled 2026-10-18): only `anthropic` drops unsigned thinking, as the host's
/// family-name rule did; widening the set is a separate, measured change.
#[test]
fn only_the_anthropic_family_drops_unsigned_thinking() {
    let anthropic = shipped("provider-anthropic");
    assert_eq!(anthropic.drops_unsigned_thinking, BTreeSet::from(["anthropic".to_owned()]));
    for package in [
        "provider-anthropic-bedrock-invoke",
        "provider-bedrock-converse",
        "provider-bedrock-converse-bearer",
        "provider-openai-compatible",
        "provider-openai-responses",
        "provider-kiro",
        "provider-gemini",
        "provider-vertex",
    ] {
        assert!(shipped(package).drops_unsigned_thinking.is_empty(), "{package}");
    }
}

/// §4: `anthropic` protects the one header a host used to hold by vendor name; the Claude Code
/// families keep their nine.
#[test]
fn the_anthropic_family_protects_anthropic_version() {
    let anthropic = shipped("provider-anthropic");
    assert_eq!(
        anthropic.immutable_request_headers.get("anthropic").map(Vec::as_slice),
        Some(&["anthropic-version".to_owned()][..])
    );
    assert_eq!(anthropic.immutable_request_headers["claude-code"].len(), 9);
    assert_eq!(anthropic.immutable_request_headers["claude-code-setup-token"].len(), 9);
}

/// §2 item 4 and MP-D2: a missing and an empty model capability are not the same input. The
/// OpenAI-compatible component builds the same request for both in general, but on a model whose
/// name starts with `deepseek` a missing entry keeps the `deepseek` name heuristic (`reasoning_content`
/// sent) and an empty one is a declared "no". So a host builds a capability only for a family that
/// declares a host-derived word, as it did before (`openai-compatible` alone, MP-Q3); building
/// one for every declaring family would drop the field on, say, `DeepSeek-R1` behind
/// `azure-openai-v1`.
#[test]
fn a_missing_and_an_empty_model_capability_differ_only_on_a_deepseek_model() {
    let config = |models: serde_json::Value| -> ProviderConfig {
        serde_json::from_value(json!({
            "provider": "azure-openai-v1",
            "base_url": "https://example.test/openai/v1",
            "models": models,
        }))
        .unwrap()
    };
    let built = |model: &str, extra: &serde_json::Value, models: serde_json::Value| {
        let mut value = json!({
            "model": model,
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "t"},
                    {"type": "text", "text": "a"}
                ], "tool_calls": [{"id": "c1", "name": "f", "arguments": "{}"}]},
                {"role": "tool", "tool_call_id": "c1", "content": "r"},
                {"role": "user", "content": "again"}
            ],
            "tools": [{"name": "f", "parameters": {"type": "object"}}],
            "sampling": {"max_output_tokens": 64},
        });
        for (key, field) in extra.as_object().unwrap() {
            value[key] = field.clone();
        }
        let request: ChatRequest = serde_json::from_value(value).unwrap();
        format!("{:?}", OpenAiCompatibleReferenceV1.build_http_request(&request, &config(models)))
    };
    for extra in [
        json!({}),
        json!({"reasoning": {"effort": "high"}}),
        json!({"reasoning": {"mode": "enabled", "budget_tokens": 2048}}),
    ] {
        let empty = json!([{"model": "m", "supported_parameters": []}]);
        assert_eq!(built("m", &extra, json!([])), built("m", &extra, empty), "{extra}");
    }
    let empty = json!([{"model": "DeepSeek-R1", "supported_parameters": []}]);
    let missing = built("DeepSeek-R1", &json!({}), json!([]));
    assert!(missing.contains("reasoning_content"), "{missing}");
    assert_ne!(missing, built("DeepSeek-R1", &json!({}), empty));
}
