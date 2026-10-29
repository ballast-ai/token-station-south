//! Gate ① for `served_models` and `required_prefetch` (the served models record,
//! `docs/design/2026-10-28-served-models-prefetch-and-failure-category.md` §2–§3; ruled SM-Q1–Q3
//! and RP-Q1 by lv on 2026-10-29).
//!
//! `served_models` is required for every family of a task, image or speech package and refused
//! elsewhere; each declaration is exactly one of `ids`, `prefixes` or `any: true`. A family that
//! leaves it out is refused by name, so "not declared" can never be read as "accepts everything".
//! `required_prefetch` is task-v2 only and optional.

use std::path::Path;

use serde_json::{Value, json};
use south_provider_api::{
    ComponentManifestV1, MAX_REQUIRED_PREFETCH_FIELDS, MAX_SERVED_MODELS, ManifestErrorV1,
    ServedModelsV1, are_required_prefetch_fields,
};

fn shipped(package: &str) -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components")
        .join(package)
        .join("manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn task_manifest_json() -> Value {
    json!({
      "name": "task-example-v2",
      "version": "0.1.0",
      "api_version": "task-adapter-v2",
      "providers": ["example-video", "example-video-pro"],
      "capabilities": ["submit", "observe", "render"],
      "auth_arms": ["bearer"],
      "served_models": {
        "example-video": {"ids": ["video-1", "video-2"]},
        "example-video-pro": {"prefixes": ["video-pro-"]}
      },
      "permissions": {"network": false, "filesystem": false, "secrets": ["provider_api_key"]},
      "conformance": {"required_suite": "south.task-component.v2", "fixtures": "fixtures/"},
      "compatibility": {
        "ir_schema_id": "token-station-protocol@0.7.0/v0.6.0",
        "kernel_version": "0.6.0",
        "kernel_revision": "a04925518b95c6557374252db7fbe7f026688bde",
        "wit_package": "token-station:task-adapter@2.0.0",
        "south_runtime": "0.61.0",
        "contracts": {"task": 7}
      }
    })
}

fn task_manifest() -> ComponentManifestV1 {
    serde_json::from_value(task_manifest_json()).unwrap()
}

fn ids(list: &[&str]) -> ServedModelsV1 {
    ServedModelsV1 {
        ids: Some(list.iter().map(|id| (*id).to_owned()).collect()),
        ..Default::default()
    }
}

fn prefixes(list: &[&str]) -> ServedModelsV1 {
    ServedModelsV1 {
        prefixes: Some(list.iter().map(|id| (*id).to_owned()).collect()),
        ..Default::default()
    }
}

fn any() -> ServedModelsV1 {
    ServedModelsV1 { any: Some(true), ..Default::default() }
}

fn invalid_served(family: &str) -> impl Fn(&ManifestErrorV1) -> bool + '_ {
    move |error| matches!(error, ManifestErrorV1::InvalidServedModels { family: named, .. } if named == family)
}

#[test]
fn each_form_parses_validates_and_round_trips() {
    let mut source = task_manifest_json();
    source["providers"] = json!(["a", "b", "c"]);
    source["served_models"] = json!({
        "a": {"ids": ["m-1"]},
        "b": {"prefixes": ["m-"]},
        "c": {"any": true}
    });
    let manifest: ComponentManifestV1 = serde_json::from_value(source.clone()).unwrap();
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(manifest.served_models["a"], ids(&["m-1"]));
    assert_eq!(manifest.served_models["b"], prefixes(&["m-"]));
    assert_eq!(manifest.served_models["c"], any());
    let json = serde_json::to_value(&manifest).unwrap();
    assert_eq!(json["served_models"], source["served_models"], "absent forms are not written");
    assert_eq!(serde_json::from_value::<ComponentManifestV1>(json).unwrap(), manifest);
}

#[test]
fn serves_matches_exact_ids_prefixes_and_any() {
    let exact = ids(&["qwen-image", "qwen-image-plus"]);
    assert!(exact.serves("qwen-image"));
    assert!(exact.serves("qwen-image-plus"));
    assert!(!exact.serves("qwen-image-3.0-pro"), "an exact list is not a prefix");
    assert!(!exact.serves("Qwen-Image"), "case-sensitive");
    let scheme = prefixes(&["wan2.", "wanx"]);
    assert!(scheme.serves("wan2.5-t2i-preview"));
    assert!(scheme.serves("wanx2.1-t2i-turbo"));
    assert!(!scheme.serves("qwen-image-3.0-pro"), "the measured mis-route");
    assert!(any().serves("whatever-the-operator-chose"));
    let two_forms = ServedModelsV1 { any: Some(true), ..ids(&["m"]) };
    assert!(!two_forms.serves("m"), "an invalid declaration serves nothing");
}

#[test]
fn exactly_one_form_is_admitted() {
    let bad = [
        ServedModelsV1::default(),
        ServedModelsV1 { any: Some(true), ..ids(&["m"]) },
        ServedModelsV1 { prefixes: Some(vec!["p".to_owned()]), ..ids(&["m"]) },
        ServedModelsV1 { any: Some(false), ..Default::default() },
    ];
    for served in bad {
        assert!(!served.is_valid(), "{served:?}");
        let mut manifest = task_manifest();
        manifest.served_models.insert("example-video".to_owned(), served.clone());
        let error = manifest.validate().unwrap_err();
        assert!(invalid_served("example-video")(&error), "{served:?}: {error}");
    }
}

#[test]
fn lists_are_non_empty_sorted_distinct_and_bounded() {
    let too_many: Vec<String> = (0..=MAX_SERVED_MODELS).map(|i| format!("m-{i:04}")).collect();
    let bad = [
        ids(&[]),
        ids(&["b", "a"]),
        ids(&["a", "a"]),
        ids(&[""]),
        ids(&["has space"]),
        ids(&["tab\tbed"]),
        ids(&["ünïcode"]),
        prefixes(&[]),
        ServedModelsV1 { ids: Some(vec!["x".repeat(129)]), ..Default::default() },
        ServedModelsV1 { ids: Some(too_many), ..Default::default() },
    ];
    for served in bad {
        assert!(!served.is_valid(), "{served:?}");
    }
    assert!(ids(&["MiniMax-Hailuo-02", "veo-3.0-generate-001", "wan2.6-i2v"]).is_valid());
    assert!(ServedModelsV1 { ids: Some(vec!["x".repeat(128)]), ..Default::default() }.is_valid());
}

#[test]
fn an_undeclared_family_is_refused_by_name() {
    let mut manifest = task_manifest();
    manifest.served_models.remove("example-video-pro");
    assert_eq!(
        manifest.validate(),
        Err(ManifestErrorV1::ServedModelsRequired("example-video-pro".to_owned()))
    );
    let mut none = task_manifest();
    none.served_models.clear();
    assert_eq!(
        none.validate(),
        Err(ManifestErrorV1::ServedModelsRequired("example-video".to_owned())),
        "absent is not `any`"
    );
}

#[test]
fn a_declaration_for_a_family_the_manifest_does_not_declare_is_refused() {
    let mut manifest = task_manifest();
    manifest.served_models.insert("someone-else".to_owned(), any());
    let error = manifest.validate().unwrap_err();
    assert!(invalid_served("someone-else")(&error), "{error}");
}

#[test]
fn every_shipped_media_package_declares_every_family_and_validates() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../components");
    let mut media = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let package = entry.unwrap().file_name().into_string().unwrap();
        let manifest = shipped(&package);
        assert_eq!(manifest.validate(), Ok(()), "{package}");
        let is_media =
            ["task-", "image-", "speech-"].iter().any(|world| package.starts_with(world));
        if is_media {
            media += 1;
            let declared: Vec<_> = manifest.served_models.keys().cloned().collect();
            let mut families = manifest.providers.clone();
            families.sort();
            assert_eq!(declared, families, "{package}");
        } else {
            assert!(manifest.served_models.is_empty(), "{package}");
        }
    }
    assert!(media >= 29, "{media} media packages found");
}

#[test]
fn only_task_image_and_speech_worlds_declare_served_models() {
    for package in ["provider-openai-compatible", "embeddings-openai-compatible"] {
        let mut manifest = shipped(package);
        assert_eq!(manifest.validate(), Ok(()), "{package}");
        let family = manifest.providers[0].clone();
        manifest.served_models.insert(family, any());
        assert_eq!(
            manifest.validate(),
            Err(ManifestErrorV1::ServedModelsIsAMediaWorldDeclaration),
            "{package} (SM-Q3)"
        );
    }
    for package in ["task-kling", "image-azure", "speech-groq"] {
        let mut manifest = shipped(package);
        assert_eq!(manifest.validate(), Ok(()), "{package}");
        manifest.served_models.clear();
        assert!(
            matches!(manifest.validate(), Err(ManifestErrorV1::ServedModelsRequired(_))),
            "{package}"
        );
    }
}

#[test]
fn required_prefetch_parses_validates_and_round_trips() {
    let mut source = task_manifest_json();
    source["required_prefetch"] =
        json!({"example-video": ["image", "image_url", "reference_images"]});
    let manifest: ComponentManifestV1 = serde_json::from_value(source.clone()).unwrap();
    assert_eq!(manifest.validate(), Ok(()));
    let json = serde_json::to_value(&manifest).unwrap();
    assert_eq!(json["required_prefetch"], source["required_prefetch"]);
    assert!(
        serde_json::to_value(task_manifest()).unwrap().get("required_prefetch").is_none(),
        "absent is not written"
    );
}

#[test]
fn required_prefetch_follows_the_route_field_grammar() {
    let too_many: Vec<String> =
        (0..=MAX_REQUIRED_PREFETCH_FIELDS).map(|i| format!("f{i:02}")).collect();
    for fields in [
        vec![],
        vec!["b".to_owned(), "a".to_owned()],
        vec!["a".to_owned(), "a".to_owned()],
        vec!["model".to_owned()],
        vec!["a.b".to_owned()],
        vec![String::new()],
        vec!["x".repeat(65)],
        too_many,
    ] {
        assert!(!are_required_prefetch_fields(&fields), "{fields:?}");
        let mut manifest = task_manifest();
        manifest.required_prefetch.insert("example-video".to_owned(), fields.clone());
        assert!(
            matches!(
                manifest.validate(),
                Err(ManifestErrorV1::InvalidRequiredPrefetch { family, .. }) if family == "example-video"
            ),
            "{fields:?}"
        );
    }
    let mut manifest = task_manifest();
    manifest.required_prefetch.insert("unknown".to_owned(), vec!["image".to_owned()]);
    assert!(matches!(
        manifest.validate(),
        Err(ManifestErrorV1::InvalidRequiredPrefetch { family, .. }) if family == "unknown"
    ));
}

#[test]
fn only_the_task_v2_world_declares_required_prefetch() {
    for package in ["task-kling", "image-azure", "speech-groq", "provider-openai-compatible"] {
        let mut manifest = shipped(package);
        let family = manifest.providers[0].clone();
        manifest.required_prefetch.insert(family, vec!["image".to_owned()]);
        assert_eq!(
            manifest.validate(),
            Err(ManifestErrorV1::RequiredPrefetchIsATaskV2Declaration),
            "{package}"
        );
    }
}
