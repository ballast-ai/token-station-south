//! Behaviour of the `OpenAI` Chat family, asserted on public output only.
//!
//! Every case here is one the two adopting products used to answer differently,
//! or one where the old answer was wrong in a way a client could see. The point
//! of a shared codec is that these become one answer; the point of these tests
//! is that the answer is written down rather than inferred from whichever
//! implementation someone reads first.

use serde_json::{Value, json};
use south_north_codec::{
    CodecError, OpenAiChatSseState, ResponseContext, chat_request_from_openai_chat,
    openai_chat_frames, openai_chat_response,
};
use token_station_protocol::{
    ChatResponse, Choice, Content, ContentPart, ErrorCode, ErrorEnvelope, FinishReason, Message,
    Role, StreamEvent, ToolCall, ToolChoice, Usage,
};

// ── inbound ────────────────────────────────────────────────────────────────

#[test]
fn developer_and_system_are_the_same_role_and_anything_else_is_refused() {
    let request = chat_request_from_openai_chat(&json!({
        "model": "m",
        "messages": [
            {"role": "developer", "content": "be terse"},
            {"role": "system", "content": "also this"},
            {"role": "user", "content": "hi"}
        ]
    }))
    .expect("a request with only known roles converts");
    assert_eq!(request.messages[0].role, Role::System);
    assert_eq!(request.messages[1].role, Role::System);
    assert_eq!(request.messages[2].role, Role::User);

    let error = chat_request_from_openai_chat(&json!({
        "model": "m",
        "messages": [{"role": "user", "content": "hi"}, {"role": "narrator", "content": "..."}]
    }))
    .expect_err("an undefined role is refused rather than dropped");
    assert_eq!(error.field(), "messages[1].role");
    assert!(matches!(error, CodecError::UnknownValue { .. }), "{error:?}");
}

#[test]
fn the_canonical_output_cap_wins_and_an_unrepresentable_one_is_reported() {
    let with = |body: Value| chat_request_from_openai_chat(&body);

    let both = with(json!({
        "model": "m", "messages": [], "max_completion_tokens": 512, "max_tokens": 99
    }))
    .expect("both keys present is not itself an error here");
    assert_eq!(
        both.sampling.max_output_tokens,
        Some(512),
        "the canonical key wins; a host that refuses disagreement does so before calling"
    );

    let legacy_only = with(json!({"model": "m", "messages": [], "max_tokens": 64}))
        .expect("the legacy key alone is honoured");
    assert_eq!(legacy_only.sampling.max_output_tokens, Some(64));

    let absent =
        with(json!({"model": "m", "messages": []})).expect("no cap at all is not an error");
    assert_eq!(
        absent.sampling.max_output_tokens, None,
        "absent stays absent: the codec does not invent a default, because a default is policy"
    );

    // Reported, never clamped. Clamping hands the client a number it never
    // asked for and tells it nothing.
    let oversized =
        with(json!({"model": "m", "messages": [], "max_completion_tokens": 10_000_000_000i64}))
            .expect_err("a cap beyond the representable range is reported");
    assert_eq!(oversized.field(), "max_completion_tokens");
    assert!(matches!(oversized, CodecError::OutOfRange { .. }), "{oversized:?}");

    let wrong_type = with(json!({"model": "m", "messages": [], "max_completion_tokens": "512"}))
        .expect_err("a cap of the wrong type is reported");
    assert_eq!(wrong_type.field(), "max_completion_tokens");
    assert_eq!(
        wrong_type.to_string(),
        "max_completion_tokens: a string is outside the representable range (a non-negative integer)",
        "the message names the field and the shape, and does not echo the value back"
    );
}

#[test]
fn stop_accepts_both_wire_shapes_and_tool_choice_survives_unrecognised_forms() {
    let one = chat_request_from_openai_chat(&json!({
        "model": "m", "messages": [], "stop": "END", "tool_choice": "required"
    }))
    .expect("converts");
    assert_eq!(one.sampling.stop, vec!["END".to_owned()]);
    assert_eq!(one.tool_choice, Some(ToolChoice::Required));

    let many = chat_request_from_openai_chat(&json!({
        "model": "m", "messages": [], "stop": ["A", "B"],
        "tool_choice": {"type": "function", "function": {"name": "f"}}
    }))
    .expect("converts");
    assert_eq!(many.sampling.stop, vec!["A".to_owned(), "B".to_owned()]);
    assert_eq!(
        many.tool_choice,
        Some(ToolChoice::Other(json!({"type": "function", "function": {"name": "f"}}))),
        "an object form is carried whole so the render side can write it back byte for byte"
    );

    let unknown_string = chat_request_from_openai_chat(
        &json!({"model": "m", "messages": [], "tool_choice": "magic"}),
    )
    .expect("converts");
    assert_eq!(
        unknown_string.tool_choice,
        Some(ToolChoice::Other(json!("magic"))),
        "an unrecognised string is not guessed at either"
    );
}

#[test]
fn unmodelled_top_level_keys_survive_in_extensions_and_modelled_ones_do_not_duplicate() {
    let request = chat_request_from_openai_chat(&json!({
        "model": "m",
        "messages": [],
        "temperature": 0.5,
        "seed": 7,
        "vendor_knob": {"deep": true}
    }))
    .expect("converts");
    assert_eq!(request.sampling.temperature, Some(0.5));
    assert_eq!(request.extensions.get("seed"), Some(&json!(7)));
    assert_eq!(request.extensions.get("vendor_knob"), Some(&json!({"deep": true})));
    for modelled in ["model", "messages", "temperature"] {
        assert!(
            !request.extensions.contains_key(modelled),
            "{modelled} has a typed home; copying it here too would duplicate the key on the wire"
        );
    }
}

#[test]
fn client_supplied_reasoning_is_kept_and_leads_the_visible_text() {
    let request = chat_request_from_openai_chat(&json!({
        "model": "m",
        "messages": [{
            "role": "assistant",
            "content": "the answer",
            "reasoning_content": "first I thought"
        }]
    }))
    .expect("converts");
    assert_eq!(
        request.messages[0].content,
        Some(Content::Parts(vec![
            ContentPart::Thinking { thinking: "first I thought".to_owned(), signature: None },
            ContentPart::Text { text: "the answer".to_owned() },
        ])),
        "reasoning is produced before the answer, so it leads; whether it is replayed upstream \
         is the host's per-model call, not something normalization may pre-empt"
    );
}

#[test]
fn an_image_block_without_a_source_is_carried_whole_rather_than_emptied() {
    let request = chat_request_from_openai_chat(&json!({
        "model": "m",
        "messages": [{"role": "user", "content": [
            {"type": "image_url", "image_url": {"detail": "high"}},
            {"type": "crystal_ball", "prophecy": "rain"}
        ]}]
    }))
    .expect("converts");
    let Some(Content::Parts(parts)) = &request.messages[0].content else {
        panic!("expected parts: {:?}", request.messages[0].content);
    };
    assert_eq!(
        parts[0],
        ContentPart::Unknown(json!({"type": "image_url", "image_url": {"detail": "high"}})),
        "no url means no image: sending one with an empty source is worse than letting the \
         upstream rule on the block as written"
    );
    assert_eq!(
        parts[1],
        ContentPart::Unknown(json!({"type": "crystal_ball", "prophecy": "rain"})),
        "an unmodelled block survives verbatim"
    );
}

// ── outbound, non-streaming ────────────────────────────────────────────────

fn context() -> ResponseContext {
    ResponseContext { created: 1_700_000_000, fallback_id: "chatcmpl-host-minted".to_owned() }
}

fn response_with(choice: Choice, usage: Usage) -> ChatResponse {
    ChatResponse {
        id: "chatcmpl-upstream".to_owned(),
        model: "m".to_owned(),
        choices: vec![choice],
        usage,
        extensions: token_station_protocol::Extensions::new(),
    }
}

const fn assistant(content: Option<Content>, tool_calls: Vec<ToolCall>) -> Choice {
    Choice {
        index: 0,
        message: Message {
            role: Role::Assistant,
            content,
            tool_calls,
            tool_call_id: None,
            name: None,
            extensions: token_station_protocol::Extensions::new(),
        },
        finish_reason: Some(FinishReason::Stop),
        stop_sequence: None,
    }
}

#[test]
fn the_completion_envelope_is_pinned_field_by_field() {
    let rendered = openai_chat_response(
        &response_with(
            assistant(Some(Content::Text("hello".to_owned())), Vec::new()),
            Usage { input_tokens: 10, output_tokens: 5, ..Usage::default() },
        ),
        &context(),
    )
    .expect("renders");
    assert_eq!(
        rendered,
        json!({
            "id": "chatcmpl-upstream",
            "object": "chat.completion",
            "created": 1_700_000_000,
            "model": "m",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "hello"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        }),
        "no cache or reasoning reported means exactly the three usage fields, unchanged"
    );
}

#[test]
fn the_id_prefix_does_not_stack_and_an_empty_id_falls_back_to_the_host() {
    let render = |id: &str| {
        let response = ChatResponse {
            id: id.to_owned(),
            model: "m".to_owned(),
            choices: vec![assistant(Some(Content::Text("x".to_owned())), Vec::new())],
            usage: Usage::default(),
            extensions: token_station_protocol::Extensions::new(),
        };
        openai_chat_response(&response, &context()).expect("renders")["id"].clone()
    };
    assert_eq!(render("chatcmpl-upstream"), json!("chatcmpl-upstream"));
    assert_eq!(render("upstream"), json!("chatcmpl-upstream"));
    assert_eq!(
        render(""),
        json!("chatcmpl-host-minted"),
        "the caller supplies the fallback, because minting one here would need a generator"
    );
}

#[test]
fn empty_text_is_null_and_usage_sub_buckets_appear_only_when_reported() {
    let rendered = openai_chat_response(
        &response_with(
            assistant(
                Some(Content::Parts(vec![
                    ContentPart::Thinking { thinking: "pondering".to_owned(), signature: None },
                    ContentPart::Text { text: String::new() },
                ])),
                Vec::new(),
            ),
            Usage {
                input_tokens: 100,
                output_tokens: 20,
                cache_read_tokens: 30,
                reasoning_tokens: 4,
                ..Usage::default()
            },
        ),
        &context(),
    )
    .expect("renders");
    assert_eq!(rendered["choices"][0]["message"]["content"], Value::Null);
    assert_eq!(rendered["choices"][0]["message"]["reasoning_content"], "pondering");
    assert_eq!(rendered["usage"]["prompt_tokens_details"]["cached_tokens"], 30);
    assert_eq!(rendered["usage"]["completion_tokens_details"]["reasoning_tokens"], 4);
}

#[test]
fn tool_arguments_that_are_not_json_are_reported_instead_of_replaced() {
    let broken = openai_chat_response(
        &response_with(
            assistant(
                None,
                vec![ToolCall {
                    id: "call_1".to_owned(),
                    name: "get_weather".to_owned(),
                    arguments: "{\"city\": Beijing".to_owned(),
                }],
            ),
            Usage::default(),
        ),
        &context(),
    )
    .expect_err("unparseable arguments are reported");
    assert_eq!(broken.field(), "choices[0].message.tool_calls[0].arguments");
    assert!(matches!(broken, CodecError::Unrenderable { .. }), "{broken:?}");

    let good = openai_chat_response(
        &response_with(
            assistant(
                None,
                vec![ToolCall {
                    id: "call_1".to_owned(),
                    name: "get_weather".to_owned(),
                    arguments: "{\"city\":\"Beijing\"}".to_owned(),
                }],
            ),
            Usage::default(),
        ),
        &context(),
    )
    .expect("well-formed arguments render");
    assert_eq!(
        good["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
        "{\"city\":\"Beijing\"}",
        "the exact bytes the model produced reach the tool, unparsed and unreordered"
    );
}

// ── outbound, streaming ────────────────────────────────────────────────────

fn stream_state() -> OpenAiChatSseState {
    OpenAiChatSseState::default()
}

fn terminal_sequence() -> Vec<StreamEvent> {
    vec![
        StreamEvent::Delta { index: 0, content: "hel".to_owned() },
        StreamEvent::Delta { index: 0, content: "lo".to_owned() },
        StreamEvent::Finish { finish_reason: Some(FinishReason::Stop), stop_sequence: None },
        StreamEvent::Usage {
            usage: Usage { input_tokens: 10, output_tokens: 5, ..Usage::default() },
        },
        StreamEvent::Done { finish_reason: Some(FinishReason::Stop), stop_sequence: None },
    ]
}

#[test]
fn framing_does_not_depend_on_how_the_caller_batches_events() {
    let events = terminal_sequence();
    let render = |sizes: &[usize]| {
        let mut state = stream_state();
        let mut out = Vec::new();
        let mut at = 0;
        for size in sizes {
            let end = (at + size).min(events.len());
            out.extend(openai_chat_frames(&events[at..end], &mut state));
            at = end;
        }
        assert_eq!(at, events.len(), "the split must cover every event");
        out
    };
    let whole = render(&[events.len()]);
    for sizes in
        [vec![1, 1, 1, 1, 1], vec![2, 3], vec![3, 2], vec![4, 1], vec![1, 4], vec![2, 1, 2]]
    {
        assert_eq!(
            render(&sizes),
            whole,
            "split {sizes:?} changed the frames: a client-visible shape must not follow \
             upstream packet timing"
        );
    }

    let terminals: Vec<&Value> = whole
        .iter()
        .filter(|chunk| {
            !chunk["choices"][0]["finish_reason"].is_null() || chunk["usage"].is_object()
        })
        .collect();
    assert_eq!(terminals.len(), 1, "finish_reason and usage share one chunk: {whole:?}");
    assert_eq!(terminals[0]["choices"][0]["finish_reason"], "stop");
    assert_eq!(terminals[0]["usage"]["prompt_tokens"], 10);
}

#[test]
fn usage_reported_in_instalments_is_folded_rather_than_replaced() {
    let mut state = stream_state();
    let first = openai_chat_frames(
        &[StreamEvent::Usage {
            usage: Usage {
                input_tokens: 700,
                cache_read_tokens: 300,
                output_tokens: 1,
                ..Usage::default()
            },
        }],
        &mut state,
    );
    let last = openai_chat_frames(
        &[StreamEvent::Usage { usage: Usage { output_tokens: 500, ..Usage::default() } }],
        &mut state,
    );
    assert_eq!(first[0]["usage"]["prompt_tokens"], 700);
    assert_eq!(
        last[0]["usage"],
        json!({
            "prompt_tokens": 700, "completion_tokens": 500, "total_tokens": 1200,
            "prompt_tokens_details": {"cached_tokens": 300}
        }),
        "a provider reporting input up front and output at the end is reporting one usage in \
         two instalments; the last frame must not zero the first half"
    );
}

#[test]
fn a_stream_that_never_finishes_produces_no_finish_chunk() {
    let mut state = stream_state();
    let mut chunks = openai_chat_frames(
        &[StreamEvent::Delta { index: 0, content: "hi".to_owned() }],
        &mut state,
    );
    chunks.extend(openai_chat_frames(
        &[StreamEvent::Finish { finish_reason: Some(FinishReason::Stop), stop_sequence: None }],
        &mut state,
    ));
    assert!(
        chunks.iter().all(|chunk| chunk["choices"][0]["finish_reason"].is_null()),
        "that stream did not finish; a clean finish_reason would render 'the upstream stopped \
         talking' as 'the model completed normally': {chunks:?}"
    );
    assert!(state.pending_finish.is_some(), "the finish waits for Done or a later event");
}

#[test]
fn an_error_ends_the_stream_and_nothing_is_rendered_after_it() {
    let mut state = stream_state();
    let mut chunks = openai_chat_frames(
        &[
            StreamEvent::Delta { index: 0, content: "half".to_owned() },
            StreamEvent::Error {
                error: ErrorEnvelope {
                    code: ErrorCode::UpstreamUnavailable,
                    http_status: 502,
                    message: "upstream died".to_owned(),
                    provider_message: None,
                    retry_after_ms: None,
                    extensions: token_station_protocol::Extensions::new(),
                },
            },
        ],
        &mut state,
    );
    assert!(state.terminated);
    chunks.extend(openai_chat_frames(
        &[
            StreamEvent::Usage {
                usage: Usage { input_tokens: 1, output_tokens: 1, ..Usage::default() },
            },
            StreamEvent::Done { finish_reason: Some(FinishReason::Stop), stop_sequence: None },
        ],
        &mut state,
    ));
    assert_eq!(chunks.len(), 1, "only the delta that preceded the error: {chunks:?}");
    assert!(
        chunks.iter().all(|chunk| chunk["choices"][0]["finish_reason"].is_null()),
        "a terminal after an error tells the client the message completed: {chunks:?}"
    );
}

#[test]
fn a_thinking_signature_has_no_slot_on_this_wire_and_does_not_leak() {
    let mut state = stream_state();
    let chunks = openai_chat_frames(
        &[
            StreamEvent::ThinkingDelta { index: 0, thinking_delta: "think".to_owned() },
            StreamEvent::ThinkingSignatureDelta {
                index: 0,
                signature_delta: "sig-fragment".to_owned(),
            },
        ],
        &mut state,
    );
    assert_eq!(chunks.len(), 1, "only the thinking delta renders: {chunks:?}");
    assert!(
        !chunks[0].to_string().contains("sig-fragment"),
        "the signature must not be smuggled into another field: {chunks:?}"
    );
}
