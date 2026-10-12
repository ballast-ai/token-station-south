//! What protocol 0.6.0 (canonical IR 4) lets the north codecs keep: prompt-cache markers, `top_k`,
//! the caller's reasoning configuration, tool strictness and parallel tool calls, and the IR keys a
//! client must not be able to write (KC1 record `docs/design/2026-10-12-kc1-canonical-ir-4-south.md`
//! §3; owner rulings K2 to K6 and KC-Q1 to KC-Q6).

use serde_json::{Value, json};
use south_north_codec::{
    CodecError, chat_request_from_anthropic_messages, chat_request_from_openai_chat,
};
use token_station_protocol::{
    CacheControl, CacheTtl, ChatRequest, Content, ContentPart, ReasoningEffort, ReasoningMode, Role,
};

const DEFAULT_TTL: CacheControl = CacheControl { ttl: None };
const ONE_HOUR: CacheControl = CacheControl { ttl: Some(CacheTtl::OneHour) };

fn messages(extra: &Value) -> Result<ChatRequest, CodecError> {
    let mut body =
        json!({"model": "m", "max_tokens": 64, "messages": [{"role": "user", "content": "hi"}]});
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    chat_request_from_anthropic_messages(&body)
}

fn chat(extra: &Value) -> Result<ChatRequest, CodecError> {
    let mut body = json!({"model": "m", "messages": [{"role": "user", "content": "hi"}]});
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    chat_request_from_openai_chat(&body)
}

fn parts(content: Option<&Content>) -> &[ContentPart] {
    match content {
        Some(Content::Parts(parts)) => parts,
        other => panic!("expected parts, got {other:?}"),
    }
}

// ── Anthropic Messages ─────────────────────────────────────────────────────

/// K2: a marked system prompt keeps one part per block, each with its own marker; an unmarked one
/// is the joined string it always was, so no unmarked request changes.
#[test]
fn marked_system_blocks_stay_parts_and_unmarked_ones_stay_a_string() {
    let marked = messages(&json!({"system": [
        {"type": "text", "text": "rules"},
        {"type": "text", "text": "long context", "cache_control": {"type": "ephemeral", "ttl": "1h"}}
    ]}))
    .unwrap();
    assert_eq!(marked.messages[0].role, Role::System);
    assert_eq!(
        parts(marked.messages[0].content.as_ref()),
        [
            ContentPart::text("rules"),
            ContentPart::Text { text: "long context".to_owned(), cache_control: Some(ONE_HOUR) },
        ]
    );

    let unmarked = messages(&json!({"system": [
        {"type": "text", "text": "rules"}, {"type": "text", "text": "more"}
    ]}))
    .unwrap();
    assert_eq!(unmarked.messages[0].content, Some(Content::Text("rules\nmore".to_owned())));
    let string = messages(&json!({"system": "rules"})).unwrap();
    assert_eq!(string.messages[0].content, Some(Content::Text("rules".to_owned())));
}

#[test]
fn content_block_markers_reach_the_ir_and_an_unmodelled_block_keeps_its_marker_verbatim() {
    let request = messages(&json!({"messages": [{"role": "user", "content": [
        {"type": "text", "text": "a", "cache_control": {"type": "ephemeral"}},
        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"},
         "cache_control": {"type": "ephemeral", "ttl": "5m"}},
        {"type": "document", "source": {"type": "text", "media_type": "text/plain", "data": "d"},
         "cache_control": {"type": "ephemeral"}}
    ]}]}))
    .unwrap();
    let parts = parts(request.messages[0].content.as_ref());
    assert_eq!(
        parts[0],
        ContentPart::Text { text: "a".to_owned(), cache_control: Some(DEFAULT_TTL) }
    );
    assert_eq!(parts[1].cache_control(), Some(CacheControl { ttl: Some(CacheTtl::FiveMinutes) }));
    let ContentPart::Unknown(document) = &parts[2] else { panic!("{:?}", parts[2]) };
    assert_eq!(document["cache_control"], json!({"type": "ephemeral"}));
}

#[test]
fn tool_markers_and_strictness_are_typed() {
    let request = messages(&json!({"tools": [
        {"name": "a", "input_schema": {"type": "object"}, "strict": true},
        {"name": "b", "input_schema": {"type": "object"},
         "cache_control": {"type": "ephemeral", "ttl": "1h"}}
    ]}))
    .unwrap();
    assert_eq!(request.tools[0].strict, Some(true));
    assert_eq!(request.tools[0].cache_control, None);
    assert_eq!(request.tools[1].strict, None);
    assert_eq!(request.tools[1].cache_control, Some(ONE_HOUR));

    let error = messages(&json!({"tools": [{"name": "a", "input_schema": {}, "strict": "yes"}]}))
        .unwrap_err();
    assert_eq!(error.field(), "tools[0].strict");
}

/// K2: a cached tool result is a tool message with parts and the marker on the last part; an
/// unmarked one keeps today's flattened text.
#[test]
fn a_marked_tool_result_becomes_parts_with_the_marker_on_the_last_part() {
    let conversation = |result: Value| {
        messages(&json!({"messages": [
            {"role": "user", "content": "weather?"},
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "t1", "name": "w", "input": {}}
            ]},
            {"role": "user", "content": [result]}
        ]}))
        .unwrap()
    };
    let string =
        conversation(json!({"type": "tool_result", "tool_use_id": "t1", "content": "sunny",
                                     "cache_control": {"type": "ephemeral", "ttl": "1h"}}));
    assert_eq!(string.messages[2].role, Role::Tool);
    assert_eq!(
        parts(string.messages[2].content.as_ref()),
        [ContentPart::Text { text: "sunny".to_owned(), cache_control: Some(ONE_HOUR) }]
    );

    let blocks = conversation(json!({"type": "tool_result", "tool_use_id": "t1",
        "content": [{"type": "text", "text": "a"},
                    {"type": "text", "text": "b", "cache_control": {"type": "ephemeral"}}]}));
    assert_eq!(
        parts(blocks.messages[2].content.as_ref()),
        [
            ContentPart::text("a"),
            ContentPart::Text { text: "b".to_owned(), cache_control: Some(DEFAULT_TTL) }
        ]
    );

    let plain = conversation(json!({"type": "tool_result", "tool_use_id": "t1",
        "content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]}));
    assert_eq!(plain.messages[2].content, Some(Content::Text("a\nb".to_owned())));
}

#[test]
fn top_k_is_typed_and_an_unrepresentable_one_is_reported() {
    assert_eq!(messages(&json!({"top_k": 40})).unwrap().sampling.top_k, Some(40));
    assert_eq!(messages(&json!({"top_k": null})).unwrap().sampling.top_k, None);
    for bad in [json!(-1), json!(1.5), json!("40"), json!(u64::from(u32::MAX) + 1)] {
        let error = messages(&json!({"top_k": bad})).unwrap_err();
        assert_eq!(error.field(), "top_k", "{bad}");
        assert_eq!(error.stable_code(), "out_of_range", "{bad}");
    }
}

/// K5: the caller's `thinking` and `output_config.effort` reach the IR unconverted.
#[test]
fn thinking_and_effort_are_carried_without_conversion() {
    let enabled =
        messages(&json!({"thinking": {"type": "enabled", "budget_tokens": 3000}})).unwrap();
    assert_eq!(enabled.reasoning.mode, Some(ReasoningMode::Enabled));
    assert_eq!(enabled.reasoning.budget_tokens, Some(3000));
    assert_eq!(enabled.reasoning.effort, None);

    let adaptive = messages(&json!({
        "thinking": {"type": "adaptive"}, "output_config": {"effort": "max"}
    }))
    .unwrap();
    assert_eq!(adaptive.reasoning.mode, Some(ReasoningMode::Adaptive));
    assert_eq!(adaptive.reasoning.effort, Some(ReasoningEffort::Other("max".to_owned())));
    assert_eq!(adaptive.reasoning.budget_tokens, None);
    // The rest of `output_config` is data; the effort is not converted or removed there.
    assert_eq!(adaptive.extensions["output_config"], json!({"effort": "max"}));
    assert!(!adaptive.extensions.contains_key("thinking"));

    let disabled = messages(&json!({"thinking": {"type": "disabled"}})).unwrap();
    assert_eq!(disabled.reasoning.mode, Some(ReasoningMode::Disabled));

    for (thinking, field) in [
        (json!("on"), "thinking"),
        (json!({}), "thinking.type"),
        (json!({"type": "between_tools"}), "thinking.type"),
        (json!({"type": "enabled", "budget_tokens": -5}), "thinking.budget_tokens"),
        (json!({"type": "enabled", "budget_tokens": 5_000_000_000_u64}), "thinking.budget_tokens"),
    ] {
        let error = messages(&json!({"thinking": thinking})).unwrap_err();
        assert_eq!(error.field(), field, "{thinking}");
    }
    let error = messages(&json!({"output_config": {"effort": 3}})).unwrap_err();
    assert_eq!(error.field(), "output_config.effort");
}

/// KC-Q6: `disable_parallel_tool_use` is the typed `parallel_tool_calls`, negated.
#[test]
fn disable_parallel_tool_use_becomes_parallel_tool_calls() {
    let tools = json!([{"name": "a", "input_schema": {"type": "object"}}]);
    let off = messages(&json!({"tools": tools,
        "tool_choice": {"type": "auto", "disable_parallel_tool_use": true}}))
    .unwrap();
    assert_eq!(off.parallel_tool_calls, Some(false));
    assert_eq!(off.tool_choice, Some(token_station_protocol::ToolChoice::Auto));
    let on = messages(&json!({"tools": tools,
        "tool_choice": {"type": "any", "disable_parallel_tool_use": false}}))
    .unwrap();
    assert_eq!(on.parallel_tool_calls, Some(true));
    let unsaid = messages(&json!({"tools": tools, "tool_choice": {"type": "auto"}})).unwrap();
    assert_eq!(unsaid.parallel_tool_calls, None);
}

/// §3.1 and KC-Q4: unmodelled top-level keys survive as data, as in the Chat decode; the raw
/// `metadata` (the caller's end-user id) never enters the IR, because the host derives the value
/// a component may send.
#[test]
fn unmodelled_keys_go_to_extensions_and_metadata_does_not() {
    let request = messages(&json!({
        "service_tier": "auto", "container": {"id": "c"},
        "metadata": {"user_id": "alice@example.com"},
        "stop_sequences": ["END"], "temperature": 0.5, "top_p": 0.9, "stream": true
    }))
    .unwrap();
    assert_eq!(request.extensions["service_tier"], json!("auto"));
    assert_eq!(request.extensions["container"], json!({"id": "c"}));
    for consumed in [
        "metadata",
        "model",
        "messages",
        "max_tokens",
        "stop_sequences",
        "temperature",
        "top_p",
        "stream",
        "system",
        "tools",
        "tool_choice",
        "thinking",
        "top_k",
    ] {
        assert!(!request.extensions.contains_key(consumed), "{consumed}");
    }
    let wire = serde_json::to_string(&request).unwrap();
    assert!(!wire.contains("alice"), "the raw end-user id must not reach the IR: {wire}");
}

/// KC-Q5: a key that is a canonical IR wire key and not part of this wire would be read as typed
/// IR on the component side (or written twice next to the host's value), so it is refused.
#[test]
fn reserved_ir_keys_are_refused_on_the_messages_surface() {
    for key in [
        "reasoning_mode",
        "reasoning_effort",
        "reasoning_budget_tokens",
        "parallel_tool_calls",
        "sampling",
        "response_format",
        "host_values",
    ] {
        let error = messages(&json!({key: {"end_user_hash": "spoofed"}})).unwrap_err();
        assert_eq!(error.field(), key);
        assert_eq!(error.stable_code(), "unknown_value", "{key}");
    }
}

/// K6: an invalid marker is a 400 at the north boundary wherever it sits, and is never forwarded.
#[test]
fn an_invalid_marker_is_refused_with_its_path() {
    let cases = [
        (
            json!({"system": [{"type": "text", "text": "s", "cache_control": {"ttl": "2h"}}]}),
            "system[0].cache_control.ttl",
        ),
        (
            json!({"messages": [{"role": "user", "content": [
            {"type": "text", "text": "a", "cache_control": {"type": "persistent"}}]}]}),
            "messages[0].content[0].cache_control.type",
        ),
        (
            json!({"messages": [{"role": "user", "content": [
            {"type": "document", "source": {}, "cache_control": {"type": "ephemeral", "scope": "org"}}]}]}),
            "messages[0].content[0].cache_control.scope",
        ),
        (
            json!({"tools": [{"name": "a", "input_schema": {}, "cache_control": "ephemeral"}]}),
            "tools[0].cache_control",
        ),
        (
            json!({"messages": [{"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "t", "content": "x",
             "cache_control": {"type": "ephemeral", "ttl": 3600}}]}]}),
            "messages[0].content[0].cache_control.ttl",
        ),
    ];
    for (extra, field) in cases {
        let error = messages(&extra).unwrap_err();
        assert_eq!(error.field(), field, "{extra}");
        assert_eq!(error.stable_code(), "unknown_value", "{extra}");
    }
}

/// KC-Q1: the top-level automatic-caching marker has no IR field; it travels as data, and an
/// invalid one is refused like any other marker.
#[test]
fn the_top_level_marker_is_checked_and_kept_as_data() {
    let request = messages(&json!({"cache_control": {"type": "ephemeral", "ttl": "1h"}})).unwrap();
    assert_eq!(request.extensions["cache_control"], json!({"type": "ephemeral", "ttl": "1h"}));
    let error =
        messages(&json!({"cache_control": {"type": "ephemeral", "ttl": "1d"}})).unwrap_err();
    assert_eq!(error.field(), "cache_control.ttl");
}

/// §7.1: the IR the codec writes for the new inputs, as JSON, and its exact round trip.
#[test]
fn the_ir_wire_form_of_the_new_fields_is_pinned_and_round_trips() {
    let request = messages(&json!({
        "system": [{"type": "text", "text": "s", "cache_control": {"type": "ephemeral", "ttl": "1h"}}],
        "messages": [{"role": "user", "content": [
            {"type": "text", "text": "q", "cache_control": {"type": "ephemeral"}}]}],
        "tools": [{"name": "f", "input_schema": {"type": "object"}, "strict": true,
                   "cache_control": {"type": "ephemeral"}}],
        "top_k": 5,
        "thinking": {"type": "enabled", "budget_tokens": 2048},
        "tool_choice": {"type": "auto", "disable_parallel_tool_use": true}
    }))
    .unwrap();
    let wire = serde_json::to_value(&request).unwrap();
    assert_eq!(
        wire,
        json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": [
                    {"type": "text", "text": "s", "cache_control": {"type": "ephemeral", "ttl": "1h"}}]},
                {"role": "user", "content": [
                    {"type": "text", "text": "q", "cache_control": {"type": "ephemeral"}}]}
            ],
            "tools": [{"name": "f", "parameters": {"type": "object"}, "strict": true,
                       "cache_control": {"type": "ephemeral"}}],
            "tool_choice": "auto",
            "sampling": {"max_output_tokens": 64, "top_k": 5},
            "reasoning_mode": "enabled",
            "reasoning_budget_tokens": 2048,
            "parallel_tool_calls": false,
            "stream": false
        })
    );
    let back: ChatRequest = serde_json::from_value(wire).unwrap();
    assert_eq!(back, request);
}

// ── OpenAI Chat ────────────────────────────────────────────────────────────

/// K4: OpenRouter-style part markers.
#[test]
fn chat_part_markers_reach_the_ir() {
    let request = chat(&json!({"messages": [
        {"role": "system", "content": [
            {"type": "text", "text": "s", "cache_control": {"type": "ephemeral", "ttl": "1h"}}]},
        {"role": "user", "content": [
            {"type": "text", "text": "q"},
            {"type": "image_url", "image_url": {"url": "https://x.test/i.png"},
             "cache_control": {"type": "ephemeral"}}]}
    ]}))
    .unwrap();
    assert_eq!(
        parts(request.messages[0].content.as_ref()),
        [ContentPart::Text { text: "s".to_owned(), cache_control: Some(ONE_HOUR) }]
    );
    let user = parts(request.messages[1].content.as_ref());
    assert_eq!(user[0], ContentPart::text("q"));
    assert_eq!(user[1].cache_control(), Some(DEFAULT_TTL));

    let error = chat(&json!({"messages": [{"role": "user", "content": [
        {"type": "text", "text": "q", "cache_control": {"type": "ephemeral", "ttl": "24h"}}]}]}))
    .unwrap_err();
    assert_eq!(error.field(), "messages[0].content[0].cache_control.ttl");
    let error = chat(&json!({"messages": [{"role": "user", "content": [
        {"type": "input_audio", "input_audio": {}, "cache_control": {"type": "other"}}]}]}))
    .unwrap_err();
    assert_eq!(error.field(), "messages[0].content[0].cache_control.type");
}

/// R-Q15 on the Chat surface: typed, never also in `extensions` (the key would be written twice).
#[test]
fn chat_reasoning_effort_parallel_and_strict_are_typed() {
    let request = chat(&json!({
        "reasoning_effort": "high", "parallel_tool_calls": false,
        "tools": [{"type": "function", "function": {"name": "f", "parameters": {}, "strict": true}}]
    }))
    .unwrap();
    assert_eq!(request.reasoning.effort, Some(ReasoningEffort::High));
    assert_eq!(request.parallel_tool_calls, Some(false));
    assert_eq!(request.tools[0].strict, Some(true));
    for key in ["reasoning_effort", "parallel_tool_calls"] {
        assert!(!request.extensions.contains_key(key), "{key}");
    }
    let wire = serde_json::to_string(&request).unwrap();
    assert_eq!(wire.matches("\"reasoning_effort\"").count(), 1, "{wire}");
    assert_eq!(wire.matches("\"parallel_tool_calls\"").count(), 1, "{wire}");

    for (extra, field) in [
        (json!({"reasoning_effort": 3}), "reasoning_effort"),
        (json!({"parallel_tool_calls": "no"}), "parallel_tool_calls"),
        (
            json!({"tools": [{"type": "function",
                           "function": {"name": "f", "parameters": {}, "strict": "yes"}}]}),
            "tools[0].function.strict",
        ),
    ] {
        assert_eq!(chat(&extra).unwrap_err().field(), field, "{extra}");
    }
    let unset = chat(&json!({"reasoning_effort": null, "parallel_tool_calls": null})).unwrap();
    assert_eq!(unset.reasoning.effort, None);
    assert_eq!(unset.parallel_tool_calls, None);
}

/// KC-Q5 on the Chat surface: the IR keys this wire does not own are refused. `sampling` was
/// copied into `extensions` before and then serialized twice beside the typed field.
#[test]
fn reserved_ir_keys_are_refused_on_the_chat_surface() {
    for key in ["reasoning_mode", "reasoning_budget_tokens", "sampling", "host_values"] {
        let error = chat(&json!({key: {"end_user_hash": "spoofed"}})).unwrap_err();
        assert_eq!(error.field(), key);
        assert_eq!(error.stable_code(), "unknown_value", "{key}");
    }
    // A key the wire owns is not reserved.
    assert!(chat(&json!({"reasoning_effort": "low"})).is_ok());
}
