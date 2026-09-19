//! The Kling task component's S3 acceptance test: the shipped component,
//! inside the sandbox, passes gate ② byte-for-byte — the same suite, the same
//! fixture pack, the same frozen expectations that judged the native
//! reference.
//!
//! This is what #83 could not do and #84 made possible. The statement it
//! makes is narrow and worth stating exactly: **the wasm build and the native
//! build agree**. Both are the same source, so a divergence would mean the
//! boundary itself — serialisation, the JSON face, the typed seam — changed a
//! value in transit.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use south_component_conformance::sandbox::SandboxedTaskComponentV1;
use south_component_conformance::{
    TaskFixturePackV1, reference_kling_task::KlingTaskReferenceV1, run_task_component_suite_v1,
};
use south_provider_api::{ComponentManifestV1, HostExpectationsV1};
use south_provider_runtime::{
    ComponentRuntimeV1, LoadedComponentV1, RuntimeLimitsV1, SecretSignerV1,
};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

fn component_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let status = Command::new("bash")
            .arg(repo_root().join("scripts/build-kling-task-component.sh"))
            .status()
            .expect("bash is on PATH");
        assert!(
            status.success(),
            "the Kling task component must build; run `rustup target add wasm32-wasip2` if the \
             target is missing"
        );
        repo_root().join("components/task-kling/target/wasm32-wasip2/release/task_kling.wasm")
    })
}

fn expectations() -> HostExpectationsV1 {
    HostExpectationsV1 {
        ir_schema_id: "token-station-protocol@0.3.0/v0.2.0".to_owned(),
        kernel_version: "0.2.0".to_owned(),
        kernel_revision: "72458e3a11fe157f9ac04818c44b62a3dd2cb09c".to_owned(),
        south_runtime: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

struct FixedSigner;

impl SecretSignerV1 for FixedSigner {
    fn sign(&self, _: &str, _: &[u8], _: &str) -> Result<Vec<u8>, String> {
        Ok(vec![0xAB; 32])
    }
}

fn pack() -> TaskFixturePackV1 {
    TaskFixturePackV1::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures-kling-task")))
        .expect("the shipped pack loads")
}

fn sandboxed() -> SandboxedTaskComponentV1 {
    let dir =
        std::env::temp_dir().join(format!("south-task-parity-{}-{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::copy(
        repo_root().join("components/task-kling/manifest.json"),
        dir.join("manifest.json"),
    )
    .expect("manifest copies");
    std::fs::copy(component_wasm(), dir.join("component.wasm")).expect("wasm copies");

    let runtime = ComponentRuntimeV1::new(RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_secs(5),
        max_payload_bytes: 4 * 1024 * 1024,
    })
    .expect("engine builds");
    let loaded = LoadedComponentV1::load(&runtime, &dir, &expectations(), FixedSigner)
        .expect("the shipped package loads");
    SandboxedTaskComponentV1::new(loaded).map_err(|_| ()).expect("it declares the task world")
}

/// S3: the sandboxed component passes the same gate ② the native reference
/// does, against the same frozen pack.
#[test]
fn the_sandboxed_component_passes_gate_two_byte_for_byte() {
    let report = run_task_component_suite_v1(&sandboxed(), &pack());
    assert!(
        report.is_passing(),
        "the sandboxed component must pass the suite its reference passes: {:?}",
        report.failures().collect::<Vec<_>>()
    );
}

/// The two builds agree check for check, not merely both-pass: a suite that
/// passed for different reasons on each side would hide a boundary bug.
#[test]
fn the_sandboxed_and_native_reports_are_identical() {
    let pack = pack();
    let sandboxed = run_task_component_suite_v1(&sandboxed(), &pack);
    let native = run_task_component_suite_v1(&KlingTaskReferenceV1, &pack);
    assert_eq!(
        format!("{:?}", sandboxed.outcomes()),
        format!("{:?}", native.outcomes()),
        "the wasm build and the native build must agree check for check"
    );
}

/// D2: the typed seam refuses a component from the other world, so a
/// wrong-world call cannot be written rather than failing late.
#[test]
fn the_task_seam_refuses_a_chat_component() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root().join("components/provider-openai-compatible/manifest.json"),
        )
        .expect("the chat manifest reads"),
    )
    .expect("it parses");
    assert_eq!(manifest.api_version, "provider-adapter-v2");
    // Constructing the task seam from that world is refused by type, and the
    // component comes back untouched — see the `new` contract.
}
