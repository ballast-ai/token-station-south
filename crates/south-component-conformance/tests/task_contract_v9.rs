//! Task contract 9 (the served models record,
//! `docs/design/2026-10-28-served-models-prefetch-and-failure-category.md` §4–§5; ruled FC-Q1–Q3 by
//! lv on 2026-10-29): a failed observation may carry a `category`, set only from an explicit
//! upstream signal.
//!
//! The key is optional and written only when present, so an earlier failure re-encodes byte for
//! byte. A decoder before contract 9 refuses the key, so only a package that emits it declares 9,
//! and gate ② (`failure_category_declared`) holds a package to that both ways.

use serde_json::json;
use south_component_conformance::task_v2_json::{observation_json, parse_observation_json};
use south_component_conformance::{
    CheckV1, TaskComponentV2, TaskFixturePackV2, VerdictV1,
    reference_minimax_task_v2::MiniMaxTaskReferenceV2, reference_veo_task_v2::VeoTaskComponentV2,
    run_task_component_suite_v2_for_manifest,
};
use south_contracts::{
    TASK_CONTRACT_VERSION, TASK_CONTRACT_VERSIONS, TaskFailureCategoryV1, TaskFailureKindV1,
    TaskObservationV2,
};
use south_provider_api::ComponentManifestV1;
use std::path::Path;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).unwrap()
}

fn manifest(package: &str) -> ComponentManifestV1 {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("components").join(package).join("manifest.json"))
            .unwrap(),
    )
    .unwrap()
}

fn pack(directory: &str) -> TaskFixturePackV2 {
    TaskFixturePackV2::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join(directory)).unwrap()
}

fn category_outcomes(
    component: &dyn TaskComponentV2,
    pack: &TaskFixturePackV2,
    manifest: &ComponentManifestV1,
) -> Vec<(String, bool)> {
    run_task_component_suite_v2_for_manifest(component, pack, manifest)
        .outcomes()
        .iter()
        .filter(|outcome| outcome.check == CheckV1::FailureCategoryDeclared)
        .map(|outcome| (outcome.case.clone(), outcome.verdict == VerdictV1::Passed))
        .collect()
}

#[test]
fn the_contract_version_is_nine_and_seven_and_eight_still_decode() {
    assert_eq!(TASK_CONTRACT_VERSION, 9);
    assert_eq!(TASK_CONTRACT_VERSIONS, [7, 8, 9]);
    assert_eq!(
        TaskFailureCategoryV1::ALL.map(TaskFailureCategoryV1::word),
        ["content_policy", "upstream_timeout"]
    );
}

#[test]
fn a_category_round_trips_and_absence_is_not_written() {
    for category in TaskFailureCategoryV1::ALL {
        let wire = json!({"state": "failed", "kind": "failed", "code": "1026", "message": "m",
            "category": category.word()});
        let observation = parse_observation_json(&wire.to_string()).unwrap();
        assert_eq!(
            observation,
            TaskObservationV2::Failed {
                kind: TaskFailureKindV1::Failed,
                code: Some("1026".into()),
                message: Some("m".into()),
                category: Some(category),
            }
        );
        assert_eq!(observation_json(&observation).unwrap(), wire);
    }
    // A contract 7 or 8 failure: no key, decoded as no category, re-encoded byte for byte.
    let earlier =
        json!({"state": "failed", "kind": "provider-expired", "code": null, "message": null});
    let observation = parse_observation_json(&earlier.to_string()).unwrap();
    assert!(matches!(observation, TaskObservationV2::Failed { category: None, .. }));
    assert_eq!(observation_json(&observation).unwrap(), earlier);
    // `null` reads as absent, like the other optional keys.
    let null = json!({"state": "failed", "kind": "failed", "code": null, "message": null,
        "category": null});
    assert!(matches!(
        parse_observation_json(&null.to_string()).unwrap(),
        TaskObservationV2::Failed { category: None, .. }
    ));
}

#[test]
fn the_category_vocabulary_is_closed() {
    for word in ["artifact_unavailable", "cancelled", "content-policy", "CONTENT_POLICY", ""] {
        let wire = json!({"state": "failed", "kind": "failed", "code": null, "message": null,
            "category": word});
        assert!(parse_observation_json(&wire.to_string()).is_err(), "{word}");
    }
    let not_a_string = json!({"state": "failed", "kind": "failed", "code": null, "message": null,
        "category": 1});
    assert!(parse_observation_json(&not_a_string.to_string()).is_err());
}

/// Veo reports `content_policy` from `raiMediaFilteredCount` and nothing for an operation error;
/// it is the one package on contract 9.
#[test]
fn veo_declares_nine_and_its_rai_filter_is_the_category_fixture() {
    let veo = manifest("task-veo-v2");
    assert_eq!(veo.compatibility.contracts.get("task"), Some(&9));
    let outcomes = category_outcomes(&VeoTaskComponentV2, &pack("fixtures-veo-task-v2"), &veo);
    assert_eq!(outcomes, [("task-v2.observation.rai-filtered".to_owned(), true)]);

    let mut seven = veo;
    seven.compatibility.contracts.insert("task".to_owned(), 7);
    let outcomes = category_outcomes(&VeoTaskComponentV2, &pack("fixtures-veo-task-v2"), &seven);
    assert_eq!(outcomes, [("task-v2.observation.rai-filtered".to_owned(), false)]);
}

/// A package that declares 9 but never reports a category is refused, so declaring the newer
/// contract stays tied to emitting it (FC-Q1). `MiniMax` reports none this round (§8 I-5).
#[test]
fn declaring_nine_without_a_category_fixture_fails() {
    let mut minimax = manifest("task-minimax-v2");
    assert_eq!(minimax.compatibility.contracts.get("task"), Some(&7));
    let pack = pack("fixtures-minimax-task-v2");
    assert!(category_outcomes(&MiniMaxTaskReferenceV2, &pack, &minimax).is_empty());
    minimax.compatibility.contracts.insert("task".to_owned(), 9);
    let outcomes = category_outcomes(&MiniMaxTaskReferenceV2, &pack, &minimax);
    assert_eq!(outcomes.len(), 1);
    assert!(!outcomes[0].1);
}
