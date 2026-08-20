//! The S3 acceptance test: the official reference component, inside the
//! sandbox, passes gate ② byte-for-byte — the same suite, the same fixture
//! pack, the same frozen expectations that judged the native reference.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::sandbox::SandboxedComponentV1;
use south_component_conformance::{
    FixturePackV1, HostExpectationsV1, ProviderComponentV1, accepts_manifest,
    compatibility_matches, reported_identity_matches, run_provider_component_suite_v1,
};
use south_provider_api::ComponentManifestV1;
use south_provider_runtime::{ComponentRuntimeV1, NoSecretsV1, RuntimeLimitsV1};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

/// Builds the official component once per test process.
fn component_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let status = Command::new("bash")
            .arg(repo_root().join("scripts/build-reference-component.sh"))
            .status()
            .expect("bash is on PATH");
        assert!(
            status.success(),
            "the official component must build; run `rustup target add wasm32-wasip2` if the \
             target is missing"
        );
        repo_root().join(
            "components/provider-openai-compatible/target/wasm32-wasip2/release/provider_openai_compatible.wasm",
        )
    })
}

fn shipped_manifest() -> (String, ComponentManifestV1) {
    let source = std::fs::read_to_string(
        repo_root().join("components/provider-openai-compatible/manifest.json"),
    )
    .expect("the shipped manifest reads");
    let manifest: ComponentManifestV1 =
        serde_json::from_str(&source).expect("the shipped manifest parses");
    (source, manifest)
}

fn sandboxed() -> SandboxedComponentV1 {
    let runtime = ComponentRuntimeV1::new(RuntimeLimitsV1::default()).expect("engine builds");
    let wasm = std::fs::read(component_wasm()).expect("the component reads");
    let (source, _) = shipped_manifest();
    let loaded = south_provider_runtime::LoadedComponentV1::load_embedded(
        &runtime,
        &source,
        &wasm,
        NoSecretsV1,
    )
    .expect("the official package passes every load gate");
    SandboxedComponentV1::new(loaded)
}

/// Gate ② inside the sandbox: the run that proves "the sandboxed output
/// equals the native output" — the suite's expectations were frozen against
/// the native reference, so a pass here is byte-identity per case.
#[test]
fn the_sandboxed_component_passes_gate_two_byte_for_byte() {
    let pack = FixturePackV1::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures"))
        .expect("the shipped fixture pack loads");
    let component = sandboxed();

    let report = run_provider_component_suite_v1(&component, &pack);
    for failure in report.failures() {
        eprintln!("{failure}");
    }
    assert!(report.is_passing(), "{report}");
}

/// Gate ① against the shipped package: manifest, identity (native and
/// sandboxed agree, both with the manifest), and the tuple handshake.
#[test]
fn the_shipped_package_passes_gate_one_and_the_tuple_handshake() {
    let (_, manifest) = shipped_manifest();
    assert_eq!(accepts_manifest(&manifest), Ok(()));

    let component = sandboxed();
    assert!(reported_identity_matches(&component.metadata(), &manifest));
    assert_eq!(component.metadata(), OpenAiCompatibleReferenceV1.metadata());

    let expectations = HostExpectationsV1 {
        ir_schema_id: "token-station-protocol@0.3.0/v0.2.0".to_owned(),
        kernel_version: "0.2.0".to_owned(),
        kernel_revision: "72458e3a11fe157f9ac04818c44b62a3dd2cb00c".to_owned(),
        south_runtime: "0.10.0".to_owned(),
    };
    // Deliberately one hex digit off first: the handshake must refuse …
    assert!(compatibility_matches(&manifest, &expectations).is_err());
    // … and accept the true values.
    let expectations = HostExpectationsV1 {
        kernel_revision: "72458e3a11fe157f9ac04818c44b62a3dd2cb09c".to_owned(),
        ..expectations
    };
    assert_eq!(compatibility_matches(&manifest, &expectations), Ok(()));
}
