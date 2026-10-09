//! Real image-world package admission, world separation and runtime limits.
//!
//! The guest (`tests/guests/test-image`) answers with canned documents that
//! echo its inputs; it proves the world's plumbing, not a dialect, so these
//! tests stay on the runtime's JSON face and never read the contract types.
//! The cases are the embeddings world's (`embeddings_world_v1.rs`), ported:
//! the image world is the second pure world without a `host` import
//! (2026-09-30 image-world record, §4 and §19 S-I-4).

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

/// Builds the image guest once per test process.
fn image_guest_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| build_guest("test-image", "test_image.wasm"))
}

/// Builds the provider-world guest once per test process.
fn provider_guest_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| build_guest("test-provider", "test_provider.wasm"))
}

/// Builds the embeddings-world guest once per test process.
fn embeddings_guest_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| build_guest("test-embeddings", "test_embeddings.wasm"))
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

/// The image test package. It declares no south contract: the release that ships the world
/// records `contracts.media` and `contracts.image` in `compatibility.json` (§19 S-I-8), and the
/// runtime's plumbing does not depend on them.
fn image_manifest_value() -> Value {
    json!({
        "name": "test-image",
        "version": "1.0.0",
        "api_version": "image-adapter-v1",
        "providers": ["test"],
        "capabilities": ["generate", "edit"],
        "auth_arms": ["bearer"],
        "permissions": { "network": false, "filesystem": false, "secrets": ["provider_api_key"] },
        "conformance": { "required_suite": "south.image-component.v1", "fixtures": "fixtures/" },
        "compatibility": compatibility("token-station:image-adapter@1.0.0", &json!({})),
    })
}

fn image_manifest() -> String {
    image_manifest_value().to_string()
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

fn embeddings_manifest() -> String {
    json!({
        "name": "test-embeddings",
        "version": "1.0.0",
        "api_version": "embeddings-adapter-v1",
        "providers": ["test"],
        "capabilities": ["embed"],
        "auth_arms": ["bearer"],
        "permissions": { "network": false, "filesystem": false, "secrets": ["provider_api_key"] },
        "conformance": { "required_suite": "south.embeddings-component.v1", "fixtures": "fixtures/" },
        "compatibility": compatibility(
            "token-station:embeddings-adapter@1.0.0",
            &json!({ "embeddings": 1 }),
        ),
    })
    .to_string()
}

fn package(name: &str, manifest: &str, wasm: &Path) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("south-image-{}-{seq}-{name}", std::process::id()));
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
    let wasm = std::fs::read(image_guest_wasm()).expect("guest bytes");
    load_embedded(&runtime, &image_manifest(), &wasm).expect("the image guest loads")
}

fn parsed(output: &str) -> Value {
    serde_json::from_str(output).expect("the guest answers JSON")
}

#[test]
fn the_image_guest_loads_and_answers_its_four_calls() {
    let dir = package("loads", &image_manifest(), image_guest_wasm());
    let loaded = LoadedComponentV1::load(&runtime(), &dir, &host_range::host_range(), FixedSigner)
        .expect("the image package loads");
    assert_eq!(loaded.metadata().api_version, "image-adapter-v1");
    assert_eq!(loaded.metadata().name, "test-image");

    let capabilities = loaded
        .call_image_model_capabilities(r#"{"base_url":"https://x.test"}"#)
        .expect("model-capabilities answers");
    assert_eq!(parsed(&capabilities)[0]["config"], json!({ "base_url": "https://x.test" }));

    let prepared = loaded
        .call_image_prepare(
            r#"{"base_url":"https://x.test"}"#,
            r#"{"prompt":"a cat"}"#,
            r#"{"n":1}"#,
        )
        .expect("prepare answers");
    let prepared = parsed(&prepared);
    assert_eq!(prepared["descriptor"]["body"], json!({ "prompt": "a cat" }));
    assert_eq!(prepared["state"]["config"], json!({ "base_url": "https://x.test" }));
    assert_eq!(prepared["state"]["context"], json!({ "n": 1 }));

    let outcome = loaded
        .call_image_parse_response(r#"{"round":1}"#, r#"{"status":200}"#)
        .expect("parse-response answers");
    let outcome = parsed(&outcome);
    assert_eq!(outcome["state"], json!({ "round": 1 }));
    assert_eq!(outcome["response"], json!({ "status": 200 }));

    let rendered = loaded
        .call_image_render(r#"{"round":1}"#, r#"[{"outcome":"succeeded"}]"#, r#"{"created":7}"#)
        .expect("render answers");
    let rendered = parsed(&rendered);
    assert_eq!(rendered["state"], json!({ "round": 1 }));
    assert_eq!(rendered["outcomes"], json!([{ "outcome": "succeeded" }]));
    assert_eq!(rendered["render_context"], json!({ "created": 7 }));

    // The guest's error channel stays opaque: the runtime hands it back as is.
    let Err(CallErrorV1::Component(envelope)) = loaded.call_image_prepare("{}", "not json", "{}")
    else {
        panic!("a guest error must arrive on the component error channel");
    };
    assert_eq!(parsed(&envelope)["code"], "internal");
}

#[test]
fn image_refuses_every_host_import_before_world_instantiation() {
    for name in [
        "token-station:adapter/host@2.0.0",
        "token-station:task-adapter/host@1.0.0",
        "token-station:task-adapter/host@2.0.0",
        "token-station:embeddings-adapter/host@1.0.0",
        "token-station:image-adapter/host@1.0.0",
        "token-station:image-adapter/image-adapter@1.0.0",
        "acme:lookalike/host@1.0.0",
        "acme:lookalike/host",
    ] {
        let wat = format!("(component (import \"{name}\" (instance)))");
        let error = load_embedded(&runtime(), &image_manifest(), wat.as_bytes())
            .expect_err("the image world is granted no host import");
        assert!(
            matches!(error, LoadErrorV1::ForbiddenImport(ref actual) if actual == name),
            "the import itself must be refused, before a missing export could disguise the \
             permission gap: {error}"
        );
    }

    // The scan is not over-broad: an ordinary WASI import passes it, and the
    // package then fails only because these bytes export no image world.
    let wat = "(component (import \"wasi:cli/environment@0.2.0\" (instance)))";
    let error =
        load_embedded(&runtime(), &image_manifest(), wat.as_bytes()).expect_err("no image export");
    assert!(
        matches!(error, LoadErrorV1::Probe { ref world, .. } if world == "image-adapter-v1"),
        "{error}"
    );
}

#[test]
fn image_calls_refuse_the_other_worlds_faces() {
    let dir = package("faces", &image_manifest(), image_guest_wasm());
    let loaded = LoadedComponentV1::load(&runtime(), &dir, &host_range::host_range(), FixedSigner)
        .expect("image");
    let refused = |outcome: Result<String, CallErrorV1>, face: &str| match outcome {
        Err(CallErrorV1::Trap(message)) => assert!(
            message.contains("exports `image-adapter-v1`") && message.contains(face),
            "{message}"
        ),
        other => panic!("the {face} must be refused on an image component: {other:?}"),
    };
    // The provider world's exports of the same WIT names are not the image ones.
    refused(loaded.call_model_capabilities("{}"), "provider face");
    refused(loaded.call_parse_response("{}"), "provider face");
    refused(loaded.call_parse_observation("{}"), "task face");
    refused(loaded.call_parse_observation_v2("{}"), "task-v2 face");
    refused(loaded.call_map_embeddings_provider_error("{}"), "embeddings face");
    assert!(matches!(
        loaded.open_stream().and_then(|mut stream| stream.parse_chunk(b"")),
        Err(CallErrorV1::Trap(_))
    ));
}

#[test]
fn image_bytes_cannot_claim_another_world_or_a_different_identity() {
    let bytes = std::fs::read(image_guest_wasm()).expect("guest");

    let mut embeddings = image_manifest_value();
    embeddings["api_version"] = "embeddings-adapter-v1".into();
    embeddings["capabilities"] = json!(["embed"]);
    embeddings["compatibility"]["wit_package"] = "token-station:embeddings-adapter@1.0.0".into();
    embeddings["compatibility"]["contracts"] = json!({ "embeddings": 1 });
    embeddings["conformance"]["required_suite"] = "south.embeddings-component.v1".into();
    let error = load_embedded(&runtime(), &embeddings.to_string(), &bytes)
        .expect_err("image bytes are not an embeddings component");
    assert!(
        matches!(error, LoadErrorV1::Probe { ref world, .. } if world == "embeddings-adapter-v1")
    );

    let mut provider = image_manifest_value();
    provider["api_version"] = "provider-adapter-v2".into();
    provider["capabilities"] = json!(["chat"]);
    provider["compatibility"]["wit_package"] = "token-station:adapter@2.0.0".into();
    provider["conformance"]["required_suite"] = "south.provider-component.v1".into();
    let error = load_embedded(&runtime(), &provider.to_string(), &bytes)
        .expect_err("image bytes are not a provider component");
    assert!(
        matches!(error, LoadErrorV1::Probe { ref world, .. } if world == "provider-adapter-v2")
    );

    let mut renamed = image_manifest_value();
    renamed["version"] = "9.9.9".into();
    let error =
        load_embedded(&runtime(), &renamed.to_string(), &bytes).expect_err("identity mismatch");
    assert!(format!("{error}").contains("not what it claims"));

    // And the other way round. Provider bytes import the provider world's
    // `host`, which the image scan refuses before instantiation is attempted;
    // embeddings bytes import nothing, and fail because they export no image
    // world.
    let provider_bytes = std::fs::read(provider_guest_wasm()).expect("provider guest");
    let mut claims = image_manifest_value();
    claims["name"] = "test-provider".into();
    let error = load_embedded(&runtime(), &claims.to_string(), &provider_bytes)
        .expect_err("provider bytes are not an image component");
    assert!(
        matches!(error, LoadErrorV1::ForbiddenImport(ref name) if name.starts_with("token-station:adapter/host@")),
        "{error}"
    );
    let embeddings_bytes = std::fs::read(embeddings_guest_wasm()).expect("embeddings guest");
    let mut claims = image_manifest_value();
    claims["name"] = "test-embeddings".into();
    let error = load_embedded(&runtime(), &claims.to_string(), &embeddings_bytes)
        .expect_err("embeddings bytes are not an image component");
    assert!(
        matches!(error, LoadErrorV1::Probe { ref world, .. } if world == "image-adapter-v1"),
        "{error}"
    );
}

#[test]
fn image_admission_keeps_both_handshakes() {
    let bytes = std::fs::read(image_guest_wasm()).expect("guest");

    // The range handshake refuses a contract number this host does not decode.
    let mut newer = image_manifest_value();
    newer["compatibility"]["contracts"] = json!({ "image": 999 });
    let error = load_embedded(&runtime(), &newer.to_string(), &bytes)
        .expect_err("image contract 999 is unknown");
    assert!(matches!(error, LoadErrorV1::OutsideRange(_)), "{error}");

    // The exact handshake: a host holding the manifest's own declaration,
    // except for the runtime, which no release can equal.
    let mut host = host_range::exact_expectations_for(&image_manifest());
    host.south_runtime = "0.0.0".into();
    let error =
        LoadedComponentV1::load_embedded(&runtime(), &image_manifest(), &bytes, &host, FixedSigner)
            .expect_err("host tuple must not come from manifest");
    assert!(format!("{error}").contains("south runtime"));

    // Gate ① runs before the bytes are opened: a manifest declaring neither
    // operation word is refused as a manifest.
    let mut no_operation = image_manifest_value();
    no_operation["capabilities"] = json!([]);
    let error = load_embedded(&runtime(), &no_operation.to_string(), &bytes)
        .expect_err("an image manifest needs an operation word");
    assert!(matches!(error, LoadErrorV1::Manifest(_)), "{error}");
}

#[test]
fn image_bounds_input_output_and_guest_error_output() {
    let limits = |max_payload_bytes| RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_secs(5),
        max_payload_bytes,
    };

    let tiny = load_with_limits(limits(2));
    for outcome in [
        tiny.call_image_model_capabilities("oversized"),
        tiny.call_image_prepare("{}", "oversized", "{}"),
        tiny.call_image_parse_response("{}", "oversized"),
        tiny.call_image_render("{}", "{}", "oversized"),
    ] {
        assert!(matches!(outcome, Err(CallErrorV1::PayloadTooLarge { limit: 2 })));
    }
    // Every input fits exactly; each answer, success or error, must
    // independently hit the bound.
    for outcome in [
        tiny.call_image_model_capabilities("{}"),
        tiny.call_image_prepare("{}", "{}", "{}"),
        tiny.call_image_parse_response("{}", "{}"),
        tiny.call_image_render("{}", "{}", "{}"),
        tiny.call_image_prepare("{}", "[x", "{}"),
    ] {
        assert!(matches!(outcome, Err(CallErrorV1::PayloadTooLarge { limit: 2 })));
    }

    let config = r#"{"base_url":"https://images.example.test"}"#;
    let request = r#"{"model":"test-image-model","prompt":"a cat"}"#;
    let normal = load_with_limits(limits(1024 * 1024));
    let prepared = normal.call_image_prepare(config, request, "{}").expect("valid prepare");
    let limit = config.len().max(request.len());
    assert!(prepared.len() > limit, "the output, not an input, must exceed the limit");
    let bounded = load_with_limits(limits(limit));
    assert!(matches!(
        bounded.call_image_prepare(config, request, "{}"),
        Err(CallErrorV1::PayloadTooLarge { limit: actual }) if actual == limit
    ));
}

/// The runtime limits are unchanged for this world (image record §15, §18.6): with the default
/// limits, a payload above 16 MiB is refused before the guest is entered, in every argument
/// position, and the component keeps answering afterwards.
#[test]
fn image_refuses_a_payload_above_the_default_16_mib() {
    let limits = RuntimeLimitsV1::default();
    assert_eq!(limits.max_payload_bytes, 16 * 1024 * 1024);
    let loaded = load_with_limits(limits);
    let oversized = format!("\"{}\"", "a".repeat(16 * 1024 * 1024 - 1));
    assert_eq!(oversized.len(), 16 * 1024 * 1024 + 1);
    for outcome in [
        loaded.call_image_model_capabilities(&oversized),
        loaded.call_image_prepare("{}", &oversized, "{}"),
        loaded.call_image_prepare(&oversized, "{}", "{}"),
        loaded.call_image_prepare("{}", "{}", &oversized),
        loaded.call_image_parse_response("{}", &oversized),
        loaded.call_image_parse_response(&oversized, "{}"),
        loaded.call_image_render("{}", &oversized, "{}"),
        loaded.call_image_render("{}", "{}", &oversized),
    ] {
        assert!(
            matches!(outcome, Err(CallErrorV1::PayloadTooLarge { limit }) if limit == 16 * 1024 * 1024),
            "{outcome:?}"
        );
    }
    assert!(loaded.call_image_parse_response("{}", "{}").is_ok());
}

/// The same inputs give byte-identical answers: repeated on one instance, and on a second
/// instance loaded from the same bytes. The world's functions are pure (image record §7), and the
/// runtime adds nothing per call.
#[test]
fn image_calls_are_deterministic() {
    let config = r#"{"base_url":"https://images.example.test","declared":{"region":"global"}}"#;
    let request = r#"{"model":"test-image-model","prompt":"a cat","n":2}"#;
    let context = r#"{"operation":"generate"}"#;
    let answers = |loaded: &LoadedComponentV1| {
        let prepared = loaded.call_image_prepare(config, request, context).expect("prepare");
        let state = parsed(&prepared)["state"].to_string();
        let outcome = loaded.call_image_parse_response(&state, r#"{"status":200}"#).expect("parse");
        let outcomes = format!("[{outcome}]");
        let rendered =
            loaded.call_image_render(&state, &outcomes, r#"{"created":1}"#).expect("render");
        let capabilities = loaded.call_image_model_capabilities(config).expect("capabilities");
        [capabilities, prepared, outcome, rendered]
    };
    let first = load_with_limits(RuntimeLimitsV1::default());
    let once = answers(&first);
    assert_eq!(answers(&first), once, "the same instance answers the same bytes");
    let second = load_with_limits(RuntimeLimitsV1::default());
    assert_eq!(answers(&second), once, "a fresh instance answers the same bytes");
}

#[test]
fn image_embeddings_and_provider_worlds_load_in_one_runtime_and_keep_separate_faces() {
    let runtime = runtime();
    let image = load_embedded(
        &runtime,
        &image_manifest(),
        &std::fs::read(image_guest_wasm()).expect("image wasm"),
    )
    .expect("image coexists");
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
    assert_eq!(image.metadata().api_version, "image-adapter-v1");
    assert_eq!(embeddings.metadata().api_version, "embeddings-adapter-v1");
    assert_eq!(provider.metadata().api_version, "provider-adapter-v2");

    for (loaded, world) in
        [(&provider, "provider-adapter-v2"), (&embeddings, "embeddings-adapter-v1")]
    {
        for outcome in [
            loaded.call_image_model_capabilities("{}"),
            loaded.call_image_prepare("{}", "{}", "{}"),
            loaded.call_image_parse_response("{}", "{}"),
            loaded.call_image_render("{}", "{}", "{}"),
        ] {
            match outcome {
                Err(CallErrorV1::Trap(message)) => assert!(
                    message.contains(&format!("exports `{world}`; the image face")),
                    "{message}"
                ),
                other => panic!("a {world} component has no image face: {other:?}"),
            }
        }
    }
    assert!(matches!(image.call_build_embeddings_request("{}", "{}"), Err(CallErrorV1::Trap(_))));
    // Each keeps answering its own face after the others were called.
    assert!(image.call_image_parse_response("{}", "{}").is_ok());
    assert!(embeddings.call_map_embeddings_provider_error("{}").is_ok());
}
