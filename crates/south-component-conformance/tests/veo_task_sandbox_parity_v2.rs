//! Task-v2 native and real-WASM parity against the same frozen fixtures.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use south_component_conformance::sandbox_task_v2::SandboxedTaskComponentV2;
use south_component_conformance::{
    TaskFixturePackV2, reference_veo_task_v2::VeoTaskComponentV2, run_task_component_suite_v2,
};
use south_provider_runtime::{
    ComponentRuntimeV1, LoadedComponentV1, RuntimeLimitsV1, SecretSignerV1,
};

#[path = "support/gate2_report.rs"]
mod gate2_report;
#[path = "support/host_range.rs"]
mod host_range;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

fn component_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let status = Command::new("bash")
            .arg(repo_root().join("scripts/build-veo-task-v2-component.sh"))
            .status()
            .expect("bash is on PATH");
        assert!(
            status.success(),
            "the Google Veo video (Gemini API) component must build; run `rustup target add wasm32-wasip2` if the \
             target is missing"
        );
        repo_root().join("components/task-veo-v2/target/wasm32-wasip2/release/task_veo_v2.wasm")
    })
}

struct FixedSigner;

impl SecretSignerV1 for FixedSigner {
    fn sign(&self, _: &str, _: &[u8], _: &str) -> Result<Vec<u8>, String> {
        Ok(vec![0xAB; 32])
    }
}

fn pack() -> TaskFixturePackV2 {
    TaskFixturePackV2::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures-veo-task-v2")))
        .expect("the shipped pack loads")
}

fn sandboxed() -> SandboxedTaskComponentV2 {
    let manifest =
        std::fs::read_to_string(repo_root().join("components/task-veo-v2/manifest.json"))
            .expect("manifest reads");
    let wasm = std::fs::read(component_wasm()).expect("component reads");

    let runtime = ComponentRuntimeV1::new(RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_secs(5),
        max_payload_bytes: 4 * 1024 * 1024,
    })
    .expect("engine builds");
    let loaded = LoadedComponentV1::load_embedded(
        &runtime,
        &manifest,
        &wasm,
        &host_range::host_range(),
        FixedSigner,
    )
    .expect("the shipped package loads");
    SandboxedTaskComponentV2::new(loaded).map_err(|_| ()).expect("it declares the task world")
}

/// S3: the sandboxed component passes the same gate ② the native reference
/// does, against the same frozen pack.
#[test]
fn the_sandboxed_component_passes_gate_two_byte_for_byte() {
    let evidence = gate2_report::Evidence::capture("task-veo-v2", component_wasm());
    let report = run_task_component_suite_v2(&sandboxed(), &pack());
    evidence.record(&report);
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
    let native = run_task_component_suite_v2(&VeoTaskComponentV2, &pack);
    assert_eq!(
        format!("{:?}", sandboxed.outcomes()),
        format!("{:?}", native.outcomes()),
        "the wasm build and the native build must agree check for check"
    );
}

#[test]
fn the_v1_typed_seam_refuses_a_loaded_v2_component() {
    let manifest =
        std::fs::read_to_string(repo_root().join("components/task-veo-v2/manifest.json"))
            .expect("manifest");
    let wasm = std::fs::read(component_wasm()).expect("wasm");
    let runtime = ComponentRuntimeV1::new(RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_secs(5),
        max_payload_bytes: 4 * 1024 * 1024,
    })
    .expect("runtime");
    let loaded = LoadedComponentV1::load_embedded(
        &runtime,
        &manifest,
        &wasm,
        &host_range::host_range(),
        FixedSigner,
    )
    .expect("v2 loads");
    let returned = south_component_conformance::sandbox::SandboxedTaskComponentV1::new(loaded)
        .expect_err("v1 seam rejects v2");
    assert!(
        SandboxedTaskComponentV2::new(*returned).is_ok(),
        "the refused component stays usable through its exact world"
    );
}
#[test]
fn the_packaged_component_uses_header_auth_and_marks_credential_gated_artifacts() {
    use serde_json::json;
    use south_component_conformance::{SubmitOutcomeV2, TaskComponentV2};
    use south_contracts::{HostMintedValuesV1, TaskArtifactRefV2, TaskObservationV2};
    use token_station_protocol::{Auth, HttpResponseParts, ProviderConfig};
    let c = sandboxed();
    let cfg: ProviderConfig = serde_json::from_value(json!({"provider":"veo-video",
        "base_url":"https://generativelanguage.example","auth":"provider_api_key"}))
    .unwrap();
    let p = c
        .build_submit_request(
            &cfg,
            &json!({"model":"veo-3.1-generate-preview","prompt":"scene","duration":8,"n":2}),
            &HostMintedValuesV1::new("host-1", None).unwrap(),
        )
        .unwrap();
    cfg.authorize(&p.descriptor).unwrap();
    assert!(
        matches!(&p.descriptor.auth, Some(Auth::Header { name, .. }) if name == "x-goog-api-key")
    );
    assert_eq!(p.request_estimate.requested_outputs(), Some(2));
    let q = c
        .build_observe_request(
            &cfg,
            "veo",
            "models/veo-3.1-generate-preview/operations/op-1",
            &p.locator,
        )
        .unwrap();
    cfg.authorize(&q).unwrap();
    assert!(matches!(&q.auth, Some(Auth::Header { name, .. }) if name == "x-goog-api-key"));
    let done = json!({"done":true,"response":{"generateVideoResponse":{"generatedSamples":[
        {"video":{"uri":"https://generativelanguage.example/v1beta/files/a:download"}}]}}});
    let parts: HttpResponseParts =
        serde_json::from_value(json!({"status":200,"body":done.to_string()})).unwrap();
    let SubmitOutcomeV2::AcceptedTerminal(TaskObservationV2::Succeeded { artifacts, usage }) =
        c.parse_submit_response(&parts).unwrap()
    else {
        panic!("a terminal create body is accepted-terminal")
    };
    let TaskArtifactRefV2::Urls(items) = artifacts else { panic!("urls expected") };
    assert!(items.iter().all(south_contracts::TaskArtifactV2::fetch_with_credential));
    assert_eq!(usage.outputs(), Some(1));
    let m: south_provider_api::ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/task-veo-v2/manifest.json")).unwrap(),
    )
    .unwrap();
    assert!(m.auth_arms.contains("header_secret") && !m.auth_arms.contains("bearer"));
    assert!(m.providers.iter().any(|p| p == "veo-video"));
}
