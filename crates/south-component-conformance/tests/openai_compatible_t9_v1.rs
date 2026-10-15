//! The T9 families of `provider-openai-compatible` (design record
//! `docs/design/2026-10-15-openai-compatible-t9.md`): `glm-coding`, the Z.ai GLM Coding Plan, and
//! `azure-openai-dated`, Azure `OpenAI`'s dated, deployment-addressed surface.
//!
//! Both are transcribed from the host's native arms (record §2): the values pinned here are the
//! native constants and the native URL tests' cases, not values derived from this reference.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::reference::{
    OpenAiCompatibleReferenceV1, azure_openai_dated, glm_coding,
};
use south_component_conformance::{
    AdmittedAuthV1, CheckV1, FixturePackV1, ProviderComponentV1, admit_descriptor_auth,
    run_provider_component_suite_v1_for_manifest,
};
use south_contracts::SecretHeaderV1;
use south_provider_api::{ComponentManifestV1, ValueSyntaxV1};
use token_station_protocol::{
    Auth, ChatRequest, ErrorCode, HttpRequestDescriptor, ProviderConfig, SafeHeaders,
};

/// Native `GLM_CODING_IMPERSONATION_UA` (server `upstream.rs`), written out.
const NATIVE_GLM_CODING_UA: &str = "opencode/1.15.6";
/// Native `AZURE_OPENAI_DEFAULT_API_VERSION` (server `upstream.rs`), written out.
const NATIVE_AZURE_DEFAULT_API_VERSION: &str = "2024-10-21";

const GLM_FIXTURES: [&str; 2] =
    ["provider.request.glm-coding", "provider.request.glm-coding-stream"];
const AZURE_FIXTURES: [&str; 4] = [
    "provider.request.azure-openai-dated",
    "provider.request.azure-openai-dated-default-version",
    "provider.request.azure-openai-dated-refuses-an-undated-version",
    "provider.request.azure-openai-dated-refuses-a-request-without-a-model",
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest_path() -> std::path::PathBuf {
    root().join("../../components/provider-openai-compatible/manifest.json")
}

fn shipped_manifest() -> ComponentManifestV1 {
    serde_json::from_str(&std::fs::read_to_string(manifest_path()).expect("the manifest reads"))
        .expect("the manifest parses")
}

fn fixture_input(case: &str) -> Value {
    let path = root().join("fixtures").join(format!("{case}.input.json"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture reads"))
        .expect("the fixture parses")
}

fn parts(case: &str) -> (ProviderConfig, ChatRequest) {
    let input = fixture_input(case);
    (
        serde_json::from_value(input["provider_config"].clone()).unwrap(),
        serde_json::from_value(input["chat_request"].clone()).unwrap(),
    )
}

fn build(config: &ProviderConfig, request: &ChatRequest) -> HttpRequestDescriptor {
    OpenAiCompatibleReferenceV1
        .build_http_request(request, config)
        .unwrap_or_else(|error| panic!("the reference refused: {error:?}"))
}

fn content_type_only() -> SafeHeaders {
    SafeHeaders::try_new([("content-type", "application/json")]).unwrap()
}

fn azure(
    base_url: &str,
    api_version: Option<&str>,
    deployment: &str,
) -> (ProviderConfig, ChatRequest) {
    let (_, mut request) = parts(AZURE_FIXTURES[0]);
    let mut input = fixture_input(AZURE_FIXTURES[0]);
    input["provider_config"]["base_url"] = json!(base_url);
    match api_version {
        Some(version) => input["provider_config"]["declared"] = json!({"api_version": version}),
        None => {
            input["provider_config"].as_object_mut().unwrap().remove("declared");
        }
    }
    deployment.clone_into(&mut request.model);
    (serde_json::from_value(input["provider_config"].clone()).unwrap(), request)
}

// -- glm-coding --------------------------------------------------------------------------------

/// Record §2.1: native posts Chat Completions to `{endpoint}/api/coding/paas/v4/chat/completions`
/// with the key as `Authorization: Bearer` and no other header but the user-agent, which the
/// package declares rather than sends.
#[test]
fn glm_coding_posts_to_the_coding_plan_path_with_a_bearer_key() {
    assert_eq!(glm_coding::CHAT_PATH, "/api/coding/paas/v4/chat/completions");
    let manifest = shipped_manifest();
    for case in GLM_FIXTURES {
        let (config, request) = parts(case);
        let descriptor = build(&config, &request);
        assert_eq!(
            descriptor.url, "https://api.z.ai/api/coding/paas/v4/chat/completions",
            "{case}"
        );
        assert_eq!(descriptor.headers, content_type_only(), "{case}");
        assert_eq!(descriptor.auth, Some(Auth::bearer(config.auth.clone().unwrap())), "{case}");
        assert_eq!(
            admit_descriptor_auth(&manifest, &config, &descriptor),
            Ok(AdmittedAuthV1::Bearer),
            "{case}"
        );
        assert_eq!(config.authorize(&descriptor), Ok(()), "{case}");
    }
}

/// Record §2.1 and §3.1: the user-agent is native's `OpenCode` impersonation, declared for this
/// family only, and the family is a subscription (native `is_coding_plan`).
#[test]
fn glm_coding_declares_the_native_user_agent_and_a_subscription_quota() {
    assert_eq!(glm_coding::USER_AGENT, NATIVE_GLM_CODING_UA);
    let manifest = shipped_manifest();
    assert_eq!(
        manifest.user_agent.get(glm_coding::FAMILY).map(String::as_str),
        Some(NATIVE_GLM_CODING_UA)
    );
    let instances = south_component_conformance::DeclaredInstancesV1::from_manifest(&manifest)
        .expect("the shipped manifest passes gate ①");
    assert_eq!(
        instances.user_agent(glm_coding::FAMILY).map(south_contracts::DeclaredUserAgentV1::as_str),
        Some(NATIVE_GLM_CODING_UA)
    );
    assert_eq!(instances.user_agent("openai-compatible"), None);
    assert_eq!(
        manifest.subscription_quota_families,
        BTreeSet::from(["github-copilot".to_owned(), glm_coding::FAMILY.to_owned()])
    );
    assert_eq!(manifest.credentials_for(glm_coding::FAMILY), None);
    assert!(!manifest.endpoint.contains_key(glm_coding::FAMILY));
    assert!(!manifest.config_schema.contains_key(glm_coding::FAMILY));
}

/// Record §3.1: the family is `openai-compatible` with the Coding Plan root fixed. The same request
/// on `openai-compatible`, configured at that root, builds the same descriptor.
#[test]
fn glm_coding_shares_the_openai_compatible_translation() {
    for case in GLM_FIXTURES {
        let (config, request) = parts(case);
        let mut plain = config.clone();
        plain.provider = "openai-compatible".to_owned();
        plain.base_url =
            serde_json::from_value(json!("https://api.z.ai/api/coding/paas/v4")).unwrap();
        assert_eq!(build(&config, &request), build(&plain, &request), "{case}");
    }
}

// -- azure-openai-dated ------------------------------------------------------------------------

/// Record §2.2: native `build_upstream_url`'s dated arm, case by case from the native URL tests
/// (`azure_openai_url_*`): both hosts used verbatim, the operator's version, the default when
/// none is set, and a trailing slash on the endpoint.
#[test]
fn azure_openai_dated_builds_the_native_deployment_url() {
    for (base, version, deployment, url) in [
        (
            "https://my-resource.openai.azure.com",
            Some("2024-10-21"),
            "gpt-5-5-deployment",
            "https://my-resource.openai.azure.com/openai/deployments/gpt-5-5-deployment/chat/completions?api-version=2024-10-21",
        ),
        (
            "https://my-resource.services.ai.azure.com",
            Some("2024-10-21"),
            "my-deployment",
            "https://my-resource.services.ai.azure.com/openai/deployments/my-deployment/chat/completions?api-version=2024-10-21",
        ),
        (
            "https://x.openai.azure.com",
            Some("2025-04-01-preview"),
            "preview-model",
            "https://x.openai.azure.com/openai/deployments/preview-model/chat/completions?api-version=2025-04-01-preview",
        ),
        (
            "https://x.openai.azure.com",
            Some("2024-12-01-preview"),
            "d",
            "https://x.openai.azure.com/openai/deployments/d/chat/completions?api-version=2024-12-01-preview",
        ),
        (
            "https://x.openai.azure.com",
            None,
            "d",
            "https://x.openai.azure.com/openai/deployments/d/chat/completions?api-version=2024-10-21",
        ),
        (
            "https://x.openai.azure.com/",
            None,
            "d",
            "https://x.openai.azure.com/openai/deployments/d/chat/completions?api-version=2024-10-21",
        ),
    ] {
        let (config, request) = azure(base, version, deployment);
        let descriptor = build(&config, &request);
        assert_eq!(descriptor.url, url);
        assert_eq!(descriptor.body.as_ref().unwrap()["model"], json!(deployment), "{url}");
        assert_eq!(config.authorize(&descriptor), Ok(()), "{url}");
    }
}

/// Record §2.2: the key in `api-key`, as native's Azure arm sends it, and nothing else.
#[test]
fn azure_openai_dated_presents_the_key_in_api_key() {
    let manifest = shipped_manifest();
    for case in &AZURE_FIXTURES[..2] {
        let (config, request) = parts(case);
        let descriptor = build(&config, &request);
        let slot = config.auth.clone().unwrap();
        assert_eq!(descriptor.auth, Some(Auth::header("api-key", slot).unwrap()), "{case}");
        assert_eq!(descriptor.headers, content_type_only(), "{case}");
        assert_eq!(
            admit_descriptor_auth(&manifest, &config, &descriptor),
            Ok(AdmittedAuthV1::HeaderSecret(SecretHeaderV1::ApiKey)),
            "{case}"
        );
    }
    let (mut config, request) = parts(AZURE_FIXTURES[0]);
    config.auth = None;
    assert_eq!(build(&config, &request).auth, None);
}

/// Record §3.2: the family is `azure-openai-v1` with another route. The same request configured
/// on the v1 surface builds the same headers, body and auth.
#[test]
fn azure_openai_dated_differs_from_azure_openai_v1_only_in_the_url() {
    for case in &AZURE_FIXTURES[..2] {
        let (config, request) = parts(case);
        let dated = build(&config, &request);
        let mut v1 = config.clone();
        v1.provider = "azure-openai-v1".to_owned();
        v1.declared = token_station_protocol::ComponentValues::new();
        v1.base_url =
            serde_json::from_value(json!(format!("{}/openai/v1", config.base_url.as_str())))
                .unwrap();
        let v1 = build(&v1, &request);
        assert_eq!((&dated.headers, &dated.body, &dated.auth), (&v1.headers, &v1.body, &v1.auth));
        assert_ne!(dated.url, v1.url, "{case}");
    }
}

/// Record §3.2: the manifest's default is native's, and the key takes native's dated values only.
#[test]
fn azure_openai_dated_declares_the_native_default_version() {
    assert_eq!(azure_openai_dated::DEFAULT_API_VERSION, NATIVE_AZURE_DEFAULT_API_VERSION);
    assert_eq!(azure_openai_dated::API_VERSION_KEY, "api_version");
    let manifest = shipped_manifest();
    let key =
        &manifest.config_schema[azure_openai_dated::FAMILY][azure_openai_dated::API_VERSION_KEY];
    assert_eq!(key.syntax, ValueSyntaxV1::ApiVersionDate);
    assert!(!key.required);
    assert_eq!(key.default.as_deref(), Some(NATIVE_AZURE_DEFAULT_API_VERSION));
    assert_eq!(manifest.config_schema[azure_openai_dated::FAMILY].len(), 1);
    assert!(!manifest.endpoint.contains_key(azure_openai_dated::FAMILY));
    assert_eq!(manifest.credentials_for(azure_openai_dated::FAMILY), None);
    assert!(!manifest.subscription_quota_families.contains(azure_openai_dated::FAMILY));
    assert_eq!(manifest.user_agent.get(azure_openai_dated::FAMILY), None);
}

/// Record §4 D3: a value that is not a dated version never reaches the query, even from a host
/// that skipped the manifest's syntax check. `v1` (any case) is the other family's selector.
#[test]
fn azure_openai_dated_refuses_an_undated_version_as_a_configuration_error() {
    for version in
        ["v1", "V1", "latest", "preview", "2024-10-21&x=1", "2024-10-21-beta", "2024-1-21"]
    {
        let (config, request) = azure("https://x.openai.azure.com", Some(version), "d");
        let error = OpenAiCompatibleReferenceV1.build_http_request(&request, &config).unwrap_err();
        assert_eq!((error.code, error.http_status), (ErrorCode::Internal, 500), "{version}");
    }
    for version in ["2024-10-21", "2025-04-01-preview", "1999-99-99"] {
        let (config, request) = azure("https://x.openai.azure.com", Some(version), "d");
        assert!(build(&config, &request).url.ends_with(&format!("?api-version={version}")));
    }
}

/// Record §4 D4 and D5: the deployment is one path segment. An empty one is refused rather than
/// sent as `deployments//`; a `/` is escaped rather than splitting the path.
#[test]
fn azure_openai_dated_puts_the_deployment_in_one_segment() {
    let (config, request) = azure("https://x.openai.azure.com", None, "");
    let error = OpenAiCompatibleReferenceV1.build_http_request(&request, &config).unwrap_err();
    assert_eq!((error.code, error.http_status), (ErrorCode::Capability, 400));

    let (config, request) = azure("https://x.openai.azure.com", None, "team/gpt?x");
    let descriptor = build(&config, &request);
    assert_eq!(
        descriptor.url,
        "https://x.openai.azure.com/openai/deployments/team%2Fgpt%3Fx/chat/completions?api-version=2024-10-21"
    );
    assert_eq!(config.authorize(&descriptor), Ok(()));
}

// -- the package -------------------------------------------------------------------------------

/// The family check: a family the package does not declare is refused, so a row naming the
/// native type (`glm_coding`, `azure_openai`) or a near miss never gets another family's route.
#[test]
fn an_undeclared_family_is_refused() {
    let (config, request) = parts(GLM_FIXTURES[0]);
    for family in ["glm_coding", "glm", "azure-openai", "azure_openai", "azure-openai-dated-v1"] {
        let mut other = config.clone();
        other.provider = family.to_owned();
        let error = OpenAiCompatibleReferenceV1.build_http_request(&request, &other).unwrap_err();
        assert_eq!((error.code, error.http_status), (ErrorCode::Capability, 400), "{family}");
        assert!(error.message.contains("unsupported provider dialect"), "{family}");
    }
}

/// The package declares both families, and gate ② runs every T9 case, the refusals included.
#[test]
fn the_package_declares_the_families_and_gate_two_runs_every_case() {
    let manifest = shipped_manifest();
    assert_eq!(manifest.validate(), Ok(()));
    for family in [glm_coding::FAMILY, azure_openai_dated::FAMILY] {
        assert!(manifest.providers.iter().any(|declared| declared == family), "{family}");
    }
    let pack = FixturePackV1::load(&root().join("fixtures")).expect("the shipped pack loads");
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &pack,
        &manifest,
    );
    assert!(report.is_passing(), "{report}");
    for case in GLM_FIXTURES.iter().chain(&AZURE_FIXTURES) {
        assert!(
            report
                .outcomes()
                .iter()
                .any(|outcome| outcome.check == CheckV1::FixtureMatch && outcome.case == *case),
            "{case} never ran"
        );
    }
}

/// Record §8: a minor version in the open cycle, past the patch 2.6.10 it supersedes, reported by
/// the reference and recorded by the crate.
#[test]
fn the_t9_families_retire_the_patch_identity() {
    let manifest = shipped_manifest();
    for retired in ["2.6.9", "2.6.10"] {
        assert_ne!(manifest.version, retired);
    }
    // Published as 2.7.0 with 0.66.0; the speech Azure OpenAI families' Q47 bump made it 2.7.1.
    assert_eq!(manifest.version, "2.7.1");
    assert_eq!(OpenAiCompatibleReferenceV1.metadata().version, manifest.version);
    assert_eq!(manifest.compatibility.south_runtime, "0.60.0");
}
