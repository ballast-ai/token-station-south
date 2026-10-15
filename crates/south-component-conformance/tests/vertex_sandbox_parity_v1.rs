//! `provider-vertex` inside the sandbox: the shipped component passes gate ② byte for byte against
//! the same frozen pack and manifest that judged its native reference, and gives the native answer
//! to the configuration refusals that live outside the pack.
//!
//! Design record: `docs/design/2026-10-15-provider-vertex.md`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::Value;
use south_component_conformance::reference_vertex::VertexReferenceV1;
use south_component_conformance::sandbox::SandboxedComponentV1;
use south_component_conformance::{
    FixturePackV1, ProviderComponentV1, abi, accepts_manifest, reported_identity_matches,
    run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::{
    ComponentManifestV1, HostExpectationsV1, compatibility_admits, compatibility_matches,
};
use south_provider_runtime::{
    CallErrorV1, ComponentRuntimeV1, LoadedComponentV1, NoSecretsV1, RuntimeLimitsV1,
};

#[path = "support/gate2_report.rs"]
mod gate2_report;
#[path = "support/host_range.rs"]
mod host_range;

const PACKAGE: &str = "provider-vertex";
const NATIVE: VertexReferenceV1 = VertexReferenceV1;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

/// Builds the component once per test process with the package's own build script.
fn component_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let status = Command::new("bash")
            .arg(repo_root().join("scripts/build-provider-vertex-component.sh"))
            .status()
            .expect("bash is on PATH");
        assert!(
            status.success(),
            "{PACKAGE} must build; run `rustup target add wasm32-wasip2` if the target is missing"
        );
        repo_root()
            .join("components")
            .join(PACKAGE)
            .join("target/wasm32-wasip2/release/provider_vertex.wasm")
    })
}

fn manifest_source() -> String {
    std::fs::read_to_string(repo_root().join("components").join(PACKAGE).join("manifest.json"))
        .expect("the shipped manifest reads")
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(&manifest_source()).expect("the shipped manifest parses")
}

/// The pack the manifest names, so the test judges what the package declares.
fn pack() -> FixturePackV1 {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(manifest().conformance.fixtures);
    FixturePackV1::load(&directory).expect("the shipped pack loads")
}

/// Loads the package through every load gate under this release's range handshake.
fn load() -> LoadedComponentV1 {
    let runtime = ComponentRuntimeV1::new(RuntimeLimitsV1::default()).expect("engine builds");
    let wasm = std::fs::read(component_wasm()).expect("the component reads");
    LoadedComponentV1::load_embedded(
        &runtime,
        &manifest_source(),
        &wasm,
        &host_range::host_range(),
        NoSecretsV1,
    )
    .expect("the shipped package passes every load gate")
}

/// The run that proves "the sandboxed output equals the native output": the suite's expectations
/// were frozen against the native reference, so a pass here is byte identity per case. The run is
/// recorded as the package's gate ② report.
#[test]
fn the_sandboxed_component_passes_gate_two_byte_for_byte() {
    let evidence = gate2_report::Evidence::capture(PACKAGE, component_wasm());
    let report = run_provider_component_suite_v1_for_manifest(
        &SandboxedComponentV1::new(load()),
        &pack(),
        &manifest(),
    );
    evidence.record(&report);
    for failure in report.failures() {
        eprintln!("{failure}");
    }
    assert!(report.is_passing(), "{report}");
}

/// The two builds agree check for check, not merely both-pass.
#[test]
fn the_sandboxed_and_native_reports_are_identical() {
    let (pack, manifest) = (pack(), manifest());
    let sandboxed = run_provider_component_suite_v1_for_manifest(
        &SandboxedComponentV1::new(load()),
        &pack,
        &manifest,
    );
    let native = run_provider_component_suite_v1_for_manifest(&NATIVE, &pack, &manifest);
    assert_eq!(format!("{:?}", sandboxed.outcomes()), format!("{:?}", native.outcomes()));
}

/// Gate ①: the manifest, the reported identity (sandboxed, native and manifest agree), the range
/// handshake of a host linking this release, and the exact tuple handshake.
#[test]
fn the_shipped_package_passes_gate_one_and_the_tuple_handshake() {
    let manifest = manifest();
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    host_range::assert_released_runtime(&manifest);
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));

    let component = SandboxedComponentV1::new(load());
    assert_eq!(component.metadata(), NATIVE.metadata());
    assert!(reported_identity_matches(&component.metadata(), &manifest));

    // The exact handshake, still supported for one release, with the runtime the manifest
    // declares (§8.6): one hex digit off is refused, the true values are accepted.
    let expectations = host_range::exact_expectations_for(&manifest);
    let off_by_one_digit = HostExpectationsV1 {
        kernel_revision: "c0bca10c2d9f9a439680865feb65c9edf598fc64".to_owned(),
        ..expectations.clone()
    };
    assert!(compatibility_matches(&manifest, &off_by_one_digit).is_err());
    assert_eq!(compatibility_matches(&manifest, &expectations), Ok(()));
}

/// The guest's answer as the shim's `Result<String, String>`: a component error is the envelope
/// the guest returned; any other runtime failure is a broken boundary, not a parity outcome.
fn guest(result: Result<String, CallErrorV1>) -> Result<String, String> {
    match result {
        Ok(json) => Ok(json),
        Err(CallErrorV1::Component(json)) => Err(json),
        Err(other) => panic!("the sandbox call failed outside the component: {other}"),
    }
}

/// The configuration refusals of record §4 (`vertex_conformance_v1::CONFIGURATION_REFUSALS`),
/// which are 500s: the guest gives the native answer byte for byte, before any refusal of the
/// request itself.
#[test]
fn the_configuration_refusals_are_byte_identical_to_the_native_reference() {
    let loaded = load();
    // No model and `OpenAI` reasoning replay: refused with 400 on a configured row.
    let request = r#"{"model":"","messages":[{"role":"assistant","content":"x","reasoning_replay_protocol_family":"openai-encrypted-reasoning"}]}"#;
    for (base_url, declared, key) in [
        (
            "https://us-central1-aiplatform.googleapis.com",
            r#"{"project_id":"test-project"}"#,
            "`region`",
        ),
        (
            "https://us-central1-aiplatform.googleapis.com",
            r#"{"region":"us-central1"}"#,
            "`project_id`",
        ),
        (
            "https://us-central1-aiplatform.googleapis.com",
            r#"{"project_id":"test-project","region":"europe-west4"}"#,
            "`europe-west4`",
        ),
        (
            "https://global-aiplatform.googleapis.com",
            r#"{"project_id":"test-project","region":"global"}"#,
            "`global`",
        ),
    ] {
        let config = format!(
            r#"{{"provider":"vertex-ai","base_url":"{base_url}","auth":"provider_api_key","declared":{declared}}}"#
        );
        let native = abi::build_http_request_json(&NATIVE, request, &config);
        assert_eq!(guest(loaded.call_build_http_request(request, &config)), native, "{config}");
        let error: Value = serde_json::from_str(&native.expect_err("a configuration refusal"))
            .expect("the refusal is JSON");
        assert_eq!(error["http_status"], 500, "{error}");
        assert!(error["message"].as_str().is_some_and(|message| message.contains(key)), "{error}");
    }
}

/// `provider-vertex` and `provider-gemini` ship distinct identities, families and packs.
#[test]
fn the_two_gemini_text_packages_are_not_the_same_package() {
    let vertex = manifest();
    let gemini: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/provider-gemini/manifest.json"))
            .expect("the other manifest reads"),
    )
    .expect("the other manifest parses");
    assert_ne!(vertex.name, gemini.name);
    assert!(vertex.providers.iter().all(|family| !gemini.providers.contains(family)));
    assert_ne!(vertex.conformance.fixtures, gemini.conformance.fixtures);
}
