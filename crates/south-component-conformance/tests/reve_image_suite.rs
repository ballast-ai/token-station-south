//! `south.image-component.v1` against the `image-reve` native reference and its shipped fixture
//! pack, and the behaviours of design record `2026-10-13-image-i3-6-reve.md` §3 to §5 pinned by
//! name.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::image_fixture::ImageFixturePackV1;
use south_component_conformance::reference_reve_image::ReveImageReferenceV1;
use south_component_conformance::{ImageComponentV1, ImageOutcomeV1, run_image_component_suite_v1};
use south_contracts::image::{ImageCallContextV1, ImageRenderContextV1};
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("../../components/image-reve/manifest.json")).unwrap(),
    )
    .unwrap()
}

fn pack() -> ImageFixturePackV1 {
    ImageFixturePackV1::load(&root().join("fixtures-image-reve")).unwrap()
}

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "reve-image",
        "base_url": "https://api.reve.com",
        "auth": "provider_api_key",
    }))
    .unwrap()
}

fn context(model: &str) -> ImageCallContextV1 {
    serde_json::from_value(json!({"upstream_model": model, "metering_required": ["credits"]}))
        .unwrap()
}

fn prepared(request: &Value) -> Result<Value, Value> {
    ReveImageReferenceV1
        .prepare(&config(), request, &context("latest"))
        .map(|prepared| serde_json::from_str(&prepared.to_json()).unwrap())
        .map_err(|error| serde_json::to_value(error).unwrap())
}

/// The prepared generation as JSON, or the refusal.
fn generate(body: &Value) -> Result<Value, Value> {
    prepared(&json!({ "json": body }))
}

fn text(name: &str, value: &str) -> Value {
    json!({"kind": "text", "name": name, "value": value})
}

fn file(name: &str, blob: &str) -> Value {
    json!({"kind": "file", "name": name, "blob": blob, "filename": "in.png", "bytes": 4096,
           "media_type": "image/png"})
}

/// The prepared edit as JSON, or the refusal.
fn edit(parts: &[Value]) -> Result<Value, Value> {
    prepared(&json!({ "multipart": { "parts": parts } }))
}

fn message(refusal: &Value) -> &str {
    refusal["message"].as_str().unwrap()
}

/// The `aspect_ratio` a generation with this `size` sends, or the refusal's message.
fn ratio(size: &Value) -> Result<Option<String>, String> {
    generate(&json!({"prompt": "p", "size": size}))
        .map(|prepared| {
            prepared["descriptor"]["body"]["json"]["template"]["aspect_ratio"]
                .as_str()
                .map(str::to_owned)
        })
        .map_err(|refusal| message(&refusal).to_owned())
}

fn answer(status: u16, body: &Value) -> ImageOutcomeV1 {
    let view = json!({"status": status, "headers": [], "body": {"json": body}});
    ReveImageReferenceV1.parse_response(&json!({}), &view).unwrap()
}

#[test]
fn the_shipped_manifest_passes_gate_one() {
    assert_eq!(manifest().validate(), Ok(()));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let report = run_image_component_suite_v1(&ReveImageReferenceV1, &pack(), &manifest());
    let failures: Vec<String> = report
        .failures()
        .map(|outcome| format!("{} {}: {}", outcome.check.as_str(), outcome.case, outcome.detail()))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Decision 1: every configured model (Reve versions are open-ended), `generate` and `edit`, no
/// input role, `b64_json` only, `credits` only, nothing elided on the request side.
#[test]
fn every_configured_model_is_declared_with_credits_only() {
    let mut config = config();
    config.models =
        serde_json::from_value(json!([{"model": "latest"}, {"model": "reve-remix@20250915"}]))
            .unwrap();
    let models =
        serde_json::to_value(ReveImageReferenceV1.model_capabilities(&config).unwrap()).unwrap();
    let ids: Vec<&str> =
        models.as_array().unwrap().iter().map(|m| m["model"].as_str().unwrap()).collect();
    assert_eq!(ids, ["latest", "reve-remix@20250915"]);
    for model in models.as_array().unwrap() {
        assert_eq!(model["operations"], json!(["generate", "edit"]));
        assert_eq!(model["input_roles"], json!([]));
        assert_eq!(model["response_formats"], json!(["b64_json"]));
        assert_eq!(model["metering_forms"], json!(["credits"]));
        assert_eq!(model["token_buckets"], json!([]));
        assert_eq!(model["repeat"], json!(false));
        assert_eq!(model["request_elision_paths"], json!([]));
    }
}

/// RV-Q2 (c): the pinned `endpoint` alone decides create, edit or remix. A JSON request is a
/// generation (`create` or no pin); a multipart request needs `edit` or `remix`, which choose the
/// path, the body keys, the image count and the input role.
#[test]
fn the_pinned_endpoint_tells_create_edit_and_remix_apart() {
    for pin in [None, Some(json!("create")), Some(Value::Null)] {
        let mut body = json!({"prompt": "p"});
        if let Some(pin) = pin.clone() {
            body["endpoint"] = pin;
        }
        let prepared = generate(&body).unwrap();
        assert_eq!(prepared["descriptor"]["path"], "v1/image/create", "{pin:?}");
        assert_eq!(prepared["facts"]["operation"], "generate");
        let template = &prepared["descriptor"]["body"]["json"]["template"];
        assert!(template.get("endpoint").is_none(), "the pin never reaches Reve");
    }
    for pin in ["edit", "remix"] {
        let refusal = generate(&json!({"prompt": "p", "endpoint": pin})).unwrap_err();
        assert_eq!(refusal["code"], "invalid_request", "{pin}");
        assert!(message(&refusal).contains("only accepts Reve create models"), "{pin}");
    }
    for pin in [json!("CREATE"), json!("variation"), json!(1)] {
        let refusal = generate(&json!({"prompt": "p", "endpoint": pin})).unwrap_err();
        assert_eq!(refusal["code"], "capability", "{pin}");
    }

    let image = file("image", "b0");
    let one = edit(&[text("endpoint", "edit"), text("prompt", "p"), image.clone()]).unwrap();
    assert_eq!(one["descriptor"]["path"], "v1/image/edit");
    assert_eq!(one["facts"]["operation"], "edit");
    assert_eq!(one["facts"]["inputs"], json!({"input_image": 1, "mask": 0, "reference_image": 0}));
    let template = &one["descriptor"]["body"]["json"]["template"];
    assert_eq!(template["edit_instruction"], "p");
    assert_eq!(
        template["reference_image"],
        json!({"$south.ref": {"blob": "b0", "transform": "base64"}})
    );
    assert!(template.get("prompt").is_none() && template.get("reference_images").is_none());

    let two = file("image[]", "b1");
    let remix = edit(&[text("endpoint", "remix"), text("prompt", "p"), image.clone(), two.clone()])
        .unwrap();
    assert_eq!(remix["descriptor"]["path"], "v1/image/remix");
    assert_eq!(
        remix["facts"]["inputs"],
        json!({"input_image": 0, "mask": 0, "reference_image": 2})
    );
    let template = &remix["descriptor"]["body"]["json"]["template"];
    assert_eq!(template["prompt"], "p");
    assert_eq!(
        template["reference_images"],
        json!([
            {"$south.ref": {"blob": "b0", "transform": "base64"}},
            {"$south.ref": {"blob": "b1", "transform": "base64"}},
        ])
    );
    assert!(
        template.get("edit_instruction").is_none() && template.get("reference_image").is_none()
    );

    // The count follows the endpoint: an edit takes exactly one image, a remix one to six.
    let refusal =
        edit(&[text("endpoint", "edit"), text("prompt", "p"), image.clone(), two]).unwrap_err();
    assert!(message(&refusal).starts_with("Reve edit takes exactly one `image` part (got 2)"));
    let mut seven = vec![text("endpoint", "remix"), text("prompt", "p")];
    seven.extend((0..7).map(|index| file("image", &format!("b{index}"))));
    let refusal = edit(&seven).unwrap_err();
    assert_eq!(message(&refusal), "Reve remix takes between 1 and 6 `image` parts (got 7)");
    seven.pop();
    assert!(edit(&seven).is_ok());

    // An edit without a pin, with `create`, with two pins or a pin as a file part is refused.
    for (parts, code) in [
        (vec![text("prompt", "p"), image.clone()], "capability"),
        (vec![text("endpoint", "create"), text("prompt", "p"), image.clone()], "invalid_request"),
        (
            vec![
                text("endpoint", "edit"),
                text("endpoint", "remix"),
                text("prompt", "p"),
                image.clone(),
            ],
            "capability",
        ),
        (vec![file("endpoint", "b1"), text("prompt", "p"), image], "capability"),
    ] {
        assert_eq!(edit(&parts).unwrap_err()["code"], code, "{parts:?}");
    }
}

/// §3: the native `size_to_reve_aspect_ratio` table (the host's unit test
/// `size_maps_to_supported_aspect_ratios`, plus the record's rows).
#[test]
fn the_size_table_is_the_native_one() {
    for (size, expected) in [
        (json!("1024x1024"), Some("1:1")),
        (json!("1536x1024"), Some("3:2")),
        (json!("1024x1536"), Some("2:3")),
        (json!("1792x1024"), Some("16:9")),
        (json!("1024x1792"), Some("9:16")),
        (json!("1280x960"), Some("4:3")),
        (json!("900x1200"), Some("3:4")),
        // 7/8 lies exactly between 3:4 and 1:1; the earlier entry wins.
        (json!("7x8"), Some("3:4")),
        (json!("1024X1024"), Some("1:1")),
        (json!("+5x5"), Some("1:1")),
        (json!(" 1600 x 900 "), Some("16:9")),
        (json!("auto"), None),
        (json!("AUTO"), None),
        (json!(""), None),
        (json!(null), None),
        // A non-string `size` is ignored (native `as_str`).
        (json!(1024), None),
    ] {
        assert_eq!(ratio(&size).unwrap().as_deref(), expected, "{size}");
    }
    for (size, refusal) in [
        ("huge", "invalid size 'huge', expected WxH (e.g. 1024x1024) or 'auto'"),
        ("axb", "invalid width in size 'axb'"),
        ("10xb", "invalid height in size '10xb'"),
        ("5x5x5", "invalid height in size '5x5x5'"),
        ("-5x100", "invalid width in size '-5x100'"),
        ("0x100", "size '0x100' must have positive width and height"),
        (" 100x0 ", "size '100x0' must have positive width and height"),
    ] {
        assert_eq!(ratio(&json!(size)).unwrap_err(), refusal, "{size}");
    }
    // An explicit `aspect_ratio` string wins, unvalidated, and `size` is then not read.
    let prepared =
        generate(&json!({"prompt": "p", "aspect_ratio": "banana", "size": "huge"})).unwrap();
    assert_eq!(prepared["descriptor"]["body"]["json"]["template"]["aspect_ratio"], "banana");
    assert!(prepared["facts"].get("size").is_none(), "P5: no size fact");
}

/// §3 and §4: the native order of refusals on both surfaces.
#[test]
fn refusals_follow_the_native_order() {
    let refused = |body: Value| message(&generate(&body).unwrap_err()).to_owned();
    assert!(refused(json!({"endpoint": "remix", "n": 2})).contains("only accepts Reve create"));
    assert!(
        refused(json!({"n": 2, "stream": true})).starts_with("Reve generates exactly one image")
    );
    assert!(refused(json!({"stream": true, "output_format": "webp"})).starts_with("streaming"));
    assert!(refused(json!({"output_format": "jpeg"})).starts_with("output_format 'jpeg'"));
    assert_eq!(refused(json!({"size": "huge"})), "Missing 'prompt' field");

    let refused = |parts: &[Value]| message(&edit(parts).unwrap_err()).to_owned();
    let pin = text("endpoint", "edit");
    assert!(refused(&[text("prompt", "p"), file("mask", "b0")]).starts_with("the route pins no"));
    assert!(refused(&[pin.clone(), text("n", "2"), file("mask", "b0")]).starts_with("`mask`"));
    assert!(
        refused(&[pin.clone(), text("n", "3"), text("stream", "true")]).contains("multipart `n`")
    );
    assert!(
        refused(&[pin.clone(), text("stream", "1"), text("output_format", "webp")])
            .starts_with("streaming")
    );
    assert!(
        refused(&[pin.clone(), text("output_format", ""), file("image", "b0")])
            .starts_with("output_format ''")
    );
    assert_eq!(
        refused(&[pin.clone(), text("size", "huge")]),
        "Missing `prompt` field in multipart body"
    );
    assert!(
        refused(&[
            pin.clone(),
            text("prompt", "p"),
            text("size", "huge"),
            text("test_time_scaling", "x")
        ])
        .starts_with("invalid size")
    );
    assert!(
        refused(&[
            pin.clone(),
            text("prompt", "p"),
            text("postprocessing", "x"),
            text("test_time_scaling", "x")
        ])
        .contains("`test_time_scaling`")
    );
    assert!(
        refused(&[pin, text("prompt", "p"), text("postprocessing", "{}")]).contains("JSON array")
    );
}

/// §3 and §4: the immutable body paths are the native `REVE_IMAGE_OWNED` on both surfaces, and
/// the generation copies only the native keys.
#[test]
fn the_immutable_paths_are_the_native_owned_fields() {
    let owned = json!(["version", "test_time_scaling", "postprocessing"]);
    let generation = generate(&json!({"prompt": "p", "model": "m", "n": 1, "quality": "hd",
        "response_format": "url", "seed": 7, "image": "x", "breadcrumb": "b"}))
    .unwrap();
    assert_eq!(generation["immutable_body_paths"], owned);
    assert_eq!(
        generation["descriptor"]["body"]["json"]["template"],
        json!({"prompt": "p", "version": "latest", "breadcrumb": "b"})
    );
    assert_eq!(
        generation["descriptor"]["auth"],
        json!({"arm": "bearer", "slot": "provider_api_key"})
    );
    assert_eq!(
        generation["descriptor"]["headers"],
        json!([{"name": "accept", "value": "application/json"}])
    );
    assert_eq!(generation["response_elision_paths"], json!(["/image"]));
    let edited =
        edit(&[text("endpoint", "edit"), text("prompt", "p"), file("image", "b0")]).unwrap();
    assert_eq!(edited["immutable_body_paths"], owned);
}

/// §4: multipart extras keep the native types: `test_time_scaling` is a JSON float (`3` goes
/// upstream as `3.0`, a non-finite number as `null`), `postprocessing` a parsed array,
/// `breadcrumb` the trimmed text; `version` the trimmed text, else the row's.
#[test]
fn multipart_extras_keep_the_native_types() {
    let template =
        |parts: &[Value]| edit(parts).unwrap()["descriptor"]["body"]["json"]["template"].clone();
    let base = [text("endpoint", "remix"), text("prompt", "p"), file("image", "b0")];
    let with = |extra: &[Value]| [base.as_slice(), extra].concat();
    let three = template(&with(&[text("test_time_scaling", "3")]));
    assert_eq!(three["test_time_scaling"].to_string(), "3.0");
    assert!(three["test_time_scaling"].is_f64());
    assert_eq!(
        template(&with(&[text("test_time_scaling", "NaN")]))["test_time_scaling"],
        Value::Null
    );
    let full = template(&with(&[
        text("postprocessing", " [{\"process\":\"upscale\"}] "),
        text("breadcrumb", " job-7 "),
        text("version", " reve-remix@20250915 "),
    ]));
    assert_eq!(full["postprocessing"], json!([{"process": "upscale"}]));
    assert_eq!(full["breadcrumb"], "job-7");
    assert_eq!(full["version"], "reve-remix@20250915");
    assert_eq!(template(&with(&[text("version", "  ")]))["version"], "latest");
    // The generation copies the value as sent: an integer stays an integer.
    let generated = generate(&json!({"prompt": "p", "test_time_scaling": 3})).unwrap();
    assert_eq!(
        generated["descriptor"]["body"]["json"]["template"]["test_time_scaling"].to_string(),
        "3"
    );
}

/// §5 and decision 3: `credits_used` must be a JSON number that is a non-negative decimal with at
/// most six fractional digits; anything else leaves the round `unknown`, never a zero charge.
#[test]
fn the_credit_meter_is_a_decimal_or_unknown() {
    let credits = |value: Option<Value>| {
        let mut body = json!({"image": "aW1n", "content_violation": false});
        if let Some(value) = value {
            body["credits_used"] = value;
        }
        match answer(200, &body) {
            ImageOutcomeV1::Succeeded { metering, .. } => {
                Some(metering.credits.unwrap().as_str().to_owned())
            }
            ImageOutcomeV1::Unknown { reason, error: None } => {
                assert_eq!(reason, "a 2xx without a usable credit meter");
                None
            }
            other => panic!("{other:?}"),
        }
    };
    for (value, expected) in [
        (json!(18), Some("18")),
        (json!(18.0), Some("18")),
        (json!(2.5), Some("2.5")),
        (json!(0), Some("0")),
        (json!(0.123_456), Some("0.123456")),
        (json!(0.000_001), Some("0.000001")),
        (json!(1.5e3), Some("1500")),
        (json!(0.123_456_7), None),
        (json!(-1), None),
        (json!(-0.5), None),
        (json!(1e21), None),
        (json!("18"), None),
        (json!(null), None),
        (json!(true), None),
    ] {
        assert_eq!(credits(Some(value.clone())).as_deref(), expected, "{value}");
    }
    assert_eq!(credits(None), None);
}

/// §5 and decision 4: `content_violation: true` (a JSON boolean) is `charged_failure` with the
/// native content-policy 400 and the credits, whatever `image` holds; without a meter it is
/// `unknown`. Any other value of the flag is not a violation.
#[test]
fn a_content_violation_is_a_charged_failure() {
    for image in [json!(""), json!("aW1n"), Value::Null] {
        let outcome =
            answer(200, &json!({"image": image, "content_violation": true, "credits_used": 30}));
        let ImageOutcomeV1::ChargedFailure { error, metering } = outcome else {
            panic!("{image}: {outcome:?}")
        };
        assert_eq!((error.code, error.http_status), (ErrorCode::ContentPolicy, 400));
        assert_eq!(
            error.message,
            "Reve flagged this request as a content policy violation; the upstream still metered \
             credits for the attempt and the gateway billed them."
        );
        assert_eq!(metering.credits.unwrap().as_str(), "30");
    }
    let outcome = answer(200, &json!({"image": "", "content_violation": true}));
    assert!(matches!(outcome, ImageOutcomeV1::Unknown { error: None, .. }), "{outcome:?}");
    for flag in [json!("true"), json!(1), json!(null), json!(false)] {
        let outcome =
            answer(200, &json!({"image": "aW1n", "content_violation": flag, "credits_used": 5}));
        assert!(matches!(outcome, ImageOutcomeV1::Succeeded { .. }), "{flag}: {outcome:?}");
    }
    // Without a violation an empty or missing image is `unknown` (host defect §10 item 3).
    for body in [json!({"image": "", "credits_used": 5}), json!({"credits_used": 5}), json!([1])] {
        let outcome = answer(200, &body);
        assert!(matches!(outcome, ImageOutcomeV1::Unknown { error: None, .. }), "{body}");
    }
}

/// §5 and RV-Q5 (b): the render is the native envelope with its flat metadata, never
/// `credits_remaining`; a placeholder in the metadata becomes `null`.
#[test]
fn the_render_drops_the_upstream_balance() {
    let placeholder = json!({"$south.blob": {"id": "b0", "bytes": 1_048_577, "head": "rsid-"}});
    let outcome = answer(
        200,
        &json!({"image": "aW1n", "version": "v", "request_id": placeholder, "credits_used": 18,
                "credits_remaining": 982}),
    );
    let rendered = ReveImageReferenceV1
        .render(&json!({}), &[outcome], &ImageRenderContextV1 { created: 7 })
        .unwrap();
    let template: Value = serde_json::from_str(&rendered.template).unwrap();
    assert_eq!(
        template,
        json!({
            "created": 7,
            "data": [{"b64_json": {"$south.artifact": {"index": 0, "as": "b64_json"}}}],
            "request_id": null,
            "version": "v",
            "content_violation": false,
            "credits_used": 18,
        })
    );
}

/// §5: a non-2xx carries the body's `error`, `message` or `detail` string, or `error.message`; a
/// 4xx other than 408 (Reve's 402 included) is `rejected`, anything else `unknown`.
#[test]
fn a_non_2xx_names_reves_error() {
    for (status, body, code, provider_message) in [
        (400, json!({"error": "bad"}), ErrorCode::InvalidRequest, Some("bad")),
        (401, json!({"message": "nope"}), ErrorCode::Auth, Some("nope")),
        (
            402,
            json!({"error": {"message": "out of credits"}}),
            ErrorCode::PaymentRequired,
            Some("out of credits"),
        ),
        (403, json!({"detail": "denied"}), ErrorCode::Auth, Some("denied")),
        (429, json!({}), ErrorCode::RateLimit, None),
    ] {
        let outcome = answer(status, &body);
        let ImageOutcomeV1::Rejected { error } = outcome else { panic!("{status}: {outcome:?}") };
        assert_eq!((error.code, error.http_status), (code, status));
        assert_eq!(error.provider_message.as_deref(), provider_message, "{status}");
    }
    for status in [408, 500, 503] {
        let outcome = answer(status, &json!({"error": "x"}));
        assert!(matches!(outcome, ImageOutcomeV1::Unknown { error: Some(_), .. }), "{status}");
    }
}

mod mutations {
    //! Each required row fails on a component that breaks it, so a passing report means the row
    //! was checked, not skipped. The last case runs another package's pack: the suite's
    //! `charged_failure` rule (record §7.3) also covers the `images` column.

    use super::{manifest, pack};
    use serde_json::{Value, json};
    use south_component_conformance::image_fixture::ImageFixturePackV1;
    use south_component_conformance::reference_ideogram_image::IdeogramImageReferenceV1;
    use south_component_conformance::reference_reve_image::ReveImageReferenceV1;
    use south_component_conformance::{
        CheckV1, ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1,
        run_image_component_suite_v1,
    };
    use south_contracts::image::{
        CreditsV1, ImageCallContextV1, ImageMeteringV1, ImageModelCapabilitiesV1,
        ImageRenderContextV1, PreparedImageCallV1,
    };
    use south_provider_api::{ComponentManifestV1, ComponentMetadataV1};
    use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

    #[derive(Clone, Copy)]
    enum Break {
        RetriableAuth,
        InternalRefusal,
        ViolationIsRejected,
        ChargedFailureWithoutCredits,
        MissingMeterIsZero,
        RemixAsEdit,
    }

    /// `inner` with one behaviour broken.
    struct Broken<C>(C, Break);

    impl<C: ImageComponentV1> ImageComponentV1 for Broken<C> {
        fn metadata(&self) -> ComponentMetadataV1 {
            self.0.metadata()
        }
        fn model_capabilities(
            &self,
            config: &ProviderConfig,
        ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
            self.0.model_capabilities(config)
        }
        fn prepare(
            &self,
            config: &ProviderConfig,
            request: &Value,
            context: &ImageCallContextV1,
        ) -> ComponentResultV1<PreparedImageCallV1> {
            let mut request = request.clone();
            if let (Break::RemixAsEdit, Some(parts)) =
                (self.1, request.pointer_mut("/multipart/parts").and_then(Value::as_array_mut))
            {
                for part in parts.iter_mut() {
                    if part["name"] == "endpoint" && part["value"] == "remix" {
                        part["value"] = json!("edit");
                    }
                }
            }
            self.0.prepare(config, &request, context).map_err(|error| match self.1 {
                Break::InternalRefusal => ErrorEnvelope::new(ErrorCode::Internal, 500, "refused"),
                _ => error,
            })
        }
        fn parse_response(
            &self,
            state: &Value,
            response: &Value,
        ) -> ComponentResultV1<ImageOutcomeV1> {
            let outcome = self.0.parse_response(state, response)?;
            Ok(match (self.1, outcome) {
                (Break::RetriableAuth, ImageOutcomeV1::Rejected { mut error })
                    if error.code == ErrorCode::Auth =>
                {
                    error.code = ErrorCode::UpstreamUnavailable;
                    ImageOutcomeV1::Rejected { error }
                }
                (Break::ViolationIsRejected, ImageOutcomeV1::ChargedFailure { error, .. }) => {
                    ImageOutcomeV1::Rejected { error }
                }
                (
                    Break::ChargedFailureWithoutCredits,
                    ImageOutcomeV1::ChargedFailure { error, .. }
                    | ImageOutcomeV1::Rejected { error },
                ) => ImageOutcomeV1::ChargedFailure { error, metering: ImageMeteringV1::default() },
                (Break::MissingMeterIsZero, ImageOutcomeV1::Unknown { reason, .. })
                    if reason.contains("credit meter") =>
                {
                    ImageOutcomeV1::ChargedFailure {
                        error: ErrorEnvelope::new(ErrorCode::ContentPolicy, 400, "x"),
                        metering: ImageMeteringV1 {
                            credits: Some(CreditsV1::parse("0").unwrap()),
                            ..ImageMeteringV1::default()
                        },
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
            self.0.render(state, outcomes, context)
        }
    }

    fn failing(
        component: &dyn ImageComponentV1,
        pack: &ImageFixturePackV1,
        manifest: &ComponentManifestV1,
    ) -> Vec<CheckV1> {
        run_image_component_suite_v1(component, pack, manifest)
            .failures()
            .map(|outcome| outcome.check)
            .collect()
    }

    #[test]
    fn each_row_bites() {
        for (broken, check) in [
            (Break::RetriableAuth, CheckV1::AuthErrorsAreNotRetriable),
            (Break::InternalRefusal, CheckV1::PreDispatchRefusal),
            (Break::ViolationIsRejected, CheckV1::FixtureMatch),
            (Break::ChargedFailureWithoutCredits, CheckV1::MissingMeterIsNotZero),
            (Break::MissingMeterIsZero, CheckV1::MissingMeterIsNotZero),
            (Break::RemixAsEdit, CheckV1::FixtureMatch),
        ] {
            let failing = failing(&Broken(ReveImageReferenceV1, broken), &pack(), &manifest());
            assert!(failing.contains(&check), "{check:?} did not fail: {failing:?}");
        }
    }

    /// The `images` column of §9.1: `image-ideogram`'s all-filtered refusal turned into a
    /// `charged_failure` without `images_reported` fails `missing_meter_is_not_zero`.
    #[test]
    fn a_charged_failure_without_images_reported_fails() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(root.join("../../components/image-ideogram/manifest.json"))
                .unwrap(),
        )
        .unwrap();
        let pack = ImageFixturePackV1::load(&root.join("fixtures-image-ideogram")).unwrap();
        assert!(
            !failing(&IdeogramImageReferenceV1, &pack, &manifest)
                .contains(&CheckV1::MissingMeterIsNotZero)
        );
        let broken = Broken(IdeogramImageReferenceV1, Break::ChargedFailureWithoutCredits);
        assert!(failing(&broken, &pack, &manifest).contains(&CheckV1::MissingMeterIsNotZero));
    }
}
