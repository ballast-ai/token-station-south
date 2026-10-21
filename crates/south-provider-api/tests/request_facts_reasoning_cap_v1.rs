//! `request_facts.reasoning_cap` and `request_facts.cap_overhead` (the Bailian thinking cap record,
//! `docs/design/2026-10-21-bailian-thinking-cap.md` §4): a family that writes a separate reasoning
//! cap says where, and how much the upstream may emit beyond the two caps. Gate ① admits the
//! declaration only in a well-formed shape; absent, both fields stay off the wire.

use std::path::Path;

use serde_json::{Value, json};
use south_provider_api::{
    ComponentManifestV1, MAX_CAP_OVERHEAD, MAX_OUTPUT_CAP_LOCATIONS, ManifestErrorV1,
    RequestFactsV1,
};

fn shipped(package: &str) -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components")
        .join(package)
        .join("manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The `openai-compatible` family's facts, with the two new fields set from `extra`.
fn facts(extra: &Value) -> RequestFactsV1 {
    let mut value = json!({
        "output_cap": ["/max_tokens"],
        "model": {"body": "/model"},
        "stream": {"body": "/stream"},
    });
    for (key, field) in extra.as_object().unwrap() {
        value[key] = field.clone();
    }
    serde_json::from_value(value).unwrap()
}

fn declared(facts: RequestFactsV1) -> Result<(), ManifestErrorV1> {
    let mut manifest = shipped("provider-openai-compatible");
    manifest.request_facts.insert("openai-compatible".to_owned(), facts);
    manifest.validate()
}

#[test]
fn absent_fields_default_and_stay_off_the_wire() {
    let plain = facts(&json!({}));
    assert!(plain.reasoning_cap.is_empty());
    assert_eq!(plain.cap_overhead, 0);
    let wire = serde_json::to_value(&plain).unwrap();
    assert!(wire.get("reasoning_cap").is_none());
    assert!(wire.get("cap_overhead").is_none());
    assert_eq!(RequestFactsV1::top_level().reasoning_cap, Vec::<String>::new());
    assert_eq!(RequestFactsV1::top_level().cap_overhead, 0);
    // Every shipped manifest without the fields still reads and validates.
    for package in ["provider-gemini", "provider-anthropic", "provider-openai-responses"] {
        assert_eq!(shipped(package).validate(), Ok(()), "{package}");
    }
}

#[test]
fn a_reasoning_cap_with_an_overhead_round_trips() {
    let split = facts(&json!({"reasoning_cap": ["/thinking_budget"], "cap_overhead": 3}));
    assert_eq!(split.reasoning_cap, ["/thinking_budget"]);
    assert_eq!(split.cap_overhead, 3);
    assert_eq!(declared(split.clone()), Ok(()));
    let wire = serde_json::to_value(&split).unwrap();
    assert_eq!(wire["reasoning_cap"], json!(["/thinking_budget"]));
    assert_eq!(wire["cap_overhead"], json!(3));
    assert_eq!(serde_json::from_value::<RequestFactsV1>(wire).unwrap(), split);

    // A reasoning cap without an overhead is allowed; the overhead's bound is inclusive.
    assert_eq!(declared(facts(&json!({"reasoning_cap": ["/thinking_budget"]}))), Ok(()));
    assert_eq!(
        declared(facts(
            &json!({"reasoning_cap": ["/thinking_budget"], "cap_overhead": MAX_CAP_OVERHEAD})
        )),
        Ok(())
    );
    let four: Vec<String> =
        (0..MAX_OUTPUT_CAP_LOCATIONS).map(|index| format!("/reasoning/{index}")).collect();
    assert_eq!(declared(facts(&json!({"reasoning_cap": four}))), Ok(()));
}

#[test]
fn an_unknown_key_inside_the_facts_is_still_refused() {
    let refused = serde_json::from_value::<RequestFactsV1>(json!({
        "output_cap": ["/max_tokens"],
        "model": {"body": "/model"},
        "stream": {"body": "/stream"},
        "reasoning_caps": ["/thinking_budget"],
    }));
    assert!(refused.is_err());
}

#[test]
fn gate_one_refuses_a_malformed_reasoning_cap() {
    let five: Vec<String> =
        (0..=MAX_OUTPUT_CAP_LOCATIONS).map(|index| format!("/reasoning/{index}")).collect();
    for (bad, why) in [
        (json!({"reasoning_cap": ["thinking_budget"]}), "not a pointer"),
        (json!({"reasoning_cap": ["/a//b"]}), "an empty reference token"),
        (json!({"reasoning_cap": ["/thinking_budget", "/thinking_budget"]}), "a repeat"),
        (json!({"reasoning_cap": five}), "more than four"),
        (json!({"reasoning_cap": ["/max_tokens"]}), "shared with output_cap"),
        (json!({"output_cap": [], "reasoning_cap": ["/thinking_budget"]}), "no answer cap"),
        (json!({"cap_overhead": 3}), "an overhead without a reasoning cap"),
        (
            json!({"reasoning_cap": ["/thinking_budget"], "cap_overhead": MAX_CAP_OVERHEAD + 1}),
            "an overhead past the bound",
        ),
    ] {
        assert!(
            matches!(declared(facts(&bad)), Err(ManifestErrorV1::InvalidRequestFacts { .. })),
            "{why}: {bad}"
        );
    }
}
