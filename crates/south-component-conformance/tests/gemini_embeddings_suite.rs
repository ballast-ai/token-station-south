//! Gate ② for `embeddings-gemini`: the native reference passes `south.embeddings-component.v1`
//! against its frozen samples and its package manifest, and the suite bites a component whose
//! fallback estimate drifts, disappears or is replaced by a reported zero.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use south_component_conformance::reference_gemini_embeddings::GeminiEmbeddingsReferenceV1;
use south_component_conformance::{
    CheckV1, EmbeddingsComponentV1, EmbeddingsFixturePackV1, PreparedEmbeddingsV1, ReportV1,
    accepts_manifest, reported_identity_matches, run_embeddings_component_suite_v1,
};
use south_contracts::{
    EmbeddingsEstimateV1, EmbeddingsFailureOutcomeV1, EmbeddingsParsedV1, EmbeddingsRequestV1,
    EmbeddingsUsageFactsV1,
};
use south_provider_api::{ComponentManifestV1, ComponentMetadataV1};
use token_station_protocol::{ErrorEnvelope, HttpResponseParts, ProviderConfig};

const REFERENCE: GeminiEmbeddingsReferenceV1 = GeminiEmbeddingsReferenceV1;

fn manifest() -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/embeddings-gemini/manifest.json");
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
        CheckV1::NamedRowAssertion,
    ] {
        assert!(ran.contains(&check), "{check} never ran");
    }
}

/// How a broken Gemini component departs from the reference.
#[derive(Clone, Copy)]
enum Fault {
    /// The fallback estimate grows by one on every build.
    DriftingEstimate,
    /// No fallback estimate.
    NoFallback,
    /// Reports zero tokens instead of `not_reported`.
    ReportedZero,
}

struct Broken {
    fault: Fault,
    builds: Cell<u64>,
}

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
        let fallback = prepared.estimate.fallback_input_tokens();
        prepared.estimate = match self.fault {
            Fault::DriftingEstimate => {
                self.builds.set(self.builds.get() + 1);
                EmbeddingsEstimateV1::new(fallback.map(|value| value + self.builds.get()), None)
            }
            Fault::NoFallback => EmbeddingsEstimateV1::new(None, None),
            Fault::ReportedZero => prepared.estimate,
        };
        Ok(prepared)
    }
    fn parse_embeddings_response(
        &self,
        parts: &HttpResponseParts,
        parse_context: &Value,
    ) -> Result<EmbeddingsParsedV1, ErrorEnvelope> {
        let parsed = REFERENCE.parse_embeddings_response(parts, parse_context)?;
        Ok(match self.fault {
            Fault::ReportedZero => EmbeddingsParsedV1::new(
                EmbeddingsUsageFactsV1::reported(0),
                parsed.vector_count(),
                None,
            ),
            Fault::DriftingEstimate | Fault::NoFallback => parsed,
        })
    }
    fn map_provider_error(
        &self,
        parts: &HttpResponseParts,
    ) -> Result<(EmbeddingsFailureOutcomeV1, ErrorEnvelope), ErrorEnvelope> {
        REFERENCE.map_provider_error(parts)
    }
}

fn broken(fault: Fault) -> ReportV1 {
    run(&Broken { fault, builds: Cell::new(0) })
}

#[test]
fn a_drifting_fallback_estimate_fails_determinism() {
    let report = broken(Fault::DriftingEstimate);
    let failed = failed(&report);
    assert!(failed.contains(&(CheckV1::Determinism, "embeddings.request.single-text".into())));
    assert!(failed.contains(&(CheckV1::Determinism, "embeddings.request.batch-text".into())));
}

#[test]
fn a_dialect_without_a_fallback_must_refuse_missing_usage() {
    let report = broken(Fault::NoFallback);
    assert!(
        failed(&report)
            .contains(&(CheckV1::NamedRowAssertion, "embeddings.response.missing-usage".into())),
        "{report}"
    );
}

#[test]
fn a_reported_zero_fails_the_usage_row() {
    let report = broken(Fault::ReportedZero);
    assert!(
        failed(&report).contains(&(CheckV1::NamedRowAssertion, "embeddings.response.usage".into())),
        "{report}"
    );
}
