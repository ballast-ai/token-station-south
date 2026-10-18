//! `model_parameters` and `drops_unsigned_thinking`, the two provider-world manifest fields of the
//! family model parameters record (`docs/design/2026-10-18-family-model-parameters.md` §2, §3):
//! gate ① admits them on the provider world only, for declared families only, with a closed word
//! grammar and exclusive groups drawn from the family's own words.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::json;
use south_provider_api::{
    ComponentManifestV1, MAX_MODEL_PARAMETER_BYTES, MAX_MODEL_PARAMETER_GROUPS,
    MAX_MODEL_PARAMETERS, ManifestErrorV1, ModelParametersV1,
};

fn shipped(package: &str) -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components")
        .join(package)
        .join("manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn gemini() -> ComponentManifestV1 {
    shipped("provider-gemini")
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

fn declared(list: &[&str], exclusive: &[&[&str]]) -> ModelParametersV1 {
    ModelParametersV1 {
        words: words(list),
        exclusive: exclusive.iter().map(|group| words(group)).collect(),
    }
}

fn with(parameters: ModelParametersV1) -> ComponentManifestV1 {
    let mut manifest = gemini();
    manifest.model_parameters = BTreeMap::from([("gemini".to_owned(), parameters)]);
    manifest
}

fn refused(parameters: ModelParametersV1) -> bool {
    matches!(
        with(parameters).validate(),
        Err(ManifestErrorV1::InvalidModelParameters { family, .. }) if family == "gemini"
    )
}

#[test]
fn absent_declarations_are_omitted_and_change_nothing() {
    let manifest = gemini();
    assert!(manifest.model_parameters.is_empty());
    assert!(manifest.drops_unsigned_thinking.is_empty());
    let wire = serde_json::to_value(&manifest).unwrap();
    assert!(wire.get("model_parameters").is_none());
    assert!(wire.get("drops_unsigned_thinking").is_none());
    assert_eq!(manifest.validate(), Ok(()));
}

#[test]
fn a_declared_family_names_its_words_and_exclusive_groups() {
    let mut manifest = with(declared(
        &["example.mode.fast", "example.mode.slow", "reasoning_effort"],
        &[&["example.mode.fast", "example.mode.slow"]],
    ));
    manifest.drops_unsigned_thinking = BTreeSet::from(["gemini".to_owned()]);
    assert_eq!(manifest.validate(), Ok(()));

    let wire = serde_json::to_value(&manifest).unwrap();
    assert_eq!(
        wire["model_parameters"],
        json!({"gemini": {
            "words": ["example.mode.fast", "example.mode.slow", "reasoning_effort"],
            "exclusive": [["example.mode.fast", "example.mode.slow"]]
        }})
    );
    assert_eq!(wire["drops_unsigned_thinking"], json!(["gemini"]));
    let back: ComponentManifestV1 = serde_json::from_value(wire).unwrap();
    assert_eq!(back, manifest);

    // `exclusive` is optional and omitted when empty; an empty word list says nothing.
    let plain = with(declared(&["reasoning_effort"], &[]));
    assert_eq!(plain.validate(), Ok(()));
    let wire = serde_json::to_value(&plain).unwrap();
    assert_eq!(wire["model_parameters"], json!({"gemini": {"words": ["reasoning_effort"]}}));
    assert_eq!(with(declared(&[], &[])).validate(), Ok(()));
}

#[test]
fn unknown_keys_inside_a_declaration_are_refused() {
    let mut wire = serde_json::to_value(gemini()).unwrap();
    wire["model_parameters"] = json!({"gemini": {"words": [], "exclusiv": []}});
    assert!(serde_json::from_value::<ComponentManifestV1>(wire).is_err());
}

#[test]
fn an_undeclared_family_is_refused_by_name() {
    let mut manifest = gemini();
    manifest.model_parameters =
        BTreeMap::from([("nowhere".to_owned(), declared(&["reasoning_effort"], &[]))]);
    assert!(matches!(
        manifest.validate(),
        Err(ManifestErrorV1::InvalidModelParameters { family, detail })
            if family == "nowhere" && detail.contains("does not declare")
    ));

    let mut manifest = gemini();
    manifest.drops_unsigned_thinking = BTreeSet::from(["nowhere".to_owned()]);
    assert_eq!(
        manifest.validate(),
        Err(ManifestErrorV1::InvalidDropsUnsignedThinkingFamily("nowhere".to_owned()))
    );
}

#[test]
fn words_are_sorted_distinct_lowercase_dotted_segments() {
    let at_limit = "a".repeat(MAX_MODEL_PARAMETER_BYTES);
    let long = "a".repeat(MAX_MODEL_PARAMETER_BYTES + 1);
    for good in [
        &["reasoning_content"][..],
        &["anthropic.thinking.adaptive", "anthropic.thinking.budget", "reasoning_replay.claude.v1"],
        &["bedrock.converse.cache_point", "bedrock.converse.cache_point.ttl_1h"],
        &[at_limit.as_str()],
    ] {
        assert_eq!(with(declared(good, &[])).validate(), Ok(()), "{good:?}");
    }
    for bad in [
        &[""][..],
        &["Reasoning_effort"],
        &["reasoning-effort"],
        &["a..b"],
        &[".a"],
        &["a."],
        &["a b"],
        &["b", "a"],
        &["a", "a"],
        &[long.as_str()],
    ] {
        assert!(refused(declared(bad, &[])), "{bad:?}");
    }
    let many: Vec<String> = (0..=MAX_MODEL_PARAMETERS).map(|n| format!("w{n:03}")).collect();
    let fits =
        ModelParametersV1 { words: many[..MAX_MODEL_PARAMETERS].to_vec(), exclusive: vec![] };
    assert_eq!(with(fits).validate(), Ok(()));
    assert!(refused(ModelParametersV1 { words: many, exclusive: vec![] }));
}

#[test]
fn exclusive_groups_are_sorted_disjoint_groups_of_declared_words() {
    let all = ["a", "b", "c", "d"];
    assert_eq!(with(declared(&all, &[&["a", "b"], &["c", "d"]])).validate(), Ok(()));
    assert_eq!(with(declared(&all, &[&["a", "b", "c"]])).validate(), Ok(()));
    for bad in [
        &[&["a"][..]][..],
        &[&[]],
        &[&["b", "a"]],
        &[&["a", "a"]],
        &[&["a", "e"]],
        &[&["c", "d"], &["a", "b"]],
        &[&["a", "b"], &["b", "c"]],
    ] {
        assert!(refused(declared(&all, bad)), "{bad:?}");
    }
    let many: Vec<String> =
        (0..2 * (MAX_MODEL_PARAMETER_GROUPS + 1)).map(|n| format!("w{n:03}")).collect();
    let groups: Vec<Vec<String>> = many.chunks(2).map(<[String]>::to_vec).collect();
    let fits = ModelParametersV1 {
        words: many.clone(),
        exclusive: groups[..MAX_MODEL_PARAMETER_GROUPS].to_vec(),
    };
    assert_eq!(with(fits).validate(), Ok(()));
    assert!(refused(ModelParametersV1 { words: many, exclusive: groups }));
}

#[test]
fn other_worlds_refuse_both_declarations() {
    for package in ["embeddings-openai-compatible", "task-kling-v2", "image-azure", "speech-groq"] {
        let mut manifest = shipped(package);
        let family = manifest.providers[0].clone();
        manifest.model_parameters =
            BTreeMap::from([(family.clone(), declared(&["reasoning_effort"], &[]))]);
        assert_eq!(
            manifest.validate(),
            Err(ManifestErrorV1::DeliveryIsAProviderWorldDeclaration),
            "{package}"
        );
        let mut manifest = shipped(package);
        manifest.drops_unsigned_thinking = BTreeSet::from([family]);
        assert_eq!(
            manifest.validate(),
            Err(ManifestErrorV1::DeliveryIsAProviderWorldDeclaration),
            "{package}"
        );
    }
}
