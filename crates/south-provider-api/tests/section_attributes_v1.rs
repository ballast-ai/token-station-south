//! Section-level credential attributes (the section attributes record,
//! `docs/design/2026-10-18-section-attributes-and-task-values.md` §2): a `credentials` section
//! exports a stored non-secret field whatever its slot's source, under a complete recipe's
//! attribute rules.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};
use south_provider_api::{ComponentManifestV1, ManifestErrorV1};

fn shipped(package: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../components/{package}/manifest.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A GMI-shaped section: a stored secret presented as is, and a non-secret organization id.
fn stored_key_section() -> Value {
    json!({
        "schema": "south.credential-recipe.v1",
        "fields": {
            "api_key": { "secret": true, "required": true },
            "organization_id": { "secret": false, "syntax": { "printable_ascii": 128 } }
        },
        "slots": { "provider_api_key": { "field": "api_key" } },
        "attributes": { "organization_id": { "field": "organization_id", "export": true } }
    })
}

fn with(package: &str, section: &Value) -> Result<ComponentManifestV1, ManifestErrorV1> {
    let mut manifest = shipped(package);
    manifest["credentials"] = section.clone();
    let manifest: ComponentManifestV1 = serde_json::from_value(manifest)
        .map_err(|error| ManifestErrorV1::InvalidCredentials(error.to_string()))?;
    manifest.validate().map(|()| manifest)
}

fn refused(section: &Value) -> String {
    match with("image-openai-compatible", section) {
        Err(ManifestErrorV1::InvalidCredentials(detail)) => detail,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_section_exports_a_stored_field_without_a_recipe() {
    let manifest = with("image-openai-compatible", &stored_key_section()).unwrap();
    let section = manifest.credentials.as_ref().unwrap();
    assert_eq!(section.attribute_names(), ["organization_id"].into_iter().collect());
    assert_eq!(section.admits_attribute("organization_id", "org-123"), Some(true));
    assert_eq!(section.admits_attribute("organization_id", "line\nbreak"), Some(false));
    assert_eq!(section.admits_attribute("project_id", "p"), None);

    let family = manifest.providers[0].clone();
    assert!(manifest.declared_keys(&family).contains("organization_id"));
    let declared = manifest
        .declared_values(
            &family,
            &BTreeMap::new(),
            &BTreeMap::from([("organization_id".to_owned(), "org-123".to_owned())]),
        )
        .unwrap();
    assert_eq!(declared.get("organization_id").map(String::as_str), Some("org-123"));

    let wire = serde_json::to_value(&manifest).unwrap();
    assert_eq!(
        wire["credentials"]["attributes"],
        json!({ "organization_id": { "field": "organization_id", "export": true, "persist": false } })
    );
}

#[test]
fn absent_section_attributes_are_omitted() {
    let mut section = stored_key_section();
    section.as_object_mut().unwrap().remove("attributes");
    let manifest = with("image-openai-compatible", &section).unwrap();
    let wire = serde_json::to_value(&manifest).unwrap();
    assert!(wire["credentials"].get("attributes").is_none());
    assert!(manifest.credentials.unwrap().attribute_names().is_empty());
}

#[test]
fn section_attributes_follow_a_complete_recipes_rules() {
    let cases: [(&str, Value); 5] = [
        ("snake_case", json!({ "Organization": { "field": "organization_id", "export": true } })),
        (
            "export: true",
            json!({ "organization_id": { "field": "organization_id", "export": false } }),
        ),
        (
            "exactly one field",
            json!({ "organization_id": { "selected_recipe": true, "export": true } }),
        ),
        ("non-secret", json!({ "organization_id": { "field": "api_key", "export": true } })),
        ("non-secret", json!({ "organization_id": { "field": "nowhere", "export": true } })),
    ];
    for (needle, attributes) in cases {
        let mut section = stored_key_section();
        section["attributes"] = attributes.clone();
        let detail = refused(&section);
        assert!(detail.contains(needle), "{attributes}: {detail}");
    }

    let mut free_text = stored_key_section();
    free_text["fields"]["organization_id"] = json!({ "secret": false });
    assert!(refused(&free_text).contains("value syntax"));

    // `persist` keeps its recipe meaning.
    let mut persisted = stored_key_section();
    persisted["attributes"]["organization_id"]["persist"] = json!(true);
    assert!(with("image-openai-compatible", &persisted).is_ok());
}

#[test]
fn a_section_attribute_may_not_share_a_name_with_a_recipe_attribute() {
    let mut vertex = shipped("provider-vertex")["credentials"].clone();
    let recipe = vertex["recipes"].as_object().unwrap().keys().next().unwrap().clone();
    let (name, attribute) = vertex["recipes"][&recipe]["attributes"]
        .as_object()
        .unwrap()
        .iter()
        .next()
        .map(|(name, attribute)| (name.clone(), attribute.clone()))
        .unwrap();
    vertex["attributes"] = json!({ name.clone(): attribute });
    let mut manifest = shipped("provider-vertex");
    manifest["credentials"] = vertex;
    let manifest: ComponentManifestV1 = serde_json::from_value(manifest).unwrap();
    match manifest.validate() {
        Err(ManifestErrorV1::InvalidCredentials(detail)) => {
            assert!(detail.contains(&name) && detail.contains("recipe"), "{detail}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}
