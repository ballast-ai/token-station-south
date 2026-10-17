//! The host harness and the runner of the pass-through delivery suite.

use std::fmt;

use serde_json::Value;
use south_host_grammars::decode_sse_v1;

use super::{
    NORTH_PASSTHROUGH_DELIVERY_SUITE_ID, NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_ID,
    NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_VERSION, NORTH_PASSTHROUGH_DELIVERY_SUITE_VERSION,
    NorthPassthroughBodyV1, NorthPassthroughBufferedVerdictV1, NorthPassthroughCaseIdV1,
    NorthPassthroughDeliveryV1, NorthPassthroughFixtureV1, NorthPassthroughFrameV1,
    NorthPassthroughPathV1, NorthPassthroughProtocolV2, NorthPassthroughScriptV1,
    NorthPassthroughSetupV1, NorthPassthroughSplitV1, NorthPassthroughStreamOutcomeV1,
    north_passthrough_buffered_upstream_text_v1, north_passthrough_fixtures_v1,
    north_passthrough_fixtures_v2, north_passthrough_frames_v1,
};

/// What a host implements to run the suite: its pass-through executor, with the component
/// replaced by a double that answers each call as the suite's script says.
///
/// Every method goes through the code the host runs in production. Each `stream` or `buffered`
/// call is one independent exchange in which the host has already chosen pass-through delivery for
/// a Responses client of a family declaring `north_passthrough: "responses"`.
pub trait NorthPassthroughHarnessV1 {
    /// The fixed public message of the host's failure event. The suite requires it in every failure
    /// event and checks that no upstream text appears instead.
    fn fixed_failure_message(&self) -> String;

    /// The delivery the host chooses for `setup`, before dispatch.
    fn delivery(&self, setup: &NorthPassthroughSetupV1) -> NorthPassthroughDeliveryV1;

    /// Streams one upstream body that arrives as `chunks`. The double answers the host's `n`-th
    /// `parse-stream-chunk` call with `script.frame(n)` and `finish` with `script.finish()`;
    /// persisting the settlement succeeds or fails as `script.settlement()` says. Reports every
    /// step the host took, in order.
    fn stream(
        &self,
        chunks: &[&[u8]],
        script: &NorthPassthroughScriptV1,
    ) -> NorthPassthroughTimelineV1;

    /// Takes one non-streaming upstream answer. The double's `parse-response` (or
    /// `map-provider-error`, for a non-2xx `status`) answers as `verdict` says, and persisting the
    /// settlement succeeds. Reports every step the host took, in order.
    fn buffered(
        &self,
        status: u16,
        body: &[u8],
        verdict: NorthPassthroughBufferedVerdictV1,
    ) -> NorthPassthroughTimelineV1;
}

/// What a host implements to run suite version two, one protocol at a time.
///
/// It has the [`NorthPassthroughHarnessV1`] methods, with `stream` and `buffered` told the
/// protocol of the exchange ([`super::NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_ID`]).
///
/// Each `stream` or `buffered` call is one independent exchange in which the host has already
/// chosen pass-through delivery for a client of `protocol`, to a family declaring
/// `north_passthrough: <protocol>` (and, for `generate_content` and `messages`,
/// `north_request_passthrough` too).
pub trait NorthPassthroughHarnessV2 {
    /// The fixed public message of the host's failure event: Responses' `response.failed` and the
    /// `messages` `error` event. `generate_content` has no failure event.
    fn fixed_failure_message(&self) -> String;

    /// The delivery the host chooses for `setup`, before dispatch.
    fn delivery(&self, setup: &NorthPassthroughSetupV1) -> NorthPassthroughDeliveryV1;

    /// As [`NorthPassthroughHarnessV1::stream`], for an exchange of `protocol`.
    fn stream(
        &self,
        protocol: NorthPassthroughProtocolV2,
        chunks: &[&[u8]],
        script: &NorthPassthroughScriptV1,
    ) -> NorthPassthroughTimelineV1;

    /// As [`NorthPassthroughHarnessV1::buffered`], for an exchange of `protocol`. When the host
    /// delivers a non-2xx `generate_content` answer it reports the status it wrote with
    /// [`NorthPassthroughStepV1::StatusWritten`].
    fn buffered(
        &self,
        protocol: NorthPassthroughProtocolV2,
        status: u16,
        body: &[u8],
        verdict: NorthPassthroughBufferedVerdictV1,
    ) -> NorthPassthroughTimelineV1;
}

/// A version-one harness seen as a version-two harness for Responses only.
struct ResponsesOnly<'a>(&'a dyn NorthPassthroughHarnessV1);

impl NorthPassthroughHarnessV2 for ResponsesOnly<'_> {
    fn fixed_failure_message(&self) -> String {
        self.0.fixed_failure_message()
    }

    fn delivery(&self, setup: &NorthPassthroughSetupV1) -> NorthPassthroughDeliveryV1 {
        self.0.delivery(setup)
    }

    fn stream(
        &self,
        _protocol: NorthPassthroughProtocolV2,
        chunks: &[&[u8]],
        script: &NorthPassthroughScriptV1,
    ) -> NorthPassthroughTimelineV1 {
        self.0.stream(chunks, script)
    }

    fn buffered(
        &self,
        _protocol: NorthPassthroughProtocolV2,
        status: u16,
        body: &[u8],
        verdict: NorthPassthroughBufferedVerdictV1,
    ) -> NorthPassthroughTimelineV1 {
        self.0.buffered(status, body, verdict)
    }
}

/// One step a host took, in the order it took them.
#[derive(Clone, PartialEq, Eq)]
pub enum NorthPassthroughStepV1 {
    /// A `parse-stream-chunk` call with these bytes, or the `parse-response` call (empty bytes).
    Fed(Vec<u8>),
    /// That call returned, whatever it answered.
    Returned,
    /// `finish` was called at end of input.
    Finished,
    /// These bytes were written to the client.
    Forwarded(Vec<u8>),
    /// The settlement of a successful exchange was persisted.
    SettlementPersisted,
    /// The exchange was parked for review (`delivery_unknown`).
    MarkedDeliveryUnknown,
    /// The exchange was settled as a failure.
    SettledAsFailure,
    /// The client response ended.
    Ended,
    /// The client response's status line was written with this status. Suite version two reads
    /// it only where the upstream's own status must reach the client (a non-2xx
    /// `generate_content` answer); elsewhere a host may report it or not.
    StatusWritten(u16),
}

impl fmt::Debug for NorthPassthroughStepV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fed(bytes) => {
                formatter.debug_struct("Fed").field("byte_count", &bytes.len()).finish()
            }
            Self::Returned => formatter.write_str("Returned"),
            Self::Finished => formatter.write_str("Finished"),
            Self::Forwarded(bytes) => {
                formatter.debug_struct("Forwarded").field("byte_count", &bytes.len()).finish()
            }
            Self::SettlementPersisted => formatter.write_str("SettlementPersisted"),
            Self::MarkedDeliveryUnknown => formatter.write_str("MarkedDeliveryUnknown"),
            Self::SettledAsFailure => formatter.write_str("SettledAsFailure"),
            Self::Ended => formatter.write_str("Ended"),
            Self::StatusWritten(status) => {
                formatter.debug_tuple("StatusWritten").field(status).finish()
            }
        }
    }
}

/// The ordered steps of one exchange.
#[derive(Clone, PartialEq, Eq)]
pub struct NorthPassthroughTimelineV1 {
    steps: Vec<NorthPassthroughStepV1>,
}

impl NorthPassthroughTimelineV1 {
    /// A timeline of `steps`, in the order the host took them.
    #[must_use]
    pub const fn new(steps: Vec<NorthPassthroughStepV1>) -> Self {
        Self { steps }
    }

    /// The steps, in order.
    #[must_use]
    pub fn steps(&self) -> &[NorthPassthroughStepV1] {
        &self.steps
    }
}

impl fmt::Debug for NorthPassthroughTimelineV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("NorthPassthroughTimelineV1").field("steps", &self.steps).finish()
    }
}

/// What a mismatch is about.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NorthPassthroughMismatchCategoryV1 {
    /// The delivery decision.
    Delivery,
    /// What the host fed the component: one whole frame per call, and no call after the end.
    Fed,
    /// What reached the client: the upstream's bytes unchanged, repaired only by NP-Q10, with only
    /// comment lines added between frames.
    Forwarded,
    /// A frame's bytes forwarded before its call returned, or a body before its parse.
    Order,
    /// The terminal frame or body released without, or before, a persisted settlement.
    Terminal,
    /// The funds transition of a failed exchange: missing, repeated, or after the failure event.
    Funds,
    /// The host's failure event: its shape, identity, sequence number or message (Responses), its
    /// shape or message (`messages`), or any frame at all written after the relayed ones
    /// (`generate_content`).
    FailureEvent,
    /// The upstream's own failure text reached the client.
    Leak,
    /// The client response did not end exactly once, last.
    Ended,
}

fixed_debug!(NorthPassthroughMismatchCategoryV1 {
    Delivery => "Delivery",
    Fed => "Fed",
    Forwarded => "Forwarded",
    Order => "Order",
    Terminal => "Terminal",
    Funds => "Funds",
    FailureEvent => "FailureEvent",
    Leak => "Leak",
    Ended => "Ended",
});

/// One failed check.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NorthPassthroughMismatchV1 {
    case_id: NorthPassthroughCaseIdV1,
    variant: usize,
    category: NorthPassthroughMismatchCategoryV1,
}

impl NorthPassthroughMismatchV1 {
    /// The case.
    #[must_use]
    pub const fn case_id(&self) -> NorthPassthroughCaseIdV1 {
        self.case_id
    }

    /// Which run of the case: a setup, a body, or a split, in order.
    #[must_use]
    pub const fn variant(&self) -> usize {
        self.variant
    }

    /// What the mismatch is about.
    #[must_use]
    pub const fn category(&self) -> NorthPassthroughMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for NorthPassthroughMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NorthPassthroughMismatchV1")
            .field("case_id", &self.case_id)
            .field("variant", &self.variant)
            .field("category", &self.category)
            .finish()
    }
}

/// A passing run.
#[derive(Clone, PartialEq, Eq)]
pub struct NorthPassthroughConformanceReportV1 {
    suite: Suite,
    passed_case_ids: Vec<NorthPassthroughCaseIdV1>,
}

impl NorthPassthroughConformanceReportV1 {
    /// The suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        self.suite.id
    }

    /// The suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        self.suite.version
    }

    /// The protocol the cases were for (always Responses for version one).
    #[must_use]
    pub const fn protocol(&self) -> NorthPassthroughProtocolV2 {
        self.suite.protocol
    }

    /// Every case, in table order, once.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[NorthPassthroughCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for NorthPassthroughConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NorthPassthroughConformanceReportV1")
            .field("suite_id", &self.suite_id())
            .field("protocol", &self.suite.protocol)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// A failing run: every mismatch, without failing fast.
#[derive(Clone, PartialEq, Eq)]
pub struct NorthPassthroughConformanceFailureV1 {
    suite: Suite,
    evaluated_case_count: usize,
    mismatches: Vec<NorthPassthroughMismatchV1>,
}

impl NorthPassthroughConformanceFailureV1 {
    /// The suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        self.suite.id
    }

    /// The suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        self.suite.version
    }

    /// The protocol the cases were for (always Responses for version one).
    #[must_use]
    pub const fn protocol(&self) -> NorthPassthroughProtocolV2 {
        self.suite.protocol
    }

    /// How many distinct cases ran.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Every mismatch, in table order.
    #[must_use]
    pub fn mismatches(&self) -> &[NorthPassthroughMismatchV1] {
        &self.mismatches
    }

    /// The distinct failing cases, in table order.
    #[must_use]
    pub fn failed_case_ids(&self) -> Vec<NorthPassthroughCaseIdV1> {
        let mut ids: Vec<_> = self.mismatches.iter().map(|mismatch| mismatch.case_id).collect();
        ids.dedup();
        ids
    }
}

impl fmt::Debug for NorthPassthroughConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NorthPassthroughConformanceFailureV1")
            .field("suite_id", &self.suite_id())
            .field("protocol", &self.suite.protocol)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs every case in table order without failing fast.
///
/// The harness is synchronous and the suite injects no time. The byte-boundary case calls
/// [`NorthPassthroughHarnessV1::stream`] once per interior split point of its body and once more
/// with one byte per chunk.
pub fn run_north_passthrough_conformance_v1(
    harness: &dyn NorthPassthroughHarnessV1,
) -> Result<NorthPassthroughConformanceReportV1, NorthPassthroughConformanceFailureV1> {
    let suite = Suite {
        id: NORTH_PASSTHROUGH_DELIVERY_SUITE_ID,
        version: NORTH_PASSTHROUGH_DELIVERY_SUITE_VERSION,
        protocol: NorthPassthroughProtocolV2::Responses,
    };
    run(&ResponsesOnly(harness), suite, &north_passthrough_fixtures_v1())
}

/// Runs every case of suite version two for `protocol`, in table order, without failing fast.
///
/// The cases are those [`north_passthrough_fixtures_v2`] gives for `protocol`; for Responses they are the
/// version-one cases, checked the same way. A host that serves several protocols by pass-through
/// runs this once per protocol.
pub fn run_north_passthrough_conformance_v2(
    harness: &dyn NorthPassthroughHarnessV2,
    protocol: NorthPassthroughProtocolV2,
) -> Result<NorthPassthroughConformanceReportV1, NorthPassthroughConformanceFailureV1> {
    let suite = Suite {
        id: NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_ID,
        version: NORTH_PASSTHROUGH_DELIVERY_SUITE_V2_VERSION,
        protocol,
    };
    run(harness, suite, &north_passthrough_fixtures_v2(protocol))
}

/// Which suite a run belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Suite {
    id: &'static str,
    version: u32,
    protocol: NorthPassthroughProtocolV2,
}

fn run(
    harness: &dyn NorthPassthroughHarnessV2,
    suite: Suite,
    fixtures: &[NorthPassthroughFixtureV1],
) -> Result<NorthPassthroughConformanceReportV1, NorthPassthroughConformanceFailureV1> {
    let protocol = suite.protocol;
    let mut case_ids: Vec<NorthPassthroughCaseIdV1> =
        fixtures.iter().map(NorthPassthroughFixtureV1::case_id).collect();
    case_ids.dedup();
    let mut mismatches = Vec::new();
    let fixed = harness.fixed_failure_message();
    for fixture in fixtures {
        let mut checker =
            Checker { case_id: fixture.case_id(), variant: 0, mismatches: &mut mismatches };
        match fixture.path() {
            NorthPassthroughPathV1::Delivery { setups, passes_through } => {
                for setup in setups {
                    let delivery = harness.delivery(setup);
                    let passed = delivery == NorthPassthroughDeliveryV1::PassThrough;
                    checker.check(
                        passed != *passes_through,
                        NorthPassthroughMismatchCategoryV1::Delivery,
                    );
                    checker.variant += 1;
                }
            }
            NorthPassthroughPathV1::Stream { body, split, script, outcome } => {
                let bytes = body.bytes();
                for chunks in variants(*split, &bytes) {
                    let timeline = harness.stream(protocol, &chunks, script);
                    let expected = StreamExpectation { protocol, body: *body, outcome: *outcome };
                    check_stream(&mut checker, &expected, &bytes, &timeline, &fixed);
                    checker.variant += 1;
                }
            }
            NorthPassthroughPathV1::Buffered { status, body, verdict, forwarded } => {
                let timeline = harness.buffered(protocol, *status, body.as_bytes(), *verdict);
                check_buffered(&mut checker, *status, body, *forwarded, &timeline);
            }
        }
    }
    if mismatches.is_empty() {
        Ok(NorthPassthroughConformanceReportV1 { suite, passed_case_ids: case_ids })
    } else {
        Err(NorthPassthroughConformanceFailureV1 {
            suite,
            evaluated_case_count: case_ids.len(),
            mismatches,
        })
    }
}

struct Checker<'a> {
    case_id: NorthPassthroughCaseIdV1,
    variant: usize,
    mismatches: &'a mut Vec<NorthPassthroughMismatchV1>,
}

impl Checker<'_> {
    fn check(&mut self, failed: bool, category: NorthPassthroughMismatchCategoryV1) {
        let mismatch =
            NorthPassthroughMismatchV1 { case_id: self.case_id, variant: self.variant, category };
        if failed && !self.mismatches.contains(&mismatch) {
            self.mismatches.push(mismatch);
        }
    }
}

/// The chunkings of `bytes` a case feeds, in variant order.
fn variants(split: NorthPassthroughSplitV1, bytes: &[u8]) -> Vec<Vec<&[u8]>> {
    match split {
        NorthPassthroughSplitV1::Whole => vec![vec![bytes]],
        NorthPassthroughSplitV1::EveryByteBoundary => {
            let mut variants: Vec<Vec<&[u8]>> = (1..bytes.len())
                .map(|point| {
                    let (head, tail) = bytes.split_at(point);
                    vec![head, tail]
                })
                .collect();
            variants.push(bytes.chunks(1).collect());
            variants
        }
    }
}

/// The forwarded bytes, concatenated, with the index of the step that wrote each byte.
fn forwarded(steps: &[NorthPassthroughStepV1]) -> (Vec<u8>, Vec<usize>) {
    let mut bytes = Vec::new();
    let mut owners = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        if let NorthPassthroughStepV1::Forwarded(chunk) = step {
            bytes.extend_from_slice(chunk);
            owners.extend(std::iter::repeat_n(index, chunk.len()));
        }
    }
    (bytes, owners)
}

/// Skips one comment line the host added between frames (NP-Q9), with the blank line that may
/// follow it. Returns `None` when `raw[pos..]` does not start with a comment.
fn skip_comment(raw: &[u8], pos: usize) -> Option<usize> {
    let rest = raw.get(pos..)?;
    if rest.first() != Some(&b':') {
        return None;
    }
    let mut at = pos + line_length(rest)?;
    if let Some(blank) = raw.get(at..).and_then(line_terminator) {
        at += blank;
    }
    Some(at)
}

/// The length of the first line of `bytes`, its terminator included.
fn line_length(bytes: &[u8]) -> Option<usize> {
    let end = bytes.iter().position(|byte| *byte == b'\n' || *byte == b'\r')?;
    Some(end + line_terminator(&bytes[end..]).unwrap_or(1))
}

/// The length of the line terminator `bytes` starts with.
fn line_terminator(bytes: &[u8]) -> Option<usize> {
    match bytes {
        [b'\r', b'\n', ..] => Some(2),
        [b'\r' | b'\n', ..] => Some(1),
        _ => None,
    }
}

fn count(steps: &[NorthPassthroughStepV1], wanted: &NorthPassthroughStepV1) -> usize {
    steps.iter().filter(|step| *step == wanted).count()
}

fn position(steps: &[NorthPassthroughStepV1], wanted: &NorthPassthroughStepV1) -> Option<usize> {
    steps.iter().position(|step| step == wanted)
}

fn funds_steps(steps: &[NorthPassthroughStepV1]) -> Vec<usize> {
    steps
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            matches!(
                step,
                NorthPassthroughStepV1::MarkedDeliveryUnknown
                    | NorthPassthroughStepV1::SettledAsFailure
            )
        })
        .map(|(index, _)| index)
        .collect()
}

fn ended_once_last(steps: &[NorthPassthroughStepV1]) -> bool {
    count(steps, &NorthPassthroughStepV1::Ended) == 1
        && steps.last() == Some(&NorthPassthroughStepV1::Ended)
}

/// What one streaming case expects.
struct StreamExpectation {
    protocol: NorthPassthroughProtocolV2,
    body: NorthPassthroughBodyV1,
    outcome: NorthPassthroughStreamOutcomeV1,
}

fn check_stream(
    checker: &mut Checker<'_>,
    expected: &StreamExpectation,
    bytes: &[u8],
    timeline: &NorthPassthroughTimelineV1,
    fixed: &str,
) {
    use NorthPassthroughMismatchCategoryV1 as Category;
    let StreamExpectation { protocol, body, outcome } = *expected;
    let steps = timeline.steps();
    let frames = north_passthrough_frames_v1(bytes).unwrap_or_default();
    let (relayed, fed_count, completes) = match outcome {
        NorthPassthroughStreamOutcomeV1::Completes => (frames.len(), frames.len(), true),
        NorthPassthroughStreamOutcomeV1::FailsAfter { relayed, fed } => (relayed, fed, false),
    };

    // Fed: one whole frame per call, in order, and no call after the exchange ended.
    let fed: Vec<&[u8]> = steps
        .iter()
        .filter_map(|step| match step {
            NorthPassthroughStepV1::Fed(bytes) => Some(bytes.as_slice()),
            _ => None,
        })
        .collect();
    let fed_right =
        fed.len() == fed_count && fed.iter().zip(&frames).all(|(fed, frame)| *fed == frame.bytes());
    checker.check(!fed_right, Category::Fed);

    // Forwarded: the relayed frames unchanged, host comments only between them.
    let (raw, owners) = forwarded(steps);
    let mut segments: Vec<Vec<u8>> =
        frames.iter().take(relayed).map(|frame| frame.bytes().to_vec()).collect();
    if completes && let (Some(last), Some(frame)) = (segments.last_mut(), frames.last()) {
        *last = frame.forwarded_bytes();
    }
    let mut pos = 0usize;
    let mut starts = Vec::with_capacity(segments.len());
    let mut forwarded_right = true;
    for segment in &segments {
        while !raw.get(pos..).is_some_and(|rest| rest.starts_with(segment)) {
            match skip_comment(&raw, pos) {
                Some(next) => pos = next,
                None => break,
            }
        }
        if raw.get(pos..).is_some_and(|rest| rest.starts_with(segment)) {
            starts.push(pos);
            pos += segment.len();
        } else {
            forwarded_right = false;
            break;
        }
    }
    while let Some(next) = skip_comment(&raw, pos) {
        pos = next;
    }
    let tail = raw.get(pos..).unwrap_or_default();
    if completes {
        forwarded_right &= tail.is_empty();
    }
    checker.check(!forwarded_right, Category::Forwarded);

    // Order: no byte of frame `i` before its call returned.
    let mut ordered = true;
    for (index, start) in starts.iter().enumerate() {
        let step = owners.get(*start).copied().unwrap_or(0);
        let returned = steps
            .get(..step)
            .unwrap_or_default()
            .iter()
            .filter(|step| **step == NorthPassthroughStepV1::Returned)
            .count();
        ordered &= returned > index;
    }
    checker.check(!ordered, Category::Order);

    // Terminal: released once, after the settlement; never settled on a failure.
    let settled = position(steps, &NorthPassthroughStepV1::SettlementPersisted);
    let settlements = count(steps, &NorthPassthroughStepV1::SettlementPersisted);
    let terminal_right = if completes {
        let terminal_start = starts.last().and_then(|start| owners.get(*start)).copied();
        settlements == 1
            && starts.len() == segments.len()
            && matches!((settled, terminal_start), (Some(settled), Some(start)) if settled < start)
    } else {
        settlements == 0
    };
    checker.check(!terminal_right, Category::Terminal);

    // Funds: a failure is recorded once, before the client sees the failure event.
    let funds = funds_steps(steps);
    let funds_right = if completes {
        funds.is_empty()
    } else {
        let tail_step = owners.get(pos).copied();
        funds.len() == 1 && tail_step.is_none_or(|step| funds[0] < step)
    };
    checker.check(!funds_right, Category::Funds);

    // The failure event: the host's own `response.failed` for Responses; for `generate_content`,
    // which has no failure event, nothing at all.
    if !completes {
        let right = match protocol {
            NorthPassthroughProtocolV2::Responses => {
                let identity = frames.iter().take(relayed).find_map(relayed_identity);
                let last_sequence = frames
                    .get(..relayed)
                    .and_then(|relayed| relayed.last())
                    .and_then(NorthPassthroughFrameV1::sequence_number);
                failure_event_right(tail, relayed, identity.as_ref(), last_sequence, fixed)
            }
            NorthPassthroughProtocolV2::GenerateContent => tail.is_empty(),
            NorthPassthroughProtocolV2::Messages => messages_failure_event_right(tail, fixed),
        };
        checker.check(!right, Category::FailureEvent);
    }

    // Leak.
    let leaked = body.upstream_failure_text().is_some_and(|text| contains(&raw, text.as_bytes()));
    checker.check(leaked, Category::Leak);

    checker.check(!ended_once_last(steps), Category::Ended);
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

/// `response.id`, `response.model` and `response.created_at` of a relayed lifecycle frame.
fn relayed_identity(frame: &NorthPassthroughFrameV1) -> Option<(Value, Value, Value)> {
    let data = frame_data(frame.bytes())?;
    let response = data.get("response")?;
    Some((
        response.get("id")?.clone(),
        response.get("model")?.clone(),
        response.get("created_at")?.clone(),
    ))
}

fn frame_data(bytes: &[u8]) -> Option<Value> {
    let events = decode_sse_v1(bytes).ok()?;
    serde_json::from_str(events.first()?.data()).ok()
}

fn failure_event_right(
    tail: &[u8],
    relayed: usize,
    identity: Option<&(Value, Value, Value)>,
    last_sequence: Option<u64>,
    fixed: &str,
) -> bool {
    let Ok(events) = decode_sse_v1(tail) else { return false };
    let data: Vec<Value> =
        events.iter().filter_map(|event| serde_json::from_str(event.data()).ok()).collect();
    if data.len() != events.len() {
        return false;
    }
    let failed_right = |event: &Value, sequence: Option<u64>| {
        let response = &event["response"];
        event["type"] == "response.failed"
            && sequence.is_some_and(|sequence| event["sequence_number"] == sequence)
            && response["status"] == "failed"
            && response["error"]["message"] == fixed
    };
    if relayed == 0 {
        // No relayed identity: the host renders its own created and failed events.
        return events.len() == 2
            && events[0].event() == "response.created"
            && events[1].event() == "response.failed"
            && data[0]["type"] == "response.created"
            && data[0]["response"]["id"].is_string()
            && data[1]["response"]["id"] == data[0]["response"]["id"]
            && failed_right(&data[1], data[0]["sequence_number"].as_u64().map(|value| value + 1));
    }
    let Some((id, model, created_at)) = identity else { return false };
    events.len() == 1
        && events[0].event() == "response.failed"
        && failed_right(&data[0], last_sequence.map(|value| value + 1))
        && data[0]["response"]["id"] == *id
        && data[0]["response"]["model"] == *model
        && data[0]["response"]["created_at"] == *created_at
}

/// The host's own Messages failure event: exactly one `error` event whose data is
/// `{"type":"error","error":{"type":<string>,"message":<fixed>}}`.
fn messages_failure_event_right(tail: &[u8], fixed: &str) -> bool {
    let Ok(events) = decode_sse_v1(tail) else { return false };
    let [event] = events.as_slice() else { return false };
    let Ok(data) = serde_json::from_str::<Value>(event.data()) else { return false };
    event.event() == "error"
        && data["type"] == "error"
        && data["error"]["type"].is_string()
        && data["error"]["message"] == fixed
}

fn check_buffered(
    checker: &mut Checker<'_>,
    status: u16,
    body: &str,
    forwarded_expected: bool,
    timeline: &NorthPassthroughTimelineV1,
) {
    use NorthPassthroughMismatchCategoryV1 as Category;
    let steps = timeline.steps();
    let (raw, owners) = forwarded(steps);
    let first_forward = owners.first().copied();
    let returned = position(steps, &NorthPassthroughStepV1::Returned);
    let settled = position(steps, &NorthPassthroughStepV1::SettlementPersisted);
    if forwarded_expected && !(200..300).contains(&status) {
        // Delivered as sent (`generate_content`): the upstream's status and body, after
        // `map-provider-error` returned and the funds transition was recorded; never settled as a
        // success.
        let statuses: Vec<(usize, u16)> = steps
            .iter()
            .enumerate()
            .filter_map(|(index, step)| match step {
                NorthPassthroughStepV1::StatusWritten(written) => Some((index, *written)),
                _ => None,
            })
            .collect();
        let status_right = matches!(
            (statuses.as_slice(), first_forward),
            ([(index, written)], Some(forward)) if *written == status && *index < forward
        );
        checker.check(raw != body.as_bytes() || !status_right, Category::Forwarded);
        checker.check(
            !matches!((returned, first_forward), (Some(r), Some(f)) if r < f),
            Category::Order,
        );
        checker.check(settled.is_some(), Category::Terminal);
        let funds = funds_steps(steps);
        checker.check(
            funds.len() != 1 || first_forward.is_some_and(|forward| funds[0] > forward),
            Category::Funds,
        );
    } else if forwarded_expected {
        checker.check(raw != body.as_bytes(), Category::Forwarded);
        checker.check(
            !matches!((returned, first_forward), (Some(r), Some(f)) if r < f),
            Category::Order,
        );
        checker.check(
            count(steps, &NorthPassthroughStepV1::SettlementPersisted) != 1
                || !matches!((settled, first_forward), (Some(s), Some(f)) if s < f),
            Category::Terminal,
        );
        checker.check(!funds_steps(steps).is_empty(), Category::Funds);
    } else {
        checker.check(raw.is_empty(), Category::Forwarded);
        let leaked = north_passthrough_buffered_upstream_text_v1(body)
            .is_some_and(|text| contains(&raw, text.as_bytes()));
        checker.check(leaked || raw == body.as_bytes(), Category::Leak);
        checker.check(settled.is_some(), Category::Terminal);
        let funds = funds_steps(steps);
        checker.check(
            funds.len() != 1 || first_forward.is_some_and(|forward| funds[0] > forward),
            Category::Funds,
        );
    }
    checker.check(!ended_once_last(steps), Category::Ended);
}
