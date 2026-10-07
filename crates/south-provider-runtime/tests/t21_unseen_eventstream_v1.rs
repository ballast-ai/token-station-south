//! The T21 eventstream guest (`tests/guests/t21-unseen-eventstream`) under a synthesized manifest
//! (`docs/design/2026-09-30-host-zero-vendor-boundary.md` §5.2, §5.3, §7.2, §7.3, §12 item 5).
//!
//! The guest is not released: an adopting host builds it from a south checkout, synthesizes the
//! manifest below (it owns the tuple values) and drives its generic `aws-eventstream` executor,
//! its buffered path and its declaration-selected `aws-sigv4` signing against it. This test
//! proves the south half: the synthesized manifest passes gate ①, the package loads under a host
//! range, and on the runtime's JSON face the guest builds the wire its module header documents,
//! parses the canonical re-encoding however it is split, refuses a body that skipped the
//! buffered path, and produces each rogue descriptor. The canonical frames are written out
//! literally; their exact bytes are pinned by the golden vectors of `south-contracts`.

#[path = "support/host_range.rs"]
mod host_range;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{Value, json};
use south_provider_api::{ComponentManifestV1, StreamFramingV1, StreamLocationV1};
use south_provider_runtime::{
    CallErrorV1, ComponentRuntimeV1, LoadedComponentV1, NoSecretsV1, RuntimeLimitsV1,
};

const FAMILY: &str = "t21-unseen-eventstream";
const BASE_URL: &str = "https://api.us-east-1.p21-unseen.test";
const CAP: u64 = 64;

/// Builds the guest once per test process and returns the component's path.
fn guest_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let guest_dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/guests/t21-unseen-eventstream");
        let status = Command::new("cargo")
            .args(["build", "--target", "wasm32-wasip2"])
            .current_dir(&guest_dir)
            .status()
            .expect("cargo is on PATH");
        assert!(status.success(), "the guest must build; `rustup target add wasm32-wasip2`");
        guest_dir.join("target/wasm32-wasip2/debug/t21_unseen_eventstream.wasm")
    })
}

/// The manifest a host synthesizes for this guest. Every value below is the host's choice except
/// the identity, the family, the request facts and the stream framing, which the guest's wire
/// fixes.
fn manifest() -> Value {
    json!({
        "name": "t21-unseen-eventstream",
        "version": "1.0.0",
        "api_version": "provider-adapter-v2",
        "providers": [FAMILY],
        "capabilities": ["chat", "stream"],
        "auth_arms": ["host_signed"],
        "emits": ["authorization", "x-amz-date", "x-amz-content-sha256", "x-amz-security-token"],
        "stream_framing": "aws-eventstream",
        "signing": {
            "scheme": "aws-sigv4",
            "service": "t21svc",
            "region": { "template_param": "region" },
            "credentials": {
                "access_key_id": "access_key_id",
                "secret_access_key": "secret_access_key",
                "session_token": "session_token",
            },
        },
        "credentials": {
            "schema": "south.credential-recipe.v1",
            "fields": {
                "access_key_id": { "secret": true, "required": true },
                "secret_access_key": { "secret": true, "required": true },
                "session_token": { "secret": true },
            },
        },
        "request_facts": {
            FAMILY: {
                "output_cap": ["/t21_limits/max_out"],
                "model": { "url": "/t21/models/{model}/invoke" },
                "stream": "none",
            },
        },
        "endpoint": { FAMILY: "https://api.{region}.p21-unseen.test" },
        "config_schema": {
            FAMILY: {
                "region": {
                    "syntax": "aws_region",
                    "required": true,
                    "description": "The region the synthetic upstream is called in.",
                },
            },
        },
        "permissions": { "network": false, "filesystem": false, "secrets": [] },
        "conformance": { "required_suite": "south.provider-component.v1", "fixtures": "fixtures/" },
        "compatibility": {
            "ir_schema_id": "token-station-protocol@0.5.0/v0.4.0",
            "kernel_version": "0.4.0",
            "kernel_revision": "8e34f5a089d0b9c7273b49ddb6952dd87e960019",
            "wit_package": "token-station:adapter@2.0.0",
            "south_runtime": env!("CARGO_PKG_VERSION"),
            "runtime_abi": 1,
            "kernel_contracts": { "canonical_ir": 3, "error_catalog": 1, "stream": 2 },
        },
    })
}

fn runtime() -> ComponentRuntimeV1 {
    // A generous deadline: these calls are tiny, and the deadline is not under test here.
    ComponentRuntimeV1::new(RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_secs(5),
        max_payload_bytes: 1024 * 1024,
    })
    .expect("engine builds")
}

/// One package directory per call: tests run in parallel.
fn package_dir(name: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("south-t21-{}-{seq}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir is writable");
    std::fs::write(dir.join("manifest.json"), manifest().to_string()).expect("manifest writes");
    std::fs::copy(guest_wasm(), dir.join("component.wasm")).expect("wasm copies");
    dir
}

fn load() -> LoadedComponentV1 {
    LoadedComponentV1::load(
        &runtime(),
        &package_dir("load"),
        &host_range::host_range(),
        NoSecretsV1,
    )
    .expect("the synthesized package loads")
}

fn config(auth: Option<&str>) -> String {
    let mut config = json!({
        "provider": FAMILY,
        "base_url": BASE_URL,
        "models": [{ "model": "t21-echo", "context_window": 8192 }],
    });
    if let Some(auth) = auth {
        config["auth"] = json!(auth);
    }
    config.to_string()
}

fn request(model: &str, stream: bool, cap: Option<u64>) -> String {
    let mut request = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": "be brief" },
            { "role": "user", "content": [{ "type": "text", "text": "hi" }] },
        ],
        "sampling": {},
        "stream": stream,
    });
    if let Some(cap) = cap {
        request["sampling"]["max_output_tokens"] = json!(cap);
    }
    request.to_string()
}

fn build(component: &LoadedComponentV1, model: &str, auth: Option<&str>) -> Value {
    let descriptor = component
        .call_build_http_request(&request(model, true, Some(CAP)), &config(auth))
        .expect("the guest builds a descriptor");
    serde_json::from_str(&descriptor).expect("descriptor json")
}

/// The descriptor the honest wire builds for `segment` (the model, already encoded as one path
/// segment) without auth.
fn honest(segment: &str) -> Value {
    json!({
        "method": "POST",
        "url": format!("{BASE_URL}/t21/models/{segment}/invoke"),
        "headers": {
            "content-type": "application/json",
            "accept": "application/vnd.amazon.eventstream",
        },
        "body": {
            "t21_turns": [
                { "speaker": "system", "words": "be brief" },
                { "speaker": "user", "words": "hi" },
            ],
            "t21_limits": { "max_out": CAP },
        },
    })
}

fn component_error(result: Result<String, CallErrorV1>) -> Value {
    match result {
        Err(CallErrorV1::Component(envelope)) => {
            serde_json::from_str(&envelope).expect("the error payload is an envelope")
        }
        other => panic!("expected a component error payload, got {other:?}"),
    }
}

/// The canonical re-encoding of one well-formed exchange: two text events, the meter and the
/// end. The second text carries escapes, which the re-encoding keeps byte for byte.
const STREAM: &str = concat!(
    "event: t21Say\ndata: {\"text\":\"Hel\"}\n\n",
    "event: t21Say\ndata: {\"text\":\"lo, \\\"w\\u00f6rld\\\"\"}\n\n",
    "event: t21Meter\ndata: {\"in\":11,\"out\":3}\n\n",
    "event: t21End\ndata: {\"reason\":\"complete\",\"ticket\":\"t21-0001\",",
    "\"served_model\":\"t21-echo\"}\n\n",
);

fn expected_stream_events() -> Value {
    json!([
        { "type": "delta", "index": 0, "content": "Hel" },
        { "type": "delta", "index": 0, "content": "lo, \"wörld\"" },
        { "type": "usage", "usage": { "input_tokens": 11, "output_tokens": 3 } },
        { "type": "done", "finish_reason": "stop" },
    ])
}

/// A well-formed `t21End`, for tests whose subject is what precedes it.
const END_FRAME: &[u8] =
    b"event: t21End\ndata: {\"reason\":\"complete\",\"ticket\":\"t\",\"served_model\":\"m\"}\n\n";

fn feed(component: &LoadedComponentV1, chunks: &[&[u8]]) -> Result<Value, CallErrorV1> {
    let mut stream = component.open_stream().expect("stream opens");
    let mut events = Vec::new();
    for chunk in chunks {
        let out = stream.parse_chunk(chunk)?;
        let parsed: Value = serde_json::from_str(&out).expect("events json");
        events.extend(parsed.as_array().expect("an event list").iter().cloned());
    }
    Ok(Value::Array(events))
}

fn parts(body: &str) -> String {
    json!({
        "status": 200,
        "headers": { "content-type": "application/vnd.amazon.eventstream" },
        "body": body,
    })
    .to_string()
}

// -- gate ① and loading -------------------------------------------------------

#[test]
fn the_synthesized_manifest_passes_gate_one_and_loads_under_a_host_range() {
    // Gate ① is the manifest's own validation; the loader runs it again below.
    let manifest: ComponentManifestV1 =
        serde_json::from_value(manifest()).expect("the manifest parses");
    manifest.validate().expect("gate ① admits the synthesized manifest");
    assert_eq!(manifest.stream_framing, StreamFramingV1::AwsEventstream);
    assert_eq!(manifest.request_facts_for(FAMILY).stream, StreamLocationV1::None);

    let component = load();
    let identity = component.metadata();
    assert_eq!(
        (identity.name.as_str(), identity.version.as_str(), identity.api_version.as_str()),
        ("t21-unseen-eventstream", "1.0.0", "provider-adapter-v2"),
        "the probe's reported identity equals the declared one"
    );

    let capabilities = component.call_model_capabilities(&config(None)).expect("capabilities");
    let capabilities: Value = serde_json::from_str(&capabilities).expect("capabilities json");
    assert_eq!(capabilities, json!([{ "model": "t21-echo", "context_window": 8192 }]));
}

// -- the request ----------------------------------------------------------------

#[test]
fn a_host_signed_request_carries_no_auth_and_has_no_stream_switch() {
    let component = load();

    let streaming = build(&component, "t21-echo", None);
    assert_eq!(streaming, honest("t21-echo"), "no auth, the model only in the URL");

    let buffered = component
        .call_build_http_request(&request("t21-echo", false, Some(CAP)), &config(None))
        .expect("a non-streaming caller builds too");
    let buffered: Value = serde_json::from_str(&buffered).expect("descriptor json");
    assert_eq!(buffered, streaming, "one URL and one body serve both callers");
}

#[test]
fn the_model_is_encoded_as_one_path_segment() {
    let component = load();
    let descriptor = build(&component, "acme/t21 model:v1", None);
    assert_eq!(
        descriptor["url"],
        json!(format!("{BASE_URL}/t21/models/acme%2Ft21%20model:v1/invoke"))
    );
}

#[test]
fn a_granted_slot_is_presented_as_bearer_for_a_bearer_variant_manifest() {
    let component = load();
    let descriptor = build(&component, "t21-echo", Some("provider_api_key"));
    let mut expected = honest("t21-echo");
    expected["auth"] = json!({ "scheme": "bearer", "secret": "provider_api_key" });
    assert_eq!(descriptor, expected);
}

#[test]
fn a_request_without_a_cap_is_refused() {
    let component = load();
    let refused =
        component.call_build_http_request(&request("t21-echo", true, None), &config(None));
    let envelope = component_error(refused);
    assert_eq!(envelope["code"], json!("capability"));
}

// -- the stream -----------------------------------------------------------------

#[test]
fn the_canonical_stream_yields_the_same_events_however_it_is_split() {
    let component = load();
    let bytes = STREAM.as_bytes();

    let whole = feed(&component, &[bytes]).expect("the stream parses");
    assert_eq!(whole, expected_stream_events());

    for split in 0..=bytes.len() {
        let events =
            feed(&component, &[&bytes[..split], &bytes[split..]]).expect("the stream parses");
        assert_eq!(events, whole, "split at byte {split}");
    }

    let bytewise: Vec<&[u8]> = bytes.chunks(1).collect();
    assert_eq!(feed(&component, &bytewise).expect("the stream parses"), whole, "byte by byte");
}

#[test]
fn an_exception_frame_ends_the_stream_with_an_error_event() {
    let component = load();
    let events = feed(
        &component,
        &[
            b"event: t21Say\ndata: {\"text\":\"Mi\"}\n\n",
            b"event: exception:t21Throttled\ndata: {\"message\":\"slow down\"}\n\n\
              event: t21Say\ndata: {\"text\":\"ld\"}\n\n",
            END_FRAME,
        ],
    )
    .expect("the stream parses");
    assert_eq!(
        events,
        json!([
            { "type": "delta", "index": 0, "content": "Mi" },
            { "type": "error", "error": {
                "code": "rate_limit",
                "http_status": 502,
                "message": "the upstream rate limited this request",
                "provider_message": "slow down",
            } },
        ]),
        "nothing is emitted after the error event"
    );
}

#[test]
fn an_error_frame_ends_the_stream_with_an_error_event() {
    let component = load();
    let events = feed(
        &component,
        &[
            b"event: error:T21Internal\ndata: {\"message\":\"upstream broke\"}\n\n",
            b"event: t21Meter\ndata: {\"in\":1,\"out\":1}\n\n",
        ],
    )
    .expect("the stream parses");
    assert_eq!(
        events,
        json!([{ "type": "error", "error": {
            "code": "upstream_unavailable",
            "http_status": 502,
            "message": "the upstream is unavailable",
            "provider_message": "upstream broke",
        } }])
    );
}

#[test]
fn frames_that_are_not_this_wire_are_refused_not_skipped() {
    let component = load();
    let refused: [&[u8]; 6] = [
        b"event: message\ndata: {}\n\n",
        b"data: {\"text\":\"hi\"}\n\n",
        b"event: t21Say\r\ndata: {\"text\":\"hi\"}\r\n\r\n",
        b"event: t21Say\ndata: {\"text\":\"hi\"}\nid: 7\n\n",
        b"event: t21Meter\ndata: {\"in\":1}\n\n",
        // A well-formed end with no meter before it: usage is never defaulted.
        END_FRAME,
    ];
    for frame in refused {
        let envelope = component_error(feed(&component, &[frame]).map(|events| events.to_string()));
        assert_eq!(
            envelope["code"],
            json!("provider_protocol_error"),
            "{}",
            String::from_utf8_lossy(frame)
        );
    }
}

// -- the buffered path ----------------------------------------------------------

#[test]
fn the_buffered_body_is_the_concatenated_re_encoding() {
    let component = load();
    let response = component.call_parse_response(&parts(STREAM)).expect("the body parses");
    let response: Value = serde_json::from_str(&response).expect("response json");
    assert_eq!(
        response,
        json!({
            "id": "t21-0001",
            "model": "t21-echo",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "Hello, \"wörld\"" },
                "finish_reason": "stop",
            }],
            "usage": { "input_tokens": 11, "output_tokens": 3 },
        })
    );
}

#[test]
fn a_plain_json_body_is_refused_on_the_buffered_path() {
    let component = load();
    for body in [
        r#"{"text":"Hello","in":11,"out":3}"#,
        "{\n\n\"t21_turns\": []\n}",
        "",
        "event: t21Say\ndata: {\"text\":\"Hel\"}\n\n",
    ] {
        let envelope = component_error(component.call_parse_response(&parts(body)));
        assert_eq!(envelope["code"], json!("provider_protocol_error"), "{body}");
    }
}

#[test]
fn an_exception_or_error_frame_in_the_buffered_body_is_a_provider_error() {
    let component = load();
    let throttled = concat!(
        "event: t21Say\ndata: {\"text\":\"Hel\"}\n\n",
        "event: exception:t21Throttled\ndata: {\"message\":\"slow down\"}\n\n",
    );
    let envelope = component_error(component.call_parse_response(&parts(throttled)));
    assert_eq!(envelope["code"], json!("rate_limit"));
    assert_eq!(envelope["provider_message"], json!("slow down"));

    let failed = "event: error:T21Internal\ndata: {\"message\":\"upstream broke\"}\n\n";
    let envelope = component_error(component.call_parse_response(&parts(failed)));
    assert_eq!(envelope["code"], json!("upstream_unavailable"));
}

// -- rogue modes ----------------------------------------------------------------

#[test]
fn each_rogue_mode_changes_exactly_its_own_field() {
    let component = load();

    let mut expected = honest("rogue-signed-auth");
    expected["auth"] = json!({ "scheme": "bearer", "secret": "provider_api_key" });
    assert_eq!(build(&component, "rogue-signed-auth", None), expected);

    let mut expected = honest("rogue-origin");
    expected["url"] = json!("https://rogue.p21-unseen.test/t21/models/rogue-origin/invoke");
    assert_eq!(build(&component, "rogue-origin", None), expected);

    let mut expected = honest("rogue-cap");
    let body = expected["body"].as_object_mut().expect("body object");
    body.remove("t21_limits");
    body.insert("max_tokens".to_owned(), json!(CAP));
    assert_eq!(build(&component, "rogue-cap", None), expected);

    let mut expected = honest("rogue-model-url");
    expected["url"] = json!(format!("{BASE_URL}/t21/models/t21-decoy/invoke"));
    assert_eq!(build(&component, "rogue-model-url", None), expected);
}

// -- errors ---------------------------------------------------------------------

#[test]
fn provider_errors_map_by_status() {
    let component = load();
    for (status, code) in [
        (401, "auth"),
        (403, "auth"),
        (429, "rate_limit"),
        (400, "capability"),
        (503, "upstream_unavailable"),
    ] {
        let parts = json!({ "status": status, "body": "" }).to_string();
        let envelope = component.call_map_provider_error(&parts).expect("an envelope");
        let envelope: Value = serde_json::from_str(&envelope).expect("envelope json");
        assert_eq!(envelope["code"], json!(code), "status {status}");
        assert_eq!(envelope["http_status"], json!(status));
    }
}
