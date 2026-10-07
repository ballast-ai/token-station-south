//! Issue #138 (`docs/design/2026-09-30-host-zero-vendor-boundary.md` §13.7 item 7): a model id
//! that contains `/` is encoded as one path segment, and since kernel protocol 0.5.0 the
//! kernel's endpoint check admits that spelling.
//!
//! Gate ② already runs `ProviderConfig::authorize` on every request case (`EndpointConfinement`),
//! so the closing evidence is a fixture row in each pack that puts a slashed model in the path.
//! This file pins the rows themselves, runs the same check directly, and holds the other half:
//! a model whose pieces would traverse the path or collapse into an empty piece is still built
//! into a descriptor that `authorize` refuses.

use std::path::Path;

use serde_json::Value;
use south_component_conformance::reference_bedrock_converse::{
    BedrockConverseBearerReferenceV1, BedrockConverseReferenceV1,
};
use south_component_conformance::reference_gemini::GeminiReferenceV1;
use south_component_conformance::{
    CheckV1, FixturePackV1, ProviderComponentV1, run_provider_component_suite_v1,
};
use token_station_protocol::{ChatRequest, DescriptorError, ProviderConfig};

/// The row that closes the issue, present in the Converse, Converse-bearer and Gemini packs.
const ROW: &str = "provider.request.model-id-with-a-slash-stays-one-segment";

struct Package {
    name: &'static str,
    directory: &'static str,
    reference: &'static dyn ProviderComponentV1,
    /// What the built URL ends with for the row's model.
    url_suffix: &'static str,
    /// Models the kernel must refuse once they are in the URL. Where the model is followed by a
    /// suffix inside its segment (Gemini's `:generateContent`), a bare `..` or a trailing `/`
    /// is an ordinary name, not a dot or empty piece, so those are Converse's alone.
    refused_models: &'static [&'static str],
}

const EVERYWHERE: [&str; 5] = ["a/../b", "a//b", "/a", "a/./b", "a\\b"];

const PACKAGES: [Package; 3] = [
    Package {
        name: "provider-bedrock-converse",
        directory: "fixtures-bedrock-converse",
        reference: &BedrockConverseReferenceV1,
        url_suffix: "/model/arn:aws:bedrock:us-east-1:123456789012:inference-profile%2Fus.anthropic.claude-sonnet-4-20250514-v1:0/converse",
        refused_models: &["a/../b", "a//b", "/a", "a/./b", "a\\b", "..", ".", "a/"],
    },
    Package {
        name: "provider-bedrock-converse-bearer",
        directory: "fixtures-bedrock-converse-bearer",
        reference: &BedrockConverseBearerReferenceV1,
        url_suffix: "/model/arn:aws:bedrock:us-east-1:123456789012:inference-profile%2Fus.anthropic.claude-sonnet-4-20250514-v1:0/converse",
        refused_models: &["a/../b", "a//b", "/a", "a/./b", "a\\b", "..", ".", "a/"],
    },
    Package {
        name: "provider-gemini",
        directory: "fixtures-gemini",
        reference: &GeminiReferenceV1,
        url_suffix: "/v1beta/models/tunedModels%2Ffixture-tuned-1:generateContent",
        refused_models: &EVERYWHERE,
    },
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn row_input(package: &Package) -> (ProviderConfig, ChatRequest) {
    let path = root().join(package.directory).join(format!("{ROW}.input.json"));
    let input: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    (
        serde_json::from_value(input["provider_config"].clone()).unwrap(),
        serde_json::from_value(input["chat_request"].clone()).unwrap(),
    )
}

#[test]
fn every_package_that_puts_the_model_in_the_path_ships_the_row() {
    for package in &PACKAGES {
        let pack = FixturePackV1::load(&root().join(package.directory)).unwrap();
        assert!(
            pack.cases().iter().any(|case| case.name == ROW),
            "{}: the pack lost `{ROW}`, the case that closes issue #138",
            package.name
        );
        // Gate ② judges the row, endpoint confinement included.
        let report = run_provider_component_suite_v1(package.reference, &pack);
        let confinement: Vec<_> = report
            .outcomes()
            .iter()
            .filter(|outcome| outcome.check == CheckV1::EndpointConfinement && outcome.case == ROW)
            .collect();
        assert_eq!(
            confinement.len(),
            1,
            "{}: EndpointConfinement never judged the row",
            package.name
        );
        assert!(!confinement[0].is_failure(), "{}: {report}", package.name);
    }
}

#[test]
fn a_model_with_a_slash_is_one_segment_and_the_kernel_authorizes_it() {
    for package in &PACKAGES {
        let (config, request) = row_input(package);
        assert!(request.model.contains('/'), "{}: the row's model must contain `/`", package.name);
        let descriptor = package.reference.build_http_request(&request, &config).unwrap();
        assert!(
            descriptor.url.ends_with(package.url_suffix),
            "{}: {}",
            package.name,
            descriptor.url
        );
        assert!(
            !descriptor.url.trim_start_matches("https://").contains("//"),
            "{}: the slash must stay encoded",
            package.name
        );
        // The check kernel 0.4.0 failed: it refused every encoded separator.
        assert_eq!(config.authorize(&descriptor), Ok(()), "{}", package.name);
    }
}

/// An encoded slash only keeps the request at or below the endpoint while every piece it splits
/// into is a real name. A model that would read as a traversal or an empty piece is built (the
/// references encode and never judge a model) and the kernel's check refuses it, so the request
/// never leaves. The same models are refused by 0.4.0, which refused all of them and the
/// legitimate ARN too.
#[test]
fn a_model_that_would_traverse_or_collapse_is_refused_by_authorize() {
    for package in &PACKAGES {
        let (config, mut request) = row_input(package);
        for model in package.refused_models {
            request.model = (*model).to_owned();
            let descriptor = package.reference.build_http_request(&request, &config).unwrap();
            assert!(
                matches!(
                    config.authorize(&descriptor),
                    Err(DescriptorError::UrlOutsideEndpoint { .. })
                ),
                "{}: `{model}` was built into {} and must be refused",
                package.name,
                descriptor.url
            );
        }
        // Control: a name with no separator is authorized as before.
        request.model = "plain-model:1".to_owned();
        let descriptor = package.reference.build_http_request(&request, &config).unwrap();
        assert_eq!(config.authorize(&descriptor), Ok(()), "{}", package.name);
    }
}
