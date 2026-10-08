//! `embeddings-openai-compatible` inside the sandbox: the shipped component passes gate ② against
//! the same frozen pack and manifest that judged its native reference, and agrees with that
//! reference byte for byte across the ABI.
//!
//! Design record: `docs/design/2026-09-30-embeddings-contract.md` (§10, §15).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use south_component_conformance::reference_openai_compatible_embeddings::OpenAiCompatibleEmbeddingsReferenceV1;
use south_component_conformance::sandbox_embeddings::SandboxedEmbeddingsComponentV1;
use south_component_conformance::sandbox_task_v2::SandboxedTaskComponentV2;
use south_component_conformance::{
    EmbeddingsComponentV1, accepts_manifest, reported_identity_matches,
    run_embeddings_component_suite_v1,
};
use south_provider_api::compatibility_admits;

#[path = "support/embeddings_parity.rs"]
mod embeddings_parity;
#[path = "support/gate2_report.rs"]
mod gate2_report;
#[path = "support/host_range.rs"]
mod host_range;

const PACKAGE: &str = "embeddings-openai-compatible";
const NATIVE: OpenAiCompatibleEmbeddingsReferenceV1 = OpenAiCompatibleEmbeddingsReferenceV1;

/// Builds the component once per test process.
fn component_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        embeddings_parity::build(PACKAGE, "build-embeddings-openai-compatible-component.sh")
    })
}

/// The sandboxed component passes the same gate ② the native reference does, against the same
/// frozen pack and manifest; the run is recorded as the package's gate ② report.
#[test]
fn the_sandboxed_component_passes_gate_two_byte_for_byte() {
    let evidence = gate2_report::Evidence::capture(PACKAGE, component_wasm());
    let report = run_embeddings_component_suite_v1(
        &embeddings_parity::sandboxed(PACKAGE, component_wasm()),
        &embeddings_parity::pack(PACKAGE),
        &embeddings_parity::manifest(PACKAGE),
    );
    evidence.record(&report);
    assert!(report.is_passing(), "{report}");
}

/// The two builds agree check for check, not merely both-pass: a suite that passed for different
/// reasons on each side would hide a boundary bug.
#[test]
fn the_sandboxed_and_native_reports_are_identical() {
    let pack = embeddings_parity::pack(PACKAGE);
    let manifest = embeddings_parity::manifest(PACKAGE);
    let sandboxed = run_embeddings_component_suite_v1(
        &embeddings_parity::sandboxed(PACKAGE, component_wasm()),
        &pack,
        &manifest,
    );
    let native = run_embeddings_component_suite_v1(&NATIVE, &pack, &manifest);
    assert_eq!(format!("{:?}", sandboxed.outcomes()), format!("{:?}", native.outcomes()));
}

/// Every case's ABI answer is the same string inside and outside the sandbox.
#[test]
fn every_abi_answer_is_byte_identical_to_the_native_reference() {
    let loaded = embeddings_parity::load(PACKAGE, component_wasm());
    let compared = embeddings_parity::assert_raw_abi_parity(
        &loaded,
        &NATIVE,
        &embeddings_parity::pack(PACKAGE),
    );
    assert!(compared.iter().all(|count| *count > 0), "every function was compared: {compared:?}");
}

/// Gate ①: the manifest, the reported identity (sandboxed, native and manifest agree) and the
/// range handshake of a host linking this release.
#[test]
fn the_shipped_package_passes_gate_one() {
    let manifest = embeddings_parity::manifest(PACKAGE);
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    host_range::assert_released_runtime(&manifest);
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
    let component = embeddings_parity::sandboxed(PACKAGE, component_wasm());
    assert_eq!(component.metadata(), NATIVE.metadata());
    assert!(reported_identity_matches(&component.metadata(), &manifest));
}

/// Another world's typed seam refuses the loaded embeddings component and hands it back intact.
#[test]
fn the_task_seam_refuses_a_loaded_embeddings_component() {
    let loaded = embeddings_parity::load(PACKAGE, component_wasm());
    let returned =
        SandboxedTaskComponentV2::new(loaded).expect_err("the task seam rejects embeddings");
    assert!(
        SandboxedEmbeddingsComponentV1::new(*returned).is_ok(),
        "the refused component stays usable through its exact world"
    );
}
