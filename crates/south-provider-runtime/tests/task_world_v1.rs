//! The runtime hosting its second world.
//!
//! Design record: `docs/design/2026-09-19-runtime-second-world.md`.
//!
//! The guest under test is the **shipped** `task-kling` component, not a
//! purpose-built fixture: the thing worth proving is that the package this
//! repository publishes can actually be loaded, which a hand-written guest
//! would not establish.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use south_provider_api::HostExpectationsV1;
use south_provider_runtime::{
    ComponentRuntimeV1, LoadedComponentV1, RuntimeLimitsV1, SecretSignerV1,
};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

/// Builds the shipped Kling task guest once per test process.
fn task_guest_wasm() -> &'static Path {
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

fn shipped_task_manifest() -> String {
    std::fs::read_to_string(repo_root().join("components/task-kling/manifest.json"))
        .expect("the shipped manifest reads")
}

fn package(name: &str, manifest: &str, wasm: &Path) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("south-task-{}-{seq}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir is writable");
    std::fs::write(dir.join("manifest.json"), manifest).expect("manifest writes");
    std::fs::copy(wasm, dir.join("component.wasm")).expect("wasm copies");
    dir
}

fn runtime() -> ComponentRuntimeV1 {
    ComponentRuntimeV1::new(RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_millis(500),
        max_payload_bytes: 1024 * 1024,
    })
    .expect("engine builds")
}

struct FixedSigner;

impl SecretSignerV1 for FixedSigner {
    fn sign(&self, _: &str, _: &[u8], _: &str) -> Result<Vec<u8>, String> {
        Ok(vec![0xAB; 32])
    }
}

fn expectations() -> HostExpectationsV1 {
    HostExpectationsV1 {
        ir_schema_id: "token-station-protocol@0.3.0/v0.2.0".to_owned(),
        kernel_version: "0.2.0".to_owned(),
        kernel_revision: "72458e3a11fe157f9ac04818c44b62a3dd2cb09c".to_owned(),
        south_runtime: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

/// The gap #83 stopped at: the guest it ships can now be loaded.
#[test]
fn the_shipped_task_guest_loads_and_reports_its_identity() {
    let dir = package("ok", &shipped_task_manifest(), task_guest_wasm());
    let loaded = LoadedComponentV1::load(&runtime(), &dir, &expectations(), FixedSigner)
        .expect("the shipped task package loads");
    assert_eq!(loaded.manifest().api_version, "task-adapter-v1");
    assert_eq!(loaded.manifest().name, "task-kling");
}

/// The identity gate runs for a task component exactly as it does for a chat
/// one — the probe goes through the declared world's accessor, so a package
/// lying about its version is still caught.
#[test]
fn the_identity_gate_runs_in_the_task_world_too() {
    let lying = shipped_task_manifest().replace("\"version\": \"1.0.0\"", "\"version\": \"9.9.9\"");
    let dir = package("lying", &lying, task_guest_wasm());
    let error = LoadedComponentV1::load(&runtime(), &dir, &expectations(), FixedSigner)
        .expect_err("a package that lies about its version is refused");
    assert!(
        format!("{error}").contains("not what it claims"),
        "the refusal must name the identity mismatch, got: {error}"
    );
}

/// A task guest declaring the chat world does not load: the bytes export one
/// world and the manifest names another, and instantiation is what notices.
#[test]
fn a_task_guest_declaring_the_chat_world_is_refused() {
    let mislabelled = shipped_task_manifest()
        .replace("task-adapter-v1", "provider-adapter-v2")
        .replace("token-station:task-adapter@1.0.0", "token-station:adapter@2.0.0")
        .replace("south.task-component.v1", "south.provider-component.v1")
        .replace("\"submit\",\n    \"observe\",\n    \"render\"", "\"chat\"");
    let dir = package("mislabelled", &mislabelled, task_guest_wasm());
    let error = LoadedComponentV1::load(&runtime(), &dir, &expectations(), FixedSigner)
        .expect_err("bytes and manifest must agree about the world");
    let rendered = format!("{error}");
    assert!(
        rendered.contains("provider-adapter-v2"),
        "the refusal names the world the manifest declared, got: {rendered}"
    );
}
