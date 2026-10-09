//! A guest trap must be a clean error for the caller and must not cost the
//! component its next call, wherever the host happens to call from.
//!
//! The 2026-10-09 host investigation (gap #101) found two defects, both
//! visible only when a guest *traps*:
//!
//! 1. A guest that traps by panicking or by failing an allocation writes its
//!    message to stderr through a blocking WASI stream call. The synchronous
//!    WASI shims re-enter the ambient tokio runtime with `block_on`, so a
//!    trap taken on a tokio worker thread panicked the calling task.
//! 2. After any trap, wasmtime refuses to enter that instance again, and the
//!    runtime kept using it: every later call on the component failed until
//!    the process restarted.
//!
//! Every test here drives a real trap through the real runtime from one of the
//! three places a host calls from — a multi-thread tokio worker, a
//! current-thread tokio runtime, a plain thread — and then asserts both halves
//! of the contract: the trap comes back as an `Err` of the right kind, and the
//! next call on the same loaded component succeeds.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use south_provider_runtime::{
    CallErrorV1, ComponentRuntimeV1, LoadedComponentV1, RuntimeLimitsV1, SecretSignerV1,
};

#[path = "support/host_range.rs"]
mod host_range;

// -- guests ------------------------------------------------------------------

/// Builds a guest under `tests/guests` once per call site and returns the component's path.
fn build_guest(directory: &str, artifact: &str) -> PathBuf {
    let guest_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/guests").join(directory);
    // `scripts/prebuild-components.sh` (the nextest setup script) has already built the guest.
    if std::env::var_os("SOUTH_COMPONENTS_PREBUILT").is_none() {
        let status = Command::new("cargo")
            .args(["build", "--target", "wasm32-wasip2"])
            .current_dir(&guest_dir)
            .status()
            .expect("cargo is on PATH");
        assert!(
            status.success(),
            "the guest must build; run `rustup target add wasm32-wasip2` if the target is missing"
        );
    }
    guest_dir.join("target/wasm32-wasip2/debug").join(artifact)
}

fn provider_guest_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| build_guest("test-provider", "test_provider.wasm"))
}

fn embeddings_guest_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| build_guest("test-embeddings", "test_embeddings.wasm"))
}

fn manifest(name: &str, api_version: &str, wit_package: &str, contracts: &Value) -> String {
    let host = host_range::host_range();
    let (suite, capabilities) = if api_version == "embeddings-adapter-v1" {
        ("south.embeddings-component.v1", json!(["embed", "batch"]))
    } else {
        ("south.provider-component.v1", json!(["chat", "stream"]))
    };
    json!({
        "name": name,
        "version": "1.0.0",
        "api_version": api_version,
        "providers": ["test"],
        "capabilities": capabilities,
        "auth_arms": ["bearer"],
        "permissions": { "network": false, "filesystem": false, "secrets": ["provider_api_key"] },
        "conformance": { "required_suite": suite, "fixtures": "fixtures/" },
        "compatibility": {
            "ir_schema_id": "token-station-protocol@0.5.0/v0.4.0",
            "kernel_version": "0.4.0",
            "kernel_revision": "8e34f5a089d0b9c7273b49ddb6952dd87e960019",
            "wit_package": wit_package,
            "south_runtime": env!("CARGO_PKG_VERSION"),
            "runtime_abi": host.runtime_abi,
            "kernel_contracts": host.kernel_contracts,
            "contracts": contracts,
        },
    })
    .to_string()
}

struct FixedSigner;

impl SecretSignerV1 for FixedSigner {
    fn sign(&self, _: &str, _: &[u8], _: &str) -> Result<Vec<u8>, String> {
        Ok(vec![0xAB; 32])
    }
}

/// The limits of the host repro in miniature: a 64 MiB guest memory and a short deadline.
fn runtime() -> ComponentRuntimeV1 {
    ComponentRuntimeV1::new(RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_millis(500),
        max_payload_bytes: 1024 * 1024,
    })
    .expect("engine builds")
}

fn load_provider() -> LoadedComponentV1 {
    load_provider_with(FixedSigner)
}

fn load_provider_with(signer: impl SecretSignerV1 + Sync) -> LoadedComponentV1 {
    let wasm = std::fs::read(provider_guest_wasm()).expect("guest bytes");
    let manifest =
        manifest("test-provider", "provider-adapter-v2", "token-station:adapter@2.0.0", &json!({}));
    LoadedComponentV1::load_embedded(
        &runtime(),
        &manifest,
        &wasm,
        &host_range::host_range(),
        signer,
    )
    .expect("the provider guest loads")
}

fn load_embeddings() -> LoadedComponentV1 {
    let wasm = std::fs::read(embeddings_guest_wasm()).expect("guest bytes");
    let manifest = manifest(
        "test-embeddings",
        "embeddings-adapter-v1",
        "token-station:embeddings-adapter@1.0.0",
        &json!({ "embeddings": 1 }),
    );
    LoadedComponentV1::load_embedded(
        &runtime(),
        &manifest,
        &wasm,
        &host_range::host_range(),
        FixedSigner,
    )
    .expect("the embeddings guest loads")
}

// -- where the host calls from ----------------------------------------------

/// The three places a host calls a loaded component from.
#[derive(Clone, Copy, Debug)]
enum Hosting {
    /// A request task on a multi-thread tokio runtime: the gateway's shape.
    TokioWorker,
    /// A current-thread tokio runtime: what `#[tokio::test]` gives a host's tests.
    TokioLocal,
    /// A thread with no tokio runtime at all.
    BareThread,
}

/// Runs `work` as the host would and fails the test, rather than the process, when it panics.
fn hosted(hosting: Hosting, work: impl FnOnce() + Send + 'static) {
    match hosting {
        Hosting::BareThread => {
            std::thread::spawn(work).join().expect("the calling thread must not panic");
        }
        Hosting::TokioWorker | Hosting::TokioLocal => {
            let runtime = match hosting {
                Hosting::TokioWorker => {
                    tokio::runtime::Builder::new_multi_thread().worker_threads(2).build()
                }
                _ => tokio::runtime::Builder::new_current_thread().build(),
            }
            .expect("tokio runtime builds");
            // The call is made synchronously from inside a spawned task, as a
            // gateway request handler does; a panic inside it surfaces as a
            // `JoinError` here instead of unwinding through the test.
            runtime
                .block_on(async { tokio::spawn(async move { work() }).await })
                .expect("the request task must not panic");
        }
    }
}

// -- the three ways a guest traps -------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Fault {
    /// The guest panics: its message goes to stderr, then it aborts.
    Panic,
    /// The guest allocates 256 MiB against a 64 MiB limit: the allocation
    /// failure message goes to stderr, then it aborts.
    OutOfMemory,
    /// The guest loops forever; the epoch deadline cuts it off.
    Hang,
    /// The guest sleeps for five seconds, which means waiting on a clock
    /// pollable. The sandbox has no blocking waits: the call is a trap at once
    /// instead of holding a host thread that the epoch deadline cannot reach.
    Sleep,
}

impl Fault {
    fn magic(self) -> Value {
        match self {
            Self::Panic => json!({ "__panic": true }),
            Self::OutOfMemory => json!({ "__grow_mb": 256 }),
            Self::Hang => json!({ "__hang": true }),
            Self::Sleep => json!({ "__sleep_ms": 5000 }),
        }
    }

    /// A deadline is the one trap the caller can act on differently; the others are plain traps.
    /// Neither may be the leftover of an earlier trap on the same instance.
    fn assert_refused(self, result: Result<String, CallErrorV1>) {
        match (self, result) {
            (Self::Hang, Err(CallErrorV1::Deadline)) => {}
            (Self::Panic | Self::OutOfMemory | Self::Sleep, Err(CallErrorV1::Trap(message))) => {
                assert!(
                    !message.contains("poisoned") && !message.contains("cannot enter"),
                    "a fresh trap must report itself, not the wreck of an earlier one: {message}"
                );
            }
            (fault, other) => panic!("{fault:?} must come back as its own error, got {other:?}"),
        }
    }
}

// -- the carriers: what is called when the trap happens ---------------------

#[derive(Clone, Copy, Debug)]
enum Carrier {
    /// A provider-world call on the component's shared instance.
    Provider,
    /// An embeddings-world call on the component's shared instance.
    Embeddings,
    /// A chunk fed to a stream, which has an instance of its own.
    Stream,
}

fn provider_config(extra: &Value) -> String {
    let mut base = json!({
        "provider": "test",
        "base_url": "https://api.test.example/v1",
        "auth": "provider_api_key",
        "models": [{ "model": "test-1", "tool": true, "context_window": 8192 }],
    });
    base.as_object_mut().expect("object").extend(extra.as_object().cloned().unwrap_or_default());
    base.to_string()
}

fn embeddings_config(extra: &Value) -> String {
    let mut base = json!({ "base_url": "https://embeddings.test.example" });
    base.as_object_mut().expect("object").extend(extra.as_object().cloned().unwrap_or_default());
    base.to_string()
}

/// One call on the carrier, with `extra` merged into its input: `{}` is the honest call.
fn call(
    carrier: Carrier,
    component: &LoadedComponentV1,
    extra: &Value,
) -> Result<String, CallErrorV1> {
    match carrier {
        Carrier::Provider => component.call_model_capabilities(&provider_config(extra)),
        Carrier::Embeddings => {
            component.call_build_embeddings_request(&embeddings_config(extra), r#"{"input":"hi"}"#)
        }
        Carrier::Stream => {
            let mut stream = component.open_stream().expect("a stream opens");
            let chunk = if extra.as_object().is_some_and(|object| !object.is_empty()) {
                extra.to_string().into_bytes()
            } else {
                b"data: hello\n\n".to_vec()
            };
            stream.parse_chunk(&chunk)
        }
    }
}

/// Trap, then call again, then trap again and call again: the component outlives its traps.
fn trap_then_recover(carrier: Carrier, fault: Fault, component: &LoadedComponentV1) {
    call(carrier, component, &json!({})).expect("the control call succeeds before any trap");
    let started = Instant::now();
    for _ in 0..2 {
        fault.assert_refused(call(carrier, component, &fault.magic()));
        call(carrier, component, &json!({}))
            .unwrap_or_else(|error| panic!("the call after a {fault:?} must succeed: {error:?}"));
    }
    if matches!(fault, Fault::Sleep) {
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "a sleeping guest must be refused, not waited for: {:?}",
            started.elapsed()
        );
    }
    // The shared instance answers too, whichever world the carrier was in.
    if matches!(carrier, Carrier::Stream) {
        call(Carrier::Provider, component, &json!({})).expect("the shared instance is untouched");
    }
}

fn run(hosting: Hosting, carrier: Carrier, fault: Fault) {
    let component = Arc::new(match carrier {
        Carrier::Embeddings => load_embeddings(),
        Carrier::Provider | Carrier::Stream => load_provider(),
    });
    hosted(hosting, move || trap_then_recover(carrier, fault, &component));
}

macro_rules! trap_matrix {
    ($($module:ident => $hosting:expr),* $(,)?) => {$(
        mod $module {
            use super::*;

            #[test]
            fn provider_panic() {
                run($hosting, Carrier::Provider, Fault::Panic);
            }
            #[test]
            fn provider_out_of_memory() {
                run($hosting, Carrier::Provider, Fault::OutOfMemory);
            }
            #[test]
            fn provider_deadline() {
                run($hosting, Carrier::Provider, Fault::Hang);
            }
            #[test]
            fn provider_sleep() {
                run($hosting, Carrier::Provider, Fault::Sleep);
            }
            #[test]
            fn embeddings_sleep() {
                run($hosting, Carrier::Embeddings, Fault::Sleep);
            }
            #[test]
            fn embeddings_panic() {
                run($hosting, Carrier::Embeddings, Fault::Panic);
            }
            #[test]
            fn embeddings_out_of_memory() {
                run($hosting, Carrier::Embeddings, Fault::OutOfMemory);
            }
            #[test]
            fn embeddings_deadline() {
                run($hosting, Carrier::Embeddings, Fault::Hang);
            }
            #[test]
            fn stream_panic() {
                run($hosting, Carrier::Stream, Fault::Panic);
            }
            #[test]
            fn stream_out_of_memory() {
                run($hosting, Carrier::Stream, Fault::OutOfMemory);
            }
            #[test]
            fn stream_deadline() {
                run($hosting, Carrier::Stream, Fault::Hang);
            }
        }
    )*};
}

trap_matrix! {
    multi_thread_runtime => Hosting::TokioWorker,
    current_thread_runtime => Hosting::TokioLocal,
    bare_thread => Hosting::BareThread,
}

/// The one host code that runs while the component's lock is held is the caller's own signer. If
/// it panics, the call panics, as it must; the component must still answer afterwards.
#[test]
fn a_signer_that_panics_does_not_leave_the_component_unusable() {
    use std::sync::atomic::{AtomicBool, Ordering};

    struct PanicsOnce(AtomicBool);
    impl SecretSignerV1 for PanicsOnce {
        fn sign(&self, _: &str, _: &[u8], _: &str) -> Result<Vec<u8>, String> {
            assert!(self.0.swap(true, Ordering::SeqCst), "the signer's own bug");
            Ok(vec![1, 2, 3])
        }
    }

    let component = load_provider_with(PanicsOnce(AtomicBool::new(false)));
    let signing_request = json!({
        "model": "test-1",
        "messages": [],
        "__sign": { "secret": "provider_api_key", "algorithm": "hmac-sha256" },
    })
    .to_string();

    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        component.call_build_http_request(&signing_request, &provider_config(&json!({})))
    }));
    assert!(panicked.is_err(), "the signer's panic belongs to the caller");

    component
        .call_build_http_request(&signing_request, &provider_config(&json!({})))
        .expect("the next call replaces the instance the panic left behind and signs");
    call(Carrier::Provider, &component, &json!({})).expect("and the component answers");
}
