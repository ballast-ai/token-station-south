//! `south.image-component.v1` against the `image-stability` native reference and its shipped
//! fixture pack, and the behaviours of design record `2026-10-13-image-i3-4-stability.md` §2 to §4
//! pinned by name.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::image_fixture::ImageFixturePackV1;
use south_component_conformance::reference_stability_image::StabilityImageReferenceV1;
use south_component_conformance::{ImageComponentV1, ImageOutcomeV1, run_image_component_suite_v1};
use south_contracts::image::ImageCallContextV1;
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("../../components/image-stability/manifest.json"))
            .unwrap(),
    )
    .unwrap()
}

fn pack() -> ImageFixturePackV1 {
    ImageFixturePackV1::load(&root().join("fixtures-image-stability")).unwrap()
}

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "stability-image",
        "base_url": "https://api.stability.ai",
        "auth": "provider_api_key",
    }))
    .unwrap()
}

fn context(model: &str) -> ImageCallContextV1 {
    serde_json::from_value(json!({"upstream_model": model, "metering_required": ["images"]}))
        .unwrap()
}

/// A host placeholder for an elided data URL of `media` with a payload of `payload` characters.
fn source(media: &str, payload: u64) -> Value {
    let head = format!("data:{media};base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAA");
    let prefix = u64::try_from(format!("data:{media};base64,").len()).unwrap();
    json!({"$south.blob": {"id": "b0", "bytes": prefix + payload, "head": &head[..64]}})
}

/// The prepared call as JSON, or the refusal.
fn prepare(model: &str, body: &Value) -> Result<Value, Value> {
    StabilityImageReferenceV1
        .prepare(&config(), &json!({ "json": body }), &context(model))
        .map(|prepared| serde_json::from_str(&prepared.to_json()).unwrap())
        .map_err(|error| serde_json::to_value(error).unwrap())
}

/// The descriptor's parts as `(name, value-or-transform)` pairs.
fn parts(prepared: &Value) -> Vec<(String, String)> {
    prepared["descriptor"]["body"]["multipart"]["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| {
            let value = part["value"].as_str().or_else(|| part["transform"].as_str()).unwrap();
            (part["name"].as_str().unwrap().to_owned(), value.to_owned())
        })
        .collect()
}

fn named(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

fn answer(state: &Value, status: u16, body: &Value) -> ImageOutcomeV1 {
    let view = json!({"status": status, "headers": [], "body": {"json": body}});
    StabilityImageReferenceV1.parse_response(state, &view).unwrap()
}

#[test]
fn the_shipped_manifest_passes_gate_one() {
    assert_eq!(manifest().validate(), Ok(()));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let report = run_image_component_suite_v1(&StabilityImageReferenceV1, &pack(), &manifest());
    let failures: Vec<String> = report
        .failures()
        .map(|outcome| format!("{} {}: {}", outcome.check.as_str(), outcome.case, outcome.detail()))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// SI-Q1 and decision 1: the five native ids are declared, `generate` only; any other configured
/// id is not (the host then refuses it before admission, P4).
#[test]
fn only_the_five_native_ids_are_declared() {
    let mut config = config();
    config.models = serde_json::from_value(json!([
        {"model": "stable-image-ultra"}, {"model": "sd3.5-flash"}, {"model": "sd3.5-medium"},
    ]))
    .unwrap();
    let models = StabilityImageReferenceV1.model_capabilities(&config).unwrap();
    let ids: Vec<&str> = models.iter().map(|model| model.model.as_str()).collect();
    assert_eq!(ids, ["stable-image-ultra", "sd3.5-medium"]);
    assert!(models.iter().all(|model| model.operations.len() == 1 && !model.repeat));
}

/// §2: each id posts to its native endpoint; only SD3 carries `model` and `mode`, and the request
/// asks for JSON.
#[test]
fn each_id_posts_to_its_endpoint_with_its_parts() {
    for (model, path, tail) in [
        ("stable-image-ultra", "v2beta/stable-image/generate/ultra", vec![]),
        ("stable-image-core", "v2beta/stable-image/generate/core", vec![]),
        (
            "sd3.5-large",
            "v2beta/stable-image/generate/sd3",
            vec![("model", "sd3.5-large"), ("mode", "text-to-image")],
        ),
        (
            "sd3.5-large-turbo",
            "v2beta/stable-image/generate/sd3",
            vec![("model", "sd3.5-large-turbo"), ("mode", "text-to-image")],
        ),
        (
            "sd3.5-medium",
            "v2beta/stable-image/generate/sd3",
            vec![("model", "sd3.5-medium"), ("mode", "text-to-image")],
        ),
    ] {
        let prepared = prepare(model, &json!({"prompt": "x"})).unwrap();
        assert_eq!(prepared["descriptor"]["path"], path, "{model}");
        assert_eq!(
            prepared["descriptor"]["headers"],
            json!([{"name": "accept", "value": "application/json"}]),
            "{model}"
        );
        let mut expected = named(&[("prompt", "x"), ("output_format", "png")]);
        expected.extend(named(&tail));
        assert_eq!(parts(&prepared), expected, "{model}");
    }
}

/// §3: the source image is the last part, `image`, read by the host through `from_data_url`, with
/// the native file name and normalised media type; it counts as one `input_image`.
#[test]
fn the_source_image_is_the_last_part_through_from_data_url() {
    for (media, filename, normalised) in [
        ("image/png", "source.png", "image/png"),
        ("image/jpg", "source.jpg", "image/jpeg"),
        ("image/jpeg", "source.jpg", "image/jpeg"),
        ("image/webp", "source.webp", "image/webp"),
    ] {
        for body in [
            json!({"prompt": "x", "image_url": source(media, 4000), "strength": 0.5}),
            json!({"prompt": "x", "image": {"url": source(media, 4000)}, "strength": 0.5}),
        ] {
            let prepared = prepare("stable-image-ultra", &body).unwrap();
            let all = prepared["descriptor"]["body"]["multipart"]["parts"].as_array().unwrap();
            assert_eq!(
                all.last().unwrap(),
                &json!({"name": "image", "blob": "b0", "transform": "from_data_url",
                        "filename": filename, "media_type": normalised}),
                "{media}"
            );
            assert_eq!(prepared["facts"]["inputs"]["input_image"], 1);
        }
    }
}

/// Decision 2: the 10 MiB limit is judged from the payload length, the largest canonical payload
/// that can decode to 10 MiB passing; an empty payload is refused.
#[test]
fn the_size_limit_is_judged_from_the_payload_length() {
    let body = |payload| json!({"prompt": "x", "image_url": source("image/png", payload), "strength": 0.5});
    assert!(prepare("stable-image-ultra", &body(13_981_016)).is_ok());
    let over = prepare("stable-image-ultra", &body(13_981_017)).unwrap_err();
    assert!(over["message"].as_str().unwrap().contains("after base64 decode"), "{over}");
    let empty = prepare("stable-image-ultra", &body(0)).unwrap_err();
    assert_eq!(empty["message"], "Stability AI `image_url` data URL payload is empty");
}

/// §3: `aspect_ratio` wins over `size`; a `WxH` size becomes the nearest allowed ratio; nothing is
/// derived on image-to-image, where SD3 refuses either field and Ultra ignores them.
#[test]
fn the_aspect_ratio_is_the_clients_or_the_nearest_to_size() {
    let ratio = |body: Value| {
        parts(&prepare("stable-image-core", &body).unwrap())
            .into_iter()
            .find(|(name, _)| name == "aspect_ratio")
            .map(|(_, value)| value)
    };
    assert_eq!(ratio(json!({"prompt": "x", "size": "1536x1024"})).as_deref(), Some("3:2"));
    assert_eq!(ratio(json!({"prompt": "x", "size": "1792x1024"})).as_deref(), Some("16:9"));
    assert_eq!(ratio(json!({"prompt": "x", "size": "1024X1792"})).as_deref(), Some("9:16"));
    assert_eq!(ratio(json!({"prompt": "x", "size": "auto"})), None);
    assert_eq!(
        ratio(json!({"prompt": "x", "aspect_ratio": "9:21", "size": "1024x1024"})).as_deref(),
        Some("9:21")
    );
    let ultra = prepare(
        "stable-image-ultra",
        &json!({"prompt": "x", "image_url": source("image/png", 8), "strength": 0.5, "size": "wide"}),
    )
    .unwrap();
    assert!(parts(&ultra).iter().all(|(name, _)| name != "aspect_ratio"));
    assert!(
        prepare(
            "sd3.5-large",
            &json!({"prompt": "x", "image_url": source("image/png", 8), "strength": 0.5, "size": null}),
        )
        .is_err()
    );
}

/// Decision 3: a number is sent as `serde_json` renders the parsed value, not as the client wrote it.
#[test]
fn numbers_are_sent_as_the_parsed_value_renders() {
    let body: Value = serde_json::from_str(
        r#"{"prompt": "x", "image_url": {"$south.blob": {"id": "b0", "bytes": 30, "head": "data:image/png;base64,iVBORw0K"}}, "strength": 0.50, "cfg_scale": 2, "seed": 7.0}"#,
    )
    .unwrap();
    let prepared = prepare("sd3.5-medium", &body).unwrap();
    let got = parts(&prepared);
    assert!(got.contains(&("strength".to_owned(), "0.5".to_owned())), "{got:?}");
    assert!(got.contains(&("cfg_scale".to_owned(), "2".to_owned())), "{got:?}");
    assert!(got.contains(&("seed".to_owned(), "7".to_owned())), "{got:?}");
}

/// The native `mode` table, including the misleading text the record registers as host defect 1.
#[test]
fn the_native_mode_table() {
    let image = source("image/png", 8);
    for (model, mode, with_image, accepted) in [
        ("sd3.5-large", "text-to-image", false, true),
        ("sd3.5-large", "image-to-image", true, true),
        ("sd3.5-large", "image-to-image", false, false),
        ("sd3.5-large", "text-to-image", true, false),
        ("stable-image-ultra", "text-to-image", false, true),
        ("stable-image-ultra", "image-to-image", true, true),
        ("stable-image-ultra", "text-to-image", true, false),
        ("stable-image-ultra", "image-to-image", false, false),
        ("stable-image-core", "text-to-image", false, true),
        ("stable-image-core", "image-to-image", false, false),
    ] {
        let mut body = json!({"prompt": "x", "mode": mode});
        if with_image {
            body["image_url"] = image.clone();
            body["strength"] = json!(0.5);
        }
        assert_eq!(prepare(model, &body).is_ok(), accepted, "{model} {mode} {with_image}");
    }
    let ultra = prepare(
        "stable-image-ultra",
        &json!({"prompt": "x", "mode": "image-to-image", "image_url": image, "strength": 0.5}),
    )
    .unwrap();
    assert!(parts(&ultra).iter().all(|(name, _)| name != "mode"), "Ultra's hint is not forwarded");
}

/// §3: the immutable part names are the native `STABILITY_IMAGE_OWNED`, so operator request extras
/// can neither set the model, the aspect ratio nor the output format.
#[test]
fn the_immutable_parts_are_the_native_owned_fields() {
    let prepared = prepare("sd3.5-large", &json!({"prompt": "x"})).unwrap();
    assert_eq!(prepared["immutable_body_paths"], json!(["model", "aspect_ratio", "output_format"]));
    assert!(prepared["facts"].get("size").is_none(), "no size fact (P5)");
}

/// SI-Q3 and P8: `CONTENT_FILTERED` succeeds like `SUCCESS`, with the artifact typed from the
/// `output_format` sent; Stability reports no image count.
#[test]
fn a_content_filtered_image_is_delivered_with_the_sent_type() {
    for (format, media) in [("png", "image/png"), ("jpeg", "image/jpeg"), ("webp", "image/webp")] {
        let prepared =
            prepare("stable-image-core", &json!({"prompt": "x", "output_format": format})).unwrap();
        let state = &prepared["state"];
        for reason in ["SUCCESS", "CONTENT_FILTERED"] {
            let outcome = answer(state, 200, &json!({"image": "AAAA", "finish_reason": reason}));
            let ImageOutcomeV1::Succeeded { artifacts, metering, .. } = outcome else {
                panic!("{reason}: {outcome:?}")
            };
            assert_eq!(
                serde_json::to_value(&artifacts).unwrap(),
                json!([{"form": "inline", "pointer": "/image", "encoding": "base64", "media_type": media}])
            );
            assert_eq!(metering.images_reported, None);
        }
    }
    for body in [
        json!({"image": "AAAA", "finish_reason": "ERROR"}),
        json!({"image": "AAAA"}),
        json!({"image": "", "finish_reason": "SUCCESS"}),
        json!({"finish_reason": "SUCCESS"}),
    ] {
        let outcome = answer(&json!({}), 200, &body);
        assert!(matches!(outcome, ImageOutcomeV1::Unknown { error: None, .. }), "{body}");
    }
}

/// §4: a non-2xx carries Stability's `name` and `errors`; `content_moderation` is content policy;
/// a 4xx other than 408 is `rejected`, anything else `unknown`.
#[test]
fn a_non_2xx_names_stabilitys_error() {
    let body = |name: &str| json!({"id": "x", "name": name, "errors": ["one", "two"]});
    let outcome = answer(&json!({}), 403, &body("content_moderation"));
    let ImageOutcomeV1::Rejected { error } = outcome else { panic!("{outcome:?}") };
    assert_eq!((error.code, error.http_status), (ErrorCode::ContentPolicy, 403));
    assert_eq!(error.provider_message.as_deref(), Some("content_moderation: one; two"));
    let outcome = answer(&json!({}), 401, &body("unauthorized"));
    assert!(
        matches!(&outcome, ImageOutcomeV1::Rejected { error } if error.code == ErrorCode::Auth),
        "{outcome:?}"
    );
    for status in [408, 500, 503] {
        let outcome = answer(&json!({}), status, &body("internal_error"));
        assert!(matches!(outcome, ImageOutcomeV1::Unknown { error: Some(_), .. }), "{status}");
    }
}

mod mutations {
    //! Each required row fails on a component that breaks it, so a passing report means the row
    //! was checked, not skipped.

    use super::{manifest, pack};
    use serde_json::{Value, json};
    use south_component_conformance::reference_stability_image::StabilityImageReferenceV1;
    use south_component_conformance::{
        CheckV1, ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1,
        run_image_component_suite_v1,
    };
    use south_contracts::image::{
        ImageCallContextV1, ImageModelCapabilitiesV1, ImageRenderContextV1, PreparedImageCallV1,
    };
    use south_provider_api::ComponentMetadataV1;
    use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

    #[derive(Clone, Copy)]
    enum Break {
        RetriableAuth,
        BlobInExtras,
        UrlDeliveryOfInline,
        InternalRefusal,
        FilteredIsUnknown,
    }

    struct Broken(Break);

    impl ImageComponentV1 for Broken {
        fn metadata(&self) -> ComponentMetadataV1 {
            StabilityImageReferenceV1.metadata()
        }
        fn model_capabilities(
            &self,
            config: &ProviderConfig,
        ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
            StabilityImageReferenceV1.model_capabilities(config)
        }
        fn prepare(
            &self,
            config: &ProviderConfig,
            request: &Value,
            context: &ImageCallContextV1,
        ) -> ComponentResultV1<PreparedImageCallV1> {
            StabilityImageReferenceV1.prepare(config, request, context).map_err(
                |error| match self.0 {
                    Break::InternalRefusal => {
                        ErrorEnvelope::new(ErrorCode::Internal, 500, "refused")
                    }
                    _ => error,
                },
            )
        }
        fn parse_response(
            &self,
            state: &Value,
            response: &Value,
        ) -> ComponentResultV1<ImageOutcomeV1> {
            let filtered =
                response.pointer("/body/json/finish_reason") == Some(&json!("CONTENT_FILTERED"));
            let outcome = StabilityImageReferenceV1.parse_response(state, response)?;
            Ok(match (self.0, outcome) {
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
                        extras: json!({"$south.blob": {"id": "b0", "bytes": 1, "head": "x"}}),
                    }
                }
                (Break::FilteredIsUnknown, ImageOutcomeV1::Succeeded { .. }) if filtered => {
                    ImageOutcomeV1::Unknown { reason: "filtered".to_owned(), error: None }
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
            let rendered = StabilityImageReferenceV1.render(state, outcomes, context)?;
            Ok(match self.0 {
                Break::UrlDeliveryOfInline => ImageRenderedV1 {
                    template: rendered.template.replace("\"as\":\"b64_json\"", "\"as\":\"url\""),
                },
                _ => rendered,
            })
        }
    }

    #[test]
    fn each_row_bites() {
        for (broken, check) in [
            (Break::RetriableAuth, CheckV1::AuthErrorsAreNotRetriable),
            (Break::BlobInExtras, CheckV1::ReferenceIntegrity),
            (Break::UrlDeliveryOfInline, CheckV1::ReferenceIntegrity),
            (Break::InternalRefusal, CheckV1::PreDispatchRefusal),
            (Break::FilteredIsUnknown, CheckV1::FixtureMatch),
        ] {
            let report = run_image_component_suite_v1(&Broken(broken), &pack(), &manifest());
            let failing: Vec<CheckV1> = report.failures().map(|outcome| outcome.check).collect();
            assert!(failing.contains(&check), "{check:?} did not fail: {failing:?}");
        }
    }
}
