//! What the `OpenAI` Responses component shares with the two crates beside it, judged by a test that
//! depends on all three (the record §9, R-Q13; §6 as amended 2026-10-10).
//!
//! - **Vocabulary.** The constants in `responses_vocabulary` and the literals `south-north-codec`
//!   keeps for the same wire agree: the codec's IR keys, event types, item types, part types and
//!   incomplete reasons are the ones the component names.
//! - **Round trip.** North parse then south build gives the pack's request rows, which are
//!   therefore north codec outputs; south parse then north render then south parse keeps text,
//!   tool calls, finish reason and every usage bucket, for responses and for streams.
//! - **SSE.** The host-only decoder's golden vectors give the component's own splitter the same
//!   events at every split, and frames cut where the decoder's `position` says each hold exactly
//!   one event and leave the splitter holding nothing (the pass-through delivery of §8.2).

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::ProviderComponentV1;
use south_component_conformance::reference_openai_responses::OpenAiResponsesReferenceV1;
use south_component_conformance::responses_vocabulary::{
    event, extension, incomplete_reason, item, part,
};
use south_component_conformance::sse_split::{SseFrameV1, SseSplitErrorV1, SseSplitterV1};
use south_host_grammars::SseDecoderV1;
use south_north_codec::responses::{
    ResponsesContext, ResponsesReasoningMode, ResponsesRequestOptions, ResponsesSseState,
    chat_request_from_responses, responses_frames, responses_response,
};
use token_station_protocol::{
    ChatRequest, ChatResponse, Content, ContentPart, FinishReason, HttpResponseParts, Message,
    Role, StreamEvent, ToolCall, Usage,
};

include!("support/openai_responses_client_bodies.rs");

const PACK: &str = "fixtures-openai-responses";

fn pack_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(PACK)
}

fn read(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(pack_dir().join(name)).unwrap()).unwrap()
}

/// Every pack file of `family` with its input and expected output, in name order.
fn rows(family: &str) -> Vec<(String, Value, Value)> {
    let mut names: Vec<String> = std::fs::read_dir(pack_dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| {
            name.starts_with(&format!("provider.{family}.")) && name.ends_with(".input.json")
        })
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let expected = read(&name.replace(".input.json", ".expected.json"));
            (name.clone(), read(&name), expected)
        })
        .collect()
}

fn context(response: &ChatResponse) -> ResponsesContext {
    ResponsesContext {
        response_id: response.id.clone(),
        model: response.model.clone(),
        created_at: 1_756_367_388,
        inbound_tools: Value::Null,
        reasoning: ResponsesReasoningMode::Summary,
        allow_incomplete_tool_calls: false,
        render_legacy_encrypted_reasoning: false,
    }
}

// -- vocabulary --------------------------------------------------------------

#[test]
fn the_codec_writes_the_extension_keys_the_component_names() {
    let body = json!({
        "model": "gpt-5.5", "instructions": "Be brief.", "input": "Hi",
        "tools": [{"type": "function", "name": "f", "parameters": {"type": "object"}, "strict": true}],
        "parallel_tool_calls": true,
        "reasoning": {"effort": "low", "summary": "auto"}
    });
    let request = chat_request_from_responses(&body, &ResponsesRequestOptions::default()).unwrap();
    for key in [
        extension::TOOL_STRICT,
        extension::PARALLEL_TOOL_CALLS,
        extension::REASONING_EFFORT,
        extension::REASONING_SUMMARY,
    ] {
        assert!(request.extensions.contains_key(key), "the codec does not write `{key}`");
    }
    assert_eq!(request.messages[0].extensions[extension::TRANSIENT_INSTRUCTIONS], json!(true));
}

/// Renders IR stream events with the north codec, as SSE text.
fn rendered_stream(events: &[StreamEvent], response: &ChatResponse) -> String {
    let mut state = ResponsesSseState::new(context(response));
    let frames = responses_frames(events, &mut state).unwrap();
    frames.iter().fold(String::new(), |mut text, frame| {
        use std::fmt::Write as _;
        write!(text, "event: {}\ndata: {}\n\n", frame.event, frame.data).unwrap();
        text
    })
}

fn sample_events() -> Vec<StreamEvent> {
    vec![
        StreamEvent::ThinkingDelta { index: 0, block_index: 0, thinking_delta: "Hmm.".to_owned() },
        StreamEvent::Delta { index: 0, content: "Hi".to_owned() },
        StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("call_1".to_owned()),
            name: Some("f".to_owned()),
            arguments_delta: "{}".to_owned(),
        },
        StreamEvent::Finish { finish_reason: Some(FinishReason::ToolCalls), stop_sequence: None },
        StreamEvent::Usage {
            usage: Usage {
                input_tokens: 9,
                output_tokens: 4,
                reasoning_tokens: 2,
                ..Usage::default()
            },
        },
        StreamEvent::Done { finish_reason: None, stop_sequence: None },
    ]
}

fn sample_response() -> ChatResponse {
    ChatResponse {
        id: "resp_1".to_owned(),
        model: "gpt-5.5".to_owned(),
        choices: Vec::new(),
        usage: Usage::default(),
        extensions: token_station_protocol::Extensions::new(),
    }
}

#[test]
fn every_event_the_codec_renders_is_in_the_component_s_closed_list() {
    let text = rendered_stream(&sample_events(), &sample_response());
    let mut seen = 0;
    for frame in text.split("\n\n").filter(|frame| !frame.is_empty()) {
        let name = frame.lines().next().unwrap().trim_start_matches("event: ");
        assert!(
            event::ACCEPTED.contains(&name),
            "the codec renders `{name}`, which the list lacks"
        );
        let data: Value =
            serde_json::from_str(frame.lines().nth(1).unwrap().trim_start_matches("data: "))
                .unwrap();
        assert_eq!(data["type"], json!(name));
        seen += 1;
    }
    assert!(seen > 10, "the codec rendered too little to judge");
    for name in [
        event::CREATED,
        event::COMPLETED,
        event::OUTPUT_ITEM_ADDED,
        event::OUTPUT_TEXT_DELTA,
        event::FUNCTION_CALL_ARGUMENTS_DELTA,
        event::REASONING_SUMMARY_TEXT_DELTA,
    ] {
        assert!(text.contains(&format!("event: {name}\n")), "the codec no longer renders `{name}`");
    }
}

#[test]
fn the_codec_s_items_parts_and_incomplete_reasons_are_the_component_s() {
    let message = Message {
        role: Role::Assistant,
        content: Some(Content::Parts(vec![
            ContentPart::Thinking { thinking: "Hmm.".to_owned(), signature: None },
            ContentPart::Text { text: "Hi".to_owned() },
        ])),
        tool_calls: vec![ToolCall {
            id: "call_1".to_owned(),
            name: "f".to_owned(),
            arguments: "{}".to_owned(),
        }],
        tool_call_id: None,
        name: None,
        extensions: token_station_protocol::Extensions::new(),
    };
    for (finish, reason) in [
        (FinishReason::Length, Some(incomplete_reason::MAX_OUTPUT_TOKENS)),
        (FinishReason::ContentFilter, Some(incomplete_reason::CONTENT_FILTER)),
        (FinishReason::Stop, None),
    ] {
        let mut response = sample_response();
        response.choices = vec![token_station_protocol::Choice {
            index: 0,
            message: message.clone(),
            finish_reason: Some(finish),
            stop_sequence: None,
        }];
        let rendered = responses_response(&response, &context(&response)).unwrap();
        assert_eq!(rendered["incomplete_details"]["reason"].as_str(), reason);
        let kinds: Vec<&str> = rendered["output"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["type"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, [item::REASONING, item::MESSAGE, item::FUNCTION_CALL]);
        assert_eq!(rendered["output"][0]["summary"][0]["type"], json!(part::SUMMARY_TEXT));
        assert_eq!(rendered["output"][1]["content"][0]["type"], json!(part::OUTPUT_TEXT));
    }
    let mut raw = sample_response();
    raw.choices = vec![token_station_protocol::Choice {
        index: 0,
        message,
        finish_reason: Some(FinishReason::Stop),
        stop_sequence: None,
    }];
    let mut context = context(&raw);
    context.reasoning = ResponsesReasoningMode::RawContent;
    let rendered = responses_response(&raw, &context).unwrap();
    assert_eq!(rendered["output"][0]["content"][0]["type"], json!(part::REASONING_TEXT));

    // The input side: the codec reads the part and item types the component writes.
    let body = json!({"model": "m", "input": [
        {"role": "user", "content": [{"type": part::INPUT_TEXT, "text": "a"},
                                     {"type": part::INPUT_IMAGE, "image_url": "https://x.test/i.png"}]},
        {"role": "assistant", "content": [{"type": part::OUTPUT_TEXT, "text": "b"}]},
        {"type": item::FUNCTION_CALL, "call_id": "c", "name": "f", "arguments": "{}"},
        {"type": item::FUNCTION_CALL_OUTPUT, "call_id": "c", "output": "ok"}
    ]});
    let request = chat_request_from_responses(&body, &ResponsesRequestOptions::default()).unwrap();
    // The codec merges the assistant text and the call that follows it into one turn.
    assert_eq!(request.messages.len(), 3);
    assert_eq!(request.messages[1].tool_calls[0].name, "f");
    assert_eq!(request.messages[2].role, Role::Tool);
}

// -- round trip ------------------------------------------------------------------

/// North parse ∘ south build: the codec-derived request rows are exactly what the north codec
/// parses from the client bodies, so the pack cannot drift from the codec.
#[test]
fn the_codec_derived_request_rows_are_north_codec_outputs() {
    let config = read("provider.capabilities.declared.input.json");
    for (name, body) in client_bodies() {
        let request =
            chat_request_from_responses(&body, &ResponsesRequestOptions::default()).unwrap();
        let input = read(&format!("provider.request.{name}.input.json"));
        assert_eq!(input["provider_config"], config, "{name}");
        assert_eq!(input["chat_request"], serde_json::to_value(&request).unwrap(), "{name}");
        // And the body the component builds keeps every mapped field of the client's body.
        let built = read(&format!("provider.request.{name}.expected.json"))["body"].clone();
        for key in [
            "model",
            "temperature",
            "top_p",
            "max_output_tokens",
            "stream",
            "parallel_tool_calls",
            "tool_choice",
            "text",
        ] {
            if let Some(value) = body.get(key) {
                assert_eq!(&built[key], value, "{name}: `{key}`");
            }
        }
        assert_eq!(built["store"], json!(false), "{name}");
    }
}

/// What a consumer keeps of a response: text, tool calls, finish reason and every usage bucket.
fn kept(response: &ChatResponse) -> (String, Vec<ToolCall>, Option<FinishReason>, Usage) {
    let choice = &response.choices[0];
    let text = match &choice.message.content {
        None => String::new(),
        Some(Content::Text(text)) => text.clone(),
        Some(Content::Parts(parts)) => parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect(),
    };
    (text, choice.message.tool_calls.clone(), choice.finish_reason.clone(), response.usage)
}

fn parse(body: &str) -> ChatResponse {
    let parts: HttpResponseParts =
        serde_json::from_value(json!({"status": 200, "headers": {}, "body": body})).unwrap();
    OpenAiResponsesReferenceV1.parse_response(&parts).unwrap()
}

/// South parse ∘ north render ∘ south parse, over every response row the component accepts.
#[test]
fn a_parsed_response_survives_the_north_codec_and_back() {
    let mut judged = 0;
    for (name, input, expected) in rows("response") {
        if expected.get("error").is_some() {
            continue;
        }
        let first = parse(input["body"].as_str().unwrap());
        let rendered = responses_response(&first, &context(&first)).unwrap();
        let second = parse(&rendered.to_string());
        assert_eq!(kept(&second), kept(&first), "{name}");
        judged += 1;
    }
    assert!(judged >= 8, "too few response rows to judge");
}

fn feed(body: &[u8]) -> Result<Vec<StreamEvent>, token_station_protocol::ErrorEnvelope> {
    let mut parser = OpenAiResponsesReferenceV1.stream_parser();
    let mut events = parser.parse_chunk(body)?;
    events.extend(parser.finish()?);
    Ok(events)
}

/// The same for streams: the component's events, rendered by the north codec, parse back through
/// the component's strict evidence rules to the same text, calls, finish and usage.
#[test]
fn a_parsed_stream_survives_the_north_codec_and_back() {
    // Per call: its id, name and whole arguments, however the fragments were cut.
    let summarize = |events: &[StreamEvent]| {
        let mut text = String::new();
        let mut calls: std::collections::BTreeMap<u32, (Option<String>, Option<String>, String)> =
            std::collections::BTreeMap::new();
        let mut finish = None;
        let mut usage = Usage::default();
        for event in events {
            match event {
                StreamEvent::Delta { content, .. } => text.push_str(content),
                StreamEvent::ToolCallDelta { index, id, name, arguments_delta } => {
                    let call = calls.entry(*index).or_default();
                    if id.is_some() {
                        call.0.clone_from(id);
                    }
                    if name.is_some() {
                        call.1.clone_from(name);
                    }
                    call.2.push_str(arguments_delta);
                }
                StreamEvent::Finish { finish_reason, .. } => finish.clone_from(finish_reason),
                StreamEvent::Usage { usage: report } => usage = *report,
                _ => {}
            }
        }
        (text, calls, finish, usage)
    };
    let mut judged = 0;
    for (name, input, expected) in rows("stream") {
        let Some(events) = expected.as_array() else { continue };
        if events.iter().any(|event| event["type"] == "error") {
            continue;
        }
        let body: String = input["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|chunk| chunk.as_str().unwrap())
            .collect();
        let first = feed(body.as_bytes()).unwrap();
        let rendered = rendered_stream(&first, &sample_response());
        let second = feed(rendered.as_bytes()).unwrap_or_else(|error| panic!("{name}: {error:?}"));
        assert_eq!(summarize(&second), summarize(&first), "{name}");
        judged += 1;
    }
    assert!(judged >= 8, "too few stream rows to judge");
}

// -- SSE ------------------------------------------------------------------------

fn split_all(chunks: &[&[u8]]) -> Result<Vec<(String, String)>, SseSplitErrorV1> {
    let mut splitter = SseSplitterV1::new();
    let mut frames = Vec::new();
    for chunk in chunks {
        frames.extend(splitter.push(chunk)?);
    }
    frames.extend(splitter.finish()?);
    Ok(frames.into_iter().map(|SseFrameV1 { event, data }| (event, data)).collect())
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

/// The decoder's 45 golden vectors, whole, byte by byte and at every single split.
#[test]
fn the_decoder_golden_vectors_hold_for_the_component_splitter() {
    let vectors: Value = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../south-host-grammars/tests/vectors/sse-v1.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let cases = vectors["cases"].as_array().unwrap();
    assert!(cases.len() >= 40, "the vector table lost cases");
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let input = case["input"].as_str().map_or_else(
            || hex(case["input_hex"].as_str().unwrap()),
            |text| text.as_bytes().to_vec(),
        );
        let expected = match case.get("error").and_then(Value::as_str) {
            // The vectors' only errors are UTF-8 ones; the size bounds are not in the table.
            Some("not_utf8") => Err(SseSplitErrorV1::NotUtf8),
            Some(other) => panic!("{name}: unexpected error vector {other}"),
            None => Ok(case["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|event| {
                    (
                        event["event"].as_str().unwrap().to_owned(),
                        event["data"].as_str().unwrap().to_owned(),
                    )
                })
                .collect::<Vec<_>>()),
        };
        assert_eq!(split_all(&[&input]), expected, "{name}: whole");
        let bytes: Vec<&[u8]> = input.chunks(1).collect();
        assert_eq!(split_all(&bytes), expected, "{name}: byte by byte");
        for at in 0..=input.len() {
            let (head, tail) = input.split_at(at);
            assert_eq!(
                split_all(&<[&[u8]; 2]>::from((head, tail))),
                expected,
                "{name}: split at {at}"
            );
        }
    }
}

/// The host's pass-through cuts the upstream bytes where `SseDecoderV1::position` marks each
/// event's end and hands the component one slice per call (§8.2). Over every stream row and the
/// decoder's own Responses vector, each slice gives the component's splitter exactly the
/// decoder's event and leaves it holding nothing.
#[test]
fn frames_cut_at_decoder_positions_are_whole_frames_for_the_component() {
    let mut bodies: Vec<(String, Vec<u8>)> = rows("stream")
        .into_iter()
        .map(|(name, input, _)| {
            let body: String =
                input["chunks"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
            (name, body.into_bytes())
        })
        .collect();
    bodies.push((
        "mixed line ends".to_owned(),
        b"data: a\r\ndata: b\rdata: c\n\r\nevent: x\rdata: y\r\r".to_vec(),
    ));
    let mut slices_judged = 0;
    for (name, body) in bodies {
        let mut decoder = SseDecoderV1::new();
        decoder.push(&body);
        let mut cuts = Vec::new();
        while let Some(event) = decoder.next_event().unwrap() {
            cuts.push((usize::try_from(decoder.position()).unwrap(), event));
        }
        if let Some(last) = decoder.finish().unwrap() {
            cuts.push((body.len(), last));
        }
        let mut splitter = SseSplitterV1::new();
        let mut start = 0;
        for (end, event) in cuts {
            let frames = splitter.push(&body[start..end]).unwrap();
            assert_eq!(
                frames,
                [SseFrameV1 { event: event.event().to_owned(), data: event.data().to_owned() }],
                "{name}: the slice ending at {end}"
            );
            assert!(splitter.is_idle(), "{name}: the splitter holds part of a frame after {end}");
            start = end;
            slices_judged += 1;
        }
    }
    assert!(slices_judged > 100, "too few slices to judge");
}

/// The same slices fed to the component parser one per call give the events of the whole body:
/// one frame per call changes nothing.
#[test]
fn one_frame_per_call_gives_the_same_events() {
    for (name, input, expected) in rows("stream") {
        let body: String =
            input["chunks"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
        let mut decoder = SseDecoderV1::new();
        decoder.push(body.as_bytes());
        let mut cuts = Vec::new();
        while decoder.next_event().unwrap().is_some() {
            cuts.push(usize::try_from(decoder.position()).unwrap());
        }
        let mut parser = OpenAiResponsesReferenceV1.stream_parser();
        let mut events = Vec::new();
        let mut start = 0;
        let mut outcome = Ok(());
        for end in cuts.into_iter().chain([body.len()]) {
            match parser.parse_chunk(&body.as_bytes()[start..end]) {
                Ok(more) => events.extend(more),
                Err(error) => {
                    outcome = Err(error);
                    break;
                }
            }
            start = end;
        }
        if outcome.is_ok() {
            match parser.finish() {
                Ok(more) => events.extend(more),
                Err(error) => outcome = Err(error),
            }
        }
        let produced = match outcome {
            Ok(()) => serde_json::to_value(&events).unwrap(),
            Err(error) => json!({"error": error}),
        };
        assert_eq!(produced, expected, "{name}");
    }
}

#[test]
fn a_request_built_from_the_codec_round_trips_its_tools() {
    // The tool restore map of a plain-function request is the identity (record §8.2 condition 2):
    // the built body names each tool as the client did.
    let (_, body) =
        client_bodies().into_iter().find(|(name, _)| *name == "tools-and-tool-choice").unwrap();
    let request: ChatRequest =
        chat_request_from_responses(&body, &ResponsesRequestOptions::default()).unwrap();
    let names: Vec<&str> = request.tools.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(names, ["get_weather", "get_time"]);
}

mod properties {
    use std::fmt::Write as _;

    use proptest::collection::vec;
    use proptest::prelude::*;
    use proptest::sample::select;
    use south_component_conformance::sse_split::{SseSplitErrorV1, SseSplitterV1};
    use south_host_grammars::decode_sse_v1;

    /// Bytes drawn mostly from SSE syntax, with the BOM's bytes and one invalid UTF-8 byte.
    fn sse_bytes() -> impl Strategy<Value = Vec<u8>> {
        vec(
            select(vec![
                b'd', b'a', b't', b'e', b'v', b'n', b'i', b':', b' ', b'\r', b'\n', b'x', b'{',
                b'}', 0xEF, 0xBB, 0xBF, 0xFF,
            ]),
            0..160,
        )
    }

    fn split(chunks: &[&[u8]]) -> Result<Vec<(String, String)>, SseSplitErrorV1> {
        let mut splitter = SseSplitterV1::new();
        let mut frames = Vec::new();
        for chunk in chunks {
            frames.extend(splitter.push(chunk)?);
        }
        frames.extend(splitter.finish()?);
        Ok(frames.into_iter().map(|frame| (frame.event, frame.data)).collect())
    }

    proptest! {
        /// The component's splitter and the host's decoder agree on every input, and a split
        /// anywhere changes nothing (the record §6, as amended 2026-10-10).
        #[test]
        fn the_splitter_agrees_with_the_decoder(bytes in sse_bytes(), at in any::<usize>()) {
            let whole = split(&[&bytes]);
            let decoded = decode_sse_v1(&bytes);
            if let Ok(events) = &decoded {
                let events: Vec<(String, String)> = events
                    .iter()
                    .map(|event| (event.event().to_owned(), event.data().to_owned()))
                    .collect();
                prop_assert_eq!(&whole, &Ok(events));
            } else {
                prop_assert_eq!(&whole, &Err(SseSplitErrorV1::NotUtf8));
            }
            let at = at % (bytes.len() + 1);
            let (head, tail) = bytes.split_at(at);
            prop_assert_eq!(split(&<[&[u8]; 2]>::from((head, tail))), whole);
        }

        /// The parser never panics on arbitrary frames, and how the bytes are chunked never
        /// changes its events or its first error.
        #[test]
        fn chunking_never_changes_what_the_parser_says(
            frames in vec((select(vec![
                "response.created", "response.output_text.delta", "response.completed",
                "response.failed", "error", "response.unknown",
            ]), any::<u8>(), any::<bool>()), 0..8),
            stride in 1usize..17,
        ) {
            use south_component_conformance::ProviderComponentV1;
            use south_component_conformance::reference_openai_responses::OpenAiResponsesReferenceV1;
            let mut body = String::new();
            for (index, (kind, sequence, with_usage)) in frames.iter().enumerate() {
                let usage = if *with_usage {
                    r#","usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}"#
                } else {
                    ""
                };
                write!(
                    body,
                    "event: {kind}\ndata: {{\"type\":\"{kind}\",\"sequence_number\":{sequence},\
                     \"item_id\":\"m\",\"output_index\":0,\"content_index\":0,\"delta\":\"d{index}\",\
                     \"response\":{{\"id\":\"r\",\"object\":\"response\",\"status\":\"completed\",\
                     \"output\":[]{usage}}}}}\n\n"
                )
                .unwrap();
            }
            let run = |stride: usize| {
                let mut parser = OpenAiResponsesReferenceV1.stream_parser();
                let mut events = Vec::new();
                for piece in body.as_bytes().chunks(stride) {
                    match parser.parse_chunk(piece) {
                        Ok(more) => events.extend(more),
                        Err(error) => return format!("{error:?}"),
                    }
                }
                match parser.finish() {
                    Ok(more) => events.extend(more),
                    // An error discards the events of its own call, so only the error is compared.
                        Err(error) => return format!("{error:?}"),
                }
                format!("{events:?}")
            };
            prop_assert_eq!(run(stride), run(body.len().max(1)));
        }
    }
}
