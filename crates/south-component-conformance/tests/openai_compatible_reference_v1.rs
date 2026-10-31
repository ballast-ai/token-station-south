//! Direct refusal tests over the `provider-openai-compatible` reference implementation (#316).
//!
//! # Why a reference-level leg exists
//!
//! Gate ② fixtures can expect a refusal (`{"error": <envelope>}`, since #131), but each one is a
//! case of the shipped suite, so the pack carries only a handful: the dated Azure version, cap
//! field and model, the Bailian cap split and budget, the `OpenAI` replay carrier and a missing
//! usage object. This file enumerates every refusal reachable through `ProviderComponentV1` —
//! request, response and stream — and pairs each with a neighbouring input that is accepted, so a
//! test cannot pass because the whole path is broken. A refusal nobody tests can silently become a
//! pass-through, and on the host side a pass-through is a dispatched, billed call; on the response
//! side the refusals are funds evidence (a usage object that does not add up must never settle as
//! zero).
//!
//! Every refusal covered here was mutation-checked: with the refusal removed or inverted in
//! `src/reference.rs`, the matching test fails (29 mutations, all 29 red).
//!
//! Not covered, and why: `SafeHeaders::try_new` and `Auth::header` / `Auth::bearer_and_header`
//! over constant header names cannot fail on any input; `checked_sum`'s overflow on the cached
//! and orchestration sums is the same guard as the prompt-plus-completion sum tested below.

use serde_json::{Value, json};
use south_component_conformance::{ProviderComponentV1, reference::OpenAiCompatibleReferenceV1};
use token_station_protocol::{
    ChatRequest, ErrorCode, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts,
    ProviderConfig, StreamEvent,
};

const MODEL: &str = "m";
const BASE: &str = "https://upstream.example/v1";

fn config_for(provider: &str, models: &Value, declared: &Value) -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": provider,
        "base_url": BASE,
        "auth": "provider_api_key",
        "declared": declared,
        "models": models,
    }))
    .unwrap()
}

fn config() -> ProviderConfig {
    config_for("openai-compatible", &json!([]), &json!({}))
}

fn request(value: &Value) -> ChatRequest {
    serde_json::from_value(value.clone()).unwrap()
}

fn hello() -> Value {
    json!({"model": MODEL, "messages": [{"role": "user", "content": "hi"}]})
}

fn build(
    request: &ChatRequest,
    config: &ProviderConfig,
) -> Result<HttpRequestDescriptor, ErrorEnvelope> {
    OpenAiCompatibleReferenceV1.build_http_request(request, config)
}

fn refused(result: Result<HttpRequestDescriptor, ErrorEnvelope>) -> ErrorEnvelope {
    result.expect_err("the request must be refused")
}

fn assert_capability(error: &ErrorEnvelope) {
    assert_eq!((error.code, error.http_status), (ErrorCode::Capability, 400), "{error:?}");
}

fn assert_internal(error: &ErrorEnvelope) {
    assert_eq!((error.code, error.http_status), (ErrorCode::Internal, 500), "{error:?}");
}

fn assert_protocol(error: &ErrorEnvelope, needle: &str) {
    assert_eq!(
        (error.code, error.http_status),
        (ErrorCode::ProviderProtocolError, 502),
        "{error:?}"
    );
    assert!(error.message.contains(needle), "expected `{needle}` in `{}`", error.message);
}

// ── Request side ────────────────────────────────────────────────────────────

fn replaying(family: &str) -> Value {
    json!({
        "model": MODEL,
        "messages": [
            {"role": "user", "content": "q"},
            {
                "role": "assistant",
                "content": [{"type": "thinking", "thinking": "t"}, {"type": "text", "text": "a"}],
                "reasoning_replay_protocol_family": family,
            },
            {"role": "user", "content": "again"},
        ],
    })
}

#[test]
fn a_claude_reasoning_replay_carrier_is_refused() {
    let error = refused(build(&request(&replaying("claude-signed-thinking")), &config()));
    assert_capability(&error);
    assert!(error.message.contains("Claude reasoning replay"));
    // The same thinking block without the carrier is plain history and goes out.
    let mut plain = replaying("claude-signed-thinking");
    plain["messages"][1].as_object_mut().unwrap().remove("reasoning_replay_protocol_family");
    build(&request(&plain), &config()).expect("unmarked thinking is sent");
}

#[test]
fn an_openai_reasoning_replay_carrier_is_refused() {
    let error = refused(build(&request(&replaying("openai-encrypted-reasoning")), &config()));
    assert_capability(&error);
    assert!(error.message.contains("OpenAI reasoning replay"));
    // Any other family label is not this carrier.
    build(&request(&replaying("some-other-family")), &config()).expect("not a replay carrier");
}

fn forcing(tool_choice: &Value) -> Value {
    json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"name": "t", "parameters": {"type": "object"}}],
        "tool_choice": tool_choice,
    })
}

#[test]
fn a_forced_tool_choice_on_a_model_declaring_auto_only_is_refused() {
    let declaring = config_for(
        "openai-compatible",
        &json!([{"model": MODEL, "supported_parameters": ["anthropic.tool_choice.auto_only"]}]),
        &json!({}),
    );
    for forced in [json!("required"), json!({"type": "function", "function": {"name": "t"}})] {
        assert_capability(&refused(build(&request(&forcing(&forced)), &declaring)));
        // Forcing is fine on a model that does not declare the restriction.
        build(&request(&forcing(&forced)), &config()).expect("undeclared model forces freely");
    }
    // `auto` is what the refusal points the client to.
    build(&request(&forcing(&json!("auto"))), &declaring).expect("auto is accepted");
}

fn with_user_part(part: &Value) -> Value {
    json!({"model": MODEL, "messages": [{"role": "user", "content": [part]}]})
}

#[test]
fn a_part_with_no_chat_completions_spelling_is_refused() {
    let error = refused(build(
        &request(&with_user_part(&json!({"type": "document", "source": {}}))),
        &config(),
    ));
    assert_capability(&error);
    assert!(error.message.contains("document"));
    // A Chat Completions part the IR does not model is forwarded as-is.
    let audio = json!({"type": "input_audio", "input_audio": {"data": "AA==", "format": "wav"}});
    build(&request(&with_user_part(&audio)), &config()).expect("a native part is forwarded");
}

fn dated(declared: &Value) -> ProviderConfig {
    config_for("azure-openai-dated", &json!([]), declared)
}

#[test]
fn an_undated_api_version_on_the_dated_azure_family_is_refused() {
    assert_internal(&refused(build(&request(&hello()), &dated(&json!({"api_version": "v1"})))));
    for version in ["2024-10-21", "2025-04-01-preview"] {
        let descriptor = build(&request(&hello()), &dated(&json!({"api_version": version})))
            .expect("a dated version is accepted");
        assert!(descriptor.url.ends_with(&format!("?api-version={version}")));
    }
}

#[test]
fn a_request_without_a_model_on_the_dated_azure_family_is_refused() {
    let mut anonymous = request(&hello());
    anonymous.model = String::new();
    assert_capability(&refused(build(&anonymous, &dated(&json!({})))));
    // The other families resolve their URL without the model.
    build(&anonymous, &config()).expect("openai-compatible needs no model in the path");
}

#[test]
fn an_unknown_cap_field_on_an_azure_family_is_refused() {
    let capped = request(&json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "hi"}],
        "sampling": {"max_output_tokens": 64},
    }));
    let field = json!({"max_tokens_field": "max_output_tokens"});
    for family in ["azure-openai-v1", "azure-openai-dated"] {
        assert_internal(&refused(build(&capped, &config_for(family, &json!([]), &field))));
        let body = build(
            &capped,
            &config_for(family, &json!([]), &json!({"max_tokens_field": "max_tokens"})),
        )
        .expect("a wire name is accepted")
        .body
        .unwrap();
        assert_eq!(body["max_tokens"], 64);
    }
    // Families that do not read the key ignore it.
    let body =
        build(&capped, &config_for("openai-compatible", &json!([]), &field)).unwrap().body.unwrap();
    assert_eq!(body["max_tokens"], 64);
}

fn bailian() -> ProviderConfig {
    config_for(
        "bailian-openai-compatible",
        &json!([{"model": MODEL, "supported_parameters": ["thinking_budget"]}]),
        &json!({}),
    )
}

fn bailian_request(cap: u32, extra: &Value) -> ChatRequest {
    let mut value = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "hi"}],
        "sampling": {"max_output_tokens": cap},
    });
    for (key, field) in extra.as_object().unwrap() {
        value[key] = field.clone();
    }
    request(&value)
}

#[test]
fn a_cap_too_small_to_split_on_a_thinking_bailian_model_is_refused() {
    assert_capability(&refused(build(&bailian_request(4, &json!({})), &bailian())));
    build(&bailian_request(5, &json!({})), &bailian()).expect("5 leaves room for both caps");
    // With thinking off nothing is split, so the same small cap goes out unchanged.
    let off = build(&bailian_request(4, &json!({"enable_thinking": false})), &bailian()).unwrap();
    assert_eq!(off.body.unwrap()["max_tokens"], 4);
}

#[test]
fn a_non_integer_thinking_budget_on_a_bailian_model_is_refused() {
    for budget in [json!("20"), json!(-1), json!(1.5)] {
        assert_capability(&refused(build(
            &bailian_request(1024, &json!({"thinking_budget": budget})),
            &bailian(),
        )));
    }
    for budget in [json!(20), Value::Null] {
        build(&bailian_request(1024, &json!({"thinking_budget": budget})), &bailian())
            .expect("an integer or null budget is accepted");
    }
}

#[test]
fn a_dialect_the_package_does_not_declare_is_refused() {
    let error =
        refused(build(&request(&hello()), &config_for("anthropic", &json!([]), &json!({}))));
    assert_capability(&error);
    assert!(error.message.contains("unsupported provider dialect"));
}

// ── Response side ───────────────────────────────────────────────────────────

fn parts(status: u16, body: &str) -> HttpResponseParts {
    serde_json::from_value(json!({"status": status, "headers": {}, "body": body})).unwrap()
}

fn completion(usage: &Value) -> Value {
    json!({
        "id": "c",
        "model": MODEL,
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
        "usage": usage,
    })
}

fn usage() -> Value {
    json!({"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15})
}

fn parse(body: &Value) -> Result<token_station_protocol::ChatResponse, ErrorEnvelope> {
    OpenAiCompatibleReferenceV1.parse_response(&parts(200, &body.to_string()))
}

fn parse_refused(body: &Value, needle: &str) {
    assert_protocol(&parse(body).expect_err("the response must be refused"), needle);
}

#[test]
fn a_well_formed_completion_is_accepted() {
    // The neighbour of every response refusal below: each one changes one field of this body.
    let response = parse(&completion(&usage())).expect("accepted");
    assert_eq!(response.usage.input_tokens, 10);
    assert_eq!(response.usage.output_tokens, 5);
}

#[test]
fn a_two_hundred_with_a_body_that_is_not_json_is_refused() {
    let error = OpenAiCompatibleReferenceV1.parse_response(&parts(200, "not json")).unwrap_err();
    assert_protocol(&error, "invalid JSON");
}

#[test]
fn an_error_embedded_in_a_two_hundred_is_refused() {
    let mut body = completion(&usage());
    body["error"] = json!({"message": "overloaded"});
    parse_refused(&body, "embedded an error");
    body["error"] = Value::Null;
    parse(&body).expect("a null error is no error");
}

#[test]
fn a_completion_without_a_choices_array_is_refused() {
    let mut body = completion(&usage());
    body["choices"] = json!({});
    parse_refused(&body, "no choices array");
}

#[test]
fn a_completion_with_no_choices_is_refused() {
    let mut body = completion(&usage());
    body["choices"] = json!([]);
    parse_refused(&body, "contains no choices");
}

#[test]
fn a_completion_without_a_usage_object_is_refused() {
    let mut body = completion(&usage());
    body.as_object_mut().unwrap().remove("usage");
    parse_refused(&body, "usage object");
    parse_refused(&completion(&json!("15")), "usage object");
}

#[test]
fn a_usage_missing_any_required_count_is_refused() {
    for key in ["prompt_tokens", "completion_tokens", "total_tokens"] {
        let mut missing = usage();
        missing.as_object_mut().unwrap().remove(key);
        parse_refused(&completion(&missing), &format!("lacks a valid {key} count"));
    }
}

#[test]
fn a_usage_count_that_is_not_a_non_negative_integer_is_refused() {
    for bad in [json!("10"), json!(-1), json!(1.5), Value::Null] {
        let mut wrong = usage();
        wrong["prompt_tokens"] = bad;
        parse_refused(&completion(&wrong), "lacks a valid prompt_tokens count");
    }
}

#[test]
fn a_details_field_that_is_not_an_object_is_refused() {
    for key in ["prompt_tokens_details", "completion_tokens_details"] {
        let mut wrong = usage();
        wrong[key] = json!([]);
        parse_refused(&completion(&wrong), &format!("invalid {key} field"));
        wrong[key] = Value::Null;
        parse(&completion(&wrong)).expect("a null details field is absent");
    }
}

#[test]
fn an_optional_count_that_is_not_an_integer_is_refused() {
    let mut wrong = usage();
    wrong["prompt_tokens_details"] = json!({"cached_tokens": "3"});
    parse_refused(&completion(&wrong), "invalid token count");
    wrong["prompt_tokens_details"] = json!({"cached_tokens": 3});
    assert_eq!(parse(&completion(&wrong)).unwrap().usage.cache_read_tokens, 3);
}

#[test]
fn an_orchestration_cached_count_above_its_input_is_refused() {
    let mut wrong = json!({
        "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 19,
        "prompt_tokens_details": {"orchestration_input_tokens": 4, "orchestration_input_cached_tokens": 5},
    });
    parse_refused(&completion(&wrong), "orchestration cached");
    wrong["prompt_tokens_details"]["orchestration_input_cached_tokens"] = json!(4);
    assert_eq!(parse(&completion(&wrong)).unwrap().usage.input_tokens, 14);
}

#[test]
fn a_total_that_does_not_add_up_is_refused() {
    let mut wrong = usage();
    wrong["total_tokens"] = json!(16);
    parse_refused(&completion(&wrong), "total_tokens does not equal");
    // Orchestration input is the one addend the total may also carry.
    let orchestrated = json!({
        "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 19,
        "prompt_tokens_details": {"orchestration_input_tokens": 4},
    });
    parse(&completion(&orchestrated)).expect("prompt + completion + orchestration input");
}

#[test]
fn cached_and_cache_write_counts_above_the_prompt_are_refused() {
    let mut wrong = usage();
    wrong["prompt_tokens_details"] = json!({"cached_tokens": 6, "cache_write_tokens": 5});
    parse_refused(&completion(&wrong), "cache-write counts exceed");
    wrong["prompt_tokens_details"] = json!({"cached_tokens": 5, "cache_write_tokens": 5});
    parse(&completion(&wrong)).expect("the subsets may fill the prompt exactly");
}

#[test]
fn reasoning_tokens_above_the_completion_are_refused() {
    let mut wrong = usage();
    wrong["completion_tokens_details"] = json!({"reasoning_tokens": 6});
    parse_refused(&completion(&wrong), "reasoning_tokens exceeds");
    wrong["completion_tokens_details"] = json!({"reasoning_tokens": 5});
    parse(&completion(&wrong)).expect("reasoning may be the whole completion");
}

#[test]
fn counts_whose_sum_overflows_are_refused() {
    let wrong = json!({"prompt_tokens": u64::MAX, "completion_tokens": 1, "total_tokens": 0});
    parse_refused(&completion(&wrong), "overflow");
}

#[test]
fn an_error_status_is_mapped_not_refused() {
    // `map_provider_error` never refuses: every status, readable body or not, becomes an envelope.
    for (status, body) in [(429, r#"{"error":{"message":"slow down"}}"#), (500, "not json")] {
        OpenAiCompatibleReferenceV1
            .map_provider_error(&parts(status, body))
            .expect("a non-2xx answer always maps to an envelope");
    }
}

// ── Stream side ─────────────────────────────────────────────────────────────

fn stream(frames: &[&[u8]]) -> Result<Vec<StreamEvent>, ErrorEnvelope> {
    let mut parser = OpenAiCompatibleReferenceV1.stream_parser();
    let mut events = Vec::new();
    for frame in frames {
        events.extend(parser.parse_chunk(frame)?);
    }
    events.extend(parser.parse_chunk(&[])?);
    Ok(events)
}

fn stream_refused(frame: &[u8], needle: &str) {
    assert_protocol(&stream(&[frame]).expect_err("the stream must be refused"), needle);
}

const DELTA: &[u8] = b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n";

#[test]
fn a_well_formed_stream_frame_is_accepted() {
    let events = stream(&[DELTA]).expect("accepted");
    assert!(events.iter().any(|event| matches!(event, StreamEvent::Delta { .. })));
}

#[test]
fn a_frame_that_is_not_json_is_refused() {
    stream_refused(b"data: {not json\n\n", "invalid SSE JSON");
}

#[test]
fn an_error_embedded_in_a_stream_frame_is_refused() {
    stream_refused(
        b"data: {\"error\":{\"message\":\"x\"},\"choices\":[]}\n\n",
        "embedded an error",
    );
    stream(&[b"data: {\"error\":null,\"choices\":[{\"index\":0,\"delta\":{}}]}\n\n"])
        .expect("a null error is no error");
}

#[test]
fn a_frame_with_an_invalid_usage_field_is_refused() {
    stream_refused(b"data: {\"choices\":[],\"usage\":\"15\"}\n\n", "invalid usage field");
}

#[test]
fn a_frame_with_an_invalid_choices_field_is_refused() {
    stream_refused(b"data: {\"choices\":{}}\n\n", "invalid choices field");
    // Without usage, a frame must carry choices.
    stream_refused(b"data: {\"id\":\"c\"}\n\n", "invalid choices field");
    // A usage-only frame needs none.
    stream(&[
        b"data: {\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n",
    ])
    .expect("a usage-only frame is accepted");
}

#[test]
fn a_frame_whose_usage_does_not_add_up_is_refused() {
    stream_refused(
        b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":3}}\n\n",
        "total_tokens does not equal",
    );
}

#[test]
fn a_frame_that_is_not_utf8_is_refused() {
    stream_refused(b"data: {\"x\":\"\xff\"}\n\n", "non-UTF-8");
    // A UTF-8 sequence split across chunks is buffered, not refused.
    let text =
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\u{e9}\"}}]}\n\n".as_bytes();
    let split = text.iter().position(|&byte| byte == 0xc3).unwrap() + 1;
    stream(&[&text[..split], &text[split..]]).expect("a split sequence is reassembled");
}

#[test]
fn an_sse_error_event_ends_in_an_error_event_not_a_refusal() {
    // The upstream named the failure itself: the parser reports it in-band instead of failing the
    // chunk, so the events before it are still delivered.
    let events = stream(&[DELTA, b"event: error\ndata: {\"message\":\"x\"}\n\n"]).expect("in-band");
    let Some(StreamEvent::Error { error }) = events.last() else {
        panic!("the last event is the error: {events:?}");
    };
    assert_protocol(error, "SSE error event");
    assert!(matches!(events.first(), Some(StreamEvent::Delta { .. })));
}
