//! Task contract 6 (docs/design/2026-09-27-task-contract-v6-facts.md): token rate and output
//! counts, delivered outputs, artifact credential fetch and immutable body paths. Every new key
//! must be present (null where the fact is absent); a contract-5 value is refused, not defaulted.

use serde_json::{Value, json};
use south_component_conformance::task_v2_json::*;
use south_contracts::{
    TASK_CONTRACT_VERSION, TaskArtifactRefV2, TaskArtifactV2, TaskObservationV2,
    TaskRequestEstimateV2, TaskScalarV2, TaskUsageFactsV2,
};

fn estimate(extra: Value) -> Value {
    let mut estimate = json!({"requested_seconds": 5.0, "milliunits_per_second": null,
        "resolution": null, "input_image_count": null,
        "tokens_per_second": null, "requested_outputs": null});
    let Value::Object(extra) = extra else { panic!("extra must be an object") };
    for (key, value) in extra {
        estimate[key] = value;
    }
    estimate
}

fn prepared(estimate: Value, immutable: Value) -> String {
    let mut out = json!({"descriptor": {"method": "POST", "url": "https://upstream.example/tasks",
                                        "body": {"duration": 5}},
                         "locator": {"schema_version": 1, "route": "v1/tasks"}});
    out["request_estimate"] = estimate;
    out["immutable_body_paths"] = immutable;
    out.to_string()
}

fn succeeded(item_extra: Value, usage: Value) -> String {
    let mut item = json!({"url": "https://cdn.example/a.mp4", "id": null, "duration": null});
    let Value::Object(item_extra) = item_extra else { panic!("item_extra must be an object") };
    for (key, value) in item_extra {
        item[key] = value;
    }
    let mut out = json!({"state": "succeeded", "artifacts": {"kind": "urls", "items": [item]}});
    out["usage"] = usage;
    out.to_string()
}

#[test]
fn the_contract_version_is_six() {
    assert_eq!(TASK_CONTRACT_VERSION, 6);
}

#[test]
fn a_contract_five_prepared_value_is_refused() {
    let v5 = json!({"descriptor": {"method": "POST", "url": "https://upstream.example/tasks",
                                   "body": {"x": 1}},
                    "locator": {"schema_version": 1, "route": "v1/tasks"},
                    "request_estimate": {"requested_seconds": null, "milliunits_per_second": null,
                                         "resolution": null, "input_image_count": null}});
    assert!(parse_prepared_task_json(&v5.to_string()).is_err(), "合同 5 形状必须拒");
    // 各缺一个新键也拒:「必须出现、可为 null」。
    for missing in ["tokens_per_second", "requested_outputs"] {
        let mut value = estimate(json!({}));
        value.as_object_mut().unwrap().remove(missing);
        assert!(parse_prepared_task_json(&prepared(value, Value::Null)).is_err(), "缺 {missing}");
    }
    let mut no_paths: Value =
        serde_json::from_str(&prepared(estimate(json!({})), Value::Null)).unwrap();
    no_paths.as_object_mut().unwrap().remove("immutable_body_paths");
    assert!(parse_prepared_task_json(&no_paths.to_string()).is_err(), "缺 immutable_body_paths");
}

#[test]
fn output_facts_round_trip_and_null_differs_from_one() {
    let wire = prepared(
        estimate(json!({"tokens_per_second": 4_860, "requested_outputs": 2})),
        json!(["duration", "parameters.resolution"]),
    );
    let value = parse_prepared_task_json(&wire).unwrap();
    assert_eq!(value.request_estimate.tokens_per_second(), Some(4_860));
    assert_eq!(value.request_estimate.requested_outputs(), Some(2));
    assert_eq!(
        value.immutable_body_paths.as_deref(),
        Some(&["duration".to_owned(), "parameters.resolution".to_owned()][..])
    );
    let back = prepared_task_json(&value).unwrap();
    assert_eq!(parse_prepared_task_json(&back.to_string()).unwrap(), value);

    let silent = parse_prepared_task_json(&prepared(estimate(json!({})), Value::Null)).unwrap();
    assert_eq!(silent.request_estimate.requested_outputs(), None);
    assert_eq!(silent.immutable_body_paths, None, "null = 组件不表态");
    let open = parse_prepared_task_json(&prepared(estimate(json!({})), json!([]))).unwrap();
    assert_eq!(open.immutable_body_paths, Some(Vec::new()), "[] 与 null 是不同事实");
}

#[test]
fn invalid_output_facts_and_paths_are_refused() {
    for bad in [
        json!({"tokens_per_second": -1}),
        json!({"requested_outputs": 0}),
        json!({"requested_outputs": -2}),
        json!({"tokens_per_second": 1.5}),
    ] {
        assert!(
            parse_prepared_task_json(&prepared(estimate(bad.clone()), Value::Null)).is_err(),
            "{bad}"
        );
    }
    let too_many: Vec<String> = (0..65).map(|i| format!("f{i}")).collect();
    for bad in [
        json!(["a..b"]),
        json!(["items[0]"]),
        json!([""]),
        json!(["dup", "dup"]),
        json!([format!("a{}", "b".repeat(256))]),
        json!(too_many),
        json!("duration"),
    ] {
        assert!(
            parse_prepared_task_json(&prepared(estimate(json!({})), bad.clone())).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn estimate_tokens_mirrors_the_milliunit_rule() {
    let estimate = TaskRequestEstimateV2::new(Some(5.0), None)
        .unwrap()
        .with_output_facts(Some(1_000), Some(1))
        .unwrap();
    assert_eq!(estimate.estimate_tokens(5.0), Ok(Some(5_000)));
    assert_eq!(estimate.estimate_tokens(0.2501), Ok(Some(251)), "向上取整");
    assert!(estimate.estimate_tokens(-1.0).is_err());
    let silent = TaskRequestEstimateV2::new(Some(5.0), None).unwrap();
    assert_eq!(silent.estimate_tokens(5.0), Ok(None));
    assert!(
        TaskRequestEstimateV2::new(None, None).unwrap().with_output_facts(Some(-1), None).is_err()
    );
    assert!(
        TaskRequestEstimateV2::new(None, None).unwrap().with_output_facts(None, Some(0)).is_err()
    );
}

#[test]
fn delivered_outputs_and_credential_fetch_are_required_facts() {
    let usage = json!({"seconds": 4.0, "milliunits": null, "tokens": null, "outputs": 2});
    let value =
        parse_observation_json(&succeeded(json!({"fetch_with_credential": true}), usage.clone()))
            .unwrap();
    let TaskObservationV2::Succeeded { artifacts, usage: facts } = &value else {
        panic!("succeeded expected")
    };
    assert_eq!(facts.outputs(), Some(2));
    let TaskArtifactRefV2::Urls(items) = artifacts else { panic!("urls expected") };
    assert!(items[0].fetch_with_credential());
    let back = observation_json(&value).unwrap();
    assert_eq!(parse_observation_json(&back.to_string()).unwrap(), value);

    // 缺 outputs / 缺 fetch_with_credential:合同 5 形状,拒。
    let v5_usage = json!({"seconds": 4.0, "milliunits": null, "tokens": null});
    assert!(
        parse_observation_json(&succeeded(json!({"fetch_with_credential": false}), v5_usage))
            .is_err()
    );
    assert!(parse_observation_json(&succeeded(json!({}), usage.clone())).is_err());
    // 非法值拒。
    assert!(
        parse_observation_json(&succeeded(json!({"fetch_with_credential": "yes"}), usage)).is_err()
    );
    let negative = json!({"seconds": null, "milliunits": null, "tokens": null, "outputs": -1});
    assert!(
        parse_observation_json(&succeeded(json!({"fetch_with_credential": false}), negative))
            .is_err()
    );
    // 缺席与 0 是不同事实。
    let zero = json!({"seconds": null, "milliunits": null, "tokens": null, "outputs": 0});
    let zero =
        parse_observation_json(&succeeded(json!({"fetch_with_credential": false}), zero)).unwrap();
    let TaskObservationV2::Succeeded { usage, .. } = zero else { panic!() };
    assert_eq!(usage.outputs(), Some(0));
}

#[test]
fn builders_keep_contract_five_defaults() {
    let artifact =
        TaskArtifactV2::new("https://cdn.example/a", TaskScalarV2::Null, TaskScalarV2::Null)
            .unwrap();
    assert!(!artifact.fetch_with_credential(), "缺省自带访问能力");
    assert!(artifact.with_bound_credential().fetch_with_credential());
    let usage = TaskUsageFactsV2::new(Some(1.0), None, None).unwrap();
    assert_eq!(usage.outputs(), None);
    assert_eq!(usage.with_outputs(Some(3)).unwrap().outputs(), Some(3));
    assert!(TaskUsageFactsV2::new(None, None, None).unwrap().with_outputs(Some(-1)).is_err());
}
