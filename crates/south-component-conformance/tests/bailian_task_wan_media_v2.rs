//! Wan's `input.media[]` request shapes: wan3.0-video, wan2.7-i2v, wan2.7-r2v and
//! wan2.7-videoedit (`docs/design/2026-10-21-bailian-wan-media.md`).
//!
//! Expectations come from Alibaba Cloud Model Studio's API references, not from the
//! implementation: the media combinations and counts, the parameter ranges, the default
//! `1080P` tier and the billed `usage.duration` each model reports.
use serde_json::{Value, json};
use south_component_conformance::{
    ComponentResultV1, PreparedTaskV2, TaskComponentV2,
    reference_bailian_task_v2::BailianTaskComponentV2,
};
use south_contracts::{HostMintedValuesV1, TaskObservationV2};
use token_station_protocol::{ErrorCode, HttpResponseParts, ProviderConfig};

fn config() -> ProviderConfig {
    serde_json::from_value(
        json!({"provider":"bailian","base_url":"https://dashscope.example","auth":"provider_api_key"}),
    )
    .unwrap()
}

fn prepare_with(config: &ProviderConfig, body: &Value) -> ComponentResultV1<PreparedTaskV2> {
    BailianTaskComponentV2.build_submit_request(
        config,
        body,
        &HostMintedValuesV1::new("host-task", None).unwrap(),
    )
}

fn prepare(body: &Value) -> ComponentResultV1<PreparedTaskV2> {
    prepare_with(&config(), body)
}

/// The upstream body and the estimate of a request that must be accepted.
fn accepted(body: &Value) -> (Value, PreparedTaskV2) {
    let prepared = prepare(body).unwrap_or_else(|e| panic!("{body} must be accepted: {e:?}"));
    assert_eq!(
        prepared.descriptor.url,
        "https://dashscope.example/api/v1/services/aigc/video-generation/video-synthesis"
    );
    assert_eq!(prepared.descriptor.headers.get("x-dashscope-async"), Some("enable"));
    config().authorize(&prepared.descriptor).unwrap();
    (prepared.descriptor.body.clone().unwrap(), prepared)
}

/// A request that must be refused before dispatch as the caller's error, without echoing
/// any URL it carried.
fn refused(body: &Value) -> String {
    let error = prepare(body).expect_err(&format!("{body} must be refused"));
    assert_eq!(error.code, ErrorCode::InvalidRequest, "{body}");
    assert_eq!(error.http_status, 400, "{body}");
    assert!(!error.message.contains("PRIVATE"), "{}", error.message);
    error.message
}

fn seconds(body: &Value) -> Option<f64> {
    accepted(body).1.request_estimate.requested_seconds()
}

// ---------------------------------------------------------------- wan2.7-i2v

#[test]
fn wan27_i2v_sends_the_first_frame_as_media_never_img_url() {
    for key in ["image", "image_url", "first_frame_image"] {
        let (body, prepared) =
            accepted(&json!({"model":"wan2.7-i2v","prompt":"p",key:"https://x/first"}));
        assert_eq!(
            body["input"],
            json!({"prompt":"p","media":[{"type":"first_frame","url":"https://x/first"}]}),
            "{key}"
        );
        assert!(body["input"].get("img_url").is_none());
        assert_eq!(body["parameters"], json!({"resolution":"1080P"}));
        assert_eq!(prepared.request_estimate.requested_seconds(), Some(5.0));
        assert_eq!(prepared.request_estimate.resolution(), Some("1080P"));
        assert_eq!(prepared.request_estimate.input_image_count(), Some(1));
    }
}

#[test]
fn wan27_i2v_carries_last_frame_driving_audio_and_negative_prompt() {
    let (body, prepared) = accepted(&json!({
        "model":"wan2.7-i2v-2026-04-25","prompt":"p","negative_prompt":"blur",
        "image":"https://x/first","last_frame_url":"https://x/last","audio_url":"https://x/voice",
        "resolution":"720p","duration":12,"seed":7,"prompt_extend":false,"watermark":true
    }));
    assert_eq!(
        body["input"],
        json!({"prompt":"p","negative_prompt":"blur","media":[
            {"type":"first_frame","url":"https://x/first"},
            {"type":"last_frame","url":"https://x/last"},
            {"type":"driving_audio","url":"https://x/voice"}
        ]})
    );
    assert_eq!(
        body["parameters"],
        json!({"resolution":"720P","duration":12,"seed":7,"prompt_extend":false,"watermark":true})
    );
    assert_eq!(prepared.request_estimate.requested_seconds(), Some(12.0));
    assert_eq!(prepared.request_estimate.resolution(), Some("720P"));
    assert_eq!(prepared.request_estimate.input_image_count(), Some(2));
}

#[test]
fn wan27_i2v_continues_a_first_clip_and_reserves_the_total_it_bills() {
    let (body, prepared) = accepted(&json!({
        "model":"wan2.7-i2v","video":"https://x/clip","last_frame":"https://x/last"
    }));
    // The prompt is optional for this model.
    assert_eq!(
        body["input"],
        json!({"media":[
            {"type":"first_clip","url":"https://x/clip"},
            {"type":"last_frame","url":"https://x/last"}
        ]})
    );
    assert_eq!(prepared.request_estimate.input_image_count(), Some(1));
    // The clip is up to 10 s and the output is never shorter than it, so the billed total
    // is at most max(duration, 10).
    assert_eq!(prepared.request_estimate.requested_seconds(), Some(10.0));
    for (duration, expected) in [(4, 10.0), (10, 10.0), (15, 15.0)] {
        assert_eq!(
            seconds(&json!({"model":"wan2.7-i2v","video":"https://x/clip","duration":duration})),
            Some(expected),
            "{duration}"
        );
    }
}

#[test]
fn wan27_i2v_refuses_combinations_the_model_does_not_take() {
    for request in [
        json!({"model":"wan2.7-i2v","prompt":"p"}),
        json!({"model":"wan2.7-i2v","prompt":"p","last_frame":"PRIVATE"}),
        json!({"model":"wan2.7-i2v","prompt":"p","image":"PRIVATE","video":"PRIVATE"}),
        json!({"model":"wan2.7-i2v","prompt":"p","video":"PRIVATE","audio_url":"PRIVATE"}),
        json!({"model":"wan2.7-i2v","prompt":"p","image":"PRIVATE","image_url":"PRIVATE"}),
        json!({"model":"wan2.7-i2v","prompt":"p","image":"PRIVATE","reference_images":["PRIVATE"]}),
        json!({"model":"wan2.7-i2v","prompt":"p","image":"PRIVATE","generate_audio":false}),
        json!({"model":"wan2.7-i2v","prompt":"p","image":"PRIVATE","aspect_ratio":"16:9"}),
        json!({"model":"wan2.7-i2v","prompt":"p","image":"PRIVATE","size":"1280*720"}),
        json!({"model":"wan2.7-i2v","prompt":"p","image":""}),
        json!({"model":"wan2.7-i2v","prompt":"p","image":{"url":"PRIVATE"}}),
    ] {
        refused(&request);
    }
}

#[test]
fn wan27_i2v_refuses_durations_and_tiers_outside_the_model_range() {
    for duration in [json!(-1), json!(0), json!(1), json!(16), json!(5.5), json!("x")] {
        refused(
            &json!({"model":"wan2.7-i2v","prompt":"p","image":"https://x/a","duration":duration}),
        );
    }
    for resolution in ["480P", "4K", ""] {
        refused(
            &json!({"model":"wan2.7-i2v","prompt":"p","image":"https://x/a","resolution":resolution}),
        );
    }
}

// ---------------------------------------------------------------- wan3.0-video

#[test]
fn wan3_text_to_video_writes_the_default_tier_and_sends_no_media() {
    let (body, prepared) = accepted(&json!({"model":"wan3.0-video","prompt":"p"}));
    assert_eq!(
        body,
        json!({"model":"wan3.0-video","input":{"prompt":"p"},"parameters":{"resolution":"1080P"}})
    );
    assert_eq!(prepared.request_estimate.requested_seconds(), Some(5.0));
    assert_eq!(prepared.request_estimate.resolution(), Some("1080P"));
    assert_eq!(prepared.request_estimate.input_image_count(), Some(0));
}

#[test]
fn wan3_frames_and_parameters_map_onto_media_and_parameters() {
    let (body, prepared) = accepted(&json!({
        "model":"wan3.0-video-prime","prompt":"p","first_frame_image":"https://x/first",
        "last_frame":"https://x/last","resolution":"480P","aspect_ratio":"adaptive","ratio":"1:1",
        "generate_audio":false,"duration":12
    }));
    assert_eq!(
        body["input"]["media"],
        json!([{"type":"first_frame","url":"https://x/first"},{"type":"last_frame","url":"https://x/last"}])
    );
    assert_eq!(
        body["parameters"],
        json!({"resolution":"480P","ratio":"adaptive","audio":false,"duration":12})
    );
    assert_eq!(prepared.request_estimate.requested_seconds(), Some(12.0));
    assert_eq!(prepared.request_estimate.resolution(), Some("480P"));
    assert_eq!(prepared.request_estimate.input_image_count(), Some(2));
}

#[test]
fn wan3_references_keep_the_client_order_within_each_type() {
    let (body, prepared) = accepted(&json!({
        "model":"wan3.0-video","prompt":"Video 1 meets Image 2",
        "reference_images":["https://x/i1","https://x/i2"],
        "video":"https://x/v1","reference_videos":["https://x/v2"],
        "reference_audios":["https://x/a1","https://x/a2"]
    }));
    assert_eq!(
        body["input"]["media"],
        json!([
            {"type":"reference_image","url":"https://x/i1"},
            {"type":"reference_image","url":"https://x/i2"},
            {"type":"reference_video","url":"https://x/v1"},
            {"type":"reference_video","url":"https://x/v2"},
            {"type":"reference_audio","url":"https://x/a1"},
            {"type":"reference_audio","url":"https://x/a2"}
        ])
    );
    assert_eq!(prepared.request_estimate.input_image_count(), Some(2));
    // Billed duration with an input video is input (at most 15 s) + output, capped at 30.
    assert_eq!(prepared.request_estimate.requested_seconds(), Some(30.0));
}

#[test]
fn wan3_estimates_the_upper_bound_of_the_billed_duration() {
    let text = |duration: Value| json!({"model":"wan3.0-video","prompt":"p","duration":duration});
    let video = |duration: Value| json!({"model":"wan3.0-video","prompt":"extend Video 1","video":"https://x/v","duration":duration});
    for (request, expected) in [
        (text(json!(12)), 12.0),
        (text(json!(30)), 30.0),
        (text(json!(-1)), 30.0),
        (text(Value::Null), 5.0),
        (video(json!(5)), 20.0),
        (video(json!(10)), 25.0),
        (video(json!(20)), 30.0),
        (video(json!(-1)), 30.0),
        (video(Value::Null), 30.0),
    ] {
        assert_eq!(seconds(&request), Some(expected), "{request}");
    }
    // Smart duration reaches the upstream unchanged.
    assert_eq!(accepted(&text(json!(-1))).0["parameters"]["duration"], -1);
}

#[test]
fn wan3_media_without_a_prompt_is_accepted_but_nothing_at_all_is_not() {
    let (body, _) = accepted(&json!({"model":"wan3.0-video","image":"https://x/first"}));
    assert_eq!(body["input"], json!({"media":[{"type":"first_frame","url":"https://x/first"}]}));
    refused(&json!({"model":"wan3.0-video"}));
}

#[test]
fn wan3_refuses_combinations_and_counts_the_model_does_not_take() {
    let images = |n: usize| vec!["PRIVATE"; n];
    for request in [
        json!({"model":"wan3.0-video","prompt":"p","last_frame":"PRIVATE"}),
        json!({"model":"wan3.0-video","prompt":"p","image":"PRIVATE","reference_images":["PRIVATE"]}),
        json!({"model":"wan3.0-video","prompt":"p","last_frame":"PRIVATE","video":"PRIVATE"}),
        json!({"model":"wan3.0-video","prompt":"p","image":"PRIVATE","reference_audios":["PRIVATE"]}),
        json!({"model":"wan3.0-video","prompt":"p","reference_images":images(11)}),
        json!({"model":"wan3.0-video","prompt":"p","reference_videos":images(6)}),
        json!({"model":"wan3.0-video","prompt":"p","video":"PRIVATE","reference_videos":images(5)}),
        json!({"model":"wan3.0-video","prompt":"p","reference_audios":images(6)}),
        json!({"model":"wan3.0-video","prompt":"p","file":"PRIVATE"}),
        json!({"model":"wan3.0-video","prompt":"p","link":"PRIVATE"}),
        json!({"model":"wan3.0-video","prompt":"p","audio_url":"PRIVATE"}),
        json!({"model":"wan3.0-video","prompt":"p","negative_prompt":"n"}),
        json!({"model":"wan3.0-video","prompt":"p","generate_audio":"no"}),
        json!({"model":"wan3.0-video","prompt":"p","reference_images":"PRIVATE"}),
        json!({"model":"wan3.0-video","prompt":"p","reference_images":[7]}),
        json!({"model":"wan3.0-video","prompt":7}),
    ] {
        refused(&request);
    }
    for duration in [json!(-2), json!(0), json!(1), json!(31), json!(2.5)] {
        refused(&json!({"model":"wan3.0-video","prompt":"p","duration":duration}));
    }
    refused(&json!({"model":"wan3.0-video","prompt":"p","resolution":"4K"}));
}

#[test]
fn file_and_link_inputs_are_refused_with_a_named_reason() {
    let message = refused(&json!({"model":"wan3.0-video","prompt":"p","file":"PRIVATE"}));
    assert!(message.contains("file"), "{message}");
}

// ---------------------------------------------------------------- wan2.7-r2v

#[test]
fn wan27_r2v_sends_references_and_an_optional_first_frame() {
    let (body, prepared) = accepted(&json!({
        "model":"wan2.7-r2v","prompt":"Image 1 talks to Video 1","negative_prompt":"n",
        "image":"https://x/first","reference_images":["https://x/i1"],
        "reference_videos":["https://x/v1"],"aspect_ratio":"9:16","duration":8
    }));
    assert_eq!(
        body["input"],
        json!({"prompt":"Image 1 talks to Video 1","negative_prompt":"n","media":[
            {"type":"first_frame","url":"https://x/first"},
            {"type":"reference_image","url":"https://x/i1"},
            {"type":"reference_video","url":"https://x/v1"}
        ]})
    );
    assert_eq!(body["parameters"], json!({"resolution":"1080P","ratio":"9:16","duration":8}));
    assert_eq!(prepared.request_estimate.input_image_count(), Some(2));
    // At most 5 s of input video is billed on top of the output.
    assert_eq!(prepared.request_estimate.requested_seconds(), Some(13.0));
}

#[test]
fn wan27_r2v_estimates_and_duration_ranges_follow_the_video_input() {
    let images = json!({"model":"wan2.7-r2v","prompt":"p","reference_images":["https://x/i"]});
    let videos = json!({"model":"wan2.7-r2v","prompt":"p","reference_videos":["https://x/v"]});
    assert_eq!(seconds(&images), Some(5.0));
    assert_eq!(seconds(&videos), Some(10.0));
    let with = |base: &Value, duration: i64| {
        let mut request = base.clone();
        request["duration"] = json!(duration);
        request
    };
    assert_eq!(seconds(&with(&images, 15)), Some(15.0));
    assert_eq!(seconds(&with(&videos, 10)), Some(15.0));
    refused(&with(&videos, 11));
    refused(&with(&images, 16));
    refused(&with(&images, -1));
}

#[test]
fn wan27_r2v_refuses_missing_or_excess_references_and_foreign_inputs() {
    let five = vec!["PRIVATE"; 5];
    for request in [
        json!({"model":"wan2.7-r2v","prompt":"p"}),
        json!({"model":"wan2.7-r2v","prompt":"p","image":"PRIVATE"}),
        json!({"model":"wan2.7-r2v","prompt":"p","reference_images":five,"reference_videos":["PRIVATE"]}),
        json!({"model":"wan2.7-r2v","reference_images":["PRIVATE"]}),
        json!({"model":"wan2.7-r2v","prompt":"p","reference_images":["PRIVATE"],"last_frame":"PRIVATE"}),
        json!({"model":"wan2.7-r2v","prompt":"p","reference_images":["PRIVATE"],"video":"PRIVATE"}),
        json!({"model":"wan2.7-r2v","prompt":"p","reference_images":["PRIVATE"],"reference_audios":["PRIVATE"]}),
    ] {
        refused(&request);
    }
}

// ---------------------------------------------------------------- wan2.7-videoedit

#[test]
fn wan27_videoedit_sends_the_video_and_references() {
    let (body, prepared) = accepted(&json!({
        "model":"wan2.7-videoedit","prompt":"clay style","video":"https://x/v",
        "reference_images":["https://x/i1","https://x/i2"],"audio_setting":"origin",
        "ratio":"1:1","resolution":"720P"
    }));
    assert_eq!(
        body["input"],
        json!({"prompt":"clay style","media":[
            {"type":"video","url":"https://x/v"},
            {"type":"reference_image","url":"https://x/i1"},
            {"type":"reference_image","url":"https://x/i2"}
        ]})
    );
    assert_eq!(
        body["parameters"],
        json!({"resolution":"720P","ratio":"1:1","audio_setting":"origin"})
    );
    assert_eq!(prepared.request_estimate.input_image_count(), Some(2));
    // Input (2-10 s) + output (the input's length, or `duration`).
    assert_eq!(prepared.request_estimate.requested_seconds(), Some(20.0));
}

#[test]
fn wan27_videoedit_estimates_input_plus_output() {
    let edit = |duration: Value| json!({"model":"wan2.7-videoedit","video":"https://x/v","duration":duration});
    for (duration, expected) in
        [(Value::Null, 20.0), (json!(0), 20.0), (json!(6), 16.0), (json!(10), 20.0)]
    {
        assert_eq!(seconds(&edit(duration.clone())), Some(expected), "{duration}");
    }
    for duration in [json!(1), json!(11), json!(-1)] {
        refused(&edit(duration));
    }
}

#[test]
fn wan27_videoedit_refuses_missing_video_and_foreign_inputs() {
    for request in [
        json!({"model":"wan2.7-videoedit","prompt":"p"}),
        json!({"model":"wan2.7-videoedit","prompt":"p","reference_images":["PRIVATE"]}),
        json!({"model":"wan2.7-videoedit","video":"PRIVATE","reference_images":vec!["PRIVATE";5]}),
        json!({"model":"wan2.7-videoedit","video":"PRIVATE","image":"PRIVATE"}),
        json!({"model":"wan2.7-videoedit","video":"PRIVATE","reference_videos":["PRIVATE"]}),
        json!({"model":"wan2.7-videoedit","video":"PRIVATE","resolution":"480P"}),
    ] {
        refused(&request);
    }
}

// ---------------------------------------------------------------- shape selection

#[test]
fn the_upstream_family_selects_the_media_shape_for_an_alias() {
    let mut c = config();
    c.extensions.insert("upstream_model_family".into(), json!("wan3.0-video-prime"));
    let prepared =
        prepare_with(&c, &json!({"model":"reseller-alias","prompt":"p","image":"https://x/a"}))
            .unwrap();
    let body = prepared.descriptor.body.unwrap();
    assert_eq!(body["model"], "reseller-alias");
    assert_eq!(body["input"]["media"], json!([{"type":"first_frame","url":"https://x/a"}]));
}

#[test]
fn other_wan_models_keep_their_existing_wire_shape() {
    for (model, expected) in [
        ("wan2.7-t2v", json!({"prompt":"p","img_url":"https://x/a"})),
        ("wan2.6-i2v", json!({"prompt":"p","img_url":"https://x/a"})),
        ("wan2.6-i2v-flash", json!({"prompt":"p","img_url":"https://x/a"})),
        ("wan2.6-r2v", json!({"prompt":"p","img_url":"https://x/a"})),
    ] {
        let body = prepare(&json!({"model":model,"prompt":"p","image":"https://x/a"}))
            .unwrap()
            .descriptor
            .body
            .unwrap();
        assert_eq!(body["input"], expected, "{model}");
        assert_eq!(body["parameters"], json!({}), "{model}");
    }
}

// ---------------------------------------------------------------- settlement

#[test]
fn settlement_reads_the_billed_total_of_input_and_output() {
    let response: HttpResponseParts = serde_json::from_value(json!({"status":200,"body":json!({
        "output":{"task_status":"SUCCEEDED","video_url":"https://media.example/v"},
        "usage":{"video_count":1,"duration":13.0,"input_video_duration":8.0,
                 "output_video_duration":5.0,"fps":30,"SR":1080,"ratio":"16:9"}
    }).to_string()}))
    .unwrap();
    let TaskObservationV2::Succeeded { usage, .. } =
        BailianTaskComponentV2.parse_observation(&response).unwrap()
    else {
        panic!("a success with a video stays a success");
    };
    assert_eq!(usage.seconds(), Some(13.0));
}
