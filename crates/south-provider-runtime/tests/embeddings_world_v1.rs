//! Real embeddings-world package admission, world separation and runtime limits.
//!
//! The guest (`tests/guests/test-embeddings`) answers with canned documents that
//! echo its inputs; it proves the world's plumbing, not a dialect, so these
//! tests stay on the runtime's JSON face and never read the contract types.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{Value, json};
use south_provider_runtime::{
    CallErrorV1, ComponentRuntimeV1, LoadErrorV1, LoadedComponentV1, RuntimeLimitsV1,
    SecretSignerV1,
};

#[path = "support/host_range.rs"]
mod host_range;

/// Builds a guest under `tests/guests` once per call site and returns the component's path.
fn build_guest(directory: &str, artifact: &str) -> PathBuf {
    let guest_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/guests").join(directory);
    let status = Command::new("cargo")
        .args(["build", "--target", "wasm32-wasip2"])
        .current_dir(&guest_dir)
        .status()
        .expect("cargo is on PATH");
    assert!(
        status.success(),
        "the guest must build; run `rustup target add wasm32-wasip2` if the target is missing"
    );
    guest_dir.join("target/wasm32-wasip2/debug").join(artifact)
}

/// Builds the embeddings guest once per test process.
fn embeddings_guest_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| build_guest("test-embeddings", "test_embeddings.wasm"))
}

/// Builds the provider-world guest once per test process.
fn provider_guest_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| build_guest("test-provider", "test_provider.wasm"))
}

/// The compatibility block of a package built for this tree's runtime.
fn compatibility(wit_package: &str, contracts: &Value) -> Value {
    let host = host_range::host_range();
    json!({
        "ir_schema_id": "token-station-protocol@0.5.0/v0.4.0",
        "kernel_version": "0.4.0",
        "kernel_revision": "8e34f5a089d0b9c7273b49ddb6952dd87e960019",
        "wit_package": wit_package,
        "south_runtime": env!("CARGO_PKG_VERSION"),
        "runtime_abi": host.runtime_abi,
        "kernel_contracts": host.kernel_contracts,
        "contracts": contracts,
    })
}

fn embeddings_manifest_value() -> Value {
    json!({
        "name": "test-embeddings",
        "version": "1.0.0",
        "api_version": "embeddings-adapter-v1",
        "providers": ["test"],
        "capabilities": ["embed", "batch"],
        "auth_arms": ["bearer"],
        "permissions": { "network": false, "filesystem": false, "secrets": ["provider_api_key"] },
        "conformance": { "required_suite": "south.embeddings-component.v1", "fixtures": "fixtures/" },
        "compatibility": compatibility(
            "token-station:embeddings-adapter@1.0.0",
            &json!({ "embeddings": 1 }),
        ),
    })
}

fn embeddings_manifest() -> String {
    embeddings_manifest_value().to_string()
}

fn provider_manifest() -> String {
    json!({
        "name": "test-provider",
        "version": "1.0.0",
        "api_version": "provider-adapter-v2",
        "providers": ["test"],
        "capabilities": ["chat", "stream"],
        "auth_arms": ["bearer"],
        "permissions": { "network": false, "filesystem": false, "secrets": ["provider_api_key"] },
        "conformance": { "required_suite": "south.provider-component.v1", "fixtures": "fixtures/" },
        "compatibility": compatibility("token-station:adapter@2.0.0", &json!({})),
    })
    .to_string()
}

fn package(name: &str, manifest: &str, wasm: &Path) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("south-embeddings-{}-{seq}-{name}", std::process::id()));
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

fn load_embedded(
    runtime: &ComponentRuntimeV1,
    manifest: &str,
    wasm: &[u8],
) -> Result<LoadedComponentV1, LoadErrorV1> {
    LoadedComponentV1::load_embedded(
        runtime,
        manifest,
        wasm,
        &host_range::host_range(),
        FixedSigner,
    )
}

fn load_with_limits(limits: RuntimeLimitsV1) -> LoadedComponentV1 {
    let runtime = ComponentRuntimeV1::new(limits).expect("runtime");
    let wasm = std::fs::read(embeddings_guest_wasm()).expect("guest bytes");
    load_embedded(&runtime, &embeddings_manifest(), &wasm).expect("the embeddings guest loads")
}

fn parsed(output: &str) -> Value {
    serde_json::from_str(output).expect("the guest answers JSON")
}

#[test]
fn the_embeddings_guest_loads_and_answers_its_three_calls() {
    let dir = package("loads", &embeddings_manifest(), embeddings_guest_wasm());
    let loaded = LoadedComponentV1::load(&runtime(), &dir, &host_range::host_range(), FixedSigner)
        .expect("the embeddings package loads");
    assert_eq!(loaded.metadata().api_version, "embeddings-adapter-v1");
    assert_eq!(loaded.metadata().name, "test-embeddings");

    let prepared = loaded
        .call_build_embeddings_request(r#"{"base_url":"https://x.test"}"#, r#"{"input":"hi"}"#)
        .expect("build answers");
    let prepared = parsed(&prepared);
    assert_eq!(prepared["descriptor"]["body"], json!({ "input": "hi" }));
    assert_eq!(prepared["parse_context"]["config"], json!({ "base_url": "https://x.test" }));

    let parsed_response = loaded
        .call_parse_embeddings_response(r#"{"status":200}"#, r#"{"count":1}"#)
        .expect("parse answers");
    let parsed_response = parsed(&parsed_response);
    assert_eq!(parsed_response["parts"], json!({ "status": 200 }));
    assert_eq!(parsed_response["context"], json!({ "count": 1 }));

    let mapped = loaded.call_map_embeddings_provider_error(r#"{"status":503}"#).expect("map");
    let mapped = parsed(&mapped);
    assert_eq!(mapped["outcome"], "unknown");
    assert_eq!(mapped["parts"], json!({ "status": 503 }));

    // The guest's error channel stays opaque: the runtime hands it back as is.
    let Err(CallErrorV1::Component(envelope)) =
        loaded.call_build_embeddings_request("not json", "{}")
    else {
        panic!("a guest error must arrive on the component error channel");
    };
    assert_eq!(parsed(&envelope)["code"], "internal");
}

#[test]
fn embeddings_refuses_every_host_import_before_world_instantiation() {
    for name in [
        "token-station:adapter/host@2.0.0",
        "token-station:task-adapter/host@1.0.0",
        "token-station:task-adapter/host@2.0.0",
        "token-station:embeddings-adapter/host@1.0.0",
        "token-station:embeddings-adapter/embeddings-adapter@1.0.0",
        "acme:lookalike/host@1.0.0",
        "acme:lookalike/host",
    ] {
        let wat = format!("(component (import \"{name}\" (instance)))");
        let error = load_embedded(&runtime(), &embeddings_manifest(), wat.as_bytes())
            .expect_err("the embeddings world is granted no host import");
        assert!(
            matches!(error, LoadErrorV1::ForbiddenImport(ref actual) if actual == name),
            "the import itself must be refused, before a missing export could disguise the \
             permission gap: {error}"
        );
    }

    // The scan is not over-broad: an ordinary WASI import passes it, and the
    // package then fails only because these bytes export no embeddings world.
    let wat = "(component (import \"wasi:cli/environment@0.2.0\" (instance)))";
    let error = load_embedded(&runtime(), &embeddings_manifest(), wat.as_bytes())
        .expect_err("no embeddings export");
    assert!(
        matches!(error, LoadErrorV1::Probe { ref world, .. } if world == "embeddings-adapter-v1"),
        "{error}"
    );
}

#[test]
fn embeddings_calls_refuse_the_other_worlds_faces() {
    let dir = package("faces", &embeddings_manifest(), embeddings_guest_wasm());
    let loaded = LoadedComponentV1::load(&runtime(), &dir, &host_range::host_range(), FixedSigner)
        .expect("embeddings");
    let refused = |outcome: Result<String, CallErrorV1>, face: &str| match outcome {
        Err(CallErrorV1::Trap(message)) => assert!(
            message.contains("exports `embeddings-adapter-v1`") && message.contains(face),
            "{message}"
        ),
        other => panic!("the {face} must be refused on an embeddings component: {other:?}"),
    };
    refused(loaded.call_parse_response("{}"), "provider face");
    refused(loaded.call_map_provider_error("{}"), "provider face");
    refused(loaded.call_parse_observation("{}"), "task face");
    refused(loaded.call_parse_observation_v2("{}"), "task-v2 face");
    assert!(matches!(
        loaded.open_stream().and_then(|mut stream| stream.parse_chunk(b"")),
        Err(CallErrorV1::Trap(_))
    ));
}

#[test]
fn embeddings_bytes_cannot_claim_another_world_or_a_different_identity() {
    let bytes = std::fs::read(embeddings_guest_wasm()).expect("guest");

    let mut task = embeddings_manifest_value();
    task["api_version"] = "task-adapter-v2".into();
    task["capabilities"] = json!(["submit", "observe", "render"]);
    task["compatibility"]["wit_package"] = "token-station:task-adapter@2.0.0".into();
    task["compatibility"]["contracts"] = json!({});
    task["conformance"]["required_suite"] = "south.task-component.v2".into();
    let error = load_embedded(&runtime(), &task.to_string(), &bytes)
        .expect_err("embeddings bytes are not a task-v2 component");
    assert!(matches!(error, LoadErrorV1::Probe { ref world, .. } if world == "task-adapter-v2"));

    let mut provider = embeddings_manifest_value();
    provider["api_version"] = "provider-adapter-v2".into();
    provider["capabilities"] = json!(["chat"]);
    provider["compatibility"]["wit_package"] = "token-station:adapter@2.0.0".into();
    provider["compatibility"]["contracts"] = json!({});
    provider["conformance"]["required_suite"] = "south.provider-component.v1".into();
    let error = load_embedded(&runtime(), &provider.to_string(), &bytes)
        .expect_err("embeddings bytes are not a provider component");
    assert!(
        matches!(error, LoadErrorV1::Probe { ref world, .. } if world == "provider-adapter-v2")
    );

    let mut renamed = embeddings_manifest_value();
    renamed["version"] = "9.9.9".into();
    let error =
        load_embedded(&runtime(), &renamed.to_string(), &bytes).expect_err("identity mismatch");
    assert!(format!("{error}").contains("not what it claims"));

    // And the other way round: provider bytes cannot pass as an embeddings
    // component. They import the provider world's `host`, which the embeddings
    // scan refuses before instantiation is even attempted.
    let provider_bytes = std::fs::read(provider_guest_wasm()).expect("provider guest");
    let mut claims = embeddings_manifest_value();
    claims["name"] = "test-provider".into();
    let error = load_embedded(&runtime(), &claims.to_string(), &provider_bytes)
        .expect_err("provider bytes are not an embeddings component");
    assert!(
        matches!(error, LoadErrorV1::ForbiddenImport(ref name) if name.starts_with("token-station:adapter/host@")),
        "{error}"
    );
}

#[test]
fn embeddings_admission_keeps_both_handshakes() {
    let bytes = std::fs::read(embeddings_guest_wasm()).expect("guest");

    // The range handshake decodes embeddings contracts 1 and 2 and nothing newer.
    let mut newer = embeddings_manifest_value();
    newer["compatibility"]["contracts"] = json!({ "embeddings": 3 });
    let error =
        load_embedded(&runtime(), &newer.to_string(), &bytes).expect_err("contract 3 is unknown");
    assert!(matches!(error, LoadErrorV1::OutsideRange(_)), "{error}");

    // The exact handshake, still supported for one release: a host holding the manifest's own
    // declaration, except for the runtime, which no release can equal.
    let mut host = host_range::exact_expectations_for(&embeddings_manifest());
    host.south_runtime = "0.0.0".into();
    let error = LoadedComponentV1::load_embedded(
        &runtime(),
        &embeddings_manifest(),
        &bytes,
        &host,
        FixedSigner,
    )
    .expect_err("host tuple must not come from manifest");
    assert!(format!("{error}").contains("south runtime"));
}

#[test]
fn embeddings_bounds_input_output_and_guest_error_output() {
    let limits = |max_payload_bytes| RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_secs(5),
        max_payload_bytes,
    };

    let tiny = load_with_limits(limits(2));
    for outcome in [
        tiny.call_build_embeddings_request("{}", "oversized"),
        tiny.call_parse_embeddings_response("oversized", "{}"),
        tiny.call_map_embeddings_provider_error("oversized"),
    ] {
        assert!(matches!(outcome, Err(CallErrorV1::PayloadTooLarge { limit: 2 })));
    }
    // Every input fits exactly; each answer, success or error, must
    // independently hit the bound.
    assert_eq!("{}".len(), 2);
    for outcome in [
        tiny.call_build_embeddings_request("{}", "{}"),
        tiny.call_parse_embeddings_response("{}", "{}"),
        tiny.call_map_embeddings_provider_error("{}"),
        tiny.call_map_embeddings_provider_error("[x"),
    ] {
        assert!(matches!(outcome, Err(CallErrorV1::PayloadTooLarge { limit: 2 })));
    }

    let config = r#"{"base_url":"https://embeddings.example.test"}"#;
    let request = r#"{"model":"test-embedding","input":["one","two"]}"#;
    let normal = load_with_limits(limits(1024 * 1024));
    let prepared = normal.call_build_embeddings_request(config, request).expect("valid build");
    let limit = config.len().max(request.len());
    assert!(prepared.len() > limit, "the output, not an input, must exceed the limit");
    let bounded = load_with_limits(limits(limit));
    assert!(matches!(
        bounded.call_build_embeddings_request(config, request),
        Err(CallErrorV1::PayloadTooLarge { limit: actual }) if actual == limit
    ));
}

#[test]
fn embeddings_and_provider_worlds_load_in_one_runtime_and_keep_separate_faces() {
    let runtime = runtime();
    let embeddings = load_embedded(
        &runtime,
        &embeddings_manifest(),
        &std::fs::read(embeddings_guest_wasm()).expect("embeddings wasm"),
    )
    .expect("embeddings coexists");
    let provider = load_embedded(
        &runtime,
        &provider_manifest(),
        &std::fs::read(provider_guest_wasm()).expect("provider wasm"),
    )
    .expect("provider coexists");
    assert_eq!(embeddings.metadata().api_version, "embeddings-adapter-v1");
    assert_eq!(provider.metadata().api_version, "provider-adapter-v2");

    for outcome in [
        provider.call_build_embeddings_request("{}", "{}"),
        provider.call_parse_embeddings_response("{}", "{}"),
        provider.call_map_embeddings_provider_error("{}"),
    ] {
        match outcome {
            Err(CallErrorV1::Trap(message)) => assert!(
                message.contains("exports `provider-adapter-v2`; the embeddings face"),
                "{message}"
            ),
            other => panic!("a provider component has no embeddings face: {other:?}"),
        }
    }
    assert!(matches!(embeddings.call_parse_response("{}"), Err(CallErrorV1::Trap(_))));
    // Each keeps answering its own face after the other was called.
    assert!(embeddings.call_map_embeddings_provider_error("{}").is_ok());
}
