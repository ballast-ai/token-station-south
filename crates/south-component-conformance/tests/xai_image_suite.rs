//! `south.image-component.v1` against the `image-xai` native reference and its shipped fixture
//! pack, and the behaviours of design record `2026-10-15-image-i3-7-xai.md` §2 to §4 pinned by
//! name.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::image_fixture::ImageFixturePackV1;
use south_component_conformance::reference_xai_image::{XaiImageReferenceV1, upstream_cost};
use south_component_conformance::{ImageComponentV1, ImageOutcomeV1, run_image_component_suite_v1};
use south_contracts::image::{ImageCallContextV1, ImageRenderContextV1};
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

const STANDARD: &str = "grok-imagine-image";
const V2: &str = "grok-imagine-image-2.0";
const QUALITY: &str = "grok-imagine-image-quality";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("../../components/image-xai/manifest.json")).unwrap(),
    )
    .unwrap()
}

fn pack() -> ImageFixturePackV1 {
    ImageFixturePackV1::load(&root().join("fixtures-image-xai")).unwrap()
}

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "xai-image",
        "base_url": "https://api.x.ai",
        "auth": "provider_api_key",
    }))
    .unwrap()
}

fn context(model: &str) -> ImageCallContextV1 {
    serde_json::from_value(json!({"upstream_model": model, "metering_required": ["images"]}))
        .unwrap()
}

fn blob(id: &str, head: &str) -> Value {
    json!({"$south.blob": {"id": id, "bytes": head.len(), "head": head}})
}

fn reference(id: &str) -> Value {
    json!({"$south.ref": {"blob": id, "transform": "as_is"}})
}

/// The prepared call as JSON, or the refusal.
fn prepare(model: &str, body: &Value) -> Result<Value, Value> {
    XaiImageReferenceV1
        .prepare(&config(), &json!({ "json": body }), &context(model))
        .map(|prepared| serde_json::from_str(&prepared.to_json()).unwrap())
        .map_err(|error| serde_json::to_value(error).unwrap())
}

fn refusal(model: &str, body: &Value) -> String {
    prepare(model, body).unwrap_err()["message"].as_str().unwrap().to_owned()
}

fn template(prepared: &Value) -> &Value {
    &prepared["descriptor"]["body"]["json"]["template"]
}

fn answer(status: u16, body: &Value) -> ImageOutcomeV1 {
    let view = json!({"status": status, "headers": [], "body": {"json": body}});
    XaiImageReferenceV1.parse_response(&json!({}), &view).unwrap()
}

#[test]
fn the_shipped_manifest_passes_gate_one() {
    assert_eq!(manifest().validate(), Ok(()));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let report = run_image_component_suite_v1(&XaiImageReferenceV1, &pack(), &manifest());
    let failures: Vec<String> = report
        .failures()
        .map(|outcome| format!("{} {}: {}", outcome.check.as_str(), outcome.case, outcome.detail()))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// XI-Q1: the six Grok Imagine ids are declared with both operations, the four source-image keys
/// as `input_image`, `url` and `b64_json`, `images` only, and the tier dimensions H5 §8.2 reports;
/// any other configured id is not.
#[test]
fn only_the_six_imagine_ids_are_declared() {
    let mut config = config();
    config.models = serde_json::from_value(json!([
        {"model": "grok-2-image-1212"}, {"model": V2}, {"model": STANDARD}, {"model": QUALITY},
        {"model": "grok-imagine-image-pro"}, {"model": "grok-imagine-image-quality-latest"},
        {"model": "grok-imagine-image-quality-20260403"}, {"model": "grok-imagine-video"},
    ]))
    .unwrap();
    let models =
        serde_json::to_value(XaiImageReferenceV1.model_capabilities(&config).unwrap()).unwrap();
    let ids: Vec<&str> =
        models.as_array().unwrap().iter().map(|m| m["model"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        [
            V2,
            STANDARD,
            QUALITY,
            "grok-imagine-image-pro",
            "grok-imagine-image-quality-latest",
            "grok-imagine-image-quality-20260403"
        ]
    );
    for model in models.as_array().unwrap() {
        let id = model["model"].as_str().unwrap();
        assert_eq!(model["operations"], json!(["generate", "edit"]), "{id}");
        assert_eq!(
            model["input_roles"],
            json!([
                {"key": "images", "role": "input_image"},
                {"key": "image", "role": "input_image"},
                {"key": "image_url", "role": "input_image"},
                {"key": "image_urls", "role": "input_image"},
            ]),
            "{id}"
        );
        assert_eq!(model["response_formats"], json!(["url", "b64_json"]), "{id}");
        assert_eq!(model["metering_forms"], json!(["images"]), "{id}");
        assert_eq!(model["token_buckets"], json!([]), "{id}");
        let dimensions =
            if id == V2 { json!(["resolution", "quality"]) } else { json!(["resolution"]) };
        assert_eq!(model["tier_dimensions"], dimensions, "{id}");
        assert_eq!(model["repeat"], json!(false), "{id}");
        assert_eq!(
            model["request_elision_paths"],
            json!([
                "/image",
                "/image/url",
                "/images/*",
                "/images/*/url",
                "/image_url",
                "/image_url/*",
                "/image_urls/*"
            ]),
            "{id}"
        );
    }
}

/// §3: `parse_xai_resolution_hint` and the native precedence (`resolution` over `size`; `size`
/// always removed; absent or `null` is billed 1K and sends nothing).
#[test]
fn the_resolution_table_is_the_native_one() {
    for (body, sent, word) in [
        (json!({}), None, "1k"),
        (json!({"resolution": null}), Some(json!(null)), "1k"),
        (json!({"resolution": ""}), Some(json!("1k")), "1k"),
        (json!({"resolution": "1K"}), Some(json!("1k")), "1k"),
        (json!({"resolution": "2K"}), Some(json!("2k")), "2k"),
        (json!({"resolution": " 2 k "}), Some(json!("2k")), "2k"),
        (json!({"resolution": "2_K"}), Some(json!("2k")), "2k"),
        (json!({"resolution": "2-k"}), Some(json!("2k")), "2k"),
        (json!({"resolution": "1024"}), Some(json!("1k")), "1k"),
        (json!({"resolution": "2048*2048"}), Some(json!("2k")), "2k"),
        (json!({"size": "2048x2048"}), Some(json!("2k")), "2k"),
        (json!({"size": "1024x1024"}), Some(json!("1k")), "1k"),
        (json!({"size": null}), None, "1k"),
        (json!({"resolution": "1k", "size": "2048x2048"}), Some(json!("1k")), "1k"),
        (json!({"resolution": "2k", "size": 5}), Some(json!("2k")), "2k"),
        (json!({"resolution": null, "size": "2k"}), Some(json!("2k")), "2k"),
    ] {
        let mut request = body.clone();
        request["prompt"] = json!("p");
        let prepared = prepare(STANDARD, &request).unwrap();
        let template = template(&prepared);
        assert_eq!(template.get("resolution").cloned(), sent, "{body}");
        assert!(template.get("size").is_none(), "{body}: `size` is never forwarded");
        assert_eq!(prepared["facts"]["tier"], json!({"resolution": word}), "{body}");
        assert!(prepared["facts"].get("size").is_none(), "{body}");
    }
    for (field, value) in [
        ("resolution", "4k"),
        ("size", "1536x1024"),
        ("size", "1024X768"),
        ("resolution", "2kk"),
        ("size", "auto"),
    ] {
        assert_eq!(
            refusal(STANDARD, &json!({"prompt": "p", field: value})),
            format!(
                "xAI `{field}` must be 1k/2k (or OpenAI-style 1024x1024/2048x2048), got '{value}'"
            )
        );
    }
    assert_eq!(
        refusal(STANDARD, &json!({"prompt": "p", "size": 1024})),
        "xAI `size` must be a string"
    );
    assert_eq!(
        refusal(STANDARD, &json!({"prompt": "p", "resolution": true})),
        "xAI `resolution` must be a string"
    );
}

/// H5 §8.2 and X-Q1 (lv, 2026-10-14): the standard and quality-family ids report the resolution
/// only; 2.0 reports a pinned quality as a tier word and otherwise the candidates `low`, `medium`
/// with the default `low` on a generation and `medium` on an edit.
#[test]
fn tier_facts_follow_the_host_price_list_plan() {
    let image = json!({"type": "image_url", "url": blob("b0", "https://example.com/a.png")});
    for (model, body, tier, candidates) in [
        (STANDARD, json!({"quality": "low"}), json!({"resolution": "1k"}), None),
        (QUALITY, json!({"resolution": "2k"}), json!({"resolution": "2k"}), None),
        (
            V2,
            json!({"quality": "low", "resolution": "2k"}),
            json!({"resolution": "2k", "quality": "low"}),
            None,
        ),
        (
            V2,
            json!({"quality": " MEDIUM "}),
            json!({"resolution": "1k", "quality": "medium"}),
            None,
        ),
        (V2, json!({}), json!({"resolution": "1k"}), Some("low")),
        (V2, json!({"quality": "auto"}), json!({"resolution": "1k"}), Some("low")),
        (V2, json!({"image": image}), json!({"resolution": "1k"}), Some("medium")),
        (
            V2,
            json!({"image": image, "quality": "Auto", "resolution": "2k"}),
            json!({"resolution": "2k"}),
            Some("medium"),
        ),
    ] {
        let mut request = body.clone();
        request["prompt"] = json!("p");
        let facts = prepare(model, &request).unwrap()["facts"].clone();
        assert_eq!(facts["tier"], tier, "{model} {body}");
        let resolution = tier["resolution"].clone();
        let expected = candidates.map(|default| {
            json!({
                "candidates": [
                    {"resolution": resolution, "quality": "low"},
                    {"resolution": resolution, "quality": "medium"},
                ],
                "default": {"resolution": resolution, "quality": default},
            })
        });
        assert_eq!(facts.get("tier_candidates").cloned(), expected, "{model} {body}");
    }
    // The quality is priced on its normalised word but forwarded as the client wrote it.
    let prepared = prepare(V2, &json!({"prompt": "p", "quality": " MEDIUM "})).unwrap();
    assert_eq!(template(&prepared)["quality"], " MEDIUM ");
    for value in [json!("high"), json!(5), json!("")] {
        assert_eq!(
            refusal(V2, &json!({"prompt": "p", "quality": value})),
            format!(
                "xAI `quality` for grok-imagine-image-2.0 must be low, medium or auto (the price \
                 card has no other tier), got {value}"
            )
        );
    }
}

/// Record §9.1: `usage.cost_in_usd_ticks` (1 USD = 10^10 ticks) becomes decimal dollars with up to
/// ten places; anything that is not a non-negative JSON integer within `i64` is absent.
#[test]
fn the_upstream_cost_is_the_ticks_in_dollars() {
    for (ticks, amount) in [
        (json!(200_000_000), Some("0.02")),
        (json!(400_000_000), Some("0.04")),
        (json!(800_000_000), Some("0.08")),
        (json!(1_100_000_000), Some("0.11")),
        (json!(123_450_000), Some("0.012345")),
        (json!(1), Some("0.0000000001")),
        (json!(0), Some("0")),
        (json!(30_000_000_000_u64), Some("3")),
        (json!(30_000_000_001_u64), Some("3.0000000001")),
        (json!(i64::MAX), Some("922337203.6854775807")),
        (json!(9_223_372_036_854_775_808_u64), None),
        (json!(-600_000_000), None),
        (json!(600_000_000.0), None),
        (json!("600000000"), None),
        (json!(null), None),
    ] {
        let body = json!({"usage": {"cost_in_usd_ticks": ticks}});
        let cost = upstream_cost(&body).map(|cost| serde_json::to_value(cost).unwrap());
        assert_eq!(
            cost,
            amount.map(|amount| json!({"currency": "USD", "amount": amount})),
            "{ticks}"
        );
    }
    assert_eq!(upstream_cost(&json!({})), None);
    assert_eq!(upstream_cost(&json!({"usage": {}})), None);
}

/// XI-Q2: a body naming a source image (non-null) is an edit and goes to `v1/images/edits`; any
/// other a generation.
#[test]
fn the_operation_follows_the_source_image_fields() {
    let url = blob("b0", "https://example.com/a.png");
    for (body, operation, path) in [
        (json!({}), "generate", "v1/images/generations"),
        (json!({"image": null, "images": null}), "generate", "v1/images/generations"),
        (json!({"image_url": null}), "generate", "v1/images/generations"),
        (json!({"image": {"url": url}}), "edit", "v1/images/edits"),
        (json!({"images": [{"url": url}]}), "edit", "v1/images/edits"),
        (json!({"image_url": url}), "edit", "v1/images/edits"),
        (json!({"image_urls": [url]}), "edit", "v1/images/edits"),
    ] {
        let mut request = body.clone();
        request["prompt"] = json!("p");
        let prepared = prepare(STANDARD, &request).unwrap();
        assert_eq!(prepared["facts"]["operation"], operation, "{body}");
        assert_eq!(prepared["descriptor"]["path"], path, "{body}");
    }
    let multipart = XaiImageReferenceV1
        .prepare(&config(), &json!({"multipart": {"parts": []}}), &context(STANDARD))
        .unwrap_err();
    assert_eq!(multipart.code, ErrorCode::InvalidRequest);
    assert!(multipart.message.starts_with("/v1/images/edits for xAI grok-imagine models requires"));
}

/// §3: `normalize_xai_image_edit_body`'s aliases and `xai_image_edit_input_count`'s refusals in the
/// native order; the per-model maximum is catalog data (record §8, R-1 = A), so four images on the
/// standard id pass here and the host refuses them.
#[test]
fn edit_aliases_and_counts_follow_the_native_arm() {
    let (a, b) = (blob("b0", "https://e.test/a.png"), blob("b1", "https://e.test/b.png"));
    let prepared = prepare(STANDARD, &json!({"prompt": "p", "image_url": a})).unwrap();
    assert_eq!(template(&prepared)["image"], json!({"type": "image_url", "url": reference("b0")}));
    assert!(template(&prepared).get("image_url").is_none());
    assert_eq!(prepared["facts"]["inputs"]["input_image"], 1);
    for key in ["image_url", "image_urls"] {
        let prepared = prepare(STANDARD, &json!({"prompt": "p", key: [a, b]})).unwrap();
        assert_eq!(
            template(&prepared)["images"],
            json!([
                {"type": "image_url", "url": reference("b0")},
                {"type": "image_url", "url": reference("b1")},
            ]),
            "{key}"
        );
        assert_eq!(prepared["facts"]["inputs"]["input_image"], 2, "{key}");
    }
    let four: Vec<Value> = (0..4).map(|i| json!({"url": blob(&format!("b{i}"), "u")})).collect();
    let prepared = prepare(STANDARD, &json!({"prompt": "p", "images": four})).unwrap();
    assert_eq!(prepared["facts"]["inputs"]["input_image"], 4);
    // `image` counts one whatever it holds, as the native billing count does.
    let prepared = prepare(STANDARD, &json!({"prompt": "p", "image": [{"url": a}, {"url": b}]}));
    assert_eq!(prepared.unwrap()["facts"]["inputs"]["input_image"], 1);

    for (body, message) in [
        (
            json!({"image": {"url": a}, "image_url": b}),
            "xAI image edits accept only one source-image field: use `image`, `images`, \
             `image_url`, or `image_urls`, not multiple forms",
        ),
        (
            json!({"images": [{"url": a}], "image_urls": null}),
            "xAI image edits accept only one source-image field: use `image`, `images`, \
             `image_url`, or `image_urls`, not multiple forms",
        ),
        (
            json!({"image_url": a, "image_urls": null}),
            "xAI image edits accept only one of `image_url` or `image_urls`",
        ),
        (
            json!({"image_url": 5}),
            "xAI `image_url` must be a string URL/data URI or an array of them (got 5)",
        ),
        (
            json!({"image_url": {"url": a}}),
            "xAI `image_url` must be a string URL/data URI or an array of them (got an object)",
        ),
        (
            json!({"image_urls": "x"}),
            "xAI `image_urls` must be an array of string URLs/data URIs (got \"x\")",
        ),
        (
            json!({"image_url": [a, 5]}),
            "xAI image URL arrays must contain only string URLs/data URIs",
        ),
        (
            json!({"image": {"url": a}, "images": [{"url": b}]}),
            "xAI image edits accept either `image` or `images`, not both",
        ),
        (json!({"images": {"url": a}}), "xAI `images` must be an array"),
        (json!({"images": []}), "xAI image edits need at least 1 input image, got 0"),
        (
            json!({"images": vec![json!({}); 65]}),
            "xAI image edits accept at most 64 input images (got 65)",
        ),
        // The native order: `n`, the resolution, the aliases, the count, then the quality.
        (
            json!({"n": 11, "resolution": "8k"}),
            "'n' must be in 1..=10 for xAI /v1/images/edits, got 11",
        ),
        (
            json!({"resolution": "8k", "image_url": 5}),
            "xAI `resolution` must be 1k/2k (or OpenAI-style 1024x1024/2048x2048), got '8k'",
        ),
    ] {
        let mut request = body.clone();
        request["prompt"] = json!("p");
        if request.get("n").is_some() {
            request["image"] = json!({"url": a});
        }
        assert_eq!(refusal(STANDARD, &request), message, "{body}");
    }
    assert!(
        refusal(V2, &json!({"prompt": "p", "quality": "high", "image_url": 5}))
            .starts_with("xAI `image_url` must be")
    );
}

/// §3: every field the native arm does not touch passes through (an elided string as an `as_is`
/// reference); `model` is the upstream id; the immutable paths are the native owned fields.
#[test]
fn the_body_passes_through() {
    let prepared = prepare(
        QUALITY,
        &json!({
            "model": "xai/grok-imagine-image-quality",
            "prompt": blob("b0", "a long prompt"),
            "n": 2.0,
            "size": "2048x2048",
            "response_format": "b64_json",
            "aspect_ratio": "16:9",
            "user": "u-1",
            "seed": {"nested": [blob("b1", "x")]},
        }),
    )
    .unwrap();
    assert_eq!(
        template(&prepared),
        &json!({
            "model": QUALITY,
            "prompt": reference("b0"),
            "n": 2.0,
            "resolution": "2k",
            "response_format": "b64_json",
            "aspect_ratio": "16:9",
            "user": "u-1",
            "seed": {"nested": [reference("b1")]},
        })
    );
    assert_eq!(prepared["facts"]["requested_outputs"], 2);
    assert_eq!(
        prepared["descriptor"]["auth"],
        json!({"arm": "bearer", "slot": "provider_api_key"})
    );
    assert!(prepared["descriptor"].get("headers").is_none());
    assert_eq!(
        prepared["immutable_body_paths"],
        json!(["model", "n", "size", "image_size", "quality", "resolution", "response_format"])
    );
    let edit = prepare(QUALITY, &json!({"prompt": "p", "image": {"url": blob("b0", "u")}}));
    assert_eq!(
        edit.unwrap()["immutable_body_paths"],
        json!(["model", "n", "resolution", "quality", "image", "images"])
    );
    assert_eq!(
        refusal(STANDARD, &json!({"prompt": "p", "resolution": blob("b0", "2k")})),
        "'resolution' is longer than the gateway accepts"
    );
}

/// §4: each entry's `b64_json` is an `inline` JPEG artifact, else its `url` a `url` artifact; the
/// render puts each marker where the upstream answered and keeps every other field; a host
/// placeholder never reaches the template.
#[test]
fn the_render_is_the_upstream_body_with_markers() {
    let outcome = answer(
        200,
        &json!({
            "created": 5,
            "data": [
                {"b64_json": blob("b0", "/9j/"), "revised_prompt": "one"},
                {"url": "https://imgen.x.ai/b.jpeg", "revised_prompt": blob("b1", "two")},
                {"b64_json": blob("b2", "/9j/"), "url": "https://imgen.x.ai/c.jpeg"},
            ],
            "usage": {"cost_in_usd_ticks": 600_000_000},
        }),
    );
    let ImageOutcomeV1::Succeeded { artifacts, metering, .. } = &outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        serde_json::to_value(artifacts).unwrap(),
        json!([
            {"form": "inline", "pointer": "/data/0/b64_json", "encoding": "base64", "media_type": "image/jpeg"},
            {"form": "url", "pointer": "/data/1/url"},
            {"form": "inline", "pointer": "/data/2/b64_json", "encoding": "base64", "media_type": "image/jpeg"},
        ])
    );
    assert_eq!(metering.images_reported, None);
    let rendered = XaiImageReferenceV1
        .render(&json!({}), &[outcome], &ImageRenderContextV1 { created: 9 })
        .unwrap();
    let marker =
        |index: usize, delivery: &str| json!({"$south.artifact": {"index": index, "as": delivery}});
    assert_eq!(
        serde_json::from_str::<Value>(&rendered.template).unwrap(),
        json!({
            "created": 5,
            "data": [
                {"b64_json": marker(0, "b64_json"), "revised_prompt": "one"},
                {"url": marker(1, "url"), "revised_prompt": null},
                {"b64_json": marker(2, "b64_json"), "url": "https://imgen.x.ai/c.jpeg"},
            ],
            "usage": {"cost_in_usd_ticks": 600_000_000},
        })
    );
}

/// Where the record is silent: a skeleton above its budget drops the rewritten prompts first, then
/// everything but `created`, `usage` and the images, so the outcome frame stays within 64 KiB.
#[test]
fn an_oversized_skeleton_keeps_the_images() {
    let long = "x".repeat(20_000);
    let entries: Vec<Value> = (0..3)
        .map(|i| json!({"url": format!("https://imgen.x.ai/{i}.jpeg"), "revised_prompt": long}))
        .collect();
    let ImageOutcomeV1::Succeeded { extras, .. } = answer(200, &json!({"data": entries})) else {
        panic!()
    };
    assert_eq!(
        extras["body"]["data"],
        json!([
            {"url": null, "revised_prompt": null},
            {"url": null, "revised_prompt": null},
            {"url": null, "revised_prompt": null},
        ])
    );
    let entries: Vec<Value> =
        (0..3).map(|i| json!({"url": format!("https://imgen.x.ai/{i}.jpeg"), "x": long})).collect();
    let ImageOutcomeV1::Succeeded { extras, .. } =
        answer(200, &json!({"created": 1, "data": entries, "usage": {"cost_in_usd_ticks": 1}}))
    else {
        panic!()
    };
    assert_eq!(
        extras["body"],
        json!({"created": 1, "data": [{}, {}, {}], "usage": {"cost_in_usd_ticks": 1}})
    );
}

/// §4: a 2xx without a JSON object, without images or with an entry that carries none is
/// `unknown`; a non-2xx carries xAI's `error` (or `error.message`, `message`), the code follows the
/// status, and a 4xx other than 408 is `rejected`.
#[test]
fn failures_follow_the_status_table() {
    for body in [
        json!([1]),
        json!({}),
        json!({"data": []}),
        json!({"data": {"url": "https://imgen.x.ai/a.jpeg"}}),
        json!({"data": [{"url": "https://imgen.x.ai/a.jpeg"}, {"revised_prompt": "p"}]}),
        json!({"data": [{"url": ""}]}),
        json!({"data": [{"b64_json": ""}]}),
        json!({"data": ["https://imgen.x.ai/a.jpeg"]}),
    ] {
        let outcome = answer(200, &body);
        assert!(matches!(outcome, ImageOutcomeV1::Unknown { error: None, .. }), "{body}");
    }
    for (status, body, code, message) in [
        (
            400,
            json!({"code": "c", "error": "bad size"}),
            ErrorCode::InvalidRequest,
            Some("bad size"),
        ),
        (400, json!({"error": {"message": "too big"}}), ErrorCode::InvalidRequest, Some("too big")),
        (401, json!({"message": "nope"}), ErrorCode::Auth, Some("nope")),
        (403, json!({"error": "no credits"}), ErrorCode::Auth, Some("no credits")),
        (429, json!({}), ErrorCode::RateLimit, None),
    ] {
        let outcome = answer(status, &body);
        let ImageOutcomeV1::Rejected { error } = outcome else { panic!("{status}: {outcome:?}") };
        assert_eq!((error.code, error.http_status), (code, status));
        assert_eq!(error.provider_message.as_deref(), message, "{status}");
    }
    for status in [408, 500, 503] {
        let outcome = answer(status, &json!({"error": "x"}));
        assert!(matches!(outcome, ImageOutcomeV1::Unknown { error: Some(_), .. }), "{status}");
    }
}

mod mutations {
    //! Each key rule fails the suite on a component that breaks it, so a passing report means the
    //! rule was checked, not skipped.

    use super::{manifest, pack};
    use serde_json::Value;
    use south_component_conformance::reference_xai_image::XaiImageReferenceV1;
    use south_component_conformance::{
        CheckV1, ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1,
        run_image_component_suite_v1,
    };
    use south_contracts::image::{
        ImageCallContextV1, ImageModelCapabilitiesV1, ImageRenderContextV1, PreparedImageCallV1,
        UsdAmountV1,
    };
    use south_provider_api::ComponentMetadataV1;
    use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

    #[derive(Clone, Copy)]
    enum Break {
        /// The tier evidence read in the wrong unit (1 USD = 10^9 ticks).
        TicksInTheWrongUnit,
        /// The upstream quote dropped: the host could never recognise the served tier.
        NoQuote,
        /// The edit surface's default candidate is `low` instead of `medium`.
        EditDefaultIsLow,
        /// One input image too many reported (the per-role count the host checks and bills).
        InputCountOffByOne,
        /// A retriable code on a credential refusal.
        RetriableAuth,
        /// A host placeholder left in `extras`.
        BlobInExtras,
        /// An `internal` pre-dispatch refusal.
        InternalRefusal,
    }

    struct Broken(Break);

    impl ImageComponentV1 for Broken {
        fn metadata(&self) -> ComponentMetadataV1 {
            XaiImageReferenceV1.metadata()
        }
        fn model_capabilities(
            &self,
            config: &ProviderConfig,
        ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
            XaiImageReferenceV1.model_capabilities(config)
        }
        fn prepare(
            &self,
            config: &ProviderConfig,
            request: &Value,
            context: &ImageCallContextV1,
        ) -> ComponentResultV1<PreparedImageCallV1> {
            let mut prepared = XaiImageReferenceV1.prepare(config, request, context).map_err(
                |error| match self.0 {
                    Break::InternalRefusal => {
                        ErrorEnvelope::new(ErrorCode::Internal, 500, "refused")
                    }
                    _ => error,
                },
            )?;
            match self.0 {
                Break::EditDefaultIsLow => {
                    if let Some(candidates) = &mut prepared.facts.tier_candidates {
                        candidates.default = candidates.candidates[0].clone();
                    }
                }
                Break::InputCountOffByOne if prepared.facts.inputs.input_image > 0 => {
                    prepared.facts.inputs.input_image += 1;
                }
                _ => {}
            }
            Ok(prepared)
        }
        fn parse_response(
            &self,
            state: &Value,
            response: &Value,
        ) -> ComponentResultV1<ImageOutcomeV1> {
            let outcome = XaiImageReferenceV1.parse_response(state, response)?;
            Ok(match (self.0, outcome) {
                (
                    Break::TicksInTheWrongUnit,
                    ImageOutcomeV1::Succeeded { artifacts, mut metering, extras },
                ) => {
                    if let Some(cost) = &mut metering.upstream_cost {
                        // Ten times the dollars: the quote of 10^9 ticks per dollar.
                        let ticks = response
                            .pointer("/body/json/usage/cost_in_usd_ticks")
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        cost.amount = UsdAmountV1::parse(&format!(
                            "{}.{:09}",
                            ticks / 1_000_000_000,
                            ticks % 1_000_000_000
                        ))
                        .unwrap();
                    }
                    ImageOutcomeV1::Succeeded { artifacts, metering, extras }
                }
                (Break::NoQuote, ImageOutcomeV1::Succeeded { artifacts, mut metering, extras }) => {
                    metering.upstream_cost = None;
                    ImageOutcomeV1::Succeeded { artifacts, metering, extras }
                }
                (Break::RetriableAuth, ImageOutcomeV1::Rejected { mut error })
                    if error.code == ErrorCode::Auth =>
                {
                    error.code = ErrorCode::UpstreamUnavailable;
                    ImageOutcomeV1::Rejected { error }
                }
                (Break::BlobInExtras, ImageOutcomeV1::Succeeded { artifacts, metering, .. }) => {
                    ImageOutcomeV1::Succeeded {
                        artifacts,
                        metering,
                        extras: serde_json::json!({"$south.blob": {"id": "b0", "bytes": 1, "head": "x"}}),
                    }
                }
                (_, outcome) => outcome,
            })
        }
        fn render(
            &self,
            state: &Value,
            outcomes: &[ImageOutcomeV1],
            context: &ImageRenderContextV1,
        ) -> ComponentResultV1<ImageRenderedV1> {
            XaiImageReferenceV1.render(state, outcomes, context)
        }
    }

    #[test]
    fn each_rule_bites() {
        for (broken, check) in [
            (Break::TicksInTheWrongUnit, CheckV1::FixtureMatch),
            (Break::NoQuote, CheckV1::FixtureMatch),
            (Break::EditDefaultIsLow, CheckV1::FixtureMatch),
            (Break::InputCountOffByOne, CheckV1::FixtureMatch),
            (Break::RetriableAuth, CheckV1::AuthErrorsAreNotRetriable),
            (Break::BlobInExtras, CheckV1::ReferenceIntegrity),
            (Break::InternalRefusal, CheckV1::PreDispatchRefusal),
        ] {
            let report = run_image_component_suite_v1(&Broken(broken), &pack(), &manifest());
            let failing: Vec<CheckV1> = report.failures().map(|outcome| outcome.check).collect();
            assert!(failing.contains(&check), "{check:?} did not fail: {failing:?}");
        }
    }
}
