//! Gate ②'s `UndeclaredValuesIgnored` (Q14, `docs/design/2026-09-30-host-zero-vendor-boundary.md`
//! §13.8): a component reads only the `ProviderConfig.declared` and `ChatRequest.host_values` keys
//! its package declares. The shipped reference passes; a component that acts on any other key is
//! red, and one that acts only on a declared key is not.

use std::path::Path;

use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::{
    CheckV1, ComponentResultV1, FixturePackV1, ProviderComponentV1, ReportV1, StreamParserV1,
    run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::{ComponentManifestV1, ComponentMetadataV1, ConfigKeyV1, ValueSyntaxV1};
use token_station_protocol::{
    ChatRequest, ChatResponse, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts,
    ModelCapability, ProviderConfig,
};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn shipped() -> ComponentManifestV1 {
    let path = root().join("../../components/provider-openai-compatible/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn run(component: &dyn ProviderComponentV1, manifest: &ComponentManifestV1) -> ReportV1 {
    let pack = FixturePackV1::load(&root().join("fixtures")).unwrap();
    run_provider_component_suite_v1_for_manifest(component, &pack, manifest)
}

/// `(ran, failed)` outcomes of the check.
fn tally(report: &ReportV1) -> (usize, usize) {
    let outcomes: Vec<_> = report
        .outcomes()
        .iter()
        .filter(|outcome| outcome.check == CheckV1::UndeclaredValuesIgnored)
        .collect();
    (outcomes.len(), outcomes.iter().filter(|outcome| outcome.is_failure()).count())
}

/// What the wrapped reference reads before building.
#[derive(Clone, Copy)]
enum Reads {
    /// Any key of `declared`, declared or not.
    AnyDeclaredKey,
    /// Any key of `host_values`.
    AnyHostValue,
    /// Only `tenant`, a key the test manifest declares.
    OnlyTheDeclaredTenant,
}

struct Reader(Reads);

impl ProviderComponentV1 for Reader {
    fn metadata(&self) -> ComponentMetadataV1 {
        OpenAiCompatibleReferenceV1.metadata()
    }

    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ModelCapability>> {
        OpenAiCompatibleReferenceV1.model_capabilities(config)
    }

    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        let mut built = OpenAiCompatibleReferenceV1.build_http_request(request, config)?;
        let read = match self.0 {
            Reads::AnyDeclaredKey => config.declared.iter().next().map(|(_, value)| value),
            Reads::AnyHostValue => request.host_values.iter().next().map(|(_, value)| value),
            Reads::OnlyTheDeclaredTenant => config.declared.get("tenant"),
        };
        if let Some(value) = read {
            built.body.get_or_insert_with(Default::default)["tenant"] = value.into();
        }
        Ok(built)
    }

    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        OpenAiCompatibleReferenceV1.parse_response(parts)
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        OpenAiCompatibleReferenceV1.map_provider_error(parts)
    }

    fn stream_parser(&self) -> Box<dyn StreamParserV1> {
        OpenAiCompatibleReferenceV1.stream_parser()
    }
}

#[test]
fn the_shipped_reference_ignores_undeclared_values_on_every_request_case() {
    let (ran, failed) = tally(&run(&OpenAiCompatibleReferenceV1, &shipped()));
    assert!(ran > 0, "the check must run on the request cases");
    assert_eq!(failed, 0);
}

#[test]
fn a_component_acting_on_an_undeclared_key_is_red() {
    for reads in [Reads::AnyDeclaredKey, Reads::AnyHostValue] {
        let (ran, failed) = tally(&run(&Reader(reads), &shipped()));
        assert_eq!(failed, ran, "every request case must catch it");
    }
}

#[test]
fn a_component_acting_only_on_a_declared_key_passes() {
    let mut manifest = shipped();
    manifest.config_schema.entry("openai-compatible".to_owned()).or_default().insert(
        "tenant".to_owned(),
        ConfigKeyV1 {
            syntax: ValueSyntaxV1::Token,
            required: false,
            description: "A tenant the component sends.".to_owned(),
            default: None,
        },
    );
    manifest.validate().unwrap();
    let (ran, failed) = tally(&run(&Reader(Reads::OnlyTheDeclaredTenant), &manifest));
    assert!(ran > 0);
    assert_eq!(failed, 0);
}
