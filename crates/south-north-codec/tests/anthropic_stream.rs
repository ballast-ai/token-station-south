//! Behaviour of the Anthropic Messages SSE renderer.
//!
//! What is pinned here is what a client acts on: the order blocks open and
//! close in, which frames may be sent once, what a stream that never finished
//! is allowed to look like, and whether the numbers the client bills against
//! survive to the end of the stream.

use serde_json::{Value, json};
use south_north_codec::{AnthropicFrame, AnthropicSseState, anthropic_frames};
use token_station_protocol::{
    ErrorCode, ErrorEnvelope, Extensions, FinishReason, StreamEvent, Usage,
};

fn state() -> AnthropicSseState {
    AnthropicSseState::new("m", "msg_fixed")
}

/// The frames as `(event, data)` pairs, which is how they reach a client.
fn pairs(frames: &[AnthropicFrame]) -> Vec<(&str, &Value)> {
    frames.iter().map(|frame| (frame.event, &frame.data)).collect()
}

fn names(frames: &[AnthropicFrame]) -> Vec<&str> {
    frames.iter().map(|frame| frame.event).collect()
}

fn text_stream() -> Vec<StreamEvent> {
    vec![
        StreamEvent::Delta { index: 0, content: "hel".to_owned() },
        StreamEvent::Delta { index: 0, content: "lo".to_owned() },
        StreamEvent::Usage {
            usage: Usage { input_tokens: 10, output_tokens: 5, ..Usage::default() },
        },
        StreamEvent::Done { finish_reason: Some(FinishReason::Stop), stop_sequence: None },
    ]
}

#[test]
fn the_frame_skeleton_is_pinned_frame_by_frame() {
    let frames = anthropic_frames(
        &[
            StreamEvent::ThinkingDelta { index: 0, thinking_delta: "thi".to_owned() },
            StreamEvent::ThinkingSignatureDelta { index: 0, signature_delta: "sig".to_owned() },
            StreamEvent::Delta { index: 0, content: "hi".to_owned() },
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("toolu_upstream".to_owned()),
                name: Some("get_weather".to_owned()),
                arguments_delta: String::new(),
            },
            StreamEvent::ToolCallDelta {
                index: 0,
                id: None,
                name: None,
                arguments_delta: "{\"city\":\"Beijing\"}".to_owned(),
            },
            StreamEvent::Usage {
                usage: Usage { input_tokens: 10, output_tokens: 5, ..Usage::default() },
            },
            StreamEvent::Done { finish_reason: Some(FinishReason::ToolCalls), stop_sequence: None },
        ],
        &mut state(),
    );

    let expected = vec![
        (
            "message_start",
            json!({"type": "message_start", "message": {
                "id": "msg_fixed", "type": "message", "role": "assistant", "content": [],
                "model": "m", "stop_reason": null, "stop_sequence": null,
                "usage": {"input_tokens": 0, "output_tokens": 0}
            }}),
        ),
        (
            "content_block_start",
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "thinking", "thinking": ""}}),
        ),
        (
            "content_block_delta",
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "thinking_delta", "thinking": "thi"}}),
        ),
        (
            "content_block_delta",
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "signature_delta", "signature": "sig"}}),
        ),
        (
            "content_block_start",
            json!({"type": "content_block_start", "index": 1,
                   "content_block": {"type": "text", "text": ""}}),
        ),
        (
            "content_block_delta",
            json!({"type": "content_block_delta", "index": 1,
                   "delta": {"type": "text_delta", "text": "hi"}}),
        ),
        (
            "content_block_start",
            json!({"type": "content_block_start", "index": 2, "content_block": {
                "type": "tool_use", "id": "toolu_upstream", "name": "get_weather", "input": {}
            }}),
        ),
        (
            "content_block_delta",
            json!({"type": "content_block_delta", "index": 2, "delta": {
                "type": "input_json_delta", "partial_json": "{\"city\":\"Beijing\"}"
            }}),
        ),
        ("content_block_stop", json!({"type": "content_block_stop", "index": 0})),
        ("content_block_stop", json!({"type": "content_block_stop", "index": 1})),
        ("content_block_stop", json!({"type": "content_block_stop", "index": 2})),
        (
            "message_delta",
            json!({"type": "message_delta",
                   "delta": {"stop_reason": "tool_use", "stop_sequence": null},
                   "usage": {"output_tokens": 5, "input_tokens": 10}}),
        ),
        ("message_stop", json!({"type": "message_stop"})),
    ];
    let expected: Vec<(&str, &Value)> =
        expected.iter().map(|(event, data)| (*event, data)).collect();
    assert_eq!(
        pairs(&frames),
        expected,
        "every block opens before its deltas and closes before the message does; the empty \
         tool fragment announces the call and adds no delta of its own"
    );
}

#[test]
fn framing_does_not_depend_on_how_the_caller_batches_events() {
    let events = text_stream();
    let render = |sizes: &[usize]| {
        let mut state = state();
        let mut out = Vec::new();
        let mut at = 0;
        for size in sizes {
            let end = (at + size).min(events.len());
            out.extend(anthropic_frames(&events[at..end], &mut state));
            at = end;
        }
        assert_eq!(at, events.len(), "the split must cover every event");
        out
    };
    let whole = render(&[events.len()]);
    for sizes in [vec![1, 1, 1, 1], vec![2, 2], vec![3, 1], vec![1, 3], vec![1, 2, 1]] {
        assert_eq!(
            render(&sizes),
            whole,
            "split {sizes:?} changed the frames: a client-visible shape must not follow \
             upstream packet timing"
        );
    }
}

#[test]
fn a_stream_that_never_finishes_leaves_the_message_open() {
    let mut state = state();
    let frames = anthropic_frames(
        &[
            StreamEvent::Delta { index: 0, content: "half".to_owned() },
            StreamEvent::Finish { finish_reason: Some(FinishReason::Stop), stop_sequence: None },
        ],
        &mut state,
    );
    assert_eq!(
        names(&frames),
        ["message_start", "content_block_start", "content_block_delta"],
        "that stream did not finish; closing the block and sending a clean stop_reason would \
         render 'the upstream stopped talking' as 'the model completed normally'"
    );
    assert_eq!(
        state.pending_finish_reason,
        Some(FinishReason::Stop),
        "the reason waits for Done rather than producing a terminal of its own"
    );
}

#[test]
fn an_error_ends_the_stream_and_no_terminal_follows_it() {
    let mut state = state();
    let mut frames = anthropic_frames(
        &[
            StreamEvent::Delta { index: 0, content: "half".to_owned() },
            StreamEvent::Error {
                error: ErrorEnvelope {
                    code: ErrorCode::UpstreamUnavailable,
                    http_status: 502,
                    message: "upstream died".to_owned(),
                    provider_message: None,
                    retry_after_ms: None,
                    extensions: Extensions::new(),
                },
            },
        ],
        &mut state,
    );
    assert!(state.terminated);
    assert_eq!(
        frames.last().map(|frame| (frame.event, &frame.data)),
        Some((
            "error",
            &json!({"type": "error", "error": {"type": "api_error", "message": "upstream died"}})
        ))
    );

    frames.extend(anthropic_frames(
        &[
            StreamEvent::Delta { index: 0, content: "more".to_owned() },
            StreamEvent::Done { finish_reason: Some(FinishReason::Stop), stop_sequence: None },
        ],
        &mut state,
    ));
    assert!(
        !names(&frames).contains(&"message_stop"),
        "a message_stop after an error tells the client the message completed, and the client \
         treats half an answer as a whole one: {frames:?}"
    );
}

#[test]
fn usage_reported_only_at_the_end_still_reaches_the_client() {
    let frames = anthropic_frames(
        &[
            StreamEvent::Delta { index: 0, content: "hi".to_owned() },
            StreamEvent::Usage {
                usage: Usage {
                    input_tokens: 1000,
                    cache_read_tokens: 300,
                    cache_write_tokens: 200,
                    output_tokens: 20,
                    ..Usage::default()
                },
            },
            StreamEvent::Done { finish_reason: Some(FinishReason::Stop), stop_sequence: None },
        ],
        &mut state(),
    );
    let start = &frames[0].data["message"]["usage"];
    assert_eq!(
        *start,
        json!({"input_tokens": 0, "output_tokens": 0}),
        "nothing had been reported yet when the message was announced"
    );
    let terminal = frames.iter().find(|frame| frame.event == "message_delta").expect("terminal");
    assert_eq!(
        terminal.data["usage"],
        json!({
            "output_tokens": 20, "input_tokens": 1000,
            "cache_read_input_tokens": 300, "cache_creation_input_tokens": 200
        }),
        "a provider that reports everything at the end would otherwise have its input count \
         announced as zero and never corrected"
    );
}

#[test]
fn usage_reported_up_front_is_announced_and_folded_rather_than_replaced() {
    let mut state = state();
    let frames = anthropic_frames(
        &[
            StreamEvent::Usage {
                usage: Usage {
                    input_tokens: 700,
                    cache_read_tokens: 300,
                    output_tokens: 1,
                    ..Usage::default()
                },
            },
            StreamEvent::Delta { index: 0, content: "hi".to_owned() },
        ],
        &mut state,
    );
    assert_eq!(
        frames[0].data["message"]["usage"],
        json!({"input_tokens": 700, "output_tokens": 0, "cache_read_input_tokens": 300}),
        "a usage report before any content is announced with the message it belongs to"
    );

    let tail = anthropic_frames(
        &[
            StreamEvent::Usage { usage: Usage { output_tokens: 500, ..Usage::default() } },
            StreamEvent::Done { finish_reason: Some(FinishReason::Stop), stop_sequence: None },
        ],
        &mut state,
    );
    let terminal = tail.iter().find(|frame| frame.event == "message_delta").expect("terminal");
    assert_eq!(
        terminal.data["usage"],
        json!({"output_tokens": 500, "input_tokens": 700, "cache_read_input_tokens": 300}),
        "input up front and output at the end is one usage in two instalments; the second must \
         not zero the first"
    );
}

#[test]
fn a_tool_call_with_no_id_is_derived_so_the_same_stream_renders_identically() {
    let events = [
        StreamEvent::ToolCallDelta {
            index: 7,
            id: None,
            name: Some("f".to_owned()),
            arguments_delta: "{}".to_owned(),
        },
        StreamEvent::ToolCallDelta {
            index: 9,
            id: None,
            name: Some("g".to_owned()),
            arguments_delta: "{}".to_owned(),
        },
        StreamEvent::ToolCallDelta {
            index: 7,
            id: None,
            name: None,
            arguments_delta: "more".to_owned(),
        },
    ];
    let render = || anthropic_frames(&events, &mut state());
    let frames = render();
    assert_eq!(frames, render(), "no random source: the same stream renders byte for byte");

    let starts: Vec<(&Value, &Value)> = frames
        .iter()
        .filter(|frame| frame.event == "content_block_start")
        .map(|frame| (&frame.data["index"], &frame.data["content_block"]["id"]))
        .collect();
    assert_eq!(
        starts,
        vec![(&json!(0), &json!("toolu_fixed_0")), (&json!(1), &json!("toolu_fixed_1")),],
        "two IR tool slots are two blocks, each numbered from the message id"
    );
    let second_fragment = frames.last().expect("a delta for the reopened slot");
    assert_eq!(
        second_fragment.data["index"], 0,
        "the later fragment of slot 7 goes back to slot 7's block, not to a third one"
    );
}

#[test]
fn blocks_are_closed_by_kind_rather_than_in_the_order_they_opened() {
    let frames = anthropic_frames(
        &[
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("toolu_1".to_owned()),
                name: Some("f".to_owned()),
                arguments_delta: "{}".to_owned(),
            },
            StreamEvent::Delta { index: 0, content: "after the call".to_owned() },
            StreamEvent::Done { finish_reason: Some(FinishReason::ToolCalls), stop_sequence: None },
        ],
        &mut state(),
    );
    let stops: Vec<&Value> = frames
        .iter()
        .filter(|frame| frame.event == "content_block_stop")
        .map(|frame| &frame.data["index"])
        .collect();
    assert_eq!(
        stops,
        vec![&json!(1), &json!(0)],
        "the tool block opened first and closes last; a client keys a stop on the block number          it carries, not on its position among the other stops"
    );
}

#[test]
fn a_signature_with_no_thinking_block_renders_nothing_and_starts_no_message() {
    let mut state = state();
    let frames = anthropic_frames(
        &[StreamEvent::ThinkingSignatureDelta { index: 0, signature_delta: "sig".to_owned() }],
        &mut state,
    );
    assert!(
        frames.is_empty(),
        "there is no block to sign, and announcing a message to say so would announce one that \
         may never get any content: {frames:?}"
    );
    assert!(!state.message_started);
}

#[test]
fn done_carries_the_stop_semantics_and_an_unmodelled_reason_survives() {
    let mut state = state();
    let frames = anthropic_frames(
        &[
            StreamEvent::Delta { index: 0, content: "hi".to_owned() },
            StreamEvent::Finish {
                finish_reason: Some(FinishReason::Stop),
                stop_sequence: Some("END".to_owned()),
            },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::Other("model_yawned".to_owned())),
                stop_sequence: None,
            },
        ],
        &mut state,
    );
    let terminal = frames.iter().find(|frame| frame.event == "message_delta").expect("terminal");
    assert_eq!(
        terminal.data["delta"],
        json!({"stop_reason": "model_yawned", "stop_sequence": "END"}),
        "Done's own reason wins over the parked one, and the sequence Finish parked is still the \
         sequence that fired"
    );
}

#[test]
fn a_finished_message_resets_the_blocks_but_keeps_the_stream_identity() {
    let mut state = state();
    let first = anthropic_frames(&text_stream(), &mut state);
    assert_eq!(names(&first).last(), Some(&"message_stop"), "the first message finished");
    assert_eq!((state.message_id.as_str(), state.model.as_str()), ("msg_fixed", "m"));

    let second = anthropic_frames(
        &[StreamEvent::Delta { index: 0, content: "again".to_owned() }],
        &mut state,
    );
    assert_eq!(
        names(&second),
        ["message_start", "content_block_start", "content_block_delta"],
        "the next message is announced afresh"
    );
    assert_eq!(second[1].data["index"], 0, "block numbering restarts with the message");
    assert_eq!(
        second[0].data["message"]["usage"],
        json!({"input_tokens": 0, "output_tokens": 0}),
        "the previous message's usage is not carried into this one"
    );
}
