//! The SSE-bodied provider components cut frames by the host decoder's line-end rules.
//!
//! A host cuts an SSE body with `SseDecoderV1` — LF, CR and CRLF line ends, mixable — and hands
//! the component each frame's raw bytes. `provider-openai-compatible`, `provider-anthropic` and
//! `provider-gemini` (and `provider-vertex`, which shares the Gemini parser) used to find frame
//! boundaries only at `\n\n` and `\r\n\r\n`, so a frame the host closed with a lone-CR blank line
//! was held forever and dropped at the end of the stream. They now split with `SseSplitterV1`.
//!
//! Every stream fixture of the three packs is re-spelled with CR-only and with mixed line ends and
//! fed whole, byte by byte, and frame by frame; the events (or the refusal) must equal those of the
//! fixture as recorded. The eventstream packages (`provider-kiro`,
//! `provider-anthropic-bedrock-invoke`, the Converse pair) read the host's canonical re-encoding,
//! which only ever writes `\n\n`, and are not in scope.

use std::path::Path;

use serde_json::Value;
use south_component_conformance::ProviderComponentV1;
use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::reference_anthropic::AnthropicReferenceV1;
use south_component_conformance::reference_gemini::GeminiReferenceV1;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Each stream fixture of `pack` as `(case, recorded chunks)`.
fn stream_cases(pack: &str) -> Vec<(String, Vec<String>)> {
    let mut cases: Vec<_> = std::fs::read_dir(root().join(pack))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.starts_with("provider.stream.") && name.ends_with(".input.json"))
        .map(|name| {
            let input: Value = serde_json::from_str(
                &std::fs::read_to_string(root().join(pack).join(&name)).unwrap(),
            )
            .unwrap();
            let chunks = input["chunks"]
                .as_array()
                .expect("a stream fixture lists its chunks")
                .iter()
                .map(|chunk| chunk.as_str().expect("text chunks").to_owned())
                .collect();
            (name, chunks)
        })
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "{pack} has stream fixtures");
    cases
}

/// The events of one stream, or its refusal, as JSON so the two outcomes compare by value.
fn run(component: &dyn ProviderComponentV1, chunks: &[Vec<u8>]) -> Value {
    let mut parser = component.stream_parser();
    let mut events = Vec::new();
    for chunk in chunks.iter().map(Vec::as_slice).chain(std::iter::once(&[][..])) {
        match parser.parse_chunk(chunk) {
            Ok(more) => events.extend(more),
            Err(error) => {
                return serde_json::json!({"events": events, "error": error});
            }
        }
    }
    serde_json::json!({"events": events})
}

/// `body` with every line end spelled as `ends` gives in turn (normalised from LF first).
fn respell(body: &str, ends: &[&str]) -> String {
    let normalised = body.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(normalised.len() * 2);
    let mut index = 0;
    for piece in normalised.split_inclusive('\n') {
        if let Some(line) = piece.strip_suffix('\n') {
            out.push_str(line);
            out.push_str(ends[index % ends.len()]);
            index += 1;
        } else {
            out.push_str(piece);
        }
    }
    out
}

/// The frames a host cuts from `body`: every blank line closes one, under LF, CR or CRLF.
fn host_frames(body: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut start = 0;
    let mut line_start = 0;
    let mut at = 0;
    while at < body.len() {
        let end = match body[at] {
            b'\r' if body.get(at + 1) == Some(&b'\n') => at + 2,
            b'\r' | b'\n' => at + 1,
            _ => {
                at += 1;
                continue;
            }
        };
        if at == line_start {
            frames.push(body[start..end].to_vec());
            start = end;
        }
        line_start = end;
        at = end;
    }
    if start < body.len() {
        frames.push(body[start..].to_vec());
    }
    frames
}

fn assert_line_ends_are_irrelevant(pack: &str, component: &dyn ProviderComponentV1) {
    for (case, chunks) in stream_cases(pack) {
        let recorded: Vec<Vec<u8>> = chunks.iter().map(|chunk| chunk.as_bytes().to_vec()).collect();
        let expected = run(component, &recorded);
        let body = chunks.concat();
        for (spelling, ends) in
            [("cr", &["\r"][..]), ("crlf", &["\r\n"][..]), ("mixed", &["\n", "\r", "\r\n"][..])]
        {
            let respelled = respell(&body, ends).into_bytes();
            let whole = vec![respelled.clone()];
            let bytes: Vec<Vec<u8>> = respelled.iter().map(|byte| vec![*byte]).collect();
            let frames = host_frames(&respelled);
            for (feed, chunks) in
                [("whole", whole), ("byte by byte", bytes), ("frame by frame", frames)]
            {
                let got = run(component, &chunks);
                // A chunk that ends in a refusal returns no events, so a body fed whole loses the
                // events of the frames before the refused one; only the refusal is comparable.
                if feed == "whole" && expected.get("error").is_some() {
                    assert_eq!(
                        got["error"], expected["error"],
                        "{pack}/{case}: {spelling} fed {feed}"
                    );
                    continue;
                }
                assert_eq!(got, expected, "{pack}/{case}: {spelling} line ends fed {feed}");
            }
        }
    }
}

#[test]
fn openai_compatible_streams_read_the_same_under_every_line_end() {
    assert_line_ends_are_irrelevant("fixtures", &OpenAiCompatibleReferenceV1);
}

#[test]
fn anthropic_streams_read_the_same_under_every_line_end() {
    assert_line_ends_are_irrelevant("fixtures-anthropic", &AnthropicReferenceV1);
}

#[test]
fn gemini_streams_read_the_same_under_every_line_end() {
    assert_line_ends_are_irrelevant("fixtures-gemini", &GeminiReferenceV1);
}

#[test]
fn a_frame_closed_by_a_lone_cr_blank_line_is_read_before_the_stream_ends() {
    // The defect this file guards: the host hands over one complete frame ending in a lone-CR blank
    // line, and the component must emit its delta at once rather than hold it for a `\n\n` that
    // never comes.
    let mut parser = OpenAiCompatibleReferenceV1.stream_parser();
    let events = parser
        .parse_chunk(b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\r\r")
        .unwrap();
    assert_eq!(events.len(), 1, "{events:?}");
}
