//! Managed-host transcriptions and explicit protocol boundary regressions.
use serde_json::{Value, json};
use south_component_conformance::{
    SubmitOutcomeV2, TaskComponentV2, reference_bailian_task_v2::BailianTaskComponentV2,
};
use south_contracts::{
    HostMintedValuesV1, TaskFailureKindV1, TaskLocatorV2, TaskObservationV2, TaskRenderContextV2,
};
use token_station_protocol::{Auth, HttpResponseParts, ProviderConfig, SecretRef};
fn config() -> ProviderConfig {
    serde_json::from_value(json!({"provider":"bailian","base_url":"https://dashscope.example","auth":"provider_api_key"})).unwrap()
}
fn parts(status: u16, body: &Value) -> HttpResponseParts {
    serde_json::from_value(json!({"status":status,"body":body.to_string()})).unwrap()
}
fn prepare(
    body: &Value,
) -> south_component_conformance::ComponentResultV1<south_component_conformance::PreparedTaskV2> {
    BailianTaskComponentV2.build_submit_request(
        &config(),
        body,
        &HostMintedValuesV1::new("host-task", None).unwrap(),
    )
}
#[test]
fn submit_and_query_keep_async_header_and_secret_reference_separate() {
    let p = prepare(&json!({"model":"wan2.7-t2v","prompt":"scene"})).unwrap();
    assert_eq!(
        p.descriptor.url,
        "https://dashscope.example/api/v1/services/aigc/video-generation/video-synthesis"
    );
    assert_eq!(p.descriptor.headers.get("x-dashscope-async"), Some("enable"));
    assert_eq!(p.descriptor.auth, Some(Auth::bearer(SecretRef::new("provider_api_key"))));
    assert_eq!(p.request_estimate.requested_seconds(), Some(5.0));
    assert_eq!(p.request_estimate.milliunits_per_second(), None);
    assert_eq!(p.locator.route(), "api/v1/tasks");
    config().authorize(&p.descriptor).unwrap();
    let q = BailianTaskComponentV2
        .build_observe_request(&config(), "wan2.7-t2v", "original-001", &p.locator)
        .unwrap();
    assert_eq!(q.url, "https://dashscope.example/api/v1/tasks/original-001");
    assert!(q.headers.get("x-dashscope-async").is_none());
    assert!(q.body.is_none());
    config().authorize(&q).unwrap();
}
#[test]
fn model_shapes_preserve_legacy_wire_fields() {
    let hh=prepare(&json!({"model":"happyhorse-1.1-i2v","prompt":"p","image":"https://x/a","audio_url":"https://x/s"})).unwrap().descriptor.body.unwrap();
    assert_eq!(
        hh["input"],
        json!({"prompt":"p","media":[{"type":"first_frame","url":"https://x/a"}],"audio_url":"https://x/s"})
    );
    let wan = prepare(&json!({"model":"wan2.7-i2v","prompt":"p","image":"https://x/a"}))
        .unwrap()
        .descriptor
        .body
        .unwrap();
    assert_eq!(wan["input"]["img_url"], "https://x/a");
    let k=prepare(&json!({"model":"wan2.2-kf2v-flash","prompt":"p","image":"https://x/a","last_frame":"https://x/b","negative_prompt":"n","duration":" 5 ","resolution":"1080P","seed":1,"watermark":false})).unwrap();
    assert!(k.descriptor.url.contains("/image2video/"));
    let body = k.descriptor.body.unwrap();
    assert_eq!(
        body["input"],
        json!({"prompt":"p","first_frame_url":"https://x/a","last_frame_url":"https://x/b","negative_prompt":"n"})
    );
    assert!(body["parameters"].get("duration").is_none());
    assert!(
        prepare(&json!({"model":"wan2.2-kf2v-flash","prompt":"p","image":"x","duration":6}))
            .is_err()
    );
}
#[test]
fn reference_images_and_aliases_preserve_wire_identity() {
    let mut c = config();
    c.extensions.insert("upstream_model_family".into(), json!("happyhorse-1.1-r2v"));
    let p=BailianTaskComponentV2.build_submit_request(&c,&json!({"model":"reseller-alias","prompt":"p","reference_images":["a","b"],"aspect_ratio":"16:9","ratio":"1:1"}),&HostMintedValuesV1::new("h",None).unwrap()).unwrap();
    let body = p.descriptor.body.unwrap();
    assert_eq!(body["model"], "reseller-alias");
    assert_eq!(
        body["input"]["media"],
        json!([{"type":"reference_image","url":"a"},{"type":"reference_image","url":"b"}])
    );
    assert_eq!(body["parameters"]["ratio"], "16:9");
    assert_eq!(p.request_estimate.input_image_count(), Some(2));
    for req in [
        json!({"model":"happyhorse-r2v","prompt":"p"}),
        json!({"model":"happyhorse-r2v","prompt":"p","image":"x","reference_images":["a"]}),
        json!({"model":"happyhorse-r2v","prompt":"p","reference_images":["a","b","c","d","e","f","g","h","i","j"]}),
    ] {
        assert!(prepare(&req).is_err());
    }
}
#[test]
fn estimates_normalize_existing_resolution_hints_without_changing_wire() {
    for (hint, expected) in [
        (" 01920*01080 ", Some("1080P")),
        ("1920X1080", Some("1080P")),
        ("01080x01920", Some("01080X01920")),
        ("00640*00480", Some("480P")),
        (" 720p ", Some("720P")),
        ("unrecognized*size", None),
    ] {
        let p = prepare(&json!({"model":"wan-t2v","prompt":"p","size":hint})).unwrap();
        assert_eq!(p.request_estimate.resolution(), expected, "{hint}");
        assert_eq!(p.descriptor.body.unwrap()["parameters"]["size"], hint);
    }
}
#[test]
fn submit_acceptance_keeps_original_id_and_never_rejects_contradiction() {
    assert_eq!(
        BailianTaskComponentV2
            .parse_submit_response(&parts(
                200,
                &json!({"output":{"task_id":"001-abc","task_status":"PENDING"}})
            ))
            .unwrap(),
        SubmitOutcomeV2::Accepted("001-abc".into())
    );
    for p in [
        parts(500, &json!({})),
        parts(200, &json!({})),
        parts(200, &json!({"code":"Bad","output":{"task_id":"id"}})),
        parts(200, &json!({"output":{"task_id":".."}})),
    ] {
        assert_eq!(
            BailianTaskComponentV2.parse_submit_response(&p).unwrap(),
            SubmitOutcomeV2::Unknown
        );
    }
    for status in [400, 401, 429, 499] {
        assert!(
            matches!(BailianTaskComponentV2.parse_submit_response(&parts(status,&json!({"message":"do not echo secret"}))).unwrap(),SubmitOutcomeV2::Rejected(e) if e.http_status==status && !e.message.contains("secret"))
        );
    }
}
#[test]
fn observation_preserves_progress_and_unknown_cost_cancellation() {
    for (word, running) in [("PENDING", false), ("RUNNING", true)] {
        assert_eq!(
            BailianTaskComponentV2
                .parse_observation(&parts(200, &json!({"output":{"task_status":word}})))
                .unwrap(),
            TaskObservationV2::Progress { running, status_word: word.into() }
        );
    }
    for word in ["UNKNOWN", "future"] {
        assert!(matches!(
            BailianTaskComponentV2
                .parse_observation(&parts(200, &json!({"output":{"task_status":word}})))
                .unwrap(),
            TaskObservationV2::Unknown { .. }
        ));
    }
    assert!(matches!(
        BailianTaskComponentV2
            .parse_observation(&parts(200, &json!({"output":{"task_status":"CANCELED"}})))
            .unwrap(),
        TaskObservationV2::Failed { kind: TaskFailureKindV1::Cancelled, .. }
    ));
}
#[test]
fn metering_prefers_valid_billable_duration_without_inventing_usage() {
    for (usage, expected) in [
        (json!({"duration":4,"video_duration":8}), Some(4.0)),
        (json!({"duration":0,"video_duration":8}), Some(0.0)),
        (json!({"duration":null,"video_duration":" 5.5 "}), Some(5.5)),
        (json!({"duration":-1,"video_duration":6}), Some(6.0)),
        (json!({"duration":"NaN","video_duration":-1}), None),
        (json!({"output_video_duration":9}), None),
        (json!({}), None),
    ] {
        let obs=BailianTaskComponentV2.parse_observation(&parts(200,&json!({"output":{"task_status":"SUCCEEDED","video_url":"https://x/v?sig=private"},"usage":usage}))).unwrap();
        let TaskObservationV2::Succeeded { usage, .. } = obs else {
            panic!("success must remain successful")
        };
        assert_eq!(usage.seconds(), expected);
        assert_eq!(usage.tokens(), None);
        assert_eq!(usage.milliunits(), None);
    }
}
#[test]
fn direct_artifact_renders_with_host_context_and_no_extra_fetch() {
    let obs = BailianTaskComponentV2
        .parse_observation(&parts(
            200,
            &json!({"output":{"task_status":"SUCCEEDED","video_url":"https://x/v?sig=private"}}),
        ))
        .unwrap();
    assert_eq!(
        BailianTaskComponentV2
            .build_artifact_request(
                &config(),
                &TaskLocatorV2::new(1, "api/v1/tasks").unwrap(),
                &obs
            )
            .unwrap(),
        None
    );
    let context = TaskRenderContextV2::new(
        "gateway-task",
        123,
        "public/model",
        "bailian-reseller",
        Some("upstream-original"),
    )
    .unwrap();
    assert_eq!(
        BailianTaskComponentV2.render_success(&obs, None, &context).unwrap(),
        json!({"created":123,"model":"public/model","provider":"bailian-reseller","task_id":"upstream-original","data":[{"url":"https://x/v?sig=private"}]})
    );
}
#[test]
fn query_refuses_invalid_locator_and_path_ids_without_echoing_them() {
    let l = TaskLocatorV2::new(1, "api/v1/tasks").unwrap();
    for id in ["", ".", "..", "private\nvalue"] {
        let e = BailianTaskComponentV2.build_observe_request(&config(), "wan", id, &l).unwrap_err();
        assert!(!e.message.contains("private"));
    }
    assert!(
        BailianTaskComponentV2
            .build_observe_request(
                &config(),
                "wan",
                "id",
                &TaskLocatorV2::new(1, "wrong/tasks").unwrap()
            )
            .is_err()
    );
}

#[test]
fn reference_input_diagnostics_preserve_safe_host_contract() {
    for (request, expected) in [
        (
            json!({"model":"happyhorse-r2v","prompt":"p"}),
            "'reference_images' (1–9 image URLs) is required",
        ),
        (
            json!({"model":"happyhorse-r2v","prompt":"p","image":"PRIVATE","reference_images":["a"]}),
            "not a first frame",
        ),
        (
            json!({"model":"happyhorse-r2v","prompt":"p","reference_images":["a","b","c","d","e","f","g","h","i","j"]}),
            "at most 9",
        ),
    ] {
        let error = prepare(&request).unwrap_err();
        assert!(error.message.contains(expected), "{}", error.message);
        assert!(!error.message.contains("PRIVATE"));
    }
}

#[test]
fn query_encoding_keeps_separators_inside_the_existing_authorization_gate() {
    let c = config();
    let l = TaskLocatorV2::new(1, "api/v1/tasks").unwrap();
    for id in ["original?#%", "original/segment", "original\\segment"] {
        let query = BailianTaskComponentV2.build_observe_request(&c, "wan", id, &l).unwrap();
        if id.contains(['/', '\\']) {
            assert!(c.authorize(&query).is_err());
        } else {
            c.authorize(&query).unwrap();
            assert!(query.url.ends_with("original%3F%23%25"));
        }
    }
    let mut wrong = c.clone();
    wrong.auth = Some(SecretRef::new("another-slot"));
    let q = BailianTaskComponentV2.build_observe_request(&c, "wan", "original-id", &l).unwrap();
    assert!(wrong.authorize(&q).is_err());
}

#[test]
fn failed_observation_preserves_business_code_but_internal_errors_do_not_echo_payloads() {
    let o=BailianTaskComponentV2.parse_observation(&parts(200,&json!({"output":{"task_status":"FAILED","code":"DataInspectionFailed","message":"inappropriate content"}}))).unwrap();
    let error = BailianTaskComponentV2.map_terminal_failure(&o).unwrap();
    assert_eq!(error.http_status, 400);
    assert!(error.message.contains("DataInspectionFailed"));
    let invalid: HttpResponseParts =
        serde_json::from_value(json!({"status":200,"body":"PRIVATE INVALID JSON"})).unwrap();
    let obs = BailianTaskComponentV2.parse_observation(&invalid).unwrap();
    assert!(matches!(obs,TaskObservationV2::Unknown{reason} if !reason.contains("PRIVATE")));
}

proptest::proptest! {
    #[test]
    fn arbitrary_responses_cannot_panic_or_synthesize_success(raw in ".{0,512}") {
        let response:HttpResponseParts=serde_json::from_value(json!({"status":200,"body":raw})).unwrap();
        let observed=BailianTaskComponentV2.parse_observation(&response).unwrap();
        observed.validate().unwrap();
        if serde_json::from_str::<Value>(&response.body).is_err() {
            proptest::prop_assert!(matches!(observed,TaskObservationV2::Unknown{..}), "invalid JSON is unknown");
            proptest::prop_assert_eq!(BailianTaskComponentV2.parse_submit_response(&response).unwrap(),SubmitOutcomeV2::Unknown);
        }
    }
    #[test]
    fn all_nonnegative_finite_reported_seconds_preserve_their_value(seconds in 0.0f64..1_000_000.0) {
        let observation=BailianTaskComponentV2.parse_observation(&parts(200,&json!({"output":{"task_status":"SUCCEEDED","video_url":"https://media.example/video"},"usage":{"duration":seconds}}))).unwrap();
        let TaskObservationV2::Succeeded{usage,..}=observation else {panic!("valid success")};
        let expected=serde_json::from_str::<Value>(&seconds.to_string()).unwrap().as_f64().unwrap();
        proptest::prop_assert_eq!(usage.seconds(),Some(expected));
    }
}

#[test]
fn invalid_request_estimates_are_client_errors_even_for_unknown_models() {
    for duration in [0_u64, u64::from(u32::MAX) + 1] {
        let error = prepare(&json!({"model":"custom-video","prompt":"scene","duration":duration}))
            .expect_err("unrepresentable request estimates must fail before host reservation");
        assert_eq!(error.code, token_station_protocol::ErrorCode::InvalidRequest);
        assert_eq!(error.http_status, 400);
    }
}
