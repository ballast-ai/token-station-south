//! Gates ① and ② for `provider-vertex` (Gemini models on Vertex AI for text), run against its
//! native reference over its own frozen fixture pack (credential recipe cases included), and the
//! behaviours of design record `docs/design/2026-10-15-provider-vertex.md` §3 and §4 pinned by
//! name.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::reference_gemini::GeminiReferenceV1;
use south_component_conformance::reference_vertex::VertexReferenceV1;
use south_component_conformance::{
    CheckV1, ComponentResultV1, FixturePackV1, PROVIDER_COMPONENT_SUITE_V1, ProviderComponentV1,
    ProviderFamilyV1, ReportV1, StreamParserV1, accepts_manifest, reported_identity_matches,
    run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::{
    COMPONENT_BEHAVIOR_SUITE, CompatibilityDeclarationV1, ComponentManifestV1, ComponentMetadataV1,
    ComponentPermissionsV1, ConformanceSpecV1, ModelLocationV1, NorthProtocolV1, PROVIDER_WORLD,
    StreamLocationV1, WIT_PACKAGE, compatibility_admits,
};
use token_station_protocol::{
    ChatRequest, ChatResponse, ErrorCode, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts,
    ModelCapability, ProviderConfig,
};

#[path = "support/host_range.rs"]
mod host_range;

const REFERENCE: VertexReferenceV1 = VertexReferenceV1;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest_source(package: &str) -> String {
    std::fs::read_to_string(root().join("../../components").join(package).join("manifest.json"))
        .expect("the shipped manifest reads")
}

fn manifest_of(package: &str) -> ComponentManifestV1 {
    serde_json::from_str(&manifest_source(package)).expect("the shipped manifest parses")
}

fn manifest() -> ComponentManifestV1 {
    manifest_of("provider-vertex")
}

fn pack(directory: &str) -> FixturePackV1 {
    FixturePackV1::load(&root().join(directory)).expect("the fixture pack loads")
}

fn run(component: &dyn ProviderComponentV1) -> ReportV1 {
    let manifest = manifest();
    run_provider_component_suite_v1_for_manifest(
        component,
        &pack(&manifest.conformance.fixtures),
        &manifest,
    )
}

fn failed(report: &ReportV1) -> BTreeSet<(CheckV1, String)> {
    report.failures().map(|outcome| (outcome.check, outcome.case.clone())).collect()
}

/// Gate ①: the package is admissible, its reported identity is the manifest's, and a host linking
/// this release admits it.
#[test]
fn gate_one_admits_the_shipped_package_and_its_reported_identity() {
    let manifest = manifest();
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    assert!(reported_identity_matches(&REFERENCE.metadata(), &manifest));
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
    assert_eq!(manifest.conformance.required_suite, PROVIDER_COMPONENT_SUITE_V1);
}

/// §3: one family, `bearer` only, `provider-gemini`'s capabilities, request facts on the
/// `:generateContent` wire with the model where the Vertex path places it, and both pass-through
/// declarations (D7).
#[test]
fn the_manifest_declares_exactly_what_the_record_decided() {
    let manifest = manifest();
    assert_eq!(manifest.providers, ["vertex-ai"]);
    assert_eq!(manifest.auth_arms, BTreeSet::from(["bearer".to_owned()]));
    assert_eq!(
        manifest.capabilities,
        BTreeSet::from(["chat".to_owned(), "stream".to_owned(), "tool_call".to_owned()]),
        "provider-gemini's capabilities; no count_tokens (D7 Q4)"
    );
    let facts = manifest.request_facts_for("vertex-ai");
    assert_eq!(facts.output_cap, ["/generationConfig/maxOutputTokens"]);
    assert_eq!(facts.model, ModelLocationV1::Url("/publishers/google/models/{model}:".to_owned()));
    assert_eq!(facts.stream, StreamLocationV1::Url);
    let gemini = manifest_of("provider-gemini");
    for (declared, by_gemini) in [
        (&manifest.north_passthrough, &gemini.north_passthrough),
        (&manifest.north_request_passthrough, &gemini.north_request_passthrough),
    ] {
        assert_eq!(declared.get("vertex-ai"), Some(&NorthProtocolV1::GenerateContent));
        assert_eq!(declared.get("vertex-ai"), by_gemini.get("gemini"));
        assert_eq!(declared.len(), 1);
    }
    assert!(manifest.endpoint.is_empty(), "the origin follows the region, so none is declared");
    assert_eq!(
        manifest.permissions,
        ComponentPermissionsV1 {
            network: false,
            filesystem: false,
            secrets: vec!["provider_api_key".to_owned()],
        },
    );
    assert_eq!(
        manifest.conformance,
        ConformanceSpecV1 {
            required_suite: COMPONENT_BEHAVIOR_SUITE.to_owned(),
            fixtures: "fixtures-vertex/".to_owned(),
        },
    );
    assert_eq!(manifest.api_version, PROVIDER_WORLD);
    assert_eq!(
        manifest.declared_keys("vertex-ai"),
        BTreeSet::from(["project", "project_id", "region"])
    );
    let credentials = manifest.credentials.as_ref().expect("the vertex_sa recipe");
    assert_eq!(credentials.endpoints(), ["https://oauth2.googleapis.com/token"]);
    host_range::assert_released_runtime(&manifest);
    assert_eq!(
        manifest.compatibility,
        CompatibilityDeclarationV1 {
            ir_schema_id: "token-station-protocol@0.6.0/v0.5.0".to_owned(),
            kernel_version: "0.5.0".to_owned(),
            kernel_revision: "c0bca10c2d9f9a439680865feb65c9edf598fc65".to_owned(),
            wit_package: WIT_PACKAGE.to_owned(),
            // `model_parameters` (record `2026-10-18-family-model-parameters.md` §7) is the newest
            // declaration, first parsed by 0.74.0.
            south_runtime: "0.74.0".to_owned(),
            runtime_abi: Some(south_provider_api::RUNTIME_ABI),
            kernel_contracts: std::collections::BTreeMap::from([
                ("canonical_ir".to_owned(), 4),
                ("error_catalog".to_owned(), 1),
                ("stream".to_owned(), 2),
            ]),
            contracts: std::collections::BTreeMap::new(),
        },
    );
}

/// The text of the top-level member `key` of a manifest, from its opening quote to its closing
/// brace: what "byte for byte" compares.
fn member_text<'a>(source: &'a str, key: &str) -> &'a str {
    let start = source.find(&format!("\n  \"{key}\": {{")).expect("the member exists") + 1;
    let open = start + source[start..].find('{').expect("an object");
    let mut depth = 0_usize;
    for (offset, byte) in source[open..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[start..=open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("`{key}` never closes")
}

/// §3: the configuration keys and the `credentials` section are `image-vertex`'s byte for byte
/// (only the family key differs), so one provider row and one credential's recipe fields serve
/// every Vertex package.
#[test]
fn the_values_and_the_recipe_are_the_image_vertex_ones_byte_for_byte() {
    let (text, image) = (manifest_source("provider-vertex"), manifest_source("image-vertex"));
    assert_eq!(member_text(&text, "credentials"), member_text(&image, "credentials"));
    assert_eq!(
        member_text(&text, "config_schema").replacen("\"vertex-ai\"", "\"vertex-ai-image\"", 1),
        member_text(&image, "config_schema")
    );
    let (manifest, image) = (manifest(), manifest_of("image-vertex"));
    assert_eq!(manifest.credentials, image.credentials);
    assert_eq!(manifest.config_schema["vertex-ai"], image.config_schema["vertex-ai-image"]);
}

/// Gate ②: the reference passes its own frozen pack, and every check the package's declarations
/// call for ran.
#[test]
fn gate_two_passes_over_the_shipped_pack() {
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
        CheckV1::EndpointConfinement,
        CheckV1::DescriptorAuthWithinManifest,
        CheckV1::RequestFactsHonoured,
        CheckV1::DescriptorIndependentOfBody,
        CheckV1::UndeclaredValuesIgnored,
        CheckV1::AuthErrorsAreNotRetriable,
        CheckV1::UsageNeverDefaulted,
        CheckV1::CredentialRecipeMatch,
    ] {
        assert!(ran.contains(&check), "{check} never ran");
    }
}

/// The pack keeps a case for every behaviour the record decided.
#[test]
fn the_shipped_pack_still_carries_every_decided_behaviour() {
    let pack = pack("fixtures-vertex");
    let names: BTreeSet<&str> = pack.cases().iter().map(|case| case.name.as_str()).collect();
    for required in [
        // §3 route: the Vertex path, its streaming operation and an encoded model segment.
        "provider.request.chat",
        "provider.request.streaming-selects-a-different-operation",
        "provider.request.streaming-proxy-base-keeps-the-query",
        "provider.request.model-id-with-a-slash-stays-one-segment",
        // §3 values: the location rules (I3-3 §4).
        "provider.request.global-location",
        "provider.request.regional-location",
        "provider.request.proxy-base",
        "provider.request.project-override",
        "provider.request.exported-project",
        "provider.request.another-regions-vertex-host-is-refused",
        // §3 shared: the request body, refusals and readers are provider-gemini's.
        "provider.request.tool-round-trip-keys-the-result-by-name",
        "provider.request.refused-openai-reasoning-replay",
        "provider.response.thinking-exhausted-bills-the-thoughts-as-output",
        "provider.stream.thinking-exhausted-single-terminal-frame",
        // SF27, which was found on Vertex.
        "provider.stream.vertex-intermediate-usage-metadata-carries-no-counts",
        "provider.stream.intermediate-counts-are-progress-not-usage",
        "provider.error.rejected-credential",
    ] {
        assert!(names.contains(required), "the pack lost `{required}`");
    }
    assert_eq!(pack.credentials().cases().len(), 5, "image-vertex's five credential cases");
}

/// The five `credential.*` cases are `fixtures-image-vertex/`'s byte for byte: the two packages
/// declare the same `credentials` section, so the same samples judge both.
#[test]
fn the_credential_cases_are_image_vertex_ones_byte_for_byte() {
    let mut copied = 0;
    for entry in std::fs::read_dir(root().join("fixtures-image-vertex")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().and_then(|name| name.to_str()).unwrap().to_owned();
        if !name.starts_with("credential.") {
            continue;
        }
        assert_eq!(
            std::fs::read(root().join("fixtures-vertex").join(&name)).unwrap(),
            std::fs::read(&path).unwrap(),
            "{name}"
        );
        copied += 1;
    }
    assert_eq!(copied, 10);
}

fn config(provider: &str, base_url: &str, declared: &Value) -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": provider,
        "base_url": base_url,
        "auth": "provider_api_key",
        "declared": declared,
    }))
    .unwrap()
}

fn chat(value: Value) -> ChatRequest {
    serde_json::from_value(value).unwrap()
}

/// The configuration refusals of record §3 and §4 (P2, P3), as `(name, base_url, declared, the
/// message)`: 500s naming the value, as `image-vertex` answers. The region mismatch is also a pack
/// row; all four live here, where a request the component would otherwise refuse checks that the
/// configuration error comes first, and the sandbox parity test runs them through the guest.
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

/// A request a configured row refuses with 400 on two counts: no model, and `OpenAI` reasoning
/// replay this wire cannot carry.
fn refused_request() -> ChatRequest {
    chat(json!({
        "model": "",
        "messages": [{"role": "assistant", "content": "x",
                      "reasoning_replay_protocol_family": "openai-encrypted-reasoning"}],
    }))
}

/// The configuration rows a component gets wrong, by name.
fn configuration_departures(component: &dyn ProviderComponentV1) -> Vec<&'static str> {
    CONFIGURATION_REFUSALS
        .into_iter()
        .filter(|(_, base_url, declared, message)| {
            let config = config("vertex-ai", base_url, &serde_json::from_str(declared).unwrap());
            !component.build_http_request(&refused_request(), &config).is_err_and(|error| {
                error.code == ErrorCode::Internal
                    && error.http_status == 500
                    && error.message == *message
            })
        })
        .map(|(name, ..)| name)
        .collect()
}

/// §4 P2, P3: a missing `region` or project, and a Vertex AI base URL the region does not select,
/// are a 500 naming the value, reported before any refusal of the request itself.
#[test]
fn a_configuration_error_is_a_500_before_any_request_refusal() {
    assert_eq!(configuration_departures(&REFERENCE), Vec::<&str>::new());
    let configured = config(
        "vertex-ai",
        "https://us-central1-aiplatform.googleapis.com",
        &json!({"region": "us-central1", "project_id": "test-project"}),
    );
    let error = REFERENCE.build_http_request(&refused_request(), &configured).unwrap_err();
    assert_eq!((error.code, error.http_status), (ErrorCode::Capability, 400), "{error:?}");
}

/// Each package serves its own family and refuses the other's (the per-package family check that
/// replaced `provider-gemini`'s fixed `config.provider != "gemini"`).
#[test]
fn each_package_refuses_the_other_family() {
    let request = chat(json!({"model": "gemini-2.5-flash",
                              "messages": [{"role": "user", "content": "hi"}]}));
    let located = json!({"region": "us-central1", "project_id": "test-project"});
    let vertex_base = "https://us-central1-aiplatform.googleapis.com";
    let gemini_base = "https://generativelanguage.googleapis.com";
    let (vertex, gemini): (&dyn ProviderComponentV1, &dyn ProviderComponentV1) =
        (&REFERENCE, &GeminiReferenceV1);
    for (component, config, family) in [
        (vertex, config("gemini", vertex_base, &located), "gemini"),
        (gemini, config("vertex-ai", gemini_base, &located), "vertex-ai"),
        (vertex, config("vertex-ai-image", vertex_base, &located), "vertex-ai-image"),
    ] {
        let error = component.build_http_request(&request, &config).unwrap_err();
        assert_eq!((error.code, error.http_status), (ErrorCode::Capability, 400));
        assert_eq!(error.message, format!("unsupported provider dialect `{family}`"));
    }
    assert!(
        REFERENCE.build_http_request(&request, &config("vertex-ai", vertex_base, &located)).is_ok()
    );
    assert!(
        GeminiReferenceV1
            .build_http_request(&request, &config("gemini", gemini_base, &json!({})))
            .is_ok()
    );
}

fn as_json<T: serde::Serialize>(result: ComponentResultV1<T>) -> Result<Value, Value> {
    result
        .map(|value| serde_json::to_value(value).unwrap())
        .map_err(|error| serde_json::to_value(error).unwrap())
}

fn streamed(component: &dyn ProviderComponentV1, input: &Value) -> Result<Value, Value> {
    let mut parser = component.stream_parser();
    let mut events = Vec::new();
    for chunk in input["chunks"].as_array().unwrap() {
        match parser.parse_chunk(chunk.as_str().unwrap().as_bytes()) {
            Ok(more) => events.extend(more),
            Err(error) => return as_json::<()>(Err(error)),
        }
    }
    match parser.finish() {
        Ok(more) => events.extend(more),
        Err(error) => return as_json::<()>(Err(error)),
    }
    as_json(Ok(events))
}

/// §3: everything but the route and the auth is `provider-gemini`'s. Over every case of
/// `fixtures-gemini/` (P6), the two references build the same method, headers and body (or the same
/// refusal) for the same IR, and read every response, stream and error the same way.
#[test]
fn everything_but_the_route_and_the_auth_is_the_gemini_package() {
    let vertex = config(
        "vertex-ai",
        "https://us-central1-aiplatform.googleapis.com",
        &json!({"region": "us-central1", "project_id": "test-project"}),
    );
    let mut compared = [0_usize; 4];
    for case in pack("fixtures-gemini").cases() {
        match case.family {
            ProviderFamilyV1::Request => {
                let gemini: ProviderConfig =
                    serde_json::from_value(case.input["provider_config"].clone()).unwrap();
                let request: ChatRequest =
                    serde_json::from_value(case.input["chat_request"].clone()).unwrap();
                let strip = |built: ComponentResultV1<HttpRequestDescriptor>| {
                    as_json(built).map(|mut value| {
                        let map = value.as_object_mut().unwrap();
                        map.remove("url");
                        map.remove("auth");
                        value
                    })
                };
                assert_eq!(
                    strip(REFERENCE.build_http_request(&request, &vertex)),
                    strip(GeminiReferenceV1.build_http_request(&request, &gemini)),
                    "{}",
                    case.name
                );
                compared[0] += 1;
            }
            ProviderFamilyV1::Response => {
                let parts: HttpResponseParts = serde_json::from_value(case.input.clone()).unwrap();
                assert_eq!(
                    as_json::<ChatResponse>(REFERENCE.parse_response(&parts)),
                    as_json(GeminiReferenceV1.parse_response(&parts)),
                    "{}",
                    case.name
                );
                compared[1] += 1;
            }
            ProviderFamilyV1::Stream => {
                assert_eq!(
                    streamed(&REFERENCE, &case.input),
                    streamed(&GeminiReferenceV1, &case.input),
                    "{}",
                    case.name
                );
                compared[2] += 1;
            }
            ProviderFamilyV1::Error => {
                let parts: HttpResponseParts = serde_json::from_value(case.input.clone()).unwrap();
                assert_eq!(
                    as_json::<ErrorEnvelope>(REFERENCE.map_provider_error(&parts)),
                    as_json(GeminiReferenceV1.map_provider_error(&parts)),
                    "{}",
                    case.name
                );
                compared[3] += 1;
            }
            ProviderFamilyV1::Capabilities => {}
        }
    }
    assert!(compared.iter().all(|count| *count > 0), "{compared:?}");
}

/// §3 route: each of `project`, `region` and `model` is one percent-encoded segment (P4), and the
/// auth is `bearer` on the configured slot with no other header.
#[test]
fn the_route_encodes_each_segment_and_presents_the_minted_slot_as_bearer() {
    let built = REFERENCE
        .build_http_request(
            &chat(json!({"model": "x/y?z", "messages": [{"role": "user", "content": "hi"}]})),
            &config(
                "vertex-ai",
                "https://us-central1-aiplatform.googleapis.com/",
                &json!({"region": "us-central1", "project_id": "a/b"}),
            ),
        )
        .unwrap();
    let built = serde_json::to_value(built).unwrap();
    assert_eq!(
        built["url"],
        "https://us-central1-aiplatform.googleapis.com/v1/projects/a%2Fb/locations/us-central1/\
         publishers/google/models/x%2Fy%3Fz:generateContent"
    );
    assert_eq!(built["auth"], json!({"scheme": "bearer", "secret": "provider_api_key"}));
    assert_eq!(built["headers"], json!({"content-type": "application/json"}));
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

impl ProviderComponentV1 for Broken {
    fn metadata(&self) -> ComponentMetadataV1 {
        REFERENCE.metadata()
    }
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ModelCapability>> {
        REFERENCE.model_capabilities(config)
    }
    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        let mut value = serde_json::to_value(config).unwrap();
        match self.0 {
            Fault::IgnoresTheOverride => {
                if let Some(declared) = value["declared"].as_object_mut() {
                    declared.remove("project");
                }
            }
            Fault::AcceptsAnyVertexBase => {
                // Builds under the region's own origin whatever Vertex AI host was configured.
                let region = config.declared.get("region").unwrap_or("us-central1").to_owned();
                if config.base_url.as_str().contains("aiplatform.googleapis.com") {
                    value["base_url"] = json!(if region == "global" {
                        "https://aiplatform.googleapis.com".to_owned()
                    } else {
                        format!("https://{region}-aiplatform.googleapis.com")
                    });
                }
            }
        }
        let config: ProviderConfig = serde_json::from_value(value).unwrap();
        REFERENCE.build_http_request(request, &config)
    }
    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        REFERENCE.parse_response(parts)
    }
    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        REFERENCE.map_provider_error(parts)
    }
    fn stream_parser(&self) -> Box<dyn StreamParserV1> {
        REFERENCE.stream_parser()
    }
}

#[test]
fn ignoring_the_project_override_fails_its_row() {
    let report = run(&Broken(Fault::IgnoresTheOverride));
    assert!(
        failed(&report)
            .contains(&(CheckV1::FixtureMatch, "provider.request.project-override".to_owned())),
        "{report}"
    );
}

#[test]
fn accepting_a_vertex_base_of_another_region_fails_its_rows() {
    assert_eq!(
        configuration_departures(&Broken(Fault::AcceptsAnyVertexBase)),
        ["region-mismatch", "global-prefixed-host"]
    );
    let report = run(&Broken(Fault::AcceptsAnyVertexBase));
    assert!(
        failed(&report).contains(&(
            CheckV1::FixtureMatch,
            "provider.request.another-regions-vertex-host-is-refused".to_owned()
        )),
        "{report}"
    );
}
