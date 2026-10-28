//! Task contract 8 (docs/design/2026-10-01-omni-task-component.md §6, §7, §13): inline artifacts
//! whose bytes the host holds, and declared token buckets in usage and in the request estimate.
//!
//! The increment is additive: every new key is optional, absent means "not reported", and a
//! contract 7 value decodes and re-encodes byte for byte, so the task packages published before
//! contract 8 keep working unchanged on a host that decodes both.

use serde_json::{Value, json};
use south_component_conformance::task_v2_json::*;
use south_contracts::image::ArtifactEncodingV1;
use south_contracts::media::{BlobSetV1, MediaLimitsV1, PathPatternV1, elide_v1};
use south_contracts::{
    TASK_CONTRACT_VERSION, TASK_CONTRACT_VERSIONS, TaskArtifactRefV2, TaskArtifactRoleV2,
    TaskContractErrorV2, TaskInlineArtifactV2, TaskObservationV2, TaskTokenBucketV2,
    TaskTokenBucketsV2, TaskUsageFactsV2, fill_task_artifact_markers_v1,
    resolve_task_inline_artifact_v1,
};

fn inline_item(pointer: &str, role: &Value) -> Value {
    json!({"pointer": pointer, "encoding": "base64", "media_type": "video/mp4", "role": role})
}

fn succeeded(artifacts: Value, usage: Value) -> String {
    let mut observation = json!({"state": "succeeded"});
    observation["artifacts"] = artifacts;
    observation["usage"] = usage;
    observation.to_string()
}

fn seven_usage() -> Value {
    json!({"seconds": null, "milliunits": null, "tokens": null, "outputs": 1})
}

fn bucket_usage(buckets: Value) -> Value {
    let mut usage = seven_usage();
    usage["token_buckets"] = buckets;
    usage
}

fn omni_buckets() -> Value {
    json!({"text_input": 17, "total_input": 17, "video_output": 57920, "total_output": 57920,
           "reasoning_output": 234})
}

fn video(pointer: &str) -> TaskInlineArtifactV2 {
    TaskInlineArtifactV2::new(pointer, ArtifactEncodingV1::Base64, "video/mp4").unwrap()
}

#[test]
fn contract_eight_is_admitted_and_seven_still_decodes() {
    // Contract 9 (the failure category) is the current version since 0.81.0; eight stays admitted.
    assert_eq!(TASK_CONTRACT_VERSION, 9);
    assert_eq!(TASK_CONTRACT_VERSIONS, [7, 8, 9]);
}

#[test]
fn a_contract_seven_observation_round_trips_without_new_keys() {
    let url_item = json!({"url": "https://cdn.example/a.mp4", "id": null, "duration": null,
                          "fetch_with_credential": false, "role": null});
    let wire = succeeded(json!({"kind": "urls", "items": [url_item]}), seven_usage());
    let observation = parse_observation_json(&wire).unwrap();
    let back = observation_json(&observation).unwrap();
    assert_eq!(back, serde_json::from_str::<Value>(&wire).unwrap());
    assert!(back["usage"].get("token_buckets").is_none(), "an absent fact stays absent");
}

#[test]
fn a_contract_seven_prepared_task_round_trips_without_bucket_ceilings() {
    let wire = json!({"descriptor": {"method": "POST", "url": "https://upstream.example/tasks"},
        "locator": {"schema_version": 1, "route": "v1/tasks"},
        "request_estimate": {"requested_seconds": 5.0, "milliunits_per_second": null,
            "resolution": null, "input_image_count": null, "tokens_per_second": null,
            "requested_outputs": null},
        "immutable_body_paths": null});
    let prepared = parse_prepared_task_json(&wire.to_string()).unwrap();
    let back = prepared_task_json(&prepared).unwrap();
    assert!(back["request_estimate"].get("bucket_ceilings").is_none());
    assert_eq!(prepared.request_estimate.bucket_ceilings(), None);
}

#[test]
fn inline_artifacts_and_token_buckets_round_trip() {
    let wire = succeeded(
        json!({"kind": "inline", "items": [inline_item("/steps/1/content/0/data", &Value::Null)]}),
        bucket_usage(omni_buckets()),
    );
    let observation = parse_observation_json(&wire).unwrap();
    let TaskObservationV2::Succeeded { artifacts: TaskArtifactRefV2::Inline(items), usage } =
        &observation
    else {
        panic!("inline expected")
    };
    assert_eq!(items[0].pointer(), "/steps/1/content/0/data");
    assert_eq!(items[0].encoding(), ArtifactEncodingV1::Base64);
    assert_eq!(items[0].media_type(), "video/mp4");
    assert_eq!(items[0].role(), TaskArtifactRoleV2::Primary);
    let buckets = usage.token_buckets().unwrap();
    assert_eq!(buckets.get(TaskTokenBucketV2::VideoOutput), Some(57_920));
    assert_eq!(buckets.get(TaskTokenBucketV2::ReasoningOutput), Some(234));
    assert_eq!(buckets.get(TaskTokenBucketV2::ImageInput), None, "not reported is not zero");
    assert_eq!(usage.tokens(), None);
    let back = observation_json(&observation).unwrap();
    assert_eq!(back, serde_json::from_str::<Value>(&wire).unwrap());
}

#[test]
fn a_reported_zero_is_kept_and_a_null_bucket_is_not_reported() {
    let usage = bucket_usage(json!({"total_input": 0, "image_input": null}));
    let wire =
        succeeded(json!({"kind": "inline", "items": [inline_item("/data", &Value::Null)]}), usage);
    let observation = parse_observation_json(&wire).unwrap();
    let TaskObservationV2::Succeeded { usage, .. } = &observation else { panic!() };
    let buckets = usage.token_buckets().unwrap();
    assert_eq!(buckets.get(TaskTokenBucketV2::TotalInput), Some(0));
    assert_eq!(buckets.get(TaskTokenBucketV2::ImageInput), None);
    assert_eq!(
        observation_json(&observation).unwrap()["usage"]["token_buckets"],
        json!({"total_input": 0})
    );
}

#[test]
fn bucket_words_are_a_closed_vocabulary() {
    for bad in ["thought", "TOTAL_INPUT", "audio", "", "cached_audio_input"] {
        let usage = bucket_usage(json!({ bad: 1 }));
        assert!(
            parse_observation_json(&succeeded(
                json!({"kind": "inline", "items": [inline_item("/data", &Value::Null)]}),
                usage
            ))
            .is_err(),
            "{bad}"
        );
    }
    for bucket in TaskTokenBucketV2::ALL {
        assert_eq!(TaskTokenBucketV2::from_word(bucket.word()), Some(bucket));
    }
    assert_eq!(TaskTokenBucketV2::ALL.len(), 14);
}

#[test]
fn bucket_counts_are_bounded_and_the_map_is_not_empty() {
    for bad in [
        json!({}),
        json!({"total_input": -1}),
        json!({"total_input": 1.5}),
        json!({"total_input": 9_223_372_036_854_775_808_u64}),
        json!({"total_input": null}),
    ] {
        let wire = succeeded(
            json!({"kind": "inline", "items": [inline_item("/data", &Value::Null)]}),
            bucket_usage(bad.clone()),
        );
        assert!(parse_observation_json(&wire).is_err(), "{bad}");
    }
    assert_eq!(TaskTokenBucketsV2::new([]), Err(TaskContractErrorV2::InvalidUsage));
    assert_eq!(
        TaskTokenBucketsV2::new([
            (TaskTokenBucketV2::TotalInput, 1),
            (TaskTokenBucketV2::TotalInput, 2)
        ]),
        Err(TaskContractErrorV2::InvalidUsage),
        "a bucket reported twice is refused"
    );
}

#[test]
fn buckets_and_a_single_token_count_are_exclusive() {
    let mut usage = bucket_usage(omni_buckets());
    usage["tokens"] = json!(58_171);
    let wire =
        succeeded(json!({"kind": "inline", "items": [inline_item("/data", &Value::Null)]}), usage);
    assert!(parse_observation_json(&wire).is_err());
    let buckets = TaskTokenBucketsV2::new([(TaskTokenBucketV2::TotalInput, 1)]).unwrap();
    assert!(
        TaskUsageFactsV2::new(None, None, Some(1))
            .unwrap()
            .with_token_buckets(Some(buckets))
            .is_err()
    );
}

#[test]
fn inline_sets_follow_the_contract_seven_rules_and_name_distinct_pointers() {
    let frame = |pointer: &str| video(pointer).with_role(TaskArtifactRoleV2::LastFrame);
    assert!(TaskArtifactRefV2::inline(vec![]).is_err());
    assert!(TaskArtifactRefV2::inline(vec![frame("/a")]).is_err(), "a primary is required");
    assert!(TaskArtifactRefV2::inline(vec![video("/a"), video("/a")]).is_err());
    assert!(TaskArtifactRefV2::inline(vec![video("/a"), frame("/b")]).is_ok());
    let many = (0..=south_contracts::MAX_ARTIFACT_URLS).map(|i| video(&format!("/{i}"))).collect();
    assert!(TaskArtifactRefV2::inline(many).is_err());
    for (pointer, media_type) in [
        ("", "video/mp4"),
        ("data", "video/mp4"),
        ("/a~2", "video/mp4"),
        ("/a", "video"),
        ("/a", ""),
    ] {
        assert!(
            TaskInlineArtifactV2::new(pointer, ArtifactEncodingV1::Base64, media_type).is_err(),
            "{pointer} {media_type}"
        );
    }
    let smuggled = TaskObservationV2::Succeeded {
        artifacts: TaskArtifactRefV2::Inline(vec![video("/a"), video("/a")]),
        usage: TaskUsageFactsV2::default(),
    };
    assert!(observation_json(&smuggled).is_err());
}

#[test]
fn an_inline_item_states_every_key() {
    let mut item = inline_item("/data", &Value::Null);
    item.as_object_mut().unwrap().remove("role");
    let wire = succeeded(json!({"kind": "inline", "items": [item]}), seven_usage());
    assert!(parse_observation_json(&wire).is_err(), "the role key is required, as for urls");
    let mut item = inline_item("/data", &Value::Null);
    item["encoding"] = json!("hex");
    let wire = succeeded(json!({"kind": "inline", "items": [item]}), seven_usage());
    assert!(parse_observation_json(&wire).is_err(), "the encoding vocabulary is closed");
}

#[test]
fn an_accepted_terminal_receipt_never_carries_inline_artifacts() {
    let observation: Value = serde_json::from_str(&succeeded(
        json!({"kind": "inline", "items": [inline_item("/data", &Value::Null)]}),
        bucket_usage(omni_buckets()),
    ))
    .unwrap();
    let wire = json!({"outcome": "accepted-terminal", "observation": observation});
    assert!(
        parse_submit_outcome_json(&wire.to_string()).is_err(),
        "inline bytes are resolved only against an observation's view"
    );
}

#[test]
fn bucket_ceilings_round_trip_in_the_request_estimate() {
    let wire = json!({"descriptor": {"method": "POST", "url": "https://upstream.example/tasks"},
        "locator": {"schema_version": 1, "route": "interactions"},
        "request_estimate": {"requested_seconds": 10.0, "milliunits_per_second": null,
            "resolution": null, "input_image_count": 0, "tokens_per_second": null,
            "requested_outputs": 1, "bucket_ceilings": {"video_output": 57920}},
        "immutable_body_paths": ["model"]});
    let prepared = parse_prepared_task_json(&wire.to_string()).unwrap();
    assert_eq!(
        prepared.request_estimate.bucket_ceilings().unwrap().get(TaskTokenBucketV2::VideoOutput),
        Some(57_920)
    );
    assert_eq!(prepared_task_json(&prepared).unwrap(), wire);
    let mut empty = wire;
    empty["request_estimate"]["bucket_ceilings"] = json!({});
    assert!(parse_prepared_task_json(&empty.to_string()).is_err());
}

const CLIP: &[u8] = b"\x00\x00\x00\x18ftypmp42 a short stand-in for a clip";

fn elided_view() -> (String, BlobSetV1) {
    let body = json!({"status": "completed", "steps": [
        {"type": "thought", "signature": "sig"},
        {"type": "model_output", "content": [{"type": "video", "mime_type": "video/mp4",
            "data": south_contracts::media::encode_base64(CLIP)}]}]});
    let paths = [PathPatternV1::parse("/steps/*/content/*/data").unwrap()];
    let (view, blobs) = elide_v1(body.to_string().as_bytes(), &paths, &MediaLimitsV1::V1).unwrap();
    (view.into_string(), BlobSetV1::new(blobs).unwrap())
}

#[test]
fn the_host_resolves_an_inline_pointer_against_the_call_blobs() {
    let (view, blobs) = elided_view();
    assert!(view.contains("$south.blob"), "the clip left the view");
    let bytes =
        resolve_task_inline_artifact_v1(&view, &blobs, &video("/steps/1/content/0/data"), 1 << 20)
            .unwrap();
    assert_eq!(bytes, CLIP);
    for bad in ["/steps/0/signature", "/steps/1/content/0/mime_type", "/steps/9/content/0/data"] {
        assert_eq!(
            resolve_task_inline_artifact_v1(&view, &blobs, &video(bad), 1 << 20),
            Err(TaskContractErrorV2::InlineArtifactUnresolved),
            "{bad}: a pointer that does not land on a blob of this call"
        );
    }
    assert_eq!(
        resolve_task_inline_artifact_v1(&view, &blobs, &video("/steps/1/content/0/data"), 4),
        Err(TaskContractErrorV2::InlineArtifactUnresolved),
        "above the host's artifact limit"
    );
    assert_eq!(
        resolve_task_inline_artifact_v1(
            &view,
            &BlobSetV1::default(),
            &video("/steps/1/content/0/data"),
            1 << 20
        ),
        Err(TaskContractErrorV2::InlineArtifactUnresolved),
        "a blob of another call"
    );
}

#[test]
fn the_host_refuses_an_inline_string_that_does_not_decode() {
    let body = json!({"data": "not base64!"});
    let paths = [PathPatternV1::parse("/data").unwrap()];
    let (view, blobs) = elide_v1(body.to_string().as_bytes(), &paths, &MediaLimitsV1::V1).unwrap();
    let blobs = BlobSetV1::new(blobs).unwrap();
    assert_eq!(
        resolve_task_inline_artifact_v1(view.as_str(), &blobs, &video("/data"), 1 << 20),
        Err(TaskContractErrorV2::InlineArtifactUnresolved)
    );
    let empty = json!({"data": ""});
    let (view, blobs) = elide_v1(empty.to_string().as_bytes(), &paths, &MediaLimitsV1::V1).unwrap();
    let blobs = BlobSetV1::new(blobs).unwrap();
    assert_eq!(
        resolve_task_inline_artifact_v1(view.as_str(), &blobs, &video("/data"), 1 << 20),
        Err(TaskContractErrorV2::InlineArtifactUnresolved),
        "an empty artifact is not an artifact"
    );
}

#[test]
fn the_host_replaces_artifact_markers_with_its_own_paths() {
    let artifacts = TaskArtifactRefV2::inline(vec![video("/a")]).unwrap();
    let mut render = json!({"created": 1, "data": [{"url": {"$south.artifact": {"index": 0}}}]});
    let filled = fill_task_artifact_markers_v1(&mut render, &artifacts, |i| {
        format!("/v1/video/tasks/t/artifacts/art_{i}")
    })
    .unwrap();
    assert_eq!(filled, 1);
    assert_eq!(
        render,
        json!({"created": 1, "data": [{"url": "/v1/video/tasks/t/artifacts/art_0"}]})
    );
    for bad in [
        json!({"data": [{"url": {"$south.artifact": {"index": 1}}}]}),
        json!({"data": [{"url": {"$south.artifact": {"index": 0}, "extra": 1}}]}),
        json!({"data": [{"url": {"$south.artifact": {"index": 0, "as": "url"}}}]}),
        json!({"data": [{"url": {"$south.blob": {"id": "b0"}}}]}),
        json!({"data": [{"url": "x", "$south.ref": 1}]}),
    ] {
        let mut render = bad.clone();
        assert_eq!(
            fill_task_artifact_markers_v1(&mut render, &artifacts, |i| format!("art_{i}")),
            Err(TaskContractErrorV2::ReferenceIntegrity),
            "{bad}"
        );
    }
}

proptest::proptest! {
    /// Any bucket set and any inline item the codec accepts survive an encode and decode intact,
    /// and a bucket word outside the vocabulary is never accepted.
    #[test]
    fn accepted_buckets_and_inline_items_round_trip(
        counts in proptest::collection::vec(proptest::option::of(0_u64..=i64::MAX as u64), 14),
        stray in "[a-z_]{1,24}",
        pointer in "(/[a-z0-9~]{0,6}){1,4}",
    ) {
        let mut buckets = serde_json::Map::new();
        for (bucket, count) in TaskTokenBucketV2::ALL.into_iter().zip(&counts) {
            if let Some(count) = count {
                buckets.insert(bucket.word().to_owned(), json!(count));
            }
        }
        let item = json!({"pointer": pointer, "encoding": "data_url", "media_type": "video/mp4",
                          "role": null});
        let wire = succeeded(json!({"kind": "inline", "items": [item]}),
                             bucket_usage(Value::Object(buckets.clone())));
        if let Ok(observation) = parse_observation_json(&wire) {
            proptest::prop_assert!(!buckets.is_empty());
            let back = observation_json(&observation).unwrap();
            proptest::prop_assert_eq!(parse_observation_json(&back.to_string()), Ok(observation));
        } else {
            proptest::prop_assert!(buckets.is_empty() || pointer.contains('~'));
        }
        if TaskTokenBucketV2::from_word(&stray).is_none() {
            let wire = succeeded(json!({"kind": "inline", "items": [inline_item("/a", &Value::Null)]}),
                                 bucket_usage(json!({ stray: 1 })));
            proptest::prop_assert!(parse_observation_json(&wire).is_err());
        }
    }
}
