//! `south.image-component.v1` against the `image-openai-compatible` native reference and its
//! shipped fixture pack, and the behaviours of design record
//! `2026-10-16-image-openai-compatible.md` §2 to §5 pinned by name.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::image_fixture::ImageFixturePackV1;
use south_component_conformance::reference_openai_compatible_image::{
    AZURE_OPENAI_DATED, AZURE_OPENAI_V1, EDIT_IMMUTABLE, GENERATION_IMMUTABLE, OPENAI_COMPATIBLE,
    OpenAiCompatibleImageReferenceV1,
};
use south_component_conformance::{ImageComponentV1, ImageOutcomeV1, run_image_component_suite_v1};
use south_contracts::image::{ImageCallContextV1, ImageRenderContextV1, ImageTokenUsageV1};
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

const REFERENCE: OpenAiCompatibleImageReferenceV1 = OpenAiCompatibleImageReferenceV1;
const GLM: &str = "https://api.z.ai/api/paas/v4";
const AZURE: &str = "https://example-resource.openai.azure.com";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(
        &std::fs::read_to_string(
            root().join("../../components/image-openai-compatible/manifest.json"),
        )
        .unwrap(),
    )
    .unwrap()
}

fn pack() -> ImageFixturePackV1 {
    ImageFixturePackV1::load(&root().join("fixtures-image-openai-compatible")).unwrap()
}

fn config(provider: &str, base: &str, declared: Option<Value>) -> ProviderConfig {
    let mut config = json!({ "provider": provider, "base_url": base, "auth": "provider_api_key" });
    if let Some(declared) = declared {
        config["declared"] = declared;
    }
    serde_json::from_value(config).unwrap()
}

fn context(model: &str, forms: &[&str]) -> ImageCallContextV1 {
    serde_json::from_value(json!({ "upstream_model": model, "metering_required": forms })).unwrap()
}

/// The prepared call as JSON, or the refusal.
fn prepare(config: &ProviderConfig, request: &Value, model: &str) -> Result<Value, ErrorEnvelope> {
    REFERENCE
        .prepare(config, request, &context(model, &["images"]))
        .map(|prepared| serde_json::from_str(&prepared.to_json()).unwrap())
}

fn generate(config: &ProviderConfig, body: &Value) -> Result<Value, ErrorEnvelope> {
    prepare(config, &json!({ "json": body }), "gpt-image-1")
}

fn edit(config: &ProviderConfig, parts: &Value) -> Result<Value, ErrorEnvelope> {
    prepare(config, &json!({ "multipart": { "parts": parts } }), "gpt-image-1")
}

fn text(name: &str, value: &str) -> Value {
    json!({ "kind": "text", "name": name, "value": value })
}

fn file(name: &str, blob: &str) -> Value {
    json!({ "kind": "file", "name": name, "blob": blob, "filename": "a.png", "bytes": 10 })
}

/// `parse-response` on a view built by hand (no elision: the strings are plain).
fn answer(forms: &[&str], status: u16, body: &Value) -> ImageOutcomeV1 {
    let view = json!({ "status": status, "headers": [], "body": { "json": body } });
    REFERENCE.parse_response(&json!({ "metering_required": forms }), &view).unwrap()
}

fn tokens_of(outcome: &ImageOutcomeV1) -> Option<ImageTokenUsageV1> {
    match outcome {
        ImageOutcomeV1::Succeeded { metering, .. } => metering.tokens,
        other => panic!("not succeeded: {other:?}"),
    }
}

#[test]
fn the_shipped_manifest_passes_gate_one() {
    assert_eq!(manifest().validate(), Ok(()));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let report = run_image_component_suite_v1(&REFERENCE, &pack(), &manifest());
    let failures: Vec<String> = report
        .failures()
        .map(|outcome| format!("{} {}: {}", outcome.check.as_str(), outcome.case, outcome.detail()))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The manifest names the three families of step 1, both auth arms and the dated family's
/// `api_version` key with native's default (record §9, OCI-Q1 (b)).
#[test]
fn the_manifest_declares_the_step_1_families() {
    let manifest = manifest();
    assert_eq!(
        manifest.providers.iter().map(String::as_str).collect::<Vec<_>>(),
        [OPENAI_COMPATIBLE, AZURE_OPENAI_V1, AZURE_OPENAI_DATED]
    );
    assert_eq!(manifest.auth_arms.iter().collect::<Vec<_>>(), ["bearer", "header_secret"]);
    assert!(manifest.declared_keys(OPENAI_COMPATIBLE).is_empty());
    assert!(manifest.declared_keys(AZURE_OPENAI_V1).is_empty());
    assert_eq!(
        manifest.declared_keys(AZURE_OPENAI_DATED).into_iter().collect::<Vec<_>>(),
        ["api_version"]
    );
    assert_eq!(
        manifest.config_schema[AZURE_OPENAI_DATED]["api_version"].default.as_deref(),
        Some("2024-10-21")
    );
}

/// §4: every configured id is declared (the fallback serves any id), the same way on each family.
#[test]
fn every_configured_id_is_declared_on_every_family() {
    let ids = ["cogview-4-250304", "seedream-4-0-250828", "black-forest-labs/FLUX-1-schnell"];
    let mut declared = Vec::new();
    for (provider, base) in [
        (OPENAI_COMPATIBLE, GLM.to_owned()),
        (AZURE_OPENAI_V1, format!("{AZURE}/openai/v1")),
        (AZURE_OPENAI_DATED, AZURE.to_owned()),
    ] {
        let mut config = config(provider, &base, None);
        config.models =
            serde_json::from_value(json!(ids.map(|id| json!({ "model": id })))).unwrap();
        let models = serde_json::to_value(REFERENCE.model_capabilities(&config).unwrap()).unwrap();
        let names: Vec<&str> =
            models.as_array().unwrap().iter().map(|m| m["model"].as_str().unwrap()).collect();
        assert_eq!(names, ids, "{provider}");
        for model in models.as_array().unwrap() {
            assert_eq!(model["metering_forms"], json!(["tokens", "images"]));
            assert_eq!(model["token_buckets"], json!(["total_output"]), "OCI-Q4");
            assert_eq!(model["request_elision_paths"], json!(["/image", "/image/*"]), "OCI-Q5");
        }
        declared.push(models);
    }
    assert!(declared.windows(2).all(|pair| pair[0] == pair[1]));
}

/// §3: the API-root rule (an endpoint with a path is the root, an origin gets `v1`), for
/// `openai-compatible` and `azure-openai-v1`; D13's two misconfigured shapes compose differently
/// from native, by design.
#[test]
fn the_api_root_rule_places_the_path() {
    for (base, generation, edit_path) in [
        (GLM, "images/generations", "images/edits"),
        ("https://ark.ap-southeast.bytepluses.com/api/v3", "images/generations", "images/edits"),
        ("https://api.deepinfra.com/v1/openai", "images/generations", "images/edits"),
        ("https://api.openai.com", "v1/images/generations", "v1/images/edits"),
        ("https://api.openai.com/", "v1/images/generations", "v1/images/edits"),
        // D13: native `{endpoint}/v1/…` for an `openai` row and `{endpoint}/images/…` for GLM.
        ("https://api.openai.com/v1", "images/generations", "images/edits"),
        ("https://api.z.ai", "v1/images/generations", "v1/images/edits"),
    ] {
        for provider in [OPENAI_COMPATIBLE, AZURE_OPENAI_V1] {
            let config = config(provider, base, None);
            let prepared = generate(&config, &json!({ "prompt": "p" })).unwrap();
            assert_eq!(prepared["descriptor"]["path"], generation, "{provider} {base}");
            assert!(prepared["descriptor"].get("query").is_none());
            let prepared = edit(&config, &json!([file("image", "b0")])).unwrap();
            assert_eq!(prepared["descriptor"]["path"], edit_path, "{provider} {base}");
        }
    }
}

/// `openai-compatible` presents `Bearer`; both Azure families the `api-key` header (native
/// `apply_provider_auth`).
#[test]
fn the_auth_arm_follows_the_family() {
    for (provider, base, auth) in [
        (OPENAI_COMPATIBLE, GLM, json!({ "arm": "bearer", "slot": "provider_api_key" })),
        (
            AZURE_OPENAI_V1,
            "https://example-resource.openai.azure.com/openai/v1",
            json!({ "arm": "header_secret", "header": "api-key", "slot": "provider_api_key" }),
        ),
        (
            AZURE_OPENAI_DATED,
            AZURE,
            json!({ "arm": "header_secret", "header": "api-key", "slot": "provider_api_key" }),
        ),
    ] {
        let prepared = generate(&config(provider, base, None), &json!({ "prompt": "p" })).unwrap();
        assert_eq!(prepared["descriptor"]["auth"], auth, "{provider}");
    }
}

/// §9: the dated family addresses the deployment (the upstream model, one escaped segment, as
/// `speech-openai-compatible` does) with the sanctioned `api-version` query; the version is the
/// config value, else native's `2024-10-21`; an undated value is a configuration error and an
/// empty deployment a capability refusal, both before any request refusal.
#[test]
fn the_dated_family_addresses_the_deployment() {
    let dated = config(AZURE_OPENAI_DATED, AZURE, None);
    for (deployment, path) in [
        ("gpt-image-1-prod", "openai/deployments/gpt-image-1-prod/images/generations"),
        ("image prod", "openai/deployments/image%20prod/images/generations"),
        ("a/b", "openai/deployments/a%2Fb/images/generations"),
    ] {
        let prepared = prepare(&dated, &json!({ "json": { "prompt": "p" } }), deployment).unwrap();
        assert_eq!(prepared["descriptor"]["path"], path, "{deployment}");
        assert_eq!(
            prepared["descriptor"]["query"],
            json!([{ "name": "api-version", "value": "2024-10-21" }])
        );
        // The body still names the deployment unescaped, as native's does.
        assert_eq!(prepared["descriptor"]["body"]["json"]["template"]["model"], deployment);
    }
    let declared =
        config(AZURE_OPENAI_DATED, AZURE, Some(json!({ "api_version": "2025-04-01-preview" })));
    let prepared = edit(&declared, &json!([file("image", "b0")])).unwrap();
    assert_eq!(prepared["descriptor"]["path"], "openai/deployments/gpt-image-1/images/edits");
    assert_eq!(
        prepared["descriptor"]["query"],
        json!([{ "name": "api-version", "value": "2025-04-01-preview" }])
    );

    let undated = config(AZURE_OPENAI_DATED, AZURE, Some(json!({ "api_version": "v1" })));
    let error = generate(&undated, &json!({ "prompt": "p", "n": 0 })).unwrap_err();
    assert_eq!((error.code, error.http_status), (ErrorCode::Internal, 500));
    let error = prepare(&dated, &json!({ "json": { "prompt": "p", "n": 0 } }), "").unwrap_err();
    assert_eq!((error.code, error.http_status), (ErrorCode::Capability, 400));
}

/// §2.3 / §4: the client's body passes through with `model` replaced; elided strings become
/// `as_is` references; the owned sets are native's.
#[test]
fn the_body_passes_through() {
    let body = json!({
        "model": "client-name", "prompt": "p", "watermark": false, "style": "vivid",
        "image": { "$south.blob": { "id": "b0", "bytes": 40000, "head": "data:image/png" } },
        "extra": { "k": [1, 2] },
    });
    let prepared = generate(&config(OPENAI_COMPATIBLE, GLM, None), &body).unwrap();
    assert_eq!(
        prepared["descriptor"]["body"]["json"]["template"],
        json!({
            "model": "gpt-image-1", "prompt": "p", "watermark": false, "style": "vivid",
            "image": { "$south.ref": { "blob": "b0", "transform": "as_is" } },
            "extra": { "k": [1, 2] },
        })
    );
    assert_eq!(
        prepared["facts"]["inputs"],
        json!({ "input_image": 0, "reference_image": 0, "mask": 0 })
    );
    assert_eq!(prepared["immutable_body_paths"], json!(GENERATION_IMMUTABLE));
    assert_eq!(
        GENERATION_IMMUTABLE,
        ["model", "n", "size", "image_size", "quality", "resolution", "response_format"]
    );
    assert_eq!(
        EDIT_IMMUTABLE,
        ["model", "n", "size", "quality", "width", "height", "image", "mask", "response_format"]
    );
}

/// §2.4: the size hint is `size`, else `image_size`, as a string; `Nk` is `N×1024`.
#[test]
fn the_size_hint_is_native() {
    let config = config(OPENAI_COMPATIBLE, GLM, None);
    for (body, size) in [
        (json!({ "size": "1024x1536" }), json!({ "width": 1024, "height": 1536 })),
        (json!({ "size": "1024*1024" }), json!({ "width": 1024, "height": 1024 })),
        (json!({ "size": "1024×768" }), json!({ "width": 1024, "height": 768 })),
        (json!({ "size": "2K" }), json!({ "width": 2048, "height": 2048 })),
        (json!({ "size": " 1k " }), json!({ "width": 1024, "height": 1024 })),
        (json!({ "image_size": "2k" }), json!({ "width": 2048, "height": 2048 })),
        (
            json!({ "size": "1024x1024", "image_size": "2k" }),
            json!({ "width": 1024, "height": 1024 }),
        ),
        (json!({ "size": 1024, "image_size": "2k" }), Value::Null),
        (json!({ "size": null, "image_size": "2k" }), Value::Null),
        (json!({ "size": "auto" }), Value::Null),
        (json!({ "size": "0x1024" }), Value::Null),
        (json!({ "size": "99999x1" }), Value::Null),
        (json!({}), Value::Null),
    ] {
        let mut request = body.clone();
        request["prompt"] = json!("p");
        let prepared = generate(&config, &request).unwrap();
        assert_eq!(prepared["facts"].get("size").cloned().unwrap_or(Value::Null), size, "{body}");
    }
    let prepared = edit(&config, &json!([file("image", "b0"), text("size", " 2K ")])).unwrap();
    assert_eq!(prepared["facts"]["size"], json!({ "width": 2048, "height": 2048 }));
}

/// §2.6: parts in order, file parts `as_is`, every `model` text part replaced (appended when
/// absent); `image` / `image[]` and `mask` counted by name; refusals in native order and text.
#[test]
fn the_multipart_edit_follows_the_native_pass_through() {
    let config = config(OPENAI_COMPATIBLE, GLM, None);
    let prepared = edit(
        &config,
        &json!([
            text("prompt", "p"),
            file("image[]", "b0"),
            file("image[]", "b1"),
            file("mask", "b2"),
            text("model", "a"),
            text("model", "b"),
            text("n", "4"),
        ]),
    )
    .unwrap();
    let parts = &prepared["descriptor"]["body"]["multipart"]["parts"];
    let names: Vec<&str> =
        parts.as_array().unwrap().iter().map(|part| part["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["prompt", "image[]", "image[]", "mask", "model", "model", "n"]);
    assert_eq!(parts[4]["value"], "gpt-image-1");
    assert_eq!(parts[5]["value"], "gpt-image-1");
    assert_eq!(parts[1]["transform"], "as_is");
    assert_eq!(
        prepared["facts"]["inputs"],
        json!({ "input_image": 2, "reference_image": 0, "mask": 1 })
    );
    assert_eq!(prepared["facts"]["requested_outputs"], 4);
    assert_eq!(prepared["immutable_body_paths"], json!(EDIT_IMMUTABLE));

    let refusal = |parts: Value| edit(&config, &parts).unwrap_err().message;
    assert_eq!(
        refusal(json!([text("n", "+0"), text("stream", "true")])),
        "multipart `n` field value 0 is outside the supported range 1..=10"
    );
    assert!(refusal(json!([text("stream", "True")])).starts_with("streaming /v1/images/edits"));
    assert!(edit(&config, &json!([text("stream", "yes")])).is_ok(), "native reads only true / 1");
    let images: Vec<Value> = (0..65).map(|i| file("image", &format!("b{i}"))).collect();
    assert_eq!(refusal(json!(images)), "an image edit accepts at most 64 input images (got 65)");
}

/// D11 (OCI-Q3): a top-level `input_tokens` is reported as `total_input` (native billed input as
/// 0 without the breakdown); D9: integral floats count; D10: a negative or fractional count parks
/// a token row and is ignored on an images row; D12: no output count parks a token row.
#[test]
fn token_usage_fixes_the_native_money_defects() {
    let image = json!([{ "b64_json": "aGk=" }]);
    let tokens = tokens_of(&answer(
        &["tokens"],
        200,
        &json!({ "data": image, "usage": { "input_tokens": 20, "output_tokens": 4160 } }),
    ))
    .unwrap();
    assert_eq!((tokens.total_input, tokens.total_output), (Some(20), Some(4160)));
    assert_eq!((tokens.text_input, tokens.image_input), (None, None));

    let tokens = tokens_of(&answer(
        &["tokens"],
        200,
        &json!({ "data": image, "usage": { "input_tokens": 20.0, "output_tokens": 4160.0 } }),
    ))
    .unwrap();
    assert_eq!((tokens.total_input, tokens.total_output), (Some(20), Some(4160)));

    for usage in [
        json!({ "input_tokens": 20, "output_tokens": -1 }),
        json!({ "input_tokens": 20.5, "output_tokens": 1 }),
        json!({ "input_tokens": 20, "output_tokens": "1" }),
    ] {
        let outcome = answer(&["tokens"], 200, &json!({ "data": image, "usage": usage }));
        assert!(matches!(outcome, ImageOutcomeV1::Unknown { .. }), "{usage}");
        let outcome = answer(&["images"], 200, &json!({ "data": image, "usage": usage }));
        assert_eq!(tokens_of(&outcome), None, "{usage}");
    }
    for usage in [json!({ "input_tokens": 50 }), Value::Null] {
        let outcome = answer(&["tokens"], 200, &json!({ "data": image, "usage": usage }));
        assert!(matches!(outcome, ImageOutcomeV1::Unknown { .. }), "{usage}");
    }
}

/// D1 / D2: the host counts delivered images, so every image is an artifact and a 2xx that
/// delivers none is `unknown`; `images_reported` stays `null`.
#[test]
fn the_delivered_images_are_the_artifacts() {
    let outcome = answer(
        &["images"],
        200,
        &json!({ "data": [
            { "url": "https://cdn.example/1.png" }, { "b64_json": "aGk=" }, { "url": "https://cdn.example/3.png" },
        ] }),
    );
    let ImageOutcomeV1::Succeeded { artifacts, metering, .. } = outcome else { panic!() };
    assert_eq!(artifacts.len(), 3);
    assert_eq!(metering.images_reported, None);
    for body in [
        json!({ "data": [] }),
        json!({ "created": 1 }),
        json!({ "data": [{ "url": "https://cdn.example/1.png" }, {}] }),
        json!([{ "url": "https://cdn.example/1.png" }]),
    ] {
        assert!(
            matches!(answer(&["images"], 200, &body), ImageOutcomeV1::Unknown { .. }),
            "{body}"
        );
    }
}

/// U5: the error message is `error.message`, `error`, `message` or `detail`; a 4xx other than 408
/// is `rejected`, anything else `unknown`.
#[test]
fn failures_follow_the_status_table() {
    for (body, message) in [
        (json!({ "error": { "message": "a" }, "message": "b" }), "a"),
        (json!({ "error": "c", "detail": "d" }), "c"),
        (json!({ "message": "e", "detail": "f" }), "e"),
        (json!({ "detail": "g" }), "g"),
    ] {
        let ImageOutcomeV1::Rejected { error } = answer(&["images"], 400, &body) else { panic!() };
        assert_eq!(error.provider_message.as_deref(), Some(message), "{body}");
    }
    for (status, code) in [
        (401, ErrorCode::Auth),
        (403, ErrorCode::Auth),
        (429, ErrorCode::RateLimit),
        (404, ErrorCode::InvalidRequest),
    ] {
        let ImageOutcomeV1::Rejected { error } = answer(&["images"], status, &Value::Null) else {
            panic!("{status}")
        };
        assert_eq!(error.code, code, "{status}");
    }
    for status in [408, 500, 502, 503] {
        assert!(matches!(
            answer(&["images"], status, &Value::Null),
            ImageOutcomeV1::Unknown { .. }
        ));
    }
}

/// The render fills the upstream body: each image a marker in the form the upstream answered in.
#[test]
fn the_render_is_the_upstream_body_with_markers() {
    let outcome = answer(
        &["images"],
        200,
        &json!({ "created": 7, "data": [
            { "url": "https://cdn.example/1.png", "revised_prompt": "r" }, { "b64_json": "aGk=" },
        ], "usage": { "output_tokens": 1 } }),
    );
    let rendered =
        REFERENCE.render(&json!({}), &[outcome], &ImageRenderContextV1 { created: 9 }).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&rendered.template).unwrap(),
        json!({ "created": 7, "data": [
            { "url": { "$south.artifact": { "index": 0, "as": "url" } }, "revised_prompt": "r" },
            { "b64_json": { "$south.artifact": { "index": 1, "as": "b64_json" } } },
        ], "usage": { "output_tokens": 1 } })
    );
}

mod mutations {
    //! Each rule the pack pins, broken on purpose: the suite must fail it.

    use super::{manifest, pack};
    use serde_json::Value;
    use south_component_conformance::reference_openai_compatible_image::OpenAiCompatibleImageReferenceV1;
    use south_component_conformance::{
        CheckV1, ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1,
        run_image_component_suite_v1,
    };
    use south_contracts::image::{
        ImageCallContextV1, ImageModelCapabilitiesV1, ImageRenderContextV1, PreparedImageCallV1,
    };
    use south_contracts::media::{MediaLimitsV1, MediaRequestDescriptorV1};
    use south_provider_api::ComponentMetadataV1;
    use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

    const NATIVE: OpenAiCompatibleImageReferenceV1 = OpenAiCompatibleImageReferenceV1;

    #[derive(Clone, Copy)]
    enum Break {
        /// The API-root rule dropped: `v1/` prefixed whatever the endpoint.
        AlwaysV1,
        /// The D11 fold dropped: a top-level `input_tokens` not reported.
        NoTotalInput,
        /// One delivered image fewer than the upstream answered.
        DropsAnImage,
        /// A render placeholder replaced by `null`.
        NulledPlaceholder,
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
            NATIVE.metadata()
        }
        fn model_capabilities(
            &self,
            config: &ProviderConfig,
        ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
            NATIVE.model_capabilities(config)
        }
        fn prepare(
            &self,
            config: &ProviderConfig,
            request: &Value,
            context: &ImageCallContextV1,
        ) -> ComponentResultV1<PreparedImageCallV1> {
            let mut prepared =
                NATIVE.prepare(config, request, context).map_err(|error| match self.0 {
                    Break::InternalRefusal => {
                        ErrorEnvelope::new(ErrorCode::Internal, 500, "refused")
                    }
                    _ => error,
                })?;
            if let Break::AlwaysV1 = self.0 {
                let mut descriptor: Value =
                    serde_json::from_str(&prepared.descriptor.to_json()).unwrap();
                let path = descriptor["path"].as_str().unwrap().to_owned();
                if !path.starts_with("v1/") && !path.starts_with("openai/") {
                    descriptor["path"] = Value::String(format!("v1/{path}"));
                }
                prepared.descriptor =
                    MediaRequestDescriptorV1::parse(&descriptor.to_string(), &MediaLimitsV1::V1)
                        .unwrap();
            }
            Ok(prepared)
        }
        fn parse_response(
            &self,
            state: &Value,
            response: &Value,
        ) -> ComponentResultV1<ImageOutcomeV1> {
            let outcome = NATIVE.parse_response(state, response)?;
            Ok(match (self.0, outcome) {
                (
                    Break::NoTotalInput,
                    ImageOutcomeV1::Succeeded { artifacts, mut metering, extras },
                ) => {
                    if let Some(tokens) = &mut metering.tokens {
                        tokens.total_input = None;
                    }
                    ImageOutcomeV1::Succeeded { artifacts, metering, extras }
                }
                (
                    Break::DropsAnImage,
                    ImageOutcomeV1::Succeeded { mut artifacts, metering, extras },
                ) if artifacts.len() > 1 => {
                    artifacts.pop();
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
            let rendered = NATIVE.render(state, outcomes, context)?;
            if let Break::NulledPlaceholder = self.0 {
                let mut template: Value = serde_json::from_str(&rendered.template).unwrap();
                if let Some(entry) = template["data"].get_mut(0).and_then(Value::as_object_mut) {
                    for value in entry.values_mut() {
                        if value.get("$south.artifact").is_some() {
                            *value = Value::Null;
                        }
                    }
                }
                return Ok(ImageRenderedV1 { template: template.to_string() });
            }
            Ok(rendered)
        }
    }

    #[test]
    fn each_rule_bites() {
        for (broken, check) in [
            (Break::AlwaysV1, CheckV1::FixtureMatch),
            (Break::NoTotalInput, CheckV1::FixtureMatch),
            (Break::DropsAnImage, CheckV1::FixtureMatch),
            (Break::NulledPlaceholder, CheckV1::FixtureMatch),
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
