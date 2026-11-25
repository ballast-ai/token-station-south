//! Behaviour of the Anthropic Messages family.
//!
//! The cases here are the ones where this protocol differs from its neighbour in
//! a way that costs something if the mapping gets it wrong: turn ordering around
//! tool results, arguments crossing between object and string form, and an
//! envelope that can carry exactly one message.

use serde_json::json;
use south_north_codec::{
    AnthropicRequestOptions, CodecError, ResponseContext, anthropic_message_response,
    chat_request_from_anthropic_messages, chat_request_from_anthropic_messages_with,
};
use token_station_protocol::{
    ChatResponse, Choice, Content, ContentPart, Extensions, FinishReason, Message, Role, ToolCall,
    ToolChoice, Usage,
};

// ── inbound ────────────────────────────────────────────────────────────────

#[test]
fn a_tool_result_becomes_its_own_turn_and_precedes_the_rest_of_the_user_message() {
    let request = chat_request_from_anthropic_messages(&json!({
        "model": "m",
        "max_tokens": 64,
        "system": "be terse",
        "messages": [
            {"role": "user", "content": "weather?"},
            {"role": "assistant", "content": [
                {"type": "text", "text": "checking"},
                {"type": "tool_use", "id": "toolu_1", "name": "get_weather",
                 "input": {"city": "Beijing"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": "sunny"},
                {"type": "text", "text": "and tomorrow?"}
            ]}
        ]
    }))
    .expect("converts");

    let roles: Vec<Role> = request.messages.iter().map(|message| message.role).collect();
    assert_eq!(
        roles,
        vec![Role::System, Role::User, Role::Assistant, Role::Tool, Role::User],
        "the result answers the previous assistant turn, so it comes before the new user text; \
         the other order rewrites the conversation"
    );
    let tool_turn = &request.messages[3];
    assert_eq!(tool_turn.tool_call_id.as_deref(), Some("toolu_1"));
    assert_eq!(tool_turn.content, Some(Content::Text("sunny".to_owned())));
    assert_eq!(
        request.messages[2].tool_calls[0].arguments, "{\"city\":\"Beijing\"}",
        "an object on this wire becomes the string a tool receives, serialized once"
    );
}

#[test]
fn thinking_blocks_keep_their_signature_and_redacted_ones_survive() {
    let request = chat_request_from_anthropic_messages(&json!({
        "model": "m",
        "max_tokens": 64,
        "messages": [{"role": "assistant", "content": [
            {"type": "thinking", "thinking": "hmm", "signature": "sig-1"},
            {"type": "redacted_thinking", "data": "opaque"},
            {"type": "text", "text": "done"}
        ]}]
    }))
    .expect("converts");
    assert_eq!(
        request.messages[0].content,
        Some(Content::Parts(vec![
            ContentPart::Thinking {
                thinking: "hmm".to_owned(),
                signature: Some("sig-1".to_owned())
            },
            ContentPart::RedactedThinking { data: "opaque".to_owned() },
            ContentPart::text("done".to_owned()),
        ])),
        "the signature is the ticket that makes a replay verifiable, and the redacted block is \
         unreadable but still has to survive the round trip"
    );
}

#[test]
fn a_named_tool_choice_keeps_its_name_and_an_undefined_role_is_refused() {
    let named = chat_request_from_anthropic_messages(&json!({
        "model": "m", "max_tokens": 1, "messages": [],
        "tool_choice": {"type": "tool", "name": "get_weather"}
    }))
    .expect("converts");
    assert_eq!(
        named.tool_choice,
        Some(ToolChoice::Other(json!({"type": "function", "function": {"name": "get_weather"}}))),
        "downgrading this to 'any tool will do' would let the model call one the client did \
         not ask for"
    );

    let any = chat_request_from_anthropic_messages(&json!({
        "model": "m", "max_tokens": 1, "messages": [], "tool_choice": {"type": "any"}
    }))
    .expect("converts");
    assert_eq!(any.tool_choice, Some(ToolChoice::Required));

    let none = chat_request_from_anthropic_messages(&json!({
        "model": "m", "max_tokens": 1, "messages": [], "tool_choice": {"type": "none"}
    }))
    .expect("converts");
    assert_eq!(
        none.tool_choice,
        Some(ToolChoice::None),
        "dropping it would turn 'no tool calls' into 'the model decides'"
    );

    let error = chat_request_from_anthropic_messages(&json!({
        "model": "m", "max_tokens": 1,
        "messages": [{"role": "narrator", "content": "..."}]
    }))
    .expect_err("an undefined role is refused");
    assert_eq!(error.field(), "messages[0].role");
    assert!(matches!(error, CodecError::UnknownValue { .. }), "{error:?}");
}

#[test]
fn an_image_source_shape_this_protocol_does_not_define_is_carried_whole() {
    let request = chat_request_from_anthropic_messages(&json!({
        "model": "m", "max_tokens": 1,
        "messages": [{"role": "user", "content": [
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}},
            {"type": "image", "source": {"type": "carrier_pigeon", "bird": "Rock Dove"}}
        ]}]
    }))
    .expect("converts");
    let Some(Content::Parts(parts)) = &request.messages[0].content else {
        panic!("expected parts: {:?}", request.messages[0].content);
    };
    assert!(
        matches!(&parts[0], ContentPart::ImageUrl { image_url, .. } if image_url.url == "data:image/png;base64,AAAA")
    );
    assert!(
        matches!(&parts[1], ContentPart::Unknown(_)),
        "an undefined source shape travels whole rather than becoming an image with no source"
    );
}

// ── outbound ───────────────────────────────────────────────────────────────

fn context() -> ResponseContext {
    ResponseContext { created: 1_700_000_000, fallback_id: "msg_host-minted".to_owned() }
}

fn response(choices: Vec<Choice>, usage: Usage) -> ChatResponse {
    ChatResponse {
        id: "upstream-1".to_owned(),
        model: "m".to_owned(),
        choices,
        usage,
        extensions: Extensions::new(),
    }
}

const fn choice(
    content: Option<Content>,
    tool_calls: Vec<ToolCall>,
    finish: FinishReason,
) -> Choice {
    Choice {
        index: 0,
        message: Message {
            role: Role::Assistant,
            content,
            tool_calls,
            tool_call_id: None,
            name: None,
            extensions: Extensions::new(),
        },
        finish_reason: Some(finish),
        stop_sequence: None,
    }
}

#[test]
fn the_message_envelope_is_pinned_block_by_block() {
    let rendered = anthropic_message_response(
        &response(
            vec![choice(
                Some(Content::Parts(vec![
                    ContentPart::Thinking { thinking: "reasoning".to_owned(), signature: None },
                    ContentPart::text("checking now".to_owned()),
                ])),
                vec![ToolCall {
                    signature: None,
                    id: "toolu_upstream".to_owned(),
                    name: "get_weather".to_owned(),
                    arguments: "{\"city\":\"Beijing\"}".to_owned(),
                }],
                FinishReason::ToolCalls,
            )],
            Usage { input_tokens: 100, output_tokens: 50, ..Usage::default() },
        ),
        &context(),
    )
    .expect("renders");
    assert_eq!(
        rendered,
        json!({
            "id": "msg_upstream-1",
            "type": "message",
            "role": "assistant",
            "model": "m",
            "content": [
                {"type": "thinking", "thinking": "reasoning"},
                {"type": "text", "text": "checking now"},
                {"type": "tool_use", "id": "toolu_upstream", "name": "get_weather",
                 "input": {"city": "Beijing"}}
            ],
            "stop_reason": "tool_use",
            "stop_sequence": null,
            "usage": {"input_tokens": 100, "output_tokens": 50}
        }),
        "thinking leads, visible text sits in the middle, tool_use goes last; no cache activity \
         means the two usage fields this wire always had"
    );
}

#[test]
fn anything_but_exactly_one_choice_is_refused() {
    let one = choice(Some(Content::Text("only".to_owned())), Vec::new(), FinishReason::Stop);
    assert!(
        anthropic_message_response(&response(vec![one.clone()], Usage::default()), &context())
            .is_ok()
    );

    for count in [0_usize, 2] {
        let error = anthropic_message_response(
            &response(vec![one.clone(); count], Usage::default()),
            &context(),
        )
        .expect_err("this envelope carries exactly one message");
        assert_eq!(error.field(), "choices");
        assert!(
            error.to_string().contains(&format!("upstream returned {count}")),
            "the refusal says how many arrived: {error}"
        );
    }
}

#[test]
fn a_missing_tool_id_is_derived_so_the_same_response_renders_identically() {
    let render = || {
        anthropic_message_response(
            &response(
                vec![choice(
                    None,
                    vec![
                        ToolCall {
                            signature: None,
                            id: String::new(),
                            name: "f".to_owned(),
                            arguments: "{}".to_owned(),
                        },
                        ToolCall {
                            signature: None,
                            id: "toolu_kept".to_owned(),
                            name: "g".to_owned(),
                            arguments: "{}".to_owned(),
                        },
                        ToolCall {
                            signature: None,
                            id: String::new(),
                            name: "h".to_owned(),
                            arguments: "{}".to_owned(),
                        },
                    ],
                    FinishReason::ToolCalls,
                )],
                Usage::default(),
            ),
            &context(),
        )
        .expect("renders")
    };
    let first = render();
    assert_eq!(first, render(), "no random source: the same response renders byte for byte");
    let ids: Vec<&str> = first["content"]
        .as_array()
        .expect("content is an array")
        .iter()
        .map(|block| block["id"].as_str().expect("tool_use blocks carry an id"))
        .collect();
    assert_eq!(ids, ["toolu_upstream-1_0", "toolu_kept", "toolu_upstream-1_2"]);
}

#[test]
fn unparseable_tool_arguments_are_refused_rather_than_rendered_as_an_empty_object() {
    let error = anthropic_message_response(
        &response(
            vec![choice(
                None,
                vec![ToolCall {
                    signature: None,
                    id: "toolu_1".to_owned(),
                    name: "get_weather".to_owned(),
                    arguments: "{\"city\": Beijing".to_owned(),
                }],
                FinishReason::ToolCalls,
            )],
            Usage::default(),
        ),
        &context(),
    )
    .expect_err("an empty object would be a tool call the model never made");
    assert_eq!(error.field(), "choices[0].message.tool_calls[0].arguments");
    assert!(matches!(error, CodecError::Unrenderable { .. }), "{error:?}");
}

#[test]
fn cache_buckets_appear_when_reported_and_an_unmodelled_stop_reason_survives() {
    let rendered = anthropic_message_response(
        &response(
            vec![choice(
                Some(Content::Text("hi".to_owned())),
                Vec::new(),
                FinishReason::Other("model_yawned".to_owned()),
            )],
            Usage {
                input_tokens: 1000,
                output_tokens: 20,
                cache_read_tokens: 300,
                cache_write_tokens: 200,
                ..Usage::default()
            },
        ),
        &context(),
    )
    .expect("renders");
    // The IR's 1000 is the whole prompt, partitioned by the cache buckets
    // (kernel `Usage::total`); this wire's `input_tokens` is the uncached rest.
    assert_eq!(
        rendered["usage"],
        json!({
            "input_tokens": 500, "output_tokens": 20,
            "cache_read_input_tokens": 300, "cache_creation_input_tokens": 200
        })
    );
    assert_eq!(
        rendered["stop_reason"], "model_yawned",
        "the provider said something specific; replacing it with 'end_turn' would report a \
         normal completion that did not happen"
    );
}

// ── Claude replay markers (opt-in) ─────────────────────────────────────────

fn with_assistant(content: &serde_json::Value) -> serde_json::Value {
    json!({"model": "m", "max_tokens": 64, "messages": [
        {"role": "user", "content": "question"},
        {"role": "assistant", "content": content},
        {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "found"}]}
    ]})
}

fn marked(body: &serde_json::Value) -> Message {
    let request = chat_request_from_anthropic_messages_with(
        body,
        AnthropicRequestOptions { claude_replay_markers: true },
    )
    .expect("converts");
    request.messages.into_iter().find(|message| message.role == Role::Assistant).expect("assistant")
}

#[test]
fn a_signed_turn_is_marked_with_its_wire_order_only_when_the_host_asks() {
    let body = with_assistant(&json!([
        {"type": "thinking", "thinking": "plan", "signature": "sig-1"},
        {"type": "redacted_thinking", "data": "opaque"},
        {"type": "text", "text": "Looking it up."},
        {"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {}}
    ]));

    let assistant = marked(&body);
    assert_eq!(
        assistant.extensions.get("reasoning_replay_protocol_family"),
        Some(&json!("claude-signed-thinking"))
    );
    assert_eq!(
        assistant.extensions.get("reasoning_replay_block_layout"),
        Some(&json!([
            {"kind": "content", "ordinal": 0},
            {"kind": "content", "ordinal": 1},
            {"kind": "content", "ordinal": 2},
            {"kind": "tool_call", "call_id": "toolu_1"}
        ]))
    );

    // The default decode is unchanged: no markers.
    let plain = chat_request_from_anthropic_messages(&body).expect("converts");
    let assistant = plain.messages.iter().find(|message| message.role == Role::Assistant).unwrap();
    assert!(assistant.extensions.is_empty(), "{:?}", assistant.extensions);
}

#[test]
fn a_turn_the_layout_cannot_describe_is_not_marked() {
    for content in [
        json!("plain answer"),
        json!([{"type": "text", "text": "no reasoning"}]),
        json!([{"type": "thinking", "thinking": "unsigned"}, {"type": "text", "text": "a"}]),
        json!([{"type": "thinking", "thinking": "empty", "signature": ""}]),
        json!([
            {"type": "thinking", "thinking": "plan", "signature": "sig-1"},
            {"type": "server_tool_use", "id": "srvtoolu_1", "name": "web_search", "input": {}}
        ]),
        json!([
            {"type": "thinking", "thinking": "plan", "signature": "sig-1"},
            {"type": "tool_use", "id": "toolu_1", "name": "a", "input": {}},
            {"type": "tool_use", "id": "toolu_1", "name": "b", "input": {}}
        ]),
    ] {
        let assistant = marked(&with_assistant(&content));
        assert!(
            !assistant.extensions.contains_key("reasoning_replay_protocol_family")
                && !assistant.extensions.contains_key("reasoning_replay_block_layout"),
            "{content}"
        );
    }

    // More blocks than a layout may list.
    let mut blocks = vec![json!({"type": "thinking", "thinking": "plan", "signature": "sig-1"})];
    blocks.extend((0..128).map(|n| json!({"type": "text", "text": format!("t{n}")})));
    let assistant = marked(&with_assistant(&serde_json::Value::Array(blocks)));
    assert!(!assistant.extensions.contains_key("reasoning_replay_block_layout"));
}
