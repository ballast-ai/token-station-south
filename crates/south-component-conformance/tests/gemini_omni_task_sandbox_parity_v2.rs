//! `task-gemini-omni-v2`: native and real-WASM parity against the same frozen fixtures, and the
//! host half of task contract 8 driven end to end through the sandbox (elide, parse, resolve,
//! render, fill), with a clip whose bytes never enter the sandbox.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::json;
use south_component_conformance::reference_gemini_omni_task_v2::GeminiOmniTaskComponentV2;
use south_component_conformance::sandbox_task_v2::SandboxedTaskComponentV2;
use south_component_conformance::{
    SubmitOutcomeV2, TaskComponentV2, TaskFixturePackV2, run_task_component_suite_v2_for_manifest,
};
use south_contracts::media::{BlobSetV1, MediaLimitsV1, PathPatternV1, elide_v1, encode_base64};
use south_contracts::{
    HostMintedValuesV1, TaskArtifactRefV2, TaskObservationV2, TaskRenderContextV2,
    TaskTokenBucketV2, fill_task_artifact_markers_v1, resolve_task_inline_artifact_v1,
};
use south_provider_api::{ComponentManifestV1, HostRangeV1, compatibility_admits};
use south_provider_runtime::{
    ComponentRuntimeV1, LoadedComponentV1, RuntimeLimitsV1, SecretSignerV1,
};
use token_station_protocol::{Auth, HttpResponseParts, ProviderConfig};

#[path = "support/gate2_report.rs"]
mod gate2_report;
#[path = "support/host_range.rs"]
mod host_range;

const PACKAGE: &str = "task-gemini-omni-v2";

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

fn component_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let status = Command::new("bash")
            .arg(repo_root().join("scripts/build-gemini-omni-task-v2-component.sh"))
            .status()
            .expect("bash is on PATH");
        assert!(
            status.success(),
            "the Gemini Omni video component must build; run `rustup target add wasm32-wasip2` if \
             the target is missing"
        );
        repo_root().join(
            "components/task-gemini-omni-v2/target/wasm32-wasip2/release/task_gemini_omni_v2.wasm",
        )
    })
}

struct FixedSigner;

impl SecretSignerV1 for FixedSigner {
    fn sign(&self, _: &str, _: &[u8], _: &str) -> Result<Vec<u8>, String> {
        Ok(vec![0xAB; 32])
    }
}

fn manifest_text() -> String {
    std::fs::read_to_string(repo_root().join("components/task-gemini-omni-v2/manifest.json"))
        .expect("manifest reads")
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(&manifest_text()).expect("manifest parses")
}

fn pack() -> TaskFixturePackV2 {
    TaskFixturePackV2::load(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures-gemini-omni-task-v2"
    )))
    .expect("the shipped pack loads")
}

fn sandboxed() -> SandboxedTaskComponentV2 {
    let wasm = std::fs::read(component_wasm()).expect("component reads");
    let runtime = ComponentRuntimeV1::new(RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_secs(5),
        max_payload_bytes: 4 * 1024 * 1024,
    })
    .expect("engine builds");
    let loaded = LoadedComponentV1::load_embedded(
        &runtime,
        &manifest_text(),
        &wasm,
        &host_range::host_range(),
        FixedSigner,
    )
    .expect("the shipped package loads");
    SandboxedTaskComponentV2::new(loaded).map_err(|_| ()).expect("it declares the task world")
}

/// The sandboxed component passes the same gate ② as the native reference, contract 8's checks
/// included, against the same frozen pack.
#[test]
fn the_sandboxed_component_passes_gate_two_byte_for_byte() {
    let evidence = gate2_report::Evidence::capture(PACKAGE, component_wasm());
    let report = run_task_component_suite_v2_for_manifest(&sandboxed(), &pack(), &manifest());
    evidence.record(&report);
    assert!(
        report.is_passing(),
        "the sandboxed component must pass the suite its reference passes: {:?}",
        report.failures().collect::<Vec<_>>()
    );
}

#[test]
fn the_sandboxed_and_native_reports_are_identical() {
    let pack = pack();
    let sandboxed = run_task_component_suite_v2_for_manifest(&sandboxed(), &pack, &manifest());
    let native =
        run_task_component_suite_v2_for_manifest(&GeminiOmniTaskComponentV2, &pack, &manifest());
    assert_eq!(
        format!("{:?}", sandboxed.outcomes()),
        format!("{:?}", native.outcomes()),
        "the wasm build and the native build must agree check for check"
    );
}

/// A host whose range lists task contract 7 only refuses the package by its contract number; a
/// host that decodes 7 and 8 admits it next to every contract 7 package.
#[test]
fn a_task_7_host_refuses_the_package_and_a_7_and_8_host_admits_it() {
    let manifest = manifest();
    let both = host_range::host_range();
    assert!(compatibility_admits(&manifest, &both).is_ok());
    let mut seven_only: HostRangeV1 = both;
    seven_only.contracts.insert("task".to_owned(), [7].into());
    assert!(compatibility_admits(&manifest, &seven_only).is_err());
}

/// The host obligations of the record's §6 and §11, end to end through the real component: the
/// host elides the upstream body with the package's declared paths, the component names the clip
/// by pointer, the host resolves the pointer against the call's blobs, and the rendered marker
/// becomes the host's artifact path. The clip (2 MiB of base64) never crosses the boundary.
#[test]
fn the_clip_stays_with_the_host_from_poll_to_delivery() {
    let component = sandboxed();
    let manifest = manifest();
    let config: ProviderConfig = serde_json::from_value(json!({
        "provider": "vertex-interactions-video",
        "base_url": "https://aiplatform.example/v1beta1/projects/p/locations/global",
        "auth": "provider_api_key"}))
    .unwrap();
    let prepared = component
        .build_submit_request(
            &config,
            &json!({"model": "gemini-omni-flash-preview", "prompt": "a kite"}),
            &HostMintedValuesV1::new("host-1", None).unwrap(),
        )
        .unwrap();
    config.authorize(&prepared.descriptor).unwrap();
    assert!(matches!(prepared.descriptor.auth, Some(Auth::Bearer { .. })));
    assert_eq!(
        prepared.request_estimate.bucket_ceilings().unwrap().get(TaskTokenBucketV2::VideoOutput),
        Some(57_920)
    );
    let submitted: HttpResponseParts = serde_json::from_value(json!({"status": 200,
        "body": json!({"id": "abc123", "status": "in_progress"}).to_string()}))
    .unwrap();
    assert_eq!(
        component.parse_submit_response(&submitted).unwrap(),
        SubmitOutcomeV2::Accepted("abc123".to_owned())
    );
    let poll = component.build_observe_request(&config, "m", "abc123", &prepared.locator).unwrap();
    config.authorize(&poll).unwrap();

    let clip: Vec<u8> = (0..1_536_000_u32).map(|index| (index % 251) as u8).collect();
    let upstream = json!({"id": "abc123", "status": "completed",
        "usage": {"total_tokens": 58171, "total_input_tokens": 17,
            "input_tokens_by_modality": [{"modality": "text", "tokens": 17}],
            "total_output_tokens": 57920,
            "output_tokens_by_modality": [{"modality": "video", "tokens": 57920}],
            "total_thought_tokens": 234},
        "steps": [{"type": "thought", "signature": "sig"},
            {"type": "model_output", "content": [{"type": "video", "mime_type": "video/mp4",
                "data": encode_base64(&clip)}]}]});
    let paths: Vec<PathPatternV1> = manifest
        .response_blob_paths
        .iter()
        .map(|path| PathPatternV1::parse(path).unwrap())
        .collect();
    let (view, blobs) =
        elide_v1(upstream.to_string().as_bytes(), &paths, &MediaLimitsV1::V1).unwrap();
    let blobs = BlobSetV1::new(blobs).unwrap();
    assert!(view.len() < 4096, "the view handed to the sandbox holds no clip bytes");
    let parts: HttpResponseParts =
        serde_json::from_value(json!({"status": 200, "body": view.as_str()})).unwrap();
    let observation = component.parse_observation(&parts).unwrap();
    let TaskObservationV2::Succeeded { artifacts, usage } = &observation else {
        panic!("succeeded expected: {observation:?}")
    };
    let TaskArtifactRefV2::Inline(items) = artifacts else { panic!("inline expected") };
    assert_eq!(
        resolve_task_inline_artifact_v1(view.as_str(), &blobs, &items[0], 64 << 20).unwrap(),
        clip
    );
    assert_eq!(usage.token_buckets().unwrap().get(TaskTokenBucketV2::ReasoningOutput), Some(234));
    let context = TaskRenderContextV2::new("host-1", 7, "omni", "vertex", Some("abc123")).unwrap();
    let mut body = component.render_success(&observation, None, &context).unwrap();
    assert_eq!(
        fill_task_artifact_markers_v1(&mut body, artifacts, |index| {
            format!("/v1/video/tasks/host-1/artifacts/art_{index}")
        }),
        Ok(1)
    );
    assert_eq!(
        body,
        json!({"created": 7, "data": [{"url": "/v1/video/tasks/host-1/artifacts/art_0"}]})
    );
}

/// The Gemini API family keeps the native arm's wire: `x-goog-api-key`, `delivery: "uri"`, a
/// synchronous answer settled at submit, and a Files API URI fetched with the bound credential.
#[test]
fn the_gemini_family_settles_a_synchronous_uri_answer_at_submit() {
    let component = sandboxed();
    let config: ProviderConfig = serde_json::from_value(json!({
        "provider": "gemini-interactions-video",
        "base_url": "https://generativelanguage.example", "auth": "provider_api_key"}))
    .unwrap();
    let prepared = component
        .build_submit_request(
            &config,
            &json!({"model": "gemini-omni-flash-preview", "prompt": "a kite"}),
            &HostMintedValuesV1::new("host-1", None).unwrap(),
        )
        .unwrap();
    config.authorize(&prepared.descriptor).unwrap();
    assert!(
        matches!(&prepared.descriptor.auth, Some(Auth::Header { name, .. }) if name == "x-goog-api-key")
    );
    assert_eq!(prepared.descriptor.body.as_ref().unwrap()["response_format"]["delivery"], "uri");
    let answer = json!({"status": "completed",
        "usage": {"total_input_tokens": 17, "total_output_tokens": 57920,
            "output_tokens_by_modality": [{"modality": "video", "tokens": 57920}],
            "total_thought_tokens": 234},
        "steps": [{"type": "model_output", "content": [{"type": "video",
            "uri": "https://generativelanguage.example/v1beta/files/x:download?alt=media"}]}]});
    let parts: HttpResponseParts =
        serde_json::from_value(json!({"status": 200, "body": answer.to_string()})).unwrap();
    let SubmitOutcomeV2::AcceptedTerminal(TaskObservationV2::Succeeded {
        artifacts: TaskArtifactRefV2::Urls(items),
        ..
    }) = component.parse_submit_response(&parts).unwrap()
    else {
        panic!("a synchronous uri answer settles at submit")
    };
    assert!(items[0].fetch_with_credential());
}
