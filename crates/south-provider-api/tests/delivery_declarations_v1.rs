//! `immutable_body_paths` and `north_passthrough`, the two manifest fields the `OpenAI` Responses
//! upstream record proposes (`docs/design/2026-09-30-openai-responses-upstream-component.md` §3.4,
//! D7, D11): gate ① admits them on the provider world only, for declared families only, with the
//! task world's immutable-path grammar and a closed protocol set.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;
use south_provider_api::{
    ComponentManifestV1, MAX_IMMUTABLE_BODY_PATH_BYTES, MAX_IMMUTABLE_BODY_PATHS, ManifestErrorV1,
    NorthProtocolV1, are_immutable_body_paths,
};

fn shipped(package: &str) -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components")
        .join(package)
        .join("manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn openai() -> ComponentManifestV1 {
    shipped("provider-openai-compatible")
}

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|path| (*path).to_owned()).collect()
}

#[test]
fn absent_declarations_are_omitted_and_change_nothing() {
    let manifest = openai();
    assert!(manifest.immutable_body_paths.is_empty());
    assert!(manifest.north_passthrough.is_empty());
    let wire = serde_json::to_value(&manifest).unwrap();
    assert!(wire.get("immutable_body_paths").is_none());
    assert!(wire.get("north_passthrough").is_none());
    assert_eq!(manifest.validate(), Ok(()));
}

#[test]
fn a_declared_family_may_fix_paths_and_name_its_northbound_protocol() {
    let mut manifest = openai();
    manifest.immutable_body_paths =
        BTreeMap::from([("openai-compatible".to_owned(), paths(&["store", "text.format"]))]);
    manifest.north_passthrough =
        BTreeMap::from([("openai-compatible".to_owned(), NorthProtocolV1::ChatCompletions)]);
    assert_eq!(manifest.validate(), Ok(()));

    let wire = serde_json::to_value(&manifest).unwrap();
    assert_eq!(
        wire["immutable_body_paths"],
        json!({"openai-compatible": ["store", "text.format"]})
    );
    assert_eq!(wire["north_passthrough"], json!({"openai-compatible": "chat_completions"}));
    let back: ComponentManifestV1 = serde_json::from_value(wire).unwrap();
    assert_eq!(back, manifest);
}

#[test]
fn the_protocol_set_is_closed_and_spelled_apart_from_family_names() {
    for (word, protocol) in [
        ("chat_completions", NorthProtocolV1::ChatCompletions),
        ("responses", NorthProtocolV1::Responses),
        ("messages", NorthProtocolV1::Messages),
    ] {
        assert_eq!(serde_json::from_value::<NorthProtocolV1>(json!(word)).unwrap(), protocol);
    }
    for word in ["openai-responses", "Responses", "chat-completions", "anthropic", ""] {
        assert!(serde_json::from_value::<NorthProtocolV1>(json!(word)).is_err(), "{word}");
    }
    let mut wire = serde_json::to_value(openai()).unwrap();
    wire["north_passthrough"] = json!({"openai-compatible": "openai-responses"});
    assert!(serde_json::from_value::<ComponentManifestV1>(wire).is_err());
}

#[test]
fn an_undeclared_family_is_refused_by_name() {
    let mut fixed = openai();
    fixed.immutable_body_paths =
        BTreeMap::from([("openai-responses".to_owned(), paths(&["store"]))]);
    assert!(matches!(
        fixed.validate(),
        Err(ManifestErrorV1::InvalidImmutableBodyPaths { family, .. }) if family == "openai-responses"
    ));

    let mut passthrough = openai();
    passthrough.north_passthrough =
        BTreeMap::from([("openai-responses".to_owned(), NorthProtocolV1::Responses)]);
    assert!(matches!(
        passthrough.validate(),
        Err(ManifestErrorV1::InvalidNorthPassthrough { family, .. }) if family == "openai-responses"
    ));
}

#[test]
fn paths_follow_the_task_world_grammar() {
    for good in [&["store"][..], &["text.format", "reasoning-effort", "a_b.c-d.E9"], &[]] {
        assert!(are_immutable_body_paths(&paths(good)), "{good:?}");
    }
    let long = "a".repeat(MAX_IMMUTABLE_BODY_PATH_BYTES + 1);
    for bad in [
        &[""][..],
        &["store."],
        &[".store"],
        &["text..format"],
        &["input[0]"],
        &["/store"],
        &["st ore"],
        &["store", "store"],
        &[long.as_str()],
    ] {
        assert!(!are_immutable_body_paths(&paths(bad)), "{bad:?}");
        let mut manifest = openai();
        manifest.immutable_body_paths =
            BTreeMap::from([("openai-compatible".to_owned(), paths(bad))]);
        assert!(
            matches!(manifest.validate(), Err(ManifestErrorV1::InvalidImmutableBodyPaths { .. })),
            "{bad:?}"
        );
    }
    let at_limit = "a".repeat(MAX_IMMUTABLE_BODY_PATH_BYTES);
    assert!(are_immutable_body_paths(&paths(&[at_limit.as_str()])));
    let many: Vec<String> = (0..=MAX_IMMUTABLE_BODY_PATHS).map(|n| format!("p{n}")).collect();
    assert!(!are_immutable_body_paths(&many));
    assert!(are_immutable_body_paths(&many[..MAX_IMMUTABLE_BODY_PATHS]));
}

#[test]
fn other_worlds_refuse_both_declarations() {
    for package in ["embeddings-openai-compatible", "task-kling-v2", "image-azure"] {
        let manifest = shipped(package);
        let family = manifest.providers[0].clone();

        let mut fixed = manifest.clone();
        fixed.immutable_body_paths = BTreeMap::from([(family.clone(), paths(&["store"]))]);
        assert_eq!(
            fixed.validate(),
            Err(ManifestErrorV1::DeliveryIsAProviderWorldDeclaration),
            "{package}"
        );

        let mut passthrough = manifest;
        passthrough.north_passthrough = BTreeMap::from([(family, NorthProtocolV1::Responses)]);
        assert_eq!(
            passthrough.validate(),
            Err(ManifestErrorV1::DeliveryIsAProviderWorldDeclaration),
            "{package}"
        );
    }
}
