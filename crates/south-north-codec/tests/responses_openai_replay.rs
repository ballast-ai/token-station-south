//! NC-1: `OpenAI` reasoning items survive the north codec in order and with their position
//! (`docs/design/2026-10-11-responses-reasoning-replay-and-passthrough-suite.md` §2.1, §2.2).

use proptest::prelude::*;
use serde_json::{Value, json};
use south_north_codec::responses::{
    CLAUDE_REASONING_REPLAY_CAPABILITY, OPENAI_REASONING_ITEMS_EXTENSION,
    OPENAI_REASONING_REPLAY_CAPABILITY, OPENAI_REASONING_REPLAY_FAMILY, ReasoningReplayBlock,
    ReasoningReplayCarrier, encode_reasoning_replay_carrier,
};
use south_north_codec::{CodecError, ResponsesRequestOptions, chat_request_from_responses};
use token_station_protocol::{ChatRequest, Content, ContentPart, Message};

fn parse(input: Value) -> ChatRequest {
    let mut body = json!({"model": "m"});
    body["input"] = input;
    chat_request_from_responses(&body, &ResponsesRequestOptions::default()).expect("valid request")
}

fn reasoning(id: &str, encrypted: &str) -> Value {
    json!({
        "type": "reasoning",
        "id": id,
        "summary": [{"type": "summary_text", "text": format!("summary of {id}")}],
        "encrypted_content": encrypted,
    })
}

/// The defect NC-1 fixes: two reasoning items in one assistant run used to leave only the
/// second item's `id` and `encrypted_content`, in two single-valued extensions, with no position.
#[test]
fn nc1_keeps_every_opaque_reasoning_item_of_a_run_in_order() {
    let request = parse(json!([
        {"role": "user", "content": "go"},
        reasoning("rs_1", "opaque-one"),
        {"type": "function_call", "call_id": "c1", "name": "lookup", "arguments": "{}"},
        reasoning("rs_2", "opaque-two"),
        {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "done"}]},
    ]));
    assert_eq!(request.messages.len(), 2);
    let turn = &request.messages[1];
    assert_eq!(
        turn.extensions["responses_opaque_reasoning_items"],
        json!([
            {"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "summary of rs_1"}], "encrypted_content": "opaque-one"},
            {"type": "reasoning", "id": "rs_2", "summary": [{"type": "summary_text", "text": "summary of rs_2"}], "encrypted_content": "opaque-two"},
        ])
    );
    assert_eq!(turn.extensions["reasoning_replay_protocol_family"], "openai-encrypted-reasoning");
    // Content parts: the two summaries (unreferenced) and the text, in input order.
    let Some(Content::Parts(parts)) = &turn.content else { panic!("parts expected") };
    assert!(
        matches!(&parts[0], ContentPart::Thinking { thinking, signature: None } if thinking == "summary of rs_1")
    );
    assert!(
        matches!(&parts[1], ContentPart::Thinking { thinking, signature: None } if thinking == "summary of rs_2")
    );
    assert!(matches!(&parts[2], ContentPart::Text { text } if text == "done"));
    assert_eq!(
        turn.extensions["reasoning_replay_block_layout"],
        json!([
            {"kind": "reasoning_item", "index": 0},
            {"kind": "tool_call", "call_id": "c1"},
            {"kind": "reasoning_item", "index": 1},
            {"kind": "content", "ordinal": 2},
        ])
    );
    assert!(!turn.extensions.contains_key("responses_reasoning_id"));
    assert!(!turn.extensions.contains_key("responses_reasoning_encrypted_content"));
}

fn refused(input: Value) -> CodecError {
    let mut body = json!({"model": "m"});
    body["input"] = input;
    chat_request_from_responses(&body, &ResponsesRequestOptions::default()).expect_err("refused")
}

fn layout(message: &Message) -> &Value {
    &message.extensions["reasoning_replay_block_layout"]
}

#[test]
fn the_published_spellings() {
    assert_eq!(OPENAI_REASONING_REPLAY_CAPABILITY, "reasoning_replay.openai.v1");
    assert_ne!(OPENAI_REASONING_REPLAY_CAPABILITY, CLAUDE_REASONING_REPLAY_CAPABILITY);
    assert_eq!(OPENAI_REASONING_REPLAY_FAMILY, "openai-encrypted-reasoning");
    assert_eq!(OPENAI_REASONING_ITEMS_EXTENSION, "responses_opaque_reasoning_items");
}

#[test]
fn texts_around_an_item_stay_separate_parts_in_place() {
    let request = parse(json!([
        {"role": "user", "content": "go"},
        {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "A"}]},
        reasoning("rs_1", "opaque"),
        {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "B"}]},
    ]));
    let turn = &request.messages[1];
    let Some(Content::Parts(parts)) = &turn.content else { panic!("parts expected") };
    assert!(matches!(&parts[0], ContentPart::Text { text } if text == "A"));
    assert!(matches!(&parts[1], ContentPart::Thinking { signature: None, .. }));
    assert!(matches!(&parts[2], ContentPart::Text { text } if text == "B"));
    assert_eq!(
        layout(turn),
        &json!([
            {"kind": "content", "ordinal": 0},
            {"kind": "reasoning_item", "index": 0},
            {"kind": "content", "ordinal": 2},
        ])
    );
}

#[test]
fn a_run_that_is_only_a_reasoning_item_and_runs_split_by_a_tool_output() {
    let request = parse(json!([
        {"role": "user", "content": "go"},
        reasoning("rs_1", "one"),
        {"type": "function_call", "call_id": "c1", "name": "f", "arguments": "{}"},
        {"type": "function_call_output", "call_id": "c1", "output": "ok"},
        reasoning("rs_2", "two"),
    ]));
    assert_eq!(request.messages.len(), 4);
    assert_eq!(request.messages[1].extensions[OPENAI_REASONING_ITEMS_EXTENSION][0]["id"], "rs_1");
    assert_eq!(
        layout(&request.messages[1]),
        &json!([{"kind": "reasoning_item", "index": 0}, {"kind": "tool_call", "call_id": "c1"}])
    );
    let last = &request.messages[3];
    assert_eq!(last.extensions[OPENAI_REASONING_ITEMS_EXTENSION][0]["id"], "rs_2");
    assert_eq!(layout(last), &json!([{"kind": "reasoning_item", "index": 0}]));
    assert!(last.tool_calls.is_empty());
}

#[test]
fn the_projection_keeps_content_and_drops_unknown_keys() {
    let request = parse(json!([
        {"type": "reasoning", "id": "rs_1", "encrypted_content": "e", "status": "completed",
         "content": [{"type": "reasoning_text", "text": "raw"}], "extra": {"x": 1}},
    ]));
    assert_eq!(
        request.messages[0].extensions[OPENAI_REASONING_ITEMS_EXTENSION],
        json!([{"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "e",
                "content": [{"type": "reasoning_text", "text": "raw"}]}])
    );
}

/// Runs without a replayable item merge exactly as before NC-1 (texts joined with a newline, no
/// replay markers), including items without `encrypted_content` (NP-Q2).
#[test]
fn runs_without_a_replayable_item_are_unchanged() {
    let request = parse(json!([
        {"role": "user", "content": "go"},
        {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "A"}]},
        {"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "s"}]},
        {"type": "reasoning", "id": "rs_2", "summary": [], "encrypted_content": ""},
        {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "B"}]},
    ]));
    let turn = &request.messages[1];
    assert!(turn.extensions.is_empty(), "{:?}", turn.extensions);
    let Some(Content::Parts(parts)) = &turn.content else { panic!("parts expected") };
    assert!(matches!(&parts[0], ContentPart::Text { text } if text == "A"));
    assert!(matches!(&parts[1], ContentPart::Thinking { thinking, .. } if thinking == "s"));
    assert!(matches!(&parts[2], ContentPart::Text { text } if text == "B"));
}

#[test]
fn refusals_name_the_field_and_never_echo_the_value() {
    let carrier = encode_reasoning_replay_carrier(&ReasoningReplayCarrier::claude(vec![
        ReasoningReplayBlock::Thinking { thinking: "t".into(), signature: "s".into() },
    ]))
    .unwrap();
    let cases = [
        // Both families in one turn, and in one request.
        json!([reasoning("rs_1", "secret-opaque"),
               {"type": "reasoning", "id": "rs_c", "summary": [], "encrypted_content": carrier}]),
        json!([reasoning("rs_1", "secret-opaque"), {"role": "user", "content": "u"},
               {"type": "reasoning", "id": "rs_c", "summary": [], "encrypted_content": carrier}]),
        // An opaque item without an id, or with an empty one.
        json!([{"type": "reasoning", "summary": [], "encrypted_content": "secret-opaque"}]),
        json!([{"type": "reasoning", "id": "", "summary": [], "encrypted_content": "secret-opaque"}]),
        // An unknown carrier version is never read as an OpenAI value.
        json!([{"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "tsr.c9.secret-opaque"}]),
    ];
    for input in cases {
        let error = refused(input);
        assert_eq!(error.stable_code(), "reasoning_replay_invalid", "{error}");
        assert!(!error.to_string().contains("secret-opaque"));
    }
    // A summary that is not an array is a shape error, as before.
    let error = refused(
        json!([{"type": "reasoning", "id": "r", "summary": "x", "encrypted_content": "e"}]),
    );
    assert!(!error.to_string().contains("\"e\""));
}

#[test]
fn the_bounds_hold_at_the_limit_and_refuse_beyond_it() {
    let mega = "e".repeat(1024 * 1024);
    let ok = parse(json!([reasoning("rs_1", &mega)]));
    assert_eq!(
        ok.messages[0].extensions[OPENAI_REASONING_ITEMS_EXTENSION][0]["encrypted_content"]
            .as_str()
            .map(str::len),
        Some(mega.len())
    );
    assert_eq!(
        refused(json!([reasoning("rs_1", &format!("{mega}e"))])).stable_code(),
        "reasoning_replay_invalid"
    );
    // Four items of 1 MiB fit in one turn; a fifth byte over 4 MiB does not.
    let four: Vec<Value> = (0..4).map(|i| reasoning(&format!("rs_{i}"), &mega)).collect();
    parse(Value::Array(four.clone()));
    let mut five = four;
    five.push(reasoning("rs_x", "e"));
    assert_eq!(refused(Value::Array(five)).stable_code(), "reasoning_replay_invalid");
    // 128 layout entries fit; 129 do not.
    let entries = |n: usize| -> Value {
        let mut items = vec![reasoning("rs_0", "e")];
        items.extend((1..n).map(|i| json!({"type": "function_call", "call_id": format!("c{i}"), "name": "f", "arguments": "{}"})));
        Value::Array(items)
    };
    parse(entries(128));
    assert_eq!(refused(entries(129)).stable_code(), "reasoning_replay_invalid");
}

#[derive(Debug, Clone)]
enum Piece {
    Reasoning,
    Text,
    Call,
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Any interleaving of items, texts and calls in one assistant run comes back as a layout that
    /// names them in input order.
    #[test]
    fn the_layout_states_the_input_order(pieces in prop::collection::vec(
        prop_oneof![Just(Piece::Reasoning), Just(Piece::Text), Just(Piece::Call)], 1..12)) {
        let mut input = vec![json!({"role": "user", "content": "go"}), reasoning("rs_first", "e")];
        let mut expected = vec![json!({"kind": "reasoning_item", "index": 0})];
        let (mut items, mut texts) = (1usize, 0usize);
        let mut part_count = 1usize;
        for (i, piece) in pieces.iter().enumerate() {
            match piece {
                Piece::Reasoning => {
                    input.push(reasoning(&format!("rs_{i}"), "e"));
                    expected.push(json!({"kind": "reasoning_item", "index": items}));
                    items += 1;
                    part_count += 1;
                }
                Piece::Text => {
                    input.push(json!({"type": "message", "role": "assistant",
                                      "content": [{"type": "output_text", "text": format!("t{texts}")}]}));
                    expected.push(json!({"kind": "content", "ordinal": part_count}));
                    texts += 1;
                    part_count += 1;
                }
                Piece::Call => {
                    input.push(json!({"type": "function_call", "call_id": format!("c{i}"), "name": "f", "arguments": "{}"}));
                    expected.push(json!({"kind": "tool_call", "call_id": format!("c{i}")}));
                }
            }
        }
        let request = parse(Value::Array(input));
        prop_assert_eq!(request.messages.len(), 2);
        prop_assert_eq!(layout(&request.messages[1]), &Value::Array(expected));
        prop_assert_eq!(
            request.messages[1].extensions[OPENAI_REASONING_ITEMS_EXTENSION].as_array().map(Vec::len),
            Some(items)
        );
    }
}

/// G3 (render-gaps record §5.3, NP-Q8): a relayed stream's failure event equals the render
/// state's failure frame for the same identity, error and position.
#[test]
fn the_relayed_failure_event_matches_the_rendered_one() {
    use south_north_codec::{
        RelayedResponseIdentity, ResponsesContext, ResponsesReasoningMode, ResponsesSseState,
        responses_error_event, responses_frames,
    };
    use token_station_protocol::{ErrorCode, ErrorEnvelope, StreamEvent};
    let error = ErrorEnvelope::new(ErrorCode::RateLimit, 429, "The upstream is busy.");
    let mut state = ResponsesSseState::new(ResponsesContext {
        response_id: "resp_1".into(),
        model: "gpt-5.5".into(),
        created_at: 1_700_000_000,
        inbound_tools: json!([]),
        reasoning: ResponsesReasoningMode::Summary,
        allow_incomplete_tool_calls: false,
        render_legacy_encrypted_reasoning: false,
    });
    let frames =
        responses_frames(&[StreamEvent::Error { error: error.clone() }], &mut state).unwrap();
    let rendered = frames.last().unwrap();
    let relayed = responses_error_event(
        &error,
        &RelayedResponseIdentity {
            response_id: "resp_1".into(),
            model: "gpt-5.5".into(),
            created_at: 1_700_000_000,
        },
        rendered.data["sequence_number"].as_u64().unwrap(),
    );
    assert_eq!(&relayed, rendered);
    assert_eq!(relayed.data["response"]["error"]["code"], "rate_limit_exceeded");
    assert_eq!(relayed.data["response"]["error"]["message"], "The upstream is busy.");
}
