//! Shared harness of the embeddings sandbox parity tests and the E-Q1 measurement.
//!
//! Each test builds its package's `component.wasm` with the package's own build script, loads it
//! through the runtime the way a host linking this release does, and compares it with the native
//! reference it was compiled from. The comparison is made on the raw ABI strings: for every case
//! of the frozen pack, the JSON the guest returns across the sandbox boundary must equal, byte for
//! byte, the JSON the same `abi_embeddings` shim returns natively.

#![allow(dead_code, reason = "each test binary mounting this module uses a different subset")]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use south_component_conformance::sandbox_embeddings::SandboxedEmbeddingsComponentV1;
use south_component_conformance::{
    EmbeddingsComponentV1, EmbeddingsFamilyV1, EmbeddingsFixturePackV1, abi_embeddings as abi,
};
use south_contracts::extract_vectors_v1;
use south_provider_api::ComponentManifestV1;
use south_provider_runtime::{
    CallErrorV1, ComponentRuntimeV1, LoadedComponentV1, NoSecretsV1, RuntimeLimitsV1,
};
use token_station_protocol::{HttpResponseParts, ProviderConfig};

use super::host_range;

pub fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

/// Runs `scripts/<script>` and returns `components/<package>/target/.../<package>.wasm`.
pub fn build(package: &str, script: &str) -> PathBuf {
    let status = Command::new("bash")
        .arg(repo_root().join("scripts").join(script))
        .status()
        .expect("bash is on PATH");
    assert!(
        status.success(),
        "{package} must build; run `rustup target add wasm32-wasip2` if the target is missing"
    );
    repo_root()
        .join("components")
        .join(package)
        .join("target/wasm32-wasip2/release")
        .join(format!("{}.wasm", package.replace('-', "_")))
}

pub fn manifest_source(package: &str) -> String {
    std::fs::read_to_string(repo_root().join("components").join(package).join("manifest.json"))
        .expect("the shipped manifest reads")
}

pub fn manifest(package: &str) -> ComponentManifestV1 {
    serde_json::from_str(&manifest_source(package)).expect("the shipped manifest parses")
}

/// The pack the manifest names, so the test judges what the package declares.
pub fn pack(package: &str) -> EmbeddingsFixturePackV1 {
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(manifest(package).conformance.fixtures);
    EmbeddingsFixturePackV1::load(&directory).expect("the shipped pack loads")
}

/// Loads the package through every load gate under this release's range handshake.
pub fn load(package: &str, wasm: &Path) -> LoadedComponentV1 {
    let runtime = ComponentRuntimeV1::new(RuntimeLimitsV1::default()).expect("engine builds");
    let wasm = std::fs::read(wasm).expect("the component reads");
    LoadedComponentV1::load_embedded(
        &runtime,
        &manifest_source(package),
        &wasm,
        &host_range::host_range(),
        NoSecretsV1,
    )
    .expect("the shipped package passes every load gate")
}

pub fn sandboxed(package: &str, wasm: &Path) -> SandboxedEmbeddingsComponentV1 {
    SandboxedEmbeddingsComponentV1::new(load(package, wasm))
        .map_err(|_| ())
        .expect("it declares the embeddings world")
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

fn text(value: &Value) -> String {
    serde_json::to_string(value).expect("a fixture value serializes")
}

/// Calls of each family compared, in `EmbeddingsFamilyV1::ALL` order.
pub type Compared = [usize; 3];

/// Every case of `pack`, run through the raw ABI of the guest and of the native shim over
/// `native`, must produce the same string. A response case's skeleton is erased with the locator
/// the native build of its paired request declares, as a host would; a body the host would refuse
/// before the component never reaches `parse-embeddings-response` and is not compared.
pub fn assert_raw_abi_parity(
    loaded: &LoadedComponentV1,
    native: &dyn EmbeddingsComponentV1,
    pack: &EmbeddingsFixturePackV1,
) -> Compared {
    let mut compared = [0; 3];
    for case in pack.cases() {
        match case.family {
            EmbeddingsFamilyV1::Request => {
                let config = text(&case.input["provider_config"]);
                let request = text(&case.input["request"]);
                assert_eq!(
                    guest(loaded.call_build_embeddings_request(&config, &request)),
                    abi::build_embeddings_request_json(native, &config, &request),
                    "{}: build-embeddings-request differs across the boundary",
                    case.name
                );
                compared[0] += 1;
            }
            EmbeddingsFamilyV1::Response => {
                let paired = pack
                    .case(
                        case.input["request_case"].as_str().expect("a response names its request"),
                    )
                    .expect("the paired request case exists");
                let config: ProviderConfig =
                    serde_json::from_value(paired.input["provider_config"].clone())
                        .expect("the paired configuration parses");
                let request = serde_json::from_value(paired.input["request"].clone())
                    .expect("the paired request parses");
                let prepared = native
                    .build_embeddings_request(&config, &request)
                    .expect("the paired request builds");
                let mut parts: HttpResponseParts =
                    serde_json::from_value(case.input["response"].clone())
                        .expect("the response parts parse");
                let Ok(extracted) = extract_vectors_v1(parts.body.as_bytes(), &prepared.vectors)
                else {
                    continue;
                };
                parts.body = extracted.skeleton().to_string();
                let parts = serde_json::to_string(&parts).expect("the parts serialize");
                let context = text(&prepared.parse_context);
                assert_eq!(
                    guest(loaded.call_parse_embeddings_response(&parts, &context)),
                    abi::parse_embeddings_response_json(native, &parts, &context),
                    "{}: parse-embeddings-response differs across the boundary",
                    case.name
                );
                compared[1] += 1;
            }
            EmbeddingsFamilyV1::Error => {
                let parts = text(&case.input);
                assert_eq!(
                    guest(loaded.call_map_embeddings_provider_error(&parts)),
                    abi::map_provider_error_json(native, &parts),
                    "{}: map-provider-error differs across the boundary",
                    case.name
                );
                compared[2] += 1;
            }
        }
    }
    compared
}
