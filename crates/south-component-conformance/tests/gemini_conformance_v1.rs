//! Gates ① and ② for the official Gemini component, run against
//! its native reference implementation over its own frozen fixture pack.
//!
//! Design record: `docs/design/2026-08-22-gemini-provider-component.md`.
//!
//! A second component means a second pack: the suite is the same, the cases
//! are not. Sharing one pack between dialects would freeze whichever dialect
//! happened to be written first.

use std::collections::BTreeSet;
use std::path::Path;

use south_component_conformance::reference_gemini::GeminiReferenceV1;
use south_component_conformance::{
    FixturePackV1, HostExpectationsV1, PROVIDER_COMPONENT_SUITE_V1, ProviderComponentV1,
    accepts_manifest, compatibility_matches, reported_identity_matches,
    run_provider_component_suite_v1,
};
use south_provider_api::{
    AuthArmV1, COMPONENT_BEHAVIOR_SUITE, CompatibilityDeclarationV1, ComponentCapabilityV1,
    ComponentManifestV1, ComponentPermissionsV1, ConformanceSpecV1, PROVIDER_WORLD, WIT_PACKAGE,
};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

/// The manifest the component actually ships, read from disk rather than
/// hand-copied: a hand-copy resembles it, which is not the same as being it.
fn shipped_manifest() -> ComponentManifestV1 {
    let source =
        std::fs::read_to_string(repo_root().join("components/provider-gemini/manifest.json"))
            .expect("the shipped component manifest reads");
    serde_json::from_str(&source).expect("the shipped component manifest parses")
}

fn shipped_pack() -> FixturePackV1 {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures-gemini");
    FixturePackV1::load(&directory).expect("the shipped fixture pack loads")
}

fn host_expectations() -> HostExpectationsV1 {
    HostExpectationsV1 {
        ir_schema_id: "token-station-protocol@0.3.0/v0.2.0".to_owned(),
        kernel_version: "0.2.0".to_owned(),
        kernel_revision: "72458e3a11fe157f9ac04818c44b62a3dd2cb09c".to_owned(),
        south_runtime: "0.15.0".to_owned(),
    }
}

/// Gate ①: the package the component ships is admissible, and the identity it
/// reports at runtime is the identity its manifest claims.
#[test]
fn gate_one_admits_the_shipped_package_and_its_reported_identity() {
    let manifest = shipped_manifest();
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    assert!(
        reported_identity_matches(&GeminiReferenceV1.metadata(), &manifest),
        "the reference reports the identity its manifest claims"
    );
    assert_eq!(
        compatibility_matches(&manifest, &host_expectations()),
        Ok(()),
        "the shipped compatibility tuple matches this host's pins"
    );
    assert_eq!(
        manifest.conformance.required_suite, PROVIDER_COMPONENT_SUITE_V1,
        "the frozen suite name"
    );
}

/// Gate ①: the manifest declares exactly the arms and capabilities this
/// dialect uses. `x-api-key` is a header secret; there is no bearer arm and
/// no OAuth arm, so declaring one would over-claim.
#[test]
fn the_manifest_declares_exactly_what_the_dialect_uses() {
    let manifest = shipped_manifest();
    assert_eq!(manifest.providers, vec!["gemini".to_owned()]);
    assert_eq!(manifest.auth_arms, BTreeSet::from([AuthArmV1::HeaderSecret]));
    assert_eq!(
        manifest.capabilities,
        BTreeSet::from([
            ComponentCapabilityV1::Chat,
            ComponentCapabilityV1::Stream,
            ComponentCapabilityV1::ToolCall,
        ]),
        "the dialect's structured-output knob is not translated yet, so it is not claimed"
    );
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
            fixtures: "fixtures-gemini/".to_owned(),
        },
    );
    assert_eq!(manifest.api_version, PROVIDER_WORLD);
    assert_eq!(
        manifest.compatibility,
        CompatibilityDeclarationV1 {
            ir_schema_id: "token-station-protocol@0.3.0/v0.2.0".to_owned(),
            kernel_version: "0.2.0".to_owned(),
            kernel_revision: "72458e3a11fe157f9ac04818c44b62a3dd2cb09c".to_owned(),
            wit_package: WIT_PACKAGE.to_owned(),
            south_runtime: "0.15.0".to_owned(),
        },
    );
}

/// Gate ②: the reference passes its own frozen pack.
#[test]
fn gate_two_passes_over_the_shipped_pack() {
    let report = run_provider_component_suite_v1(&GeminiReferenceV1, &shipped_pack());
    for failure in report.failures() {
        eprintln!("{failure}");
    }
    assert!(report.is_passing(), "{report}");
}

/// The pack keeps a case for every behaviour the design record decided. A
/// decision with no case is a decision the gate cannot hold anyone to.
#[test]
fn the_shipped_pack_still_carries_every_decided_behaviour() {
    let pack = shipped_pack();
    let names: BTreeSet<&str> = pack.cases().iter().map(|case| case.name.as_str()).collect();
    for required in [
        // G1 — the model is in the path and the operation is a `:method`
        // suffix; streaming changes the suffix, not a body field.
        "provider.request.chat",
        "provider.request.streaming-selects-a-different-operation",
        // G2 — a tool result is keyed by the called function's name.
        "provider.request.tool-round-trip-keys-the-result-by-name",
        // G3 — a synthetic call id, stable for a given response.
        "provider.response.tool-call-gets-a-synthetic-id",
        "provider.stream.tool-call",
        // G4 — reasoning rides on a flag, not a block type.
        "provider.request.thinking-and-images",
        "provider.response.thought-parts-and-token-buckets",
        "provider.stream.thought-parts-are-not-visible-text",
        // G5 — unknown enum values survive.
        "provider.response.unknown-finish-reason-survives",
        // The terminal sequence, its EOF path, and its absence.
        "provider.stream.text-and-terminal",
        "provider.stream.eof-terminates-without-a-usage-frame",
        "provider.stream.an-unfinished-stream-gets-no-synthetic-terminal",
    ] {
        assert!(names.contains(required), "the pack lost `{required}`");
    }
}
