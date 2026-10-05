//! Self-test of `south.eventstream-framing.v1` (gate ③ of B2, design record §5.2 and §5.4).
//!
//! The suite is only worth something if a correct host passes it and each plausible wrong host
//! fails it where it should. So this file carries a reference host built on South's own deframer
//! and re-encoding, shows that it passes every case, and then breaks it one way at a time: each
//! broken host fails exactly the cases that guard what it broke, and no other.

use std::fmt::Display;

use south_contracts::{
    AwsEventStreamDeframerV1, EventStreamErrorV1, EventStreamHeaderValueV1, EventStreamMessageV1,
    deframe_aws_eventstream_v1, reencode_eventstream_v1,
};
use south_provider_api::StreamFramingV1;
use south_provider_conformance::{
    EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_ID, EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_VERSION,
    EventStreamBufferedObservationV1, EventStreamFeedObservationV1, EventStreamFramingBodyV1,
    EventStreamFramingCaseIdV1, EventStreamFramingConformanceFailureV1,
    EventStreamFramingFixtureV1, EventStreamFramingHarnessV1, EventStreamFramingPathV1,
    EventStreamOutcomeV1, eventstream_framing_fixtures_v1, run_eventstream_framing_conformance_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(EventStreamFramingFixtureV1: Display);
assert_not_impl_any!(EventStreamFeedObservationV1: Display);

use EventStreamFramingCaseIdV1 as Case;

#[test]
fn suite_identity_and_canonical_case_order_are_frozen() {
    assert_eq!(EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_VERSION, 1);
    assert_eq!(EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_ID, "south.eventstream-framing.v1");
    let case_ids: Vec<_> = eventstream_framing_fixtures_v1()
        .iter()
        .map(EventStreamFramingFixtureV1::case_id)
        .collect();
    assert_eq!(
        case_ids,
        [
            Case::BytesFramingPassesBytesUnchanged,
            Case::EventFramesReencodeCanonically,
            Case::ExceptionAndErrorFramesPassThrough,
            Case::SplitAtEveryByteBoundaryIsIdentical,
            Case::ChecksumMismatchFailsAfterEarlierMessages,
            Case::TruncatedTailFailsAtEndOfInput,
            Case::NonJsonPayloadFails,
            Case::BufferedPathConcatenatesTheReencoding,
            Case::BufferedPathFailsOnAnyFramingError,
        ]
    );
}

/// The fixtures' expectations agree with South's own deframer and re-encoding, so a reference
/// host built on them is the one the suite describes.
#[test]
fn expectations_agree_with_south_deframer() {
    for fixture in eventstream_framing_fixtures_v1() {
        for body in fixture.bodies() {
            let bytes = body.bytes();
            let (delivered, completed) = match fixture.path() {
                EventStreamFramingPathV1::Streaming { framing: StreamFramingV1::Bytes, .. } => {
                    (bytes.clone(), true)
                }
                EventStreamFramingPathV1::Streaming { .. } => {
                    let observed = reference_stream(&[&bytes]);
                    (observed.fed().concat(), observed.outcome() == EventStreamOutcomeV1::Completed)
                }
                EventStreamFramingPathV1::Buffered => match reference_buffered(&bytes) {
                    EventStreamBufferedObservationV1::Parsed(text) => (text.into_bytes(), true),
                    EventStreamBufferedObservationV1::Failed => (Vec::new(), false),
                },
            };
            let expected = fixture.expected();
            assert_eq!(completed, expected.completes(), "{:?} {body:?}", fixture.case_id());
            assert_eq!(
                delivered,
                expected.delivered().bytes(&bytes),
                "{:?} {body:?}",
                fixture.case_id()
            );
        }
    }
}

/// Each fault body really carries the fault its case names.
#[test]
fn fault_bodies_carry_the_fault_they_name() {
    let error = |body: EventStreamFramingBodyV1| {
        deframe_aws_eventstream_v1(&body.bytes())
            .and_then(|messages| {
                messages.iter().map(reencode_eventstream_v1).collect::<Result<String, _>>()
            })
            .err()
    };
    assert_eq!(
        error(EventStreamFramingBodyV1::ChecksumMismatchMidStream),
        Some(EventStreamErrorV1::MessageChecksumMismatch)
    );
    assert_eq!(
        error(EventStreamFramingBodyV1::TruncatedTail),
        Some(EventStreamErrorV1::TruncatedFrame)
    );
    assert_eq!(
        error(EventStreamFramingBodyV1::NonJsonPayload),
        Some(EventStreamErrorV1::PayloadNotJson)
    );
    // The checksum body has a valid frame after the corrupt one, so a host that resynchronises
    // past a bad frame delivers something the suite can see.
    let bytes = EventStreamFramingBodyV1::ChecksumMismatchMidStream.bytes();
    let (frames, truncated) = split_frames(&bytes);
    assert_eq!(frames.len(), 3);
    assert!(!truncated);
    assert!(deframe_aws_eventstream_v1(frames[2]).is_ok());
}

/// The exception and error frames carry the headers the canonical forms read.
#[test]
fn exception_and_error_body_carries_all_three_message_types() {
    let messages =
        deframe_aws_eventstream_v1(&EventStreamFramingBodyV1::ExceptionAndError.bytes()).unwrap();
    let types: Vec<_> = messages
        .iter()
        .map(|message| match message.header(":message-type") {
            Some(EventStreamHeaderValueV1::String(value)) => value.clone(),
            other => panic!("unexpected message type {other:?}"),
        })
        .collect();
    assert_eq!(types, ["event", "exception", "error"]);
}

#[test]
fn debug_output_carries_no_payload_text() {
    let rendered = format!("{:?}", eventstream_framing_fixtures_v1());
    assert!(!rendered.contains("assistant"), "{rendered}");
    assert!(!rendered.contains("event:"), "{rendered}");
}

// ---------------------------------------------------------------------------------------------
// The reference host and its broken variants.
// ---------------------------------------------------------------------------------------------

/// One wiring mistake made on purpose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    None,
    /// Deframes every stream as eventstream, whatever the package declares.
    DeframesBytesFraming,
    /// Feeds eventstream upstream bytes unchanged, whatever the package declares.
    PassesEventstreamRaw,
    /// Builds the SSE frame from the payload as framed, without the canonical compaction.
    PayloadNotCompacted,
    /// Feeds only `event` messages and drops `exception` and `error` ones.
    DropsExceptionAndErrorFrames,
    /// Starts a fresh deframer for every chunk, so a frame split across chunks is lost.
    FreshDeframerPerChunk,
    /// Skips a frame that fails to deframe and carries on with the next one.
    ResyncsPastBadFrame,
    /// Never calls `finish`, so a truncated tail at end of input goes unnoticed.
    SkipsFinish,
    /// Passes a payload that is not JSON through as its raw text.
    PassesNonJsonPayloadRaw,
    /// Feeds nothing at all unless the whole stream deframes, so a fault swallows the messages
    /// before it.
    FailsWithoutDelivering,
    /// The buffered path builds frames without the canonical compaction.
    BufferedPayloadNotCompacted,
    /// The buffered path hands `parse-response` whatever deframed before a fault.
    BufferedKeepsPartialResult,
}

struct ReferenceHost {
    fault: Fault,
}

impl EventStreamFramingHarnessV1 for ReferenceHost {
    fn stream(&self, framing: StreamFramingV1, chunks: &[&[u8]]) -> EventStreamFeedObservationV1 {
        let deframe = match self.fault {
            Fault::DeframesBytesFraming => true,
            Fault::PassesEventstreamRaw => false,
            _ => framing == StreamFramingV1::AwsEventstream,
        };
        if !deframe {
            let fed = chunks.iter().map(|chunk| chunk.to_vec()).collect();
            return EventStreamFeedObservationV1::new(fed, EventStreamOutcomeV1::Completed);
        }
        match self.fault {
            Fault::FreshDeframerPerChunk => {
                let mut fed = Vec::new();
                for chunk in chunks {
                    let observed = drive(&[chunk], self.fault);
                    let failed = observed.outcome() == EventStreamOutcomeV1::Failed;
                    fed.extend(observed.fed().iter().cloned());
                    if failed {
                        return EventStreamFeedObservationV1::new(
                            fed,
                            EventStreamOutcomeV1::Failed,
                        );
                    }
                }
                EventStreamFeedObservationV1::new(fed, EventStreamOutcomeV1::Completed)
            }
            Fault::ResyncsPastBadFrame => {
                let whole = chunks.concat();
                let (frames, truncated) = split_frames(&whole);
                let mut fed = Vec::new();
                for frame in frames {
                    if let Ok(messages) = deframe_aws_eventstream_v1(frame) {
                        for message in messages {
                            match reencode_eventstream_v1(&message) {
                                Ok(sse) => fed.push(sse.into_bytes()),
                                Err(_) => {
                                    return EventStreamFeedObservationV1::new(
                                        fed,
                                        EventStreamOutcomeV1::Failed,
                                    );
                                }
                            }
                        }
                    }
                }
                let outcome = if truncated {
                    EventStreamOutcomeV1::Failed
                } else {
                    EventStreamOutcomeV1::Completed
                };
                EventStreamFeedObservationV1::new(fed, outcome)
            }
            Fault::FailsWithoutDelivering => {
                let observed = drive(chunks, Fault::None);
                if observed.outcome() == EventStreamOutcomeV1::Failed {
                    EventStreamFeedObservationV1::new(Vec::new(), EventStreamOutcomeV1::Failed)
                } else {
                    observed
                }
            }
            fault => drive(chunks, fault),
        }
    }

    fn buffered(&self, body: &[u8]) -> EventStreamBufferedObservationV1 {
        match self.fault {
            Fault::BufferedPayloadNotCompacted => {
                let text = deframe_aws_eventstream_v1(body).and_then(|messages| {
                    messages
                        .iter()
                        .map(|message| {
                            encode(message, Fault::PayloadNotCompacted).unwrap_or(Ok(String::new()))
                        })
                        .collect::<Result<String, _>>()
                });
                text.map_or(
                    EventStreamBufferedObservationV1::Failed,
                    EventStreamBufferedObservationV1::Parsed,
                )
            }
            Fault::BufferedKeepsPartialResult => {
                let observed = drive(&[body], Fault::None);
                let text = String::from_utf8(observed.fed().concat()).unwrap();
                EventStreamBufferedObservationV1::Parsed(text)
            }
            _ => reference_buffered(body),
        }
    }
}

/// The correct streaming path: one deframer across chunks, drained after every push, each
/// message re-encoded canonically, `finish` at end of input.
fn reference_stream(chunks: &[&[u8]]) -> EventStreamFeedObservationV1 {
    drive(chunks, Fault::None)
}

/// The correct buffered path: the whole body deframed and the re-encodings concatenated, or
/// nothing on any error.
fn reference_buffered(body: &[u8]) -> EventStreamBufferedObservationV1 {
    let text = deframe_aws_eventstream_v1(body)
        .and_then(|messages| messages.iter().map(reencode_eventstream_v1).collect());
    text.map_or(EventStreamBufferedObservationV1::Failed, EventStreamBufferedObservationV1::Parsed)
}

/// The streaming loop, with the faults that live inside it.
fn drive(chunks: &[&[u8]], fault: Fault) -> EventStreamFeedObservationV1 {
    let failed = |fed| EventStreamFeedObservationV1::new(fed, EventStreamOutcomeV1::Failed);
    let mut deframer = AwsEventStreamDeframerV1::new();
    let mut fed = Vec::new();
    for chunk in chunks {
        deframer.push(chunk);
        loop {
            match deframer.next_message() {
                Ok(Some(message)) => match encode(&message, fault) {
                    Some(Ok(sse)) => fed.push(sse.into_bytes()),
                    Some(Err(_)) => return failed(fed),
                    None => {}
                },
                Ok(None) => break,
                Err(_) => return failed(fed),
            }
        }
    }
    if fault != Fault::SkipsFinish && deframer.finish().is_err() {
        return failed(fed);
    }
    EventStreamFeedObservationV1::new(fed, EventStreamOutcomeV1::Completed)
}

/// Re-encodes one message under `fault`; `None` when the fault drops it.
fn encode(
    message: &EventStreamMessageV1,
    fault: Fault,
) -> Option<Result<String, EventStreamErrorV1>> {
    let message_type = match message.header(":message-type") {
        Some(EventStreamHeaderValueV1::String(value)) => value.as_str(),
        _ => "",
    };
    match fault {
        // The payload is still validated; only the compaction is skipped.
        Fault::PayloadNotCompacted => {
            Some(reencode_eventstream_v1(message).map(|_| uncompacted(message)))
        }
        Fault::DropsExceptionAndErrorFrames if message_type != "event" => None,
        Fault::PassesNonJsonPayloadRaw => Some(reencode_eventstream_v1(message).or_else(|error| {
            if error == EventStreamErrorV1::PayloadNotJson {
                Ok(uncompacted(message))
            } else {
                Err(error)
            }
        })),
        _ => Some(reencode_eventstream_v1(message)),
    }
}

/// The canonical frame, except that an `event` or `exception` payload is carried exactly as
/// framed.
fn uncompacted(message: &EventStreamMessageV1) -> String {
    let string = |name| match message.header(name) {
        Some(EventStreamHeaderValueV1::String(value)) => value.clone(),
        _ => String::new(),
    };
    let payload = String::from_utf8_lossy(message.payload());
    match string(":message-type").as_str() {
        "event" => format!("event: {}\ndata: {payload}\n\n", string(":event-type")),
        "exception" => {
            format!("event: exception:{}\ndata: {payload}\n\n", string(":exception-type"))
        }
        _ => reencode_eventstream_v1(message).unwrap_or_default(),
    }
}

/// Splits an eventstream body into frames by their length prelude, trusting the lengths: what a
/// host that resynchronises does. The flag reports a last frame shorter than it declares.
fn split_frames(bytes: &[u8]) -> (Vec<&[u8]>, bool) {
    let mut frames = Vec::new();
    let mut rest = bytes;
    while let Some(prelude) = rest.first_chunk::<4>() {
        let declared = usize::try_from(u32::from_be_bytes(*prelude)).unwrap().max(4);
        if declared > rest.len() {
            return (frames, true);
        }
        let (frame, tail) = rest.split_at(declared);
        frames.push(frame);
        rest = tail;
    }
    (frames, !rest.is_empty())
}

fn run(fault: Fault) -> Result<Vec<Case>, EventStreamFramingConformanceFailureV1> {
    run_eventstream_framing_conformance_v1(&ReferenceHost { fault })
        .map(|report| report.passed_case_ids().to_vec())
}

#[test]
fn the_reference_host_passes_every_case() {
    let passed = run(Fault::None).unwrap();
    assert_eq!(passed.len(), eventstream_framing_fixtures_v1().len());
}

#[test]
fn each_wiring_mistake_fails_exactly_the_cases_that_guard_it() {
    let aws_streaming = [
        Case::EventFramesReencodeCanonically,
        Case::ExceptionAndErrorFramesPassThrough,
        Case::SplitAtEveryByteBoundaryIsIdentical,
        Case::ChecksumMismatchFailsAfterEarlierMessages,
        Case::TruncatedTailFailsAtEndOfInput,
        Case::NonJsonPayloadFails,
    ];
    let expectations: &[(Fault, &[Case])] = &[
        (Fault::DeframesBytesFraming, &[Case::BytesFramingPassesBytesUnchanged]),
        (Fault::PassesEventstreamRaw, &aws_streaming),
        (
            Fault::PayloadNotCompacted,
            &[Case::EventFramesReencodeCanonically, Case::SplitAtEveryByteBoundaryIsIdentical],
        ),
        (Fault::DropsExceptionAndErrorFrames, &[Case::ExceptionAndErrorFramesPassThrough]),
        (Fault::FreshDeframerPerChunk, &[Case::SplitAtEveryByteBoundaryIsIdentical]),
        (Fault::ResyncsPastBadFrame, &[Case::ChecksumMismatchFailsAfterEarlierMessages]),
        (Fault::SkipsFinish, &[Case::TruncatedTailFailsAtEndOfInput]),
        (Fault::PassesNonJsonPayloadRaw, &[Case::NonJsonPayloadFails]),
        (
            Fault::FailsWithoutDelivering,
            &[
                Case::ChecksumMismatchFailsAfterEarlierMessages,
                Case::TruncatedTailFailsAtEndOfInput,
                Case::NonJsonPayloadFails,
            ],
        ),
        (Fault::BufferedPayloadNotCompacted, &[Case::BufferedPathConcatenatesTheReencoding]),
        (Fault::BufferedKeepsPartialResult, &[Case::BufferedPathFailsOnAnyFramingError]),
    ];
    for (fault, expected) in expectations {
        let failure = run(*fault).expect_err(&format!("{fault:?} must fail the suite"));
        assert_eq!(failure.failed_case_ids(), *expected, "{fault:?}");
        assert_eq!(failure.evaluated_case_count(), eventstream_framing_fixtures_v1().len());
    }
}
