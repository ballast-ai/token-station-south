//! Gates ① and ② for the official `OpenAI` Responses component (`provider-openai-responses`), run
//! against its native reference implementation over its own frozen fixture pack, and the facts
//! the design record decided that a passing pack alone would not show.
//!
//! Design record: `docs/design/2026-09-30-openai-responses-upstream-component.md` (step R1); the
//! `openai-codex` family and the four client fields are step R3
//! (`docs/design/2026-10-12-responses-r3-codex-and-request-fields.md`), whose own facts are in
//! `openai_codex_conformance_v1`.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::reference_openai_responses::{
    CODEX_FAMILY, FAMILY, NAME, OpenAiResponsesReferenceV1, VERSION,
};
use south_component_conformance::responses_vocabulary::{extension, replay};
use south_component_conformance::{
    CheckV1, FixturePackV1, ProviderComponentV1, accepts_manifest, reported_identity_matches,
    run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::{
    ComponentManifestV1, NorthProtocolV1, PROVIDER_WORLD, StreamFramingV1, compatibility_admits,
};
use token_station_protocol::{ChatRequest, ErrorCode, HttpResponseParts, ProviderConfig};

#[path = "support/host_range.rs"]
mod host_range;
#[path = "support/openai_responses_packs.rs"]
mod packs;

const PACK: &str = packs::RESPONSES_PACK;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

fn shipped_manifest() -> ComponentManifestV1 {
    let source =
        std::fs::read_to_string(repo_root().join("components").join(NAME).join("manifest.json"))
            .expect("the shipped component manifest reads");
    serde_json::from_str(&source).expect("the shipped component manifest parses")
}

fn shipped_pack() -> FixturePackV1 {
    packs::load(PACK)
}

fn read_json(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(PACK).join(name);
    serde_json::from_str(&std::fs::read_to_string(&path).expect("the fixture reads"))
        .expect("the fixture parses")
}

fn config() -> ProviderConfig {
    serde_json::from_value(read_json("provider.capabilities.declared.input.json")).unwrap()
}

fn build(request: &Value) -> Result<Value, token_station_protocol::ErrorEnvelope> {
    let request: ChatRequest = serde_json::from_value(request.clone()).unwrap();
    OpenAiResponsesReferenceV1
        .build_http_request(&request, &config())
        .map(|descriptor| serde_json::to_value(descriptor).unwrap()["body"].clone())
}

/// Gate ①: the package is admissible under this release's range, and the identity the component
/// reports is the one its manifest claims.
#[test]
fn gate_one_admits_the_shipped_package_and_its_reported_identity() {
    let manifest = shipped_manifest();
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    assert!(compatibility_admits(&manifest, &host_range::host_range()).is_ok());
    assert!(reported_identity_matches(&OpenAiResponsesReferenceV1.metadata(), &manifest));
    assert_eq!(manifest.api_version, PROVIDER_WORLD);
    assert_eq!(manifest.version, VERSION);
    assert_eq!(manifest.providers, [FAMILY, CODEX_FAMILY]);
}

/// Gate ②: the reference implementation answers its own frozen pack — both directories — exactly.
#[test]
fn gate_two_passes_over_the_shipped_pack() {
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiResponsesReferenceV1,
        &packs::package_pack(),
        &shipped_manifest(),
    );
    let failures: Vec<String> = report.failures().map(|outcome| format!("{outcome:?}")).collect();
    assert!(failures.is_empty(), "{} gate ② failures:\n{}", failures.len(), failures.join("\n"));
    // The two checks this package adds to gate ② ran (record §12.1, §12.2).
    let ran = |check: CheckV1| report.outcomes().iter().any(|outcome| outcome.check == check);
    assert!(ran(CheckV1::ImmutablePathsHonoured));
    assert!(ran(CheckV1::UsageNeverDefaulted));
    assert!(ran(CheckV1::CredentialRecipeMatch));
}

/// Record §3.4, as reconciled for step R3 (R3 record §2.1): two families, with exactly the
/// declarations the sketch gives each; the credential section is judged in
/// `openai_codex_conformance_v1`.
#[test]
fn the_manifest_declares_what_the_record_gives_the_families() {
    let manifest = shipped_manifest();
    assert_eq!(
        manifest.capabilities.iter().map(String::as_str).collect::<Vec<_>>(),
        ["chat", "json_schema", "stream", "tool_call"]
    );
    assert_eq!(manifest.auth_arms.iter().map(String::as_str).collect::<Vec<_>>(), ["bearer"]);
    assert_eq!(manifest.permissions.secrets, ["provider_api_key"]);
    assert!(!manifest.permissions.network && !manifest.permissions.filesystem);
    assert!(manifest.usage_evidence.is_reported());
    assert_eq!(manifest.stream_framing, StreamFramingV1::Bytes);
    let credentials = manifest.credentials.as_ref().expect("the minted Codex slot (R3)");
    assert_eq!(credentials.families.as_deref(), Some(&[CODEX_FAMILY.to_owned()][..]));
    assert!(manifest.signing.is_none() && manifest.emits.is_empty());
    assert!(manifest.endpoint.is_empty() && manifest.config_schema.is_empty());
    assert!(manifest.user_agent.is_empty() && manifest.host_values.is_empty());
    assert_eq!(
        serde_json::to_value(&manifest.request_facts).unwrap(),
        json!({FAMILY: {"output_cap": ["/max_output_tokens"], "model": {"body": "/model"},
                        "stream": {"body": "/stream"}},
               CODEX_FAMILY: {"output_cap": [], "model": {"body": "/model"},
                              "stream": {"body": "/stream"}}})
    );
    assert_eq!(
        serde_json::to_value(&manifest.immutable_body_paths).unwrap(),
        json!({FAMILY: ["store"],
               CODEX_FAMILY: ["store", "stream", "instructions", "temperature", "max_output_tokens"]})
    );
    assert_eq!(manifest.north_passthrough.get(FAMILY), Some(&NorthProtocolV1::Responses));
    assert_eq!(manifest.north_passthrough.get(CODEX_FAMILY), Some(&NorthProtocolV1::Responses));
    assert_eq!(manifest.north_passthrough.len(), 2);
    assert_eq!(manifest.conformance.fixtures, format!("{PACK}/"));
}

/// The rows the record lists (§12.1; the Codex and credential rows of step R3 are pinned in
/// `openai_codex_conformance_v1`), plus this implementation's own and the R3 §4 rows. A pack can
/// pass while having quietly lost a case.
#[test]
fn the_shipped_pack_carries_every_decided_row() {
    let names: Vec<String> = shipped_pack().cases().iter().map(|case| case.name.clone()).collect();
    for row in [
        "request.text",
        "request.instructions-and-history",
        "request.tool-result-turn",
        "request.image-input",
        "request.tools-and-tool-choice",
        "request.tool-choice-anthropic-form",
        "request.json-object-output",
        "request.structured-output",
        "request.reasoning-effort",
        "request.reasoning-effort-withheld",
        "request.stop-is-dropped",
        "request.thinking-is-not-replayed",
        "request.stateful-fields-never-sent",
        "request.stream",
        "request.stream-with-cap",
        "request.refused-claude-carrier",
        "request.refused-unmappable-part",
        "request.refused-unknown-tool-choice",
        "request.refused-input-file",
        "request.refused-file-id",
        "request.refused-tool-result-without-call-id",
        "request.request-fields",
        "request.refused-request-field-out-of-bounds",
        "request.refused-unknown-truncation",
        "response.usage",
        "response.cached-usage",
        "response.cache-write-tokens-are-a-subset",
        "response.reasoning-usage",
        "response.missing-usage",
        "response.total-mismatch",
        "response.subset-violation",
        "response.reasoning-exceeds-output",
        "response.nonzero-tool-usage",
        "response.zero-tool-usage-is-accepted",
        "response.tool-call",
        "response.reasoning-summary",
        "response.refusal",
        "response.incomplete-max-output",
        "response.incomplete-content-filter",
        "response.incomplete-unknown-reason",
        "response.failed-status",
        "response.unmapped-output-item",
        "stream.usage-terminal",
        "stream.no-usage",
        "stream.text",
        "stream.tool-call",
        "stream.tool-call-prebuffered-arguments",
        "stream.arguments-contradict-the-stream",
        "stream.reasoning-summary",
        "stream.incomplete",
        "stream.incomplete-content-filter",
        "stream.incomplete-unknown-reason",
        "stream.failed",
        "stream.failed-with-usage",
        "stream.error-event",
        "stream.error-event-without-sequence",
        "stream.typeless-error-object",
        "stream.error-code-rate-limit",
        "stream.duplicate-terminal",
        "stream.event-after-terminal",
        "stream.sequence-regression",
        "stream.response-id-change",
        "stream.item-identity-mismatch",
        "stream.unknown-item-type",
        "stream.unknown-event-type",
        "stream.event-name-disagrees-with-type",
        "stream.missing-terminal",
        "stream.crlf-comments-and-split-frames",
        "error.rejected-credential",
        "error.rate-limit",
        "error.context-length",
        "capabilities.declared",
    ] {
        let row = format!("provider.{row}");
        assert!(names.contains(&row), "the pack lost `{row}`");
    }
}

/// D4 / R-Q4: every request is stateless. Whatever the IR carries — here every stateful field a
/// client could have smuggled into `extensions` — the body says `store: false`, never names a
/// previous response, and holds no field outside the record's §4.1 mapping. A smuggled `include`
/// never reaches the wire: `include` is sent only for a model declaring `reasoning_replay.openai.v1`
/// (NC-1, NP-Q3), and then with exactly that one value.
#[test]
fn every_request_is_stateless_whatever_the_ir_carries() {
    const MAPPED: [&str; 18] = [
        "include",
        "model",
        "instructions",
        "input",
        "tools",
        "parallel_tool_calls",
        "tool_choice",
        "text",
        "reasoning",
        "temperature",
        "top_p",
        "max_output_tokens",
        "stream",
        "store",
        // R3 §4.3: the four client fields, from their own extension keys only.
        "user",
        "metadata",
        "prompt_cache_key",
        "truncation",
    ];
    let mut built = 0;
    for case in shipped_pack().cases() {
        if !case.name.starts_with("provider.request.") || case.expected.get("error").is_some() {
            continue;
        }
        let mut request = case.input["chat_request"].clone();
        for (key, value) in [
            ("store", json!(true)),
            ("previous_response_id", json!("resp_68af0a1c7f8c8190b7d5c3b9a0e1f2d3")),
            ("conversation", json!("conv_1")),
            ("include", json!(["reasoning.encrypted_content"])),
            ("background", json!(true)),
        ] {
            request[key] = value;
        }
        let config: ProviderConfig =
            serde_json::from_value(case.input["provider_config"].clone()).unwrap();
        let declares = config
            .models
            .iter()
            .any(|model| model.supported_parameters.contains("reasoning_replay.openai.v1"));
        let request: ChatRequest = serde_json::from_value(request).unwrap();
        let descriptor = OpenAiResponsesReferenceV1
            .build_http_request(&request, &config)
            .unwrap_or_else(|error| panic!("{}: {error:?}", case.name));
        let body = serde_json::to_value(descriptor).unwrap()["body"].clone();
        assert_eq!(body["store"], json!(false), "{}", case.name);
        let include = if declares { json!(["reasoning.encrypted_content"]) } else { Value::Null };
        assert_eq!(body["include"], include, "{}", case.name);
        for key in body.as_object().unwrap().keys() {
            assert!(MAPPED.contains(&key.as_str()), "{}: `{key}` is not a mapped field", case.name);
        }
        built += 1;
    }
    assert!(built >= 10, "too few request rows to judge");
}

/// R-Q15: the component acts on the three `extensions` keys the OpenAI-compatible reference reads
/// and, since R3, on the four client fields (R3 §4.3). `responses_reasoning_summary`, the north
/// codec's transient-instructions marker and any other key change nothing.
#[test]
fn only_the_named_extension_keys_change_the_body() {
    let input = read_json("provider.request.reasoning-effort.input.json");
    let request = input["chat_request"].clone();
    assert_eq!(request[extension::REASONING_SUMMARY], json!("auto"), "the row carries the key");
    let baseline = build(&request).unwrap();
    let mut without = request.clone();
    without.as_object_mut().unwrap().remove(extension::REASONING_SUMMARY);
    assert_eq!(build(&without).unwrap(), baseline);
    let mut noisy = request.clone();
    noisy[extension::REASONING_SUMMARY] = json!("detailed");
    noisy[extension::TRANSIENT_INSTRUCTIONS] = json!(true);
    noisy["responses_reasoning_id"] = json!("rs_1");
    noisy["responses_reasoning_encrypted_content"] = json!("gAAAAAB");
    assert_eq!(build(&noisy).unwrap(), baseline);

    // Each of the three precedent keys does change the body.
    let mut effort = request;
    effort[extension::REASONING_EFFORT] = json!("low");
    assert_ne!(build(&effort).unwrap(), baseline);
    let tools =
        read_json("provider.request.tools-and-tool-choice.input.json")["chat_request"].clone();
    let tool_baseline = build(&tools).unwrap();
    for key in [extension::TOOL_STRICT, extension::PARALLEL_TOOL_CALLS] {
        let mut changed = tools.clone();
        changed.as_object_mut().unwrap().remove(key);
        assert_ne!(build(&changed).unwrap(), tool_baseline, "{key}");
    }

    // R3 §4.3: each of the four client fields reaches the body under its own name, unchanged,
    // and removing it removes exactly that member.
    let fields = read_json("provider.request.request-fields.input.json")["chat_request"].clone();
    let fields_body = build(&fields).unwrap();
    for key in extension::CLIENT_FIELDS {
        assert_eq!(fields_body[key], fields[key], "{key}");
        let mut changed = fields.clone();
        changed.as_object_mut().unwrap().remove(key);
        let mut expected = fields_body.clone();
        expected.as_object_mut().unwrap().remove(key);
        assert_eq!(build(&changed).unwrap(), expected, "{key}");
        // `null` is the same as absent.
        changed[key] = Value::Null;
        assert_eq!(build(&changed).unwrap(), expected, "{key}");
    }
}

/// R3 §4.3: the component holds the four fields to the north codec's bounds, so a Chat request —
/// whose codec copies `user`, `metadata` and `prompt_cache_key` verbatim — cannot widen the body
/// either. The bounds are inclusive.
#[test]
fn the_client_fields_are_held_to_the_codec_bounds() {
    let fields = read_json("provider.request.request-fields.input.json")["chat_request"].clone();
    let longest = "u".repeat(extension::MAX_FIELD_BYTES);
    let full: serde_json::Map<String, Value> = (0..extension::MAX_METADATA_ENTRIES)
        .map(|index| {
            (
                format!("{index:0>width$}", width = extension::MAX_METADATA_KEY_CHARS),
                json!("\u{4e00}".repeat(extension::MAX_METADATA_VALUE_CHARS)),
            )
        })
        .collect();
    for (key, value) in [
        (extension::USER, json!(longest)),
        (extension::PROMPT_CACHE_KEY, json!(longest)),
        (extension::METADATA, Value::Object(full)),
        (extension::METADATA, json!({})),
        (extension::TRUNCATION, json!("disabled")),
    ] {
        let mut request = fields.clone();
        request[key] = value.clone();
        assert_eq!(build(&request).unwrap()[key], value, "{key}");
    }
    let too_long = "s".repeat(extension::MAX_FIELD_BYTES + 1);
    let many: serde_json::Map<String, Value> = (0..=extension::MAX_METADATA_ENTRIES)
        .map(|index| (format!("k{index}"), json!("v")))
        .collect();
    for (key, value) in [
        (extension::USER, json!(too_long)),
        (extension::USER, json!("\u{4e00}".repeat(171))),
        (extension::USER, json!(42)),
        (extension::PROMPT_CACHE_KEY, json!(too_long)),
        (extension::PROMPT_CACHE_KEY, json!({"k": "v"})),
        (extension::METADATA, json!("tenant")),
        (extension::METADATA, Value::Object(many)),
        (extension::METADATA, json!({"k".repeat(extension::MAX_METADATA_KEY_CHARS + 1): "v"})),
        (extension::METADATA, json!({"k": "v".repeat(extension::MAX_METADATA_VALUE_CHARS + 1)})),
        (extension::METADATA, json!({"k": 7})),
        (extension::TRUNCATION, json!("middle")),
        (extension::TRUNCATION, json!(true)),
    ] {
        let mut request = fields.clone();
        request[key] = value.clone();
        let refused = build(&request).unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidRequest, "{key}: {value}");
        assert_eq!(refused.http_status, 400, "{key}");
    }
}

/// The package serves its two families: a request for any other is a capability error with no
/// request built.
#[test]
fn only_the_two_families_are_served() {
    let mut request: ChatRequest = serde_json::from_value(
        read_json("provider.request.text.input.json")["chat_request"].clone(),
    )
    .unwrap();
    request.stream = true;
    for family in [FAMILY, CODEX_FAMILY] {
        let mut config = config();
        family.clone_into(&mut config.provider);
        assert!(OpenAiResponsesReferenceV1.build_http_request(&request, &config).is_ok());
    }
    for family in ["openai-compatible", "azure-openai-v1", "codex", ""] {
        let mut config = config();
        family.clone_into(&mut config.provider);
        let refused = OpenAiResponsesReferenceV1.build_http_request(&request, &config).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Capability, "{family}");
    }
}

/// R-Q2: an `incomplete` answer settles only for the two ruled reasons, with the ruled finish
/// reasons; every other reason, and a missing one, is a protocol error.
#[test]
fn incomplete_settles_only_for_the_two_ruled_reasons() {
    let body = |reason: Value| {
        json!({"id": "resp_1", "object": "response", "status": "incomplete",
               "incomplete_details": reason, "model": "m",
               "output": [{"id": "msg_1", "type": "message", "role": "assistant",
                           "content": [{"type": "output_text", "text": "Part"}]}],
               "usage": {"input_tokens": 5, "output_tokens": 2, "total_tokens": 7}})
    };
    let parse = |body: Value| {
        let parts: HttpResponseParts =
            serde_json::from_value(json!({"status": 200, "headers": {}, "body": body.to_string()}))
                .unwrap();
        OpenAiResponsesReferenceV1.parse_response(&parts)
    };
    let finish = |reason: &str| {
        serde_json::to_value(
            &parse(body(json!({"reason": reason}))).unwrap().choices[0].finish_reason,
        )
        .unwrap()
    };
    assert_eq!(finish("max_output_tokens"), json!("length"));
    assert_eq!(finish("content_filter"), json!("content_filter"));
    for reason in [json!({"reason": "turn_limit"}), json!({"reason": ""}), json!({}), Value::Null] {
        let refused = parse(body(reason.clone())).unwrap_err();
        assert_eq!(refused.code, ErrorCode::ProviderProtocolError, "{reason}");
    }
    let mut no_usage = body(json!({"reason": "max_output_tokens"}));
    no_usage.as_object_mut().unwrap().remove("usage");
    assert_eq!(parse(no_usage).unwrap_err().code, ErrorCode::ProviderProtocolError);
}

/// The component's lockfile links none of the host-only crates: it splits SSE with its own code
/// (record §6, as amended 2026-10-10).
#[test]
fn the_component_does_not_link_the_host_only_sse_decoder() {
    let lockfile =
        std::fs::read_to_string(repo_root().join("components").join(NAME).join("Cargo.lock"))
            .unwrap();
    assert!(!lockfile.contains("south-host-grammars"));
    assert!(lockfile.contains("name = \"south-component-conformance\""));
}

/// NC-1 (design record 2026-10-11 §2.4, §7 item 12): the replay markers change the body only for a
/// model declaring `reasoning_replay.openai.v1`; for any other model they are refused, never
/// silently dropped. The two retired single-valued keys still change nothing (the R-Q15 test
/// above).
#[test]
fn the_replay_keys_change_the_body_only_for_a_declaring_model() {
    let input = read_json("provider.request.reasoning-replay.input.json");
    let declaring: ProviderConfig =
        serde_json::from_value(input["provider_config"].clone()).unwrap();
    let request: ChatRequest = serde_json::from_value(input["chat_request"].clone()).unwrap();
    let mut stripped = request.clone();
    for message in &mut stripped.messages {
        for key in [replay::FAMILY_KEY, replay::LAYOUT_KEY, replay::ITEMS_KEY] {
            message.extensions.remove(key);
        }
    }
    let body = |request: &ChatRequest, config: &ProviderConfig| {
        OpenAiResponsesReferenceV1
            .build_http_request(request, config)
            .map(|descriptor| serde_json::to_value(descriptor).unwrap()["body"].clone())
    };
    let with = body(&request, &declaring).unwrap();
    let without = body(&stripped, &declaring).unwrap();
    assert_ne!(with["input"], without["input"]);
    assert!(with["input"].as_array().unwrap().iter().any(|item| item["type"] == "reasoning"));
    assert!(!without["input"].as_array().unwrap().iter().any(|item| item["type"] == "reasoning"));
    // `include` follows the declaration, not the markers (NP-Q3).
    assert_eq!(with["include"], without["include"]);

    let undeclared = config();
    let error = body(&request, &undeclared).unwrap_err();
    assert_eq!(error.code, ErrorCode::Capability);
    let plain = body(&stripped, &undeclared).unwrap();
    assert!(plain.get("include").is_none());
}
