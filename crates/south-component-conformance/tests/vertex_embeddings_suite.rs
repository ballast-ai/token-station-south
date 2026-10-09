//! Gate ② for `embeddings-vertex`: the native reference passes `south.embeddings-component.v1`
//! against its frozen samples and its package manifest (credential recipe cases included), and the
//! suite bites a component that rounds the wrong way, defaults a missing count to zero, or derives
//! the API host from the region instead of staying below the configured base URL.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use south_component_conformance::reference_vertex_embeddings::VertexEmbeddingsReferenceV1;
use south_component_conformance::{
    CheckV1, EmbeddingsComponentV1, EmbeddingsFixturePackV1, PreparedEmbeddingsV1, ReportV1,
    accepts_manifest, reported_identity_matches, run_embeddings_component_suite_v1,
};
use south_contracts::{
    EmbeddingsFailureOutcomeV1, EmbeddingsParsedV1, EmbeddingsRequestV1, EmbeddingsUsageFactsV1,
    InputShapeV1, UsageSourceV1,
};
use south_provider_api::{ComponentManifestV1, ComponentMetadataV1};
use token_station_protocol::{ErrorCode, ErrorEnvelope, HttpResponseParts, ProviderConfig};

const REFERENCE: VertexEmbeddingsReferenceV1 = VertexEmbeddingsReferenceV1;

fn manifest() -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/embeddings-vertex/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn run(component: &dyn EmbeddingsComponentV1) -> ReportV1 {
    let manifest = manifest();
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(&manifest.conformance.fixtures);
    let pack = EmbeddingsFixturePackV1::load(&directory).unwrap();
    run_embeddings_component_suite_v1(component, &pack, &manifest)
}

fn failed(report: &ReportV1) -> BTreeSet<(CheckV1, String)> {
    report.failures().map(|outcome| (outcome.check, outcome.case.clone())).collect()
}

#[test]
fn the_reference_passes_its_suite_with_its_manifest() {
    let manifest = manifest();
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    assert!(reported_identity_matches(&REFERENCE.metadata(), &manifest));
    let report = run(&REFERENCE);
    assert!(report.is_passing(), "{report}");
    let ran: BTreeSet<CheckV1> = report.outcomes().iter().map(|outcome| outcome.check).collect();
    for check in [
        CheckV1::Coverage,
        CheckV1::FixtureMatch,
        CheckV1::Determinism,
        CheckV1::UnknownFieldTolerance,
        CheckV1::EndpointConfinement,
        CheckV1::DescriptorAuthWithinManifest,
        CheckV1::AuthErrorsAreNotRetriable,
        CheckV1::LocatorResolves,
        CheckV1::UsageNeverDefaulted,
        CheckV1::NamedRowAssertion,
        CheckV1::UndeclaredValuesIgnored,
        CheckV1::CredentialRecipeMatch,
    ] {
        assert!(ran.contains(&check), "{check} never ran");
    }
}

/// The values the package declares are the two config keys and the exported project id, and a
/// host builds them per attempt with `declared_values` (record §16).
#[test]
fn the_package_declares_its_region_project_override_and_exported_project() {
    let manifest = manifest();
    assert_eq!(
        manifest.declared_keys("vertex-ai"),
        BTreeSet::from(["project", "project_id", "region"])
    );
    let built = manifest
        .declared_values(
            "vertex-ai",
            &[("region".to_owned(), "global".to_owned())].into(),
            &[("project_id".to_owned(), "fake-project-1".to_owned())].into(),
        )
        .unwrap();
    assert_eq!(built.len(), 2);
    let credentials = manifest.credentials.as_ref().unwrap();
    assert_eq!(credentials.endpoints(), ["https://oauth2.googleapis.com/token"]);
}

/// How a broken Vertex component departs from the reference.
#[derive(Clone, Copy)]
enum Fault {
    /// Rounds a fractional count half to even.
    HalfToEven,
    /// Reports zero tokens when the count is missing.
    DefaultsToZero,
    /// Builds the URL on the region's own host, ignoring the base URL.
    DerivesTheHost,
}

struct Broken(Fault);

impl EmbeddingsComponentV1 for Broken {
    fn metadata(&self) -> ComponentMetadataV1 {
        REFERENCE.metadata()
    }
    fn build_embeddings_request(
        &self,
        config: &ProviderConfig,
        request: &EmbeddingsRequestV1,
    ) -> Result<PreparedEmbeddingsV1, ErrorEnvelope> {
        let mut prepared = REFERENCE.build_embeddings_request(config, request)?;
        if matches!(self.0, Fault::DerivesTheHost) {
            let region = config.declared.get("region").unwrap_or_default();
            let path = prepared.descriptor.url.split_once("/v1/").map(|(_, path)| path.to_owned());
            if let Some(path) = path {
                prepared.descriptor.url =
                    format!("https://{region}-aiplatform.googleapis.com/v1/{path}");
            }
        }
        Ok(prepared)
    }
    fn parse_embeddings_response(
        &self,
        parts: &HttpResponseParts,
        parse_context: &Value,
    ) -> Result<EmbeddingsParsedV1, ErrorEnvelope> {
        let parsed = REFERENCE.parse_embeddings_response(parts, parse_context);
        match (self.0, parsed) {
            (Fault::DefaultsToZero, Err(_)) => {
                Ok(EmbeddingsParsedV1::new(EmbeddingsUsageFactsV1::reported(0), 1, None))
            }
            (Fault::HalfToEven, Ok(parsed)) => {
                let body: Value = serde_json::from_str(&parts.body).unwrap();
                let per: Vec<u64> = body["predictions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|prediction| {
                        let statistics = &prediction["embeddings"]["statistics"];
                        let count = statistics
                            .get("token_count")
                            .or_else(|| statistics.get("tokenCount"))
                            .and_then(Value::as_f64)
                            .unwrap();
                        format!("{:.0}", count.round_ties_even()).parse().unwrap()
                    })
                    .collect();
                let usage = EmbeddingsUsageFactsV1::new(
                    UsageSourceV1::Reported,
                    Some(per.iter().sum()),
                    Some(per),
                )
                .unwrap();
                Ok(EmbeddingsParsedV1::new(usage, parsed.vector_count(), None))
            }
            (_, parsed) => parsed,
        }
    }
    fn map_provider_error(
        &self,
        parts: &HttpResponseParts,
    ) -> Result<(EmbeddingsFailureOutcomeV1, ErrorEnvelope), ErrorEnvelope> {
        REFERENCE.map_provider_error(parts)
    }
}

#[test]
fn rounding_half_to_even_fails_the_rounding_sample() {
    let report = run(&Broken(Fault::HalfToEven));
    assert_eq!(
        failed(&report),
        BTreeSet::from([(
            CheckV1::FixtureMatch,
            "embeddings.response.rounding-half-away".to_owned()
        )]),
        "{report}"
    );
}

#[test]
fn a_count_defaulted_to_zero_fails_usage_never_defaulted() {
    let report = run(&Broken(Fault::DefaultsToZero));
    let failed = failed(&report);
    assert!(
        failed.contains(&(CheckV1::UsageNeverDefaulted, "embeddings.response.usage".into())),
        "{report}"
    );
    assert!(
        failed.contains(&(CheckV1::NamedRowAssertion, "embeddings.response.missing-usage".into())),
        "{report}"
    );
}

#[test]
fn a_host_derived_from_the_region_fails_endpoint_confinement() {
    let report = run(&Broken(Fault::DerivesTheHost));
    let failed = failed(&report);
    // The global location's prefixed host is the one that answers 404 (the native arm's trap).
    assert!(
        failed
            .contains(&(CheckV1::EndpointConfinement, "embeddings.request.global-location".into())),
        "{report}"
    );
    assert!(
        failed.contains(&(CheckV1::EndpointConfinement, "embeddings.request.proxy-base".into())),
        "{report}"
    );
}

// -- Embeddings contract 2: this package speaks contract 1 and refuses media (record §17.5) ----

#[test]
fn a_media_input_is_a_capability_error_whatever_else_the_request_holds() {
    let manifest = manifest();
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(&manifest.conformance.fixtures);
    let pack = EmbeddingsFixturePackV1::load(&directory).unwrap();
    let case = pack.case("embeddings.request.refused-media").expect("the pack ships the refusal");
    let config: ProviderConfig =
        serde_json::from_value(case.input["provider_config"].clone()).unwrap();
    for (inputs, shape) in [
        (vec![media()], InputShapeV1::Single),
        (vec![text("hello"), media()], InputShapeV1::Array),
        (vec![media(), media()], InputShapeV1::Array),
    ] {
        let request = EmbeddingsRequestV1::new_v2(
            "m".into(),
            inputs,
            shape,
            None,
            None,
            None,
            serde_json::Map::new(),
        )
        .unwrap();
        let error = REFERENCE.build_embeddings_request(&config, &request).unwrap_err();
        assert_eq!(error.code, ErrorCode::Capability);
        assert_eq!(error.http_status, 400);
    }
}

#[test]
fn the_media_refusal_row_passes_and_the_package_owes_no_media_row() {
    let manifest = manifest();
    assert!(!manifest.capabilities.contains("media"));
    assert_eq!(manifest.compatibility.contracts.get("embeddings"), Some(&1));
    let report = run(&REFERENCE);
    assert!(report.is_passing(), "{report}");
    assert!(report.outcomes().iter().any(|outcome| {
        outcome.check == CheckV1::MediaInputsFollowTheDeclaration
            && outcome.case == "embeddings.request.refused-media"
    }));
}

fn text(value: &str) -> south_contracts::EmbeddingInputV1 {
    south_contracts::EmbeddingInputV1::Text(value.to_owned())
}

fn media() -> south_contracts::EmbeddingInputV1 {
    south_contracts::EmbeddingInputV1::Media {
        media_type: "image/png".to_owned(),
        data: "iVBORw0KGgo=".to_owned(),
    }
}
