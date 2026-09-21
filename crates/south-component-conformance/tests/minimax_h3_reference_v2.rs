//! H3 request facts and recovery behavior transcribed from the managed host.
use serde_json::{Value, json};
use south_component_conformance::{
    TaskComponentV2, reference_minimax_task_v2::MiniMaxTaskReferenceV2,
};
use south_contracts::{HostMintedValuesV1, TaskLocatorV2, TaskObservationV2};
use token_station_protocol::{HttpResponseParts, ProviderConfig};
fn config(group: bool) -> ProviderConfig {
    let mut value = json!({"provider":"minimax","base_url":"https://api.minimax.io","auth":"provider_api_key","upstream_model_family":"MiniMax-H3"});
    if group {
        value["group_id"] = json!("19000");
    }
    serde_json::from_value(value).unwrap()
}
fn parts(value: &Value) -> HttpResponseParts {
    serde_json::from_value(json!({"status":200,"headers":{},"body":value.to_string()})).unwrap()
}
#[test]
fn actual_wire_fields_supply_all_estimation_facts_for_aliases() {
    for images in 0..=9 {
        let request = json!({"model":"minimax-h3-turbo-v4","prompt":"scene","reference_images":vec!["https://cdn/img";images],"resolution":" 2 k ","duration":"6.0"});
        let p = MiniMaxTaskReferenceV2
            .build_submit_request(
                &config(false),
                &request,
                &HostMintedValuesV1::new("h3", None).unwrap(),
            )
            .unwrap();
        let body = p.descriptor.body.as_ref().unwrap();
        assert_eq!(body["model"], "minimax-h3-turbo-v4");
        assert_eq!(body["content"].as_array().unwrap().len(), images + 1);
        assert_eq!(p.request_estimate.resolution(), Some("2K"));
        assert_eq!(p.request_estimate.input_image_count(), Some(u32::try_from(images).unwrap()));
        assert_eq!(p.request_estimate.requested_seconds(), Some(6.0));
        assert_eq!(p.request_estimate.milliunits_per_second(), None);
        config(false).authorize(&p.descriptor).unwrap();
    }
}
#[test]
fn h3_locator_is_stable_across_model_drift_and_uses_original_encoded_id() {
    let locator = TaskLocatorV2::new(1, "v2/query/video_generation").unwrap();
    for group in [true, false] {
        let cfg = config(group);
        let req = MiniMaxTaskReferenceV2
            .build_observe_request(&cfg, "MiniMax-Hailuo-02", "id?x#%", &locator)
            .unwrap();
        assert!(req.url.contains("/v2/query/video_generation/id%3Fx%23%25"));
        assert_eq!(req.url.contains("GroupId=19000"), group);
        cfg.authorize(&req).unwrap();
        for id in ["id/slash", "id\\backslash"] {
            let req = MiniMaxTaskReferenceV2
                .build_observe_request(&cfg, "changed", id, &locator)
                .unwrap();
            assert!(cfg.authorize(&req).is_err());
        }
    }
}
#[test]
fn output_seconds_distinguishes_missing_zero_valid_and_invalid() {
    for (seconds, expected) in
        [(Value::Null, None), (json!(0), Some(0.0)), (json!(" 6.5 "), Some(6.5))]
    {
        let obs=MiniMaxTaskReferenceV2.parse_observation(&parts(&json!({"task":{"status":"succeeded","content":{"url":"https://cdn/result"},"usage":{"output_seconds":seconds}}}))).unwrap();
        let TaskObservationV2::Succeeded { usage, .. } = obs else {
            panic!("valid terminal observation")
        };
        assert_eq!(usage.seconds(), expected);
        assert_eq!(usage.milliunits(), None);
        assert_eq!(usage.tokens(), None);
    }
    for invalid in [json!(-1), json!("NaN"), json!("inf"), json!({}), json!(true)] {
        let obs=MiniMaxTaskReferenceV2.parse_observation(&parts(&json!({"task":{"status":"succeeded","content":{"url":"https://cdn/result"},"usage":{"output_seconds":invalid}}}))).unwrap();
        assert!(matches!(obs, TaskObservationV2::Unknown { .. }));
    }
}
#[test]
fn normalized_hailuo_resolution_and_images_are_also_explicit() {
    let mut cfg = config(false);
    cfg.extensions.remove("upstream_model_family");
    let p=MiniMaxTaskReferenceV2.build_submit_request(&cfg,&json!({"model":"MiniMax-Hailuo-02","prompt":"scene","resolution":" 1080 p ","image":"first","last_frame":"last"}),&HostMintedValuesV1::new("v1",None).unwrap()).unwrap();
    assert_eq!(p.request_estimate.resolution(), Some("1080P"));
    assert_eq!(p.request_estimate.input_image_count(), Some(2));
}

#[test]
fn dot_segment_ids_never_produce_a_query_descriptor() {
    let locator = TaskLocatorV2::new(1, "v2/query/video_generation").unwrap();
    for id in [".", ".."] {
        let error = MiniMaxTaskReferenceV2
            .build_observe_request(&config(false), "MiniMax-H3", id, &locator)
            .expect_err("dot segments cannot identify a task path");
        assert_eq!(error.message, "invalid MiniMax H3 task id");
    }
}
#[test]
fn dot_segment_submit_ids_remain_unknown_instead_of_becoming_unqueryable_tasks() {
    use south_component_conformance::SubmitOutcomeV2;
    for id in [".", "..", " .. "] {
        let outcome =
            MiniMaxTaskReferenceV2.parse_submit_response(&parts(&json!({"task_id":id}))).unwrap();
        assert_eq!(outcome, SubmitOutcomeV2::Unknown);
    }
}

#[test]
fn accepted_id_and_business_rejection_are_unknown_until_reconciled() {
    use south_component_conformance::SubmitOutcomeV2;
    let ambiguous = parts(
        &json!({"task_id":"accepted-id","base_resp":{"status_code":1002,"status_msg":"untrusted provider text"}}),
    );
    assert_eq!(
        MiniMaxTaskReferenceV2.parse_submit_response(&ambiguous).unwrap(),
        SubmitOutcomeV2::Unknown
    );
    let rejected =
        parts(&json!({"base_resp":{"status_code":1002,"status_msg":"untrusted provider text"}}));
    let SubmitOutcomeV2::Rejected(error) =
        MiniMaxTaskReferenceV2.parse_submit_response(&rejected).unwrap()
    else {
        panic!("an explicit refusal without an accepted id stays rejected")
    };
    assert_eq!(error.http_status, 429);
    assert_eq!(error.message, "MiniMax rate limit exceeded");
}
