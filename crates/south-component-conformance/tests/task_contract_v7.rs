//! Task contract 7 (docs/design/2026-09-28-task-contract-v7-artifact-role.md): every direct
//! artifact states the part it plays. The `role` key is required and `null` names the primary
//! role; a word outside the closed vocabulary is refused rather than defaulted, and a set that
//! carries no primary artifact is refused because the host would have nothing to deliver.

use serde_json::{Value, json};
use south_component_conformance::task_v2_json::*;
use south_contracts::{
    TASK_CONTRACT_VERSION, TaskArtifactRefV2, TaskArtifactRoleV2, TaskArtifactV2,
    TaskObservationV2, TaskScalarV2, TaskUsageFactsV2,
};

const VIDEO: &str = "https://cdn.example/a.mp4";
const FRAME: &str = "https://cdn.example/a-last.png";

fn item(url: &str, role: &Value) -> Value {
    json!({"url": url, "id": null, "duration": null, "fetch_with_credential": false, "role": role})
}

fn succeeded(items: &[Value], outputs: &Value) -> String {
    json!({"state": "succeeded", "artifacts": {"kind": "urls", "items": items},
        "usage": {"seconds": null, "milliunits": null, "tokens": null, "outputs": outputs}})
    .to_string()
}

fn video() -> TaskArtifactV2 {
    TaskArtifactV2::new(VIDEO, TaskScalarV2::Null, TaskScalarV2::Null).unwrap()
}

fn frame() -> TaskArtifactV2 {
    TaskArtifactV2::new(FRAME, TaskScalarV2::Null, TaskScalarV2::Null)
        .unwrap()
        .with_role(TaskArtifactRoleV2::LastFrame)
}

#[test]
fn the_contract_version_is_seven() {
    assert_eq!(TASK_CONTRACT_VERSION, 7);
}

#[test]
fn a_contract_six_artifact_without_the_role_key_is_refused() {
    let mut six = item(VIDEO, &Value::Null);
    six.as_object_mut().unwrap().remove("role");
    assert!(parse_observation_json(&succeeded(&[six], &json!(1))).is_err(), "缺 role 键必须拒");
}

#[test]
fn a_role_outside_the_closed_vocabulary_is_refused() {
    for bad in [json!("primary"), json!("thumbnail"), json!("LAST_FRAME"), json!(1), json!(true)] {
        assert!(
            parse_observation_json(&succeeded(&[item(VIDEO, &bad)], &json!(1))).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn roles_round_trip_and_null_names_the_primary() {
    let wire =
        succeeded(&[item(VIDEO, &Value::Null), item(FRAME, &json!("last_frame"))], &json!(1));
    let observation = parse_observation_json(&wire).unwrap();
    let TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::Urls(items), usage } =
        &observation
    else {
        panic!("urls expected")
    };
    assert_eq!(items[0].role(), TaskArtifactRoleV2::Primary);
    assert_eq!(items[1].role(), TaskArtifactRoleV2::LastFrame);
    assert_eq!(items[1].url(), FRAME);
    // Delivered outputs are the upstream's count of primaries; a companion does not raise it.
    assert_eq!(usage.outputs(), Some(1));
    let back = observation_json(&observation).unwrap();
    assert_eq!(back, serde_json::from_str::<Value>(&wire).unwrap(), "编码后逐字节同形");
    assert_eq!(parse_observation_json(&back.to_string()).unwrap(), observation);
}

#[test]
fn a_set_made_only_of_companions_is_refused() {
    assert!(
        parse_observation_json(&succeeded(&[item(FRAME, &json!("last_frame"))], &json!(1)))
            .is_err(),
        "只有尾帧、没有主产物:宿主无物可交付"
    );
    assert!(TaskArtifactRefV2::urls(vec![frame()]).is_err());
    let smuggled = TaskObservationV2::Succeeded {
        artifacts: TaskArtifactRefV2::Urls(vec![frame()]),
        usage: TaskUsageFactsV2::default(),
    };
    assert!(smuggled.validate().is_err(), "绕过构造器也过不了边界校验");
    assert!(observation_json(&smuggled).is_err());
    // Any order is fine as long as one primary is present.
    assert!(TaskArtifactRefV2::urls(vec![frame(), video()]).is_ok());
}

#[test]
fn a_companion_counts_toward_the_artifact_bound() {
    let mut items = vec![video(); south_contracts::MAX_ARTIFACT_URLS];
    assert!(TaskArtifactRefV2::urls(items.clone()).is_ok());
    items.push(frame());
    assert!(TaskArtifactRefV2::urls(items).is_err(), "尾帧也计入 MAX_ARTIFACT_URLS");
}

#[test]
fn builders_keep_the_primary_default_and_roles_spell_themselves() {
    assert_eq!(video().role(), TaskArtifactRoleV2::Primary, "缺省是主产物");
    assert!(video().role().is_primary());
    assert!(!frame().role().is_primary());
    assert_eq!(frame().with_bound_credential().role(), TaskArtifactRoleV2::LastFrame);
    assert!(frame().with_bound_credential().fetch_with_credential());
    assert!(format!("{:?}", frame()).contains("LastFrame"));
    assert!(!format!("{:?}", frame()).contains("a-last.png"));
    for role in TaskArtifactRoleV2::ALL {
        assert_eq!(TaskArtifactRoleV2::from_word(role.word()), Ok(role));
    }
    assert_eq!(TaskArtifactRoleV2::from_word(None), Ok(TaskArtifactRoleV2::Primary));
    assert_eq!(
        TaskArtifactRoleV2::from_word(Some("last_frame")),
        Ok(TaskArtifactRoleV2::LastFrame)
    );
    assert!(TaskArtifactRoleV2::from_word(Some("primary")).is_err());
    assert!(TaskArtifactRoleV2::from_word(Some("")).is_err());
}

#[test]
fn an_accepted_terminal_receipt_carries_roles_through_the_submit_codec() {
    let observation: Value = serde_json::from_str(&succeeded(
        &[item(VIDEO, &Value::Null), item(FRAME, &json!("last_frame"))],
        &json!(1),
    ))
    .unwrap();
    let wire = json!({"outcome": "accepted-terminal", "observation": observation});
    let outcome = parse_submit_outcome_json(&wire.to_string()).unwrap();
    assert_eq!(submit_outcome_json(&outcome).unwrap(), wire);
    // Dropping the role key on any one item is a contract-6 shape again: refused.
    let mut sixish = wire;
    sixish["observation"]["artifacts"]["items"][1].as_object_mut().unwrap().remove("role");
    assert!(parse_submit_outcome_json(&sixish.to_string()).is_err());
}
