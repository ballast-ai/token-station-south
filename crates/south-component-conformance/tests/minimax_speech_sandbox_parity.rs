//! `speech-minimax` inside the sandbox: the shipped component passes gate ② against the same frozen pack
//! and manifest that judged its native reference, and agrees with that reference byte for byte
//! across the ABI.
//!
//! Design records: `docs/design/2026-09-30-speech-world.md` (§7, §11) and
//! `docs/design/2026-10-14-speech-v3-2-tts.md` (§3, §8).
//!
//! The raw ABI comparison replays what the suite itself sent: the native reference runs the suite
//! behind a recorder, so every argument a function is compared on is one the suite built the way a
//! host would (the response view through `build_media_response_view_v1` or, for the `sse` form,
//! `build_media_sse_response_view_v1`), including the suite's own mutations of the inputs. The
//! sandbox seam parses each prepared call with the package's own declared query parameters, as a
//! host does (record §6).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::Value;
use south_component_conformance::reference_minimax_speech::MiniMaxSpeechReferenceV1;
use south_component_conformance::sandbox_image::SandboxedImageComponentV1;
use south_component_conformance::sandbox_speech::SandboxedSpeechComponentV1;
use south_component_conformance::speech_fixture::SpeechFixturePackV1;
use south_component_conformance::{
    ComponentResultV1, SpeechComponentV1, SpeechOutcomeV1, abi_speech as abi, accepts_manifest,
    reported_identity_matches, run_speech_component_suite_v1,
};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechModelCapabilitiesV1,
};
use south_provider_api::{
    ComponentManifestV1, ComponentMetadataV1, HostExpectationsV1, compatibility_admits,
    compatibility_matches,
};
use south_provider_runtime::{
    CallErrorV1, ComponentRuntimeV1, LoadedComponentV1, NoSecretsV1, RuntimeLimitsV1,
};
use token_station_protocol::ProviderConfig;

#[path = "support/gate2_report.rs"]
mod gate2_report;
#[path = "support/host_range.rs"]
mod host_range;

const PACKAGE: &str = "speech-minimax";
const NATIVE: MiniMaxSpeechReferenceV1 = MiniMaxSpeechReferenceV1;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

/// Builds the component once per test process with the package's own build script.
fn component_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let status = Command::new("bash")
            .arg(repo_root().join("scripts/build-speech-minimax-component.sh"))
            .status()
            .expect("bash is on PATH");
        assert!(
            status.success(),
            "{PACKAGE} must build; run `rustup target add wasm32-wasip2` if the target is missing"
        );
        repo_root()
            .join("components")
            .join(PACKAGE)
            .join("target/wasm32-wasip2/release/speech_minimax.wasm")
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
fn pack() -> SpeechFixturePackV1 {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(manifest().conformance.fixtures);
    SpeechFixturePackV1::load(&directory).expect("the shipped pack loads")
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

fn sandboxed() -> SandboxedSpeechComponentV1 {
    SandboxedSpeechComponentV1::new(load()).map_err(|_| ()).expect("it declares the speech world")
}

/// The sandboxed component passes the same gate ② the native reference does, against the same
/// frozen pack and manifest; the run is recorded as the package's gate ② report.
#[test]
fn the_sandboxed_component_passes_gate_two_byte_for_byte() {
    let evidence = gate2_report::Evidence::capture(PACKAGE, component_wasm());
    let report = run_speech_component_suite_v1(&sandboxed(), &pack(), &manifest());
    evidence.record(&report);
    assert!(report.is_passing(), "{report}");
}

/// The two builds agree check for check, not merely both-pass: a suite that passed for different
/// reasons on each side would hide a boundary bug.
#[test]
fn the_sandboxed_and_native_reports_are_identical() {
    let (pack, manifest) = (pack(), manifest());
    let sandboxed = run_speech_component_suite_v1(&sandboxed(), &pack, &manifest);
    let native = run_speech_component_suite_v1(&NATIVE, &pack, &manifest);
    assert_eq!(format!("{:?}", sandboxed.outcomes()), format!("{:?}", native.outcomes()));
}

/// One ABI call the suite made, with its arguments as they cross the boundary.
enum Call {
    ModelCapabilities { config: String },
    Prepare { config: String, request: String, context: String },
    ParseResponse { state: String, response: String },
}

/// The native reference, recording every call the suite makes of it.
struct Recorder(RefCell<Vec<Call>>);

fn text<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("a suite argument serializes")
}

impl SpeechComponentV1 for Recorder {
    fn metadata(&self) -> ComponentMetadataV1 {
        NATIVE.metadata()
    }
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<SpeechModelCapabilitiesV1>> {
        self.0.borrow_mut().push(Call::ModelCapabilities { config: text(config) });
        NATIVE.model_capabilities(config)
    }
    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        self.0.borrow_mut().push(Call::Prepare {
            config: text(config),
            request: text(request),
            context: text(context),
        });
        NATIVE.prepare(config, request, context)
    }
    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        self.0
            .borrow_mut()
            .push(Call::ParseResponse { state: text(state), response: text(response) });
        NATIVE.parse_response(state, response)
    }
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

/// Every call the suite makes, run through the raw ABI of the guest and of the native shim, gives
/// the same string, refusals included.
#[test]
fn every_abi_answer_is_byte_identical_to_the_native_reference() {
    let recorder = Recorder(RefCell::new(Vec::new()));
    let report = run_speech_component_suite_v1(&recorder, &pack(), &manifest());
    assert!(report.is_passing(), "{report}");
    let loaded = load();
    let mut compared = [0_usize; 3];
    for call in recorder.0.into_inner() {
        match call {
            Call::ModelCapabilities { config } => {
                assert_eq!(
                    guest(loaded.call_speech_model_capabilities(&config)),
                    abi::model_capabilities_json(&NATIVE, &config),
                    "model-capabilities differs across the boundary for {config}"
                );
                compared[0] += 1;
            }
            Call::Prepare { config, request, context } => {
                assert_eq!(
                    guest(loaded.call_speech_prepare(&config, &request, &context)),
                    abi::prepare_json(&NATIVE, &config, &request, &context),
                    "prepare differs across the boundary for {request}"
                );
                compared[1] += 1;
            }
            Call::ParseResponse { state, response } => {
                assert_eq!(
                    guest(loaded.call_speech_parse_response(&state, &response)),
                    abi::parse_response_json(&NATIVE, &state, &response),
                    "parse-response differs across the boundary for {response}"
                );
                compared[2] += 1;
            }
        }
    }
    assert!(compared.iter().all(|count| *count > 0), "every function was compared: {compared:?}");
}

/// Gate ①: the manifest, the reported identity (sandboxed, native and manifest agree), the range
/// handshake of a host linking this release, and the exact tuple handshake.
#[test]
fn the_shipped_package_passes_gate_one_and_the_tuple_handshake() {
    let manifest = manifest();
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    host_range::assert_released_runtime(&manifest);
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));

    let component = sandboxed();
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

/// The image seam refuses the loaded speech component and hands it back intact.
#[test]
fn the_image_seam_refuses_a_loaded_speech_component() {
    let returned = SandboxedImageComponentV1::new(load())
        .expect_err("the image seam rejects a speech component");
    assert!(
        SandboxedSpeechComponentV1::new(*returned).is_ok(),
        "the refused component stays usable through its exact world"
    );
}
