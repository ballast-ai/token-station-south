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
fn streaming_is_refused_before_the_request_is_sent() {
    // The response half cannot decode eventstream frames yet, so opening the
    // stream would produce a request whose answer is unreadable. Refusing here
    // beats a stream that opens and then dies.
    let mut request = turn(&json!([{"role":"user","content":"hi"}]));
    request.stream = true;
    assert!(BedrockConverseReferenceV1.build_http_request(&request, &config()).is_err());
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
