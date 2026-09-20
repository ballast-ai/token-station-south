//! Public request-only estimation behavior and its strict prepared wire boundary.
use proptest::prelude::*;
use serde_json::{Value, json};
use south_component_conformance::{
    TaskComponentV2,
    reference_kling_task_v2::KlingTaskReferenceV2,
    task_v2_json::{parse_prepared_task_json, prepared_task_json},
};
use south_contracts::{HostMintedValuesV1, TaskRequestEstimateV2};
use token_station_protocol::ProviderConfig;

fn prepare(
    request: &Value,
) -> south_component_conformance::ComponentResultV1<south_component_conformance::PreparedTaskV2> {
    let config: ProviderConfig = serde_json::from_value(
        json!({"provider":"kling","base_url":"https://kling.example","models":[]}),
    )
    .unwrap();
    KlingTaskReferenceV2.build_submit_request(
        &config,
        request,
        &HostMintedValuesV1::new("estimate-host-id", None).unwrap(),
    )
}
fn request(model: &str, operation: &str, mode: &str, sound: bool, reference: bool) -> Value {
    let mut value = json!({"model":model,"operation":operation,"mode":mode,"prompt":"scene","sound":if sound {"on"} else {"off"},"duration":"2.001","image_url":"https://cdn/image","video_url":"https://cdn/video","character_orientation":"image"});
    if reference {
        value["video_list"] = json!([]);
    }
    value
}
#[test]
fn estimate_is_request_only_and_uses_explicit_host_seconds() {
    let value = TaskRequestEstimateV2::new(Some(2.0), Some(600)).unwrap();
    assert_eq!(value.estimate_milliunits(3.0001).unwrap(), Some(1801));
    assert_eq!(value.requested_seconds(), Some(2.0));
    assert_eq!(
        TaskRequestEstimateV2::new(None, None).unwrap().estimate_milliunits(5.0).unwrap(),
        None
    );
    assert_eq!(
        TaskRequestEstimateV2::new(None, Some(0)).unwrap().estimate_milliunits(5.0).unwrap(),
        Some(0)
    );
    assert_eq!(value.estimate_milliunits(0.0).unwrap(), Some(0));
}
#[test]
fn estimate_rejects_invalid_facts_host_seconds_and_overflow() {
    for seconds in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(TaskRequestEstimateV2::new(Some(seconds), None).is_err());
        assert!(TaskRequestEstimateV2::default().estimate_milliunits(seconds).is_err());
    }
    assert!(TaskRequestEstimateV2::new(None, Some(-1)).is_err());
    let value = TaskRequestEstimateV2::new(None, Some(1)).unwrap();
    assert!(value.estimate_milliunits(9_223_372_036_854_775_808.0).is_err());
    assert_eq!(
        value.estimate_milliunits(9_223_372_036_854_774_784.0).unwrap(),
        Some(9_223_372_036_854_774_784)
    );
    assert!(
        TaskRequestEstimateV2::new(None, Some(i64::MAX))
            .unwrap()
            .estimate_milliunits(f64::MAX)
            .is_err()
    );
}
#[test]
fn every_managed_rate_card_axis_comes_from_the_final_body() {
    for (model, operation, rates) in [
        (
            "kling-v3",
            "text-or-image",
            [
                Some(600),
                Some(900),
                None,
                None,
                Some(800),
                Some(1200),
                None,
                None,
                Some(3000),
                Some(3000),
                None,
                None,
                None,
                None,
                None,
                None,
            ],
        ),
        (
            "kling-v3-omni",
            "omni",
            [
                Some(600),
                Some(800),
                Some(900),
                None,
                Some(800),
                Some(1000),
                Some(1200),
                None,
                Some(3000),
                Some(3000),
                None,
                None,
                None,
                None,
                None,
                None,
            ],
        ),
        (
            "kling-video-o1",
            "omni",
            [
                Some(600),
                None,
                Some(900),
                None,
                Some(800),
                None,
                Some(1200),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
        ),
    ] {
        for (m, mode) in ["std", "pro", "4k", "unknown"].into_iter().enumerate() {
            for (r, reference) in [false, true].into_iter().enumerate() {
                for (s, sound) in [false, true].into_iter().enumerate() {
                    let prepared =
                        prepare(&request(model, operation, mode, sound, reference)).unwrap();
                    // Ordinary t2v/i2v does not forward video_list at all.
                    let index =
                        if operation == "text-or-image" { m * 4 + s } else { m * 4 + r * 2 + s };
                    assert_eq!(
                        prepared.request_estimate.milliunits_per_second(),
                        rates[index],
                        "{model}/{mode}/{sound}/{reference}"
                    );
                    assert_eq!(prepared.request_estimate.requested_seconds(), Some(2.001));
                }
            }
        }
    }
    for mode in ["std", "pro", "4k", "unknown"] {
        let prepared = prepare(&request("kling-v3", "motion-control", mode, true, true)).unwrap();
        assert_eq!(
            prepared.request_estimate.milliunits_per_second(),
            Some(1200),
            "motion forces pro in the final body"
        );
        assert_eq!(prepared.request_estimate.requested_seconds(), None);
    }
}
#[test]
fn presence_not_nonempty_and_omitted_duration_preserve_managed_semantics() {
    for value in [Value::Null, json!([]), json!([{"refer_type":"feature"}])] {
        let mut input = request("kling-v3-omni", "omni", "std", false, false);
        input["video_list"] = value;
        assert_eq!(prepare(&input).unwrap().request_estimate.milliunits_per_second(), Some(900));
    }
    let mut base = request("kling-v3-omni", "omni", "std", false, false);
    base["video_list"] = json!([{}]);
    let prepared = prepare(&base).unwrap();
    assert!(prepared.descriptor.body.as_ref().unwrap().get("duration").is_none());
    assert_eq!(prepared.request_estimate.requested_seconds(), None);
    assert_eq!(prepared.request_estimate.estimate_milliunits(5.0).unwrap(), Some(4500));
    let mut defaults = json!({"model":"kling-v3","operation":"text-or-image","prompt":"scene"});
    let prepared = prepare(&defaults).unwrap();
    assert_eq!(prepared.request_estimate.requested_seconds(), Some(5.0));
    assert_eq!(prepared.request_estimate.milliunits_per_second(), Some(600));
    defaults["model"] = json!("legacy-model");
    assert_eq!(prepare(&defaults).unwrap().request_estimate.milliunits_per_second(), None);
}
#[test]
fn invalid_present_duration_is_not_silently_zero_or_missing() {
    for duration in
        [json!("NaN"), json!("inf"), json!("-1"), json!(-1), json!(true), json!({}), json!("bad")]
    {
        let mut input = request("kling-v3", "text-or-image", "std", false, false);
        input["duration"] = duration;
        assert!(prepare(&input).is_err());
    }
    let mut input = request("kling-v3", "text-or-image", "std", false, false);
    input["duration"] = Value::Null;
    assert_eq!(prepare(&input).unwrap().request_estimate.requested_seconds(), None);
}
#[test]
fn prepared_codec_requires_and_roundtrips_a_strict_estimate() {
    let wire = json!({"descriptor":{"method":"POST","url":"https://kling.example/task"},"locator":{"schema_version":1,"route":"v1/tasks"},"request_estimate":{"requested_seconds":0,"milliunits_per_second":0}});
    let prepared = parse_prepared_task_json(&wire.to_string()).unwrap();
    assert_eq!(prepared.request_estimate.estimate_milliunits(5.0).unwrap(), Some(0));
    assert_eq!(
        parse_prepared_task_json(&prepared_task_json(&prepared).unwrap().to_string()).unwrap(),
        prepared
    );
    let mut missing = wire.clone();
    missing.as_object_mut().unwrap().remove("request_estimate");
    assert!(parse_prepared_task_json(&missing.to_string()).is_err());
    for invalid in [
        json!({"requested_seconds":-1,"milliunits_per_second":1}),
        json!({"requested_seconds":null,"milliunits_per_second":-1}),
        json!({"requested_seconds":null,"milliunits_per_second":null,"actual_units":2}),
    ] {
        let mut bad = wire.clone();
        bad["request_estimate"] = invalid;
        assert!(parse_prepared_task_json(&bad.to_string()).is_err());
    }
}
proptest! {
    #[test]
    fn validated_estimate_roundtrips_and_calculates_without_panicking(seconds in 0_u32..1_000_000, rate in 0_i64..1_000_000) {
        let estimate=TaskRequestEstimateV2::new(Some(f64::from(seconds)),Some(rate)).unwrap();
        prop_assert_eq!(estimate.estimate_milliunits(f64::from(seconds)).unwrap(),Some(i64::from(seconds)*rate));
        let mut wire=json!({"descriptor":{"method":"POST","url":"https://kling.example/task"},"locator":{"schema_version":1,"route":"v1/tasks"},"request_estimate":{"requested_seconds":seconds,"milliunits_per_second":rate}});
        let prepared=parse_prepared_task_json(&wire.to_string()).unwrap();
        wire=prepared_task_json(&prepared).unwrap();
        prop_assert_eq!(parse_prepared_task_json(&wire.to_string()).unwrap(),prepared);
    }
}
