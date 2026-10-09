//! Gates ① and ② for the official Bedrock `InvokeModel` Anthropic component
//! (`provider-anthropic-bedrock-invoke`), run against its native reference implementation over its
//! own frozen fixture pack, and the facts the design record decided that a passing pack alone
//! would not show.
//!
//! Design record: `docs/design/2026-10-08-bedrock-invoke-anthropic-component.md`.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::reference_anthropic::AnthropicReferenceV1;
use south_component_conformance::reference_anthropic_bedrock_invoke::{
    AnthropicBedrockInvokeReferenceV1, FAMILY,
};
use south_component_conformance::reference_bedrock_converse::BedrockConverseReferenceV1;
use south_component_conformance::{
    FixturePackV1, ProviderComponentV1, accepts_manifest, reported_identity_matches,
    run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::{ComponentManifestV1, PROVIDER_WORLD, compatibility_admits};
use token_station_protocol::{ChatRequest, HttpResponseParts, ProviderConfig, StreamEvent};

#[path = "support/host_range.rs"]
mod host_range;

const PACK: &str = "fixtures-anthropic-bedrock-invoke";

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

fn read_manifest(package: &str) -> ComponentManifestV1 {
    let source =
        std::fs::read_to_string(repo_root().join("components").join(package).join("manifest.json"))
            .expect("the shipped component manifest reads");
    serde_json::from_str(&source).expect("the shipped component manifest parses")
}

fn shipped_manifest() -> ComponentManifestV1 {
    read_manifest("provider-anthropic-bedrock-invoke")
}

fn shipped_pack() -> FixturePackV1 {
    FixturePackV1::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join(PACK))
        .expect("the shipped fixture pack loads")
}

fn read_json(directory: &str, name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(directory).join(name);
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
    assert!(reported_identity_matches(&AnthropicBedrockInvokeReferenceV1.metadata(), &manifest));
    assert_eq!(manifest.api_version, PROVIDER_WORLD);
    assert_eq!(manifest.providers, [FAMILY]);
}

/// Gate ②: the reference implementation answers its own frozen pack exactly.
#[test]
fn gate_two_passes_over_the_shipped_pack() {
    let report = run_provider_component_suite_v1_for_manifest(
        &AnthropicBedrockInvokeReferenceV1,
        &shipped_pack(),
        &shipped_manifest(),
    );
    let failures: Vec<String> = report.failures().map(|outcome| format!("{outcome:?}")).collect();
    assert!(failures.is_empty(), "{} gate ② failures:\n{}", failures.len(), failures.join("\n"));
}

/// Decision D6: signing is declared exactly as Converse declares it. The two manifests differ only
/// in identity, family, the fixture directory and the request facts (the Messages body caps
/// output at `/max_tokens`, and the model template names the `invoke` operation).
#[test]
fn the_manifest_equals_converse_but_for_identity_family_fixtures_and_request_facts() {
    let invoke = shipped_manifest();
    let converse = read_manifest("provider-bedrock-converse");
    assert_eq!(invoke.capabilities, converse.capabilities);
    assert_eq!(invoke.auth_arms, converse.auth_arms);
    assert_eq!(invoke.auth_arms.iter().collect::<Vec<_>>(), ["host_signed"]);
    assert_eq!(invoke.permissions, converse.permissions);
    assert!(invoke.permissions.secrets.is_empty(), "a signed-for component holds no secret slot");
    assert_eq!(invoke.credentials, converse.credentials);
    assert_eq!(invoke.emits, converse.emits);
    assert_eq!(invoke.stream_framing, converse.stream_framing);
    assert_eq!(invoke.signing, converse.signing);
    assert_eq!(invoke.usage_evidence, converse.usage_evidence);
    assert!(invoke.usage_evidence.is_reported());
    assert_eq!(invoke.compatibility, converse.compatibility);
    assert_eq!(invoke.endpoint.get(FAMILY), converse.endpoint.get("bedrock"));
    assert_eq!(invoke.config_schema.get(FAMILY), converse.config_schema.get("bedrock"));
    assert_eq!(invoke.conformance.required_suite, converse.conformance.required_suite);
    assert_eq!(invoke.conformance.fixtures, format!("{PACK}/"));
    assert_eq!(
        serde_json::to_value(&invoke.request_facts).unwrap(),
        json!({FAMILY: {"output_cap": ["/max_tokens"], "model": {"url": "/model/{model}/invoke"},
                        "stream": "url"}})
    );
}

/// The rows the design record lists (§9.2, as ruled under I-Q6), plus the I-Q7 and I-Q12 rows. A
/// pack can pass while having quietly lost a case.
#[test]
fn the_shipped_pack_carries_every_decided_row() {
    let names: Vec<String> = shipped_pack().cases().iter().map(|case| case.name.clone()).collect();
    for row in [
        "provider.request.chat",
        "provider.request.stream-uses-the-stream-operation",
        "provider.request.default-max-tokens-when-the-caller-sets-none",
        "provider.request.model-id-with-a-slash-stays-one-segment",
        "provider.request.dialect-sampling-none-drops-temperature-and-top-p",
        "provider.request.dialect-sampling-exclusive-keeps-temperature-over-top-p",
        "provider.request.dialect-adaptive-thinking-carries-the-effort",
        "provider.request.dialect-budget-thinking-stays-below-max-tokens",
        "provider.request.reasoning-replay-multiple-blocks",
        "provider.request.tool-choice-none-withholds-the-declarations",
        "provider.request.parallel-tool-results-are-consecutive-user-turns",
        "provider.response.text",
        "provider.response.tool-use",
        "provider.response.usage",
        "provider.response.cached-usage",
        "provider.response.missing-usage",
        "provider.response.reasoning-replay-multiple-blocks",
        "provider.response.unknown-stop-reason-survives",
        "provider.stream.text",
        "provider.stream.usage-terminal",
        "provider.stream.terminal-delta-carries-the-real-input",
        "provider.stream.padding-member-is-ignored",
        "provider.stream.chunk-without-bytes-is-refused",
        "provider.stream.chunk-with-invalid-base64-is-refused",
        "provider.stream.chunk-that-decodes-to-non-json-is-refused",
        "provider.stream.exception-ends-the-stream",
        "provider.stream.error-frame-ends-the-stream",
        "provider.stream.no-usage",
        "provider.stream.reasoning-replay-multiple-blocks",
        "provider.stream.a-tool-call-names-itself-once",
        // I-Q7: the stream fold refuses a cumulative count that shrinks, as the host does.
        "provider.stream.a-shrinking-cumulative-count-is-refused",
        // I-Q12: an in-band Anthropic `error` event ends the stream with the upstream's error.
        "provider.stream.in-band-error-ends-the-stream",
        "provider.error.throttling-carries-retry-after",
        "provider.error.rejected-credential",
        "provider.error.validation-is-invalid-request",
        "provider.capabilities.declared",
    ] {
        assert!(names.iter().any(|name| name == row), "the pack lost `{row}`");
    }
    // The cache-write tiers are in both usage rows that carry a cache write.
    for row in ["provider.response.cached-usage", "provider.stream.usage-terminal"] {
        let expected =
            read_json(PACK, &format!("{row}.expected.json")).to_string().replace(' ', "");
        assert!(expected.contains("\"cache_write_1h_tokens\":200"), "{row}");
    }
}

/// The I-Q7 and I-Q12 rows are in the Messages pack too: the shared state machine changed for
/// `provider-anthropic` as well.
#[test]
fn the_messages_pack_carries_the_shared_behavior_rows() {
    let pack =
        FixturePackV1::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures-anthropic"))
            .unwrap();
    for row in [
        "provider.stream.a-shrinking-cumulative-count-is-refused",
        "provider.stream.in-band-error-ends-the-stream",
    ] {
        assert!(
            pack.cases().iter().any(|case| case.name == row),
            "fixtures-anthropic lost `{row}`"
        );
    }
}

/// Decision D2: on the rows derived from the Messages pack, the `InvokeModel` request is the
/// Messages request with exactly the native arm's three body edits, the `InvokeModel` URL and
/// headers, and no auth. Recomputed here from `fixtures-anthropic/`, not from the reference, so a
/// change to either pack that forgot the other fails.
#[test]
fn the_derived_request_rows_are_the_messages_rows_with_the_three_edits() {
    const ARN: &str = "arn:aws:bedrock:us-east-1:123456789012:inference-profile/us.anthropic.claude-sonnet-4-20250514-v1:0";
    let rows: [(&str, &str, Option<bool>, Option<&str>); 9] = [
        ("chat", "chat", Some(false), None),
        ("chat", "stream-uses-the-stream-operation", Some(true), None),
        ("max-tokens-defaults", "default-max-tokens-when-the-caller-sets-none", None, None),
        ("chat", "model-id-with-a-slash-stays-one-segment", Some(false), Some(ARN)),
        ("dialect-sampling-none-drops-temperature-and-top-p", "", None, None),
        ("dialect-sampling-exclusive-keeps-temperature-over-top-p", "", None, None),
        ("dialect-adaptive-thinking-carries-the-effort", "", None, None),
        ("dialect-budget-thinking-stays-below-max-tokens", "", None, None),
        ("tool-choice-none-withholds-the-declarations", "", None, None),
    ];
    let bedrock_model = |model: &str| format!("anthropic.{model}-v1:0");
    for (source, target, stream, model) in rows {
        let target = if target.is_empty() { source } else { target };
        let messages_input =
            read_json("fixtures-anthropic", &format!("provider.request.{source}.input.json"));
        let messages_expected =
            read_json("fixtures-anthropic", &format!("provider.request.{source}.expected.json"));
        let invoke_input = read_json(PACK, &format!("provider.request.{target}.input.json"));
        let invoke_expected = read_json(PACK, &format!("provider.request.{target}.expected.json"));

        let mut input = messages_input.clone();
        let config = &mut input["provider_config"];
        config["provider"] = json!(FAMILY);
        config["base_url"] = json!("https://bedrock-runtime.us-east-1.amazonaws.com");
        config.as_object_mut().unwrap().remove("auth");
        for entry in config["models"].as_array_mut().unwrap() {
            entry["model"] = json!(
                model
                    .map_or_else(|| bedrock_model(entry["model"].as_str().unwrap()), str::to_owned)
            );
        }
        let request = &mut input["chat_request"];
        request["model"] = json!(
            model.map_or_else(|| bedrock_model(request["model"].as_str().unwrap()), str::to_owned)
        );
        match stream {
            Some(true) => request["stream"] = json!(true),
            Some(false) => {
                request.as_object_mut().unwrap().remove("stream");
            }
            None => {}
        }
        assert_eq!(invoke_input, input, "{target}: the input is the Messages row's");

        let streaming = input["chat_request"]["stream"] == json!(true);
        let mut body = messages_expected["body"].clone();
        let fields = body.as_object_mut().unwrap();
        fields.remove("model");
        fields.remove("stream");
        fields.insert("anthropic_version".to_owned(), json!("bedrock-2023-05-31"));
        let segment = input["chat_request"]["model"].as_str().unwrap().replace('/', "%2F");
        let operation = if streaming { "invoke-with-response-stream" } else { "invoke" };
        let accept =
            if streaming { "application/vnd.amazon.eventstream" } else { "application/json" };
        assert_eq!(
            invoke_expected,
            json!({
                "method": "POST",
                "url": format!("https://bedrock-runtime.us-east-1.amazonaws.com/model/{segment}/{operation}"),
                "headers": {"accept": accept, "content-type": "application/json",
                            "x-amzn-bedrock-accept": "application/json"},
                "body": body,
            }),
            "{target}: the expectation is the Messages row's with the three edits"
        );
    }
}

fn chat_row() -> (ChatRequest, ProviderConfig) {
    let input = read_json(PACK, "provider.request.chat.input.json");
    (
        serde_json::from_value(input["chat_request"].clone()).unwrap(),
        serde_json::from_value(input["provider_config"].clone()).unwrap(),
    )
}

/// The package serves only its own family, refuses a request without a model before building
/// anything (the model is the URL), and never puts `stream` in the body.
#[test]
fn the_request_is_refused_outside_the_family_or_without_a_model() {
    let (mut request, mut config) = chat_row();
    for stream in [false, true] {
        request.stream = stream;
        let built =
            AnthropicBedrockInvokeReferenceV1.build_http_request(&request, &config).unwrap();
        assert!(built.auth.is_none(), "the host_signed arm's descriptor carries no auth");
        let body = built.body.unwrap();
        assert!(body.get("stream").is_none() && body.get("model").is_none(), "{body}");
        assert_eq!(body["anthropic_version"], "bedrock-2023-05-31");
    }
    let mut empty = request.clone();
    empty.model.clear();
    let refusal =
        AnthropicBedrockInvokeReferenceV1.build_http_request(&empty, &config).unwrap_err();
    assert_eq!(refusal.code.as_str(), "capability");
    for family in ["anthropic", "bedrock"] {
        family.clone_into(&mut config.provider);
        assert!(AnthropicBedrockInvokeReferenceV1.build_http_request(&request, &config).is_err());
    }
}

fn parts(status: u16, headers: &[(&str, &str)], body: &Value) -> HttpResponseParts {
    let headers: serde_json::Map<String, Value> =
        headers.iter().map(|(name, value)| ((*name).to_owned(), json!(value))).collect();
    serde_json::from_value(json!({"status": status, "headers": headers, "body": body.to_string()}))
        .unwrap()
}

/// §8: the Bedrock exception name first, then an Anthropic error `type`, then the status; the
/// provider message from either body shape.
#[test]
fn errors_read_the_bedrock_name_then_the_anthropic_type_then_the_status() {
    let invoke = AnthropicBedrockInvokeReferenceV1;
    // The exception name wins over a contradicting Anthropic type.
    let both = parts(
        429,
        &[("x-amzn-errortype", "ThrottlingException")],
        &json!({"type": "error", "error": {"type": "overloaded_error", "message": "busy"}}),
    );
    assert_eq!(invoke.map_provider_error(&both).unwrap().code.as_str(), "rate_limit");
    // No exception name: the Anthropic shape decides, and its message is the provider message.
    let anthropic = parts(
        529,
        &[],
        &json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}),
    );
    let envelope = invoke.map_provider_error(&anthropic).unwrap();
    assert_eq!(envelope.code.as_str(), "capacity");
    assert_eq!(envelope.provider_message.as_deref(), Some("Overloaded"));
    // Neither: the status.
    let bare = parts(503, &[], &json!({}));
    assert_eq!(invoke.map_provider_error(&bare).unwrap().code.as_str(), "upstream_unavailable");
    // The Converse table and normalization are shared: a qualified `__type` reads the same.
    let qualified = parts(
        400,
        &[],
        &json!({"__type": "com.amazon.bedrock#ValidationException", "message": "bad"}),
    );
    assert_eq!(
        invoke.map_provider_error(&qualified).unwrap().code,
        BedrockConverseReferenceV1.map_provider_error(&qualified).unwrap().code
    );
}

fn chunk(event: &Value) -> Vec<u8> {
    // Encoded here, so the test does not share the decoder it judges.
    format!("event: chunk\ndata: {}\n\n", json!({"bytes": encode(event.to_string().as_bytes())}))
        .into_bytes()
}

fn events(chunks: &[Vec<u8>]) -> Vec<StreamEvent> {
    let mut parser = AnthropicBedrockInvokeReferenceV1.stream_parser();
    let mut events = Vec::new();
    for piece in chunks {
        events.extend(parser.parse_chunk(piece).unwrap());
    }
    events.extend(parser.finish().unwrap());
    events
}

/// I-Q12: unknown top-level eventstream events are ignored (the Converse precedent); a decoded
/// event the Messages machine does not model (`ping`) is ignored; and nothing — no frame, no
/// pending `Finish` / `Done` at EOF — follows a failure.
#[test]
fn unknown_events_are_ignored_and_nothing_follows_a_failure() {
    let start = chunk(
        &json!({"type": "message_start", "message": {"usage": {"input_tokens": 3, "output_tokens": 1}}}),
    );
    let ping = chunk(&json!({"type": "ping"}));
    let unknown = b"event: somethingNew\ndata: {\"x\":1}\n\n".to_vec();
    let text = chunk(
        &json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
    );
    let stop = chunk(&json!({"type": "content_block_stop", "index": 0}));
    // A `message_delta` without usage leaves `Finish` / `Done` for the EOF.
    let delta = chunk(&json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}}));
    let settled =
        events(&[start.clone(), ping, unknown, text.clone(), stop.clone(), delta.clone()]);
    assert!(matches!(settled.last(), Some(StreamEvent::Done { .. })), "{settled:?}");

    let exception =
        b"event: exception:modelStreamErrorException\ndata: {\"message\":\"cut\"}\n\n".to_vec();
    let garbage = b"event: chunk\ndata: {\"bytes\":\"%%%\"}\n\n".to_vec();
    let failed = events(&[start, text, stop, delta, exception, garbage]);
    assert!(
        matches!(failed.as_slice(), [.., StreamEvent::Error { error }] if error.code.as_str() == "transport_truncated"),
        "the failure is the last event, even with a finish pending and a bad frame after it: {failed:?}"
    );
}

/// The non-streaming answer is the Messages answer: parsed by the shared parser, unchanged.
#[test]
fn the_response_is_parsed_as_messages_parses_it() {
    for row in ["text", "tool-use", "usage", "cached-usage", "reasoning-replay-multiple-blocks"] {
        let input: HttpResponseParts =
            serde_json::from_value(read_json(PACK, &format!("provider.response.{row}.input.json")))
                .unwrap();
        assert_eq!(
            AnthropicBedrockInvokeReferenceV1.parse_response(&input),
            AnthropicReferenceV1.parse_response(&input),
            "{row}"
        );
    }
}

mod properties {
    use proptest::prelude::*;
    use south_component_conformance::ProviderComponentV1;
    use south_component_conformance::reference_anthropic_bedrock_invoke::AnthropicBedrockInvokeReferenceV1;

    fn run(body: &[u8], stride: usize) -> String {
        let mut parser = AnthropicBedrockInvokeReferenceV1.stream_parser();
        let mut events = Vec::new();
        for piece in body.chunks(stride) {
            match parser.parse_chunk(piece) {
                Ok(more) => events.extend(more),
                Err(error) => return format!("{events:?} {error:?}"),
            }
        }
        match parser.finish() {
            Ok(more) => events.extend(more),
            Err(error) => return format!("{events:?} {error:?}"),
        }
        format!("{events:?}")
    }

    proptest! {
        /// The upstream's bytes are untrusted: whatever arrives, raw or as a `chunk` payload,
        /// the parser answers without panicking, and how the socket split it changes neither
        /// the events nor the first error.
        #[test]
        fn any_chunking_of_any_body_gives_the_same_answer(
            payload in proptest::collection::vec(any::<u8>(), 0..96),
            stride in 1usize..9,
        ) {
            let wrapped = format!(
                "event: chunk\ndata: {}\n\n",
                serde_json::json!({"bytes": super::encode(&payload)})
            );
            for body in [payload.as_slice(), wrapped.as_bytes()] {
                prop_assert_eq!(run(body, stride), run(body, body.len().max(1)));
            }
        }
    }
}

/// Standard base64 with padding, for the property test.
fn encode(bytes: &[u8]) -> String {
    const SYMBOLS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for group in bytes.chunks(3) {
        let value = group
            .iter()
            .enumerate()
            .fold(0usize, |value, (at, byte)| value | (usize::from(*byte) << (16 - 8 * at)));
        for at in 0..4 {
            encoded.push(if at <= group.len() {
                char::from(SYMBOLS[(value >> (18 - 6 * at)) & 0x3f])
            } else {
                '='
            });
        }
    }
    encoded
}
