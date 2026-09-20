//! Public behavior transcribed from the server's Hailuo v1 implementation.
use serde_json::{Value, json};
use south_component_conformance::{
    SubmitOutcomeV2, TaskComponentV2, reference_minimax_task_v2::MiniMaxTaskReferenceV2,
};
use south_contracts::{
    HostMintedValuesV1, TaskArtifactRefV2, TaskLocatorV2, TaskObservationV2, TaskRenderContextV2,
};
use token_station_protocol::{Auth, HttpResponseParts, ProviderConfig, SecretRef};
fn config(group: Option<&str>) -> ProviderConfig {
    let mut value = json!({"provider":"minimax","base_url":if group.is_some(){"https://api.minimaxi.com"}else{"https://api.minimax.io"},"auth":"provider_api_key"});
    if let Some(group) = group {
        value["group_id"] = json!(group);
    }
    serde_json::from_value(value).unwrap()
}
#[expect(
    clippy::needless_pass_by_value,
    reason = "test callers pass disposable owned JSON fixtures"
)]
fn parts(status: u16, body: Value) -> HttpResponseParts {
    serde_json::from_value(json!({"status":status,"headers":{},"body":body.to_string()})).unwrap()
}
fn locator() -> TaskLocatorV2 {
    TaskLocatorV2::new(1, "v1/query/video_generation").unwrap()
}
#[expect(
    clippy::needless_pass_by_value,
    reason = "test callers pass disposable owned JSON fixtures"
)]
fn prepare(
    value: Value,
) -> south_component_conformance::ComponentResultV1<south_component_conformance::PreparedTaskV2> {
    MiniMaxTaskReferenceV2.build_submit_request(
        &config(None),
        &value,
        &HostMintedValuesV1::new("host-1", None).unwrap(),
    )
}
#[test]
fn prepare_preserves_fields_defaults_and_normalized_estimate() {
    let p=prepare(json!({"model":"MiniMax-Hailuo-02","prompt":"scene","image_url":"first","last_frame_url":"last","duration":"6.0","resolution":" 768p ","prompt_optimizer":false,"fast_pretreatment":true,"ignored":"not-forwarded"})).unwrap();
    assert_eq!(
        p.descriptor.body,
        Some(
            json!({"model":"MiniMax-Hailuo-02","prompt":"scene","first_frame_image":"first","last_frame_image":"last","duration":6,"resolution":"768P","prompt_optimizer":false,"fast_pretreatment":true})
        )
    );
    assert_eq!(p.locator, locator());
    assert_eq!(p.request_estimate.requested_seconds(), Some(6.0));
    assert_eq!(p.request_estimate.milliunits_per_second(), None);
    let p = prepare(json!({"model":"MiniMax-Hailuo-2.3","prompt":""})).unwrap();
    assert_eq!(p.descriptor.body.unwrap()["duration"], 6);
}
#[test]
fn prepare_rejects_wrong_family_invalid_shape_and_frame_modes() {
    for value in [
        json!({"model":"MiniMax-H3","prompt":"x"}),
        json!({"model":"MiniMax-Hailuo-02"}),
        json!({"model":"MiniMax-Hailuo-2.3","last_frame":"last"}),
        json!({"model":"MiniMax-Hailuo-02","last_frame":"last","resolution":"512P"}),
        json!({"model":"MiniMax-Hailuo-02","prompt":"x","duration":1.5}),
        json!({"model":"MiniMax-Hailuo-02","prompt":"x","duration":-1}),
    ] {
        assert!(prepare(value).is_err());
    }
}
#[test]
fn query_and_file_lookup_keep_secret_refs_and_group_query_authorized() {
    for group in [None, Some("19000")] {
        let cfg = config(group);
        let p = MiniMaxTaskReferenceV2
            .build_submit_request(
                &cfg,
                &json!({"model":"MiniMax-Hailuo-02","prompt":"cat"}),
                &HostMintedValuesV1::new("host-1", None).unwrap(),
            )
            .unwrap();
        let q = MiniMaxTaskReferenceV2
            .build_observe_request(&cfg, "MiniMax-Hailuo-02", "001234", &p.locator)
            .unwrap();
        let obs = MiniMaxTaskReferenceV2
            .parse_observation(&parts(200, json!({"status":"Success","file_id":"005678"})))
            .unwrap();
        let f =
            MiniMaxTaskReferenceV2.build_artifact_request(&cfg, &p.locator, &obs).unwrap().unwrap();
        for d in [&p.descriptor, &q, &f] {
            cfg.authorize(d).unwrap();
            assert_eq!(d.auth, Some(Auth::bearer(SecretRef::new("provider_api_key"))));
            assert_eq!(d.url.contains("GroupId=19000"), group.is_some());
            let mut wrong = cfg.clone();
            wrong.auth = Some(SecretRef::new("another-slot"));
            assert!(wrong.authorize(d).is_err());
        }
        assert!(q.url.contains("task_id=001234"));
        assert!(f.url.contains("file_id=005678"));
        assert!(q.body.is_none());
        assert!(f.body.is_none());
    }
}
#[test]
fn submit_distinguishes_rejection_from_acceptance_uncertainty() {
    assert!(
        matches!(MiniMaxTaskReferenceV2.parse_submit_response(&parts(200,json!({"task_id":"00123","base_resp":{"status_code":0}}))).unwrap(),SubmitOutcomeV2::Accepted(id) if id=="00123")
    );
    assert!(matches!(
        MiniMaxTaskReferenceV2
            .parse_submit_response(&parts(
                200,
                json!({"base_resp":{"status_code":1008,"status_msg":"insufficient balance"}})
            ))
            .unwrap(),
        SubmitOutcomeV2::Rejected(_)
    ));
    for p in [
        parts(503, json!({"task_id":"123"})),
        parts(200, json!({})),
        parts(200, json!({"task_id":""})),
    ] {
        assert!(matches!(
            MiniMaxTaskReferenceV2.parse_submit_response(&p).unwrap(),
            SubmitOutcomeV2::Unknown
        ));
    }
}
#[test]
fn observation_preserves_execution_without_inventing_usage() {
    for (word, running) in [("Preparing", false), ("Queueing", false), ("Processing", true)] {
        assert!(
            matches!(MiniMaxTaskReferenceV2.parse_observation(&parts(200,json!({"status":word}))).unwrap(),TaskObservationV2::Progress{running:r,..} if r==running)
        );
    }
    let obs = MiniMaxTaskReferenceV2
        .parse_observation(&parts(200, json!({"status":"Success","file_id":123_456})))
        .unwrap();
    let TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::FileId(id), usage } = obs
    else {
        panic!("expected file id")
    };
    assert_eq!(id, "123456");
    assert_eq!(usage.seconds(), None);
    assert_eq!(usage.milliunits(), None);
    assert_eq!(usage.tokens(), None);
    for v in [
        json!({"status":"Success"}),
        json!({"status":"Success","file_id":{}}),
        json!({"status":"Success","file_id":-1}),
        json!({"status":"other"}),
        json!({"status":"Fail","base_resp":{"status_code":9}}),
    ] {
        assert!(matches!(
            MiniMaxTaskReferenceV2.parse_observation(&parts(200, v)).unwrap(),
            TaskObservationV2::Unknown { .. }
        ));
    }
}
#[test]
fn render_requires_fetch_and_uses_host_identity_and_time() {
    let obs = MiniMaxTaskReferenceV2
        .parse_observation(&parts(200, json!({"status":"Success","file_id":"123"})))
        .unwrap();
    let cx =
        TaskRenderContextV2::new("host-1", 123, "public-model", "minimax-alias", Some("00123"))
            .unwrap();
    assert!(MiniMaxTaskReferenceV2.render_success(&obs, None, &cx).is_err());
    let fetched = parts(
        200,
        json!({"file":{"download_url":"https://cdn.example/video.mp4"},"base_resp":{"status_code":0}}),
    );
    assert_eq!(
        MiniMaxTaskReferenceV2.render_success(&obs, Some(&fetched), &cx).unwrap(),
        json!({"created":123,"model":"public-model","provider":"minimax-alias","task_id":"00123","data":[{"url":"https://cdn.example/video.mp4"}]})
    );
    for f in [
        parts(500, json!({})),
        parts(200, json!({"file":{"download_url":""}})),
        parts(
            200,
            json!({"file":{"download_url":"https://cdn.example/video"},"base_resp":{"status_code":9}}),
        ),
    ] {
        assert!(MiniMaxTaskReferenceV2.render_success(&obs, Some(&f), &cx).is_err());
    }
}

#[test]
fn base_response_errors_keep_host_status_semantics_without_echoing_provider_text() {
    for (code, status) in [
        (1004, 401),
        (2049, 401),
        (2013, 400),
        (1026, 400),
        (1002, 429),
        (1008, 503),
        (2153, 503),
        (9999, 502),
    ] {
        let SubmitOutcomeV2::Rejected(error) = MiniMaxTaskReferenceV2
            .parse_submit_response(&parts(
                200,
                json!({"base_resp":{"status_code":code,"status_msg":"secret-echo"}}),
            ))
            .unwrap()
        else {
            panic!("explicit rejection")
        };
        assert_eq!(error.http_status, status);
        assert!(!error.message.contains("secret-echo"));
        let obs = MiniMaxTaskReferenceV2
            .parse_observation(&parts(200, json!({"status":"Success","file_id":"123"})))
            .unwrap();
        let cx = TaskRenderContextV2::new("host-1", 1, "public", "minimax", Some("123")).unwrap();
        let error = MiniMaxTaskReferenceV2
            .render_success(
                &obs,
                Some(&parts(
                    200,
                    json!({"base_resp":{"status_code":code,"status_msg":"secret-echo"}}),
                )),
                &cx,
            )
            .unwrap_err();
        assert_eq!(error.http_status, status);
    }
}
#[test]
fn duration_whitespace_is_normalized_and_host_owns_tier_eligibility() {
    for (input, want) in [(json!(" 6.0 "), 6), (json!(20), 20)] {
        let p =
            prepare(json!({"model":"MiniMax-Hailuo-02","prompt":"x","duration":input})).unwrap();
        assert_eq!(p.descriptor.body.unwrap()["duration"], want);
    }
}
#[test]
fn reseller_model_stays_original_and_shape_is_host_config_only() {
    let mut cfg = config(None);
    cfg.extensions.insert("upstream_model_family".into(), json!("MiniMax-Hailuo-02"));
    let p=MiniMaxTaskReferenceV2.build_submit_request(&cfg,&json!({"model":"reseller-original-v1","last_frame":"tail","upstream_model_family":"MiniMax-H3"}),&HostMintedValuesV1::new("host-1",None).unwrap()).unwrap();
    assert_eq!(p.descriptor.body.unwrap()["model"], "reseller-original-v1");
    assert!(prepare(json!({"model":"reseller-original-v1","prompt":"x","upstream_model_family":"MiniMax-Hailuo-02"})).is_err());
}
#[test]
fn descriptor_json_roundtrip_and_unauthenticated_config_do_not_leak_slots() {
    let mut cfg = config(None);
    cfg.auth = None;
    let p = MiniMaxTaskReferenceV2
        .build_submit_request(
            &cfg,
            &json!({"model":"MiniMax-Hailuo-02","prompt":"x"}),
            &HostMintedValuesV1::new("host-1", None).unwrap(),
        )
        .unwrap();
    let encoded = south_component_conformance::task_v2_json::prepared_task_json(&p).unwrap();
    assert!(!encoded.to_string().contains("provider_api_key"));
    assert_eq!(
        south_component_conformance::task_v2_json::parse_prepared_task_json(&encoded.to_string())
            .unwrap(),
        p
    );
    cfg.authorize(&p.descriptor).unwrap();
    let mut wrong = cfg.clone();
    wrong.base_url =
        token_station_protocol::ProviderEndpoint::try_new("https://other.example").unwrap();
    assert!(wrong.authorize(&p.descriptor).is_err());
    assert!(
        MiniMaxTaskReferenceV2
            .build_observe_request(&cfg, "MiniMax-Hailuo-02", "abc", &p.locator)
            .is_err()
    );
    assert!(
        matches!(MiniMaxTaskReferenceV2.parse_submit_response(&parts(200,json!({"task_id":"abc"}))).unwrap(),SubmitOutcomeV2::Accepted(id) if id=="abc")
    );
}

#[test]
fn descriptor_queries_use_the_controlled_canonical_order() {
    let cfg = config(Some("19000"));
    let q = MiniMaxTaskReferenceV2
        .build_observe_request(&cfg, "MiniMax-Hailuo-02", "00123", &locator())
        .unwrap();
    assert!(q.url.ends_with("?GroupId=19000&task_id=00123"));
    let obs = MiniMaxTaskReferenceV2
        .parse_observation(&parts(200, json!({"status":"Success","file_id":"00567"})))
        .unwrap();
    let f = MiniMaxTaskReferenceV2.build_artifact_request(&cfg, &locator(), &obs).unwrap().unwrap();
    assert!(f.url.ends_with("?GroupId=19000&file_id=00567"));
}
#[test]
fn explicit_http_rejections_release_acceptance_uncertainty_and_files_keep_status() {
    for status in [400, 401, 403, 429, 499] {
        let p = parts(status, json!({"task_id":"123","error":"secret-echo"}));
        let SubmitOutcomeV2::Rejected(error) =
            MiniMaxTaskReferenceV2.parse_submit_response(&p).unwrap()
        else {
            panic!("HTTP {status} explicitly rejected submission")
        };
        assert_eq!(error.http_status, status);
        assert!(!error.message.contains("secret-echo"));
    }
    let observation = MiniMaxTaskReferenceV2
        .parse_observation(&parts(200, json!({"status":"Success","file_id":"123"})))
        .unwrap();
    let cx = TaskRenderContextV2::new("host-1", 123, "public", "minimax", Some("123")).unwrap();
    for status in [400, 401, 403, 429, 499, 500, 503] {
        let p = parts(status, json!({"error":"secret-echo"}));
        let error = MiniMaxTaskReferenceV2.render_success(&observation, Some(&p), &cx).unwrap_err();
        assert_eq!(error.http_status, status);
        assert!(!error.message.contains("secret-echo"));
    }
    for status in [500, 503] {
        assert!(matches!(
            MiniMaxTaskReferenceV2
                .parse_submit_response(&parts(status, json!({"task_id":"123"})))
                .unwrap(),
            SubmitOutcomeV2::Unknown
        ));
    }
    let invalid: HttpResponseParts =
        serde_json::from_value(json!({"status":200,"headers":{},"body":"not json"})).unwrap();
    assert!(matches!(
        MiniMaxTaskReferenceV2.parse_submit_response(&invalid).unwrap(),
        SubmitOutcomeV2::Unknown
    ));
}
#[test]
fn id_parsing_trims_outer_whitespace_once_without_rewriting_digits() {
    assert!(
        matches!(MiniMaxTaskReferenceV2.parse_submit_response(&parts(200,json!({"task_id":" 00123 "}))).unwrap(),SubmitOutcomeV2::Accepted(id) if id=="00123")
    );
    for id in [json!("   "), json!(-1), json!("-1")] {
        assert!(matches!(
            MiniMaxTaskReferenceV2
                .parse_submit_response(&parts(200, json!({"task_id":id})))
                .unwrap(),
            SubmitOutcomeV2::Unknown
        ));
    }
    let observation = MiniMaxTaskReferenceV2
        .parse_observation(&parts(200, json!({"status":"Success","file_id":" 00567 "})))
        .unwrap();
    assert!(
        matches!(observation,TaskObservationV2::Succeeded{artifacts:TaskArtifactRefV2::FileId(id),..} if id=="00567")
    );
}
proptest::proptest! {
 #[test]
 fn arbitrary_response_text_is_total_and_produces_bounded_typed_facts(text in ".{0,2048}",status in 100u16..600) {
  let parts:HttpResponseParts=serde_json::from_value(json!({"status":status,"headers":{},"body":text})).unwrap();
  let observation=MiniMaxTaskReferenceV2.parse_observation(&parts).unwrap();
  proptest::prop_assert!(observation.validate().is_ok());
  let wire=south_component_conformance::task_v2_json::observation_json(&observation).unwrap();
  proptest::prop_assert_eq!(south_component_conformance::task_v2_json::parse_observation_json(&wire.to_string()).unwrap(),observation);
  let outcome=MiniMaxTaskReferenceV2.parse_submit_response(&parts).unwrap();
  let wire=south_component_conformance::task_v2_json::submit_outcome_json(&outcome).unwrap();
  proptest::prop_assert_eq!(south_component_conformance::task_v2_json::parse_submit_outcome_json(&wire.to_string()).unwrap(),outcome);
 }
}
