//! Real task-v2 package admission, world separation and runtime limits.

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
            .arg(repo_root().join("scripts/build-kling-task-v2-component.sh"))
            .status()
            .expect("bash is on PATH");
        assert!(
            status.success(),
            "the Kling task component must build; run `rustup target add wasm32-wasip2` if the \
             target is missing"
        );
        repo_root().join("components/task-kling-v2/target/wasm32-wasip2/release/task_kling_v2.wasm")
    })
}

fn shipped_task_manifest() -> String {
    std::fs::read_to_string(repo_root().join("components/task-kling-v2/manifest.json"))
        .expect("the shipped manifest reads")
}

fn package(name: &str, manifest: &str, wasm: &Path) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("south-task-v2-{}-{seq}-{name}", std::process::id()));
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

#[test]
fn the_task_v2_guest_loads_without_replacing_v1() {
    let dir = package("v2", &shipped_task_manifest(), task_guest_wasm());
    let loaded = LoadedComponentV1::load(&runtime(), &dir, &expectations(), FixedSigner)
        .expect("the task-v2 package loads");
    assert_eq!(loaded.metadata().api_version, "task-adapter-v2");
    assert_eq!(loaded.metadata().name, "task-kling-v2");
}

#[test]
fn task_v2_refuses_signing_imports_before_world_instantiation() {
    for name in [
        "token-station:adapter/host@2.0.0",
        "token-station:task-adapter/host@1.0.0",
        "token-station:task-adapter/host@2.0.0",
    ] {
        let wat = format!("(component (import \"{name}\" (instance)))");
        let error = LoadedComponentV1::load_embedded(
            &runtime(),
            &shipped_task_manifest(),
            wat.as_bytes(),
            &expectations(),
            FixedSigner,
        )
        .expect_err("task-v2 is not granted signing imports");
        assert!(
            matches!(error, south_provider_runtime::LoadErrorV1::ForbiddenImport(ref actual) if actual == name),
            "the import itself must be refused, before a missing export could disguise the permission gap: {error}"
        );
    }
}

fn load_with_limits(limits: RuntimeLimitsV1) -> LoadedComponentV1 {
    let runtime = ComponentRuntimeV1::new(limits).expect("runtime");
    let wasm = std::fs::read(task_guest_wasm()).expect("guest bytes");
    LoadedComponentV1::load_embedded(
        &runtime,
        &shipped_task_manifest(),
        &wasm,
        &expectations(),
        FixedSigner,
    )
    .expect("v2 loads")
}

#[test]
fn task_v2_calls_refuse_the_v1_and_provider_faces() {
    let dir = package("faces", &shipped_task_manifest(), task_guest_wasm());
    let loaded =
        LoadedComponentV1::load(&runtime(), &dir, &expectations(), FixedSigner).expect("v2");
    assert!(matches!(
        loaded.call_parse_observation("{}"),
        Err(south_provider_runtime::CallErrorV1::Trap(_))
    ));
    assert!(matches!(
        loaded.call_parse_response("{}"),
        Err(south_provider_runtime::CallErrorV1::Trap(_))
    ));
}

#[test]
fn task_v2_bytes_cannot_claim_the_v1_world_or_a_different_identity() {
    let source = shipped_task_manifest();
    let mut manifest: serde_json::Value = serde_json::from_str(&source).expect("manifest");
    manifest["api_version"] = "task-adapter-v1".into();
    manifest["compatibility"]["wit_package"] = "token-station:task-adapter@1.0.0".into();
    manifest["conformance"]["required_suite"] = "south.task-component.v1".into();
    let bytes = std::fs::read(task_guest_wasm()).expect("guest");
    assert!(
        LoadedComponentV1::load_embedded(
            &runtime(),
            &manifest.to_string(),
            &bytes,
            &expectations(),
            FixedSigner
        )
        .is_err()
    );
    let mut manifest: serde_json::Value = serde_json::from_str(&source).expect("manifest");
    manifest["version"] = "9.9.9".into();
    let error = LoadedComponentV1::load_embedded(
        &runtime(),
        &manifest.to_string(),
        &bytes,
        &expectations(),
        FixedSigner,
    )
    .expect_err("identity mismatch");
    assert!(format!("{error}").contains("not what it claims"));
}

#[test]
fn task_v2_admission_keeps_exact_runtime_compatibility() {
    let mut host = expectations();
    host.south_runtime = "0.0.0".into();
    let bytes = std::fs::read(task_guest_wasm()).expect("guest");
    let error = LoadedComponentV1::load_embedded(
        &runtime(),
        &shipped_task_manifest(),
        &bytes,
        &host,
        FixedSigner,
    )
    .expect_err("host tuple must not come from manifest");
    assert!(format!("{error}").contains("south runtime"));
}

#[test]
fn task_v2_bounds_both_input_and_guest_error_output() {
    use south_provider_runtime::CallErrorV1;
    let loaded = load_with_limits(RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_secs(5),
        max_payload_bytes: 2,
    });
    assert!(matches!(
        loaded.call_parse_observation_v2("oversized"),
        Err(CallErrorV1::PayloadTooLarge { limit: 2 })
    ));
    // Input fits exactly. Parsing an incomplete HttpResponseParts produces a
    // guest error envelope, whose output must independently hit the bound.
    assert_eq!("{}".len(), 2);
    assert!(matches!(
        loaded.call_parse_observation_v2("{}"),
        Err(CallErrorV1::PayloadTooLarge { limit: 2 })
    ));
}

#[test]
fn task_v2_bounds_prepared_output_even_when_each_input_fits() {
    use south_provider_runtime::CallErrorV1;
    let config = r#"{"provider":"kling","base_url":"https://api.kling.example","models":[]}"#;
    let request =
        r#"{"operation":"text-or-image","model":"kling-v1","prompt":"a cinematic scene"}"#;
    let minted = r#"{"task_id":"task-v2-output-limit"}"#;
    let normal = load_with_limits(RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_secs(5),
        max_payload_bytes: 4 * 1024 * 1024,
    });
    let prepared =
        normal.call_build_submit_request_v2(config, request, minted).expect("valid prepare");
    let limit = [config.len(), request.len(), minted.len()].into_iter().max().expect("inputs");
    assert!(prepared.len() > limit, "the output, not an input, must exceed the limit");
    let bounded = load_with_limits(RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_secs(5),
        max_payload_bytes: limit,
    });
    assert!(
        matches!(bounded.call_build_submit_request_v2(config,request,minted), Err(CallErrorV1::PayloadTooLarge{limit:actual}) if actual==limit)
    );
}

#[test]
fn both_task_versions_load_in_one_runtime_and_keep_separate_faces() {
    let status = Command::new("bash")
        .arg(repo_root().join("scripts/build-kling-task-component.sh"))
        .status()
        .expect("build v1");
    assert!(status.success());
    let v1_wasm = std::fs::read(
        repo_root().join("components/task-kling/target/wasm32-wasip2/release/task_kling.wasm"),
    )
    .expect("v1 wasm");
    let v1_manifest =
        std::fs::read_to_string(repo_root().join("components/task-kling/manifest.json"))
            .expect("v1 manifest");
    let v2_wasm = std::fs::read(task_guest_wasm()).expect("v2 wasm");
    let runtime = runtime();
    let v1 = LoadedComponentV1::load_embedded(
        &runtime,
        &v1_manifest,
        &v1_wasm,
        &expectations(),
        FixedSigner,
    )
    .expect("v1 coexists");
    let v2 = LoadedComponentV1::load_embedded(
        &runtime,
        &shipped_task_manifest(),
        &v2_wasm,
        &expectations(),
        FixedSigner,
    )
    .expect("v2 coexists");
    assert_eq!(v1.metadata().api_version, "task-adapter-v1");
    assert_eq!(v2.metadata().api_version, "task-adapter-v2");
    assert!(matches!(
        v1.call_parse_observation_v2("{}"),
        Err(south_provider_runtime::CallErrorV1::Trap(_))
    ));
    assert!(matches!(
        v2.call_parse_observation("{}"),
        Err(south_provider_runtime::CallErrorV1::Trap(_))
    ));
}
