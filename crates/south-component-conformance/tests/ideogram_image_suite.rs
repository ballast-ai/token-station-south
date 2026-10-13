//! `south.image-component.v1` against the `image-ideogram` native reference and its shipped
//! fixture pack, and the behaviours of design record `2026-10-13-image-i3-5-ideogram.md` §2 to §4
//! pinned by name.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::image_fixture::ImageFixturePackV1;
use south_component_conformance::reference_ideogram_image::IdeogramImageReferenceV1;
use south_component_conformance::{ImageComponentV1, ImageOutcomeV1, run_image_component_suite_v1};
use south_contracts::image::{ImageCallContextV1, ImageRenderContextV1};
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

/// The 38 members of the native `IDEOGRAM_V4_RESOLUTIONS`, in its order.
const MEMBERS: [&str; 38] = [
    "2048x2048",
    "1440x2880",
    "2880x1440",
    "1664x2496",
    "2496x1664",
    "1792x2240",
    "2240x1792",
    "1440x2560",
    "2560x1440",
    "1600x2560",
    "2560x1600",
    "1728x2304",
    "2304x1728",
    "1296x3168",
    "3168x1296",
    "1152x2944",
    "2944x1152",
    "1248x3328",
    "3328x1248",
    "1280x3072",
    "3072x1280",
    "1024x3072",
    "3072x1024",
    "1024x1024",
    "896x1120",
    "1120x896",
    "864x1152",
    "1152x864",
    "832x1248",
    "1248x832",
    "800x1280",
    "1280x800",
    "720x1280",
    "1280x720",
    "720x1440",
    "1440x720",
    "512x1536",
    "1536x512",
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("../../components/image-ideogram/manifest.json"))
            .unwrap(),
    )
    .unwrap()
}

fn pack() -> ImageFixturePackV1 {
    ImageFixturePackV1::load(&root().join("fixtures-image-ideogram")).unwrap()
}

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "ideogram-image",
        "base_url": "https://api.ideogram.ai",
        "auth": "provider_api_key",
    }))
    .unwrap()
}

fn context(model: &str) -> ImageCallContextV1 {
    serde_json::from_value(json!({"upstream_model": model, "metering_required": ["images"]}))
        .unwrap()
}

/// The prepared call as JSON, or the refusal.
fn prepare(model: &str, body: &Value) -> Result<Value, Value> {
    IdeogramImageReferenceV1
        .prepare(&config(), &json!({ "json": body }), &context(model))
        .map(|prepared| serde_json::from_str(&prepared.to_json()).unwrap())
        .map_err(|error| serde_json::to_value(error).unwrap())
}

/// The descriptor's parts as `(name, value)` pairs.
fn parts(prepared: &Value) -> Vec<(String, String)> {
    prepared["descriptor"]["body"]["multipart"]["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| {
            (part["name"].as_str().unwrap().to_owned(), part["value"].as_str().unwrap().to_owned())
        })
        .collect()
}

/// The `resolution` part a `size` gives, or the refusal's message.
fn resolution(size: &Value) -> Result<Option<String>, String> {
    prepare("ideogram-v4", &json!({"prompt": "x", "size": size}))
        .map(|prepared| {
            let resolution = parts(&prepared)
                .into_iter()
                .find(|(name, _)| name == "resolution")
                .map(|(_, value)| value);
            // IG-Q3: the size fact is the resolution sent, and nothing without one.
            match &resolution {
                Some(member) => {
                    let (width, height) = member.split_once('x').unwrap();
                    assert_eq!(
                        prepared["facts"]["size"],
                        json!({"width": width.parse::<u32>().unwrap(), "height": height.parse::<u32>().unwrap()}),
                        "{size}"
                    );
                }
                None => assert!(prepared["facts"].get("size").is_none(), "{size}"),
            }
            resolution
        })
        .map_err(|error| error["message"].as_str().unwrap().to_owned())
}

fn answer(state: &Value, status: u16, body: &Value) -> ImageOutcomeV1 {
    let view = json!({"status": status, "headers": [], "body": {"json": body}});
    IdeogramImageReferenceV1.parse_response(state, &view).unwrap()
}

#[test]
fn the_shipped_manifest_passes_gate_one() {
    assert_eq!(manifest().validate(), Ok(()));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let report = run_image_component_suite_v1(&IdeogramImageReferenceV1, &pack(), &manifest());
    let failures: Vec<String> = report
        .failures()
        .map(|outcome| format!("{} {}: {}", outcome.check.as_str(), outcome.case, outcome.detail()))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// IG-Q1 and decision 1: the three v4 ids are declared, `generate` only, `url` and `b64_json`,
/// no input role; any other configured id (FLASH, v3) is not.
#[test]
fn only_the_three_v4_ids_are_declared() {
    let mut config = config();
    config.models = serde_json::from_value(json!([
        {"model": "ideogram-v4-flash"}, {"model": "ideogram-v4-quality"}, {"model": "ideogram-v3"},
        {"model": "ideogram-v4"}, {"model": "ideogram-v4-turbo"},
    ]))
    .unwrap();
    let models =
        serde_json::to_value(IdeogramImageReferenceV1.model_capabilities(&config).unwrap())
            .unwrap();
    let ids: Vec<&str> =
        models.as_array().unwrap().iter().map(|m| m["model"].as_str().unwrap()).collect();
    assert_eq!(ids, ["ideogram-v4-quality", "ideogram-v4", "ideogram-v4-turbo"]);
    for model in models.as_array().unwrap() {
        assert_eq!(model["operations"], json!(["generate"]));
        assert_eq!(model["input_roles"], json!([]));
        assert_eq!(model["response_formats"], json!(["url", "b64_json"]));
        assert_eq!(model["metering_forms"], json!(["images"]));
        assert_eq!(model["repeat"], json!(false));
        assert_eq!(model["request_elision_paths"], json!([]));
    }
}

/// §2 and §3: each id posts the same path with its pinned `rendering_speed`, authenticated by the
/// `api-key` header (never Bearer), with no other header; the prompt is sent as the client wrote it.
#[test]
fn each_id_pins_its_rendering_speed() {
    for (model, speed) in [
        ("ideogram-v4-turbo", "TURBO"),
        ("ideogram-v4", "DEFAULT"),
        ("ideogram-v4-quality", "QUALITY"),
    ] {
        let prepared = prepare(model, &json!({"prompt": " a red fox "})).unwrap();
        let descriptor = &prepared["descriptor"];
        assert_eq!(descriptor["method"], "POST", "{model}");
        assert_eq!(descriptor["path"], "v1/ideogram-v4/generate", "{model}");
        assert_eq!(
            descriptor["auth"],
            json!({"arm": "header_secret", "header": "api-key", "slot": "provider_api_key"}),
            "{model}"
        );
        assert!(descriptor.get("headers").is_none(), "{model}: no accept, no content-type");
        assert_eq!(
            parts(&prepared),
            [
                ("text_prompt".to_owned(), " a red fox ".to_owned()),
                ("rendering_speed".to_owned(), speed.to_owned())
            ],
            "{model}"
        );
    }
    let refused = prepare("ideogram-v4-flash", &json!({"prompt": "x"})).unwrap_err();
    assert_eq!(refused["code"], "invalid_request");
}

/// §3: the native `ideogram_map_size` table (the host's unit test
/// `ideogram_map_size_aspect_matching`, plus the record's extra rows).
#[test]
fn the_size_table_is_the_native_one() {
    for (size, expected) in [
        (json!(null), None),
        (json!(""), None),
        (json!("auto"), None),
        (json!("AUTO"), None),
        (json!("2560x1440"), Some("2560x1440")),
        (json!(" 2560x1440 "), Some("2560x1440")),
        (json!("1024x1024"), Some("1024x1024")),
        (json!("1536x512"), Some("1536x512")),
        (json!("1000x1000"), Some("2048x2048")),
        (json!("1792x1024"), Some("2560x1440")),
        (json!("1024x1792"), Some("1440x2560")),
        (json!("1536x1024"), Some("2496x1664")),
        (json!("+5x5"), Some("2048x2048")),
        (json!("1600 x 900"), Some("2560x1440")),
    ] {
        assert_eq!(resolution(&size).unwrap().as_deref(), expected, "{size}");
    }
    for bad in ["1024X1024", "1024", "axb", "0x1024", "1024x0", "-5x100", "5x5x5"] {
        let message = resolution(&json!(bad)).unwrap_err();
        assert_eq!(
            message,
            format!(
                "'size' value '{bad}' is not supported for Ideogram models; pass 'auto', a WxH pair \
                 like '1024x1024', or a native Ideogram v4 resolution (e.g. '2048x2048')"
            )
        );
    }
    assert_eq!(resolution(&json!(1024)).unwrap_err(), "'size' must be a string (got 1024)");
}

/// Every member passes through as itself, and a non-member of the same shape snaps to the first
/// member of its ratio.
#[test]
fn every_member_passes_through() {
    for member in MEMBERS {
        assert_eq!(resolution(&json!(member)).unwrap().as_deref(), Some(member));
        let (width, height) = member.split_once('x').unwrap();
        let (width, height): (u32, u32) = (width.parse().unwrap(), height.parse().unwrap());
        let scaled = format!("{}x{}", width * 3, height * 3);
        let first_of_ratio = MEMBERS
            .iter()
            .find(|other| {
                let (w, h) = other.split_once('x').unwrap();
                u64::from(w.parse::<u32>().unwrap()) * u64::from(height)
                    == u64::from(h.parse::<u32>().unwrap()) * u64::from(width)
            })
            .unwrap();
        assert_eq!(
            resolution(&json!(scaled)).unwrap().as_deref(),
            Some(*first_of_ratio),
            "{scaled}"
        );
    }
}

/// §3: the delivery the client asked for is carried in `state` and decides the render; every
/// native acceptance and refusal of `response_format`.
#[test]
fn response_format_reaches_the_render() {
    for (format, delivery) in [
        (json!(null), "url"),
        (json!("url"), "url"),
        (json!("b64_json"), "b64_json"),
        (json!({"type": "url"}), "url"),
        (json!({"type": "b64_json"}), "b64_json"),
        (json!({}), "url"),
        (json!({"type": 5}), "url"),
    ] {
        let prepared =
            prepare("ideogram-v4", &json!({"prompt": "x", "response_format": format})).unwrap();
        let state = &prepared["state"];
        assert_eq!(state, &json!({"format": delivery}), "{format}");
        let outcome = answer(
            state,
            200,
            &json!({"data": [{"url": "https://ideogram.test/a.png", "prompt": "p", "is_image_safe": true}]}),
        );
        let rendered = IdeogramImageReferenceV1
            .render(state, &[outcome], &ImageRenderContextV1 { created: 7 })
            .unwrap();
        let template: Value = serde_json::from_str(&rendered.template).unwrap();
        assert_eq!(
            template,
            json!({"created": 7, "data": [{
                delivery: {"$south.artifact": {"index": 0, "as": delivery}},
                "revised_prompt": "p",
            }]}),
            "{format}"
        );
    }
    for (format, message) in [
        (
            json!({"type": "png"}),
            "'response_format.type' must be 'url' or 'b64_json' for Ideogram models (got 'png')",
        ),
        (
            json!("png"),
            "'response_format' must be 'url', 'b64_json', or {\"type\": \"...\"} for Ideogram \
             models (got \"png\")",
        ),
        (
            json!(5),
            "'response_format' must be 'url', 'b64_json', or {\"type\": \"...\"} for Ideogram \
             models (got 5)",
        ),
    ] {
        let refused =
            prepare("ideogram-v4", &json!({"prompt": "x", "response_format": format})).unwrap_err();
        assert_eq!(refused["message"], message, "{format}");
    }
}

/// §3: the native order of refusals after the upstream id: `prompt`, `n`, `response_format`,
/// `size`.
#[test]
fn refusals_follow_the_native_order() {
    let message = |model: &str, body: Value| {
        prepare(model, &body).unwrap_err()["message"].as_str().unwrap().to_owned()
    };
    assert!(message("ideogram-v3", json!({})).starts_with("Model upstream id 'ideogram-v3'"));
    assert_eq!(message("ideogram-v4", json!({"n": 2})), "Missing 'prompt' field");
    assert_eq!(
        message(
            "ideogram-v4",
            json!({"prompt": "x", "n": 2, "response_format": "png", "size": "?"})
        ),
        "Ideogram v4 generates exactly one image per request; 'n' must be 1 (got 2)"
    );
    assert!(
        message("ideogram-v4", json!({"prompt": "x", "response_format": "png", "size": "?"}))
            .starts_with("'response_format' must be")
    );
    assert_eq!(
        message("ideogram-v4", json!({"prompt": null})),
        "'prompt' must be a string (got null)"
    );
    assert_eq!(message("ideogram-v4", json!({"prompt": "  "})), "'prompt' must not be empty");
}

/// §3: the immutable part names are the native `IDEOGRAM_IMAGE_OWNED`; no other client field
/// reaches the form, and nothing is elided either way.
#[test]
fn the_immutable_parts_are_the_native_owned_fields() {
    let prepared = prepare(
        "ideogram-v4",
        &json!({"prompt": "x", "model": "m", "n": 1, "num_images": 3, "size": "1024x1024",
                "response_format": "b64_json", "quality": "hd", "seed": 7, "image_url": "https://e.test/a.png"}),
    )
    .unwrap();
    assert_eq!(
        prepared["immutable_body_paths"],
        json!(["text_prompt", "rendering_speed", "resolution", "num_images"])
    );
    let names: Vec<String> = parts(&prepared).into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["text_prompt", "rendering_speed", "resolution"]);
    assert_eq!(prepared["response_elision_paths"], json!([]));
    assert_eq!(prepared["facts"]["requested_outputs"], 1);
}

/// §4: unsafe entries (`is_image_safe: false`, or no non-empty `url`) are skipped; the artifacts are
/// `url` pointers with no media type (IG-Q2); a non-empty `data` with every entry filtered is a
/// content-policy refusal; an empty or missing `data` is `unknown`.
#[test]
fn filtered_entries_are_skipped_and_all_filtered_is_refused() {
    let state = json!({"format": "url"});
    let outcome = answer(
        &state,
        200,
        &json!({"data": [
            {"url": "https://i.test/a.png", "is_image_safe": false, "prompt": "a"},
            {"url": "", "prompt": "b"},
            {"prompt": "c"},
            {"url": "https://i.test/d.png", "prompt": "d"},
            {"url": "https://i.test/e.png", "is_image_safe": true},
        ]}),
    );
    let ImageOutcomeV1::Succeeded { artifacts, metering, extras } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        serde_json::to_value(&artifacts).unwrap(),
        json!([{"form": "url", "pointer": "/data/3/url"}, {"form": "url", "pointer": "/data/4/url"}])
    );
    assert_eq!(extras, json!({"revised_prompts": ["d", null]}));
    assert_eq!(metering.images_reported, None);

    let outcome = answer(&state, 200, &json!({"data": [{"url": "", "is_image_safe": false}]}));
    let ImageOutcomeV1::Rejected { error } = outcome else { panic!("{outcome:?}") };
    assert_eq!((error.code, error.http_status), (ErrorCode::ContentPolicy, 422));
    assert_eq!(
        error.message,
        "Ideogram filtered all generated images as unsafe; nothing was billed"
    );

    for body in [json!({"data": []}), json!({}), json!({"data": "nope"}), json!([1])] {
        let outcome = answer(&state, 200, &body);
        assert!(matches!(outcome, ImageOutcomeV1::Unknown { error: None, .. }), "{body}");
    }
}

/// Where the record is silent: a URL or a rewritten prompt the host elided (above its fallback
/// threshold) is a placeholder in the view. The URL is still an artifact (the host then refuses to
/// deliver it, record §10.1), and the prompt is `null` (a placeholder never reaches a template).
#[test]
fn an_elided_url_stays_an_artifact_and_an_elided_prompt_is_null() {
    let placeholder =
        json!({"$south.blob": {"id": "b0", "bytes": 1_048_577, "head": "https://i.test/"}});
    let outcome = answer(
        &json!({"format": "url"}),
        200,
        &json!({"data": [{"url": placeholder, "prompt": placeholder, "is_image_safe": true}]}),
    );
    let ImageOutcomeV1::Succeeded { artifacts, extras, .. } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        serde_json::to_value(&artifacts).unwrap(),
        json!([{"form": "url", "pointer": "/data/0/url"}])
    );
    assert_eq!(extras, json!({"revised_prompts": [null]}));
}

/// §4: a non-2xx carries the body's `error`, `message` or `detail` string, or `error.message`; the
/// code follows the status; a 4xx other than 408 is `rejected`, anything else `unknown`.
#[test]
fn a_non_2xx_names_ideograms_error() {
    for (status, body, code, message) in [
        (401, json!({"message": "nope"}), ErrorCode::Auth, Some("nope")),
        (403, json!({"error": {"message": "denied"}}), ErrorCode::Auth, Some("denied")),
        (422, json!({"error": "unsafe prompt"}), ErrorCode::InvalidRequest, Some("unsafe prompt")),
        (400, json!({"detail": "bad"}), ErrorCode::InvalidRequest, Some("bad")),
        (429, json!({}), ErrorCode::RateLimit, None),
    ] {
        let outcome = answer(&json!({}), status, &body);
        let ImageOutcomeV1::Rejected { error } = outcome else { panic!("{status}: {outcome:?}") };
        assert_eq!((error.code, error.http_status), (code, status));
        assert_eq!(error.provider_message.as_deref(), message, "{status}");
    }
    for status in [408, 500, 503] {
        let outcome = answer(&json!({}), status, &json!({"error": "x"}));
        assert!(matches!(outcome, ImageOutcomeV1::Unknown { error: Some(_), .. }), "{status}");
    }
}

mod mutations {
    //! Each required row fails on a component that breaks it, so a passing report means the row
    //! was checked, not skipped.

    use super::{manifest, pack};
    use serde_json::{Value, json};
    use south_component_conformance::reference_ideogram_image::IdeogramImageReferenceV1;
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
        InternalRefusal,
        AllFilteredIsUnknown,
        UrlDeliveryIgnoresTheClient,
    }

    struct Broken(Break);

    impl ImageComponentV1 for Broken {
        fn metadata(&self) -> ComponentMetadataV1 {
            IdeogramImageReferenceV1.metadata()
        }
        fn model_capabilities(
            &self,
            config: &ProviderConfig,
        ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
            IdeogramImageReferenceV1.model_capabilities(config)
        }
        fn prepare(
            &self,
            config: &ProviderConfig,
            request: &Value,
            context: &ImageCallContextV1,
        ) -> ComponentResultV1<PreparedImageCallV1> {
            IdeogramImageReferenceV1.prepare(config, request, context).map_err(|error| {
                match self.0 {
                    Break::InternalRefusal => {
                        ErrorEnvelope::new(ErrorCode::Internal, 500, "refused")
                    }
                    _ => error,
                }
            })
        }
        fn parse_response(
            &self,
            state: &Value,
            response: &Value,
        ) -> ComponentResultV1<ImageOutcomeV1> {
            let outcome = IdeogramImageReferenceV1.parse_response(state, response)?;
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
                (Break::AllFilteredIsUnknown, ImageOutcomeV1::Rejected { error })
                    if error.code == ErrorCode::ContentPolicy =>
                {
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
            match self.0 {
                Break::UrlDeliveryIgnoresTheClient => {
                    IdeogramImageReferenceV1.render(&json!({"format": "url"}), outcomes, context)
                }
                _ => IdeogramImageReferenceV1.render(state, outcomes, context),
            }
        }
    }

    #[test]
    fn each_row_bites() {
        for (broken, check) in [
            (Break::RetriableAuth, CheckV1::AuthErrorsAreNotRetriable),
            (Break::BlobInExtras, CheckV1::ReferenceIntegrity),
            (Break::InternalRefusal, CheckV1::PreDispatchRefusal),
            (Break::AllFilteredIsUnknown, CheckV1::FixtureMatch),
            (Break::UrlDeliveryIgnoresTheClient, CheckV1::FixtureMatch),
        ] {
            let report = run_image_component_suite_v1(&Broken(broken), &pack(), &manifest());
            let failing: Vec<CheckV1> = report.failures().map(|outcome| outcome.check).collect();
            assert!(failing.contains(&check), "{check:?} did not fail: {failing:?}");
        }
    }
}
