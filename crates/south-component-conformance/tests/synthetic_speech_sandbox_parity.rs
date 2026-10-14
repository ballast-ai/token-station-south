//! The speech suite's synthetic reference inside the sandbox: the guest
//! `tests/guests/synthetic-speech` passes gate ② against the same pack and manifest that judged the
//! native reference, and agrees with it byte for byte across the ABI (speech record §7, §11, step
//! S-S-3). It exercises the speech seams this step adds — the guest shim
//! (`abi_speech`), the host seam (`sandbox_speech`) and the outcome codec (`speech_json`) — with
//! every response body form and both delivery kinds.
//!
//! The guest is a test double, not a shipped package, so no gate ② report is recorded for it.
//!
//! The raw ABI comparison replays what the suite itself sent: the native reference runs the suite
//! behind a recorder, so every argument a function is compared on is one the suite built the way a
//! host would (the response view through `build_media_response_view_v1` or, for the `sse` form,
//! `build_media_sse_response_view_v1`), including the suite's own mutations of the inputs.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::Value;
use south_component_conformance::reference_synthetic_speech::SyntheticSpeechReferenceV1;
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
use south_provider_api::{ComponentManifestV1, ComponentMetadataV1, compatibility_admits};
use south_provider_runtime::{
    CallErrorV1, ComponentRuntimeV1, LoadedComponentV1, NoSecretsV1, RuntimeLimitsV1,
};
use token_station_protocol::ProviderConfig;

#[path = "support/host_range.rs"]
mod host_range;

const NATIVE: SyntheticSpeechReferenceV1 = SyntheticSpeechReferenceV1;

fn guest_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/guests/synthetic-speech")
}

/// Builds the guest once per test process; `scripts/prebuild-components.sh` (the nextest setup
/// script) has already built it under nextest.
fn component_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        if std::env::var_os("SOUTH_COMPONENTS_PREBUILT").is_none() {
            let status = Command::new("cargo")
                .args(["build", "--target", "wasm32-wasip2"])
                .current_dir(guest_dir())
                .status()
                .expect("cargo is on PATH");
            assert!(
                status.success(),
                "the guest must build; run `rustup target add wasm32-wasip2` if the target is \
                 missing"
            );
        }
        guest_dir().join("target/wasm32-wasip2/debug/synthetic_speech.wasm")
    })
}

fn manifest_source() -> String {
    std::fs::read_to_string(guest_dir().join("manifest.json")).expect("the guest manifest reads")
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(&manifest_source()).expect("the guest manifest parses")
}

/// The pack the manifest names.
fn pack() -> SpeechFixturePackV1 {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(manifest().conformance.fixtures);
    SpeechFixturePackV1::load(&directory).expect("the pack loads")
}

/// Loads the guest through every load gate under this release's range handshake.
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
    .expect("the guest passes every load gate")
}

fn sandboxed() -> SandboxedSpeechComponentV1 {
    SandboxedSpeechComponentV1::new(load()).map_err(|_| ()).expect("it declares the speech world")
}

/// The sandboxed component passes the same gate ② the native reference does.
#[test]
fn the_sandboxed_component_passes_gate_two() {
    let report = run_speech_component_suite_v1(&sandboxed(), &pack(), &manifest());
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

/// Gate ①: the manifest, the reported identity (sandboxed, native and manifest agree) and the
/// range handshake of a host linking this release.
#[test]
fn the_guest_passes_gate_one() {
    let manifest = manifest();
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
    let component = sandboxed();
    assert_eq!(component.metadata(), NATIVE.metadata());
    assert!(reported_identity_matches(&component.metadata(), &manifest));
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
