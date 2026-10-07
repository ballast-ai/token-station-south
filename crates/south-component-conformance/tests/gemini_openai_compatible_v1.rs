//! The `gemini-openai-compatible` family of `provider-openai-compatible` (B7b,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §13.7 item 6 and §16 Q38).
//!
//! Gemini's OpenAI-compatible surface takes the `OpenAI` Chat Completions body and wants the key
//! twice, as `Authorization: Bearer` and in `x-goog-api-key`. The family reuses the package's
//! translation unchanged and differs in two declarations: the combined auth arm in its descriptor
//! and the endpoint template in its manifest.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::{
    AdmittedAuthV1, CheckV1, FixturePackV1, ProviderComponentV1, admit_descriptor_auth,
    run_provider_component_suite_v1_for_manifest,
};
use south_contracts::SecretHeaderV1;
use south_provider_api::{ComponentManifestV1, ConfigErrorV1};
use token_station_protocol::{
    Auth, ChatRequest, DescriptorError, HttpRequestDescriptor, ProviderConfig,
};

const FAMILY: &str = "gemini-openai-compatible";
const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/openai";
const FIXTURES: [&str; 2] = [
    "provider.request.gemini-openai-compatible",
    "provider.request.gemini-openai-compatible-stream",
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn shipped_manifest() -> ComponentManifestV1 {
    let path = root().join("../../components/provider-openai-compatible/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("the manifest reads"))
        .expect("the manifest parses")
}

fn fixture_input(case: &str) -> Value {
    let path = root().join("fixtures").join(format!("{case}.input.json"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture reads"))
        .expect("the fixture parses")
}

/// The configuration and request of a shipped request fixture, and the descriptor the reference
/// builds from them.
fn built(case: &str) -> (ProviderConfig, ChatRequest, HttpRequestDescriptor) {
    let input = fixture_input(case);
    let config: ProviderConfig = serde_json::from_value(input["provider_config"].clone()).unwrap();
    let request: ChatRequest = serde_json::from_value(input["chat_request"].clone()).unwrap();
    let descriptor = OpenAiCompatibleReferenceV1
        .build_http_request(&request, &config)
        .unwrap_or_else(|error| panic!("{case}: the reference refused its own fixture: {error:?}"));
    (config, request, descriptor)
}

fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
}

/// (a) The reference presents the pair, and the shipped manifest admits exactly that.
#[test]
fn the_reference_presents_the_combined_arm_and_the_shipped_manifest_admits_it() {
    let manifest = shipped_manifest();
    for case in FIXTURES {
        let (config, _, descriptor) = built(case);
        let slot = config.auth.clone().expect("the fixture configures a slot");
        assert_eq!(
            descriptor.auth,
            Some(Auth::bearer_and_header("x-goog-api-key", slot).unwrap()),
            "{case}"
        );
        assert_eq!(
            descriptor.url,
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
            "{case}"
        );
        assert_eq!(
            admit_descriptor_auth(&manifest, &config, &descriptor),
            Ok(AdmittedAuthV1::BearerAndHeaderSecret(SecretHeaderV1::XGoogApiKey)),
            "{case}"
        );
    }
}

/// An upstream configured without a slot gets a descriptor without auth, as Azure's does.
#[test]
fn a_family_configured_without_a_credential_presents_no_auth() {
    let (mut config, request, _) = built(FIXTURES[0]);
    config.auth = None;
    let descriptor = OpenAiCompatibleReferenceV1.build_http_request(&request, &config).unwrap();
    assert_eq!(descriptor.auth, None);
    assert_eq!(
        admit_descriptor_auth(&shipped_manifest(), &config, &descriptor),
        Ok(AdmittedAuthV1::None)
    );
}

/// The family differs from `openai-compatible` in its auth and nothing else: the same request,
/// configured against the same URL, builds the same method, URL, headers and body.
#[test]
fn the_family_shares_the_openai_translation_and_differs_only_in_auth() {
    for case in FIXTURES {
        let (gemini_config, request, gemini) = built(case);
        let mut plain_config = gemini_config.clone();
        plain_config.provider = "openai-compatible".to_owned();
        let plain =
            OpenAiCompatibleReferenceV1.build_http_request(&request, &plain_config).unwrap();

        assert_eq!(gemini.method, plain.method, "{case}");
        assert_eq!(gemini.url, plain.url, "{case}");
        assert_eq!(gemini.headers, plain.headers, "{case}");
        assert_eq!(gemini.body, plain.body, "{case}");
        assert_ne!(gemini.auth, plain.auth, "{case}");
        assert!(matches!(plain.auth, Some(Auth::Bearer { .. })), "{case}");
    }
}

/// (b) The package's older families keep their own presentation.
#[test]
fn the_older_families_keep_their_auth_presentation() {
    let manifest = shipped_manifest();
    for (case, header_secret) in [
        ("provider.request.chat", None),
        ("provider.request.github-copilot", None),
        ("provider.request.azure-header-auth", Some(SecretHeaderV1::ApiKey)),
    ] {
        let (config, _, descriptor) = built(case);
        let slot = config.auth.clone().expect("the fixture configures a slot");
        let (presented, admitted) = match header_secret {
            None => (Auth::bearer(slot), AdmittedAuthV1::Bearer),
            Some(header) => (
                Auth::header(header.header_name(), slot).unwrap(),
                AdmittedAuthV1::HeaderSecret(header),
            ),
        };
        assert_eq!(descriptor.auth, Some(presented), "{case}");
        assert_eq!(admit_descriptor_auth(&manifest, &config, &descriptor), Ok(admitted), "{case}");
    }
}

/// (c) A manifest that does not declare the combined arm fails gate ② on the new fixtures, and
/// only there: the check is what ties the descriptor's presentation to the declaration.
#[test]
fn gate_two_refuses_the_family_when_the_manifest_does_not_declare_the_combined_arm() {
    let pack = FixturePackV1::load(&root().join("fixtures")).expect("the shipped pack loads");
    let mut manifest = shipped_manifest();
    assert!(manifest.auth_arms.remove("bearer_and_header_secret"));
    assert_eq!(manifest.validate(), Ok(()));

    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &pack,
        &manifest,
    );
    let failed: BTreeSet<&str> = report.failures().map(|outcome| outcome.case.as_str()).collect();
    assert_eq!(failed, BTreeSet::from(FIXTURES), "{report}");
    for failure in report.failures() {
        assert_eq!(failure.check, CheckV1::DescriptorAuthWithinManifest, "{failure}");
        assert!(failure.detail().contains("combined bearer-and-header arm"), "{failure}");
    }

    // With the arm declared, the same two cases pass both the match and the admission.
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &pack,
        &shipped_manifest(),
    );
    assert!(report.is_passing(), "{report}");
    for check in [CheckV1::FixtureMatch, CheckV1::DescriptorAuthWithinManifest] {
        for case in FIXTURES {
            assert!(
                report
                    .outcomes()
                    .iter()
                    .any(|outcome| outcome.check == check && outcome.case == case),
                "{check}: {case} never ran"
            );
        }
    }
}

/// (d) The endpoint template has no parameters, so it fills from no operator values, and the
/// descriptor built against it is authorized.
#[test]
fn the_family_endpoint_fills_without_parameters_and_authorizes_the_descriptor() {
    let manifest = shipped_manifest();
    assert_eq!(manifest.validate(), Ok(()));
    assert!(!manifest.config_schema.contains_key(FAMILY));
    assert_eq!(manifest.fill_endpoint(FAMILY, &values(&[])), Ok(Some(BASE_URL.to_owned())));
    assert_eq!(
        manifest.fill_endpoint(FAMILY, &values(&[("plan", "individual")])),
        Err(ConfigErrorV1::UnknownKey("plan".to_owned()))
    );
    assert_eq!(manifest.endpoint_values(FAMILY, BASE_URL), Ok(values(&[])));
    for url in [BASE_URL, "https://generativelanguage.googleapis.com/v1beta/openai/"] {
        assert!(manifest.endpoint_admits(FAMILY, url), "{url}");
    }
    for url in [
        "https://generativelanguage.googleapis.com",
        "https://generativelanguage.googleapis.com/v1beta",
        "https://generativelanguage.googleapis.com/v1/openai",
        "https://generativelanguage.googleapis.com.example/v1beta/openai",
        "https://example.com/v1beta/openai",
    ] {
        assert!(!manifest.endpoint_admits(FAMILY, url), "{url}");
    }

    let filled = manifest.fill_endpoint(FAMILY, &values(&[])).unwrap().unwrap();
    let (_, request, _) = built(FIXTURES[0]);
    let mut input = fixture_input(FIXTURES[0]);
    input["provider_config"]["base_url"] = json!(filled);
    let config: ProviderConfig = serde_json::from_value(input["provider_config"].clone()).unwrap();
    let descriptor = OpenAiCompatibleReferenceV1.build_http_request(&request, &config).unwrap();
    assert_eq!(config.authorize(&descriptor), Ok(()));

    // The same descriptor is refused by an upstream configured at another origin.
    input["provider_config"]["base_url"] = json!("https://example.com/v1beta/openai");
    let elsewhere: ProviderConfig =
        serde_json::from_value(input["provider_config"].clone()).unwrap();
    assert!(matches!(
        elsewhere.authorize(&descriptor),
        Err(DescriptorError::UrlOutsideEndpoint { .. })
    ));
}

/// The package declares the family and the arm, in the sorted order the file keeps, and the
/// family's endpoint beside Copilot's. (That the family declares no credential recipe, user-agent
/// or request facts is pinned beside the other families, in `component_conformance_v1` and
/// `request_facts_v1`.)
#[test]
fn the_shipped_manifest_declares_the_family_the_arm_and_the_endpoint() {
    let manifest = shipped_manifest();
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(
        manifest.providers,
        ["openai-compatible", "azure-openai-v1", "github-copilot", FAMILY]
    );
    assert_eq!(
        manifest.auth_arms,
        BTreeSet::from([
            "bearer".to_owned(),
            "bearer_and_header_secret".to_owned(),
            "header_secret".to_owned()
        ])
    );
    let source: Value = serde_json::from_str(
        &std::fs::read_to_string(
            root().join("../../components/provider-openai-compatible/manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(source["auth_arms"], json!(["bearer", "bearer_and_header_secret", "header_secret"]));

    assert_eq!(manifest.endpoint.keys().collect::<Vec<_>>(), [FAMILY, "github-copilot"]);
}
