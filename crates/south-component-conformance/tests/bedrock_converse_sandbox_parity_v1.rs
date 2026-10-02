//! The Bedrock Converse component's sandbox acceptance test over its frozen
//! fixture pack.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use south_component_conformance::reference_bedrock_converse::BedrockConverseReferenceV1;
use south_component_conformance::sandbox::SandboxedComponentV1;
use south_component_conformance::{
    FixturePackV1, ProviderComponentV1, accepts_manifest, reported_identity_matches,
    run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::{ComponentManifestV1, HostExpectationsV1};
use south_provider_runtime::{ComponentRuntimeV1, NoSecretsV1, RuntimeLimitsV1};

#[path = "support/gate2_report.rs"]
mod gate2_report;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

fn component_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let status = Command::new("bash")
            .arg(repo_root().join("scripts/build-bedrock-converse-component.sh"))
            .status()
            .expect("bash is on PATH");
        assert!(status.success(), "the Bedrock Converse component must build");
        repo_root().join(
            "components/provider-bedrock-converse/target/wasm32-wasip2/release/provider_bedrock_converse.wasm",
        )
    })
}

fn shipped_manifest() -> (String, ComponentManifestV1) {
    let source = std::fs::read_to_string(
        repo_root().join("components/provider-bedrock-converse/manifest.json"),
    )
    .expect("the shipped manifest reads");
    let manifest = serde_json::from_str(&source).expect("the shipped manifest parses");
    (source, manifest)
}

fn host_expectations() -> HostExpectationsV1 {
    HostExpectationsV1 {
        ir_schema_id: "token-station-protocol@0.4.0/v0.3.0".to_owned(),
        kernel_version: "0.3.0".to_owned(),
        kernel_revision: "6822aab1dea54ef646cb2206595cd4955ff9764a".to_owned(),
        south_runtime: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

fn sandboxed() -> SandboxedComponentV1 {
    let runtime = ComponentRuntimeV1::new(RuntimeLimitsV1::default()).expect("engine builds");
    let wasm = std::fs::read(component_wasm()).expect("the component reads");
    let (source, _) = shipped_manifest();
    let loaded = south_provider_runtime::LoadedComponentV1::load_embedded(
        &runtime,
        &source,
        &wasm,
        &host_expectations(),
        NoSecretsV1,
    )
    .expect("the shipped package passes every load gate");
    SandboxedComponentV1::new(loaded)
}

fn shipped_pack() -> FixturePackV1 {
    FixturePackV1::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures-bedrock-converse"))
        .expect("the shipped fixture pack loads")
}

#[test]
fn the_sandboxed_component_passes_gate_two_byte_for_byte() {
    let evidence = gate2_report::Evidence::capture("provider-bedrock-converse", component_wasm());
    let component = sandboxed();
    let report = run_provider_component_suite_v1_for_manifest(
        &component,
        &shipped_pack(),
        &shipped_manifest().1,
    );
    evidence.record(&report);
    for failure in report.failures() {
        eprintln!("{failure}");
    }
    assert!(report.is_passing(), "{report}");
}

#[test]
fn the_shipped_package_passes_gate_one_and_the_tuple_handshake() {
    let (_, manifest) = shipped_manifest();
    assert_eq!(accepts_manifest(&manifest), Ok(()));

    let component = sandboxed();
    assert!(reported_identity_matches(&component.metadata(), &manifest));
    assert_eq!(component.metadata(), BedrockConverseReferenceV1.metadata());
}
