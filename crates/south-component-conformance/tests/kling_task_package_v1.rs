//! The `task-kling` package, as gate ① sees it.
//!
//! The sandbox half of S3 is **absent on purpose**: `south-provider-runtime`
//! binds `provider-adapter-v2` and only that world, so nothing can yet
//! instantiate a task component. What this file proves is the part that does
//! exist — the shipped manifest is admissible, and it declares the world,
//! suite and package the component was actually built against.
//!
//! The parity test beside the three provider ones arrives with the runtime's
//! second world, which is its own slice.

use std::path::Path;

use south_component_conformance::{TaskComponentV1, reference_kling_task::KlingTaskReferenceV1};
use south_provider_api::{
    CompatibilityMismatchV1, CompatibilityMismatchV2, ComponentManifestV1, TASK_BEHAVIOR_SUITE,
    TASK_WIT_PACKAGE, TASK_WORLD, compatibility_admits, compatibility_matches,
};

#[path = "support/host_range.rs"]
mod host_range;

fn shipped_manifest() -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .join("components/task-kling/manifest.json");
    let source = std::fs::read_to_string(&path).expect("the shipped manifest reads");
    serde_json::from_str(&source).expect("the shipped manifest parses")
}

/// Gate ①: the package the repository ships is admissible as a task component.
#[test]
fn the_shipped_package_passes_gate_one() {
    let manifest = shipped_manifest();
    assert_eq!(manifest.validate(), Ok(()));

    let tuple = manifest.compatibility_tuple();
    assert_eq!(tuple.wit_world, TASK_WORLD);
    assert_eq!(tuple.wit_package, TASK_WIT_PACKAGE);
    assert_eq!(tuple.conformance_suite, TASK_BEHAVIOR_SUITE);
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
}

/// The manifest's identity is what the loaded component will report.
///
/// Until the runtime can instantiate a task world this is compared against the
/// native reference rather than the sandboxed one — the same implementation,
/// which is the point of the guest being a shell around it.
#[test]
fn the_manifest_identity_matches_what_the_component_reports() {
    let manifest = shipped_manifest();
    let reported = KlingTaskReferenceV1.metadata();
    assert_eq!(manifest.name, reported.name);
    assert_eq!(manifest.version, reported.version);
    assert_eq!(manifest.api_version, reported.api_version);
}

/// Both handshakes refuse a stale peer by name.
///
/// The exact host holds the shipped manifest's own runtime declaration, not the workspace version,
/// so this test does not depend on the two being equal.
#[test]
fn a_mismatched_runtime_is_named_in_the_refusal() {
    let expectations = host_range::exact_expectations_for(&shipped_manifest());
    let mut manifest = shipped_manifest();
    manifest.compatibility.south_runtime = "0.15.0".to_owned();
    assert!(matches!(
        compatibility_matches(&manifest, &expectations),
        Err(CompatibilityMismatchV1::SouthRuntime { declared, .. }) if declared == "0.15.0"
    ));
    assert!(matches!(
        compatibility_admits(&manifest, &host_range::host_range()),
        Err(CompatibilityMismatchV2::SouthRuntimeBelowMinimum { declared, .. }) if declared == "0.15.0"
    ));
}

/// A task manifest declaring the chat world's suite is refused: the two worlds
/// must not be confusable for each other.
#[test]
fn the_chat_suite_is_not_accepted_for_a_task_component() {
    let mut manifest = shipped_manifest();
    manifest.conformance.required_suite = south_provider_api::COMPONENT_BEHAVIOR_SUITE.to_owned();
    assert!(manifest.validate().is_err());
}
