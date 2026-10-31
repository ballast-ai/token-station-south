//! Direct refusal tests over the Anthropic Messages reference implementation
//! (`provider-anthropic`: `reference_anthropic.rs` and the shared Claude dialect in
//! `anthropic_dialect.rs`).
//!
//! # Why a reference-level leg exists at all
//!
//! Gate ② fixtures can expect an error envelope (`expected_error` in `suite.rs`), but every such
//! fixture is a shipped-suite case, so they are added sparingly and `fixtures-anthropic` has a
//! handful. A refusal nobody tests can silently become a pass-through, and on the host side a
//! pass-through is a dispatched, billed call. So this leg enumerates every refusal reachable
//! through the `ProviderComponentV1` surface (`build_http_request`, `parse_response`,
//! `map_provider_error`, `stream_parser().parse_chunk`) and pairs each with a neighbouring input
//! that differs only in the refused aspect and is accepted, so no test can pass merely because
//! the whole path is broken. Four refusals here are also expected by a fixture, and say so:
//! the foreign replay family, the malformed end-user hash, the missing usage object and the
//! shrinking cumulative input count.
//!
//! Each covered refusal (41, one per test) was mutation-checked: removed or inverted in the
//! source, the matching test went red, and the source was restored. Three need a note. A foreign
//! replay layout on an assistant turn is refused at two call sites, so its mutation removed both;
//! a block opened while open is refused by both halves of one condition, so its mutation removed
//! the whole condition; and a 2xx body that is not JSON, or a usage that is not an object, would
//! also fail a later check, so those two tests tell their refusal apart by its message (the
//! fixtures pin messages verbatim, so they are contract).
//!
//! # Not covered, and why
//!
//! - The `validated_layout` call inside `conversation_of`: `build_http_request` validates every
//!   message first, so the inner call can never be the one that refuses.
//! - `assistant_part_to_block(.., true).ok_or_else(..)` ("unsupported reasoning replay block"):
//!   it fires only for an unsigned thinking part, which `validated_layout` already refuses.
//! - `SafeHeaders::try_new(..).map_err(..)` and `Auth::header("x-api-key", ..).map_err(..)`: both
//!   take constant names that are valid by construction.
//! - `refuse_declared_forced_tool` in `anthropic_dialect.rs`: this component does not call it.
//! - `map_provider_error`: it has no refusal; any body maps to an envelope.
//! - The stream's in-band `error` event: it yields a `StreamEvent::Error`, not an `Err`, and the
//!   fixture `provider.stream.in-band-error-ends-the-stream` pins it.
//! - Unknown content parts, `http` image URLs and tool arguments that are not JSON: this
//!   component forwards them and lets the upstream rule, so there is no refusal to test.
//! - The individual rules of the replay layout itself (bijection, ordinals, signatures): they
//!   live in `reasoning_replay.rs` and are covered by `reasoning_replay_reference_v1.rs`.

use serde_json::{Value, json};
use south_component_conformance::{
    ProviderComponentV1, StreamParserV1, anthropic_dialect,
    reference_anthropic::AnthropicReferenceV1,
};
use token_station_protocol::{
    ChatRequest, ErrorCode, ErrorEnvelope, HttpResponseParts, ProviderConfig, StreamEvent,
};

const MODEL: &str = "claude-x";
const BASE: &str = "https://api.anthropic.com";
const REPLAY: &str = "reasoning_replay.claude.v1";

fn config_for(provider: &str, words: &[&str]) -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": provider,
        "base_url": BASE,
        "models": [{"model": MODEL, "supported_parameters": words}],
    }))
    .unwrap()
}

fn config(words: &[&str]) -> ProviderConfig {
    config_for("anthropic", words)
}

fn request(value: &Value) -> ChatRequest {
    serde_json::from_value(value.clone()).unwrap()
}

/// A request for `MODEL` with `messages` and any extra top-level fields.
fn turn(messages: &Value, extra: &Value) -> ChatRequest {
    let mut value = json!({"model": MODEL, "messages": messages});
    for (key, field) in extra.as_object().unwrap() {
        value[key] = field.clone();
    }
    request(&value)
}

fn hi() -> Value {
    json!([{"role": "user", "content": "hi"}])
}

fn build(request: &ChatRequest, config: &ProviderConfig) -> Result<Value, ErrorEnvelope> {
    AnthropicReferenceV1.build_http_request(request, config).map(|built| built.body.unwrap())
}

#[track_caller]
fn accepted(request: &ChatRequest, config: &ProviderConfig) -> Value {
    build(request, config).expect("must be accepted")
}

/// A request refused before anything is built: a capability the request or the declared model
/// lacks, which the host presents as 400.
#[track_caller]
fn refused(request: &ChatRequest, config: &ProviderConfig) -> ErrorEnvelope {
    let error = build(request, config).expect_err("must be refused");
    assert_eq!(error.code, ErrorCode::Capability, "{error:?}");
    assert_eq!(error.http_status, 400, "{error:?}");
    error
}

#[track_caller]
fn protocol_error(error: &ErrorEnvelope) {
    assert_eq!(error.code, ErrorCode::ProviderProtocolError, "{error:?}");
    assert_eq!(error.http_status, 502, "{error:?}");
}

/// A signed-thinking assistant turn with a tool call, laid out for replay.
fn replay_turn(call_name: &str) -> Value {
    json!({
        "role": "assistant",
        "content": [
            {"type": "thinking", "thinking": "first", "signature": "sig-1"},
            {"type": "text", "text": "answer"},
        ],
        "tool_calls": [{"id": "tu_1", "name": call_name, "arguments": "{}"}],
        "reasoning_replay_protocol_family": "claude-signed-thinking",
        "reasoning_replay_block_layout": [
            {"kind": "content", "ordinal": 0},
            {"kind": "content", "ordinal": 1},
            {"kind": "tool_call", "call_id": "tu_1"},
        ],
    })
}

fn with_tool(choice: &Value) -> ChatRequest {
    turn(
        &hi(),
        &json!({"tools": [{"name": "t", "parameters": {"type": "object"}}], "tool_choice": choice}),
    )
}

// ── Request: whose config, and which replay carriers ─────────────────────────

#[test]
fn another_dialects_config_is_refused() {
    refused(&turn(&hi(), &json!({})), &config_for("bedrock", &[]));
    for own in ["anthropic", "claude-code", "claude-code-setup-token"] {
        accepted(&turn(&hi(), &json!({})), &config_for(own, &[]));
    }
}

#[test]
fn a_replay_marker_on_a_user_message_is_refused() {
    // Only the up-front check sees this: the conversation builder reads the
    // layout of assistant turns alone, so a marker elsewhere would otherwise
    // pass unread.
    let mut marked = replay_turn("t");
    marked["role"] = json!("user");
    marked.as_object_mut().unwrap().remove("tool_calls");
    marked["reasoning_replay_block_layout"] =
        json!([{"kind": "content", "ordinal": 0}, {"kind": "content", "ordinal": 1}]);
    refused(&turn(&json!([marked]), &json!({})), &config(&[REPLAY]));

    let mut assistant = marked;
    assistant["role"] = json!("assistant");
    accepted(&turn(&json!([assistant]), &json!({})), &config(&[REPLAY]));
}

#[test]
fn an_assistant_replay_layout_from_another_family_is_refused() {
    // Also expected by the fixture `provider.request.refused-openai-reasoning-replay`.
    let mut foreign = replay_turn("t");
    foreign["reasoning_replay_protocol_family"] = json!("openai-encrypted-reasoning");
    let error = refused(&turn(&json!([foreign]), &json!({})), &config(&[REPLAY]));
    assert!(error.message.contains("replay"), "{error:?}");
    accepted(&turn(&json!([replay_turn("t")]), &json!({})), &config(&[REPLAY]));
}

#[test]
fn a_replay_turn_on_a_model_without_the_replay_capability_is_refused() {
    // Sending a signature to a model that cannot verify it gets a 400 that
    // names nothing; the host declares the capability per model.
    refused(&turn(&json!([replay_turn("t")]), &json!({})), &config(&[]));
    let body = accepted(&turn(&json!([replay_turn("t")]), &json!({})), &config(&[REPLAY]));
    assert_eq!(body["messages"][0]["content"][0]["signature"], "sig-1");
}

#[test]
fn a_replayed_tool_call_without_a_name_is_refused() {
    // The replay path builds tool_use blocks from the layout, not from the
    // plain tool_calls loop, so it carries its own copy of the check.
    refused(&turn(&json!([replay_turn("")]), &json!({})), &config(&[REPLAY]));
    accepted(&turn(&json!([replay_turn("t")]), &json!({})), &config(&[REPLAY]));
}

// ── Request: the declared Claude dialect ─────────────────────────────────────

#[test]
fn a_model_declaring_both_thinking_forms_is_refused() {
    let both = [anthropic_dialect::THINKING_ADAPTIVE, anthropic_dialect::THINKING_BUDGET];
    refused(&turn(&hi(), &json!({})), &config(&both));
    accepted(&turn(&hi(), &json!({})), &config(&[anthropic_dialect::THINKING_ADAPTIVE]));
}

#[test]
fn a_required_tool_choice_on_an_auto_only_model_is_refused() {
    // Downgrading to auto would let the model answer without the tool the
    // caller required.
    let auto_only = [anthropic_dialect::TOOL_CHOICE_AUTO_ONLY];
    refused(&with_tool(&json!("required")), &config(&auto_only));
    accepted(&with_tool(&json!("auto")), &config(&auto_only));
    accepted(&with_tool(&json!("required")), &config(&[]));
    // Nothing to force without tools.
    accepted(&turn(&hi(), &json!({"tool_choice": "required"})), &config(&auto_only));
}

#[test]
fn a_named_tool_choice_on_an_auto_only_model_is_refused() {
    let auto_only = [anthropic_dialect::TOOL_CHOICE_AUTO_ONLY];
    refused(
        &with_tool(&json!({"type": "function", "function": {"name": "t"}})),
        &config(&auto_only),
    );
    // A choice with no readable name is left out downstream, so it forces nothing.
    accepted(&with_tool(&json!({"type": "something_else"})), &config(&auto_only));
}

#[test]
fn an_effort_outside_the_vocabulary_is_refused_on_a_model_with_a_thinking_form() {
    let adaptive = [anthropic_dialect::THINKING_ADAPTIVE];
    refused(&turn(&hi(), &json!({"reasoning_effort": "max"})), &config(&adaptive));
    accepted(&turn(&hi(), &json!({"reasoning_effort": "high"})), &config(&adaptive));
    // Without a declared form the effort is not read at all.
    accepted(&turn(&hi(), &json!({"reasoning_effort": "max"})), &config(&[]));
    // With the caller's own mode the effort travels verbatim and the upstream rules.
    let body = accepted(
        &turn(&hi(), &json!({"reasoning_effort": "max", "reasoning_mode": "adaptive"})),
        &config(&adaptive),
    );
    assert_eq!(body["output_config"]["effort"], "max");
}

// ── Request: tool calls and tool results that cannot be paired ───────────────

#[test]
fn an_assistant_tool_call_without_an_id_is_refused() {
    let call = |id: &str| {
        json!([{
            "role": "assistant",
            "tool_calls": [{"id": id, "name": "t", "arguments": "{}"}],
        }])
    };
    refused(&turn(&call(""), &json!({})), &config(&[]));
    accepted(&turn(&call("tu_1"), &json!({})), &config(&[]));
}

#[test]
fn an_assistant_tool_call_without_a_name_is_refused() {
    let call = |name: &str| {
        json!([{
            "role": "assistant",
            "tool_calls": [{"id": "tu_1", "name": name, "arguments": "{}"}],
        }])
    };
    refused(&turn(&call(""), &json!({})), &config(&[]));
    accepted(&turn(&call("t"), &json!({})), &config(&[]));
}

#[test]
fn a_tool_result_without_a_tool_call_id_is_refused() {
    refused(&turn(&json!([{"role": "tool", "content": "12C"}]), &json!({})), &config(&[]));
    accepted(
        &turn(&json!([{"role": "tool", "tool_call_id": "tu_1", "content": "12C"}]), &json!({})),
        &config(&[]),
    );
}

#[test]
fn a_tool_result_with_an_empty_tool_call_id_is_refused() {
    refused(
        &turn(&json!([{"role": "tool", "tool_call_id": "", "content": "12C"}]), &json!({})),
        &config(&[]),
    );
    accepted(
        &turn(&json!([{"role": "tool", "tool_call_id": "tu_1", "content": "12C"}]), &json!({})),
        &config(&[]),
    );
}

// ── Request: a host value outside its syntax is the host's error ─────────────

#[test]
fn a_malformed_end_user_hash_is_refused_as_a_host_error() {
    // Also expected by the fixture `provider.request.refused-malformed-end-user-hash`. An email
    // address here would leak a raw identity into `metadata.user_id`.
    let malformed = turn(&hi(), &json!({"host_values": {"end_user_hash": "alice@example.com"}}));
    let error = build(&malformed, &config(&[])).expect_err("must be refused");
    assert_eq!(error.code, ErrorCode::Internal, "{error:?}");
    assert_eq!(error.http_status, 500, "{error:?}");

    let hashed = turn(&hi(), &json!({"host_values": {"end_user_hash": "tenant_7-abc"}}));
    assert_eq!(accepted(&hashed, &config(&[]))["metadata"]["user_id"], "tenant_7-abc");
}

// ── Response: the body and its usage, which is funds evidence ────────────────

fn response_body(body: &str) -> HttpResponseParts {
    serde_json::from_value(json!({"status": 200, "headers": {}, "body": body})).unwrap()
}

fn message_with(usage: &Value) -> HttpResponseParts {
    let mut body = json!({
        "id": "msg_1",
        "model": MODEL,
        "content": [{"type": "text", "text": "x"}],
        "stop_reason": "end_turn",
    });
    if !usage.is_null() {
        body["usage"] = usage.clone();
    }
    response_body(&body.to_string())
}

#[track_caller]
fn refused_response(parts: &HttpResponseParts) -> ErrorEnvelope {
    let error = AnthropicReferenceV1.parse_response(parts).expect_err("must be refused");
    protocol_error(&error);
    error
}

#[track_caller]
fn accepted_usage(usage: &Value) -> token_station_protocol::Usage {
    AnthropicReferenceV1.parse_response(&message_with(usage)).expect("must be accepted").usage
}

#[test]
fn a_2xx_that_is_not_json_is_a_protocol_error() {
    // A body that is not JSON would also fail the content check below, so
    // only the message tells this refusal apart (the fixtures pin messages
    // verbatim, so they are contract).
    let error = refused_response(&response_body("not json"));
    assert!(error.message.contains("invalid JSON"), "{error:?}");
    accepted_usage(&json!({"input_tokens": 5, "output_tokens": 1}));
}

#[test]
fn a_2xx_with_an_embedded_error_is_a_protocol_error() {
    let body = |error: &Value| {
        json!({
            "content": [{"type": "text", "text": "x"}],
            "usage": {"input_tokens": 5, "output_tokens": 1},
            "error": error,
        })
        .to_string()
    };
    refused_response(&response_body(&body(&json!({"type": "overloaded_error"}))));
    AnthropicReferenceV1.parse_response(&response_body(&body(&Value::Null))).unwrap();
}

#[test]
fn a_2xx_without_a_content_array_is_a_protocol_error() {
    let body = |content: &Value| {
        json!({"content": content, "usage": {"input_tokens": 5, "output_tokens": 1}}).to_string()
    };
    refused_response(&response_body(&body(&json!("x"))));
    // An empty array is a legal (tool-less, text-less) answer.
    AnthropicReferenceV1.parse_response(&response_body(&body(&json!([])))).unwrap();
}

#[test]
fn a_response_without_a_usage_object_is_a_protocol_error() {
    // Also expected by the fixture `provider.response.missing-usage`.
    // A non-object also lacks the required counts, so only the message tells
    // this refusal apart from theirs.
    for usage in [Value::Null, json!("5")] {
        let error = refused_response(&message_with(&usage));
        assert!(error.message.contains("usage object"), "{error:?}");
    }
    accepted_usage(&json!({"input_tokens": 5, "output_tokens": 1}));
}

#[test]
fn a_response_usage_without_input_tokens_is_a_protocol_error_not_a_zero() {
    refused_response(&message_with(&json!({"output_tokens": 1})));
    refused_response(&message_with(&json!({"input_tokens": null, "output_tokens": 1})));
    assert_eq!(accepted_usage(&json!({"input_tokens": 0, "output_tokens": 1})).input_tokens, 0);
}

#[test]
fn a_response_usage_without_output_tokens_is_a_protocol_error_not_a_zero() {
    refused_response(&message_with(&json!({"input_tokens": 5})));
    refused_response(&message_with(&json!({"input_tokens": 5, "output_tokens": null})));
    assert_eq!(accepted_usage(&json!({"input_tokens": 5, "output_tokens": 0})).output_tokens, 0);
}

#[test]
fn a_token_count_that_is_not_a_non_negative_integer_is_a_protocol_error() {
    for bad in [json!(-1), json!(1.5), json!("5")] {
        refused_response(&message_with(&json!({"input_tokens": bad, "output_tokens": 1})));
        // An optional bucket is held to the same type: absent is zero, wrong is not.
        refused_response(&message_with(
            &json!({"input_tokens": 5, "output_tokens": 1, "cache_read_input_tokens": bad}),
        ));
    }
    let usage = accepted_usage(
        &json!({"input_tokens": 5, "output_tokens": 1, "cache_read_input_tokens": 3}),
    );
    assert_eq!((usage.input_tokens, usage.cache_read_tokens), (8, 3));
}

#[test]
fn a_cache_creation_breakdown_without_its_total_is_a_protocol_error() {
    // Zero tiers sum to the zero an absent total would read as, so only the
    // missing total can refuse this one.
    for orphan in [
        json!({"ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": 0}),
        json!({"ephemeral_5m_input_tokens": 4, "ephemeral_1h_input_tokens": 3}),
    ] {
        refused_response(&message_with(
            &json!({"input_tokens": 5, "output_tokens": 1, "cache_creation": orphan}),
        ));
    }
    let tiers = json!({"ephemeral_5m_input_tokens": 4, "ephemeral_1h_input_tokens": 3});
    let usage = accepted_usage(&json!({
        "input_tokens": 5, "output_tokens": 1,
        "cache_creation_input_tokens": 7, "cache_creation": tiers,
    }));
    assert_eq!((usage.cache_write_5m_tokens, usage.cache_write_1h_tokens), (4, 3));
}

#[test]
fn a_cache_creation_breakdown_missing_its_5m_tier_is_a_protocol_error() {
    // The total equals the tier that is present, so only the missing count,
    // not the arithmetic, can refuse this.
    refused_response(&message_with(&json!({
        "input_tokens": 5, "output_tokens": 1, "cache_creation_input_tokens": 7,
        "cache_creation": {"ephemeral_1h_input_tokens": 7},
    })));
    accepted_usage(&json!({
        "input_tokens": 5, "output_tokens": 1, "cache_creation_input_tokens": 7,
        "cache_creation": {"ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": 7},
    }));
}

#[test]
fn a_cache_creation_breakdown_missing_its_1h_tier_is_a_protocol_error() {
    refused_response(&message_with(&json!({
        "input_tokens": 5, "output_tokens": 1, "cache_creation_input_tokens": 7,
        "cache_creation": {"ephemeral_5m_input_tokens": 7},
    })));
    accepted_usage(&json!({
        "input_tokens": 5, "output_tokens": 1, "cache_creation_input_tokens": 7,
        "cache_creation": {"ephemeral_5m_input_tokens": 7, "ephemeral_1h_input_tokens": 0},
    }));
}

#[test]
fn cache_creation_tiers_that_do_not_add_up_are_a_protocol_error() {
    // Billing prices the two tiers differently, so a split that disagrees with
    // the total leaves no way to know which part to bill at which price.
    refused_response(&message_with(&json!({
        "input_tokens": 5, "output_tokens": 1, "cache_creation_input_tokens": 7,
        "cache_creation": {"ephemeral_5m_input_tokens": 4, "ephemeral_1h_input_tokens": 4},
    })));
    accepted_usage(&json!({
        "input_tokens": 5, "output_tokens": 1, "cache_creation_input_tokens": 8,
        "cache_creation": {"ephemeral_5m_input_tokens": 4, "ephemeral_1h_input_tokens": 4},
    }));
}

#[test]
fn a_cache_creation_field_that_is_not_an_object_is_a_protocol_error() {
    refused_response(&message_with(&json!({
        "input_tokens": 5, "output_tokens": 1, "cache_creation_input_tokens": 7,
        "cache_creation": 7,
    })));
    accepted_usage(&json!({
        "input_tokens": 5, "output_tokens": 1, "cache_creation_input_tokens": 7,
        "cache_creation": null,
    }));
}

#[test]
fn prompt_buckets_whose_sum_overflows_are_a_protocol_error() {
    // The IR's input count is the sum of the three wire buckets; a saturated
    // sum would bill a number the upstream never reported.
    refused_response(&message_with(
        &json!({"input_tokens": u64::MAX, "output_tokens": 1, "cache_read_input_tokens": 1}),
    ));
    assert_eq!(
        accepted_usage(&json!({"input_tokens": u64::MAX, "output_tokens": 1})).input_tokens,
        u64::MAX
    );
}

// ── Stream ───────────────────────────────────────────────────────────────────

fn frame(event: &str, data: &Value) -> Vec<u8> {
    format!("event: {event}\ndata: {data}\n\n").into_bytes()
}

fn start(index: &Value) -> Vec<u8> {
    frame(
        "content_block_start",
        &json!({"index": index, "content_block": {"type": "text", "text": ""}}),
    )
}

fn delta(index: u32) -> Vec<u8> {
    frame(
        "content_block_delta",
        &json!({"index": index, "delta": {"type": "text_delta", "text": "hi"}}),
    )
}

fn stop(index: u32) -> Vec<u8> {
    frame("content_block_stop", &json!({"index": index}))
}

fn message_start(usage: &Value) -> Vec<u8> {
    frame("message_start", &json!({"message": {"usage": usage}}))
}

fn message_delta(usage: &Value) -> Vec<u8> {
    frame("message_delta", &json!({"delta": {"stop_reason": "end_turn"}, "usage": usage}))
}

/// Feeds `chunks` to a fresh parser; every chunk but the last must be accepted, and the last
/// one's result is returned.
fn feed(chunks: &[Vec<u8>]) -> Result<Vec<StreamEvent>, ErrorEnvelope> {
    let mut parser: Box<dyn StreamParserV1> = AnthropicReferenceV1.stream_parser();
    let (last, earlier) = chunks.split_last().unwrap();
    for chunk in earlier {
        parser.parse_chunk(chunk).expect("an earlier chunk must be accepted");
    }
    parser.parse_chunk(last)
}

#[track_caller]
fn refused_stream(chunks: &[Vec<u8>]) {
    protocol_error(&feed(chunks).expect_err("must be refused"));
}

#[track_caller]
fn accepted_stream(chunks: &[Vec<u8>]) -> Vec<StreamEvent> {
    feed(chunks).expect("must be accepted")
}

/// The usage the last accepted chunk reported.
#[track_caller]
fn last_usage(chunks: &[Vec<u8>]) -> token_station_protocol::Usage {
    accepted_stream(chunks)
        .into_iter()
        .find_map(|event| match event {
            StreamEvent::Usage { usage } => Some(usage),
            _ => None,
        })
        .expect("a usage report")
}

#[test]
fn a_stream_frame_that_is_not_utf8_is_a_protocol_error() {
    refused_stream(&[b"event: ping\ndata: \xff\n\n".to_vec()]);
    accepted_stream(&[b"event: ping\ndata: {}\n\n".to_vec()]);
}

#[test]
fn a_stream_frame_with_invalid_json_is_a_protocol_error() {
    refused_stream(&[b"event: ping\ndata: {not json\n\n".to_vec()]);
    // A frame without data is not an event at all and is skipped.
    assert!(accepted_stream(&[b"event: ping\n\n".to_vec()]).is_empty());
}

#[test]
fn a_content_block_index_that_is_missing_negative_or_too_large_is_a_protocol_error() {
    for index in [Value::Null, json!(-1), json!(u64::from(u32::MAX) + 1), json!("0")] {
        refused_stream(&[start(&index)]);
    }
    accepted_stream(&[start(&json!(u32::MAX))]);
}

#[test]
fn a_content_block_opened_while_open_is_a_protocol_error() {
    refused_stream(&[start(&json!(0)), start(&json!(0))]);
    accepted_stream(&[start(&json!(0)), start(&json!(1))]);
}

#[test]
fn a_content_block_reopened_after_it_stopped_is_a_protocol_error() {
    // A reopened index would merge two blocks' text or tool arguments.
    refused_stream(&[start(&json!(0)), stop(0), start(&json!(0))]);
    accepted_stream(&[start(&json!(0)), stop(0), start(&json!(1))]);
}

#[test]
fn a_delta_for_a_content_block_that_is_not_open_is_a_protocol_error() {
    refused_stream(&[delta(0)]);
    refused_stream(&[start(&json!(0)), stop(0), delta(0)]);
    assert_eq!(accepted_stream(&[start(&json!(0)), delta(0)]).len(), 1);
}

#[test]
fn a_stop_for_a_content_block_that_is_not_open_is_a_protocol_error() {
    refused_stream(&[stop(0)]);
    refused_stream(&[start(&json!(0)), stop(0), stop(0)]);
    accepted_stream(&[start(&json!(0)), stop(0)]);
}

#[test]
fn a_message_delta_while_a_content_block_is_open_is_a_protocol_error() {
    // The terminal delta announces Done; announcing it with a block still open
    // would settle a message whose content never finished.
    let usage = json!({"output_tokens": 2});
    refused_stream(&[start(&json!(0)), message_delta(&usage)]);
    let events = accepted_stream(&[start(&json!(0)), stop(0), message_delta(&usage)]);
    assert!(matches!(events.last(), Some(StreamEvent::Done { .. })), "{events:?}");
}

#[test]
fn a_message_start_usage_without_input_tokens_is_a_protocol_error() {
    refused_stream(&[message_start(&json!({"output_tokens": 1}))]);
    // The output side arrives later, so message_start may leave it out.
    assert_eq!(last_usage(&[message_start(&json!({"input_tokens": 5}))]).input_tokens, 5);
}

#[test]
fn a_message_delta_usage_without_output_tokens_is_a_protocol_error() {
    refused_stream(&[message_delta(&json!({"input_tokens": 5}))]);
    assert_eq!(last_usage(&[message_delta(&json!({"output_tokens": 2}))]).output_tokens, 2);
}

/// `message_start` reporting `first`, then a terminal `message_delta` reporting `later`.
fn reports(first: &Value, later: &Value) -> Vec<Vec<u8>> {
    vec![message_start(first), message_delta(later)]
}

#[test]
fn a_shrinking_cumulative_input_count_is_a_protocol_error() {
    // Also expected by the fixture `provider.stream.a-shrinking-cumulative-count-is-refused`.
    let first = json!({"input_tokens": 10});
    refused_stream(&reports(&first, &json!({"input_tokens": 9, "output_tokens": 2})));
    // Zero means "not filled in" on this wire and keeps the earlier count.
    let usage = last_usage(&reports(&first, &json!({"input_tokens": 0, "output_tokens": 2})));
    assert_eq!(usage.input_tokens, 10);
    let usage = last_usage(&reports(&first, &json!({"input_tokens": 11, "output_tokens": 2})));
    assert_eq!(usage.input_tokens, 11);
}

#[test]
fn a_shrinking_cumulative_output_count_is_a_protocol_error() {
    let first = json!({"input_tokens": 10, "output_tokens": 3});
    refused_stream(&reports(&first, &json!({"output_tokens": 2})));
    assert_eq!(last_usage(&reports(&first, &json!({"output_tokens": 3}))).output_tokens, 3);
}

#[test]
fn a_shrinking_cumulative_cache_read_count_is_a_protocol_error() {
    let first = json!({"input_tokens": 10, "cache_read_input_tokens": 6});
    refused_stream(&reports(&first, &json!({"output_tokens": 2, "cache_read_input_tokens": 5})));
    let usage =
        last_usage(&reports(&first, &json!({"output_tokens": 2, "cache_read_input_tokens": 7})));
    assert_eq!(usage.cache_read_tokens, 7);
}

#[test]
fn a_shrinking_cumulative_cache_write_count_is_a_protocol_error() {
    let first = json!({"input_tokens": 10, "cache_creation_input_tokens": 6});
    refused_stream(&reports(
        &first,
        &json!({"output_tokens": 2, "cache_creation_input_tokens": 5}),
    ));
    let usage = last_usage(&reports(
        &first,
        &json!({"output_tokens": 2, "cache_creation_input_tokens": 7}),
    ));
    assert_eq!(usage.cache_write_tokens, 7);
}
