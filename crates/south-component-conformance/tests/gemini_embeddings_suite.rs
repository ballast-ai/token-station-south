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
        CheckV1::UndeclaredValuesIgnored,
        CheckV1::MediaInputsFollowTheDeclaration,
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
    /// Re-spells a media input's `data` in the body (here: trims one character).
    AltersMediaData,
    /// Estimates every media input as an image, whatever its type.
    FlatMediaEstimate,
    /// Builds media for the text-only model too.
    IgnoresTextOnlyModel,
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
        if matches!(self.fault, Fault::IgnoresTextOnlyModel) && request.carries_media() {
            // Build the same request for a model that takes media, then put the real model back.
            let mut multimodal = request.clone();
            let wire = serde_json::to_string(&multimodal).unwrap().replace(request.model(), "m");
            multimodal = serde_json::from_str(&wire).unwrap();
            return REFERENCE.build_embeddings_request(config, &multimodal);
        }
        let mut prepared = REFERENCE.build_embeddings_request(config, request)?;
        let fallback = prepared.estimate.fallback_input_tokens();
        prepared.estimate = match self.fault {
            Fault::DriftingEstimate => {
                self.builds.set(self.builds.get() + 1);
                EmbeddingsEstimateV1::new(fallback.map(|value| value + self.builds.get()), None)
            }
            Fault::NoFallback => EmbeddingsEstimateV1::new(None, None),
            Fault::FlatMediaEstimate => {
                let flat = request
                    .inputs()
                    .iter()
                    .map(|input| match input {
                        south_contracts::EmbeddingInputV1::Media { .. } => 258,
                        south_contracts::EmbeddingInputV1::Text(text) => {
                            (text.len() as u64).div_ceil(4)
                        }
                        south_contracts::EmbeddingInputV1::TokenIds(_) => 0,
                    })
                    .sum();
                EmbeddingsEstimateV1::new(Some(flat), Some(flat))
            }
            Fault::ReportedZero | Fault::AltersMediaData | Fault::IgnoresTextOnlyModel => {
                prepared.estimate
            }
        };
        if matches!(self.fault, Fault::AltersMediaData) {
            let mut body = prepared.descriptor.body.take();
            let mut stack: Vec<&mut Value> = body.iter_mut().collect();
            while let Some(value) = stack.pop() {
                match value {
                    Value::Object(map) => {
                        if let Some(Value::String(data)) = map.get_mut("data") {
                            data.pop();
                        }
                        stack.extend(map.values_mut());
                    }
                    Value::Array(items) => stack.extend(items.iter_mut()),
                    _ => {}
                }
            }
            prepared.descriptor.body = body;
        }
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
            _ => parsed,
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

// -- Embeddings contract 2: inline media (record §17) ---------------------------------------

fn pack_without(rows: &[&str]) -> EmbeddingsFixturePackV1 {
    let manifest = manifest();
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(&manifest.conformance.fixtures);
    let pack = EmbeddingsFixturePackV1::load(&directory).unwrap();
    EmbeddingsFixturePackV1::from_cases(
        pack.cases().iter().filter(|case| !rows.contains(&case.name.as_str())).cloned().collect(),
    )
}

#[test]
fn the_package_declares_media_and_so_owes_the_media_row() {
    let manifest = manifest();
    assert!(manifest.capabilities.contains("media"));
    assert_eq!(manifest.compatibility.contracts.get("embeddings"), Some(&2));
    let without_the_row = pack_without(&["embeddings.request.media"]);
    let report = run_embeddings_component_suite_v1(&REFERENCE, &without_the_row, &manifest);
    assert!(
        failed(&report).contains(&(CheckV1::Coverage, "embeddings.request.media".into())),
        "{report}"
    );
}

#[test]
fn a_package_without_media_is_not_asked_for_the_row_but_must_refuse_media() {
    let mut text_only = manifest();
    text_only.capabilities.remove("media");
    text_only.compatibility.contracts.insert("embeddings".to_owned(), 1);
    let media_cases = [
        "embeddings.request.media",
        "embeddings.request.media-batch",
        "embeddings.request.media-empty-payload",
        "embeddings.request.media-verbatim-payload",
        "embeddings.request.media-text-only-model",
        "embeddings.response.media",
        "embeddings.response.media-batch",
    ];
    // No media row in the pack: nothing is owed, and the reference passes as a text-only package.
    let report =
        run_embeddings_component_suite_v1(&REFERENCE, &pack_without(&media_cases), &text_only);
    assert!(report.is_passing(), "{report}");
    // A pack that does carry media requests judges the reference, which builds them, as a package
    // that did not honour its own declaration.
    let report = run_embeddings_component_suite_v1(&REFERENCE, &pack_without(&[]), &text_only);
    let failed = failed(&report);
    for case in ["embeddings.request.media", "embeddings.request.media-batch"] {
        assert!(
            failed.contains(&(CheckV1::MediaInputsFollowTheDeclaration, case.into())),
            "{report}"
        );
    }
    // The model that takes no media is refused as a capability error, which both answers allow.
    assert!(!failed.contains(&(
        CheckV1::MediaInputsFollowTheDeclaration,
        "embeddings.request.media-text-only-model".into()
    )));
}

#[test]
fn a_component_that_alters_the_clients_media_data_fails_the_media_row() {
    let report = broken(Fault::AltersMediaData);
    let failed = failed(&report);
    assert!(
        failed.contains(&(CheckV1::NamedRowAssertion, "embeddings.request.media".into())),
        "{report}"
    );
    assert!(
        failed
            .contains(&(CheckV1::FixtureMatch, "embeddings.request.media-verbatim-payload".into()))
    );
}

#[test]
fn a_media_estimate_that_ignores_the_top_level_type_fails_the_fixture() {
    let report = broken(Fault::FlatMediaEstimate);
    let failed = failed(&report);
    assert!(
        failed.contains(&(CheckV1::FixtureMatch, "embeddings.request.media-batch".into())),
        "{report}"
    );
    assert!(!failed.contains(&(CheckV1::FixtureMatch, "embeddings.request.media".into())));
}

#[test]
fn building_media_for_the_text_only_model_fails_its_fixture() {
    let report = broken(Fault::IgnoresTextOnlyModel);
    let failed = failed(&report);
    assert!(
        failed
            .contains(&(CheckV1::FixtureMatch, "embeddings.request.media-text-only-model".into())),
        "{report}"
    );
}
