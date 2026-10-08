//! The model catalog data artifact, `south.model-catalog.v1`
//! (`docs/design/2026-09-30-host-zero-vendor-boundary.md` §7.5 and §13.11): South owns the
//! document's shape; the meaning of `capabilities` stays with the host.

use std::path::{Path, PathBuf};

use proptest::prelude::*;
use serde_json::{Map, Value, json};
use south_provider_runtime::{
    MODEL_CATALOG_SCHEMA_V1, ModelCatalogEntryV1, ModelCatalogErrorV1, ModelCatalogV1, ModelMatchV1,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn shipped_bytes() -> Vec<u8> {
    std::fs::read(repo_root().join("catalogs/model-catalog.json")).unwrap()
}

fn parse(document: &Value) -> Result<ModelCatalogV1, ModelCatalogErrorV1> {
    let catalog = ModelCatalogV1::parse(&serde_json::to_vec(document).unwrap());
    assert_eq!(ModelCatalogV1::from_value(document), catalog);
    catalog
}

fn one_entry(rules: Value, capabilities: Value) -> Value {
    let mut entry = Map::new();
    entry.insert("match".to_owned(), rules);
    entry.insert("capabilities".to_owned(), capabilities);
    let mut document = json!({ "schema": MODEL_CATALOG_SCHEMA_V1 });
    document["entries"] = Value::Array(vec![Value::Object(entry)]);
    document
}

#[test]
fn the_schema_id_is_the_published_one() {
    assert_eq!(MODEL_CATALOG_SCHEMA_V1, "south.model-catalog.v1");
}

#[test]
fn the_shipped_catalog_is_a_valid_document_that_round_trips() {
    let bytes = shipped_bytes();
    let catalog = ModelCatalogV1::parse(&bytes).unwrap();
    assert!(!catalog.entries().is_empty());

    // Serializing the parsed catalog gives back the same JSON value, and parsing that again gives
    // the same catalog: nothing in the shipped file is dropped or rewritten.
    let original: Value = serde_json::from_slice(&bytes).unwrap();
    let written = catalog.to_value();
    assert_eq!(written, original);
    assert_eq!(parse(&written).unwrap(), catalog);
}

#[test]
fn the_shipped_catalog_is_ascii_json_ending_in_a_newline() {
    let bytes = shipped_bytes();
    assert!(bytes.is_ascii());
    assert_eq!(bytes.last(), Some(&b'\n'));
}

#[test]
fn the_first_matching_entry_wins_in_the_shipped_catalog() {
    let catalog = ModelCatalogV1::parse(&shipped_bytes()).unwrap();
    let fast = catalog.entry_for("seedance-2-0-fast-260128").unwrap();
    let generic = catalog.entry_for("seedance-2-0-260128").unwrap();
    // The generic `contains: seedance-2-0` rule also matches the fast id; the fast entry comes
    // first, so it decides.
    assert!(fast.matches().contains(&ModelMatchV1::Contains("seedance-2-0-fast".to_owned())));
    assert!(generic.matches().contains(&ModelMatchV1::Contains("seedance-2-0".to_owned())));
    assert_ne!(fast, generic);
    assert!(catalog.entry_for("gpt-image-2").is_some());
    assert!(catalog.entry_for("gpt-image-2-unknown-suffix").is_none());
    assert!(catalog.entry_for("an-unlisted-model").is_none());
}

#[test]
fn a_minimal_document_loads_and_writes_back() {
    let document = json!({
        "schema": "south.model-catalog.v1",
        "entries": [
            { "match": [{ "prefix": "model-a-" }], "capabilities": {} },
            { "match": [{ "exact": "model-b" }, { "contains": "b-v2" }], "capabilities": { "x": 1 } },
        ],
    });
    let catalog = parse(&document).unwrap();
    assert_eq!(catalog.entries().len(), 2);
    assert_eq!(
        catalog.entries()[1].matches(),
        [ModelMatchV1::Exact("model-b".to_owned()), ModelMatchV1::Contains("b-v2".to_owned())]
    );
    assert_eq!(catalog.to_value(), document);
}

#[test]
fn an_empty_entry_list_is_a_valid_shape() {
    let catalog = parse(&json!({ "schema": MODEL_CATALOG_SCHEMA_V1, "entries": [] })).unwrap();
    assert!(catalog.entries().is_empty());
    assert!(catalog.entry_for("anything").is_none());
}

#[test]
fn capabilities_are_carried_verbatim_whatever_their_vocabulary() {
    // South does not interpret capabilities: a key no host knows today is kept, not refused.
    let capabilities = json!({
        "params": { "duration": { "min": 4, "max": 15 } },
        "a_future_host_key": [1, "two", { "three": null }],
    });
    let catalog = parse(&one_entry(json!([{ "exact": "m" }]), capabilities.clone())).unwrap();
    assert_eq!(Value::Object(catalog.entries()[0].capabilities().clone()), capabilities);
}

#[test]
fn match_rules_compare_case_sensitively() {
    assert!(ModelMatchV1::Exact("m".to_owned()).hits("m"));
    assert!(!ModelMatchV1::Exact("m".to_owned()).hits("M"));
    assert!(!ModelMatchV1::Exact("m".to_owned()).hits("m-1"));
    assert!(ModelMatchV1::Prefix("m-".to_owned()).hits("m-1"));
    assert!(!ModelMatchV1::Prefix("m-".to_owned()).hits("x-m-1"));
    assert!(ModelMatchV1::Contains("m-".to_owned()).hits("x-m-1"));
}

#[test]
fn an_unknown_schema_is_refused() {
    let mut document = one_entry(json!([{ "exact": "m" }]), json!({}));
    document["schema"] = json!("south.model-catalog.v2");
    assert_eq!(
        parse(&document),
        Err(ModelCatalogErrorV1::UnknownSchema { found: "south.model-catalog.v2".to_owned() })
    );
}

#[test]
fn unknown_or_missing_fields_are_refused() {
    let mut top = one_entry(json!([{ "exact": "m" }]), json!({}));
    top["models"] = json!([]);
    let mut entry = one_entry(json!([{ "exact": "m" }]), json!({}));
    entry["entries"][0]["modality"] = json!("image");
    let no_schema = json!({ "entries": [] });
    let no_entries = json!({ "schema": MODEL_CATALOG_SCHEMA_V1 });
    let no_capabilities =
        json!({ "schema": MODEL_CATALOG_SCHEMA_V1, "entries": [{ "match": [{ "exact": "m" }] }] });
    let schema_not_string = json!({ "schema": 1, "entries": [] });
    let entries_not_array = json!({ "schema": MODEL_CATALOG_SCHEMA_V1, "entries": {} });
    let entry_not_object = json!({ "schema": MODEL_CATALOG_SCHEMA_V1, "entries": [[]] });
    let match_not_array = one_entry(json!({ "exact": "m" }), json!({}));
    for (document, reason) in [
        (top, "the document has unknown field `models`"),
        (entry, "entry 0 has unknown field `modality`"),
        (no_schema, "the document has no `schema`"),
        (no_entries, "the document has no `entries`"),
        (no_capabilities, "entry 0 has no `capabilities`"),
        (schema_not_string, "`schema` is not a string"),
        (entries_not_array, "`entries` is not an array"),
        (entry_not_object, "entry 0 is not a JSON object"),
        (match_not_array, "entry 0 `match` is not an array"),
        (json!([]), "the document is not a JSON object"),
    ] {
        assert_eq!(
            parse(&document),
            Err(ModelCatalogErrorV1::Malformed { reason: reason.to_owned() }),
            "{document}"
        );
    }
    assert!(matches!(ModelCatalogV1::parse(b"not json"), Err(ModelCatalogErrorV1::NotJson { .. })));
}

#[test]
fn an_entry_without_a_match_rule_is_refused() {
    assert_eq!(
        parse(&one_entry(json!([]), json!({}))),
        Err(ModelCatalogErrorV1::EmptyMatchList { entry: 0 })
    );
    assert_eq!(
        ModelCatalogV1::new(vec![ModelCatalogEntryV1::new(Vec::new(), Map::new())]),
        Err(ModelCatalogErrorV1::EmptyMatchList { entry: 0 })
    );
}

#[test]
fn a_bad_match_rule_is_refused() {
    let cases = [
        (
            json!([{ "regex": "m.*" }]),
            ModelCatalogErrorV1::UnknownMatchKind { entry: 0, rule: 0, kind: "regex".to_owned() },
        ),
        (
            json!([{ "exact": "m" }, { "exact": "n", "prefix": "n-" }]),
            ModelCatalogErrorV1::MatchRuleNotOneKey { entry: 0, rule: 1 },
        ),
        (json!([{}]), ModelCatalogErrorV1::MatchRuleNotOneKey { entry: 0, rule: 0 }),
        (json!(["m"]), ModelCatalogErrorV1::MatchRuleNotOneKey { entry: 0, rule: 0 }),
        (json!([{ "prefix": "" }]), ModelCatalogErrorV1::InvalidMatchValue { entry: 0, rule: 0 }),
        (json!([{ "contains": 7 }]), ModelCatalogErrorV1::InvalidMatchValue { entry: 0, rule: 0 }),
    ];
    for (rules, expected) in cases {
        assert_eq!(parse(&one_entry(rules, json!({}))), Err(expected));
    }
}

#[test]
fn a_repeated_rule_is_refused_because_it_could_never_decide() {
    let document = json!({
        "schema": MODEL_CATALOG_SCHEMA_V1,
        "entries": [
            { "match": [{ "exact": "m" }], "capabilities": {} },
            { "match": [{ "prefix": "m" }, { "exact": "m" }], "capabilities": {} },
        ],
    });
    assert_eq!(
        parse(&document),
        Err(ModelCatalogErrorV1::DuplicateMatchRule { entry: 1, rule: 1, first_entry: 0 })
    );
    // The same value under another kind is a different rule.
    let distinct = json!({
        "schema": MODEL_CATALOG_SCHEMA_V1,
        "entries": [{ "match": [{ "exact": "m" }, { "prefix": "m" }], "capabilities": {} }],
    });
    assert!(parse(&distinct).is_ok());
}

#[test]
fn capabilities_that_are_not_an_object_are_refused() {
    for capabilities in [json!(null), json!([]), json!("all"), json!(1)] {
        assert_eq!(
            parse(&one_entry(json!([{ "exact": "m" }]), capabilities)),
            Err(ModelCatalogErrorV1::CapabilitiesNotObject { entry: 0 })
        );
    }
}

fn rule() -> impl Strategy<Value = ModelMatchV1> {
    let value = "[a-z0-9.-]{1,12}";
    prop_oneof![
        value.prop_map(ModelMatchV1::Exact),
        value.prop_map(ModelMatchV1::Prefix),
        value.prop_map(ModelMatchV1::Contains),
    ]
}

fn capabilities() -> impl Strategy<Value = Map<String, Value>> {
    prop::collection::btree_map("[a-z_]{1,8}", any::<i64>().prop_map(Value::from), 0..4)
        .prop_map(|map| map.into_iter().collect())
}

proptest! {
    #[test]
    fn a_valid_catalog_round_trips(
        entries in prop::collection::vec(
            (prop::collection::vec(rule(), 1..4), capabilities()),
            0..6,
        )
    ) {
        let entries: Vec<_> = entries
            .into_iter()
            .map(|(matches, capabilities)| ModelCatalogEntryV1::new(matches, capabilities))
            .collect();
        match ModelCatalogV1::new(entries) {
            Ok(catalog) => {
                let bytes = serde_json::to_vec(&catalog.to_value()).unwrap();
                prop_assert_eq!(ModelCatalogV1::parse(&bytes).unwrap(), catalog);
            }
            Err(error) => {
                let duplicate = matches!(error, ModelCatalogErrorV1::DuplicateMatchRule { .. });
                prop_assert!(duplicate, "unexpected refusal: {}", error);
            }
        }
    }
}

#[test]
fn the_release_workflow_publishes_and_checksums_every_catalog() {
    let workflow =
        std::fs::read_to_string(repo_root().join(".github/workflows/release.yml")).unwrap();
    for needle in [
        "for catalog in catalogs/*.json",
        r#"dist/$(basename "${catalog}" .json)-${GITHUB_REF_NAME}.json"#,
        r#"./*-"${GITHUB_REF_NAME}".json"#,
        r#"dist/*-"${GITHUB_REF_NAME}".json"#,
    ] {
        assert!(
            workflow.contains(needle),
            "release.yml no longer contains `{needle}`, so a release could ship without the model \
             catalog or without its checksum"
        );
    }
}
