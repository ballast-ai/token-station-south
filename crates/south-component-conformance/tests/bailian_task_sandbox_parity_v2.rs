//! Task-v2 native and real-WASM parity against the same frozen fixtures.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use south_component_conformance::sandbox_task_v2::SandboxedTaskComponentV2;
use south_component_conformance::{
    TaskFixturePackV2, reference_bailian_task_v2::BailianTaskComponentV2,
    run_task_component_suite_v2,
};
use south_provider_api::HostExpectationsV1;
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
            .arg(repo_root().join("scripts/build-bailian-task-v2-component.sh"))
            .status()
            .expect("bash is on PATH");
        assert!(
            status.success(),
            "the Bailian task component must build; run `rustup target add wasm32-wasip2` if the \
             target is missing"
        );
        repo_root()
            .join("components/task-bailian-v2/target/wasm32-wasip2/release/task_bailian_v2.wasm")
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

fn pack() -> TaskFixturePackV2 {
    TaskFixturePackV2::load(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures-bailian-task-v2"
    )))
    .expect("the shipped pack loads")
}

fn sandboxed() -> SandboxedTaskComponentV2 {
    let manifest =
        std::fs::read_to_string(repo_root().join("components/task-bailian-v2/manifest.json"))
            .expect("manifest reads");
    let wasm = std::fs::read(component_wasm()).expect("component reads");

    let runtime = ComponentRuntimeV1::new(RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_secs(5),
        max_payload_bytes: 4 * 1024 * 1024,
    })
    .expect("engine builds");
    let loaded =
        LoadedComponentV1::load_embedded(&runtime, &manifest, &wasm, &expectations(), FixedSigner)
            .expect("the shipped package loads");
    SandboxedTaskComponentV2::new(loaded).map_err(|_| ()).expect("it declares the task world")
}

/// S3: the sandboxed component passes the same gate ② the native reference
/// does, against the same frozen pack.
#[test]
fn the_sandboxed_component_passes_gate_two_byte_for_byte() {
    let report = run_task_component_suite_v2(&sandboxed(), &pack());
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
    let sandboxed = run_task_component_suite_v2(&sandboxed(), &pack);
    let native = run_task_component_suite_v2(&BailianTaskComponentV2, &pack);
    assert_eq!(
        format!("{:?}", sandboxed.outcomes()),
        format!("{:?}", native.outcomes()),
        "the wasm build and the native build must agree check for check"
    );
}

#[test]
fn the_v1_typed_seam_refuses_a_loaded_v2_component() {
    let manifest =
        std::fs::read_to_string(repo_root().join("components/task-bailian-v2/manifest.json"))
            .expect("manifest");
    let wasm = std::fs::read(component_wasm()).expect("wasm");
    let runtime = ComponentRuntimeV1::new(RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_secs(5),
        max_payload_bytes: 4 * 1024 * 1024,
    })
    .expect("runtime");
    let loaded =
        LoadedComponentV1::load_embedded(&runtime, &manifest, &wasm, &expectations(), FixedSigner)
            .expect("v2 loads");
    let returned = south_component_conformance::sandbox::SandboxedTaskComponentV1::new(loaded)
        .expect_err("v1 seam rejects v2");
    assert!(
        SandboxedTaskComponentV2::new(*returned).is_ok(),
        "the refused component stays usable through its exact world"
    );
}

#[test]
fn async_header_and_direct_artifact_match_the_manifest_contract() {
    use serde_json::json;
    use south_component_conformance::TaskComponentV2;
    use south_contracts::HostMintedValuesV1;
    use token_station_protocol::{HttpResponseParts, ProviderConfig};
    let c = sandboxed();
    let cfg:ProviderConfig=serde_json::from_value(json!({"provider":"bailian","base_url":"https://dashscope.example","auth":"provider_api_key"})).unwrap();
    let p = c
        .build_submit_request(
            &cfg,
            &json!({"model":"wan2.7-t2v","prompt":"scene"}),
            &HostMintedValuesV1::new("host-1", None).unwrap(),
        )
        .unwrap();
    cfg.authorize(&p.descriptor).unwrap();
    assert_eq!(p.descriptor.headers.get("x-dashscope-async"), Some("enable"));
    let q = c.build_observe_request(&cfg, "wan2.7-t2v", "original-001", &p.locator).unwrap();
    cfg.authorize(&q).unwrap();
    assert!(q.headers.get("x-dashscope-async").is_none());
    let response:HttpResponseParts=serde_json::from_value(json!({"status":200,"body":json!({"output":{"task_status":"SUCCEEDED","video_url":"https://media.example/a"},"usage":{"duration":4}}).to_string()})).unwrap();
    let obs = c.parse_observation(&response).unwrap();
    assert!(c.build_artifact_request(&cfg, &p.locator, &obs).unwrap().is_none());
    let m: south_provider_api::ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/task-bailian-v2/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(c.metadata(), BailianTaskComponentV2.metadata());
    assert!(!m.capabilities.contains("artifact_fetch"));
    assert_eq!(m.name, c.metadata().name);
}
