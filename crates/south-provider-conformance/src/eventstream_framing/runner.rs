//! The host harness and the runner of the eventstream-framing suite.

use std::fmt;

use south_provider_api::StreamFramingV1;

use super::{
    EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_ID, EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_VERSION,
    EventStreamFramingCaseIdV1, EventStreamFramingFixtureV1, EventStreamFramingPathV1,
    EventStreamSplitV1, eventstream_framing_fixtures_v1,
};

/// What a host implements to run the suite: the code between its transport and the component,
/// with the component replaced by a recorder.
///
/// Both methods go through the same code the host runs in production. Each call is one
/// independent upstream response.
pub trait EventStreamFramingHarnessV1 {
    /// Streams one upstream body that arrives as `chunks`, for a package declaring `framing`, and
    /// reports every chunk the host fed `parse-stream-chunk`, in order, and how the stream ended.
    fn stream(&self, framing: StreamFramingV1, chunks: &[&[u8]]) -> EventStreamFeedObservationV1;

    /// Takes the buffered path for one complete upstream body of an `aws-eventstream` package
    /// whose family declares `request_facts.stream: "none"`, and reports the text the host handed
    /// `parse-response`, or that it failed without calling it.
    fn buffered(&self, body: &[u8]) -> EventStreamBufferedObservationV1;
}

/// How a host's stream ended.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EventStreamOutcomeV1 {
    /// The upstream body ended and everything in it was delivered.
    Completed,
    /// The host ended the stream as failed: a framing error, a truncated tail at end of input, or
    /// a payload the re-encoding refused.
    Failed,
}

fixed_debug!(EventStreamOutcomeV1 {
    Completed => "Completed",
    Failed => "Failed",
});

/// What the host fed the component while streaming one body.
#[derive(Clone, PartialEq, Eq)]
pub struct EventStreamFeedObservationV1 {
    fed: Vec<Vec<u8>>,
    outcome: EventStreamOutcomeV1,
}

impl EventStreamFeedObservationV1 {
    /// `fed` holds every chunk handed to `parse-stream-chunk`, in order; the suite compares their
    /// concatenation, so how the host chunks its feed is not judged.
    #[must_use]
    pub const fn new(fed: Vec<Vec<u8>>, outcome: EventStreamOutcomeV1) -> Self {
        Self { fed, outcome }
    }

    /// The chunks fed, in order.
    #[must_use]
    pub fn fed(&self) -> &[Vec<u8>] {
        &self.fed
    }

    /// How the stream ended.
    #[must_use]
    pub const fn outcome(&self) -> EventStreamOutcomeV1 {
        self.outcome
    }
}

impl fmt::Debug for EventStreamFeedObservationV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EventStreamFeedObservationV1")
            .field("fed_chunk_count", &self.fed.len())
            .field("fed_byte_count", &self.fed.iter().map(Vec::len).sum::<usize>())
            .field("outcome", &self.outcome)
            .finish()
    }
}

/// What the host's buffered path did with one body.
#[derive(Clone, PartialEq, Eq)]
pub enum EventStreamBufferedObservationV1 {
    /// The host handed `parse-response` this text.
    Parsed(String),
    /// The host failed the response without calling `parse-response`.
    Failed,
}

impl fmt::Debug for EventStreamBufferedObservationV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parsed(text) => {
                formatter.debug_struct("Parsed").field("byte_count", &text.len()).finish()
            }
            Self::Failed => formatter.write_str("Failed"),
        }
    }
}

/// The closed reasons why a case can fail.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventStreamFramingMismatchCategoryV1 {
    /// The bytes delivered to the component differ from the expected ones.
    Delivered,
    /// The stream or the buffered path ended otherwise than expected (completed or failed).
    Outcome,
}

fixed_debug!(EventStreamFramingMismatchCategoryV1 {
    Delivered => "Delivered",
    Outcome => "Outcome",
});

/// One mismatch, without expected or observed bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventStreamFramingMismatchV1 {
    case_id: EventStreamFramingCaseIdV1,
    variant: usize,
    category: EventStreamFramingMismatchCategoryV1,
}

impl EventStreamFramingMismatchV1 {
    /// The case that mismatched.
    #[must_use]
    pub const fn case_id(&self) -> EventStreamFramingCaseIdV1 {
        self.case_id
    }

    /// Which input of the case mismatched first, counting bodies in order and, within a body
    /// split [`EventStreamSplitV1::EveryByteBoundary`], the two-chunk splits by split point and
    /// then the one-byte-per-chunk split. The runner stops a case at its first mismatching input.
    #[must_use]
    pub const fn variant(&self) -> usize {
        self.variant
    }

    /// The closed mismatch category.
    #[must_use]
    pub const fn category(&self) -> EventStreamFramingMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for EventStreamFramingMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EventStreamFramingMismatchV1")
            .field("case_id", &self.case_id)
            .field("variant", &self.variant)
            .field("category", &self.category)
            .finish()
    }
}

/// A successful report for the complete suite.
pub struct EventStreamFramingConformanceReportV1 {
    passed_case_ids: Vec<EventStreamFramingCaseIdV1>,
}

impl EventStreamFramingConformanceReportV1 {
    /// The stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_ID
    }

    /// The suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_VERSION
    }

    /// Every passed case, in table order.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[EventStreamFramingCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for EventStreamFramingConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EventStreamFramingConformanceReportV1")
            .field("suite_id", &EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_ID)
            .field("suite_version", &EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_VERSION)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// Every mismatch of an evaluated suite.
pub struct EventStreamFramingConformanceFailureV1 {
    evaluated_case_count: usize,
    mismatches: Vec<EventStreamFramingMismatchV1>,
}

impl EventStreamFramingConformanceFailureV1 {
    /// The stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_ID
    }

    /// The suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_VERSION
    }

    /// How many cases were evaluated.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Every mismatch in evaluation order.
    #[must_use]
    pub fn mismatches(&self) -> &[EventStreamFramingMismatchV1] {
        &self.mismatches
    }

    /// The cases with at least one mismatch, in table order, without repeats.
    #[must_use]
    pub fn failed_case_ids(&self) -> Vec<EventStreamFramingCaseIdV1> {
        let mut failed: Vec<_> = self.mismatches.iter().map(|mismatch| mismatch.case_id).collect();
        failed.dedup();
        failed
    }
}

impl fmt::Debug for EventStreamFramingConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EventStreamFramingConformanceFailureV1")
            .field("suite_id", &EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_ID)
            .field("suite_version", &EVENTSTREAM_FRAMING_CONFORMANCE_SUITE_VERSION)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs every case in table order without failing fast.
///
/// The harness is synchronous and the suite injects no time, so the run has no timing behavior of
/// its own. The byte-boundary cases call [`EventStreamFramingHarnessV1::stream`] once per interior
/// split point of their body, and once more with one byte per chunk.
pub fn run_eventstream_framing_conformance_v1(
    harness: &dyn EventStreamFramingHarnessV1,
) -> Result<EventStreamFramingConformanceReportV1, EventStreamFramingConformanceFailureV1> {
    let fixtures = eventstream_framing_fixtures_v1();
    let mut passed_case_ids = Vec::with_capacity(fixtures.len());
    let mut mismatches = Vec::new();
    for fixture in fixtures {
        let before = mismatches.len();
        run_case(harness, fixture, &mut mismatches);
        if mismatches.len() == before {
            passed_case_ids.push(fixture.case_id());
        }
    }
    if mismatches.is_empty() {
        Ok(EventStreamFramingConformanceReportV1 { passed_case_ids })
    } else {
        Err(EventStreamFramingConformanceFailureV1 {
            evaluated_case_count: fixtures.len(),
            mismatches,
        })
    }
}

fn run_case(
    harness: &dyn EventStreamFramingHarnessV1,
    fixture: &EventStreamFramingFixtureV1,
    mismatches: &mut Vec<EventStreamFramingMismatchV1>,
) {
    let expected = fixture.expected();
    let mut variant = 0;
    for body in fixture.bodies() {
        let bytes = body.bytes();
        let want = expected.delivered().bytes(&bytes);
        for chunks in variants(fixture.path(), &bytes) {
            let (delivered, completed) = match fixture.path() {
                EventStreamFramingPathV1::Streaming { framing, .. } => {
                    let observed = harness.stream(framing, &chunks);
                    (observed.fed.concat(), observed.outcome == EventStreamOutcomeV1::Completed)
                }
                EventStreamFramingPathV1::Buffered => match harness.buffered(&bytes) {
                    EventStreamBufferedObservationV1::Parsed(text) => (text.into_bytes(), true),
                    EventStreamBufferedObservationV1::Failed => (Vec::new(), false),
                },
            };
            let before = mismatches.len();
            let mut record = |differs: bool, category| {
                if differs {
                    mismatches.push(EventStreamFramingMismatchV1 {
                        case_id: fixture.case_id(),
                        variant,
                        category,
                    });
                }
            };
            record(delivered != want, EventStreamFramingMismatchCategoryV1::Delivered);
            record(
                completed != expected.completes(),
                EventStreamFramingMismatchCategoryV1::Outcome,
            );
            if mismatches.len() > before {
                return;
            }
            variant += 1;
        }
    }
}

/// The chunkings of `bytes` a case feeds, in variant order.
fn variants(path: EventStreamFramingPathV1, bytes: &[u8]) -> Vec<Vec<&[u8]>> {
    match path {
        EventStreamFramingPathV1::Streaming {
            split: EventStreamSplitV1::EveryByteBoundary,
            ..
        } => {
            let mut variants: Vec<Vec<&[u8]>> = (1..bytes.len())
                .map(|point| {
                    let (head, tail) = bytes.split_at(point);
                    vec![head, tail]
                })
                .collect();
            variants.push(bytes.chunks(1).collect());
            variants
        }
        EventStreamFramingPathV1::Streaming { split: EventStreamSplitV1::Whole, .. }
        | EventStreamFramingPathV1::Buffered => vec![vec![bytes]],
    }
}
