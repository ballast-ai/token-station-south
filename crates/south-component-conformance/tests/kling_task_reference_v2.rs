use serde_json::{Value, json};
use south_component_conformance::{
    SubmitOutcomeV2, TaskComponentV2, reference_kling_task_v2::KlingTaskReferenceV2,
};
use south_contracts::{HostMintedValuesV1, TaskLocatorV2, TaskObservationV2, TaskRenderContextV2};
use token_station_protocol::{HttpResponseParts, ProviderConfig};

fn config() -> ProviderConfig {
    serde_json::from_value(
        json!({"provider":"kling","base_url":"https://api.kling.example","models":[]}),
    )
    .unwrap()
}
fn response(status: u16, body: &Value) -> HttpResponseParts {
    serde_json::from_value(json!({"status":status,"headers":{},"body":body.to_string()})).unwrap()
}

#[test]
fn v2_prepare_preserves_image_tail_and_real_model_and_query_locator() {
    let prepared = KlingTaskReferenceV2.build_submit_request(&config(), &json!({"operation":"text-or-image","model":"real-model","prompt":"", "image_tail":"tail", "sound":"on", "voice_list":["voice"]}), &HostMintedValuesV1::new("host-1",None).unwrap()).unwrap();
    assert_eq!(prepared.locator.route(), "v1/videos/image2video");
    assert_eq!(
        prepared.descriptor.body,
        Some(
            json!({"model_name":"real-model","prompt":"","image_tail":"tail","sound":"on","voice_list":["voice"],"duration":"5","external_task_id":"host-1"})
        )
    );
    let observe = KlingTaskReferenceV2
        .build_observe_request(&config(), "not-an-i2v-model", "original-id", &prepared.locator)
        .unwrap();
    assert_eq!(
        serde_json::to_value(observe).unwrap()["url"],
        "https://api.kling.example/v1/videos/image2video/original-id"
    );
}

#[test]
fn v2_omni_base_edit_omits_duration_and_ratio_and_requires_resolved_mode() {
    let mut request = json!({"operation":"omni","model":"real-model","prompt":"edit","mode":"std","video_list":[{"url":"base"}],"duration":10,"aspect_ratio":"16:9"});
    let minted = HostMintedValuesV1::new("host-1", None).unwrap();
    let prepared = KlingTaskReferenceV2.build_submit_request(&config(), &request, &minted).unwrap();
    assert_eq!(prepared.locator.route(), "v1/videos/omni-video");
    assert_eq!(
        prepared.descriptor.body,
        Some(
            json!({"model_name":"real-model","prompt":"edit","mode":"std","video_list":[{"url":"base"}]})
        )
    );
    request.as_object_mut().unwrap().remove("mode");
    assert!(KlingTaskReferenceV2.build_submit_request(&config(), &request, &minted).is_err());
}

#[test]
fn v2_motion_is_explicit_and_retains_its_wire_fields() {
    let prepared=KlingTaskReferenceV2.build_submit_request(&config(),&json!({"operation":"motion-control","model":"real-model","image":"fallback","image_url":"preferred","video_url":"video","character_orientation":"video","mode":"std","keep_original_sound":true}),&HostMintedValuesV1::new("host-1",None).unwrap()).unwrap();
    assert_eq!(prepared.locator.route(), "v1/videos/motion-control");
    assert_eq!(
        prepared.descriptor.body,
        Some(
            json!({"model_name":"real-model","image_url":"preferred","video_url":"video","character_orientation":"video","mode":"pro","keep_original_sound":true})
        )
    );
}

#[test]
fn v2_submit_unknown_is_conservative_and_ids_are_not_wrapped() {
    for parts in [
        response(503, &json!({"data":{"task_id":"id"}})),
        response(200, &json!({"code":9,"message":"refused"})),
        response(200, &json!({"data":{"task_id":""}})),
    ] {
        assert!(matches!(
            KlingTaskReferenceV2.parse_submit_response(&parts).unwrap(),
            SubmitOutcomeV2::Unknown
        ));
    }
    for body in [
        json!({"data":{"task_id":"nested"},"task_id":"top"}),
        json!({"code":9,"data":{"task_id":"nested"}}),
    ] {
        assert!(
            matches!(KlingTaskReferenceV2.parse_submit_response(&response(200,&body)).unwrap(),SubmitOutcomeV2::Accepted(id) if id=="nested")
        );
    }
    assert!(
        matches!(KlingTaskReferenceV2.parse_submit_response(&response(200,&json!({"data":{},"task_id":"top"}))).unwrap(),SubmitOutcomeV2::Accepted(id) if id=="top")
    );
}

#[test]
fn v2_observation_retains_both_meters_and_scalar_artifact_fields() {
    let observation=KlingTaskReferenceV2.parse_observation(&response(200,&json!({"data":{"task_status":"succeed","final_unit_deduction":"1.234","task_result":{"videos":[{"url":"https://cdn/one","id":12,"duration":"5"},{"url":"https://cdn/two","id":"second","duration":2.5}]}}}))).unwrap();
    let TaskObservationV2::Succeeded { ref usage, .. } = observation else {
        panic!("expected success")
    };
    assert_eq!(usage.seconds(), Some(5.0));
    assert_eq!(usage.milliunits(), Some(1234));
    let context =
        TaskRenderContextV2::new("host-1", 123, "public-model", "provider-alias", Some("raw-id"))
            .unwrap();
    assert_eq!(
        KlingTaskReferenceV2.render_success(&observation, None, &context).unwrap(),
        json!({"created":123,"model":"public-model","provider":"provider-alias","task_id":"raw-id","data":[{"url":"https://cdn/one","id":12,"duration":"5"},{"url":"https://cdn/two","id":"second","duration":2.5}]})
    );
    let missing =
        TaskRenderContextV2::new("host-1", 123, "public-model", "provider-alias", None).unwrap();
    assert!(KlingTaskReferenceV2.render_success(&observation, None, &missing).is_err());
}

#[test]
fn v2_incomplete_or_nonscalar_artifact_list_is_wholly_unknown() {
    for bad in [
        json!({"id":"missing-url"}),
        json!({"url":""}),
        json!({"url":"https://cdn/two","id":{}}),
        json!({"url":"https://cdn/two","duration":[]}),
    ] {
        let parts = response(
            200,
            &json!({"task_status":"succeed","task_result":{"videos":[{"url":"https://cdn/one"},bad]}}),
        );
        assert!(matches!(
            KlingTaskReferenceV2.parse_observation(&parts).unwrap(),
            TaskObservationV2::Unknown { .. }
        ));
    }
}

#[test]
fn v2_query_failure_and_invalid_meter_never_invent_terminal_or_zero() {
    for status in [401, 403, 404, 429, 500] {
        assert!(matches!(
            KlingTaskReferenceV2
                .parse_observation(&response(status, &json!({"task_status":"failed"})))
                .unwrap(),
            TaskObservationV2::Unknown { .. }
        ));
    }
    for word in ["submitted", "processing"] {
        assert!(
            matches!(KlingTaskReferenceV2.parse_observation(&response(200,&json!({"task_status":word}))).unwrap(),TaskObservationV2::Progress{running,status_word} if running==(word=="processing") && status_word==word)
        );
    }
    for (duration, units) in [
        (json!("-1"), json!("NaN")),
        (json!("bad"), json!(1)),
        (json!(5), json!(-1)),
        (json!(5), json!("bad")),
    ] {
        assert!(matches!(KlingTaskReferenceV2.parse_observation(&response(200,&json!({"task_status":"succeed","final_unit_deduction":units,"task_result":{"videos":[{"url":"https://cdn/one","duration":duration}]}}))).unwrap(),TaskObservationV2::Unknown{..}));
    }
    for video in
        [json!({"url":"https://cdn/one"}), json!({"url":"https://cdn/one","duration":null})]
    {
        let TaskObservationV2::Succeeded{usage,..}=KlingTaskReferenceV2.parse_observation(&response(200,&json!({"task_status":"succeed","final_unit_deduction":null,"task_result":{"videos":[video]}}))).unwrap() else { panic!("missing meter is not invalid") };
        assert_eq!(usage.seconds(), None);
        assert_eq!(usage.milliunits(), None);
    }
}

#[test]
fn v2_observe_rejects_locator_versions_and_non_kling_routes() {
    assert!(TaskLocatorV2::new(2, "v1/videos/text2video").is_err());
    let locator = TaskLocatorV2::new(1, "other").unwrap();
    assert!(
        KlingTaskReferenceV2
            .build_observe_request(&config(), "real-model", "id", &locator)
            .is_err()
    );
}

#[test]
fn v2_prepare_and_observe_keep_the_same_bound_credential() {
    let mut config = config();
    config.auth = Some(token_station_protocol::SecretRef::new("bound-task-slot"));
    let prepared = KlingTaskReferenceV2
        .build_submit_request(
            &config,
            &json!({"operation":"text-or-image","model":"real-model","prompt":"a cat"}),
            &HostMintedValuesV1::new("host-1", None).unwrap(),
        )
        .unwrap();
    let query = KlingTaskReferenceV2
        .build_observe_request(&config, "different-model", "id", &prepared.locator)
        .unwrap();
    for descriptor in [&prepared.descriptor, &query] {
        config.authorize(descriptor).unwrap();
        assert_eq!(
            descriptor.auth,
            Some(token_station_protocol::Auth::bearer(token_station_protocol::SecretRef::new(
                "bound-task-slot"
            )))
        );
    }
}

#[test]
fn v2_artifact_bounds_fail_as_unknown_without_dropping_entries() {
    for videos in
        [vec![json!({"url":"https://cdn/one"}); 17], vec![json!({"url":"x".repeat(8193)})]]
    {
        assert!(matches!(
            KlingTaskReferenceV2
                .parse_observation(&response(
                    200,
                    &json!({"task_status":"succeed","task_result":{"videos":videos}})
                ))
                .unwrap(),
            TaskObservationV2::Unknown { .. }
        ));
    }
}

#[test]
fn v2_observe_encodes_one_id_segment_and_preserves_endpoint_prefix() {
    let config: ProviderConfig = serde_json::from_value(
        json!({"provider":"kling","base_url":"https://api.kling.example/prefix","models":[]}),
    )
    .unwrap();
    let locator = TaskLocatorV2::new(1, "v1/videos/text2video").unwrap();
    let descriptor = KlingTaskReferenceV2
        .build_observe_request(&config, "real-model", "ab?c#d%e", &locator)
        .unwrap();
    config.authorize(&descriptor).unwrap();
    assert_eq!(
        serde_json::to_value(descriptor).unwrap()["url"],
        "https://api.kling.example/prefix/v1/videos/text2video/ab%3Fc%23d%25e"
    );
    // The component retains single-segment encoding, while the pinned kernel
    // intentionally refuses encoded separators. Do not weaken that host gate.
    for (id, encoded) in [("a/b", "a%2Fb"), ("a\\b", "a%5Cb")] {
        let descriptor = KlingTaskReferenceV2
            .build_observe_request(&config, "real-model", id, &locator)
            .unwrap();
        assert!(descriptor.url.ends_with(encoded));
        assert!(config.authorize(&descriptor).is_err());
    }
    for id in [String::new(), ".".to_owned(), "..".to_owned(), "x".repeat(8193)] {
        let error = KlingTaskReferenceV2
            .build_observe_request(&config, "real-model", &id, &locator)
            .unwrap_err();
        assert_eq!(error.message, "invalid kling upstream task id");
    }
}

#[test]
fn v2_artifact_lookup_revalidates_public_observation_variants() {
    let locator = TaskLocatorV2::new(1, "v1/videos/text2video").unwrap();
    for observation in [
        TaskObservationV2::Succeeded {
            artifacts: south_contracts::TaskArtifactRefV2::Urls(Vec::new()),
            usage: south_contracts::TaskUsageFactsV2::default(),
        },
        TaskObservationV2::Unknown { reason: "x".repeat(8193) },
    ] {
        assert!(
            KlingTaskReferenceV2.build_artifact_request(&config(), &locator, &observation).is_err(),
            "native typed calls must enforce the same facts boundary as the sandbox codec"
        );
    }
}

#[test]
fn v2_provider_milliunit_bound_is_inclusive_and_never_clamps() {
    for (units, valid) in [(1_000_000_000_000_u64, true), (1_000_000_000_001_u64, false)] {
        let observation=KlingTaskReferenceV2.parse_observation(&response(200,&json!({"task_status":"succeed","final_unit_deduction":units,"task_result":{"videos":[{"url":"https://cdn/one"}]}}))).unwrap();
        if valid {
            let TaskObservationV2::Succeeded { usage, .. } = observation else {
                panic!("inclusive bound must be accepted")
            };
            assert_eq!(usage.milliunits(), Some(1_000_000_000_000_000));
        } else {
            assert!(matches!(observation, TaskObservationV2::Unknown { .. }));
        }
    }
}
