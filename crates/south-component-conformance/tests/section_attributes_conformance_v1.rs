//! Section-level credential attributes in a host's hands (the section attributes record,
//! `docs/design/2026-10-18-section-attributes-and-task-values.md` §2, §4): `exported_attributes_v1`
//! exports them for every slot source, and `task-gmi-image-v2` declares its organization id as one.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::credential_recipe::{
    ExportedAttributesV1, RecipeOutcomeV1, exported_attributes_v1,
};
use south_provider_api::{ComponentManifestV1, CredentialsV1};

fn shipped_value(package: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../components/{package}/manifest.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn validated(manifest: Value) -> ComponentManifestV1 {
    let manifest: ComponentManifestV1 = serde_json::from_value(manifest).unwrap();
    manifest.validate().expect("gate ① admits it");
    manifest
}

fn stored(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

/// The GMI section as shipped (§4).
fn gmi() -> CredentialsV1 {
    validated(shipped_value("task-gmi-image-v2")).credentials.unwrap()
}

#[test]
fn a_field_slot_exports_its_section_attributes() {
    let section = gmi();
    let slot = "provider_api_key";
    let none = BTreeMap::new();
    let exported = exported_attributes_v1(
        &section,
        slot,
        &stored(&[("api_key", "fake-key"), ("organization_id", "org-123")]),
        &none,
    )
    .unwrap();
    assert_eq!(exported.values, stored(&[("organization_id", "org-123")]));
    assert!(exported.persist.is_empty());

    // An absent or empty optional field exports nothing, never an empty value.
    for fields in
        [stored(&[("api_key", "fake-key")]), stored(&[("api_key", "k"), ("organization_id", "")])]
    {
        assert_eq!(
            exported_attributes_v1(&section, slot, &fields, &none).unwrap(),
            ExportedAttributesV1::default()
        );
    }
    // The required secret is still required.
    assert!(matches!(
        exported_attributes_v1(&section, slot, &stored(&[("organization_id", "org-123")]), &none),
        Err(RecipeOutcomeV1::Configuration { .. })
    ));
}

#[test]
fn section_attributes_keep_default_and_persist_and_join_a_minted_slots_recipe_attributes() {
    let mut vertex = shipped_value("provider-vertex");
    vertex["credentials"]["fields"]["label"] =
        json!({ "secret": false, "syntax": "token", "default": "standard" });
    vertex["credentials"]["fields"]["tier"] = json!({ "secret": false, "syntax": "token" });
    vertex["credentials"]["attributes"] = json!({
        "label": { "field": "label", "export": true },
        "tier": { "field": "tier", "export": true, "persist": true }
    });
    let section = validated(vertex).credentials.unwrap();
    let recipe_attributes: Vec<String> =
        section.recipes.values().flat_map(|recipe| recipe.attributes.keys().cloned()).collect();
    assert!(!recipe_attributes.is_empty(), "provider-vertex's recipe exports an attribute");
    let slot = section.slots.keys().next().unwrap().clone();
    let fields = [
        ("service_account", r#"{"client_email":"a@b.test","private_key":"k"}"#),
        ("project_id", "my-project-1"),
        ("tier", "gold"),
    ];
    let exported =
        exported_attributes_v1(&section, &slot, &stored(&fields), &BTreeMap::new()).unwrap();
    assert_eq!(exported.values.get("label").map(String::as_str), Some("standard"));
    assert_eq!(exported.values.get("tier").map(String::as_str), Some("gold"));
    for name in &recipe_attributes {
        assert!(exported.values.contains_key(name), "{name}");
    }
    assert_eq!(exported.persist, stored(&[("tier", "gold")]));

    // The field is gone: the persisted value stands in.
    let without = stored(&fields[..2]);
    let exported =
        exported_attributes_v1(&section, &slot, &without, &stored(&[("tier", "gold")])).unwrap();
    assert_eq!(exported.values.get("tier").map(String::as_str), Some("gold"));
    assert!(exported.persist.is_empty());
}

#[test]
fn the_gmi_package_declares_its_organization_id_as_a_section_attribute() {
    let manifest = validated(shipped_value("task-gmi-image-v2"));
    // Published as 0.36.0 with 0.74.0; the Kiro image input record's Q47 bump made it 0.36.1
    // (`kiro_images_retires_every_published_074_package_identity`).
    // Published as 0.36.1 with 0.75.0; the Bailian records' Q47 bump made it 0.36.2
    // (`bailian_retires_every_published_075_package_identity`).
    // Published as 0.36.2 with 0.76.0; the Gemini signatures re-pin made it 0.36.3
    // (`gemini_signatures_retires_every_published_076_package_identity`).
    // Published as 0.36.3 with 0.77.0; the Gemini placeholder signature round made it 0.36.4
    // (`gemini_placeholder_retires_every_published_077_package_identity`).
    // Published as 0.36.4 with 0.78.0; the 0.79.0 round made it 0.36.5
    // (`qwen_tier_and_responses_stop_retires_every_published_078_package_identity`).
    assert_eq!(manifest.version, "0.36.5");
    let wire = serde_json::to_value(manifest.credentials.as_ref().unwrap()).unwrap();
    assert_eq!(
        wire,
        json!({
            "schema": "south.credential-recipe.v1",
            "fields": {
                "api_key": {
                    "secret": true, "required": true, "description": "The GMI Cloud API key."
                },
                "organization_id": {
                    "secret": false, "required": false, "syntax": { "printable_ascii": 128 },
                    "description":
                        "The GMI Cloud organization id, sent as X-Organization-ID when set."
                }
            },
            "slots": { "provider_api_key": { "field": "api_key" } },
            "attributes": {
                "organization_id": { "field": "organization_id", "export": true, "persist": false }
            }
        })
    );
    assert_eq!(manifest.declared_keys("gmi-image"), std::iter::once("organization_id").collect());
}
