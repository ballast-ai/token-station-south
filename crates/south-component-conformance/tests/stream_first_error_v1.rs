//! The SSE-bodied provider parsers report the first error of a stream in byte order, whatever the
//! chunking (stream parser first-error record 2026-11-04, F2).
//!
//! `SseSplitterV1::push` used to refuse a whole chunk at its first line that is not UTF-8 and
//! drop the frames it had completed before that line, so a frame that was itself invalid was
//! never judged. Fed in small chunks, that frame arrived in an earlier call and its error won; fed
//! in one chunk, the UTF-8 refusal won. The scheduled fuzz run of 2026-10-26 found it in
//! `provider-openai-responses`; the other three parsers that split with `SseSplitterV1` had the
//! same shape.
//!
//! Each case is an invalid frame followed by a line that is not UTF-8. The error fed in one chunk
//! and fed byte by byte must both be the error of the invalid frame alone, so it must not be the
//! UTF-8 refusal.

use south_component_conformance::ProviderComponentV1;
use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::reference_anthropic::AnthropicReferenceV1;
use south_component_conformance::reference_gemini::GeminiReferenceV1;
use south_component_conformance::reference_openai_responses::OpenAiResponsesReferenceV1;
use south_component_conformance::reference_vertex::VertexReferenceV1;

/// A frame whose data is not JSON. It is named, because the Messages parser reads only named
/// events; every SSE parser refuses it on its own.
const INVALID_FRAME: &[u8] = b"event: message_start\ndata: {\"truncated\":\n\n";
/// A line that is not UTF-8, after the invalid frame.
const NOT_UTF8_LINE: &[u8] = b"data: \xff\n\n";

/// The first error of the stream fed as `chunks` and then a clean end, as text.
fn first_error(component: &dyn ProviderComponentV1, chunks: &[&[u8]]) -> String {
    let mut parser = component.stream_parser();
    for chunk in chunks.iter().copied().chain(std::iter::once(&[][..])) {
        if let Err(error) = parser.parse_chunk(chunk) {
            return format!("{error:?}");
        }
    }
    panic!("the stream was accepted");
}

fn assert_first_error_in_byte_order(name: &str, component: &dyn ProviderComponentV1) {
    let alone = first_error(component, &[INVALID_FRAME]);
    let not_utf8 = first_error(component, &[NOT_UTF8_LINE]);
    assert_ne!(alone, not_utf8, "{name}: the two refusals must be distinguishable");

    let body = [INVALID_FRAME, NOT_UTF8_LINE].concat();
    let whole = first_error(component, &[&body]);
    let bytes: Vec<&[u8]> = body.chunks(1).collect();
    let byte_by_byte = first_error(component, &bytes);
    assert_eq!(whole, alone, "{name}: fed in one chunk");
    assert_eq!(byte_by_byte, alone, "{name}: fed byte by byte");
}

#[test]
fn openai_compatible_reports_the_invalid_frame_first() {
    assert_first_error_in_byte_order("provider-openai-compatible", &OpenAiCompatibleReferenceV1);
}

#[test]
fn anthropic_reports_the_invalid_frame_first() {
    assert_first_error_in_byte_order("provider-anthropic", &AnthropicReferenceV1);
}

#[test]
fn gemini_reports_the_invalid_frame_first() {
    assert_first_error_in_byte_order("provider-gemini", &GeminiReferenceV1);
}

#[test]
fn vertex_reports_the_invalid_frame_first() {
    assert_first_error_in_byte_order("provider-vertex", &VertexReferenceV1);
}

#[test]
fn openai_responses_reports_the_invalid_frame_first() {
    assert_first_error_in_byte_order("provider-openai-responses", &OpenAiResponsesReferenceV1);
}
