//! Task-v2 native and real-WASM parity against the same frozen fixtures.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use south_component_conformance::sandbox_task_v2::SandboxedTaskComponentV2;
use south_component_conformance::{
    TaskFixturePackV2, reference_minimax_task_v2::MiniMaxTaskReferenceV2,
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
            .arg(repo_root().join("scripts/build-minimax-task-v2-component.sh"))
            .status()
            .expect("bash is on PATH");
        assert!(
            status.success(),
            "the MiniMax task component must build; run `rustup target add wasm32-wasip2` if the \
             target is missing"
        );
        repo_root()
            .join("components/task-minimax-v2/target/wasm32-wasip2/release/task_minimax_v2.wasm")
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
        "/fixtures-minimax-task-v2"
    )))
    .expect("the shipped pack loads")
}

fn sandboxed() -> SandboxedTaskComponentV2 {
    let manifest =
        std::fs::read_to_string(repo_root().join("components/task-minimax-v2/manifest.json"))
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
    let native = run_task_component_suite_v2(&MiniMaxTaskReferenceV2, &pack);
    assert_eq!(
        format!("{:?}", sandboxed.outcomes()),
        format!("{:?}", native.outcomes()),
        "the wasm build and the native build must agree check for check"
    );
}

#[test]
fn the_v1_typed_seam_refuses_a_loaded_v2_component() {
    let manifest =
        std::fs::read_to_string(repo_root().join("components/task-minimax-v2/manifest.json"))
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
fn request_estimates_and_authorized_queries_match_native() {
    use serde_json::json;
    use south_component_conformance::TaskComponentV2;
    use south_contracts::HostMintedValuesV1;
    use token_station_protocol::ProviderConfig;
    let component = sandboxed();
    let config: ProviderConfig = serde_json::from_value(json!({"provider":"minimax","base_url":"https://api.minimaxi.com","auth":"provider_api_key","group_id":"19000"})).unwrap();
    let minted = HostMintedValuesV1::new("estimate-parity", None).unwrap();
    for model in ["MiniMax-Hailuo-02", "MiniMax-Hailuo-2.3", "MiniMax-Hailuo-2.3-Fast"] {
        for duration in [json!(6), json!("10.0"), json!(-1), json!("NaN")] {
            let request = json!({"model":model,"prompt":"scene","duration":duration});
            let native = MiniMaxTaskReferenceV2.build_submit_request(&config, &request, &minted);
            let guest = component.build_submit_request(&config, &request, &minted);
            assert_eq!(guest, native);
            if let Ok(prepared) = guest {
                config.authorize(&prepared.descriptor).unwrap();
                assert_eq!(prepared.request_estimate.milliunits_per_second(), None);
                let query = component
                    .build_observe_request(&config, model, "00123", &prepared.locator)
                    .unwrap();
                config.authorize(&query).unwrap();
            }
        }
    }
}
