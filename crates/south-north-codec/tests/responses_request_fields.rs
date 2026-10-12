//! The four Responses request fields the IR has no typed slot for — `user`, `metadata`,
//! `prompt_cache_key` and `truncation` — carried as request extensions under the names the Chat
//! codec already writes (design record `2026-10-12-responses-r3-codex-and-request-fields.md` §4.3,
//! RQ3-Q3, RQ3-Q4).

use serde_json::{Value, json};
use south_north_codec::responses::{
    RESPONSES_METADATA_KEY_MAX_CHARS, RESPONSES_METADATA_MAX_ENTRIES,
    RESPONSES_METADATA_VALUE_MAX_CHARS, RESPONSES_REQUEST_FIELD_MAX_BYTES, ResponsesRequestOptions,
    chat_request_from_responses,
};
use south_north_codec::{CodecError, chat_request_from_openai_chat};

fn parse(body: &Value) -> Result<token_station_protocol::ChatRequest, CodecError> {
    chat_request_from_responses(body, &ResponsesRequestOptions::default())
}

fn with(patch: &Value) -> Value {
    let mut body = json!({"model": "gpt-5.5", "input": "hello"});
    body.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
    body
}

#[test]
fn the_four_fields_reach_the_ir_as_request_extensions() {
    let metadata = json!({"tenant": "acme", "trace": "t-1"});
    let request = parse(&with(&json!({
        "user": "user-42",
        "metadata": metadata,
        "prompt_cache_key": "conversation-7",
        "truncation": "auto",
    })))
    .unwrap();
    assert_eq!(request.extensions["user"], json!("user-42"));
    assert_eq!(request.extensions["metadata"], metadata);
    assert_eq!(request.extensions["prompt_cache_key"], json!("conversation-7"));
    assert_eq!(request.extensions["truncation"], json!("auto"));
    let request = parse(&with(&json!({"truncation": "disabled"}))).unwrap();
    assert_eq!(request.extensions["truncation"], json!("disabled"));
}

/// RQ3-Q4: one vocabulary for both north surfaces. The Chat codec copies these three keys
/// verbatim; the Responses codec writes the same names.
#[test]
fn the_keys_are_the_ones_the_chat_codec_writes() {
    let fields = json!({"user": "user-42", "metadata": {"a": "b"}, "prompt_cache_key": "k"});
    let mut chat = json!({"model": "gpt-5.5", "messages": [{"role": "user", "content": "hello"}]});
    chat.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
    let from_chat = chat_request_from_openai_chat(&chat).unwrap();
    let from_responses = parse(&with(&fields)).unwrap();
    for key in ["user", "metadata", "prompt_cache_key"] {
        assert_eq!(from_chat.extensions.get(key), from_responses.extensions.get(key), "{key}");
        assert!(from_responses.extensions.contains_key(key), "{key}");
    }
}

#[test]
fn null_is_the_same_as_absent() {
    let absent = parse(&with(&json!({}))).unwrap();
    let null = parse(&with(&json!({
        "user": null, "metadata": null, "prompt_cache_key": null, "truncation": null,
    })))
    .unwrap();
    assert_eq!(null, absent);
    for key in ["user", "metadata", "prompt_cache_key", "truncation"] {
        assert!(!null.extensions.contains_key(key), "{key}");
    }
}

#[test]
fn the_bounds_are_inclusive() {
    let longest = "u".repeat(RESPONSES_REQUEST_FIELD_MAX_BYTES);
    let metadata: serde_json::Map<String, Value> = (0..RESPONSES_METADATA_MAX_ENTRIES)
        .map(|index| {
            let key = format!("{index:0>RESPONSES_METADATA_KEY_MAX_CHARS$}");
            (key, json!("v".repeat(RESPONSES_METADATA_VALUE_MAX_CHARS)))
        })
        .collect();
    let request = parse(&with(&json!({
        "user": longest, "prompt_cache_key": longest, "metadata": metadata,
    })))
    .unwrap();
    assert_eq!(request.extensions["metadata"].as_object().unwrap().len(), 16);
    // Characters, not bytes, for metadata: 512 three-byte characters are still in bounds.
    let wide = json!({"k": "\u{4e00}".repeat(RESPONSES_METADATA_VALUE_MAX_CHARS)});
    assert!(parse(&with(&json!({"metadata": wide}))).is_ok());
    assert!(parse(&with(&json!({"metadata": {}}))).is_ok());
}

#[test]
fn each_bound_is_refused_with_its_field_and_no_payload() {
    let too_long = "s".repeat(RESPONSES_REQUEST_FIELD_MAX_BYTES + 1);
    // Bytes, not characters, for `user` and `prompt_cache_key`: 171 three-byte characters are
    // 513 bytes.
    let wide = "\u{4e00}".repeat(171);
    let many: serde_json::Map<String, Value> = (0..=RESPONSES_METADATA_MAX_ENTRIES)
        .map(|index| (format!("k{index}"), json!("secret")))
        .collect();
    let long_key = "k".repeat(RESPONSES_METADATA_KEY_MAX_CHARS + 1);
    for (patch, field) in [
        (json!({"user": too_long}), "user"),
        (json!({"user": wide}), "user"),
        (json!({"user": 42}), "user"),
        (json!({"prompt_cache_key": too_long}), "prompt_cache_key"),
        (json!({"prompt_cache_key": wide}), "prompt_cache_key"),
        (json!({"prompt_cache_key": ["secret"]}), "prompt_cache_key"),
        (json!({"metadata": "secret"}), "metadata"),
        (json!({"metadata": ["secret"]}), "metadata"),
        (json!({"metadata": many}), "metadata"),
        (json!({"metadata": {long_key: "secret"}}), "metadata"),
        (
            json!({"metadata": {"k": "s".repeat(RESPONSES_METADATA_VALUE_MAX_CHARS + 1)}}),
            "metadata",
        ),
        (json!({"metadata": {"k": 7}}), "metadata"),
        (json!({"metadata": {"k": null}}), "metadata"),
        (json!({"truncation": "secret"}), "truncation"),
        (json!({"truncation": "Auto"}), "truncation"),
        (json!({"truncation": true}), "truncation"),
    ] {
        let error = parse(&with(&patch)).unwrap_err();
        assert_eq!(error.field(), field, "{patch}");
        assert!(!error.to_string().contains("secret"), "{error}");
    }
}

/// A request without the four fields parses to the same IR, byte for byte, as before the change.
/// The literal was taken from the codec at `origin/main` 7ef9995, before these fields existed, and
/// changed once since: protocol 0.6.0 (KC1, R-Q15) types `reasoning_effort`,
/// `parallel_tool_calls` and the per-tool `strict`, which moves the first two out of the sorted
/// `extensions` tail to their struct positions and adds `strict` to the tool.
#[test]
fn a_request_without_them_is_unchanged_byte_for_byte() {
    let body = json!({
        "model": "gpt-5.5", "instructions": "Be brief.",
        "input": [{"role": "user", "content": "Weather in Paris?"}],
        "tools": [{"type": "function", "name": "get_weather", "strict": true,
                   "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}}],
        "tool_choice": "auto", "parallel_tool_calls": false,
        "reasoning": {"effort": "low", "summary": "auto"},
        "text": {"format": {"type": "json_object"}},
        "max_output_tokens": 64, "temperature": 0.5, "top_p": 0.9, "stream": true,
        "store": true, "previous_response_id": "resp_1", "service_tier": "flex",
    });
    let ir = serde_json::to_string(&parse(&body).unwrap()).unwrap();
    assert_eq!(ir, GOLDEN_IR);
}

const GOLDEN_IR: &str = concat!(
    r#"{"model":"gpt-5.5","messages":[{"role":"system","content":"Be brief.","responses_transient"#,
    r#"_instructions":true},{"role":"user","content":"Weather in Paris?"}],"tools":[{"name":"get_"#,
    r#"weather","parameters":{"properties":{"city":{"type":"string"}},"type":"object"},"strict":"#,
    r#"true}],"response_format":{"type":"json_object"},"tool_choice":"auto","sampling":{"tempera"#,
    r#"ture":0.5,"top_p":0.9,"max_output_tokens":64},"reasoning_effort":"low","parallel_tool_cal"#,
    r#"ls":false,"stream":true,"responses_reasoning_summary":"auto","responses_tool_strict":{"get"#,
    r#"_weather":true}}"#,
);
