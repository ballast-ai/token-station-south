//! The load gates and the sandbox, exercised against a real `wasm32-wasip2`
//! component.
//!
//! The guest (`tests/guests/test-provider`) is an honest small provider
//! component that turns hostile on demand: magic keys in its inputs make it
//! hang, allocate without bound, panic, or ask the host to sign with names it
//! did not declare. Every limit test drives a *real* misbehaviour through the
//! *real* runtime — no mock trap, no simulated deadline. Everything here
//! stays on the runtime's JSON face: this crate never parses the IR, and
//! neither do its tests.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use south_provider_runtime::{
    CallErrorV1, ComponentRuntimeV1, LoadErrorV1, LoadedComponentV1, RuntimeLimitsV1,
    SecretSignerV1,
};

/// Builds the guest once per test process and returns the component's path.
fn guest_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let guest_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/guests/test-provider");
        let status = Command::new("cargo")
            .args(["build", "--target", "wasm32-wasip2"])
            .current_dir(&guest_dir)
            .status()
            .expect("cargo is on PATH");
        assert!(
            status.success(),
            "the guest must build; run `rustup target add wasm32-wasip2` if the target is missing"
        );
        guest_dir.join("target/wasm32-wasip2/debug/test_provider.wasm")
    })
}

fn manifest_json(version: &str) -> String {
    json!({
        "name": "test-provider",
        "version": version,
        "api_version": "provider-adapter-v2",
        "providers": ["test"],
        "capabilities": ["chat", "stream"],
        "auth_arms": ["bearer"],
        "permissions": { "network": false, "filesystem": false, "secrets": ["provider_api_key"] },
        "conformance": { "required_suite": "south.provider-component.v1", "fixtures": "fixtures/" },
        "compatibility": {
            "ir_schema_id": "token-station-protocol@0.3.0/v0.2.0",
            "kernel_version": "0.2.0",
            "kernel_revision": "72458e3a11fe157f9ac04818c44b62a3dd2cb09c",
            "wit_package": "token-station:adapter@2.0.0",
            "south_runtime": "0.11.0",
        },
    })
    .to_string()
}

/// Assembles a component package directory: `manifest.json` next to
/// `component.wasm`. Unique per call: tests run in parallel.
fn package(name: &str, manifest: &str, wasm: Option<&Path>) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("south-component-{}-{seq}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir is writable");
    std::fs::write(dir.join("manifest.json"), manifest).expect("manifest writes");
    if let Some(wasm) = wasm {
        std::fs::copy(wasm, dir.join("component.wasm")).expect("wasm copies");
    }
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

fn config_json(extra: &Value) -> String {
    let mut base = json!({
        "provider": "test",
        "base_url": "https://api.test.example/v1",
        "auth": "provider_api_key",
        "models": [{ "model": "test-1", "tool": true, "context_window": 8192 }],
    });
    base.as_object_mut().expect("object").extend(extra.as_object().cloned().unwrap_or_default());
    base.to_string()
}

fn load() -> LoadedComponentV1 {
    let dir = package("ok", &manifest_json("1.0.0"), Some(guest_wasm()));
    LoadedComponentV1::load(&runtime(), &dir, FixedSigner).expect("the honest package loads")
}

// -- gates -------------------------------------------------------------------

#[test]
fn a_faithful_package_passes_every_gate_and_answers_on_the_json_face() {
    let component = load();

    assert_eq!(component.metadata().name, "test-provider");

    let capabilities =
        component.call_model_capabilities(&config_json(&json!({}))).expect("capabilities json");
    let parsed: Value = serde_json::from_str(&capabilities).expect("guest returned JSON");
    assert_eq!(parsed[0]["model"], json!("test-1"));
}

#[test]
fn the_manifest_gate_runs_before_any_code_is_read() {
    let mut manifest: Value = serde_json::from_str(&manifest_json("1.0.0")).expect("valid json");
    manifest["permissions"]["network"] = json!(true);

    // No component.wasm in the package at all: if the manifest gate is really
    // first, the loader never notices.
    let dir = package("network", &manifest.to_string(), None);
    let refused = LoadedComponentV1::load(&runtime(), &dir, FixedSigner);

    assert!(matches!(refused, Err(LoadErrorV1::Manifest(_))), "got {refused:?}");
}

#[test]
fn the_identity_gate_refuses_a_package_that_lies_about_its_version() {
    // Same wasm, same name, but the manifest claims 9.9.9. This is the
    // repackaged-around-vetting case.
    let dir = package("liar", &manifest_json("9.9.9"), Some(guest_wasm()));

    let refused = LoadedComponentV1::load(&runtime(), &dir, FixedSigner);

    match refused {
        Err(LoadErrorV1::IdentityMismatch { declared, reported }) => {
            assert_eq!(declared.version, "9.9.9");
            assert_eq!(reported.version, "1.0.0");
        }
        other => panic!("expected an identity mismatch, got {other:?}"),
    }
}

#[test]
fn a_component_that_asks_for_the_network_is_refused_by_name() {
    // A component that imports wasi:sockets. Hand-written, because no honest
    // build of a provider component produces one — which is the point.
    let wat = r#"(component
        (import "wasi:sockets/instance-network@0.2.0" (instance))
    )"#;
    let dir = package("sockets", &manifest_json("1.0.0"), None);
    std::fs::write(dir.join("component.wasm"), wat).expect("wat writes");

    let refused = LoadedComponentV1::load(&runtime(), &dir, FixedSigner);

    match refused {
        Err(LoadErrorV1::ForbiddenImport(name)) => {
            assert!(name.starts_with("wasi:sockets/"), "{name}");
        }
        other => panic!("expected a forbidden import, got {other:?}"),
    }
}

// -- sandbox -----------------------------------------------------------------

#[test]
fn a_hung_guest_is_cut_off_at_the_deadline_not_at_infinity() {
    let component = load();

    let started = Instant::now();
    let refused = component
        .call_model_capabilities(&config_json(&json!({ "__hang": true })))
        .expect_err("an infinite loop must not return");
    let elapsed = started.elapsed();

    assert!(matches!(refused, CallErrorV1::Deadline), "got {refused:?}");
    assert!(
        elapsed < Duration::from_secs(10),
        "the deadline is 500ms; {elapsed:?} means the epoch never fired"
    );

    // The instance trapped, but the *component* must survive: the next call
    // reports an error rather than panicking the host.
    let _ = component.call_model_capabilities(&config_json(&json!({})));
}

#[test]
fn a_guest_that_allocates_past_the_limit_traps_instead_of_pressuring_the_host() {
    let component = load();

    let refused = component
        .call_model_capabilities(&config_json(&json!({ "__grow_mb": 256 })))
        .expect_err("256MB against a 64MB limit must fail");

    assert!(matches!(refused, CallErrorV1::Trap(_)), "got {refused:?}");
}

#[test]
fn a_panicking_guest_becomes_a_trap_not_a_host_panic() {
    let component = load();

    let refused = component
        .call_model_capabilities(&config_json(&json!({ "__panic": true })))
        .expect_err("a guest panic is a trap");

    assert!(matches!(refused, CallErrorV1::Trap(_)), "got {refused:?}");
}

#[test]
fn an_oversized_payload_is_refused_before_the_guest_runs() {
    let component = load();

    let oversized = format!(r#"{{"filler":"{}"}}"#, "x".repeat(1024 * 1024 + 1));
    let refused = component
        .call_model_capabilities(&oversized)
        .expect_err("a payload past the ceiling must not cross the boundary");

    assert!(matches!(refused, CallErrorV1::PayloadTooLarge { .. }), "got {refused:?}");
}

// -- credentials -------------------------------------------------------------

#[test]
fn a_declared_secret_reaches_the_signer_and_an_undeclared_one_never_does() {
    struct Tattletale;
    impl SecretSignerV1 for Tattletale {
        fn sign(&self, secret_ref: &str, _: &[u8], _: &str) -> Result<Vec<u8>, String> {
            // The runtime promises the signer never sees an undeclared name.
            assert_eq!(secret_ref, "provider_api_key", "manifest boundary breached");
            Ok(vec![1, 2, 3])
        }
    }

    let dir = package("signing", &manifest_json("1.0.0"), Some(guest_wasm()));
    let component =
        LoadedComponentV1::load(&runtime(), &dir, Tattletale).expect("the honest package loads");

    let request = |secret: &str| {
        json!({
            "model": "test-1",
            "messages": [{ "role": "user", "content": "hi" }],
            "__sign": { "secret": secret, "algorithm": "hmac-sha256" },
        })
        .to_string()
    };

    let signed = component
        .call_build_http_request(&request("provider_api_key"), &config_json(&json!({})))
        .expect("a declared secret signs");
    let descriptor: Value = serde_json::from_str(&signed).expect("descriptor json");
    assert_eq!(descriptor["signature_bytes"], json!(3));

    let refused = component
        .call_build_http_request(&request("someone_elses_key"), &config_json(&json!({})))
        .expect_err("an undeclared secret is refused before the signer sees it");
    match refused {
        CallErrorV1::Component(error_json) => {
            assert!(error_json.contains("not declared"), "{error_json}");
        }
        other => panic!("expected a component error payload, got {other:?}"),
    }
}

// -- streams -----------------------------------------------------------------

#[test]
fn each_stream_gets_its_own_instance_so_buffers_cannot_interleave() {
    let component = load();

    let mut left = component.open_stream().expect("stream opens");
    let mut right = component.open_stream().expect("stream opens");

    // Each parser receives half a frame. If they shared the guest's buffer,
    // the halves would concatenate and one would emit a mangled event.
    let none_yet = left.parse_chunk(b"data: from-left").expect("parses");
    assert_eq!(none_yet, "[]", "half a frame is not an event");
    let none_yet = right.parse_chunk(b"data: from-right").expect("parses");
    assert_eq!(none_yet, "[]");

    let finished = left.parse_chunk(b"\n\n").expect("parses");
    let events: Value = serde_json::from_str(&finished).expect("events json");
    assert_eq!(
        events,
        json!([{ "type": "delta", "index": 0, "content": "from-left" }]),
        "the left stream must complete with only its own bytes"
    );

    let finished = right.parse_chunk(b"\n\n").expect("parses");
    let events: Value = serde_json::from_str(&finished).expect("events json");
    assert_eq!(events, json!([{ "type": "delta", "index": 0, "content": "from-right" }]));
}
