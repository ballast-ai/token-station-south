//! Direct tests over the Bedrock Converse reference implementation.
//!
//! # Why a reference-level leg exists at all
//!
//! Gate ②'s fixture packs can only state a **successful** translation: a
//! component that returns `Err` makes the suite fail, so no fixture can say
//! "this input must be refused". Every refusal in a provider component is
//! therefore invisible to the fixture layer.
//!
//! The task world already solved this with per-component reference tests
//! (`kling_task_reference_v2.rs` and its siblings, which assert `is_err()`
//! directly). The provider world had no equivalent — `anthropic_sandbox_parity`
//! and `gemini_sandbox_parity` assert only the happy path. This file is that
//! missing layer for the dialect that needs it most: Converse rejects several
//! shapes with a generic validation error that names nothing, so this component
//! refuses them locally, and a refusal nobody tests is a refusal that can
//! silently become a pass-through.

use serde_json::{Value, json};
use south_component_conformance::{
    ProviderComponentV1, reference_bedrock_converse::BedrockConverseReferenceV1,
};
use token_station_protocol::{ChatRequest, HttpResponseParts, ProviderConfig};

const MODEL: &str = "anthropic.claude-sonnet-4-v1:0";
const BASE: &str = "https://bedrock-runtime.us-east-1.amazonaws.com";

fn config() -> ProviderConfig {
    serde_json::from_value(json!({"provider":"bedrock","base_url":BASE,"models":[]})).unwrap()
}

fn request(value: Value) -> ChatRequest {
    serde_json::from_value(value).unwrap()
}

fn turn(messages: &Value) -> ChatRequest {
    request(json!({"model": MODEL, "messages": messages}))
}

fn response(status: u16, body: &Value) -> HttpResponseParts {
    serde_json::from_value(json!({"status":status,"headers":{},"body":body.to_string()})).unwrap()
}

fn built(request: &ChatRequest) -> Value {
    BedrockConverseReferenceV1.build_http_request(request, &config()).unwrap().body.unwrap()
}

// ── The URL, and the one structural difference from every other provider ────

#[test]
fn the_model_sits_in_the_path_and_the_descriptor_carries_no_credential() {
    let descriptor = BedrockConverseReferenceV1
        .build_http_request(&turn(&json!([{"role":"user","content":"hi"}])), &config())
        .unwrap();
    assert_eq!(descriptor.url, format!("{BASE}/model/{MODEL}/converse"));
    // The whole point of the `host_signed` arm: the host's finalizer signs the
    // finished request, so a credential never reaches the component and the
    // descriptor names no auth slot.
    assert!(descriptor.auth.is_none(), "a host_signed component must not name an auth slot");
}

#[test]
fn a_request_without_a_model_is_refused_because_the_path_needs_one() {
    let mut request = turn(&json!([{"role":"user","content":"hi"}]));
    request.model = String::new();
    assert!(BedrockConverseReferenceV1.build_http_request(&request, &config()).is_err());
}

#[test]
fn another_dialects_config_is_refused() {
    let foreign: ProviderConfig =
        serde_json::from_value(json!({"provider":"anthropic","base_url":BASE,"models":[]}))
            .unwrap();
    assert!(
        BedrockConverseReferenceV1
            .build_http_request(&turn(&json!([{"role":"user","content":"hi"}])), &foreign)
            .is_err()
    );
}

#[test]
fn streaming_is_a_different_last_path_segment_not_a_body_field() {
    let mut request = turn(&json!([{"role":"user","content":"hi"}]));
    request.stream = true;
    let descriptor = BedrockConverseReferenceV1.build_http_request(&request, &config()).unwrap();
    assert_eq!(descriptor.url, format!("{BASE}/model/{MODEL}/converse-stream"));
    // Nothing in the body says "stream": the operation is the URL's last
    // segment, the same shape Gemini uses.
    let body = descriptor.body.unwrap();
    assert!(body.get("stream").is_none(), "Converse has no `stream` body field");
}

// ── The trap that silently changes meaning ─────────────────────────────────

#[test]
fn tool_choice_none_withholds_the_whole_tool_config() {
    let body = built(&request(json!({
        "model": MODEL,
        "messages": [{"role":"user","content":"hi"}],
        "tools": [{"name":"t","parameters":{"type":"object"}}],
        "tool_choice": "none",
    })));
    // Converse has no `toolChoice: none`, and an absent `toolChoice` means
    // `auto`. Sending `tools` while dropping only `toolChoice` would upgrade
    // "do not use tools" into "use them if you like".
    assert!(
        body.get("toolConfig").is_none(),
        "tool_choice=none must withhold toolConfig entirely, not just its toolChoice"
    );
}

#[test]
fn required_becomes_any_and_auto_is_written_explicitly() {
    let required = built(&request(json!({
        "model": MODEL,
        "messages": [{"role":"user","content":"hi"}],
        "tools": [{"name":"t","parameters":{"type":"object"}}],
        "tool_choice": "required",
    })));
    assert_eq!(required["toolConfig"]["toolChoice"], json!({"any":{}}));
    let auto = built(&request(json!({
        "model": MODEL,
        "messages": [{"role":"user","content":"hi"}],
        "tools": [{"name":"t","parameters":{"type":"object"}}],
        "tool_choice": "auto",
    })));
    assert_eq!(auto["toolConfig"]["toolChoice"], json!({"auto":{}}));
    // The schema key is `inputSchema.json`, not `parameters`.
    assert_eq!(auto["toolConfig"]["tools"][0]["toolSpec"]["inputSchema"]["json"]["type"], "object");
}

#[test]
fn the_openai_object_form_of_tool_choice_is_translated_not_forwarded() {
    // A client naming one tool sends OpenAI's object form. Converse spells the
    // same intent differently, and a host-side contract check refuses anything
    // that is not `{auto:{}}` / `{any:{}}` / `{tool:{name}}` — so forwarding the
    // caller's shape verbatim reaches the upstream as an undefined toolChoice.
    let body = built(&request(json!({
        "model": MODEL,
        "messages": [{"role":"user","content":"hi"}],
        "tools": [{"name":"lookup_weather","parameters":{"type":"object"}}],
        "tool_choice": {"type":"function","function":{"name":"lookup_weather"}},
    })));
    assert_eq!(
        body["toolConfig"]["toolChoice"],
        json!({"tool": {"name": "lookup_weather"}}),
        "OpenAI 的对象形必须翻成 Converse 的具名形：{body:#}"
    );
}

#[test]
fn an_unreadable_tool_choice_object_is_omitted_rather_than_forwarded() {
    // No `function.name` to read. Omitting means `auto`, which is where an
    // unreadable choice would have to land anyway — and it keeps the body legal.
    let body = built(&request(json!({
        "model": MODEL,
        "messages": [{"role":"user","content":"hi"}],
        "tools": [{"name":"t","parameters":{"type":"object"}}],
        "tool_choice": {"type":"something_else"},
    })));
    assert!(
        body["toolConfig"].get("toolChoice").is_none(),
        "读不出名字的 choice 应当整个不发，而不是原样透传：{body:#}"
    );
}

// ── Tool-call and tool-result shapes Bedrock rejects unhelpfully ───────────

#[test]
fn a_tool_result_must_answer_a_call_announced_earlier() {
    // An id that names nothing can never match a prior toolUse, and the
    // upstream's refusal does not say which id was wrong.
    assert!(
        BedrockConverseReferenceV1
            .build_http_request(
                &turn(&json!([
                    {"role":"user","content":"hi"},
                    {"role":"tool","tool_call_id":"never-announced","content":"12C"},
                ])),
                &config()
            )
            .is_err()
    );
}

#[test]
fn an_empty_tool_use_id_or_name_is_refused() {
    for call in [
        json!({"id":"","name":"t","arguments":"{}"}),
        json!({"id":"tu_1","name":"","arguments":"{}"}),
    ] {
        assert!(
            BedrockConverseReferenceV1
                .build_http_request(
                    &turn(&json!([{"role":"assistant","tool_calls":[call]}])),
                    &config()
                )
                .is_err()
        );
    }
}

#[test]
fn tool_arguments_that_are_not_json_are_refused_rather_than_emptied() {
    // IR keeps `arguments` as a string because providers stream it in
    // fragments; Converse wants an object. Substituting `{}` would send a call
    // the model did not ask for.
    assert!(
        BedrockConverseReferenceV1
            .build_http_request(
                &turn(&json!([
                    {"role":"assistant","tool_calls":[{"id":"tu_1","name":"t","arguments":"{\"a\":"}]}
                ])),
                &config()
            )
            .is_err()
    );
}

#[test]
fn an_assistant_turn_with_nothing_in_it_is_dropped_not_sent_empty() {
    // Bedrock 400s on an empty assistant content array.
    let body = built(&turn(&json!([
        {"role":"user","content":"hi"},
        {"role":"assistant"},
    ])));
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1, "the empty assistant turn must not be sent at all");
    assert_eq!(messages[0]["role"], "user");
}

#[test]
fn reasoning_parts_are_dropped_on_the_way_out_and_never_forwarded_raw() {
    // Converse has no request-side slot for a replayed reasoning block, and an
    // unknown part has none by definition. Dropping is the decision; this test
    // exists because a *silent* decision is one a later change can reverse
    // without anything going red — and forwarding either shape would send
    // Converse a content block it does not define.
    let body = built(&turn(&json!([{"role":"user","content":[
        {"type":"text","text":"hi"},
        {"type":"thinking","thinking":"secret reasoning","signature":"sig"},
        {"type":"redacted_thinking","data":"opaque"},
        {"type":"some_future_part","whatever":1},
    ]}])));
    let blocks = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(blocks.len(), 1, "只有 text 该活下来，其余三臂全丢：{body:#}");
    assert_eq!(blocks[0], json!({"text": "hi"}));
    let rendered = body.to_string();
    for leaked in ["secret reasoning", "opaque", "some_future_part"] {
        assert!(
            !rendered.contains(leaked),
            "被丢弃的部件不得以任何形式出现在请求体里：`{leaked}` 在 {body:#}"
        );
    }
}

// ── Images: inline bytes only ──────────────────────────────────────────────

#[test]
fn an_http_image_url_is_refused_because_converse_has_no_slot_for_one() {
    assert!(
        BedrockConverseReferenceV1
            .build_http_request(
                &turn(&json!([{"role":"user","content":[
                    {"type":"image_url","image_url":{"url":"https://example.test/cat.png"}}
                ]}])),
                &config()
            )
            .is_err()
    );
}

#[test]
fn an_unsupported_image_media_type_is_refused_locally() {
    assert!(
        BedrockConverseReferenceV1
            .build_http_request(
                &turn(&json!([{"role":"user","content":[
                    {"type":"image_url","image_url":{"url":"data:image/tiff;base64,AAAA"}}
                ]}])),
                &config()
            )
            .is_err()
    );
}

#[test]
fn a_data_url_passes_its_base64_through_untouched() {
    let body = built(&turn(&json!([{"role":"user","content":[
        {"type":"image_url","image_url":{"url":"data:image/png;base64,QUJD"}}
    ]}])));
    let image = &body["messages"][0]["content"][0]["image"];
    assert_eq!(image["format"], "png");
    // Verbatim: this component never decodes base64, which is why it needs no
    // base64 dependency.
    assert_eq!(image["source"]["bytes"], "QUJD");
}

// ── Usage is billing input, so a partial report is an error, not a zero ────

#[test]
fn a_usage_report_missing_a_bucket_is_a_protocol_error() {
    let parts = response(
        200,
        &json!({
            "output": {"message": {"content": [{"text": "x"}]}},
            "stopReason": "end_turn",
            "usage": {"inputTokens": 5, "outputTokens": 1},
        }),
    );
    assert!(
        BedrockConverseReferenceV1.parse_response(&parts).is_err(),
        "a missing totalTokens must fail rather than default to zero — billing reads this"
    );
}

#[test]
fn a_usage_total_that_does_not_add_up_is_a_protocol_error() {
    let parts = response(
        200,
        &json!({
            "output": {"message": {"content": [{"text": "x"}]}},
            "stopReason": "end_turn",
            "usage": {"inputTokens": 5, "outputTokens": 1, "totalTokens": 99},
        }),
    );
    assert!(BedrockConverseReferenceV1.parse_response(&parts).is_err());
}

#[test]
fn a_body_without_the_output_message_content_is_a_protocol_error() {
    let parts = response(200, &json!({"stopReason": "end_turn", "usage": {}}));
    assert!(BedrockConverseReferenceV1.parse_response(&parts).is_err());
}

#[test]
fn a_2xx_that_is_not_json_is_a_protocol_error() {
    let parts: HttpResponseParts =
        serde_json::from_value(json!({"status":200,"headers":{},"body":"not json"})).unwrap();
    assert!(BedrockConverseReferenceV1.parse_response(&parts).is_err());
}

// ── Errors the host must not retry as if they were transient ───────────────

#[test]
fn an_access_denied_exception_is_an_auth_failure_whatever_the_status() {
    let parts = response(400, &json!({"__type":"AccessDeniedException","message":"nope"}));
    let envelope = BedrockConverseReferenceV1.map_provider_error(&parts).unwrap();
    assert_eq!(
        format!("{:?}", envelope.code),
        "Auth",
        "the exception name decides, not the status: a 400 AccessDeniedException is still auth"
    );
}

#[test]
fn the_exception_name_is_read_from_the_header_too() {
    // Bedrock names the exception in a header on some paths and in the body's
    // `__type` on others.
    let parts: HttpResponseParts = serde_json::from_value(json!({
        "status": 429,
        "headers": {"x-amzn-errortype": "ThrottlingException:http://internal.amazon.com/coral/"},
        "body": "{}",
    }))
    .unwrap();
    let envelope = BedrockConverseReferenceV1.map_provider_error(&parts).unwrap();
    assert_eq!(format!("{:?}", envelope.code), "RateLimit");
}

// ── Streaming: the two-phase ending is the whole point ─────────────────────

/// One decoded Converse event, re-encoded the way the host's seam does it.
fn frame(event: &str, data: &Value) -> Vec<u8> {
    format!("event: {event}\ndata: {data}\n\n").into_bytes()
}

#[test]
fn done_waits_for_metadata_because_message_stop_has_no_usage_yet() {
    let mut parser = BedrockConverseReferenceV1.stream_parser();
    assert!(
        parser
            .parse_chunk(&frame("messageStart", &json!({"role":"assistant"})))
            .unwrap()
            .is_empty()
    );
    let deltas = parser
        .parse_chunk(&frame(
            "contentBlockDelta",
            &json!({"contentBlockIndex":0,"delta":{"text":"Mild."}}),
        ))
        .unwrap();
    assert_eq!(format!("{deltas:?}"), r#"[Delta { index: 0, content: "Mild." }]"#);

    // messageStop knows the reason but not the counts, so it may only Finish.
    let stop =
        parser.parse_chunk(&frame("messageStop", &json!({"stopReason":"end_turn"}))).unwrap();
    assert_eq!(stop.len(), 1, "messageStop must emit Finish alone, never Done");
    assert!(
        format!("{stop:?}").starts_with("[Finish"),
        "announcing Done here would claim a complete exchange before usage arrived: {stop:?}"
    );

    // metadata carries the counts and closes the stream: Usage, then Done.
    let end = parser
        .parse_chunk(&frame(
            "metadata",
            &json!({"usage":{"inputTokens":10,"outputTokens":2,"totalTokens":12}}),
        ))
        .unwrap();
    assert_eq!(end.len(), 2);
    assert!(format!("{:?}", end[0]).starts_with("Usage"));
    // The finish reason messageStop announced rides out on Done.
    assert!(format!("{:?}", end[1]).contains("Stop"), "{:?}", end[1]);
    // EOF after a closed stream is clean.
    assert!(parser.finish().unwrap().is_empty());
}

#[test]
fn a_stream_cut_before_metadata_emits_no_done_at_all() {
    let mut parser = BedrockConverseReferenceV1.stream_parser();
    parser.parse_chunk(&frame("messageStop", &json!({"stopReason":"end_turn"}))).unwrap();
    // The reason arrived, the counts never did. The truncation is reported by
    // the **absence** of a terminal event, which is the contract's own signal
    // and what the host settles on — not by an error, because an empty fragment
    // is not reliably a real EOF (the suite's incrementality check produces one
    // mid-stream).
    let tail = parser.finish().unwrap();
    assert!(
        tail.is_empty(),
        "no Done may be emitted without usage, so a cut stream cannot settle as complete: {tail:?}"
    );
}

#[test]
fn a_tool_call_names_itself_on_its_first_fragment_only() {
    let mut parser = BedrockConverseReferenceV1.stream_parser();
    let start = parser
        .parse_chunk(&frame(
            "contentBlockStart",
            &json!({"contentBlockIndex":1,"start":{"toolUse":{"toolUseId":"tu_1","name":"get_weather"}}}),
        ))
        .unwrap();
    let rendered = format!("{start:?}");
    assert!(rendered.contains("tu_1") && rendered.contains("get_weather"), "{rendered}");
    // Converse's delta carries only `input`, and IR wants id/name absent after
    // the first fragment — so no lookup table is needed on either side.
    let delta = parser
        .parse_chunk(&frame(
            "contentBlockDelta",
            &json!({"contentBlockIndex":1,"delta":{"toolUse":{"input":"{\"city\":"}}}),
        ))
        .unwrap();
    let rendered = format!("{delta:?}");
    assert!(rendered.contains("id: None") && rendered.contains("name: None"), "{rendered}");
    assert!(rendered.contains(r#"{\"city\":"#), "{rendered}");
}

#[test]
fn a_frame_split_across_chunks_is_buffered_until_it_closes() {
    let mut parser = BedrockConverseReferenceV1.stream_parser();
    let whole = frame("contentBlockDelta", &json!({"contentBlockIndex":0,"delta":{"text":"hi"}}));
    let (head, tail) = whole.split_at(whole.len() / 2);
    assert!(parser.parse_chunk(head).unwrap().is_empty(), "half a frame completes nothing");
    let events = parser.parse_chunk(tail).unwrap();
    assert_eq!(format!("{events:?}"), r#"[Delta { index: 0, content: "hi" }]"#);
}

#[test]
fn a_streamed_usage_report_is_held_to_the_same_arithmetic() {
    let mut parser = BedrockConverseReferenceV1.stream_parser();
    assert!(
        parser
            .parse_chunk(&frame(
                "metadata",
                &json!({"usage":{"inputTokens":10,"outputTokens":2,"totalTokens":99}}),
            ))
            .is_err(),
        "a total that does not add up is a protocol error in the stream too"
    );
}

#[test]
fn an_event_this_dialect_has_not_got_yet_is_ignored_not_fatal() {
    let mut parser = BedrockConverseReferenceV1.stream_parser();
    assert!(parser.parse_chunk(&frame("trace", &json!({"whatever":1}))).unwrap().is_empty());
}
