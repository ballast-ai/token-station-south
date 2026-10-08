//! Gate ②'s `UndeclaredValuesIgnored` (Q14, `docs/design/2026-09-30-host-zero-vendor-boundary.md`
//! §13.8): a component reads only the `ProviderConfig.declared` and `ChatRequest.host_values` keys
//! its package declares. The shipped reference passes; a component that acts on any other key is
//! red, and one that acts only on a declared key is not. The embeddings suite runs the same check on
//! `declared` alone (the embeddings record §16).

use std::path::Path;

use serde_json::Value;
use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::reference_gemini_embeddings::GeminiEmbeddingsReferenceV1;
use south_component_conformance::{
    CheckV1, ComponentResultV1, EmbeddingsComponentV1, EmbeddingsFixturePackV1, FixturePackV1,
    PreparedEmbeddingsV1, ProviderComponentV1, ReportV1, StreamParserV1,
    run_embeddings_component_suite_v1, run_provider_component_suite_v1_for_manifest,
};
use south_contracts::{EmbeddingsFailureOutcomeV1, EmbeddingsParsedV1, EmbeddingsRequestV1};
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

fn gemini_embeddings() -> ComponentManifestV1 {
    let path = root().join("../../components/embeddings-gemini/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn run_embeddings(
    component: &dyn EmbeddingsComponentV1,
    manifest: &ComponentManifestV1,
) -> ReportV1 {
    let pack = EmbeddingsFixturePackV1::load(&root().join(&manifest.conformance.fixtures)).unwrap();
    run_embeddings_component_suite_v1(component, &pack, manifest)
}

/// The Gemini embeddings reference, writing a `declared` value into its body when it finds one.
struct EmbeddingsReader {
    /// `None` reads any key; `Some(key)` only that one.
    only: Option<&'static str>,
}

impl EmbeddingsComponentV1 for EmbeddingsReader {
    fn metadata(&self) -> ComponentMetadataV1 {
        GeminiEmbeddingsReferenceV1.metadata()
    }

    fn build_embeddings_request(
        &self,
        config: &ProviderConfig,
        request: &EmbeddingsRequestV1,
    ) -> ComponentResultV1<PreparedEmbeddingsV1> {
        let mut prepared = GeminiEmbeddingsReferenceV1.build_embeddings_request(config, request)?;
        let read = self.only.map_or_else(
            || config.declared.iter().next().map(|(_, value)| value),
            |key| config.declared.get(key),
        );
        if let Some(value) = read {
            prepared.descriptor.body.get_or_insert_with(Default::default)["region"] = value.into();
        }
        Ok(prepared)
    }

    fn parse_embeddings_response(
        &self,
        parts: &HttpResponseParts,
        parse_context: &Value,
    ) -> ComponentResultV1<EmbeddingsParsedV1> {
        GeminiEmbeddingsReferenceV1.parse_embeddings_response(parts, parse_context)
    }

    fn map_provider_error(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<(EmbeddingsFailureOutcomeV1, ErrorEnvelope)> {
        GeminiEmbeddingsReferenceV1.map_provider_error(parts)
    }
}

#[test]
fn the_embeddings_suite_runs_the_check_on_every_request_case() {
    let manifest = gemini_embeddings();
    let report = run_embeddings(&GeminiEmbeddingsReferenceV1, &manifest);
    let (ran, failed) = tally(&report);
    let pack = EmbeddingsFixturePackV1::load(&root().join(&manifest.conformance.fixtures)).unwrap();
    let requests = pack.cases().iter().filter(|case| case.name.contains(".request.")).count();
    assert_eq!(ran, requests, "once per request case, refused ones included");
    assert_eq!(failed, 0);
}

#[test]
fn an_embeddings_component_acting_on_an_undeclared_key_is_red() {
    let (ran, failed) =
        tally(&run_embeddings(&EmbeddingsReader { only: None }, &gemini_embeddings()));
    // A request the reference refuses stays refused whatever it reads.
    assert!(failed > 0 && failed < ran, "{failed} of {ran}");
}

#[test]
fn an_embeddings_component_acting_only_on_a_declared_key_passes() {
    let mut manifest = gemini_embeddings();
    manifest.config_schema.entry("gemini".to_owned()).or_default().insert(
        "region".to_owned(),
        ConfigKeyV1 {
            syntax: ValueSyntaxV1::AwsRegion,
            required: false,
            description: "A region the component sends.".to_owned(),
            default: None,
        },
    );
    manifest.validate().unwrap();
    let (ran, failed) =
        tally(&run_embeddings(&EmbeddingsReader { only: Some("region") }, &manifest));
    assert!(ran > 0);
    assert_eq!(failed, 0);
}
