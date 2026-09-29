use serde_json::json;
use south_north_codec::responses::{
    CLAUDE_REASONING_REPLAY_CAPABILITY, ReasoningReplayBlock, ReasoningReplayCarrier,
    decode_reasoning_replay_carrier, encode_reasoning_replay_carrier,
};
use south_north_codec::{
    ResponsesContext, ResponsesReasoningMode, ResponsesRequestOptions, ResponsesSseState,
    chat_request_from_responses, responses_frames, responses_response,
};
use token_station_protocol::{
    ChatResponse, Choice, Content, ContentPart, Extensions, FinishReason, StreamEvent, Usage,
};

const fn carrier(blocks: Vec<ReasoningReplayBlock>) -> ReasoningReplayCarrier {
    ReasoningReplayCarrier::claude(blocks)
}

#[test]
fn c1_round_trips_ordered_claude_blocks_without_rewriting_opaque_values() {
    let original = carrier(vec![
        ReasoningReplayBlock::Thinking {
            thinking: "思考".into(),
            signature: "sig+/= unchanged".into(),
        },
        ReasoningReplayBlock::TextRef { ordinal: 0 },
        ReasoningReplayBlock::RedactedThinking { data: "opaque+/= 数据".into() },
        ReasoningReplayBlock::ToolCallRef { call_id: "call_α".into() },
    ]);

    let encoded = encode_reasoning_replay_carrier(&original).expect("valid carrier");
    assert!(encoded.starts_with("tsr.c1."));
    assert!(!encoded["tsr.c1.".len()..].contains('='));
    assert_eq!(decode_reasoning_replay_carrier(&encoded).unwrap(), original);
    assert_eq!(CLAUDE_REASONING_REPLAY_CAPABILITY, "reasoning_replay.claude.v1");
}

#[test]
fn c2_refuses_unknown_fields_and_block_count_without_truncating() {
    let unknown = "tsr.c1.eyJuYW1lc3BhY2UiOiJ0b2tlbi1zdGF0aW9uLnJlYXNvbmluZy1yZXBsYXkiLCJ2ZXJzaW9uIjoxLCJwcm90b2NvbF9mYW1pbHkiOiJjbGF1ZGUtc2lnbmVkLXRoaW5raW5nIiwiYmxvY2tzIjpbXSwicHJvdmlkZXIiOiJmb3JnZWQifQ";
    let error = decode_reasoning_replay_carrier(unknown).unwrap_err();
    assert_eq!(error.stable_code(), "reasoning_replay_invalid");

    let too_many =
        carrier((0..129).map(|ordinal| ReasoningReplayBlock::TextRef { ordinal }).collect());
    let error = encode_reasoning_replay_carrier(&too_many).unwrap_err();
    assert_eq!(error.stable_code(), "reasoning_replay_invalid");
}

#[test]
fn c2_enforces_per_block_and_total_opaque_bounds() {
    let one_mib = "a".repeat(1024 * 1024);
    assert!(
        encode_reasoning_replay_carrier(&carrier(vec![ReasoningReplayBlock::RedactedThinking {
            data: one_mib.clone()
        }]))
        .is_ok()
    );
    let error =
        encode_reasoning_replay_carrier(&carrier(vec![ReasoningReplayBlock::RedactedThinking {
            data: format!("{one_mib}x"),
        }]))
        .unwrap_err();
    assert_eq!(error.stable_code(), "reasoning_replay_invalid");

    let total = carrier(
        (0..5).map(|_| ReasoningReplayBlock::RedactedThinking { data: one_mib.clone() }).collect(),
    );
    assert_eq!(
        encode_reasoning_replay_carrier(&total).unwrap_err().stable_code(),
        "reasoning_replay_invalid"
    );

    let exactly_four_mib = carrier(
        (0..4).map(|_| ReasoningReplayBlock::RedactedThinking { data: one_mib.clone() }).collect(),
    );
    assert!(encode_reasoning_replay_carrier(&exactly_four_mib).is_ok());
    let four_mib_plus_one = carrier(
        (0..4)
            .map(|_| ReasoningReplayBlock::RedactedThinking { data: one_mib.clone() })
            .chain(std::iter::once(ReasoningReplayBlock::RedactedThinking { data: "x".into() }))
            .collect(),
    );
    assert!(encode_reasoning_replay_carrier(&four_mib_plus_one).is_err());
}

#[test]
fn c2_rejects_empty_signatures_and_counts_call_ids_and_encoded_bytes() {
    let error = encode_reasoning_replay_carrier(&carrier(vec![ReasoningReplayBlock::Thinking {
        thinking: String::new(),
        signature: String::new(),
    }]))
    .unwrap_err();
    assert_eq!(error.stable_code(), "reasoning_replay_invalid");

    let one_mib = "c".repeat(1024 * 1024);
    assert!(
        encode_reasoning_replay_carrier(&carrier(vec![ReasoningReplayBlock::ToolCallRef {
            call_id: one_mib.clone(),
        }]))
        .is_ok()
    );
    assert!(
        encode_reasoning_replay_carrier(&carrier(vec![ReasoningReplayBlock::ToolCallRef {
            call_id: format!("{one_mib}x"),
        }]))
        .is_err()
    );

    let escaped = "\0".repeat(1024 * 1024);
    let error = encode_reasoning_replay_carrier(&carrier(
        (0..4).map(|_| ReasoningReplayBlock::RedactedThinking { data: escaped.clone() }).collect(),
    ))
    .unwrap_err();
    assert_eq!(error.stable_code(), "reasoning_replay_invalid");
}

#[test]
fn c1_responses_request_and_output_preserve_carrier_and_ordered_blocks() {
    let encoded = encode_reasoning_replay_carrier(&carrier(vec![
        ReasoningReplayBlock::Thinking { thinking: "first".into(), signature: "sig".into() },
        ReasoningReplayBlock::TextRef { ordinal: 0 },
        ReasoningReplayBlock::RedactedThinking { data: "redacted".into() },
        ReasoningReplayBlock::ToolCallRef { call_id: "call_1".into() },
    ]))
    .unwrap();
    let request = chat_request_from_responses(
        &json!({"model":"m","input":[
            {"type":"reasoning","id":"rs_1","encrypted_content":encoded,"summary":[]},
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"answer"}]},
            {"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{}"}
        ]}),
        &ResponsesRequestOptions::default(),
    )
    .unwrap();
    assert_eq!(request.messages.len(), 1);
    assert!(matches!(
        &request.messages[0].content,
        Some(Content::Parts(parts))
            if matches!(&parts[0], ContentPart::Thinking { thinking, signature: Some(signature) } if thinking == "first" && signature == "sig")
                && matches!(&parts[1], ContentPart::Text { text } if text == "answer")
                && matches!(&parts[2], ContentPart::RedactedThinking { data } if data == "redacted")
    ));

    let response = ChatResponse {
        id: "upstream".into(),
        model: "m".into(),
        choices: vec![Choice {
            index: 0,
            message: request.messages[0].clone(),
            finish_reason: Some(FinishReason::Stop),
            stop_sequence: None,
        }],
        usage: Usage::default(),
        extensions: Extensions::new(),
    };
    let output = responses_response(
        &response,
        &ResponsesContext {
            response_id: "resp_1".into(),
            model: "m".into(),
            created_at: 1,
            inbound_tools: json!([{"type":"function","name":"lookup"}]),
            reasoning: ResponsesReasoningMode::Summary,
            allow_incomplete_tool_calls: false,
            render_legacy_encrypted_reasoning: false,
        },
    )
    .unwrap();
    assert_eq!(output["output"][0]["type"], "reasoning");
    assert_eq!(output["output"][0]["id"], "rs_1");
    assert_eq!(output["output"][0]["encrypted_content"], encoded);
    assert_eq!(output["output"][1]["type"], "message");
    assert_eq!(output["output"][2]["call_id"], "call_1");
}

#[test]
fn x1_rejects_missing_or_mismatched_references_before_translation() {
    for blocks in [
        vec![ReasoningReplayBlock::TextRef { ordinal: 1 }],
        vec![ReasoningReplayBlock::ToolCallRef { call_id: "missing".into() }],
    ] {
        let encoded = encode_reasoning_replay_carrier(&carrier(blocks)).unwrap();
        let error = chat_request_from_responses(
            &json!({"model":"m","input":[
                {"type":"reasoning","id":"rs_1","encrypted_content":encoded,"summary":[]},
                {"type":"message","role":"assistant","content":"one"}
            ]}),
            &ResponsesRequestOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.stable_code(), "reasoning_replay_invalid");
    }
}

#[test]
fn c1_output_requires_a_bijection_over_content_and_tool_references() {
    let context = ResponsesContext {
        response_id: "resp_refs".into(),
        model: "m".into(),
        created_at: 1,
        inbound_tools: json!([{"type":"function","name":"lookup"}]),
        reasoning: ResponsesReasoningMode::Summary,
        allow_incomplete_tool_calls: false,
        render_legacy_encrypted_reasoning: false,
    };
    for layout in [
        json!([
            {"kind":"content","ordinal":0},
            {"kind":"content","ordinal":0},
            {"kind":"tool_call","call_id":"call_1"}
        ]),
        json!([
            {"kind":"content","ordinal":0},
            {"kind":"tool_call","call_id":"call_1"}
        ]),
        json!([
            {"kind":"content","ordinal":0},
            {"kind":"content","ordinal":1},
            {"kind":"tool_call","call_id":"call_1"},
            {"kind":"tool_call","call_id":"call_1"}
        ]),
    ] {
        let mut extensions = Extensions::new();
        extensions
            .insert("reasoning_replay_protocol_family".into(), json!("claude-signed-thinking"));
        extensions.insert("reasoning_replay_block_layout".into(), layout);
        let response = ChatResponse {
            id: "upstream".into(),
            model: "m".into(),
            choices: vec![Choice {
                index: 0,
                message: token_station_protocol::Message {
                    role: token_station_protocol::Role::Assistant,
                    content: Some(Content::Parts(vec![
                        ContentPart::Text { text: "one".into() },
                        ContentPart::Text { text: "two".into() },
                    ])),
                    tool_calls: vec![token_station_protocol::ToolCall {
                        id: "call_1".into(),
                        name: "lookup".into(),
                        arguments: "{}".into(),
                    }],
                    tool_call_id: None,
                    name: None,
                    extensions,
                },
                finish_reason: Some(FinishReason::ToolCalls),
                stop_sequence: None,
            }],
            usage: Usage::default(),
            extensions: Extensions::new(),
        };
        assert_eq!(
            responses_response(&response, &context).unwrap_err().stable_code(),
            "reasoning_replay_invalid"
        );
    }
}

#[test]
fn c1_output_uses_text_ordinals_and_preserves_adjacent_reasoning_items_verbatim() {
    let first = encode_reasoning_replay_carrier(&carrier(vec![ReasoningReplayBlock::Thinking {
        thinking: "private one".into(),
        signature: "sig-1".into(),
    }]))
    .unwrap();
    let second = encode_reasoning_replay_carrier(&carrier(vec![ReasoningReplayBlock::Thinking {
        thinking: "private two".into(),
        signature: "sig-2".into(),
    }]))
    .unwrap();
    let request = chat_request_from_responses(
        &json!({"model":"m","input":[
            {"type":"reasoning","id":"rs_1","encrypted_content":first,"summary":[],"vendor_extension":{"kept":1}},
            {"type":"reasoning","id":"rs_2","encrypted_content":second,"summary":[],"vendor_extension":{"kept":2}}
        ]}),
        &ResponsesRequestOptions::default(),
    )
    .unwrap();
    let response = ChatResponse {
        id: "upstream".into(),
        model: "m".into(),
        choices: vec![Choice {
            index: 0,
            message: request.messages[0].clone(),
            finish_reason: Some(FinishReason::Stop),
            stop_sequence: None,
        }],
        usage: Usage::default(),
        extensions: Extensions::new(),
    };
    let output = responses_response(&response, &stream_context()).unwrap();
    assert_eq!(output["output"][0]["id"], "rs_1");
    assert_eq!(output["output"][0]["summary"], json!([]));
    assert_eq!(output["output"][0]["vendor_extension"], json!({"kept":1}));
    assert_eq!(output["output"][1]["id"], "rs_2");
    assert_eq!(output["output"][1]["summary"], json!([]));
    assert_eq!(output["output"][1]["vendor_extension"], json!({"kept":2}));
}

#[test]
fn c1_output_text_refs_name_text_position_in_content_not_layout_order() {
    let mut extensions = Extensions::new();
    extensions.insert("reasoning_replay_protocol_family".into(), json!("claude-signed-thinking"));
    extensions.insert(
        "reasoning_replay_block_layout".into(),
        json!([
            {"kind":"content","ordinal":1},
            {"kind":"content","ordinal":0}
        ]),
    );
    let response = ChatResponse {
        id: "upstream".into(),
        model: "m".into(),
        choices: vec![Choice {
            index: 0,
            message: token_station_protocol::Message {
                role: token_station_protocol::Role::Assistant,
                content: Some(Content::Parts(vec![
                    ContentPart::Text { text: "zero".into() },
                    ContentPart::Text { text: "one".into() },
                ])),
                tool_calls: Vec::new(),
                tool_call_id: None,
                name: None,
                extensions,
            },
            finish_reason: Some(FinishReason::Stop),
            stop_sequence: None,
        }],
        usage: Usage::default(),
        extensions: Extensions::new(),
    };
    let output = responses_response(&response, &stream_context()).unwrap();
    let replay =
        decode_reasoning_replay_carrier(output["output"][0]["encrypted_content"].as_str().unwrap())
            .unwrap();
    assert_eq!(
        replay.blocks(),
        &[
            ReasoningReplayBlock::TextRef { ordinal: 1 },
            ReasoningReplayBlock::TextRef { ordinal: 0 },
        ]
    );
}

fn stream_context() -> ResponsesContext {
    ResponsesContext {
        response_id: "resp_stream".into(),
        model: "m".into(),
        created_at: 1,
        inbound_tools: json!([{"type":"function","name":"lookup"}]),
        reasoning: ResponsesReasoningMode::Summary,
        allow_incomplete_tool_calls: false,
        render_legacy_encrypted_reasoning: false,
    }
}

#[test]
fn x5_stream_preserves_interleaved_layout_and_reuses_the_completed_carrier() {
    let mut state = ResponsesSseState::new(stream_context());
    let frames = responses_frames(
        &[
            StreamEvent::ThinkingDelta { index: 0, block_index: 0, thinking_delta: "first".into() },
            StreamEvent::ThinkingSignatureDelta {
                index: 0,
                block_index: 0,
                signature_delta: "sig".into(),
            },
            StreamEvent::Delta { index: 0, content: "answer".into() },
            StreamEvent::RedactedThinking { index: 0, block_index: 2, data: "opaque".into() },
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("call_1".into()),
                name: Some("lookup".into()),
                arguments_delta: "{}".into(),
            },
            StreamEvent::Done { finish_reason: Some(FinishReason::ToolCalls), stop_sequence: None },
        ],
        &mut state,
    )
    .unwrap();
    let done_item = frames
        .iter()
        .find(|frame| {
            frame.event == "response.output_item.done" && frame.data["item"]["type"] == "reasoning"
        })
        .unwrap();
    let carrier = done_item.data["item"]["encrypted_content"].as_str().unwrap();
    let completed = &frames.last().unwrap().data["response"]["output"];
    assert_eq!(completed[0]["encrypted_content"], carrier);
    assert_eq!(completed[1]["type"], "message");
    assert_eq!(completed[2]["type"], "function_call");
    assert_eq!(
        decode_reasoning_replay_carrier(carrier).unwrap().blocks(),
        &[
            ReasoningReplayBlock::Thinking { thinking: "first".into(), signature: "sig".into() },
            ReasoningReplayBlock::TextRef { ordinal: 0 },
            ReasoningReplayBlock::RedactedThinking { data: "opaque".into() },
            ReasoningReplayBlock::ToolCallRef { call_id: "call_1".into() },
        ]
    );
    let terminal = state.terminal_response().unwrap();
    assert!(matches!(
        terminal.choices[0].message.content.as_ref(),
        Some(Content::Parts(parts))
            if matches!(&parts[0], ContentPart::Thinking { signature: Some(signature), .. } if signature == "sig")
                && matches!(&parts[1], ContentPart::Text { text } if text == "answer")
                && matches!(&parts[2], ContentPart::RedactedThinking { data } if data == "opaque")
    ));
}

#[test]
fn x6_stream_refuses_missing_signature_duplicate_redacted_and_out_of_order_blocks() {
    let cases = vec![
        vec![
            StreamEvent::ThinkingDelta {
                index: 0,
                block_index: 0,
                thinking_delta: "unsigned".into(),
            },
            StreamEvent::RedactedThinking { index: 0, block_index: 1, data: "opaque".into() },
            StreamEvent::Done { finish_reason: None, stop_sequence: None },
        ],
        vec![
            StreamEvent::RedactedThinking { index: 0, block_index: 0, data: "a".into() },
            StreamEvent::RedactedThinking { index: 0, block_index: 0, data: "b".into() },
        ],
        vec![
            StreamEvent::RedactedThinking { index: 0, block_index: 2, data: "a".into() },
            StreamEvent::RedactedThinking { index: 0, block_index: 1, data: "b".into() },
        ],
    ];
    for events in cases {
        let mut state = ResponsesSseState::new(stream_context());
        let error = responses_frames(&events, &mut state).unwrap_err();
        assert_eq!(error.stable_code(), "reasoning_replay_invalid");
        assert!(state.terminal_response().is_none());
    }
}

#[test]
fn x5_stream_partitions_replay_carriers_and_terminal_messages_by_choice() {
    let mut state = ResponsesSseState::new(stream_context());
    let frames = responses_frames(
        &[
            StreamEvent::ThinkingDelta { index: 0, block_index: 0, thinking_delta: "zero".into() },
            StreamEvent::ThinkingSignatureDelta {
                index: 0,
                block_index: 0,
                signature_delta: "sig-zero".into(),
            },
            StreamEvent::Delta { index: 0, content: "answer-zero".into() },
            StreamEvent::ThinkingDelta { index: 1, block_index: 0, thinking_delta: "one".into() },
            StreamEvent::ThinkingSignatureDelta {
                index: 1,
                block_index: 0,
                signature_delta: "sig-one".into(),
            },
            StreamEvent::Delta { index: 1, content: "answer-one".into() },
            StreamEvent::Done { finish_reason: Some(FinishReason::Stop), stop_sequence: None },
        ],
        &mut state,
    )
    .unwrap();
    let carriers = frames
        .iter()
        .filter(|frame| {
            frame.event == "response.output_item.done" && frame.data["item"]["type"] == "reasoning"
        })
        .map(|frame| {
            decode_reasoning_replay_carrier(
                frame.data["item"]["encrypted_content"].as_str().unwrap(),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(carriers.len(), 2);
    assert_eq!(
        carriers[0].blocks(),
        &[
            ReasoningReplayBlock::Thinking {
                thinking: "zero".into(),
                signature: "sig-zero".into(),
            },
            ReasoningReplayBlock::TextRef { ordinal: 0 },
        ]
    );
    assert_eq!(
        carriers[1].blocks(),
        &[
            ReasoningReplayBlock::Thinking { thinking: "one".into(), signature: "sig-one".into() },
            ReasoningReplayBlock::TextRef { ordinal: 0 },
        ]
    );
    let terminal = state.terminal_response().unwrap();
    assert_eq!(terminal.choices.len(), 2);
    assert_eq!(terminal.choices[0].index, 0);
    assert_eq!(terminal.choices[1].index, 1);
}

#[test]
fn x6_stream_rejects_replay_budget_overflow_before_done() {
    let mut too_many = Vec::new();
    for block_index in 0..129 {
        too_many.push(StreamEvent::RedactedThinking { index: 0, block_index, data: String::new() });
    }
    let mut state = ResponsesSseState::new(stream_context());
    assert_eq!(
        responses_frames(&too_many, &mut state).unwrap_err().stable_code(),
        "reasoning_replay_invalid"
    );

    let mut state = ResponsesSseState::new(stream_context());
    assert_eq!(
        responses_frames(
            &[StreamEvent::RedactedThinking {
                index: 0,
                block_index: 0,
                data: "x".repeat((1024 * 1024) + 1),
            }],
            &mut state,
        )
        .unwrap_err()
        .stable_code(),
        "reasoning_replay_invalid"
    );

    let mut over_total = (0..4)
        .map(|block_index| StreamEvent::RedactedThinking {
            index: 0,
            block_index,
            data: "x".repeat(1024 * 1024),
        })
        .collect::<Vec<_>>();
    over_total.push(StreamEvent::RedactedThinking { index: 0, block_index: 4, data: "x".into() });
    let mut state = ResponsesSseState::new(stream_context());
    assert_eq!(
        responses_frames(&over_total, &mut state).unwrap_err().stable_code(),
        "reasoning_replay_invalid"
    );
}

#[test]
fn x5_stream_accepts_an_empty_thinking_block_with_a_nonempty_signature() {
    let mut state = ResponsesSseState::new(stream_context());
    let frames = responses_frames(
        &[
            StreamEvent::ThinkingDelta { index: 0, block_index: 0, thinking_delta: String::new() },
            StreamEvent::ThinkingSignatureDelta {
                index: 0,
                block_index: 0,
                signature_delta: "sig-empty".into(),
            },
            StreamEvent::Done { finish_reason: Some(FinishReason::Stop), stop_sequence: None },
        ],
        &mut state,
    )
    .unwrap();
    let carrier = frames
        .iter()
        .find(|frame| {
            frame.event == "response.output_item.done" && frame.data["item"]["type"] == "reasoning"
        })
        .unwrap()
        .data["item"]["encrypted_content"]
        .as_str()
        .unwrap();
    assert_eq!(
        decode_reasoning_replay_carrier(carrier).unwrap().blocks(),
        &[ReasoningReplayBlock::Thinking {
            thinking: String::new(),
            signature: "sig-empty".into(),
        }]
    );
}
