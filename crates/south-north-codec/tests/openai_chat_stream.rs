//! `OpenAiChatStream`: the `OpenAI` Chat stream with chunk identity, frame kinds, the usage
//! fold and the first-delta role (render-gaps record G1, `docs/design/2026-09-30-north-codec-render-gaps.md` §3).
//!
//! Assertions compare JSON values, never bytes (the record's D7 and N-Q5): the codec does not own key
//! order, so a host built with `serde_json/preserve_order` and one built without it must both pass.
//!
//! The fixture pack under `fixtures/render-gaps/chat.stream/` is read at the end of this file. A second
//! repository reads the same files at the pinned tag (record §8), so the pack is checked here against the
//! code rather than trusted.

use std::fs;
use std::path::Path;

use proptest::prelude::*;
use serde_json::{Value, json};
use south_north_codec::{
    ChatStreamContext, CodecError, OpenAiChatFrame, OpenAiChatFrameKind, OpenAiChatStream,
    openai_chat_usage,
};
use token_station_protocol::{
    ErrorCode, ErrorEnvelope, Extensions, FinishReason, StreamEvent, Usage,
};

fn context() -> ChatStreamContext {
    ChatStreamContext {
        id: "chatcmpl-abc".to_owned(),
        model: "upstream-model".to_owned(),
        created: 1_700_000_000,
    }
}

fn text(index: u32, content: &str) -> StreamEvent {
    StreamEvent::Delta { index, content: content.to_owned() }
}

const fn finish(reason: Option<FinishReason>) -> StreamEvent {
    StreamEvent::Finish { finish_reason: reason, stop_sequence: None }
}

const fn done() -> StreamEvent {
    StreamEvent::Done { finish_reason: None, stop_sequence: None }
}

fn usage(input: u64, output: u64) -> StreamEvent {
    StreamEvent::Usage {
        usage: Usage { input_tokens: input, output_tokens: output, ..Usage::default() },
    }
}

fn error() -> StreamEvent {
    StreamEvent::Error {
        error: ErrorEnvelope {
            code: ErrorCode::UpstreamUnavailable,
            http_status: 502,
            message: "upstream died".to_owned(),
            provider_message: None,
            retry_after_ms: None,
            extensions: Extensions::new(),
        },
    }
}

fn render(events: &[StreamEvent]) -> Result<Vec<OpenAiChatFrame>, CodecError> {
    OpenAiChatStream::new(context()).frames(events)
}

fn kinds(frames: &[OpenAiChatFrame]) -> Vec<OpenAiChatFrameKind> {
    frames.iter().map(|frame| frame.kind).collect()
}

// ── identity ────────────────────────────────────────────────────────────────

#[test]
fn every_frame_carries_the_context_identity() {
    let frames = render(&[
        text(0, "hel"),
        StreamEvent::ThinkingDelta { index: 0, block_index: 0, thinking_delta: "t".to_owned() },
        StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("call_1".to_owned()),
            name: Some("f".to_owned()),
            arguments_delta: "{}".to_owned(),
        },
        finish(Some(FinishReason::Stop)),
        usage(3, 4),
        done(),
    ])
    .expect("renders");
    assert_eq!(frames.len(), 4, "{frames:?}");
    for frame in &frames {
        assert_eq!(frame.data["id"], "chatcmpl-abc", "{frame:?}");
        assert_eq!(frame.data["object"], "chat.completion.chunk", "{frame:?}");
        assert_eq!(frame.data["created"], 1_700_000_000, "{frame:?}");
        assert_eq!(frame.data["model"], "upstream-model", "{frame:?}");
    }
}

#[test]
fn frames_carry_exactly_the_identity_choices_and_usage_keys() {
    // Key order is not a codec contract (N-Q5); this only pins the insertion order the record states,
    // which a host built with `preserve_order` serializes as is.
    let frames = render(&[text(0, "x"), finish(Some(FinishReason::Stop)), usage(1, 1), done()])
        .expect("renders");
    for frame in &frames {
        let keys: Vec<&str> =
            frame.data.as_object().expect("an object").keys().map(String::as_str).collect();
        let mut expected = vec!["choices", "created", "id", "model", "object"];
        if frame.data.get("usage").is_some() {
            expected.push("usage");
        }
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, expected, "exactly the identity, choices and usage keys: {keys:?}");
    }
}

#[test]
fn the_id_prefix_is_applied_once_and_never_stacked() {
    let mut bare = context();
    bare.id = "abc".to_owned();
    let frames = OpenAiChatStream::new(bare).frames(&[text(0, "x")]).expect("renders");
    assert_eq!(frames[0].data["id"], "chatcmpl-abc");

    let frames = render(&[text(0, "x")]).expect("renders");
    assert_eq!(frames[0].data["id"], "chatcmpl-abc", "an already prefixed id is kept as it is");
}

// ── role ────────────────────────────────────────────────────────────────────

#[test]
fn the_first_delta_of_each_choice_carries_the_role_once() {
    let frames =
        render(&[text(0, "a"), text(1, "b"), text(0, "c"), text(1, "d")]).expect("renders");
    let deltas: Vec<Value> = frames.iter().map(|f| f.data["choices"][0]["delta"].clone()).collect();
    assert_eq!(
        deltas,
        vec![
            json!({"role": "assistant", "content": "a"}),
            json!({"role": "assistant", "content": "b"}),
            json!({"content": "c"}),
            json!({"content": "d"}),
        ]
    );
}

#[test]
fn the_role_goes_on_the_first_delta_frame_whatever_it_carries() {
    let reasoning = render(&[
        StreamEvent::ThinkingDelta { index: 0, block_index: 0, thinking_delta: "t".to_owned() },
        text(0, "x"),
    ])
    .expect("renders");
    assert_eq!(
        reasoning[0].data["choices"][0]["delta"],
        json!({"role": "assistant", "reasoning_content": "t"})
    );
    assert_eq!(reasoning[1].data["choices"][0]["delta"], json!({"content": "x"}));

    let tool = render(&[
        StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("call_1".to_owned()),
            name: Some("f".to_owned()),
            arguments_delta: "{\"a\"".to_owned(),
        },
        StreamEvent::ToolCallDelta {
            index: 0,
            id: None,
            name: None,
            arguments_delta: ":1}".to_owned(),
        },
    ])
    .expect("renders");
    assert_eq!(
        tool[0].data["choices"][0]["delta"],
        json!({"role": "assistant", "tool_calls": [
            {"index": 0, "id": "call_1", "function": {"name": "f", "arguments": "{\"a\""}}
        ]})
    );
    assert_eq!(
        tool[1].data["choices"][0]["delta"],
        json!({"tool_calls": [{"index": 0, "function": {"arguments": ":1}"}}]})
    );
}

#[test]
fn events_this_wire_drops_do_not_consume_the_role() {
    let frames = render(&[
        StreamEvent::ThinkingSignatureDelta {
            index: 0,
            block_index: 0,
            signature_delta: "sig".to_owned(),
        },
        StreamEvent::RedactedThinking { index: 0, block_index: 1, data: "opaque".to_owned() },
        text(0, "x"),
    ])
    .expect("renders");
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0].data["choices"][0]["delta"], json!({"role": "assistant", "content": "x"}));
}

#[test]
fn terminal_and_usage_frames_never_carry_the_role() {
    let frames = render(&[finish(Some(FinishReason::Stop)), usage(1, 1), done()]).expect("renders");
    assert_eq!(kinds(&frames), vec![OpenAiChatFrameKind::Terminal]);
    assert_eq!(frames[0].data["choices"][0]["delta"], json!({}));
}

// ── kinds ───────────────────────────────────────────────────────────────────

#[test]
fn the_chunk_rendered_from_a_finish_is_terminal_even_with_a_null_reason() {
    let frames = render(&[text(0, "x"), finish(None), done()]).expect("renders");
    assert_eq!(kinds(&frames), vec![OpenAiChatFrameKind::Delta, OpenAiChatFrameKind::Terminal]);
    assert_eq!(frames[1].data["choices"][0]["finish_reason"], Value::Null);
    assert!(frames[1].data.get("usage").is_none(), "no usage was folded: {:?}", frames[1]);
}

#[test]
fn a_finish_flushed_by_a_later_delta_is_still_terminal() {
    let frames = render(&[finish(Some(FinishReason::Stop)), text(0, "late")]).expect("renders");
    assert_eq!(kinds(&frames), vec![OpenAiChatFrameKind::Terminal, OpenAiChatFrameKind::Delta]);
    assert_eq!(frames[0].data["choices"][0]["finish_reason"], "stop");
}

#[test]
fn a_finish_without_done_renders_no_terminal() {
    let mut stream = OpenAiChatStream::new(context());
    let frames = stream.frames(&[text(0, "x"), finish(Some(FinishReason::Stop))]).expect("renders");
    assert_eq!(kinds(&frames), vec![OpenAiChatFrameKind::Delta]);
}

// ── usage fold (N-Q2) ───────────────────────────────────────────────────────

#[test]
fn early_usage_is_folded_into_the_terminal_frame() {
    let frames = render(&[
        StreamEvent::Usage {
            usage: Usage { input_tokens: 700, cache_read_tokens: 300, ..Usage::default() },
        },
        text(0, "x"),
        finish(Some(FinishReason::Stop)),
        StreamEvent::Usage {
            usage: Usage { output_tokens: 50, reasoning_tokens: 20, ..Usage::default() },
        },
        done(),
    ])
    .expect("renders");
    assert_eq!(
        kinds(&frames),
        vec![OpenAiChatFrameKind::Delta, OpenAiChatFrameKind::Terminal],
        "no `choices: []` chunk before the terminal one"
    );
    assert_eq!(
        frames[1].data,
        json!({
            "id": "chatcmpl-abc",
            "object": "chat.completion.chunk",
            "created": 1_700_000_000,
            "model": "upstream-model",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            "usage": {
                "prompt_tokens": 700, "completion_tokens": 50, "total_tokens": 750,
                "prompt_tokens_details": {"cached_tokens": 300},
                "completion_tokens_details": {"reasoning_tokens": 20}
            }
        })
    );
}

#[test]
fn done_without_a_finish_emits_one_usage_frame_when_usage_was_folded() {
    let frames = render(&[text(0, "x"), usage(2, 3), done()]).expect("renders");
    assert_eq!(kinds(&frames), vec![OpenAiChatFrameKind::Delta, OpenAiChatFrameKind::Usage]);
    assert_eq!(
        frames[1].data,
        json!({
            "id": "chatcmpl-abc",
            "object": "chat.completion.chunk",
            "created": 1_700_000_000,
            "model": "upstream-model",
            "choices": [],
            "usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5}
        })
    );

    let nothing = render(&[text(0, "x"), done()]).expect("renders");
    assert_eq!(kinds(&nothing), vec![OpenAiChatFrameKind::Delta], "no usage was folded");
}

// ── error ───────────────────────────────────────────────────────────────────

#[test]
fn an_error_renders_nothing_and_ends_the_stream() {
    let mut stream = OpenAiChatStream::new(context());
    let mut frames = stream
        .frames(&[text(0, "half"), finish(Some(FinishReason::Stop)), error()])
        .expect("renders");
    frames.extend(stream.frames(&[usage(1, 1), text(0, "more"), done()]).expect("renders"));
    assert_eq!(kinds(&frames), vec![OpenAiChatFrameKind::Delta], "{frames:?}");
}

// ── overflow ────────────────────────────────────────────────────────────────

#[test]
fn a_total_that_overflows_is_out_of_range() {
    let huge = Usage { input_tokens: u64::MAX, output_tokens: 1, ..Usage::default() };
    let error = openai_chat_usage(&huge).expect_err("the total does not fit in u64");
    assert!(matches!(error, CodecError::OutOfRange { .. }), "{error:?}");
    assert_eq!(error.field(), "usage.total_tokens");

    let error = render(&[
        text(0, "x"),
        finish(Some(FinishReason::Stop)),
        StreamEvent::Usage { usage: huge },
        done(),
    ])
    .expect_err("the terminal frame cannot render its usage");
    assert!(matches!(error, CodecError::OutOfRange { .. }), "{error:?}");
    assert_eq!(error.field(), "usage.total_tokens");
}

#[test]
fn after_an_overflow_the_stream_renders_nothing() {
    let mut stream = OpenAiChatStream::new(context());
    let huge = Usage { input_tokens: u64::MAX, output_tokens: 1, ..Usage::default() };
    stream
        .frames(&[StreamEvent::Usage { usage: huge }, done()])
        .expect_err("the usage frame cannot render");
    assert_eq!(stream.frames(&[text(0, "x"), done()]).expect("renders"), Vec::new());
}

#[test]
fn the_usage_object_matches_the_existing_renderers() {
    let full = Usage {
        input_tokens: 10,
        output_tokens: 5,
        cache_read_tokens: 4,
        reasoning_tokens: 2,
        cache_write_tokens: 9,
        ..Usage::default()
    };
    assert_eq!(
        openai_chat_usage(&full).expect("fits"),
        json!({
            "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15,
            "prompt_tokens_details": {"cached_tokens": 4},
            "completion_tokens_details": {"reasoning_tokens": 2}
        })
    );
    assert_eq!(
        openai_chat_usage(&Usage { input_tokens: 1, ..Usage::default() }).expect("fits"),
        json!({"prompt_tokens": 1, "completion_tokens": 0, "total_tokens": 1}),
        "zero detail buckets are omitted, as the existing renderers do"
    );
}

// ── batching ────────────────────────────────────────────────────────────────

fn event_strategy() -> impl Strategy<Value = StreamEvent> {
    prop_oneof![
        (0u32..2, "[a-z]{0,4}").prop_map(|(index, content)| StreamEvent::Delta { index, content }),
        (0u32..2, "[a-z]{0,4}").prop_map(|(index, thinking_delta)| StreamEvent::ThinkingDelta {
            index,
            block_index: 0,
            thinking_delta
        }),
        (0u32..2, proptest::option::of("[a-z]{1,3}"), "[a-z]{0,4}").prop_map(
            |(index, name, arguments_delta)| StreamEvent::ToolCallDelta {
                index,
                id: name.clone(),
                name,
                arguments_delta
            }
        ),
        Just(StreamEvent::ThinkingSignatureDelta {
            index: 0,
            block_index: 0,
            signature_delta: "s".to_owned()
        }),
        proptest::option::of(Just(FinishReason::Stop)).prop_map(finish),
        (0u64..1000, 0u64..1000).prop_map(|(input, output)| usage(input, output)),
        Just(done()),
        Just(error()),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn frames_do_not_depend_on_how_the_caller_batches_events(
        events in proptest::collection::vec(event_strategy(), 0..16),
        cut in 0usize..16,
    ) {
        let whole = render(&events).expect("no overflow in these ranges");

        let mut one_by_one = OpenAiChatStream::new(context());
        let mut single = Vec::new();
        for event in &events {
            single.extend(one_by_one.frames(std::slice::from_ref(event)).expect("renders"));
        }
        prop_assert_eq!(&single, &whole);

        let cut = cut.min(events.len());
        let mut halves = OpenAiChatStream::new(context());
        let mut split = halves.frames(&events[..cut]).expect("renders");
        split.extend(halves.frames(&events[cut..]).expect("renders"));
        prop_assert_eq!(&split, &whole);
    }
}

// ── fixture pack (record §8, family `chat.stream`) ───────────────────────────

const CHAT_STREAM_ROWS: [&str; 8] = [
    "identity-on-every-chunk",
    "role-on-first-delta",
    "terminal-kind",
    "terminal-with-null-reason",
    "usage-folded-into-terminal",
    "usage-only",
    "error-renders-nothing",
    "usage-overflow",
];

const fn kind_name(kind: OpenAiChatFrameKind) -> &'static str {
    match kind {
        OpenAiChatFrameKind::Delta => "delta",
        OpenAiChatFrameKind::Terminal => "terminal",
        OpenAiChatFrameKind::Usage => "usage",
    }
}

#[test]
fn the_chat_stream_fixture_pack_matches_the_codec() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/render-gaps/chat.stream");
    let mut found: Vec<String> = fs::read_dir(&dir)
        .expect("the fixture directory exists")
        .map(|entry| {
            let name = entry.expect("a readable entry").file_name();
            name.to_str().expect("a UTF-8 name").to_owned()
        })
        .collect();
    found.sort_unstable();
    let mut required: Vec<String> =
        CHAT_STREAM_ROWS.iter().map(|row| format!("{row}.json")).collect();
    required.sort_unstable();
    assert_eq!(found, required, "every row the record names, and nothing else");

    for row in CHAT_STREAM_ROWS {
        let raw = fs::read_to_string(dir.join(format!("{row}.json"))).expect("readable fixture");
        let case: Value = serde_json::from_str(&raw).expect("valid JSON");
        let context: ChatStreamContext = ChatStreamContext {
            id: case["context"]["id"].as_str().expect("context.id").to_owned(),
            model: case["context"]["model"].as_str().expect("context.model").to_owned(),
            created: case["context"]["created"].as_i64().expect("context.created"),
        };
        let mut stream = OpenAiChatStream::new(context.clone());
        let mut frames = Vec::new();
        let mut failure = None;
        for (call, batch) in
            case["input"].as_array().expect("input is a list of calls").iter().enumerate()
        {
            let events: Vec<StreamEvent> =
                serde_json::from_value(batch.clone()).expect("input events deserialize");
            match stream.frames(&events) {
                Ok(more) => frames.extend(more),
                Err(error) => {
                    failure = Some(json!({
                        "call": call,
                        "code": error.stable_code(),
                        "field": error.field(),
                    }));
                    break;
                }
            }
        }
        let actual = json!({
            "frames": frames
                .iter()
                .map(|frame| json!({"kind": kind_name(frame.kind), "data": frame.data}))
                .collect::<Vec<_>>(),
            "error": failure,
        });
        assert_eq!(actual, case["expected"], "fixture row {row}");

        // The record's row assertion: every chunk carries the context's three fields.
        for frame in &frames {
            assert_eq!(frame.data["created"], json!(context.created), "row {row}");
            assert_eq!(frame.data["model"], json!(context.model), "row {row}");
        }
    }
}

// ── readings where the record is silent (record §3, "Implemented") ──────────

#[test]
fn usage_folded_after_the_terminal_was_emitted_is_not_rendered() {
    let frames = render(&[finish(Some(FinishReason::Stop)), text(0, "late"), usage(1, 1), done()])
        .expect("renders");
    assert_eq!(kinds(&frames), vec![OpenAiChatFrameKind::Terminal, OpenAiChatFrameKind::Delta]);
    assert!(frames[0].data.get("usage").is_none(), "{frames:?}");
}

#[test]
fn a_repeated_done_emits_no_second_usage_frame() {
    let frames = render(&[usage(1, 1), done(), usage(2, 2), done()]).expect("renders");
    assert_eq!(kinds(&frames), vec![OpenAiChatFrameKind::Usage]);
}

#[test]
fn an_empty_context_id_renders_the_bare_prefix() {
    let mut empty = context();
    empty.id = String::new();
    let frames = OpenAiChatStream::new(empty).frames(&[text(0, "x")]).expect("renders");
    assert_eq!(frames[0].data["id"], "chatcmpl-");
}
