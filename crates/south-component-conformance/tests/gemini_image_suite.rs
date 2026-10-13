//! `south.image-component.v1` against the `image-gemini` native reference and its shipped fixture
//! pack, and the behaviours of design record `2026-10-13-image-i3-2-gemini.md` §2 to §4 pinned by
//! name.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::image_fixture::ImageFixturePackV1;
use south_component_conformance::reference_gemini_image::GeminiImageReferenceV1;
use south_component_conformance::{ImageComponentV1, ImageOutcomeV1, run_image_component_suite_v1};
use south_contracts::image::{ImageCallContextV1, ImageRenderContextV1};
use south_contracts::media::{
    BlobIdV1, BlobSetV1, BlobV1, MediaBodyV1, MediaLimitsV1, MediaRequestDescriptorV1,
    expand_json_template_v1,
};
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("../../components/image-gemini/manifest.json"))
            .unwrap(),
    )
    .unwrap()
}

fn pack() -> ImageFixturePackV1 {
    ImageFixturePackV1::load(&root().join("fixtures-image-gemini")).unwrap()
}

#[test]
fn the_shipped_manifest_passes_gate_one() {
    assert_eq!(manifest().validate(), Ok(()));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let report = run_image_component_suite_v1(&GeminiImageReferenceV1, &pack(), &manifest());
    let failures: Vec<String> = report
        .failures()
        .map(|outcome| format!("{} {}: {}", outcome.check.as_str(), outcome.case, outcome.detail()))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "gemini-image",
        "base_url": "https://generativelanguage.googleapis.com",
        "auth": "provider_api_key",
    }))
    .unwrap()
}

fn context(forms: &[&str]) -> ImageCallContextV1 {
    serde_json::from_value(
        json!({"upstream_model": "gemini-3.1-flash-image", "metering_required": forms}),
    )
    .unwrap()
}

/// The prepared call as JSON, or the refusal.
fn prepare(request: &Value, forms: &[&str]) -> Result<Value, Value> {
    GeminiImageReferenceV1
        .prepare(&config(), request, &context(forms))
        .map(|prepared| serde_json::from_str(&prepared.to_json()).unwrap())
        .map_err(|error| serde_json::to_value(error).unwrap())
}

fn generate(body: &Value, forms: &[&str]) -> Result<Value, Value> {
    prepare(&json!({ "json": body }), forms)
}

fn edit(parts: &Value) -> Result<Value, Value> {
    prepare(&json!({ "multipart": { "parts": parts } }), &["tokens"])
}

fn template(prepared: &Value) -> &Value {
    &prepared["descriptor"]["body"]["json"]["template"]
}

fn state(forms: &[&str]) -> Value {
    json!({ "metering_required": forms })
}

fn answer(forms: &[&str], status: u16, body: &Value) -> ImageOutcomeV1 {
    let view = json!({"status": status, "headers": [], "body": {"json": body}});
    GeminiImageReferenceV1.parse_response(&state(forms), &view).unwrap()
}

fn image(data: &str) -> Value {
    json!({"inlineData": {"mimeType": "image/png", "data": {"$south.blob": {"id": data, "bytes": 10, "head": "iVBO"}}}})
}

/// GI-Q1: the reference image is sent as the base64 payload of the client's data URL, through the
/// `data_url_payload` transform, so the host writes exactly that payload into `inlineData.data`.
#[test]
fn the_reference_image_is_sent_as_its_base64_payload() {
    let url = "data:image/jpeg;base64,/9j/4AAQSkZJRg==";
    let prepared = generate(
        &json!({
            "prompt": "x",
            "image_url": {"$south.blob": {"id": "b0", "bytes": url.len(), "head": url}},
        }),
        &["tokens"],
    )
    .unwrap();
    let descriptor =
        MediaRequestDescriptorV1::parse(&prepared["descriptor"].to_string(), &MediaLimitsV1::V1)
            .unwrap();
    assert_eq!(descriptor.minimum_contract_version(), 2);
    let MediaBodyV1::Json(template) = descriptor.body() else { panic!("a JSON body") };
    let blobs = BlobSetV1::new([BlobV1::text(BlobIdV1::from_index(0), url.to_owned())]).unwrap();
    let body: Value =
        serde_json::from_str(expand_json_template_v1(template, &blobs).unwrap().as_str()).unwrap();
    assert_eq!(
        body["contents"][0]["parts"][0],
        json!({"inlineData": {"mimeType": "image/jpeg", "data": "/9j/4AAQSkZJRg=="}})
    );
    assert_eq!(body["contents"][0]["parts"][1], json!({"text": "x"}));
    assert_eq!(prepared["facts"]["inputs"]["reference_image"], 1);
}

/// GI-Q2: `n` is `repeat`: one image per call, never sent upstream.
#[test]
fn n_is_repeat_and_never_sent_upstream() {
    for n in [1, 4, 10] {
        let prepared = generate(&json!({"prompt": "x", "n": n}), &["tokens"]).unwrap();
        assert_eq!(prepared["repeat"], n, "{prepared}");
        assert_eq!(prepared["facts"]["requested_outputs"], n);
        let text = template(&prepared).to_string();
        assert!(!text.contains("candidateCount") && !text.contains("\"n\""), "{text}");
    }
    let edited = edit(&json!([
        {"kind": "text", "name": "prompt", "value": "x"},
        {"kind": "file", "name": "image", "blob": "b0", "bytes": 4},
        {"kind": "text", "name": "n", "value": "4"},
    ]))
    .unwrap();
    assert_eq!(
        (edited["repeat"].as_u64(), edited["facts"]["requested_outputs"].as_u64()),
        (Some(4), Some(4))
    );
}

/// GI-Q3: the tier word is reported only when the host prices the call per image and the client
/// sent a resolution; the upstream receives the resolution either way.
#[test]
fn the_tier_word_exists_only_on_per_image_calls_with_a_resolution() {
    for (body, forms, tier) in [
        (json!({"prompt": "x", "resolution": "2k"}), &["tokens"][..], json!({})),
        (json!({"prompt": "x", "resolution": "2k"}), &["images"][..], json!({"resolution": "2K"})),
        (
            json!({"prompt": "x", "resolution": "4K"}),
            &["tokens", "images"][..],
            json!({"resolution": "4K"}),
        ),
        (json!({"prompt": "x"}), &["images"][..], json!({})),
        (json!({"prompt": "x", "resolution": " "}), &["images"][..], json!({})),
    ] {
        let prepared = generate(&body, forms).unwrap();
        assert_eq!(prepared["facts"]["tier"], tier, "{body} {forms:?}");
        let sent = body["resolution"].as_str().map(str::trim).filter(|word| !word.is_empty());
        assert_eq!(
            template(&prepared)["generationConfig"]["imageConfig"]["imageSize"].as_str(),
            sent.map(str::to_ascii_uppercase).as_deref(),
            "{body}"
        );
    }
}

/// §2: `aspect_ratio` and `resolution` become `imageConfig`; every other field is dropped.
#[test]
fn only_the_prompt_image_and_image_config_reach_the_upstream() {
    let prepared = generate(
        &json!({
            "prompt": "x", "aspect_ratio": "16:9", "resolution": "1k", "size": "1024x1024",
            "quality": "high", "response_format": "url", "style": "vivid",
        }),
        &["tokens"],
    )
    .unwrap();
    assert_eq!(
        *template(&prepared),
        json!({
            "contents": [{"role": "user", "parts": [{"text": "x"}]}],
            "generationConfig": {
                "responseModalities": ["IMAGE"],
                "imageConfig": {"aspectRatio": "16:9", "imageSize": "1K"},
            },
        })
    );
    assert!(prepared["facts"].get("size").is_none(), "{prepared}");
    assert_eq!(
        prepared["descriptor"]["auth"],
        json!({"arm": "header_secret", "header": "x-goog-api-key", "slot": "provider_api_key"})
    );
    assert_eq!(
        prepared["descriptor"]["path"],
        "v1beta/models/gemini-3.1-flash-image:generateContent"
    );
}

/// GI-Q6: the edit labels its image with the part's own `image/*` type, else `image/png`; only the
/// first `image` part is sent, and every one is counted.
#[test]
fn the_edit_labels_the_image_with_the_parts_type_or_png() {
    for (media_type, label) in [
        (json!("image/jpeg"), "image/jpeg"),
        (json!("application/octet-stream"), "image/png"),
        (Value::Null, "image/png"),
    ] {
        let mut file = json!({"kind": "file", "name": "image", "blob": "b0", "bytes": 4});
        if !media_type.is_null() {
            file["media_type"] = media_type.clone();
        }
        let prepared = edit(&json!([
            {"kind": "text", "name": "prompt", "value": "x"},
            file,
            {"kind": "file", "name": "image", "blob": "b1", "bytes": 4, "media_type": "image/webp"},
        ]))
        .unwrap();
        assert_eq!(
            template(&prepared)["contents"][0]["parts"][0],
            json!({"inlineData": {
                "mimeType": label,
                "data": {"$south.ref": {"blob": "b0", "transform": "base64"}},
            }}),
            "{media_type}"
        );
        assert_eq!(prepared["facts"]["inputs"]["reference_image"], 2);
        assert_eq!(prepared["facts"]["operation"], "edit");
    }
}

/// §3: a mask and a streamed edit are refused before dispatch.
#[test]
fn the_edit_refuses_a_mask_and_streaming() {
    let base = || {
        vec![
            json!({"kind": "text", "name": "prompt", "value": "x"}),
            json!({"kind": "file", "name": "image", "blob": "b0", "bytes": 4}),
        ]
    };
    let mut masked = base();
    masked.push(json!({"kind": "file", "name": "mask", "blob": "b1", "bytes": 4}));
    assert!(edit(&Value::Array(masked)).unwrap_err()["message"].as_str().unwrap().contains("mask"));
    for value in ["true", "True", "1"] {
        let mut streamed = base();
        streamed.push(json!({"kind": "text", "name": "stream", "value": value}));
        assert_eq!(
            edit(&Value::Array(streamed)).unwrap_err()["code"],
            "invalid_request",
            "{value}"
        );
    }
    let mut not_streamed = base();
    not_streamed.push(json!({"kind": "text", "name": "stream", "value": "false"}));
    assert!(edit(&Value::Array(not_streamed)).is_ok());
}

/// GI-Q4: the totals are the declared buckets; a modality split is reported only when it adds up
/// to its total, and `thoughtsTokenCount` is not reported.
#[test]
fn the_modality_split_is_reported_only_when_it_adds_up() {
    let body = |prompt_details: Value| {
        json!({
            "candidates": [{"content": {"parts": [image("b0")]}}],
            "usageMetadata": {
                "promptTokenCount": 270, "candidatesTokenCount": 1290, "thoughtsTokenCount": 99,
                "promptTokensDetails": prompt_details,
                "candidatesTokensDetails": [{"modality": "IMAGE", "tokenCount": 1290}],
            },
        })
    };
    let tokens = |outcome: ImageOutcomeV1| {
        let ImageOutcomeV1::Succeeded { metering, .. } = outcome else { panic!("{outcome:?}") };
        metering.tokens.unwrap()
    };
    let closed = tokens(answer(
        &["tokens"],
        200,
        &body(
            json!([{"modality": "TEXT", "tokenCount": 12}, {"modality": "IMAGE", "tokenCount": 258}]),
        ),
    ));
    assert_eq!((closed.text_input, closed.image_input), (Some(12), Some(258)));
    assert_eq!(
        (closed.image_output, closed.total_input, closed.total_output),
        (Some(1290), Some(270), Some(1290))
    );
    let open =
        tokens(answer(&["tokens"], 200, &body(json!([{"modality": "TEXT", "tokenCount": 12}]))));
    assert_eq!((open.text_input, open.image_input), (None, None));
    assert_eq!(open.image_output, Some(1290));
    let serialized = serde_json::to_string(&closed).unwrap();
    assert!(!serialized.contains("99"), "{serialized}");
}

/// §4 and record Q16: a declared bucket missing on a 2xx is `unknown` when tokens are required;
/// a per-image call succeeds without usage and reports no image count.
#[test]
fn a_missing_declared_bucket_is_unknown_only_when_tokens_are_required() {
    let body = json!({
        "candidates": [{"content": {"parts": [image("b0")]}}],
        "usageMetadata": {"promptTokenCount": 12},
    });
    assert!(matches!(answer(&["tokens"], 200, &body), ImageOutcomeV1::Unknown { .. }));
    let ImageOutcomeV1::Succeeded { metering, .. } = answer(&["images"], 200, &body) else {
        panic!("a per-image call succeeds")
    };
    assert_eq!(metering.images_reported, None);
}

/// GI-Q5: a prompt blocked before generation is `rejected`; a candidate stopped for a policy reason
/// without an image may have been charged and is `unknown`, still labelled `content_policy`.
#[test]
fn a_blocked_prompt_is_rejected_and_a_policy_stop_is_unknown() {
    let blocked = answer(&["tokens"], 200, &json!({"promptFeedback": {"blockReason": "SAFETY"}}));
    let ImageOutcomeV1::Rejected { error } = blocked else { panic!("{blocked:?}") };
    assert_eq!(error.code, ErrorCode::ContentPolicy);
    let stopped =
        answer(&["tokens"], 200, &json!({"candidates": [{"finishReason": "IMAGE_SAFETY"}]}));
    let ImageOutcomeV1::Unknown { error: Some(error), .. } = stopped else { panic!("{stopped:?}") };
    assert_eq!(error.code, ErrorCode::ContentPolicy);
    let empty = answer(&["tokens"], 200, &json!({"candidates": [{"finishReason": "STOP"}]}));
    assert!(matches!(empty, ImageOutcomeV1::Unknown { error: None, .. }), "{empty:?}");
}

/// GI-Q7: an interim `thought` image is not delivered.
#[test]
fn thought_images_are_not_delivered() {
    let mut thought = image("b0");
    thought["thought"] = json!(true);
    let outcome = answer(
        &["images"],
        200,
        &json!({"candidates": [{"content": {"parts": [thought, image("b1")]}}]}),
    );
    let ImageOutcomeV1::Succeeded { artifacts, .. } = outcome else { panic!("{outcome:?}") };
    assert_eq!(
        serde_json::to_value(&artifacts).unwrap(),
        json!([{"form": "inline", "pointer": "/candidates/0/content/parts/1/inlineData/data", "encoding": "base64", "media_type": "image/png"}])
    );
}

/// §4: the client body lists every round's images in order, indices counted across rounds.
#[test]
fn render_counts_artifacts_across_rounds() {
    let round = |parts: Value| {
        answer(&["images"], 200, &json!({"candidates": [{"content": {"parts": parts}}]}))
    };
    let outcomes = [round(json!([image("b0")])), round(json!([image("b0"), image("b1")]))];
    let rendered = GeminiImageReferenceV1
        .render(&state(&["images"]), &outcomes, &ImageRenderContextV1 { created: 7 })
        .unwrap();
    let template: Value = serde_json::from_str(&rendered.template).unwrap();
    let indices: Vec<u64> = template["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["b64_json"]["$south.artifact"]["index"].as_u64().unwrap())
        .collect();
    assert_eq!(indices, [0, 1, 2]);
    assert_eq!(template["created"], 7);
}
