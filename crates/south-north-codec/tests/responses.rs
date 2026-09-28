use serde_json::{Value, json};
use south_north_codec::{
    responses::{responses_event_json, responses_request_json, responses_response_json},
    *,
};
use token_station_protocol::{
    ChatResponse, Choice, Content, ContentPart, Extensions, FinishReason, Message, Role,
    StreamEvent, ToolCall, Usage,
};
fn context(mode: ResponsesReasoningMode) -> ResponsesContext {
    ResponsesContext {
        response_id: "resp_test".into(),
        model: "m".into(),
        created_at: 123,
        inbound_tools: json!([]),
        reasoning: mode,
        allow_incomplete_tool_calls: false,
        render_legacy_encrypted_reasoning: mode == ResponsesReasoningMode::Summary,
    }
}
fn response() -> ChatResponse {
    ChatResponse {
        id: "upstream".into(),
        model: "upstream-model".into(),
        choices: vec![Choice {
            index: 0,
            message: Message::text(Role::Assistant, "hello"),
            finish_reason: Some(FinishReason::Stop),
            stop_sequence: None,
        }],
        usage: Usage {
            input_tokens: 7,
            output_tokens: 3,
            cache_read_tokens: 2,
            cache_write_tokens: 1,
            reasoning_tokens: 1,
            ..Usage::default()
        },
        extensions: Extensions::new(),
    }
}
#[test]
fn request_preserves_semantic_options_i01_i02_i11_i14_i15_i16_i17_i18() {
    let body = json!({"model":"m","instructions":"obey","input":[{"role":"developer","content":"d"},{"role":"user","content":[{"type":"input_text","text":"hello"}]}],"reasoning":{"effort":"high"},"text":{"format":{"type":"json_schema","name":"schema","schema":{"type":"object"},"strict":true}},"tools":[{"type":"function","name":"f","parameters":{},"strict":true}],"tool_choice":{"type":"function","name":"f"},"parallel_tool_calls":false,"max_output_tokens":25});
    let ir = checked_request(&body, &ResponsesRequestOptions::default()).expect("valid request");
    assert_eq!(ir.messages[0].content, Some(Content::Text("obey".into())));
    assert_eq!(ir.messages[1].role, Role::System);
    assert_eq!(ir.sampling.max_output_tokens, Some(25));
    assert_eq!(ir.extensions["reasoning_effort"], "high");
    assert_eq!(ir.extensions["parallel_tool_calls"], false);
    assert_eq!(ir.extensions["responses_tool_strict"]["f"], true);
    assert!(ir.response_format.is_some());
    let facade: Value = serde_json::from_str(
        &responses_request_json(&body.to_string(), &ResponsesRequestOptions::default()).unwrap(),
    )
    .unwrap();
    assert_eq!(facade, serde_json::to_value(ir).unwrap());
}
#[test]
fn request_compatibility_i03_i04_i05_i06_and_unknown_i20() {
    let options = ResponsesRequestOptions {
        allow_messages: true,
        allow_empty_input: false,
        allow_call_aliases: true,
        preserve_unknown_content: true,
        preserve_text_parts: false,
    };
    let ir=checked_request(&json!({"model":"m","messages":[{"role":"user","content":[{"type":"video_url","video_url":{"url":"x"}}]}]}),&options).unwrap();
    assert!(
        matches!(&ir.messages[0].content,Some(Content::Parts(parts)) if matches!(parts[0],ContentPart::Unknown(_)))
    );
    let ir=checked_request(&json!({"model":"m","input":[{"type":"function_call","id":"c","name":"f","arguments":{"x":1}},{"type":"function_call_output","tool_call_id":"c","output":{"result":2}}]}),&options).unwrap();
    assert_eq!(ir.messages[0].tool_calls[0].arguments, "{\"x\":1}");
    assert_eq!(ir.messages[1].tool_call_id.as_deref(), Some("c"));
    let err =
        checked_request(&json!({"model":"m","input":[{"type":"secret-user-content"}]}), &options)
            .unwrap_err();
    assert_eq!(err.field(), "input[0].type");
    assert!(!err.to_string().contains("secret-user-content"));
}
#[test]
fn community_tool_families_i07_i08_i09_i10_i21_restore_o01() {
    let tools = json!([{"type":"custom","name":"patch","format":{"type":"text"}},{"type":"tool_search"},{"type":"local_shell"},{"type":"namespace","name":"fs","tools":[{"type":"function","name":"read","parameters":{}}]},{"type":"web_search","external_web_access":false}]);
    let ir = checked_request(
        &json!({"model":"m","input":"x","tools":tools}),
        &ResponsesRequestOptions::default(),
    )
    .unwrap();
    assert_eq!(ir.tools.len(), 4);
    assert!(ir.tools.iter().any(|t| t.name == "fs__read"));
    let mut response = response();
    response.choices[0].message.tool_calls = vec![
        ToolCall {
            id: "a".into(),
            name: "patch".into(),
            arguments: "{\"input\":\"patch text\"}".into(),
        },
        ToolCall {
            id: "b".into(),
            name: "tool_search".into(),
            arguments: "{\"query\":\"files\"}".into(),
        },
        ToolCall {
            id: "c".into(),
            name: "__token_station_responses_local_shell".into(),
            arguments: "{\"action\":{\"type\":\"exec\",\"command\":[\"pwd\"]}}".into(),
        },
        ToolCall { id: "d".into(), name: "fs__read".into(), arguments: "{}".into() },
    ];
    let mut ctx = context(ResponsesReasoningMode::Summary);
    ctx.inbound_tools = tools;
    let out = checked_response(&response, &ctx).unwrap();
    assert_eq!(out["output"][1]["type"], "custom_tool_call");
    assert_eq!(out["output"][1]["input"], "patch text");
    assert_eq!(out["output"][2]["type"], "tool_search_call");
    assert_eq!(out["output"][3]["type"], "local_shell_call");
    assert_eq!(out["output"][4]["namespace"], "fs");
}
#[test]
fn reasoning_and_response_context_i12_i13_o02_o03_o04_o05_o06_o07() {
    let ir=checked_request(&json!({"model":"m","input":[{"type":"reasoning","id":"rs1","encrypted_content":"opaque","content":[{"type":"reasoning_text","text":"raw"}],"summary":[]},{"type":"function_call","call_id":"c","name":"f","arguments":"{}"}]}),&ResponsesRequestOptions::default()).unwrap();
    assert_eq!(ir.messages[0].extensions["responses_reasoning_encrypted_content"], "opaque");
    assert_eq!(ir.messages.len(), 1);
    let mut ir = response();
    ir.choices[0].message.content = Some(Content::Parts(vec![
        ContentPart::Thinking { thinking: "think".into(), signature: Some("signature".into()) },
        ContentPart::Text { text: "answer".into() },
    ]));
    ir.choices[0].finish_reason = Some(FinishReason::ContentFilter);
    for mode in [ResponsesReasoningMode::RawContent, ResponsesReasoningMode::Summary] {
        let ctx = context(mode);
        let out = checked_response(&ir, &ctx).unwrap();
        assert_eq!(out["created_at"], 123);
        assert_eq!(out["id"], "resp_test");
        assert_eq!(out["status"], "incomplete");
        assert_eq!(out["incomplete_details"]["reason"], "content_filter");
        assert_eq!(out["usage"]["input_tokens_details"]["cached_tokens"], 2);
        assert_eq!(out["output_text"], "answer");
        if mode == ResponsesReasoningMode::RawContent {
            assert_eq!(out["output"][0]["content"][0]["text"], "think");
            assert!(out["output"][0].get("encrypted_content").is_none());
        } else {
            assert_eq!(out["output"][0]["summary"][0]["text"], "think");
            assert_eq!(out["output"][0]["encrypted_content"], "signature");
        }
        let facade: Value = serde_json::from_str(
            &responses_response_json(&serde_json::to_string(&ir).unwrap(), &ctx).unwrap(),
        )
        .unwrap();
        assert_eq!(facade, out);
    }
}
#[test]
fn stream_lifecycle_indices_usage_and_once_e01_e02_e03_e06_e07_e08() {
    let mut state = ResponsesSseState::new(context(ResponsesReasoningMode::RawContent));
    let events = vec![
        StreamEvent::Usage {
            usage: Usage { input_tokens: 7, cache_read_tokens: 2, ..Usage::default() },
        },
        StreamEvent::Delta { index: 0, content: "hello".into() },
        StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("a".into()),
            name: Some("f".into()),
            arguments_delta: "{".into(),
        },
        StreamEvent::ToolCallDelta {
            index: 1,
            id: Some("b".into()),
            name: Some("g".into()),
            arguments_delta: "{}".into(),
        },
        StreamEvent::ToolCallDelta { index: 0, id: None, name: None, arguments_delta: "}".into() },
        StreamEvent::Finish { finish_reason: Some(FinishReason::Length), stop_sequence: None },
        StreamEvent::Usage { usage: Usage { output_tokens: 4, ..Usage::default() } },
        StreamEvent::Done { finish_reason: None, stop_sequence: None },
    ];
    let frames = checked_frames(&events, &mut state).unwrap();
    assert_eq!(frames[0].event, "response.created");
    for (n, f) in frames.iter().enumerate() {
        assert_eq!(f.data["sequence_number"], n);
    }
    let last = &frames.last().unwrap().data;
    assert_eq!(last["type"], "response.incomplete");
    assert_eq!(last["response"]["usage"]["input_tokens"], 7);
    assert_eq!(last["response"]["usage"]["output_tokens"], 4);
    assert_eq!(last["response"]["output"][1]["arguments"], "{}");
    assert_eq!(last["response"]["output"][2]["call_id"], "b");
    assert!(frames.iter().any(|f| f.event == "response.output_text.done"));
    assert!(state.is_terminated());
    assert!(checked_frames(&events, &mut state).unwrap().is_empty());
}
#[test]
fn raw_reasoning_closes_before_tools_and_json_state_stays_local_e04_e05_e09() {
    let ctx = context(ResponsesReasoningMode::RawContent);
    let mut state = ResponsesSseState::new(ctx.clone());
    let mut json_state = ResponsesSseState::new(ctx);
    let events = [
        StreamEvent::ThinkingDelta { index: 0, thinking_delta: "think".into() },
        StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("c".into()),
            name: Some("f".into()),
            arguments_delta: "{}".into(),
        },
        StreamEvent::Done { finish_reason: Some(FinishReason::ToolCalls), stop_sequence: None },
    ];
    let mut all = Vec::new();
    for event in events {
        let frames = checked_frames(std::slice::from_ref(&event), &mut state).unwrap();
        let output: Value = serde_json::from_str(
            &responses_event_json(&serde_json::to_string(&event).unwrap(), &mut json_state)
                .unwrap(),
        )
        .unwrap();
        let expected = frames.iter().fold(String::new(), |mut output, frame| {
            use std::fmt::Write;
            write!(output, "event: {}\ndata: {}\n\n", frame.event, frame.data).unwrap();
            output
        });
        assert_eq!(output["data"], expected);
        all.extend(frames);
    }
    let done = all.iter().position(|f| f.event == "response.reasoning_text.done").unwrap();
    let tool = all.iter().position(|f| f.data["item"]["type"] == "function_call").unwrap();
    assert!(done < tool);
}
#[test]
fn malformed_fields_report_paths_without_payload_i15_i16_i17_i19_i20() {
    for (patch, path) in [
        (json!({"input":false}), "input"),
        (json!({"reasoning":{"effort":7}}), "reasoning.effort"),
        (json!({"parallel_tool_calls":"secret"}), "parallel_tool_calls"),
        (json!({"text":{"format":{"type":"json_schema","schema":true}}}), "text.format.schema"),
        (json!({"max_output_tokens":4_294_967_296_u64}), "max_output_tokens"),
        (json!({"previous_response_id":""}), "previous_response_id"),
    ] {
        let mut body = json!({"model":"m","input":"hello"});
        body.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
        let error = checked_request(&body, &ResponsesRequestOptions::default()).unwrap_err();
        assert_eq!(error.field(), path);
        assert!(!error.to_string().contains("secret"));
    }
}
#[test]
fn replays_all_client_tool_items_without_losing_results_i07_i08_i09_i10() {
    let input = json!([{"type":"custom_tool_call","call_id":"c","name":"patch","input":"a\nb"},{"type":"custom_tool_call_output","call_id":"c","output":"ok"},{"type":"tool_search_call","id":"s","arguments":{"query":"a"}},{"type":"tool_search_output","call_id":"s","output":"found"},{"type":"local_shell_call","call_id":"l","action":{"type":"exec","command":["pwd"]}},{"type":"local_shell_call_output","id":"l","output":"/tmp"},{"type":"function_call","call_id":"n","name":"read","namespace":"fs","arguments":"{}"}]);
    let ir =
        checked_request(&json!({"model":"m","input":input}), &ResponsesRequestOptions::default())
            .unwrap();
    assert_eq!(ir.messages.len(), 7);
    assert_eq!(ir.messages[0].tool_calls[0].arguments, "{\"input\":\"a\\nb\"}");
    assert_eq!(ir.messages[2].tool_calls[0].name, "tool_search");
    assert_eq!(ir.messages[4].tool_calls[0].name, "__token_station_responses_local_shell");
    assert_eq!(ir.messages[6].tool_calls[0].name, "fs__read");
}
#[test]
fn errors_are_terminal_and_never_leak_between_streams_e08_e09() {
    use token_station_protocol::{ErrorCode, ErrorEnvelope};
    let mut a = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
    let mut ctx = context(ResponsesReasoningMode::Summary);
    ctx.response_id = "resp_other".into();
    let mut b = ResponsesSseState::new(ctx);
    let frames = checked_frames(
        &[StreamEvent::Error { error: ErrorEnvelope::new(ErrorCode::Internal, 500, "failure") }],
        &mut a,
    )
    .unwrap();
    assert_eq!(frames.last().unwrap().event, "response.failed");
    assert!(
        checked_frames(&[StreamEvent::Done { finish_reason: None, stop_sequence: None }], &mut a)
            .unwrap()
            .is_empty()
    );
    let frames =
        checked_frames(&[StreamEvent::Delta { index: 0, content: "other".into() }], &mut b)
            .unwrap();
    assert_eq!(frames[0].data["response"]["id"], "resp_other");
    assert_eq!(frames[0].data["sequence_number"], 0);
}
#[test]
fn namespace_descriptions_and_collisions_i10_i11() {
    let tools = json!([{"type":"namespace","name":"fs","description":"File tools","tools":[{"type":"function","name":"read","description":"Read file","strict":true}]}]);
    let ir = checked_request(
        &json!({"model":"m","input":"x","tools":tools}),
        &ResponsesRequestOptions::default(),
    )
    .unwrap();
    assert_eq!(ir.tools[0].description.as_deref(), Some("File tools\n\nRead file"));
    assert_eq!(ir.extensions["responses_tool_strict"]["fs__read"], true);
    let result = checked_request(
        &json!({"model":"m","input":"x","tools":[{"type":"function","name":"fs__read"},{"type":"namespace","name":"fs","tools":[{"type":"function","name":"read"}]}]}),
        &ResponsesRequestOptions::default(),
    );
    assert!(result.is_err());
}
#[test]
fn tool_streams_restore_custom_search_shell_namespace_e04() {
    let mut ctx = context(ResponsesReasoningMode::Summary);
    ctx.inbound_tools = json!([{"type":"custom","name":"patch"},{"type":"tool_search"},{"type":"local_shell"},{"type":"namespace","name":"fs","tools":[{"type":"function","name":"read"}]}]);
    let mut state = ResponsesSseState::new(ctx);
    let calls = [
        ("patch", "{\"input\":\"patch text\"}"),
        ("tool_search", "{\"query\":\"files\"}"),
        (
            "__token_station_responses_local_shell",
            "{\"action\":{\"type\":\"exec\",\"command\":[\"pwd\"]}}",
        ),
        ("fs__read", "{}"),
    ];
    let mut events = Vec::new();
    for (index, (name, args)) in calls.iter().enumerate() {
        events.push(StreamEvent::ToolCallDelta {
            index: u32::try_from(index).unwrap(),
            id: Some(format!("c{index}")),
            name: Some((*name).into()),
            arguments_delta: (*args).into(),
        });
    }
    events.push(StreamEvent::Done {
        finish_reason: Some(FinishReason::ToolCalls),
        stop_sequence: None,
    });
    let frames = checked_frames(&events, &mut state).unwrap();
    let output = &frames.last().unwrap().data["response"]["output"];
    assert_eq!(output[0]["input"], "patch text");
    assert_eq!(output[1]["arguments"]["query"], "files");
    assert_eq!(output[2]["action"]["command"][0], "pwd");
    assert_eq!(output[3]["namespace"], "fs");
    assert!(frames.iter().any(|f| f.event == "response.custom_tool_call_input.done"));
}
#[test]
fn missing_identity_and_signature_compatibility_are_independent_e04_o04() {
    let fragment =
        StreamEvent::ToolCallDelta { index: 0, id: None, name: None, arguments_delta: "{".into() };
    let mut strict = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
    assert!(checked_frames(std::slice::from_ref(&fragment), &mut strict).is_err());
    let mut ctx = context(ResponsesReasoningMode::Summary);
    ctx.allow_incomplete_tool_calls = true;
    ctx.render_legacy_encrypted_reasoning = false;
    let mut tolerant = ResponsesSseState::new(ctx);
    let frames = checked_frames(
        &[
            fragment,
            StreamEvent::ToolCallDelta {
                index: 0,
                id: None,
                name: Some("f".into()),
                arguments_delta: "}".into(),
            },
            StreamEvent::ThinkingDelta { index: 0, thinking_delta: "think".into() },
            StreamEvent::ThinkingSignatureDelta {
                index: 0,
                signature_delta: "secret-signature".into(),
            },
            StreamEvent::Done { finish_reason: Some(FinishReason::Stop), stop_sequence: None },
        ],
        &mut tolerant,
    )
    .unwrap();
    assert_eq!(frames.last().unwrap().data["response"]["output"][0]["name"], "f");
    assert!(!format!("{frames:?}").contains("secret-signature"));
}
#[test]
fn image_inputs_and_invalid_roles_preserve_only_declared_compatibility_i02_i04_i20() {
    let body = json!({"model":"m","input":[{"role":"user","content":[{"type":"input_image","image_url":"https://example.test/a","detail":"high"}]}]});
    let ir = checked_request(&body, &ResponsesRequestOptions::default()).unwrap();
    assert!(
        matches!(&ir.messages[0].content,Some(Content::Parts(parts)) if matches!(&parts[0],ContentPart::ImageUrl{image_url} if image_url.detail.as_deref()==Some("high")))
    );
    for input in [
        json!([{"type":"message"}]),
        json!([{"role":"intruder","content":"x"}]),
        json!([{"role":"user","content":[{"type":"input_image","file_id":"file_a"}]}]),
    ] {
        assert!(
            checked_request(
                &json!({"model":"m","input":input}),
                &ResponsesRequestOptions::default()
            )
            .is_err()
        );
    }
}
use proptest::prelude::*;
fn arbitrary_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        ".*".prop_map(Value::String)
    ];
    leaf.prop_recursive(4, 64, 8, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..8).prop_map(Value::Array),
            proptest::collection::btree_map("[a-z_]{0,15}", inner, 0..8)
                .prop_map(|map| Value::Object(map.into_iter().collect()))
        ]
    })
}
proptest! {
 #![proptest_config(ProptestConfig::with_cases(32))]
 #[test]
 fn untrusted_input_never_panics_i20(body in arbitrary_json()) {let _=checked_request(&body,&ResponsesRequestOptions::default());let _=checked_request(&json!({"model":"m","input":body}),&ResponsesRequestOptions::default());let _=checked_request(&json!({"model":"m","input":"x","tools":body}),&ResponsesRequestOptions::default());let _=checked_request(&json!({"model":"m","input":[{"role":"user","content":body}]}),&ResponsesRequestOptions::default());let _=checked_request(&body,&ResponsesRequestOptions{allow_messages:true,allow_empty_input:false,allow_call_aliases:true,preserve_unknown_content:true,preserve_text_parts:false});}
 #[test]
 fn stream_batching_and_isolation_e09(chunks in proptest::collection::vec(".{0,30}",0..10)) {let ctx=context(ResponsesReasoningMode::RawContent);let mut whole=ResponsesSseState::new(ctx.clone());let mut split=ResponsesSseState::new(ctx);let mut unrelated=ResponsesSseState::new(context(ResponsesReasoningMode::Summary));let mut events=chunks.iter().map(|content|StreamEvent::Delta{index:0,content:content.clone()}).collect::<Vec<_>>();events.push(StreamEvent::Done{finish_reason:Some(FinishReason::Stop),stop_sequence:None});let expected=checked_frames(&events,&mut whole).unwrap();let mut actual=Vec::new();for event in &events{actual.extend(checked_frames(std::slice::from_ref(event),&mut split).unwrap());let _=checked_frames(&[StreamEvent::Delta{index:0,content:"unrelated".into()}],&mut unrelated).unwrap();}prop_assert_eq!(expected,actual);}
}
#[test]
fn continuation_snapshot_is_only_available_after_success_i19_e09() {
    let mut state = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
    assert!(state.terminal_response().is_none());
    checked_frames(
        &[
            StreamEvent::ThinkingDelta { index: 0, thinking_delta: "reason".into() },
            StreamEvent::ThinkingSignatureDelta { index: 0, signature_delta: "sig".into() },
            StreamEvent::Delta { index: 0, content: "answer".into() },
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("c".into()),
                name: Some("fs__read".into()),
                arguments_delta: "{}".into(),
            },
            StreamEvent::Usage { usage: Usage { input_tokens: 3, ..Usage::default() } },
            StreamEvent::Done { finish_reason: Some(FinishReason::ToolCalls), stop_sequence: None },
        ],
        &mut state,
    )
    .unwrap();
    let terminal = state.terminal_response().expect("completed canonical snapshot");
    assert_eq!(terminal.choices.len(), 1);
    assert_eq!(terminal.choices[0].message.tool_calls[0].name, "fs__read");
    assert_eq!(terminal.usage.input_tokens, 3);
    assert!(
        matches!(&terminal.choices[0].message.content,Some(Content::Parts(parts)) if matches!(&parts[0],ContentPart::Thinking{signature:Some(signature),..} if signature=="sig"))
    );
    let mut failed = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
    checked_frames(
        &[StreamEvent::Error {
            error: token_station_protocol::ErrorEnvelope::new(
                token_station_protocol::ErrorCode::Internal,
                500,
                "failed",
            ),
        }],
        &mut failed,
    )
    .unwrap();
    assert!(failed.terminal_response().is_none());
}
#[test]
fn overflowing_usage_is_refused_without_panic_o05_e06() {
    let mut ir = response();
    ir.usage.input_tokens = u64::MAX;
    ir.usage.output_tokens = 1;
    assert!(checked_response(&ir, &context(ResponsesReasoningMode::Summary)).is_err());
    let mut state = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
    assert!(
        checked_frames(
            &[
                StreamEvent::Usage { usage: ir.usage },
                StreamEvent::Done { finish_reason: None, stop_sequence: None }
            ],
            &mut state
        )
        .is_err()
    );
}
#[test]
fn explicit_message_reports_missing_role_and_malformed_reasoning_content_i02_i12_i20() {
    let error = checked_request(
        &json!({"model":"m","input":[{"type":"message","content":"x"}]}),
        &ResponsesRequestOptions::default(),
    )
    .unwrap_err();
    assert_eq!(error.field(), "input[0].role");
    let error = checked_request(
        &json!({"model":"m","input":[{"type":"reasoning","content":true,"summary":[]}]}),
        &ResponsesRequestOptions::default(),
    )
    .unwrap_err();
    assert_eq!(error.field(), "input[0].content");
}
#[test]
fn unsupported_capabilities_remain_distinct_from_malformed_shapes_i04_i10_i20_i21() {
    for patch in [
        json!({"tools":[{"type":"web_search"}]}),
        json!({"tools":[{"type":"namespace","name":"n","tools":[{"type":"custom","name":"c"}]}]}),
        json!({"input":[{"role":"user","content":[{"type":"input_image","file_id":"f"}]}]}),
    ] {
        let mut body = json!({"model":"m","input":"x"});
        body.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
        assert!(matches!(
            checked_request(&body, &ResponsesRequestOptions::default()),
            Err(CodecError::UnknownValue { .. })
        ));
    }
    assert!(matches!(
        checked_request(
            &json!({"model":"m","input":"x","tools":{}}),
            &ResponsesRequestOptions::default()
        ),
        Err(CodecError::Unrenderable { .. })
    ));
}
#[test]
fn malformed_local_shell_preserves_host_diagnostic_contract() {
    let mut state = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
    let error = checked_frames(
        &[
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("c".into()),
                name: Some("__token_station_responses_local_shell".into()),
                arguments_delta: "invalid-json".into(),
            },
            StreamEvent::Done { finish_reason: Some(FinishReason::ToolCalls), stop_sequence: None },
        ],
        &mut state,
    )
    .unwrap_err();
    assert_eq!(error.field(), "tool_calls.arguments");
    assert!(error.to_string().contains("invalid arguments"));
    assert!(state.terminal_response().is_none());
}

// Every fixture exercises the typed and JSON entry points with identical inputs.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "Mirrors the public typed and JSON API signatures."
)]
fn checked_request(
    body: &Value,
    options: &ResponsesRequestOptions,
) -> Result<token_station_protocol::ChatRequest, CodecError> {
    let typed = south_north_codec::chat_request_from_responses(body, options);
    let json = responses_request_json(&body.to_string(), options);
    match (&typed, &json) {
        (Ok(ir), Ok(json)) => assert_eq!(
            serde_json::to_value(ir).unwrap(),
            serde_json::from_str::<Value>(json).unwrap()
        ),
        (Err(typed), Err(json)) => assert_eq!(typed, json),
        _ => panic!("typed/JSON request results differ"),
    }
    typed
}
fn checked_response(ir: &ChatResponse, context: &ResponsesContext) -> Result<Value, CodecError> {
    let typed = south_north_codec::responses_response(ir, context);
    let json = responses_response_json(&serde_json::to_string(ir).unwrap(), context);
    match (&typed, &json) {
        (Ok(wire), Ok(json)) => assert_eq!(*wire, serde_json::from_str::<Value>(json).unwrap()),
        (Err(typed), Err(json)) => assert_eq!(typed, json),
        _ => panic!("typed/JSON response results differ"),
    }
    typed
}
fn checked_frames(
    events: &[StreamEvent],
    state: &mut ResponsesSseState,
) -> Result<Vec<ResponsesFrame>, CodecError> {
    let mut json_state = state.clone();
    let typed = south_north_codec::responses_frames(events, state);
    let json: Result<Vec<String>, CodecError> = events
        .iter()
        .map(|event| responses_event_json(&serde_json::to_string(event).unwrap(), &mut json_state))
        .collect();
    match (&typed, &json) {
        (Ok(frames), Ok(outputs)) => {
            use std::fmt::Write;
            let wire = frames.iter().fold(String::new(), |mut wire, frame| {
                write!(wire, "event: {}\ndata: {}\n\n", frame.event, frame.data).unwrap();
                wire
            });
            let json_wire = outputs
                .iter()
                .map(|output| {
                    serde_json::from_str::<Value>(output).unwrap()["data"]
                        .as_str()
                        .unwrap()
                        .to_owned()
                })
                .collect::<String>();
            assert_eq!(wire, json_wire);
        }
        (Err(typed), Err(json)) => assert_eq!(typed, json),
        _ => panic!("typed/JSON event results differ"),
    }
    assert_eq!(state.is_terminated(), json_state.is_terminated());
    assert_eq!(state.terminal_response(), json_state.terminal_response());
    typed
}
#[test]
fn string_history_empty_input_and_instruction_scope_i01_i18_i19() {
    let ir=checked_request(&json!({"model":"m","input":"","instructions":"current only","previous_response_id":"resp_previous"}),&ResponsesRequestOptions::default()).unwrap();
    assert_eq!(ir.messages.len(), 2);
    assert_eq!(ir.messages[1].content, Some(Content::Text(String::new())));
    assert_eq!(ir.messages[0].extensions["responses_transient_instructions"], true);
    assert!(!ir.extensions.contains_key("previous_response_id"));
    // Parsing never fetches history; a host has to supply already-resolved turns.
    assert!(
        checked_request(
            &json!({"model":"m","input":[],"previous_response_id":"resp_previous"}),
            &ResponsesRequestOptions::default()
        )
        .is_err()
    );
    assert_eq!(
        checked_request(
            &json!({"model":"m","input":"x","instructions":7}),
            &ResponsesRequestOptions::default()
        )
        .unwrap_err()
        .field(),
        "instructions"
    );
}
#[test]
fn sampling_formats_and_selection_are_preserved_i15_i16_i17() {
    let options =
        ResponsesRequestOptions { allow_call_aliases: true, ..ResponsesRequestOptions::default() };
    for (format, expected) in [
        (json!({"type":"text"}), json!({"type":"text"})),
        (json!({"type":"json_object"}), json!({"type":"json_object"})),
    ] {
        let ir=checked_request(&json!({"model":"m","input":"x","text":{"format":format},"max_output_tokens":12,"max_tokens":99,"temperature":0.4,"top_p":0.8,"stop":["end","stop"],"stream":true,"tool_choice":"none"}),&options).unwrap();
        assert_eq!(serde_json::to_value(ir.response_format).unwrap(), expected);
        assert_eq!(ir.sampling.max_output_tokens, Some(12));
        assert_eq!(ir.sampling.stop, vec!["end", "stop"]);
        assert!(ir.stream);
        assert!(ir.tool_choice.is_none());
    }
    let ir=checked_request(&json!({"model":"m","input":"x","max_tokens":8,"stop":"end","tools":[{"type":"function","name":"f"}],"tool_choice":"required"}),&options).unwrap();
    assert_eq!(ir.sampling.max_output_tokens, Some(8));
    assert_eq!(ir.sampling.stop, vec!["end"]);
    assert_eq!(ir.tool_choice, Some(token_station_protocol::ToolChoice::Required));
    assert_eq!(
        checked_request(&json!({"model":"m","input":"x","tool_choice":"required"}), &options)
            .unwrap_err()
            .field(),
        "tool_choice"
    );
}
#[test]
fn output_items_keep_all_choices_ids_arguments_and_usage_o01_o02_o05_o06_o07() {
    let mut ir = response();
    ir.choices[0].message.tool_calls.push(ToolCall {
        id: "call_a".into(),
        name: "f".into(),
        arguments: "{ \"x\" : 1 }".into(),
    });
    ir.choices.push(Choice {
        index: 1,
        message: Message::text(Role::Assistant, "second"),
        finish_reason: Some(FinishReason::Other("provider_stop".into())),
        stop_sequence: None,
    });
    let output = checked_response(&ir, &context(ResponsesReasoningMode::RawContent)).unwrap();
    assert_eq!(output["output"].as_array().unwrap().len(), 3);
    assert_eq!(output["output"][1]["arguments"], "{ \"x\" : 1 }");
    assert_eq!(output["output"][1]["call_id"], "call_a");
    assert_eq!(output["output"][2]["id"], "msg_resp_test_1");
    assert_eq!(output["output_text"], "hellosecond");
    assert_eq!(output["status"], "completed");
    assert!(output["incomplete_details"].is_null());
    assert_eq!(output["usage"]["input_tokens_details"]["cache_write_tokens"], 1);
    assert_eq!(output["usage"]["output_tokens_details"]["reasoning_tokens"], 1);
    assert_eq!(output["usage"]["total_tokens"], 10);
    for key in ["tools", "tool_choice", "parallel_tool_calls", "instructions"] {
        assert!(output.get(key).is_none());
    }
}
#[test]
fn finish_waits_for_done_and_identity_mutations_fail_e04_e06_e08() {
    let mut state = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
    let before = checked_frames(
        &[
            StreamEvent::Delta { index: 0, content: "partial".into() },
            StreamEvent::Finish { finish_reason: Some(FinishReason::Stop), stop_sequence: None },
        ],
        &mut state,
    )
    .unwrap();
    assert!(!state.is_terminated());
    assert!(before.iter().all(|frame| frame.event != "response.completed"));
    let after = checked_frames(
        &[
            StreamEvent::Usage { usage: Usage { output_tokens: 9, ..Usage::default() } },
            StreamEvent::Done { finish_reason: None, stop_sequence: None },
        ],
        &mut state,
    )
    .unwrap();
    assert_eq!(after.last().unwrap().data["response"]["usage"]["output_tokens"], 9);
    assert!(
        checked_frames(
            &[StreamEvent::Error {
                error: token_station_protocol::ErrorEnvelope::new(
                    token_station_protocol::ErrorCode::Internal,
                    500,
                    "late error"
                )
            }],
            &mut state
        )
        .unwrap()
        .is_empty()
    );
    let mut state = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
    checked_frames(
        &[StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("a".into()),
            name: Some("f".into()),
            arguments_delta: "{".into(),
        }],
        &mut state,
    )
    .unwrap();
    let error = checked_frames(
        &[StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("b".into()),
            name: None,
            arguments_delta: "}".into(),
        }],
        &mut state,
    )
    .unwrap_err();
    assert_eq!(error.field(), "tool_call.id");
    assert!(!state.is_terminated());
}
#[test]
fn reasoning_block_order_and_summary_lifecycle_e01_e03_e05() {
    let mut raw = ResponsesSseState::new(context(ResponsesReasoningMode::RawContent));
    let frames = checked_frames(
        &[
            StreamEvent::ThinkingDelta { index: 0, thinking_delta: "first".into() },
            StreamEvent::Delta { index: 0, content: "answer".into() },
            StreamEvent::ThinkingDelta { index: 0, thinking_delta: "second".into() },
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("c".into()),
                name: Some("f".into()),
                arguments_delta: "{}".into(),
            },
            StreamEvent::Done { finish_reason: Some(FinishReason::ToolCalls), stop_sequence: None },
        ],
        &mut raw,
    )
    .unwrap();
    let output = &frames.last().unwrap().data["response"]["output"];
    assert_eq!(output[0]["content"][0]["text"], "first");
    assert_eq!(output[1]["content"][0]["text"], "answer");
    assert_eq!(output[2]["content"][0]["text"], "second");
    assert_ne!(output[0]["id"], output[2]["id"]);
    assert_eq!(output[3]["type"], "function_call");
    let mut summary = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
    let frames = checked_frames(
        &[
            StreamEvent::ThinkingDelta { index: 0, thinking_delta: "summary".into() },
            StreamEvent::Done { finish_reason: Some(FinishReason::Stop), stop_sequence: None },
        ],
        &mut summary,
    )
    .unwrap();
    let kinds = frames.iter().map(|frame| frame.event.as_str()).collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            "response.created",
            "response.output_item.added",
            "response.reasoning_summary_part.added",
            "response.reasoning_summary_text.delta",
            "response.reasoning_summary_text.done",
            "response.reasoning_summary_part.done",
            "response.output_item.done",
            "response.completed"
        ]
    );
}
#[test]
fn malformed_json_facades_return_typed_errors() {
    assert_eq!(
        responses_request_json("{", &ResponsesRequestOptions::default()).unwrap_err().field(),
        "request"
    );
    assert_eq!(
        responses_response_json("{", &context(ResponsesReasoningMode::Summary))
            .unwrap_err()
            .field(),
        "response"
    );
    let mut state = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
    assert_eq!(responses_event_json("{", &mut state).unwrap_err().field(), "event");
}

#[test]
fn empty_input_is_a_separate_host_allowance_i01_i19() {
    let body = json!({"model":"m","input":[],"previous_response_id":"resp_previous"});
    assert!(checked_request(&body, &ResponsesRequestOptions::default()).is_err());
    let options =
        ResponsesRequestOptions { allow_empty_input: true, ..ResponsesRequestOptions::default() };
    let parsed =
        checked_request(&body, &options).expect("continuation host accepts empty current input");
    assert!(parsed.messages.is_empty());
    let parsed =
        checked_request(&json!({"model":"m","input":[],"instructions":"current"}), &options)
            .unwrap();
    assert_eq!(parsed.messages.len(), 1);
    assert_eq!(parsed.messages[0].extensions["responses_transient_instructions"], true);
}
#[test]
fn legacy_messages_precedence_and_file_image_shape_i03_i04() {
    let options = ResponsesRequestOptions {
        allow_messages: true,
        preserve_unknown_content: true,
        ..ResponsesRequestOptions::default()
    };
    let ir=checked_request(&json!({"model":"m","input":"ignored","messages":[{"role":"user","content":[{"type":"input_image","file_id":"file_1","detail":"high"}]}]}),&options).unwrap();
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(
        ir.messages[0].content,
        Some(Content::Parts(vec![ContentPart::Unknown(
            json!({"type":"image_url","image_url":{"file_id":"file_1","detail":"high"}})
        )]))
    );
}

#[test]
fn text_parts_shape_is_an_independent_compatibility_choice_i04() {
    let body = json!({"model":"m","input":[{"role":"assistant","content":[{"type":"output_text","text":"answer"}]}]});
    let parts = checked_request(
        &body,
        &ResponsesRequestOptions {
            preserve_text_parts: true,
            ..ResponsesRequestOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        parts.messages[0].content,
        Some(Content::Parts(vec![ContentPart::Text { text: "answer".into() }]))
    );
    let text = checked_request(&body, &ResponsesRequestOptions::default()).unwrap();
    assert_eq!(text.messages[0].content, Some(Content::Text("answer".into())));
}
#[test]
fn legacy_function_results_stringify_every_json_kind_i06() {
    let options =
        ResponsesRequestOptions { allow_call_aliases: true, ..ResponsesRequestOptions::default() };
    for value in [json!(7), json!(true), Value::Null, json!([1, 2]), json!({"x":1}), json!("text")]
    {
        let request=checked_request(&json!({"model":"m","input":[{"type":"function_call_output","call_id":"c","output":value}]}),&options).unwrap();
        let expected = value.as_str().map_or_else(|| value.to_string(), str::to_owned);
        assert_eq!(request.messages[0].content, Some(Content::Text(expected)));
    }
    let request = checked_request(
        &json!({"model":"m","input":[{"type":"function_call_output","call_id":"c"}]}),
        &options,
    )
    .unwrap();
    assert_eq!(request.messages[0].content, Some(Content::Text(String::new())));
}
#[test]
fn tool_search_parameter_events_reference_an_announced_item_e03_e04() {
    let mut ctx = context(ResponsesReasoningMode::Summary);
    ctx.inbound_tools = json!([{"type":"tool_search"}]);
    let mut state = ResponsesSseState::new(ctx);
    let frames = checked_frames(
        &[
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("search".into()),
                name: Some("tool_search".into()),
                arguments_delta: "{}".into(),
            },
            StreamEvent::Done { finish_reason: Some(FinishReason::ToolCalls), stop_sequence: None },
        ],
        &mut state,
    )
    .unwrap();
    let added = &frames.iter().find(|f| f.event == "response.output_item.added").unwrap().data["item"]
        ["id"];
    assert!(added.is_string());
    for frame in &frames {
        if let Some(id) = frame.data.get("item_id") {
            assert_eq!(id, added);
        }
    }
    assert_eq!(&frames.last().unwrap().data["response"]["output"][0]["id"], added);
}
#[test]
fn invalid_roles_and_unsupported_selection_keep_distinct_categories_i02_i16() {
    assert!(matches!(
        checked_request(
            &json!({"model":"m","input":[{"role":"invalid","content":"x"}]}),
            &ResponsesRequestOptions::default()
        ),
        Err(CodecError::Unrenderable { .. })
    ));
    assert!(matches!(
        checked_request(
            &json!({"model":"m","input":"x","tool_choice":"unsupported"}),
            &ResponsesRequestOptions::default()
        ),
        Err(CodecError::UnknownValue { .. })
    ));
}

#[test]
fn render_error_allows_one_explicit_failed_terminal_without_sequence_gaps_e08() {
    for established in [false, true] {
        let mut state = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
        let mut visible = Vec::new();
        if established {
            visible.extend(
                checked_frames(
                    &[StreamEvent::Delta { index: 0, content: "hello".into() }],
                    &mut state,
                )
                .unwrap(),
            );
        }
        let malformed = StreamEvent::ToolCallDelta {
            index: 1,
            id: None,
            name: None,
            arguments_delta: "{".into(),
        };
        assert!(checked_frames(&[malformed], &mut state).is_err());
        assert!(!state.is_terminated());
        assert!(
            checked_frames(
                &[
                    StreamEvent::Delta { index: 0, content: "ignored".into() },
                    StreamEvent::Done { finish_reason: None, stop_sequence: None }
                ],
                &mut state
            )
            .unwrap()
            .is_empty()
        );
        let failure = StreamEvent::Error {
            error: token_station_protocol::ErrorEnvelope::new(
                token_station_protocol::ErrorCode::Internal,
                500,
                "invalid arguments",
            ),
        };
        visible.extend(checked_frames(std::slice::from_ref(&failure), &mut state).unwrap());
        assert_eq!(visible.iter().filter(|frame| frame.event == "response.created").count(), 1);
        assert_eq!(visible.last().unwrap().event, "response.failed");
        for (sequence, frame) in visible.iter().enumerate() {
            assert_eq!(frame.data["sequence_number"], sequence);
        }
        assert!(state.is_terminated());
        assert!(state.terminal_response().is_none());
        assert!(checked_frames(&[failure], &mut state).unwrap().is_empty());
    }
}

#[test]
fn failed_frames_use_client_error_codes_e08() {
    use token_station_protocol::{ErrorCode, ErrorEnvelope};
    for (code, expected) in [
        (ErrorCode::InvalidRequest, "invalid_request"),
        (ErrorCode::Auth, "authentication_error"),
        (ErrorCode::PaymentRequired, "insufficient_quota"),
        (ErrorCode::RateLimit, "rate_limit_exceeded"),
        (ErrorCode::Capacity, "server_overloaded"),
        (ErrorCode::Capability, "unsupported_capability"),
        (ErrorCode::ContextLength, "context_length_exceeded"),
        (ErrorCode::ContentPolicy, "invalid_prompt"),
        (ErrorCode::UpstreamUnavailable, "server_error"),
        (ErrorCode::TransportTruncated, "server_error"),
        (ErrorCode::ProviderProtocolError, "upstream_protocol_error"),
        (ErrorCode::Timeout, "timeout"),
        (ErrorCode::Internal, "internal_error"),
    ] {
        let mut state = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
        let frames = checked_frames(
            &[StreamEvent::Error { error: ErrorEnvelope::new(code, 500, "failure") }],
            &mut state,
        )
        .unwrap();
        assert_eq!(frames.last().unwrap().data["response"]["error"]["code"], expected);
    }
}

#[test]
fn known_hosted_tool_names_remain_diagnostic_while_unknown_values_are_redacted_i21() {
    for name in [
        "web_search",
        "web_search_preview",
        "file_search",
        "code_interpreter",
        "image_generation",
        "computer_use_preview",
        "mcp",
    ] {
        let error = checked_request(
            &json!({"model":"m","input":"x","tools":[{"type":name}]}),
            &ResponsesRequestOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(error, CodecError::UnknownValue { .. }));
        assert!(error.to_string().contains(name), "{error}");
        assert!(!error.to_string().contains("local_shell"), "{error}");
    }
    let secret = "private-client-tool-credential";
    let error = checked_request(
        &json!({"model":"m","input":"x","tools":[{"type":secret}]}),
        &ResponsesRequestOptions::default(),
    )
    .unwrap_err();
    assert!(!error.to_string().contains(secret));
}

#[test]
fn empty_batch_explicitly_starts_once_and_preserves_terminal_sequence_e01_e08() {
    use token_station_protocol::{ErrorCode, ErrorEnvelope};
    for failed in [false, true] {
        let mut state = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
        let mut visible = responses_frames(&[], &mut state).unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].event, "response.created");
        assert_eq!(visible[0].data["sequence_number"], 0);
        assert!(responses_frames(&[], &mut state).unwrap().is_empty());
        visible.extend(
            checked_frames(&[StreamEvent::Delta { index: 0, content: "hi".into() }], &mut state)
                .unwrap(),
        );
        let terminal = if failed {
            StreamEvent::Error { error: ErrorEnvelope::new(ErrorCode::Internal, 500, "failed") }
        } else {
            StreamEvent::Done { finish_reason: None, stop_sequence: None }
        };
        visible.extend(checked_frames(std::slice::from_ref(&terminal), &mut state).unwrap());
        assert_eq!(visible.iter().filter(|frame| frame.event == "response.created").count(), 1);
        for (sequence, frame) in visible.iter().enumerate() {
            assert_eq!(frame.data["sequence_number"], sequence);
        }
        assert!(responses_frames(&[], &mut state).unwrap().is_empty());
        assert!(checked_frames(&[terminal], &mut state).unwrap().is_empty());
    }
    let mut state = ResponsesSseState::new(context(ResponsesReasoningMode::Summary));
    assert!(
        checked_frames(
            &[StreamEvent::ToolCallDelta {
                index: 0,
                id: None,
                name: None,
                arguments_delta: String::new()
            }],
            &mut state
        )
        .is_err()
    );
    assert!(responses_frames(&[], &mut state).unwrap().is_empty());
}

#[test]
fn malformed_text_parts_preserve_only_legacy_unknown_compatibility_i04() {
    for part in [
        json!({"type":"input_text"}),
        json!({"type":"input_text","text":42}),
        json!({"type":"input_text","text":null}),
    ] {
        let body = json!({"model":"m","input":[{"role":"user","content":[part]}]});
        assert!(checked_request(&body, &ResponsesRequestOptions::default()).is_err());
        let ir = checked_request(
            &body,
            &ResponsesRequestOptions { preserve_unknown_content: true, ..Default::default() },
        )
        .unwrap();
        assert_eq!(ir.messages[0].content, Some(Content::Parts(vec![ContentPart::Unknown(part)])));
    }
}
