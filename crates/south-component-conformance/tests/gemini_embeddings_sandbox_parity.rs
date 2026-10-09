//! `embeddings-gemini` inside the sandbox: the shipped component passes gate ② against the same
//! frozen pack and manifest that judged its native reference, and agrees with that reference byte
//! for byte across the ABI.
//!
//! Design record: `docs/design/2026-09-30-embeddings-contract.md` (§10, §15).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use south_component_conformance::reference_gemini_embeddings::GeminiEmbeddingsReferenceV1;
use south_component_conformance::sandbox::SandboxedTaskComponentV1;
use south_component_conformance::sandbox_embeddings::SandboxedEmbeddingsComponentV1;
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

const PACKAGE: &str = "embeddings-gemini";
const NATIVE: GeminiEmbeddingsReferenceV1 = GeminiEmbeddingsReferenceV1;

/// Builds the component once per test process.
fn component_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| embeddings_parity::build(PACKAGE, "build-embeddings-gemini-component.sh"))
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

/// Every case's ABI answer is the same string inside and outside the sandbox, the fallback
/// estimate included.
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

/// The task-v1 seam refuses the loaded embeddings component and hands it back intact.
#[test]
fn the_task_v1_seam_refuses_a_loaded_embeddings_component() {
    let loaded = embeddings_parity::load(PACKAGE, component_wasm());
    let returned =
        SandboxedTaskComponentV1::new(loaded).expect_err("the task-v1 seam rejects embeddings");
    assert!(
        SandboxedEmbeddingsComponentV1::new(*returned).is_ok(),
        "the refused component stays usable through its exact world"
    );
}

/// A package declaring `media` receives a media input through the seam, and the guest builds the
/// same prepared request the native reference does.
#[test]
fn the_seam_hands_media_to_the_package_that_declares_it() {
    use south_contracts::{EmbeddingInputV1, EmbeddingsRequestV1, InputShapeV1};
    use token_station_protocol::ProviderConfig;

    let sandboxed = embeddings_parity::sandboxed(PACKAGE, component_wasm());
    let config: ProviderConfig = serde_json::from_str(
        r#"{"provider":"gemini","base_url":"https://generativelanguage.googleapis.com","auth":"provider_api_key"}"#,
    )
    .unwrap();
    let request = EmbeddingsRequestV1::new_v2(
        "gemini-embedding-2-preview".into(),
        vec![
            EmbeddingInputV1::Text("caption".into()),
            EmbeddingInputV1::Media { media_type: "video/mp4".into(), data: "AAAAIGZ0eXA=".into() },
        ],
        InputShapeV1::Array,
        Some(8),
        None,
        None,
        serde_json::Map::new(),
    )
    .unwrap();
    let built = sandboxed.build_embeddings_request(&config, &request).unwrap();
    assert_eq!(built, NATIVE.build_embeddings_request(&config, &request).unwrap());
    // 2 for "caption" (7 bytes), 1024 for video.
    assert_eq!(built.estimate.fallback_input_tokens(), Some(2 + 1024));
    assert_eq!(
        built.descriptor.body.as_ref().unwrap()["requests"][1]["content"]["parts"][0]["inline_data"]
            ["data"],
        "AAAAIGZ0eXA="
    );
}
