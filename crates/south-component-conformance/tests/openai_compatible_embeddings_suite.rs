//! Gate ② for `embeddings-openai-compatible`: the native reference passes
//! `south.embeddings-component.v1` against its frozen samples and its package manifest, and the
//! suite bites a component that breaks each property it checks.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use south_component_conformance::reference_openai_compatible_embeddings::OpenAiCompatibleEmbeddingsReferenceV1;
use south_component_conformance::{
    CheckV1, EmbeddingsComponentV1, EmbeddingsFixturePackV1, PreparedEmbeddingsV1, ReportV1,
    accepts_manifest, reported_identity_matches, run_embeddings_component_suite_v1,
};
use south_contracts::{
    EmbeddingsFailureOutcomeV1, EmbeddingsParsedV1, EmbeddingsRequestV1, EmbeddingsUsageFactsV1,
    JsonPointerV1, VectorLocatorV1,
};
use south_provider_api::{ComponentManifestV1, ComponentMetadataV1};
use token_station_protocol::{ErrorCode, ErrorEnvelope, HttpResponseParts, ProviderConfig};

const REFERENCE: OpenAiCompatibleEmbeddingsReferenceV1 = OpenAiCompatibleEmbeddingsReferenceV1;

fn manifest() -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/embeddings-openai-compatible/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn pack() -> EmbeddingsFixturePackV1 {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(manifest().conformance.fixtures);
    EmbeddingsFixturePackV1::load(&directory).unwrap()
}

fn run(component: &dyn EmbeddingsComponentV1) -> ReportV1 {
    run_embeddings_component_suite_v1(component, &pack(), &manifest())
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
        CheckV1::UsageNeverDefaulted,
        CheckV1::LocatorResolves,
        CheckV1::NamedRowAssertion,
        CheckV1::UndeclaredValuesIgnored,
    ] {
        assert!(ran.contains(&check), "{check} never ran");
    }
}

/// How a broken component departs from the reference; it delegates everywhere else.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Broken {
    /// A missing `usage.prompt_tokens` is reported as zero.
    ZeroUsage,
    /// A rejected credential maps onto a code the router retries elsewhere.
    RetriableAuth,
    /// The URL leaves the configured endpoint.
    EscapeEndpoint,
    /// The locator names the first vector only.
    FirstVectorOnly,
    /// `dimensions` is dropped from the body.
    DropDimensions,
    /// A provider configuration carrying an unknown field is refused.
    RefuseUnknownConfig,
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
        if *self == Self::RefuseUnknownConfig && !config.extensions.is_empty() {
            return Err(ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, "unknown field"));
        }
        let mut prepared = REFERENCE.build_embeddings_request(config, request)?;
        if *self == Self::EscapeEndpoint {
            "https://collector.example/embeddings".clone_into(&mut prepared.descriptor.url);
        }
        if *self == Self::FirstVectorOnly {
            prepared.vectors = VectorLocatorV1::Single {
                vector: JsonPointerV1::parse("/data/0/embedding").unwrap(),
            };
        }
        if *self == Self::DropDimensions
            && let Some(Value::Object(body)) = &mut prepared.descriptor.body
        {
            body.remove("dimensions");
        }
        Ok(prepared)
    }
    fn parse_embeddings_response(
        &self,
        parts: &HttpResponseParts,
        parse_context: &Value,
    ) -> Result<EmbeddingsParsedV1, ErrorEnvelope> {
        if *self == Self::ZeroUsage {
            let raw: Value = serde_json::from_str(&parts.body).unwrap_or_default();
            let count = raw["data"].as_array().map_or(0, Vec::len);
            let tokens = raw["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
            return Ok(EmbeddingsParsedV1::new(
                EmbeddingsUsageFactsV1::reported(tokens),
                u32::try_from(count).unwrap(),
                None,
            ));
        }
        REFERENCE.parse_embeddings_response(parts, parse_context)
    }
    fn map_provider_error(
        &self,
        parts: &HttpResponseParts,
    ) -> Result<(EmbeddingsFailureOutcomeV1, ErrorEnvelope), ErrorEnvelope> {
        let (outcome, mut error) = REFERENCE.map_provider_error(parts)?;
        if *self == Self::RetriableAuth && error.code == ErrorCode::Auth {
            error.code = ErrorCode::RateLimit;
        }
        Ok((outcome, error))
    }
}

fn has(report: &ReportV1, check: CheckV1, case: &str) -> bool {
    failed(report).contains(&(check, case.to_owned()))
}

#[test]
fn a_usage_defaulted_to_zero_fails_usage_never_defaulted() {
    let report = run(&Broken::ZeroUsage);
    assert!(has(&report, CheckV1::UsageNeverDefaulted, "embeddings.response.usage"), "{report}");
    assert!(has(&report, CheckV1::NamedRowAssertion, "embeddings.response.missing-usage"));
}

#[test]
fn a_missing_row_fails_coverage() {
    let cases: Vec<_> = pack()
        .cases()
        .iter()
        .filter(|case| case.name != "embeddings.error.server")
        .cloned()
        .collect();
    let report = run_embeddings_component_suite_v1(
        &REFERENCE,
        &EmbeddingsFixturePackV1::from_cases(cases),
        &manifest(),
    );
    assert_eq!(
        failed(&report),
        BTreeSet::from([(CheckV1::Coverage, "embeddings.error.server".into())])
    );
}

#[test]
fn a_retriable_rejected_credential_fails() {
    let report = run(&Broken::RetriableAuth);
    assert!(has(
        &report,
        CheckV1::AuthErrorsAreNotRetriable,
        "embeddings.error.rejected-credential"
    ));
}

#[test]
fn an_escaping_url_fails_endpoint_confinement() {
    let report = run(&Broken::EscapeEndpoint);
    assert!(has(&report, CheckV1::EndpointConfinement, "embeddings.request.single-text"));
    assert!(has(&report, CheckV1::DescriptorAuthWithinManifest, "embeddings.request.single-text"));
}

#[test]
fn an_undeclared_auth_arm_fails_descriptor_auth() {
    let mut manifest = manifest();
    manifest.auth_arms.remove("bearer");
    let report = run_embeddings_component_suite_v1(&REFERENCE, &pack(), &manifest);
    assert!(
        report.failures().all(|outcome| outcome.check == CheckV1::DescriptorAuthWithinManifest),
        "{report}"
    );
    assert!(has(&report, CheckV1::DescriptorAuthWithinManifest, "embeddings.request.single-text"));
    assert!(!has(
        &report,
        CheckV1::DescriptorAuthWithinManifest,
        "embeddings.request.azure-header-auth"
    ));
}

#[test]
fn a_locator_resolving_the_wrong_count_fails_locator_resolves() {
    let report = run(&Broken::FirstVectorOnly);
    assert!(has(&report, CheckV1::LocatorResolves, "embeddings.response.base64-without-index"));
    assert!(!has(&report, CheckV1::LocatorResolves, "embeddings.response.usage"));
}

#[test]
fn dimensions_not_placed_fail_the_named_row() {
    let report = run(&Broken::DropDimensions);
    assert!(has(&report, CheckV1::NamedRowAssertion, "embeddings.request.dimensions"));
    assert!(has(&report, CheckV1::FixtureMatch, "embeddings.request.dimensions"));
}

#[test]
fn a_component_refusing_a_newer_peer_field_fails_unknown_field_tolerance() {
    let report = run(&Broken::RefuseUnknownConfig);
    assert!(has(&report, CheckV1::UnknownFieldTolerance, "embeddings.request.single-text"));
}
