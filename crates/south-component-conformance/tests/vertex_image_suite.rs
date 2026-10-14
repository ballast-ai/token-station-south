//! `south.image-component.v1` against the `image-vertex` native reference and its shipped fixture
//! pack (credential recipe cases included), and the behaviours of design record
//! `2026-10-13-image-i3-3-vertex.md` §3 and §4 pinned by name.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::image_fixture::ImageFixturePackV1;
use south_component_conformance::reference_gemini_image::GeminiImageReferenceV1;
use south_component_conformance::reference_vertex_image::VertexImageReferenceV1;
use south_component_conformance::{
    CheckV1, ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1, ReportV1,
    accepts_manifest, reported_identity_matches, run_image_component_suite_v1,
};
use south_contracts::image::{
    ImageCallContextV1, ImageModelCapabilitiesV1, ImageOperationV1, ImageRenderContextV1,
    PreparedImageCallV1,
};
use south_provider_api::{ComponentManifestV1, ComponentMetadataV1};
use token_station_protocol::{ErrorCode, ProviderConfig};

const REFERENCE: VertexImageReferenceV1 = VertexImageReferenceV1;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest_of(package: &str) -> ComponentManifestV1 {
    let path = root().join("../../components").join(package).join("manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn manifest() -> ComponentManifestV1 {
    manifest_of("image-vertex")
}

fn run(component: &dyn ImageComponentV1) -> ReportV1 {
    let manifest = manifest();
    let pack = ImageFixturePackV1::load(&root().join(&manifest.conformance.fixtures)).unwrap();
    run_image_component_suite_v1(component, &pack, &manifest)
}

fn failed(report: &ReportV1) -> BTreeSet<(CheckV1, String)> {
    report.failures().map(|outcome| (outcome.check, outcome.case.clone())).collect()
}

#[test]
fn the_shipped_manifest_passes_gate_one() {
    let manifest = manifest();
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    assert!(reported_identity_matches(&REFERENCE.metadata(), &manifest));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let report = run(&REFERENCE);
    let failures: Vec<String> = report
        .failures()
        .map(|outcome| format!("{} {}: {}", outcome.check.as_str(), outcome.case, outcome.detail()))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
    let ran: BTreeSet<CheckV1> = report.outcomes().iter().map(|outcome| outcome.check).collect();
    for check in [
        CheckV1::Coverage,
        CheckV1::FixtureMatch,
        CheckV1::UndeclaredValuesIgnored,
        CheckV1::PreDispatchRefusal,
        CheckV1::AuthErrorsAreNotRetriable,
        CheckV1::MissingMeterIsNotZero,
        CheckV1::CredentialRecipeMatch,
    ] {
        assert!(ran.contains(&check), "{check} never ran");
    }
}

/// §4: the config keys and the `credentials` section are `embeddings-vertex`'s, so one provider row
/// may route models to both packages and one credential's recipe fields serve both.
#[test]
fn the_values_and_the_recipe_are_the_embeddings_vertex_ones() {
    let (image, embeddings) = (manifest(), manifest_of("embeddings-vertex"));
    assert_eq!(image.providers.iter().collect::<Vec<_>>(), ["vertex-ai-image"]);
    assert_eq!(image.config_schema["vertex-ai-image"], embeddings.config_schema["vertex-ai"]);
    assert_eq!(image.credentials, embeddings.credentials);
    assert_eq!(
        image.declared_keys("vertex-ai-image"),
        BTreeSet::from(["project", "project_id", "region"])
    );
    assert_eq!(image.auth_arms.iter().collect::<Vec<_>>(), ["bearer"]);
    assert_eq!(image.capabilities.iter().collect::<Vec<_>>(), ["generate"]);
    let credentials = image.credentials.as_ref().unwrap();
    assert_eq!(credentials.endpoints(), ["https://oauth2.googleapis.com/token"]);
}

fn config(declared: &Value) -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "vertex-ai-image",
        "base_url": "https://us-central1-aiplatform.googleapis.com",
        "auth": "provider_api_key",
        "declared": declared,
    }))
    .unwrap()
}

fn located() -> Value {
    json!({"region": "us-central1", "project_id": "test-project"})
}

fn context(model: &str, forms: &[&str]) -> ImageCallContextV1 {
    serde_json::from_value(json!({"upstream_model": model, "metering_required": forms})).unwrap()
}

fn prepare(declared: &Value, model: &str, request: &Value) -> Result<Value, Value> {
    REFERENCE
        .prepare(&config(declared), request, &context(model, &["tokens"]))
        .map(|prepared| serde_json::from_str(&prepared.to_json()).unwrap())
        .map_err(|error| serde_json::to_value(error).unwrap())
}

/// §3: the route is relative to the base URL, each of `project`, `region` and `model` one
/// percent-encoded segment, and the auth is `bearer` on the configured slot with no other header.
#[test]
fn the_route_encodes_each_segment_and_presents_the_minted_slot_as_bearer() {
    let prepared = prepare(
        &json!({"region": "us-central1", "project_id": "test-project"}),
        "gemini-3.1-flash-image",
        &json!({"json": {"prompt": "a cat"}}),
    )
    .unwrap();
    assert_eq!(
        prepared["descriptor"]["path"],
        "v1/projects/test-project/locations/us-central1/publishers/google/models/\
         gemini-3.1-flash-image:generateContent"
    );
    assert_eq!(
        prepared["descriptor"]["auth"],
        json!({"arm": "bearer", "slot": "provider_api_key"})
    );
    assert!(prepared["descriptor"].get("headers").is_none(), "{prepared}");
    let odd = prepare(
        &json!({"region": "us-central1", "project_id": "a/b"}),
        "x/y?z",
        &json!({"json": {"prompt": "a cat"}}),
    )
    .unwrap();
    assert_eq!(
        odd["descriptor"]["path"],
        "v1/projects/a%2Fb/locations/us-central1/publishers/google/models/x%2Fy%3Fz:\
         generateContent"
    );
    // A dot segment hidden in an escape never reaches a descriptor (the relative-path contract).
    let dotted = prepare(&located(), "x/../y", &json!({"json": {"prompt": "a cat"}})).unwrap_err();
    assert_eq!(dotted["http_status"], 500, "{dotted}");
}

/// The configuration refusals of §4, as `(name, base_url, declared, the message)`.
///
/// A missing `region`, a missing project and a Vertex AI base URL of another region are 500s
/// naming the value, so they live here and not in the gate ② pack, whose `pre_dispatch_refusal`
/// row admits only `invalid_request` and `capability` from `prepare` (record Implementation,
/// departure 1); the sandbox parity test runs the same rows through the guest.
const CONFIGURATION_REFUSALS: [(&str, &str, &str, &str); 4] = [
    (
        "missing-region",
        "https://us-central1-aiplatform.googleapis.com",
        r#"{"project_id": "test-project"}"#,
        "the provider configuration carries no `region` for Vertex AI",
    ),
    (
        "missing-project",
        "https://us-central1-aiplatform.googleapis.com",
        r#"{"region": "us-central1"}"#,
        "the provider configuration carries no `project_id` for Vertex AI",
    ),
    (
        "region-mismatch",
        "https://us-central1-aiplatform.googleapis.com",
        r#"{"project_id": "test-project", "region": "europe-west4"}"#,
        "the base URL https://us-central1-aiplatform.googleapis.com is not the Vertex AI endpoint \
         of location `europe-west4`, which is https://europe-west4-aiplatform.googleapis.com",
    ),
    (
        "global-prefixed-host",
        "https://global-aiplatform.googleapis.com",
        r#"{"project_id": "test-project", "region": "global"}"#,
        "the base URL https://global-aiplatform.googleapis.com is not the Vertex AI endpoint of \
         location `global`, which is https://aiplatform.googleapis.com",
    ),
];

/// The configuration rows a component gets wrong, by name.
fn configuration_departures(component: &dyn ImageComponentV1) -> Vec<&'static str> {
    // An `n` of zero with no prompt, which a configured row refuses with 400: the configuration
    // error must be reported first.
    let no_prompt = json!({"json": {"n": 0}});
    CONFIGURATION_REFUSALS
        .into_iter()
        .filter(|(_, base_url, declared, message)| {
            let config: ProviderConfig = serde_json::from_value(json!({
                "provider": "vertex-ai-image",
                "base_url": base_url,
                "auth": "provider_api_key",
                "declared": serde_json::from_str::<Value>(declared).unwrap(),
            }))
            .unwrap();
            !component
                .prepare(&config, &no_prompt, &context("gemini-3.1-flash-image", &["tokens"]))
                .is_err_and(|error| {
                    error.code == ErrorCode::Internal
                        && error.http_status == 500
                        && error.message == *message
                })
        })
        .map(|(name, ..)| name)
        .collect()
}

/// §4: a missing `region` or project, and a Vertex AI base URL the region does not select, are a
/// 500 naming the value, reported before any refusal of the request itself.
#[test]
fn a_configuration_error_is_a_500_before_any_request_refusal() {
    assert_eq!(configuration_departures(&REFERENCE), Vec::<&str>::new());
    let error =
        prepare(&located(), "gemini-3.1-flash-image", &json!({"json": {"n": 0}})).unwrap_err();
    assert_eq!(error["http_status"], 400, "{error}");
}

/// §3, decision 5: Vertex is not served for edits. Capabilities list `generate` and `image_url`
/// only, and a multipart request is refused with 400 before dispatch.
#[test]
fn edit_is_not_served() {
    let mut config = config(&located());
    config.models = serde_json::from_value(json!([{"model": "gemini-3.1-flash-image"}])).unwrap();
    let capabilities = REFERENCE.model_capabilities(&config).unwrap();
    assert_eq!(capabilities[0].operations, [ImageOperationV1::Generate]);
    assert_eq!(
        capabilities[0].input_roles.iter().map(|role| role.key.as_str()).collect::<Vec<_>>(),
        ["image_url"]
    );
    let error = prepare(
        &located(),
        "gemini-3.1-flash-image",
        &json!({"multipart": {"parts": [
            {"kind": "text", "name": "prompt", "value": "x"},
            {"kind": "file", "name": "image", "blob": "b0", "bytes": 4},
        ]}}),
    )
    .unwrap_err();
    assert_eq!(error["code"], json!(ErrorCode::InvalidRequest));
    assert_eq!(error["http_status"], 400);
}

/// Everything but the path and the auth is the shared Gemini module (§3, §5): for the same
/// generation request both packages build the same body, facts, `repeat`, paths and state, and
/// read the same answers the same way.
#[test]
fn everything_but_the_route_is_the_gemini_package() {
    let gemini: ProviderConfig = serde_json::from_value(json!({
        "provider": "gemini-image",
        "base_url": "https://generativelanguage.googleapis.com",
        "auth": "provider_api_key",
    }))
    .unwrap();
    let url = "data:image/jpeg;base64,/9j/4AAQSkZJRg==";
    for (body, forms) in [
        (json!({"prompt": "a cat"}), vec!["tokens"]),
        (json!({"prompt": "a cat", "n": 4, "aspect_ratio": "16:9"}), vec!["tokens"]),
        (json!({"prompt": "a cat", "resolution": "2k"}), vec!["images"]),
        (json!({"prompt": "a cat", "resolution": "2k"}), vec!["tokens", "images"]),
        (
            json!({"prompt": "x", "image_url": {"$south.blob": {"id": "b0", "bytes": url.len(), "head": url}}}),
            vec!["tokens"],
        ),
        (json!({"prompt": "x", "image_url": "https://example.test/a.png"}), vec!["tokens"]),
        (json!({"prompt": "x", "n": 11}), vec!["tokens"]),
    ] {
        let request = json!({"json": body});
        let context = context("gemini-3.1-flash-image", &forms);
        let strip = |prepared: ComponentResultV1<PreparedImageCallV1>| {
            prepared
                .map(|prepared| {
                    let mut value: Value = serde_json::from_str(&prepared.to_json()).unwrap();
                    value["descriptor"].as_object_mut().unwrap().remove("path");
                    value["descriptor"].as_object_mut().unwrap().remove("auth");
                    value
                })
                .map_err(|error| serde_json::to_value(error).unwrap())
        };
        assert_eq!(
            strip(REFERENCE.prepare(&config(&located()), &request, &context)),
            strip(GeminiImageReferenceV1.prepare(&gemini, &request, &context)),
            "{request}"
        );
    }
    let state = json!({"metering_required": ["tokens"]});
    for (status, body) in [
        (
            200,
            json!({"candidates": [{"content": {"parts": [{"inlineData": {"mimeType": "image/png", "data": "aGk="}}]}}],
                   "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1, "trafficType": "ON_DEMAND"}}),
        ),
        (200, json!({"promptFeedback": {"blockReason": "SAFETY"}})),
        (403, json!({"error": {"code": 403, "message": "denied", "status": "PERMISSION_DENIED"}})),
    ] {
        let view = json!({"status": status, "headers": [], "body": {"json": body}});
        assert_eq!(
            REFERENCE.parse_response(&state, &view),
            GeminiImageReferenceV1.parse_response(&state, &view)
        );
    }
}

/// How a broken Vertex component departs from the reference.
#[derive(Clone, Copy)]
enum Fault {
    /// Reads the exported `project_id` even when the operator set the `project` override.
    IgnoresTheOverride,
    /// Accepts a Vertex AI base URL of another region.
    AcceptsAnyVertexBase,
}

struct Broken(Fault);

impl ImageComponentV1 for Broken {
    fn metadata(&self) -> ComponentMetadataV1 {
        REFERENCE.metadata()
    }
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
        REFERENCE.model_capabilities(config)
    }
    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &ImageCallContextV1,
    ) -> ComponentResultV1<PreparedImageCallV1> {
        let mut config = config.clone();
        match self.0 {
            Fault::IgnoresTheOverride => {
                let mut declared: serde_json::Map<String, Value> =
                    serde_json::from_value(serde_json::to_value(&config.declared).unwrap())
                        .unwrap();
                declared.remove("project");
                config.declared = serde_json::from_value(Value::Object(declared)).unwrap();
            }
            Fault::AcceptsAnyVertexBase => {
                if config.base_url.as_str().ends_with("aiplatform.googleapis.com") {
                    let mut value = serde_json::to_value(&config).unwrap();
                    value["base_url"] = json!("https://vertex-proxy.example.test");
                    config = serde_json::from_value(value).unwrap();
                }
            }
        }
        REFERENCE.prepare(&config, request, context)
    }
    fn parse_response(&self, state: &Value, response: &Value) -> ComponentResultV1<ImageOutcomeV1> {
        REFERENCE.parse_response(state, response)
    }
    fn render(
        &self,
        state: &Value,
        outcomes: &[ImageOutcomeV1],
        context: &ImageRenderContextV1,
    ) -> ComponentResultV1<ImageRenderedV1> {
        REFERENCE.render(state, outcomes, context)
    }
}

#[test]
fn ignoring_the_project_override_fails_its_row() {
    let report = run(&Broken(Fault::IgnoresTheOverride));
    assert!(
        failed(&report)
            .contains(&(CheckV1::FixtureMatch, "image-v1.prepare.project-override".to_owned())),
        "{report}"
    );
}

#[test]
fn accepting_a_vertex_base_of_another_region_fails_its_configuration_rows() {
    assert_eq!(
        configuration_departures(&Broken(Fault::AcceptsAnyVertexBase)),
        ["region-mismatch", "global-prefixed-host"]
    );
}
