//! Behaviour of the Anthropic Messages family.
//!
//! The cases here are the ones where this protocol differs from its neighbour in
//! a way that costs something if the mapping gets it wrong: turn ordering around
//! tool results, arguments crossing between object and string form, and an
//! envelope that can carry exactly one message.

use serde_json::json;
use south_north_codec::{
    CodecError, ResponseContext, anthropic_message_response, chat_request_from_anthropic_messages,
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
            ContentPart::Text { text: "done".to_owned() },
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
        matches!(&parts[0], ContentPart::ImageUrl { image_url } if image_url.url == "data:image/png;base64,AAAA")
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
                    ContentPart::Text { text: "checking now".to_owned() },
                ])),
                vec![ToolCall {
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
                            id: String::new(),
                            name: "f".to_owned(),
                            arguments: "{}".to_owned(),
                        },
                        ToolCall {
                            id: "toolu_kept".to_owned(),
                            name: "g".to_owned(),
                            arguments: "{}".to_owned(),
                        },
                        ToolCall {
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
    assert_eq!(
        rendered["usage"],
        json!({
            "input_tokens": 1000, "output_tokens": 20,
            "cache_read_input_tokens": 300, "cache_creation_input_tokens": 200
        })
    );
    assert_eq!(
        rendered["stop_reason"], "model_yawned",
        "the provider said something specific; replacing it with 'end_turn' would report a \
         normal completion that did not happen"
    );
}
