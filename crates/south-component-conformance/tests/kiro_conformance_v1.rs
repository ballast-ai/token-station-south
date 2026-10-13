//! Gates ① and ② for the official Kiro provider component (`provider-kiro`), run against its native
//! reference implementation over its own frozen fixture pack, and the facts the design record
//! decided that a passing pack alone would not show.
//!
//! Design record: `docs/design/2026-09-30-kiro-provider-component.md`.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::reference_kiro::{FAMILY, KiroReferenceV1, NAME, VERSION};
use south_component_conformance::{
    FixturePackV1, ProviderComponentV1, accepts_manifest, reported_identity_matches,
    run_provider_component_suite_v1_for_manifest,
};
use south_contracts::{EventStreamMessageV1, deframe_aws_eventstream_v1, reencode_eventstream_v1};
use south_provider_api::{
    ComponentManifestV1, HOST_VALUE_ATTEMPT_ID, PROVIDER_WORLD, StreamFramingV1, UsageEvidenceV1,
    compatibility_admits,
};
use token_station_protocol::{
    ChatRequest, HttpRequestDescriptor, HttpResponseParts, ProviderConfig, StreamEvent,
};

#[path = "support/host_range.rs"]
mod host_range;

const PACK: &str = "fixtures-kiro";

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

fn shipped_manifest() -> ComponentManifestV1 {
    let source =
        std::fs::read_to_string(repo_root().join("components/provider-kiro/manifest.json"))
            .expect("the shipped component manifest reads");
    serde_json::from_str(&source).expect("the shipped component manifest parses")
}

fn shipped_pack() -> FixturePackV1 {
    FixturePackV1::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join(PACK))
        .expect("the shipped fixture pack loads")
}

fn read_json(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(PACK).join(name);
    serde_json::from_str(&std::fs::read_to_string(&path).expect("the fixture reads"))
        .expect("the fixture parses")
}

/// Gate ①: the package is admissible under this release's range, and the identity the component
/// reports is the one its manifest claims.
#[test]
fn gate_one_admits_the_shipped_package_and_its_reported_identity() {
    let manifest = shipped_manifest();
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    assert!(compatibility_admits(&manifest, &host_range::host_range()).is_ok());
    assert!(reported_identity_matches(&KiroReferenceV1.metadata(), &manifest));
    assert_eq!((manifest.name.as_str(), manifest.version.as_str()), (NAME, VERSION));
    assert_eq!(manifest.api_version, PROVIDER_WORLD);
    assert_eq!(manifest.providers, [FAMILY]);
}

/// Gate ②: the reference implementation answers its own frozen pack exactly.
#[test]
fn gate_two_passes_over_the_shipped_pack() {
    let report = run_provider_component_suite_v1_for_manifest(
        &KiroReferenceV1,
        &shipped_pack(),
        &shipped_manifest(),
    );
    let failures: Vec<String> = report.failures().map(|outcome| format!("{outcome:?}")).collect();
    assert!(failures.is_empty(), "{} gate ② failures:\n{}", failures.len(), failures.join("\n"));
}

/// The declarations the record decided (§3.4, §4.6, §6, §8; boundary §5.2, §7.2, §10, §13.8),
/// spelled out, so that a manifest edit that still passes gate ① cannot quietly drop one.
#[test]
fn the_manifest_declares_what_the_record_decided() {
    let manifest = shipped_manifest();
    assert_eq!(manifest.usage_evidence, UsageEvidenceV1::Absent);
    assert_eq!(manifest.stream_framing, StreamFramingV1::AwsEventstream);
    assert_eq!(manifest.auth_arms.iter().collect::<Vec<_>>(), ["bearer"]);
    assert_eq!(manifest.permissions.secrets, ["provider_api_key"]);
    assert_eq!(manifest.host_values, [HOST_VALUE_ATTEMPT_ID]);
    assert_eq!(
        manifest.capabilities.iter().collect::<Vec<_>>(),
        ["chat", "stream", "tool_call"],
        "no json_schema: the wire has no slot for it"
    );
    assert_eq!(
        serde_json::to_value(&manifest.request_facts).unwrap(),
        json!({FAMILY: {"output_cap": [],
                        "model": {"body": "/conversationState/currentMessage/userInputMessage/modelId"},
                        "stream": "none"}})
    );
    assert_eq!(
        manifest.user_agent.get(FAMILY).map(String::as_str),
        Some("aws-sdk-js/1.0.0 KiroIDE"),
        "the value the native arm sends"
    );
    assert!(manifest.subscription_quota_families.contains(FAMILY));
    assert!(manifest.endpoint.is_empty(), "the inference endpoint is the provider row's base_url");
    assert!(manifest.config_schema.is_empty(), "the component reads no configuration key");
    assert_eq!(manifest.declared_keys(FAMILY).into_iter().collect::<Vec<_>>(), ["profile_arn"]);
    let credentials = manifest.credentials.as_ref().expect("a credentials section");
    assert_eq!(
        credentials.endpoints(),
        [
            "https://oidc.{region}.amazonaws.com/token",
            "https://prod.{region}.auth.desktop.kiro.dev/refreshToken",
        ],
        "the two token endpoints the operator confirms, and no other"
    );
}

/// The rows the design record lists (§11.2) and the refusals it numbers. A pack can pass while
/// having quietly lost a case.
#[test]
fn the_shipped_pack_carries_every_decided_row() {
    let pack = shipped_pack();
    let names: Vec<String> = pack
        .cases()
        .iter()
        .map(|case| case.name.clone())
        .chain(pack.credentials().cases().iter().map(|case| case.name.clone()))
        .collect();
    for row in [
        "provider.request.chat",
        "provider.request.no-profile-arn",
        "provider.request.stream-is-the-same-request",
        "provider.request.system-tools-and-tool-results",
        "provider.request.system-without-history-prepends-to-the-current-message",
        "provider.request.system-replaces-an-empty-first-user-entry",
        "provider.request.parallel-tool-results-are-separate-user-entries",
        "provider.request.tool-result-then-user",
        "provider.request.tool-choice-none-withholds-tools",
        "provider.request.sampling-has-no-slot",
        "provider.request.model-is-verbatim",
        "provider.request.reasoning-parts-are-dropped",
        "provider.request.prompt-cache-markers-are-dropped",
        "provider.request.refused-image",
        "provider.request.refused-unknown-part",
        "provider.request.refused-required-tool-choice",
        "provider.request.refused-named-tool-choice",
        "provider.request.refused-assistant-final-turn",
        "provider.request.empty-arguments-are-an-empty-object",
        "provider.request.non-object-arguments-pass-through",
        "provider.request.refused-empty-tool-call-id",
        "provider.request.refused-tool-result-without-id",
        "provider.request.refused-json-response-format",
        "provider.request.refused-no-message",
        "provider.request.refused-missing-attempt-id",
        "provider.request.refused-malformed-attempt-id",
        "provider.response.text",
        "provider.response.tool-use",
        "provider.response.no-frames",
        "provider.response.open-tool-call",
        "provider.response.exception-frame",
        "provider.response.error-frame",
        "provider.stream.text",
        "provider.stream.tool-use",
        "provider.stream.text-then-tool",
        "provider.stream.metering-is-never-usage",
        "provider.stream.unknown-event-ignored",
        "provider.stream.empty-content-ignored",
        "provider.stream.exception-frame",
        "provider.stream.error-frame",
        "provider.stream.open-tool-call-at-eof",
        "provider.stream.no-frames",
        "provider.stream.tool-call-without-stop-at-eof",
        "provider.stream.tool-event-without-id-is-the-empty-id",
        "provider.stream.tool-input-not-a-string-is-no-fragment",
        "provider.stream.tool-input-after-stop-extends-the-call",
        "provider.error.rejected-credential",
        "provider.error.throttled-carries-retry-after",
        "provider.error.opaque-500",
        "provider.capabilities.declared",
        "credential.rotation.social-refresh",
        "credential.rotation.social-without-new-token-keeps-the-stored-one",
        "credential.rotation.idc-refresh",
        "credential.clock.expiry-default",
        "credential.clock.select-client-fields-pick-idc",
        "credential.clock.select-explicit-social-wins-over-client-fields",
        "credential.clock.select-enterprise-is-idc",
        "credential.clock.no-refresh-token-fails-as-configuration",
        "credential.clock.idc-missing-client-fields-sends-nothing",
        "credential.on-status.status-400",
        "credential.on-status.status-401",
        "credential.on-status.status-403-is-transient",
        "credential.on-status.status-500-is-transient",
    ] {
        assert!(names.iter().any(|name| name == row), "the pack lost `{row}`");
    }
}

// -- the stream rows are the host's bytes, re-encoded ------------------------------------------

/// CRC-32 (IEEE), for building eventstream frames the way the host's tests do.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// One eventstream frame with string headers, built as the host's `kiro.rs` and
/// `kiro_eventstream.rs` tests build theirs.
fn frame(headers: &[(&str, &str)], payload: &[u8]) -> Vec<u8> {
    let mut header_bytes = Vec::new();
    for (name, value) in headers {
        header_bytes.push(u8::try_from(name.len()).unwrap());
        header_bytes.extend_from_slice(name.as_bytes());
        header_bytes.push(7);
        header_bytes.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
        header_bytes.extend_from_slice(value.as_bytes());
    }
    let total = u32::try_from(16 + header_bytes.len() + payload.len()).unwrap();
    let mut frame = Vec::new();
    frame.extend_from_slice(&total.to_be_bytes());
    frame.extend_from_slice(&u32::try_from(header_bytes.len()).unwrap().to_be_bytes());
    frame.extend_from_slice(&crc32(&frame).to_be_bytes());
    frame.extend_from_slice(&header_bytes);
    frame.extend_from_slice(payload);
    frame.extend_from_slice(&crc32(&frame).to_be_bytes());
    frame
}

fn event(event_type: &str, payload: &[u8]) -> Vec<u8> {
    frame(
        &[
            (":message-type", "event"),
            (":event-type", event_type),
            (":content-type", "application/json"),
        ],
        payload,
    )
}

/// What the host hands the component for a binary body: south's deframer, then the canonical
/// re-encoding of each message (boundary §5.2).
fn re_encoded(body: &[u8]) -> String {
    deframe_aws_eventstream_v1(body)
        .expect("the host's frames deframe")
        .iter()
        .map(|message: &EventStreamMessageV1| reencode_eventstream_v1(message).expect("re-encodes"))
        .collect()
}

fn stream_body(row: &str) -> String {
    let input = read_json(&format!("provider.stream.{row}.input.json"));
    input["chunks"].as_array().unwrap().iter().map(|chunk| chunk.as_str().unwrap()).collect()
}

/// The stream rows taken from the host are its binary bodies byte for byte, deframed and
/// re-encoded the way every host must (record §11.2: inputs in the canonical re-encoding). Each
/// body is the one a host test feeds its native arm; the payload bytes are the ones that test
/// writes (serde's compact, key-sorted form where the test builds a JSON value).
#[test]
fn the_host_derived_stream_rows_are_the_hosts_bytes_re_encoded() {
    // gateway/tests/kiro_eventstream.rs `normal()`.
    let kiro_eventstream_normal = [
        event("assistantResponseEvent", br#"{"content":"hello from kiro"}"#),
        event("meteringEvent", br#"{"unit":"credit","usage":0.02}"#),
    ]
    .concat();
    assert_eq!(re_encoded(&kiro_eventstream_normal), stream_body("text"));

    // gateway/src/modules/inference/engine/kiro.rs tests `normal()`: a tool fragment with no start
    // and no stop, which the end of the body completes, as natively.
    let decoder_normal = [
        event("assistantResponseEvent", br#"{"content":"he"}"#),
        event("assistantResponseEvent", br#"{"content":"llo"}"#),
        event("toolUseEvent", br#"{"name":"f","toolUseId":"t1","input":"{\"a\":1}"}"#),
        event("meteringEvent", br#"{"unit":"credit","usage":0.02}"#),
    ]
    .concat();
    assert_eq!(re_encoded(&decoder_normal), stream_body("open-tool-call-at-eof"));

    // translate_kiro.rs `kiro_accumulate_builds_tool_use_from_streamed_input_fragments`, "exactly
    // the captured sequence".
    let captured_tool = [
        event("toolUseEvent", br#"{"name":"get_weather","toolUseId":"tooluse_X"}"#),
        event(
            "toolUseEvent",
            br#"{"input":"{\"city\": \"P","name":"get_weather","toolUseId":"tooluse_X"}"#,
        ),
        event(
            "toolUseEvent",
            br#"{"input":"aris\"}","name":"get_weather","toolUseId":"tooluse_X"}"#,
        ),
        event("toolUseEvent", br#"{"name":"get_weather","stop":true,"toolUseId":"tooluse_X"}"#),
        event("meteringEvent", br#"{"unit":"credit","usage":0.0267}"#),
    ]
    .concat();
    assert_eq!(re_encoded(&captured_tool), stream_body("tool-use"));

    // translate_kiro.rs `kiro_accumulate_builds_text_message_and_sums_credits`, with whitespace
    // in a payload: the re-encoding removes it and changes nothing else.
    let text = [
        event("assistantResponseEvent", br#"{"content": "Hi ", "modelId": "claude-haiku-4.5"}"#),
        event("assistantResponseEvent", br#"{"content":"there"}"#),
        event("contextUsageEvent", br#"{"contextUsagePercentage":2.0}"#),
        event("meteringEvent", br#"{"unit":"credit","usage":0.0055778}"#),
    ]
    .concat();
    assert_eq!(re_encoded(&text), stream_body("metering-is-never-usage"));

    // kiro.rs tests: the exception frame reaches the component as `exception:<type>`.
    let exception = frame(
        &[(":message-type", "exception"), (":exception-type", "ThrottlingException")],
        br#"{"message":"slow down"}"#,
    );
    assert!(stream_body("exception-frame").contains(&re_encoded(&exception)));
    // kiro.rs tests' `error` frame carries no `:error-message`: south's re-encoding refuses it, so
    // that body fails in the host's deframing step and never reaches the component, as it fails
    // natively. The pack's `error-frame` row is the well-formed shape (boundary §5.2).
    let bare_error = frame(&[(":message-type", "error"), (":error-code", "InternalError")], b"{}");
    let messages = deframe_aws_eventstream_v1(&bare_error).unwrap();
    assert!(reencode_eventstream_v1(&messages[0]).is_err());
}

// -- mutation checks (record §11.2) ---------------------------------------------------------

fn stream_events(chunks: &[String]) -> Vec<StreamEvent> {
    let mut parser = KiroReferenceV1.stream_parser();
    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(parser.parse_chunk(chunk.as_bytes()).expect("the chunk parses"));
    }
    events.extend(parser.finish().expect("the stream finishes"));
    events
}

/// *Metering inflation*: the component is deaf to the credit number. Multiplying every
/// `meteringEvent` value by 1,000 changes no stream and no response output.
#[test]
fn the_metering_number_moves_nothing() {
    let pack = shipped_pack();
    let mut inflated = 0;
    for case in pack.cases() {
        let is_stream = case.name.starts_with("provider.stream.");
        if !is_stream && !case.name.starts_with("provider.response.") {
            continue;
        }
        let text = case.input.to_string();
        if !text.contains("meteringEvent") {
            continue;
        }
        let mutated: Value = serde_json::from_str(
            &text
                .replace("\\\"usage\\\":0.02", "\\\"usage\\\":20")
                .replace("\\\"usage\\\":0.0267", "\\\"usage\\\":26.7")
                .replace("\\\"usage\\\":0.0055778", "\\\"usage\\\":5.5778"),
        )
        .unwrap();
        assert_ne!(mutated, case.input, "{}: the mutation must change the input", case.name);
        let run = |input: &Value| -> Value {
            if is_stream {
                let chunks: Vec<String> = serde_json::from_value(input["chunks"].clone()).unwrap();
                let mut parser = KiroReferenceV1.stream_parser();
                let mut events = Vec::new();
                for chunk in &chunks {
                    match parser.parse_chunk(chunk.as_bytes()) {
                        Ok(more) => events.extend(more),
                        Err(error) => return serde_json::to_value(error).unwrap(),
                    }
                }
                events.extend(parser.finish().unwrap());
                serde_json::to_value(events).unwrap()
            } else {
                let parts: HttpResponseParts = serde_json::from_value(input.clone()).unwrap();
                match KiroReferenceV1.parse_response(&parts) {
                    Ok(response) => serde_json::to_value(response).unwrap(),
                    Err(error) => serde_json::to_value(error).unwrap(),
                }
            }
        };
        assert_eq!(run(&mutated), run(&case.input), "{}", case.name);
        inflated += 1;
    }
    assert!(inflated >= 10, "only {inflated} rows carried a meteringEvent");
}

fn chat_row() -> (ChatRequest, ProviderConfig) {
    let input = read_json("provider.request.chat.input.json");
    (
        serde_json::from_value(input["chat_request"].clone()).unwrap(),
        serde_json::from_value(input["provider_config"].clone()).unwrap(),
    )
}

/// Every leaf of `value`, by JSON Pointer.
fn leaves(value: &Value, at: &str, out: &mut Vec<(String, Value)>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                leaves(child, &format!("{at}/{key}"), out);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                leaves(child, &format!("{at}/{index}"), out);
            }
        }
        leaf => out.push((at.to_owned(), leaf.clone())),
    }
}

/// *Attempt-id sensitivity*: a different attempt id changes exactly one body value, at
/// `/conversationState/conversationId`, and nothing else in the request (K-Q16, boundary §13.8).
#[test]
fn the_attempt_id_moves_exactly_the_conversation_id() {
    let (request, config) = chat_row();
    let mut other = request.clone();
    let fresh = "0e9d8c7b-6a5f-4e3d-8c1b-0a9f8e7d6c5b";
    other.host_values.insert(HOST_VALUE_ATTEMPT_ID.to_owned(), fresh.to_owned()).unwrap();
    let first = KiroReferenceV1.build_http_request(&request, &config).unwrap();
    let second = KiroReferenceV1.build_http_request(&other, &config).unwrap();
    assert_eq!(
        (&first.method, &first.url, &first.headers),
        (&second.method, &second.url, &second.headers)
    );
    assert_eq!(first.auth, second.auth);
    let (mut a, mut b) = (Vec::new(), Vec::new());
    leaves(first.body.as_ref().unwrap(), "", &mut a);
    leaves(second.body.as_ref().unwrap(), "", &mut b);
    let differing: Vec<&str> = a
        .iter()
        .zip(&b)
        .filter(|(left, right)| left != right)
        .map(|(left, _)| left.0.as_str())
        .collect();
    assert_eq!(a.len(), b.len());
    assert_eq!(differing, ["/conversationState/conversationId"]);
    assert_eq!(second.body.unwrap()["conversationState"]["conversationId"], json!(fresh));
}

/// *Frame reordering inside a tool call*: swapping two `input` fragments changes the
/// concatenated arguments accordingly; the component neither reorders nor deduplicates.
#[test]
fn tool_input_fragments_are_kept_in_wire_order() {
    let input = read_json("provider.stream.tool-use.input.json");
    let chunks: Vec<String> = serde_json::from_value(input["chunks"].clone()).unwrap();
    let mut swapped = chunks.clone();
    swapped.swap(1, 2);
    let arguments = |events: Vec<StreamEvent>| -> String {
        events
            .into_iter()
            .filter_map(|event| match event {
                StreamEvent::ToolCallDelta { arguments_delta, .. } => Some(arguments_delta),
                _ => None,
            })
            .collect()
    };
    assert_eq!(arguments(stream_events(&chunks)), "{\"city\": \"Paris\"}");
    assert_eq!(arguments(stream_events(&swapped)), "aris\"}{\"city\": \"P");
    let mut repeated = chunks.clone();
    repeated.insert(2, chunks[1].clone());
    assert_eq!(arguments(stream_events(&repeated)), "{\"city\": \"P{\"city\": \"Paris\"}");
}

// -- the two paths fold one parser (record §5.4) -------------------------------------------

/// A streaming and a non-streaming answer to the same upstream bytes fold to the same content,
/// tool calls and finish reason: `parse-response` runs the stream parser (the buffered path).
#[test]
fn the_buffered_path_folds_what_the_stream_emits() {
    for (row, response_row) in [
        ("text", "text"),
        ("metering-is-never-usage", "metering-is-never-usage"),
        ("tool-use", "tool-use"),
        ("text-then-tool", "text-then-tool"),
        ("two-tool-calls", "two-tool-calls"),
        ("open-tool-call-at-eof", "open-tool-call"),
    ] {
        let stream = read_json(&format!("provider.stream.{row}.input.json"));
        let response = read_json(&format!("provider.response.{response_row}.input.json"));
        let chunks: Vec<String> = serde_json::from_value(stream["chunks"].clone()).unwrap();
        assert_eq!(response["body"], json!(chunks.concat()), "{row}: the same upstream bytes");

        let events = stream_events(&chunks);
        let parts: HttpResponseParts = serde_json::from_value(response).unwrap();
        let folded = KiroReferenceV1.parse_response(&parts).unwrap();
        let message = &folded.choices[0].message;
        let text: String = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::Delta { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            message.content.as_ref().map(|content| serde_json::to_value(content).unwrap()),
            (!text.is_empty()).then(|| json!(text)),
            "{row}"
        );
        let calls = events
            .iter()
            .filter(|event| matches!(event, StreamEvent::ToolCallDelta { id: Some(_), .. }))
            .count();
        assert_eq!(message.tool_calls.len(), calls, "{row}");
        let done = events.iter().find_map(|event| match event {
            StreamEvent::Done { finish_reason, .. } => finish_reason.clone(),
            _ => None,
        });
        assert_eq!(folded.choices[0].finish_reason, done, "{row}");
        assert_eq!(folded.usage, token_station_protocol::Usage::default(), "{row}");
    }
}

/// The descriptor of every built request: the bearer slot, the URL off the row's `base_url`, the
/// four headers the native arm sends, no `content-type` (K-Q10) and no `user-agent` (declared in
/// the manifest, sent by the host).
#[test]
fn every_built_request_carries_the_native_headers_and_the_bearer_slot() {
    let mut built = 0;
    for case in shipped_pack().cases() {
        if !case.name.starts_with("provider.request.") || case.expected.get("error").is_some() {
            continue;
        }
        let descriptor: HttpRequestDescriptor =
            serde_json::from_value(case.expected.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(&descriptor.headers).unwrap(),
            json!({"accept": "application/vnd.amazon.eventstream",
                   "amz-sdk-request": "attempt=1; max=1",
                   "x-amz-target": "AmazonCodeWhispererStreamingService.GenerateAssistantResponse",
                   "x-amzn-kiro-agent-mode": "vibe"}),
            "{}",
            case.name
        );
        assert_eq!(descriptor.url, "https://q.us-east-1.amazonaws.com/generateAssistantResponse");
        assert_eq!(
            serde_json::to_value(&descriptor.auth).unwrap(),
            json!({"scheme": "bearer", "secret": "provider_api_key"})
        );
        built += 1;
    }
    assert!(built >= 10);
}

/// The native arm's request-side reading of tool-call arguments (server `reference_anthropic.rs`
/// `tool_use_block`, then `translate_kiro.rs` `content_tool_uses`): any JSON value is sent as it
/// parses and text that does not parse as a JSON string; only empty or whitespace-only arguments
/// differ, becoming `{}` (KD14). Nothing about the arguments is refused.
#[test]
fn tool_arguments_follow_the_native_reading() {
    let (mut request, config) = chat_row();
    let input_for = |request: &ChatRequest| {
        let body = KiroReferenceV1.build_http_request(request, &config).unwrap().body.unwrap();
        body["conversationState"]["history"][1]["assistantResponseMessage"]["toolUses"][0]["input"]
            .clone()
    };
    for (arguments, input) in [
        ("{\"city\":\"Paris\"}", json!({"city": "Paris"})),
        ("", json!({})),
        (" \t\n", json!({})),
        ("Paris", json!("Paris")),
        ("{\"city\":", json!("{\"city\":")),
        ("[1,2]", json!([1, 2])),
        ("42", json!(42)),
        ("null", Value::Null),
        ("\"x\"", json!("x")),
    ] {
        request.messages = serde_json::from_value(json!([
            {"role": "user", "content": "Weather?"},
            {"role": "assistant", "tool_calls": [
                {"id": "call_a", "name": "get_weather", "arguments": arguments}]},
            {"role": "tool", "tool_call_id": "call_a", "content": "rain"},
        ]))
        .unwrap();
        assert_eq!(input_for(&request), input, "{arguments:?}");
    }
}
