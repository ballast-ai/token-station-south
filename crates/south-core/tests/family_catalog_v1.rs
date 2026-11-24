//! The family catalog data artifact, `south.family-catalog.v1`
//! (`docs/design/2026-11-24-family-catalog.md`).
//!
//! The shipped catalog must parse and agree with every package under `components/`; each rule of
//! the format and of the coverage check has a reverse case.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use south_core::{
    DeclaredFamilyV1, FAMILY_CATALOG_SCHEMA_V1, FamilyCatalogCoverageErrorV1, FamilyCatalogErrorV1,
    FamilyCatalogV1, FamilyModalityV1, declared_families_v1,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn shipped() -> Value {
    serde_json::from_slice(
        &std::fs::read(repo_root().join("catalogs/family-catalog.json")).unwrap(),
    )
    .unwrap()
}

fn manifests() -> Vec<Value> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(repo_root().join("components"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("manifest.json"))
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no component manifests found");
    paths
        .iter()
        .map(|path| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())
        .collect()
}

fn declared() -> Vec<DeclaredFamilyV1> {
    manifests().iter().flat_map(declared_families_v1).collect()
}

fn text(word: &str) -> Value {
    json!({ "zh-CN": format!("{word}（中文)"), "en": word })
}

fn minimal() -> Value {
    json!({
        "schema": "south.family-catalog.v1",
        "groups": [{ "id": "cloud", "display_name": text("Cloud") }],
        "entries": [
            {
                "world": "provider-adapter-v2",
                "family": "alpha",
                "group": "cloud",
                "display_name": text("Alpha"),
                "description": text("Alpha chat"),
                "config_keys": { "region": text("Region") },
                "credential_fields": { "secret": text("Secret") }
            },
            {
                "world": "task-adapter-v2",
                "family": "alpha",
                "display_name": text("Alpha video"),
                "description": text("Alpha video tasks"),
                "modality": "video"
            }
        ]
    })
}

fn declared_minimal() -> Vec<DeclaredFamilyV1> {
    let mut families = declared_families_v1(&json!({
        "api_version": "provider-adapter-v2",
        "providers": ["alpha"],
        "config_schema": { "alpha": { "region": {} } },
        "credentials": { "fields": { "secret": {} } }
    }));
    families.extend(declared_families_v1(&json!({
        "api_version": "task-adapter-v2",
        "providers": ["alpha"]
    })));
    families
}

fn refusal(document: &Value) -> FamilyCatalogErrorV1 {
    let from_value = FamilyCatalogV1::from_value(document).unwrap_err();
    assert_eq!(
        FamilyCatalogV1::parse(&serde_json::to_vec(document).unwrap()).unwrap_err(),
        from_value
    );
    from_value
}

fn malformed(document: &Value) -> String {
    match refusal(document) {
        FamilyCatalogErrorV1::Malformed { reason } => reason,
        other => panic!("expected Malformed, got {other:?}"),
    }
}

#[test]
fn the_schema_id_is_frozen() {
    assert_eq!(FAMILY_CATALOG_SCHEMA_V1, "south.family-catalog.v1");
}

#[test]
fn the_shipped_catalog_parses_and_agrees_with_every_package() {
    let catalog = FamilyCatalogV1::from_value(&shipped()).unwrap();
    assert_eq!(catalog.check_against(&declared()), Ok(()));
}

#[test]
fn every_shipped_provider_and_task_family_has_an_entry() {
    let catalog = FamilyCatalogV1::from_value(&shipped()).unwrap();
    let required: Vec<_> = declared()
        .into_iter()
        .filter(|family| matches!(family.world.as_str(), "provider-adapter-v2" | "task-adapter-v2"))
        .collect();
    assert!(!required.is_empty());
    for family in required {
        let entry = catalog
            .entry(&family.world, &family.family)
            .unwrap_or_else(|| panic!("no entry for {} / {}", family.world, family.family));
        assert_eq!(entry.modality().is_some(), family.world == "task-adapter-v2");
    }
}

#[test]
fn shipped_task_families_name_their_modality() {
    let catalog = FamilyCatalogV1::from_value(&shipped()).unwrap();
    let image = catalog.entry("task-adapter-v2", "wan-image").unwrap();
    assert_eq!(image.modality(), Some(FamilyModalityV1::Image));
    let video = catalog.entry("task-adapter-v2", "kling").unwrap();
    assert_eq!(video.modality(), Some(FamilyModalityV1::Video));
}

#[test]
fn a_lookup_is_by_world_and_family() {
    let catalog = FamilyCatalogV1::from_value(&minimal()).unwrap();
    let provider = catalog.entry("provider-adapter-v2", "alpha").unwrap();
    let task = catalog.entry("task-adapter-v2", "alpha").unwrap();
    assert_eq!(provider.display_name().en(), "Alpha");
    assert_eq!(task.display_name().en(), "Alpha video");
    assert_eq!(provider.display_name().get("zh-CN"), Some("Alpha（中文)"));
    assert_eq!(provider.display_name().get("fr"), None);
    assert_eq!(provider.group(), Some("cloud"));
    assert_eq!(catalog.group("cloud").unwrap().display_name().en(), "Cloud");
    assert_eq!(task.group(), None);
    assert_eq!(provider.config_keys()["region"].en(), "Region");
    assert_eq!(provider.credential_fields()["secret"].en(), "Secret");
    assert!(catalog.entry("image-adapter-v1", "alpha").is_none());
    assert_eq!(catalog.check_against(&declared_minimal()), Ok(()));
}

#[test]
fn bytes_that_are_not_json_are_refused() {
    assert!(matches!(
        FamilyCatalogV1::parse(b"not json"),
        Err(FamilyCatalogErrorV1::NotJson { .. })
    ));
}

#[test]
fn another_schema_is_refused() {
    let mut document = minimal();
    document["schema"] = json!("south.family-catalog.v2");
    assert_eq!(
        refusal(&document),
        FamilyCatalogErrorV1::UnknownSchema { found: "south.family-catalog.v2".to_owned() }
    );
}

#[test]
fn unknown_and_missing_fields_are_refused() {
    let mut top = minimal();
    top["extra"] = json!(1);
    assert!(malformed(&top).contains("unknown field `extra`"));

    let mut no_groups = minimal();
    no_groups.as_object_mut().unwrap().remove("groups");
    assert!(malformed(&no_groups).contains("no `groups`"));

    let mut entry = minimal();
    entry["entries"][0]["icon"] = json!("x");
    assert!(malformed(&entry).contains("entry 0 has the unknown field `icon`"));

    let mut group = minimal();
    group["groups"][0]["order"] = json!(1);
    assert!(malformed(&group).contains("group 0 has the unknown field `order`"));

    let mut no_description = minimal();
    no_description["entries"][1].as_object_mut().unwrap().remove("description");
    assert!(malformed(&no_description).contains("entry 1 has no `description`"));
}

#[test]
fn every_text_needs_both_languages_and_nothing_else() {
    let mut missing = minimal();
    missing["entries"][0]["display_name"] = json!({ "en": "Alpha" });
    assert!(malformed(&missing).contains("has no `zh-CN`"));

    let mut extra = minimal();
    extra["entries"][0]["description"]["ja"] = json!("アルファ");
    assert!(malformed(&extra).contains("unknown field `ja`"));

    let mut blank = minimal();
    blank["groups"][0]["display_name"]["en"] = json!("  ");
    assert!(malformed(&blank).contains("`en` is not a non-empty string"));

    let mut key_text = minimal();
    key_text["entries"][0]["config_keys"]["region"] = json!("Region");
    assert!(malformed(&key_text).contains("`config_keys`.`region` is not a JSON object"));
}

#[test]
fn ids_must_be_non_empty_strings() {
    let mut world = minimal();
    world["entries"][0]["world"] = json!("");
    assert!(malformed(&world).contains("`world` is not a non-empty string"));

    let mut family = minimal();
    family["entries"][0]["family"] = json!(7);
    assert!(malformed(&family).contains("`family` is not a non-empty string"));
}

#[test]
fn a_repeated_group_id_is_refused() {
    let mut document = minimal();
    document["groups"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "id": "cloud", "display_name": text("Again") }));
    assert_eq!(
        refusal(&document),
        FamilyCatalogErrorV1::DuplicateGroup { group: 1, id: "cloud".to_owned() }
    );
}

#[test]
fn a_repeated_world_and_family_is_refused() {
    let mut document = minimal();
    let copy = document["entries"][0].clone();
    document["entries"].as_array_mut().unwrap().push(copy);
    assert_eq!(
        refusal(&document),
        FamilyCatalogErrorV1::DuplicateEntry {
            entry: 2,
            world: "provider-adapter-v2".to_owned(),
            family: "alpha".to_owned(),
        }
    );
}

#[test]
fn a_reference_to_an_unknown_group_is_refused() {
    let mut document = minimal();
    document["entries"][0]["group"] = json!("subscription");
    assert_eq!(
        refusal(&document),
        FamilyCatalogErrorV1::UnknownGroup { entry: 0, group: "subscription".to_owned() }
    );
}

#[test]
fn task_v2_entries_and_only_they_carry_a_modality() {
    let mut missing = minimal();
    missing["entries"][1].as_object_mut().unwrap().remove("modality");
    assert_eq!(refusal(&missing), FamilyCatalogErrorV1::MissingModality { entry: 1 });

    let mut unexpected = minimal();
    unexpected["entries"][0]["modality"] = json!("video");
    assert_eq!(refusal(&unexpected), FamilyCatalogErrorV1::UnexpectedModality { entry: 0 });

    let mut unknown = minimal();
    unknown["entries"][1]["modality"] = json!("audio");
    assert!(malformed(&unknown).contains("`modality` is not `video` or `image`"));
}

#[test]
fn a_declared_provider_or_task_family_without_an_entry_is_reported() {
    let catalog = FamilyCatalogV1::from_value(&minimal()).unwrap();
    let mut families = declared_minimal();
    families.extend(declared_families_v1(&json!({
        "api_version": "task-adapter-v2",
        "providers": ["beta"]
    })));
    assert_eq!(
        catalog.check_against(&families),
        Err(vec![FamilyCatalogCoverageErrorV1::MissingEntry {
            world: "task-adapter-v2".to_owned(),
            family: "beta".to_owned(),
        }])
    );
}

#[test]
fn families_of_the_optional_worlds_need_no_entry() {
    let catalog = FamilyCatalogV1::from_value(&minimal()).unwrap();
    let mut families = declared_minimal();
    for world in
        ["image-adapter-v1", "speech-adapter-v1", "embeddings-adapter-v1", "task-adapter-v1"]
    {
        families
            .extend(declared_families_v1(&json!({ "api_version": world, "providers": ["gamma"] })));
    }
    assert_eq!(catalog.check_against(&families), Ok(()));
}

#[test]
fn an_entry_no_package_declares_is_reported() {
    let catalog = FamilyCatalogV1::from_value(&minimal()).unwrap();
    let families: Vec<_> = declared_minimal()
        .into_iter()
        .filter(|family| family.world == "provider-adapter-v2")
        .collect();
    assert_eq!(
        catalog.check_against(&families),
        Err(vec![FamilyCatalogCoverageErrorV1::UndeclaredFamily {
            world: "task-adapter-v2".to_owned(),
            family: "alpha".to_owned(),
        }])
    );
}

#[test]
fn keys_the_package_does_not_declare_are_reported() {
    let catalog = FamilyCatalogV1::from_value(&minimal()).unwrap();
    let mut families = declared_families_v1(&json!({
        "api_version": "provider-adapter-v2",
        "providers": ["alpha"],
        "config_schema": { "other": { "region": {} } },
        "credentials": { "families": ["other"], "fields": { "secret": {} } }
    }));
    families.extend(declared_families_v1(
        &json!({ "api_version": "task-adapter-v2", "providers": ["alpha"] }),
    ));
    assert_eq!(
        catalog.check_against(&families),
        Err(vec![
            FamilyCatalogCoverageErrorV1::UndeclaredConfigKey {
                world: "provider-adapter-v2".to_owned(),
                family: "alpha".to_owned(),
                key: "region".to_owned(),
            },
            FamilyCatalogCoverageErrorV1::UndeclaredCredentialField {
                world: "provider-adapter-v2".to_owned(),
                family: "alpha".to_owned(),
                field: "secret".to_owned(),
            },
        ])
    );
}

#[test]
fn credential_fields_follow_the_families_the_recipe_names() {
    let families = declared_families_v1(&json!({
        "api_version": "provider-adapter-v2",
        "providers": ["plain", "minted"],
        "credentials": { "families": ["minted"], "fields": { "token": {} } }
    }));
    let fields = |family: &str| {
        families
            .iter()
            .find(|declared| declared.family == family)
            .unwrap()
            .credential_fields
            .clone()
    };
    assert!(fields("plain").is_empty());
    assert_eq!(fields("minted").into_iter().collect::<Vec<_>>(), ["token"]);
}

#[test]
fn the_release_knows_the_schema() {
    let script = std::fs::read_to_string(repo_root().join("scripts/release_index.py")).unwrap();
    assert!(
        script.contains("\"south.family-catalog.v1\""),
        "release_index.py must accept the schema"
    );
}
