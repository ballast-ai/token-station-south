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
    FixturePackV1, PROVIDER_COMPONENT_SUITE_V1, ProviderComponentV1, accepts_manifest,
    reported_identity_matches, run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::compatibility_admits;
use south_provider_api::{
    COMPONENT_BEHAVIOR_SUITE, CompatibilityDeclarationV1, ComponentManifestV1,
    ComponentPermissionsV1, ConformanceSpecV1, PROVIDER_WORLD, WIT_PACKAGE,
};

#[path = "support/host_range.rs"]
mod host_range;

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
        compatibility_admits(&manifest, &host_range::host_range()),
        Ok(()),
        "the shipped compatibility declaration falls inside this host's range"
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
    assert_eq!(manifest.auth_arms, BTreeSet::from(["header_secret".to_owned()]));
    assert_eq!(
        manifest.capabilities,
        BTreeSet::from(["chat".to_owned(), "stream".to_owned(), "tool_call".to_owned(),]),
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
    // `south_runtime` may lag this release (§8.6); it is held to the shipped-package rule rather
    // than to the workspace version.
    host_range::assert_released_runtime(&manifest);
    assert_eq!(
        manifest.compatibility,
        CompatibilityDeclarationV1 {
            ir_schema_id: "token-station-protocol@0.5.0/v0.4.0".to_owned(),
            kernel_version: "0.4.0".to_owned(),
            kernel_revision: "8e34f5a089d0b9c7273b49ddb6952dd87e960019".to_owned(),
            wit_package: WIT_PACKAGE.to_owned(),
            south_runtime: manifest.compatibility.south_runtime.clone(),
            runtime_abi: Some(south_provider_api::RUNTIME_ABI),
            kernel_contracts: std::collections::BTreeMap::from([
                ("canonical_ir".to_owned(), 3),
                ("error_catalog".to_owned(), 1),
                ("stream".to_owned(), 2),
            ]),
            contracts: std::collections::BTreeMap::new(),
        },
    );
}

/// Gate ②: the reference passes its own frozen pack.
#[test]
fn gate_two_passes_over_the_shipped_pack() {
    let report = run_provider_component_suite_v1_for_manifest(
        &GeminiReferenceV1,
        &shipped_pack(),
        &shipped_manifest(),
    );
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
        // SF27 — usage comes from the chunk that carries `finishReason`; an
        // earlier `usageMetadata` (Vertex's count-less one, or running counts)
        // is progress, not usage.
        "provider.stream.vertex-intermediate-usage-metadata-carries-no-counts",
        "provider.stream.intermediate-counts-are-progress-not-usage",
    ] {
        assert!(names.contains(required), "the pack lost `{required}`");
    }
}
