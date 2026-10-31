//! Direct refusal tests over the Gemini provider reference (`GeminiReferenceV1`), whose request
//! body, response reader, stream parser and refusals live in the crate-private `gemini_text`.
//!
//! # Why a reference-level leg exists
//!
//! Gate ②'s fixture packs can expect an error envelope (`expected_error`, south #131), but each
//! such case is a shipped-suite case, so they are added sparingly: `fixtures-gemini` carries four
//! (an empty request-side call signature, `OpenAI` reasoning replay, a non-string response-side
//! call signature, a response without `usageMetadata`). A refusal nobody tests can silently become
//! a pass-through, and on the host side a pass-through is a dispatched, billed call. So this leg
//! enumerates every refusal reachable through the `ProviderComponentV1` surface and pins each one
//! with a neighbouring accepted input that differs only in the refused aspect, so no test can pass
//! because the whole path is broken. The four refusals a fixture already covers are tested here
//! too, next to their neighbours; their tests say so.
//!
//! `gemini_text` is shared with `provider-vertex`; this file reaches it only through
//! `GeminiReferenceV1`, so it says nothing about Vertex's route or auth. The forced-tool refusal
//! lives in `anthropic_dialect` (every family that serves a model declared auto-only calls it) and
//! is pinned here at the Gemini call site.
//!
//! Each refusal is asserted by code, status and a fragment of its own message: several refusals sit
//! in front of later ones (an unparseable body would otherwise fail as "no candidates"), and
//! without the message a test would stay green when its refusal is removed and a later one fires.
//!
//! # Mutation check
//!
//! Each of the 29 refusal points below was removed or inverted in `src/` one at a time (the
//! refusal's branch made to accept, or its `?` turned into a default), and the matching test went
//! red; the source was then restored byte for byte. Three points are shared by two tests each (the
//! response-side call signature with the streamed one, the count validity check with the
//! intermediate-count test, the decrease check with the terminal-omission test).
//!
//! # Not covered, and why
//!
//! - `map_provider_error` never refuses: it classifies every non-2xx (an unparseable body falls
//!   back to the status), so there is no refusal to pin.
//! - The `content-type` header (`SafeHeaders::try_new`) and the `x-goog-api-key` auth placement
//!   (`Auth::header`) can fail only on the header *name*, which is a constant in the source; no
//!   input reaches either error.

use serde_json::{Value, json};
use south_component_conformance::{ProviderComponentV1, reference_gemini::GeminiReferenceV1};
use token_station_protocol::{
    ChatRequest, ChatResponse, ErrorCode, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts,
    ProviderConfig, StreamEvent,
};

const MODEL: &str = "gemini-2.5-flash";
const BASE: &str = "https://generativelanguage.googleapis.com";

fn config() -> ProviderConfig {
    serde_json::from_value(json!({"provider":"gemini","base_url":BASE,"models":[]})).unwrap()
}

fn request(value: Value) -> ChatRequest {
    serde_json::from_value(value).unwrap()
}

fn turn(messages: &Value) -> ChatRequest {
    request(json!({"model": MODEL, "messages": messages}))
}

fn build(request: &ChatRequest) -> Result<HttpRequestDescriptor, ErrorEnvelope> {
    GeminiReferenceV1.build_http_request(request, &config())
}

fn built(request: &ChatRequest) -> Value {
    build(request).unwrap().body.unwrap()
}

fn response(body: &Value) -> HttpResponseParts {
    raw_response(&body.to_string())
}

fn raw_response(body: &str) -> HttpResponseParts {
    serde_json::from_value(json!({"status":200,"headers":{},"body":body})).unwrap()
}

fn parse(body: &Value) -> Result<ChatResponse, ErrorEnvelope> {
    GeminiReferenceV1.parse_response(&response(body))
}

/// A request-side refusal is the caller's request this wire cannot carry: 400 capability.
///
/// `names` is a fragment of the refusal's own message, so the test goes red when a different,
/// later refusal fires instead of this one.
fn assert_capability(result: Result<HttpRequestDescriptor, ErrorEnvelope>, names: &str) {
    let error = result.expect_err(names);
    assert_eq!(error.code, ErrorCode::Capability, "{names}: {}", error.message);
    assert_eq!(error.http_status, 400, "{names}");
    assert!(error.message.contains(names), "expected `{names}`, got: {}", error.message);
}

/// A response- or stream-side refusal is the upstream breaking its protocol: 502.
fn assert_protocol<T: std::fmt::Debug>(result: Result<T, ErrorEnvelope>, names: &str) {
    let error = result.expect_err(names);
    assert_eq!(error.code, ErrorCode::ProviderProtocolError, "{names}: {}", error.message);
    assert_eq!(error.http_status, 502, "{names}");
    assert!(error.message.contains(names), "expected `{names}`, got: {}", error.message);
}

/// The neighbour of every usage refusal: a candidate and counts that add up.
fn body_with_usage(usage: &Value) -> Value {
    json!({
        "candidates": [{"content": {"role": "model", "parts": [{"text": "hi"}]},
                        "finishReason": "STOP"}],
        "usageMetadata": usage,
    })
}

fn usage(prompt: u64, candidates: u64) -> Value {
    json!({"promptTokenCount": prompt, "candidatesTokenCount": candidates,
           "totalTokenCount": prompt + candidates})
}

// ── Request: routing and addressing ────────────────────────────────────────

#[test]
fn another_dialects_config_is_refused_as_a_host_routing_error() {
    let foreign: ProviderConfig =
        serde_json::from_value(json!({"provider":"vertex-ai","base_url":BASE,"models":[]}))
            .unwrap();
    let hi = turn(&json!([{"role":"user","content":"hi"}]));
    assert_capability(
        GeminiReferenceV1.build_http_request(&hi, &foreign),
        "unsupported provider dialect",
    );
    assert!(build(&hi).is_ok(), "the same request on the gemini family is accepted");
}

#[test]
fn a_request_without_a_model_is_refused_because_the_path_needs_one() {
    let mut empty = turn(&json!([{"role":"user","content":"hi"}]));
    empty.model = String::new();
    assert_capability(build(&empty), "addresses the model in the URL path");
    let descriptor = build(&turn(&json!([{"role":"user","content":"hi"}]))).unwrap();
    assert_eq!(descriptor.url, format!("{BASE}/v1beta/models/{MODEL}:generateContent"));
}

#[test]
fn a_forced_tool_choice_on_a_model_declared_auto_only_is_refused() {
    let declared: ProviderConfig = serde_json::from_value(json!({
        "provider": "gemini", "base_url": BASE,
        "models": [{"model": MODEL, "supported_parameters": ["anthropic.tool_choice.auto_only"]}],
    }))
    .unwrap();
    let with_choice = |choice: Value| {
        request(json!({
            "model": MODEL,
            "messages": [{"role":"user","content":"hi"}],
            "tools": [{"name":"t","parameters":{"type":"object"}}],
            "tool_choice": choice,
        }))
    };
    for forced in [json!("required"), json!({"type":"function","function":{"name":"t"}})] {
        assert_capability(
            GeminiReferenceV1.build_http_request(&with_choice(forced.clone()), &declared),
            "does not support forcing a tool call",
        );
        // The same forced choice on a model that declares nothing is accepted.
        assert!(build(&with_choice(forced)).is_ok());
    }
    assert!(GeminiReferenceV1.build_http_request(&with_choice(json!("auto")), &declared).is_ok());
}

// ── Request: reasoning replay this wire cannot carry ───────────────────────

fn replayed_assistant(family: Option<&str>) -> ChatRequest {
    let mut assistant = json!({"role":"assistant","content":[
        {"type":"thinking","thinking":"7 x 13 = 91."},
        {"type":"text","text":"No."},
    ]});
    if let Some(family) = family {
        assistant["reasoning_replay_protocol_family"] = json!(family);
    }
    turn(&json!([{"role":"user","content":"Is 91 prime?"}, assistant]))
}

#[test]
fn claude_signed_thinking_replay_is_refused() {
    assert_capability(
        build(&replayed_assistant(Some("claude-signed-thinking"))),
        "cannot consume Claude reasoning replay",
    );
    assert!(build(&replayed_assistant(None)).is_ok(), "the same turn without the carrier");
}

#[test]
fn openai_encrypted_reasoning_replay_is_refused() {
    // Also an error-expecting fixture (`provider.request.refused-openai-reasoning-replay`).
    assert_capability(
        build(&replayed_assistant(Some("openai-encrypted-reasoning"))),
        "cannot consume OpenAI reasoning replay",
    );
    assert!(build(&replayed_assistant(None)).is_ok(), "the same turn without the carrier");
}

// ── Request: content and tool shapes Gemini cannot spell ───────────────────

#[test]
fn a_content_part_with_no_gemini_spelling_is_refused_by_name() {
    let error = build(&turn(&json!([{"role":"user","content":[
        {"type":"text","text":"hi"},
        {"type":"document","source":{"type":"text","media_type":"text/plain","data":"d"}},
    ]}])))
    .expect_err("a foreign block has no `parts` rendering");
    assert_eq!(error.code, ErrorCode::Capability);
    assert!(error.message.contains("`document`"), "{}", error.message);
    let body = built(&turn(&json!([{"role":"user","content":[{"type":"text","text":"hi"}]}])));
    assert_eq!(body["contents"][0]["parts"], json!([{"text":"hi"}]));
}

fn assistant_call(call: &Value) -> ChatRequest {
    turn(&json!([
        {"role":"user","content":"weather?"},
        {"role":"assistant","tool_calls":[call]},
    ]))
}

#[test]
fn an_assistant_tool_call_without_a_name_is_refused_because_gemini_keys_calls_by_name() {
    assert_capability(
        build(&assistant_call(&json!({"id":"c1","name":"","arguments":"{}"}))),
        "has no name",
    );
    let body = built(&assistant_call(&json!({"id":"c1","name":"get_weather","arguments":"{}"})));
    assert_eq!(body["contents"][1]["parts"][0]["functionCall"]["name"], "get_weather");
}

#[test]
fn an_assistant_tool_call_with_an_empty_signature_is_refused() {
    // Also an error-expecting fixture (`provider.request.an-empty-tool-call-signature-is-refused`).
    assert_capability(
        build(&assistant_call(&json!({"id":"c1","name":"t","arguments":"{}","signature":""}))),
        "carries an empty signature",
    );
    let body =
        built(&assistant_call(&json!({"id":"c1","name":"t","arguments":"{}","signature":"sig"})));
    assert_eq!(body["contents"][1]["parts"][0]["thoughtSignature"], "sig");
}

#[test]
fn a_tool_result_for_a_call_never_made_is_refused() {
    let exchange = |result: Value| {
        turn(&json!([
            {"role":"user","content":"weather?"},
            {"role":"assistant","tool_calls":[{"id":"c1","name":"get_weather","arguments":"{}"}]},
            result,
        ]))
    };
    // Gemini keys a result by the function's name, which only the call carries: an unknown id
    // and a missing id both leave nothing to key it by.
    for orphan in [
        json!({"role":"tool","tool_call_id":"never-made","content":"18C"}),
        json!({"role":"tool","content":"18C"}),
    ] {
        assert_capability(build(&exchange(orphan)), "a call this exchange never made");
    }
    let body = built(&exchange(json!({"role":"tool","tool_call_id":"c1","content":"18C"})));
    assert_eq!(body["contents"][2]["parts"][0]["functionResponse"]["name"], "get_weather");
}

// ── Response: protocol errors in a 2xx ─────────────────────────────────────

#[test]
fn a_2xx_that_is_not_json_is_a_protocol_error() {
    assert_protocol(
        GeminiReferenceV1.parse_response(&raw_response("not json")),
        "invalid JSON in a 2xx",
    );
    assert!(parse(&body_with_usage(&usage(3, 1))).is_ok());
}

#[test]
fn an_error_embedded_in_a_2xx_is_a_protocol_error() {
    let mut embedded = body_with_usage(&usage(3, 1));
    embedded["error"] = json!({"code": 500, "message": "internal"});
    assert_protocol(parse(&embedded), "embedded an error");
    // An explicit null is no error.
    let mut null = body_with_usage(&usage(3, 1));
    null["error"] = Value::Null;
    assert!(parse(&null).is_ok());
}

#[test]
fn a_2xx_without_candidates_is_a_protocol_error() {
    for candidates in [None, Some(json!([])), Some(json!({}))] {
        let mut body = json!({"usageMetadata": usage(3, 0)});
        if let Some(candidates) = candidates {
            body["candidates"] = candidates;
        }
        assert_protocol(parse(&body), "has no candidates");
    }
    assert!(parse(&body_with_usage(&usage(3, 1))).is_ok());
}

#[test]
fn a_function_call_whose_signature_is_not_a_non_empty_string_is_a_protocol_error() {
    // Also an error-expecting fixture (`provider.response.a-non-string-call-signature-is-refused`).
    let with_signature = |signature: Value| {
        let mut body = body_with_usage(&usage(3, 1));
        body["candidates"][0]["content"]["parts"] =
            json!([{"functionCall":{"name":"t","args":{}},"thoughtSignature":signature}]);
        body
    };
    for bad in [json!(""), json!(7), json!({"s":"x"})] {
        assert_protocol(parse(&with_signature(bad)), "thoughtSignature is not a non-empty string");
    }
    let signed = parse(&with_signature(json!("sig"))).unwrap();
    assert_eq!(signed.choices[0].message.tool_calls[0].signature.as_deref(), Some("sig"));
    let unsigned = parse(&with_signature(Value::Null)).unwrap();
    assert_eq!(unsigned.choices[0].message.tool_calls[0].signature, None);
}

// ── Response: usage is funds evidence, so a gap is an error, never a zero ──

#[test]
fn a_response_without_usage_metadata_is_a_protocol_error() {
    // Also an error-expecting fixture (`provider.response.missing-usage`).
    for meta in [None, Some(Value::Null), Some(json!("ON_DEMAND"))] {
        let mut body = body_with_usage(&usage(3, 1));
        match meta {
            Some(meta) => body["usageMetadata"] = meta,
            None => {
                body.as_object_mut().unwrap().remove("usageMetadata");
            }
        }
        assert_protocol(parse(&body), "must carry usageMetadata");
    }
    assert_eq!(parse(&body_with_usage(&usage(3, 1))).unwrap().usage.input_tokens, 3);
}

#[test]
fn a_usage_report_without_a_valid_prompt_count_is_a_protocol_error() {
    for prompt in [None, Some(json!(-1)), Some(json!(1.5)), Some(json!("3"))] {
        let mut meta = usage(3, 1);
        match prompt {
            Some(prompt) => meta["promptTokenCount"] = prompt,
            None => {
                meta.as_object_mut().unwrap().remove("promptTokenCount");
            }
        }
        assert_protocol(
            parse(&body_with_usage(&meta)),
            "lacks a valid promptTokenCount or totalTokenCount",
        );
    }
}

#[test]
fn a_usage_report_without_a_valid_total_is_a_protocol_error() {
    for total in [None, Some(json!(-4)), Some(json!("4"))] {
        let mut meta = usage(3, 1);
        match total {
            Some(total) => meta["totalTokenCount"] = total,
            None => {
                meta.as_object_mut().unwrap().remove("totalTokenCount");
            }
        }
        assert_protocol(
            parse(&body_with_usage(&meta)),
            "lacks a valid promptTokenCount or totalTokenCount",
        );
    }
}

#[test]
fn an_optional_count_that_is_not_a_non_negative_integer_is_a_protocol_error() {
    for key in [
        "candidatesTokenCount",
        "thoughtsTokenCount",
        "toolUsePromptTokenCount",
        "cachedContentTokenCount",
    ] {
        for bad in [json!(-1), json!(0.5), json!("0")] {
            let mut meta = usage(3, 1);
            meta[key] = bad;
            assert_protocol(parse(&body_with_usage(&meta)), "invalid token count");
        }
        // A null optional count reads zero, so only `candidatesTokenCount` changes the sum;
        // the neighbour keeps the total closed either way.
        let mut null = usage(3, 0);
        null[key] = Value::Null;
        assert!(parse(&body_with_usage(&null)).is_ok(), "{key}: null reads zero");
    }
}

#[test]
fn a_cached_count_above_the_prompt_count_is_a_protocol_error() {
    let mut over = usage(3, 1);
    over["cachedContentTokenCount"] = json!(4);
    assert_protocol(
        parse(&body_with_usage(&over)),
        "cachedContentTokenCount exceeds promptTokenCount",
    );
    let mut equal = usage(3, 1);
    equal["cachedContentTokenCount"] = json!(3);
    assert_eq!(parse(&body_with_usage(&equal)).unwrap().usage.cache_read_tokens, 3);
}

#[test]
fn a_missing_candidates_count_is_an_error_unless_the_total_closes_without_it() {
    // The wire's JSON mapping omits zero counts, so an absent candidates count is a zero only
    // when the other counts already reach the total.
    let unreported = json!({"promptTokenCount": 3, "totalTokenCount": 4});
    assert_protocol(parse(&body_with_usage(&unreported)), "lacks candidatesTokenCount");
    let omitted_zero = json!({"promptTokenCount": 3, "totalTokenCount": 3});
    assert_eq!(parse(&body_with_usage(&omitted_zero)).unwrap().usage.output_tokens, 0);
}

#[test]
fn a_total_that_does_not_add_up_is_a_protocol_error() {
    let mut off = usage(3, 1);
    off["totalTokenCount"] = json!(5);
    assert_protocol(parse(&body_with_usage(&off)), "totalTokenCount does not equal");
    // The total is prompt + tool-use prompt + candidates + thoughts.
    let full = json!({"promptTokenCount": 3, "toolUsePromptTokenCount": 2,
                      "candidatesTokenCount": 1, "thoughtsTokenCount": 4, "totalTokenCount": 10});
    let usage = parse(&body_with_usage(&full)).unwrap().usage;
    assert_eq!((usage.input_tokens, usage.output_tokens, usage.reasoning_tokens), (5, 5, 4));
}

#[test]
fn counts_whose_sum_overflows_are_a_protocol_error_not_a_wrapped_total() {
    // With a wrapping sum, `u64::MAX + 1` would read 0 and close against a total of 0.
    let overflow = json!({"promptTokenCount": u64::MAX, "toolUsePromptTokenCount": 1,
                          "totalTokenCount": 0});
    assert_protocol(parse(&body_with_usage(&overflow)), "counts overflow");
    let at_the_edge = json!({"promptTokenCount": u64::MAX - 1, "toolUsePromptTokenCount": 1,
                             "totalTokenCount": u64::MAX});
    assert_eq!(parse(&body_with_usage(&at_the_edge)).unwrap().usage.input_tokens, u64::MAX);
}

// ── Stream ─────────────────────────────────────────────────────────────────

fn frame(payload: &Value) -> Vec<u8> {
    format!("data: {payload}\n\n").into_bytes()
}

fn text_chunk(text: &str) -> Value {
    json!({"candidates": [{"content": {"role": "model", "parts": [{"text": text}]}}]})
}

fn terminal_chunk(usage: Option<Value>) -> Value {
    let mut chunk = json!({"candidates": [{"content": {"role": "model", "parts": []},
                                            "finishReason": "STOP"}]});
    if let Some(usage) = usage {
        chunk["usageMetadata"] = usage;
    }
    chunk
}

fn with_meta(mut chunk: Value, meta: Value) -> Value {
    chunk["usageMetadata"] = meta;
    chunk
}

/// Feeds every frame through one parser, then EOF; the first error wins.
fn run(frames: &[Value]) -> Result<Vec<StreamEvent>, ErrorEnvelope> {
    let mut parser = GeminiReferenceV1.stream_parser();
    let mut events = Vec::new();
    for payload in frames {
        events.extend(parser.parse_chunk(&frame(payload))?);
    }
    events.extend(parser.finish()?);
    Ok(events)
}

fn usage_events(events: &[StreamEvent]) -> usize {
    events.iter().filter(|event| matches!(event, StreamEvent::Usage { .. })).count()
}

#[test]
fn a_stream_frame_that_is_not_utf8_is_a_protocol_error() {
    let mut parser = GeminiReferenceV1.stream_parser();
    assert_protocol(parser.parse_chunk(b"data: \xff\xfe\n\n"), "not UTF-8");
    let mut parser = GeminiReferenceV1.stream_parser();
    assert_eq!(parser.parse_chunk(&frame(&text_chunk("hi"))).unwrap().len(), 1);
}

#[test]
fn a_stream_frame_with_invalid_json_is_a_protocol_error() {
    let mut parser = GeminiReferenceV1.stream_parser();
    assert_protocol(
        parser.parse_chunk(b"data: {\"candidates\":\n\n"),
        "stream frame with invalid JSON",
    );
    // A frame with no data line is skipped, not refused.
    let mut parser = GeminiReferenceV1.stream_parser();
    assert!(parser.parse_chunk(b": keep-alive\n\n").unwrap().is_empty());
}

#[test]
fn a_frame_after_the_terminal_chunk_is_a_protocol_error() {
    assert_protocol(
        run(&[terminal_chunk(Some(usage(10, 5))), text_chunk("late")]),
        "after the terminal finishReason",
    );
    let events = run(&[text_chunk("hi"), terminal_chunk(Some(usage(10, 5)))]).unwrap();
    assert!(matches!(events.last(), Some(StreamEvent::Done { .. })));
}

#[test]
fn a_finish_reason_that_is_not_a_string_is_a_protocol_error() {
    let with_reason = |reason: Value| {
        let mut chunk = text_chunk("hi");
        chunk["candidates"][0]["finishReason"] = reason;
        chunk
    };
    assert_protocol(run(&[with_reason(json!(1))]), "finishReason is not a string");
    // A null reason is a non-terminal chunk, and the stream goes on.
    let events = run(&[with_reason(Value::Null), terminal_chunk(Some(usage(10, 5)))]).unwrap();
    assert_eq!(usage_events(&events), 1);
}

#[test]
fn a_streamed_function_call_whose_signature_is_not_a_non_empty_string_is_a_protocol_error() {
    let call = |signature: Value| {
        json!({"candidates": [{"content": {"role": "model", "parts": [
            {"functionCall": {"name": "t", "args": {}}, "thoughtSignature": signature}
        ]}}]})
    };
    for bad in [json!(""), json!(false)] {
        assert_protocol(run(&[call(bad)]), "thoughtSignature is not a non-empty string");
    }
    let events = run(&[call(json!("sig"))]).unwrap();
    assert!(format!("{events:?}").contains("signature: Some(\"sig\")"), "{events:?}");
}

#[test]
fn an_intermediate_usage_metadata_that_is_not_an_object_is_a_protocol_error() {
    assert_protocol(
        run(&[with_meta(text_chunk("hi"), json!("ON_DEMAND")), terminal_chunk(Some(usage(10, 5)))]),
        "usageMetadata is not an object",
    );
    // A count-less object (Vertex's `trafficType` chunk) is accepted and never emitted as usage.
    let events = run(&[
        with_meta(text_chunk("hi"), json!({"trafficType": "ON_DEMAND"})),
        terminal_chunk(Some(usage(10, 5))),
    ])
    .unwrap();
    assert_eq!(usage_events(&events), 1);
}

#[test]
fn an_intermediate_count_that_is_not_a_non_negative_integer_is_a_protocol_error() {
    for bad in [json!(-1), json!(1.5), json!("3")] {
        assert_protocol(
            run(&[
                with_meta(text_chunk("hi"), json!({"promptTokenCount": bad})),
                terminal_chunk(Some(usage(10, 5))),
            ]),
            "invalid token count",
        );
    }
    let events = run(&[
        with_meta(text_chunk("hi"), json!({"promptTokenCount": 10})),
        terminal_chunk(Some(usage(10, 5))),
    ])
    .unwrap();
    assert_eq!(usage_events(&events), 1);
}

#[test]
fn a_count_that_decreases_across_chunks_is_a_protocol_error() {
    assert_protocol(
        run(&[with_meta(text_chunk("a"), usage(10, 6)), terminal_chunk(Some(usage(10, 5)))]),
        "decrease across the stream",
    );
    let events =
        run(&[with_meta(text_chunk("a"), usage(10, 5)), terminal_chunk(Some(usage(10, 5)))])
            .unwrap();
    assert_eq!(usage_events(&events), 1, "an equal count is no decrease");
}

#[test]
fn a_terminal_chunk_omitting_a_count_an_earlier_chunk_reported_is_a_protocol_error() {
    // On the terminal chunk an omitted count reads zero, so dropping one an earlier chunk
    // reported is a decrease, not an omission. An intermediate chunk may omit it freely.
    assert_protocol(
        run(&[
            with_meta(text_chunk("a"), json!({"thoughtsTokenCount": 4})),
            terminal_chunk(Some(usage(10, 5))),
        ]),
        "decrease across the stream",
    );
    let events = run(&[
        with_meta(text_chunk("a"), json!({"thoughtsTokenCount": 4})),
        with_meta(text_chunk("b"), json!({"promptTokenCount": 10})),
        terminal_chunk(Some(json!({"promptTokenCount": 10, "candidatesTokenCount": 5,
                                   "thoughtsTokenCount": 4, "totalTokenCount": 19}))),
    ])
    .unwrap();
    assert_eq!(usage_events(&events), 1);
}

#[test]
fn the_terminal_usage_metadata_is_held_to_the_non_streaming_arithmetic() {
    let mut off = usage(10, 5);
    off["totalTokenCount"] = json!(16);
    assert_protocol(run(&[terminal_chunk(Some(off))]), "totalTokenCount does not equal");
    assert_protocol(
        run(&[terminal_chunk(Some(json!({"trafficType": "ON_DEMAND"})))]),
        "lacks a valid promptTokenCount",
    );
    // No usageMetadata at all is no usage event, never a zero, and no error.
    assert_eq!(usage_events(&run(&[terminal_chunk(None)]).unwrap()), 0);
    assert_eq!(usage_events(&run(&[terminal_chunk(Some(usage(10, 5)))]).unwrap()), 1);
}
