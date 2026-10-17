//! The reference pass-through executor for `south.north-passthrough-delivery.v1` and, for both
//! of its protocols, `south.north-passthrough-delivery.v2`.
//!
//! It is what the suite describes, written as plainly as possible: cut the upstream bytes with
//! `SseDecoderV1::position`, feed one frame per call, forward a frame after its call returned,
//! release the terminal frame only on a terminal type **and** `Done` and only after the settlement
//! is persisted, and on any failure record the funds transition, then end the stream with one
//! fixed-message `response.failed`. Design record
//! `docs/design/2026-10-11-responses-reasoning-replay-and-passthrough-suite.md` §3.
//!
//! For `generate_content` (record `docs/design/2026-10-14-generate-content-request-passthrough.md`
//! §6) the terminal frame is one with some `candidates[*].finishReason` set, a failure ends the
//! stream without any frame of the executor's own, and a non-2xx answer is delivered with the
//! upstream's status and body.
//!
//! For `messages` (record `docs/design/2026-10-17-messages-passthrough-and-count-tokens.md` §3.4,
//! corrected in §3.5) `Done` arrives on `message_delta`: the executor withholds that frame, any
//! `ping` after it and `message_stop`, and releases them together after the settlement; a failure
//! ends the stream with one fixed-message `error` event of the executor's own, and a non-2xx answer
//! is replaced by the executor's own error.
//!
//! [`NorthPassthroughFaultV1`] names one deliberate wiring mistake per assertion of the suite.
//! The suite's self-tests (`tests/north_passthrough_runner_v1.rs`, `..._v2.rs`) show the
//! reference passes and each mistake fails exactly the cases that guard it.

use serde_json::{Value, json};
use south_host_grammars::{DEFAULT_SSE_EVENT_TYPE, SseDecoderV1, decode_sse_v1};
use south_provider_api::NorthProtocolV1;
use south_provider_conformance::{
    NORTH_PASSTHROUGH_MESSAGES_DONE_TYPE, NORTH_PASSTHROUGH_MESSAGES_PING_TYPE,
    NORTH_PASSTHROUGH_MESSAGES_TERMINAL_TYPE, NorthPassthroughBufferedVerdictV1,
    NorthPassthroughDeliveryV1, NorthPassthroughFinishVerdictV1, NorthPassthroughFrameVerdictV1,
    NorthPassthroughHarnessV1, NorthPassthroughHarnessV2, NorthPassthroughProtocolV2,
    NorthPassthroughScriptV1, NorthPassthroughSettlementV1, NorthPassthroughSetupV1,
    NorthPassthroughStepV1, NorthPassthroughTimelineV1, north_passthrough_is_terminal_data_v2,
    north_passthrough_terminator_v1,
};

/// The reference executor's fixed public failure message.
pub const REFERENCE_NORTH_PASSTHROUGH_FAILURE_MESSAGE_V1: &str =
    "The request could not be completed. Try again later.";

/// One deliberate wiring mistake, for the suite's self-test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NorthPassthroughFaultV1 {
    /// Forwards a frame before its call returned.
    ForwardBeforeReturn,
    /// Feeds every frame a chunk completed in one call.
    CoalesceFrames,
    /// Releases the terminal frame before persisting the settlement.
    ReleaseTerminalBeforeSettlement,
    /// Treats `Done` alone as the terminal signal.
    TerminalOnDoneAlone,
    /// Treats a terminal northbound type alone as the terminal signal.
    TerminalOnTypeAlone,
    /// Forwards the upstream's failure frame before its own failure event.
    RelayFailureFrame,
    /// Sends the failure event before recording the funds transition.
    FailureEventBeforeFunds,
    /// Numbers the failure event two past the last relayed frame.
    WrongFailureSequence,
    /// Puts the upstream's own failure text in the failure event.
    LeakUpstreamMessage,
    /// Releases the terminal frame although persisting the settlement failed.
    ForwardTerminalWhenSettlementFails,
    /// Releases a terminal frame the end of input dispatched without the NP-Q10 terminator.
    NoTerminatorRepair,
    /// Writes a keep-alive comment in the middle of a frame.
    KeepAliveMidFrame,
    /// Settles a stream that ended without a terminal frame as a success.
    TruncationSettles,
    /// Passes through whatever the client's tools are.
    DeliveryIgnoresLossless,
    /// Passes through for a family without a declaration.
    DeliveryIgnoresDeclaration,
    /// Passes through whatever the inbound surface is.
    DeliveryIgnoresSurface,
    /// Forwards a non-streaming body before persisting the settlement.
    BufferedForwardBeforeSettlement,
    /// Forwards a non-streaming body the component refused, or a non-2xx body.
    BufferedRelaysFailure,
    /// `generate_content`: ends a failed stream with a frame of its own.
    GenerateContentFailureFrame,
    /// `generate_content`: answers a non-2xx upstream with its own error, as for Responses.
    GenerateContentNon2xxReplaced,
    /// `generate_content`: delivers a non-2xx body under a status of its own.
    GenerateContentNon2xxStatusRewritten,
    /// `generate_content`: looks for `finishReason` on the first candidate only.
    TerminalOnFirstCandidateOnly,
    /// `messages`: ends a failed stream without the `error` event of its own, as for
    /// `generate_content`.
    MessagesNoFailureEvent,
    /// `messages`: forwards the `message_delta` that answered `Done` at once, withholding only
    /// `message_stop` (record §3.5).
    MessagesForwardDoneFrame,
}

/// The reference executor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReferenceNorthPassthroughExecutorV1 {
    fault: Option<NorthPassthroughFaultV1>,
    keep_alive_at_boundaries: bool,
}

impl ReferenceNorthPassthroughExecutorV1 {
    /// The executor as the suite describes it.
    #[must_use]
    pub const fn new() -> Self {
        Self { fault: None, keep_alive_at_boundaries: false }
    }

    /// The executor with one deliberate mistake.
    #[must_use]
    pub const fn with_fault(fault: NorthPassthroughFaultV1) -> Self {
        Self { fault: Some(fault), keep_alive_at_boundaries: false }
    }

    /// The executor writing its own keep-alive comment before every frame it forwards, which NP-Q9
    /// allows between frames.
    #[must_use]
    pub const fn with_keep_alive_at_boundaries(self) -> Self {
        Self { fault: self.fault, keep_alive_at_boundaries: true }
    }

    fn is(self, fault: NorthPassthroughFaultV1) -> bool {
        self.fault == Some(fault)
    }
}

impl NorthPassthroughHarnessV1 for ReferenceNorthPassthroughExecutorV1 {
    fn fixed_failure_message(&self) -> String {
        NorthPassthroughHarnessV2::fixed_failure_message(self)
    }

    fn delivery(&self, setup: &NorthPassthroughSetupV1) -> NorthPassthroughDeliveryV1 {
        NorthPassthroughHarnessV2::delivery(self, setup)
    }

    fn stream(
        &self,
        chunks: &[&[u8]],
        script: &NorthPassthroughScriptV1,
    ) -> NorthPassthroughTimelineV1 {
        NorthPassthroughHarnessV2::stream(
            self,
            NorthPassthroughProtocolV2::Responses,
            chunks,
            script,
        )
    }

    fn buffered(
        &self,
        status: u16,
        body: &[u8],
        verdict: NorthPassthroughBufferedVerdictV1,
    ) -> NorthPassthroughTimelineV1 {
        NorthPassthroughHarnessV2::buffered(
            self,
            NorthPassthroughProtocolV2::Responses,
            status,
            body,
            verdict,
        )
    }
}

impl NorthPassthroughHarnessV2 for ReferenceNorthPassthroughExecutorV1 {
    fn fixed_failure_message(&self) -> String {
        REFERENCE_NORTH_PASSTHROUGH_FAILURE_MESSAGE_V1.to_owned()
    }

    fn delivery(&self, setup: &NorthPassthroughSetupV1) -> NorthPassthroughDeliveryV1 {
        use NorthPassthroughFaultV1 as Fault;
        // The client's tools matter only when the codec maps the request; `generate_content` and
        // `messages` pass the request through too.
        let request_passes_through =
            matches!(setup.inbound(), NorthProtocolV1::GenerateContent | NorthProtocolV1::Messages);
        let lossless = request_passes_through
            || self.is(Fault::DeliveryIgnoresLossless)
            || setup.client_tools().as_array().is_some_and(|tools| {
                tools
                    .iter()
                    .all(|tool| tool["type"] == "function" && tool.get("namespace").is_none())
            });
        let declared: Option<NorthProtocolV1> = if self.is(Fault::DeliveryIgnoresDeclaration) {
            setup.declared().or_else(|| Some(setup.inbound()))
        } else {
            setup.declared()
        };
        let surface_matches = if self.is(Fault::DeliveryIgnoresSurface) {
            declared.is_some()
        } else {
            declared == Some(setup.inbound())
        };
        if surface_matches && lossless {
            NorthPassthroughDeliveryV1::PassThrough
        } else {
            NorthPassthroughDeliveryV1::Rendered
        }
    }

    fn stream(
        &self,
        protocol: NorthPassthroughProtocolV2,
        chunks: &[&[u8]],
        script: &NorthPassthroughScriptV1,
    ) -> NorthPassthroughTimelineV1 {
        let mut run = Run::new(*self, protocol, script);
        let mut decoder = SseDecoderV1::new();
        let mut buffer = Vec::new();
        let mut start = 0usize;
        'chunks: for chunk in chunks {
            decoder.push(chunk);
            buffer.extend_from_slice(chunk);
            let mut batch = Vec::new();
            loop {
                match decoder.next_event() {
                    Ok(Some(_)) => {
                        let end = usize::try_from(decoder.position()).unwrap_or(buffer.len());
                        batch.push(buffer.get(start..end).unwrap_or_default().to_vec());
                        start = end;
                    }
                    Ok(None) => break,
                    Err(_) => {
                        run.fail(NorthPassthroughStepV1::MarkedDeliveryUnknown, None);
                        break 'chunks;
                    }
                }
            }
            if self.is(NorthPassthroughFaultV1::CoalesceFrames) && batch.len() > 1 {
                run.handle(batch.concat(), false);
            } else {
                for frame in batch {
                    run.handle(frame, false);
                    if run.done {
                        break;
                    }
                }
            }
            if run.done {
                break;
            }
        }
        if !run.done {
            match decoder.finish() {
                Ok(Some(_)) => run.handle(buffer.get(start..).unwrap_or_default().to_vec(), true),
                Ok(None) => {}
                Err(_) => run.fail(NorthPassthroughStepV1::MarkedDeliveryUnknown, None),
            }
        }
        if !run.done {
            run.finish();
        }
        NorthPassthroughTimelineV1::new(run.steps)
    }

    fn buffered(
        &self,
        protocol: NorthPassthroughProtocolV2,
        status: u16,
        body: &[u8],
        verdict: NorthPassthroughBufferedVerdictV1,
    ) -> NorthPassthroughTimelineV1 {
        use NorthPassthroughFaultV1 as Fault;
        use NorthPassthroughStepV1 as Step;
        let mut steps = vec![Step::Fed(Vec::new()), Step::Returned];
        let is_2xx = (200..300).contains(&status);
        let success = is_2xx && verdict == NorthPassthroughBufferedVerdictV1::Parses;
        let delivered_as_sent = protocol == NorthPassthroughProtocolV2::GenerateContent
            && !is_2xx
            && !self.is(Fault::GenerateContentNon2xxReplaced);
        if delivered_as_sent {
            steps.push(Step::SettledAsFailure);
            let written =
                if self.is(Fault::GenerateContentNon2xxStatusRewritten) { 502 } else { status };
            steps.push(Step::StatusWritten(written));
            steps.push(Step::Forwarded(body.to_vec()));
        } else if success {
            if self.is(NorthPassthroughFaultV1::BufferedForwardBeforeSettlement) {
                steps.push(Step::Forwarded(body.to_vec()));
                steps.push(Step::SettlementPersisted);
            } else {
                steps.push(Step::SettlementPersisted);
                steps.push(Step::Forwarded(body.to_vec()));
            }
        } else {
            steps.push(Step::SettledAsFailure);
            let answer = if self.is(NorthPassthroughFaultV1::BufferedRelaysFailure) {
                body.to_vec()
            } else if protocol == NorthPassthroughProtocolV2::Messages {
                json!({"type": "error", "error": {"type": "api_error",
                                 "message": REFERENCE_NORTH_PASSTHROUGH_FAILURE_MESSAGE_V1}})
                .to_string()
                .into_bytes()
            } else {
                json!({"error": {"type": "server_error",
                                 "message": REFERENCE_NORTH_PASSTHROUGH_FAILURE_MESSAGE_V1}})
                .to_string()
                .into_bytes()
            };
            steps.push(Step::Forwarded(answer));
        }
        steps.push(Step::Ended);
        NorthPassthroughTimelineV1::new(steps)
    }
}

/// The relayed stream's identity: `response.id`, `response.model`, `response.created_at`.
type Identity = (Value, Value, Value);

struct Run<'a> {
    executor: ReferenceNorthPassthroughExecutorV1,
    protocol: NorthPassthroughProtocolV2,
    script: &'a NorthPassthroughScriptV1,
    steps: Vec<NorthPassthroughStepV1>,
    calls: usize,
    identity: Option<Identity>,
    last_sequence: Option<u64>,
    done: bool,
    /// `messages`: whether a `message_delta` answered `Done`.
    armed: bool,
    /// `messages`: the frames withheld since `Done`.
    held: Vec<Vec<u8>>,
}

/// The Messages event type of a frame: its SSE `event:` name, or the `type` of its JSON data
/// when it names none (`NorthPassthroughFrameV1::messages_type`).
fn messages_type_of(frame: &[u8], data: Option<&Value>) -> Option<String> {
    let named = decode_sse_v1(frame)
        .ok()
        .and_then(|events| events.first().map(|event| event.event().to_owned()))
        .filter(|name| name != DEFAULT_SSE_EVENT_TYPE);
    named.or_else(|| data.and_then(|data| data["type"].as_str()).map(str::to_owned))
}

/// The JSON data of a frame's first event.
fn data_of(frame: &[u8]) -> Option<Value> {
    let events = decode_sse_v1(frame).ok()?;
    serde_json::from_str(events.first()?.data()).ok()
}

impl<'a> Run<'a> {
    const fn new(
        executor: ReferenceNorthPassthroughExecutorV1,
        protocol: NorthPassthroughProtocolV2,
        script: &'a NorthPassthroughScriptV1,
    ) -> Self {
        Self {
            executor,
            protocol,
            script,
            steps: Vec::new(),
            calls: 0,
            identity: None,
            last_sequence: None,
            done: false,
            armed: false,
            held: Vec::new(),
        }
    }

    fn is(&self, fault: NorthPassthroughFaultV1) -> bool {
        self.executor.is(fault)
    }

    fn handle(&mut self, frame: Vec<u8>, at_end_of_input: bool) {
        use NorthPassthroughFaultV1 as Fault;
        use NorthPassthroughFrameVerdictV1 as Verdict;
        use NorthPassthroughStepV1 as Step;
        if self.protocol == NorthPassthroughProtocolV2::Messages {
            self.handle_messages(frame, at_end_of_input);
            return;
        }
        let data = data_of(&frame);
        let terminal_type = data.as_ref().is_some_and(|data| self.is_terminal(data));
        self.steps.push(Step::Fed(frame.clone()));
        let verdict = self.script.frame(self.calls);
        self.calls += 1;
        let early =
            self.is(Fault::ForwardBeforeReturn) && verdict == Verdict::Events && !terminal_type;
        if early {
            self.forward(&frame, data.as_ref());
        }
        self.steps.push(Step::Returned);
        let releases = if self.is(Fault::TerminalOnDoneAlone) {
            verdict == Verdict::Done
        } else if self.is(Fault::TerminalOnTypeAlone) {
            terminal_type && matches!(verdict, Verdict::Events | Verdict::Done)
        } else {
            terminal_type && verdict == Verdict::Done
        };
        match verdict {
            Verdict::Error => {
                if self.is(Fault::RelayFailureFrame) {
                    self.steps.push(Step::Forwarded(frame));
                }
                self.fail(Step::SettledAsFailure, data.as_ref());
            }
            Verdict::Trap => self.fail(Step::MarkedDeliveryUnknown, None),
            _ if releases => self.release(&frame, at_end_of_input),
            Verdict::Done => self.fail(Step::MarkedDeliveryUnknown, None),
            Verdict::Events if terminal_type => self.fail(Step::MarkedDeliveryUnknown, None),
            Verdict::Events => {
                if !early {
                    self.forward(&frame, data.as_ref());
                }
            }
        }
    }

    /// One `messages` frame (record §3.5): `Done` must arrive on `message_delta`; from it on every
    /// frame is withheld, `ping` and `message_stop` with nothing to report, and `message_stop`
    /// releases them all once the settlement persists. Anything else after `Done` parks.
    fn handle_messages(&mut self, frame: Vec<u8>, at_end_of_input: bool) {
        use NorthPassthroughFaultV1 as Fault;
        use NorthPassthroughFrameVerdictV1 as Verdict;
        use NorthPassthroughStepV1 as Step;
        let data = data_of(&frame);
        let kind = messages_type_of(&frame, data.as_ref());
        let is = |wanted: &str| kind.as_deref() == Some(wanted);
        let terminal = is(NORTH_PASSTHROUGH_MESSAGES_TERMINAL_TYPE);
        self.steps.push(Step::Fed(frame.clone()));
        let verdict = self.script.frame(self.calls);
        self.calls += 1;
        let early = self.is(Fault::ForwardBeforeReturn)
            && verdict == Verdict::Events
            && !self.armed
            && !terminal;
        if early {
            self.forward(&frame, data.as_ref());
        }
        self.steps.push(Step::Returned);
        match verdict {
            Verdict::Error => {
                if self.is(Fault::RelayFailureFrame) {
                    self.steps.push(Step::Forwarded(frame));
                }
                self.fail(Step::SettledAsFailure, data.as_ref());
            }
            Verdict::Done
                if !self.armed
                    && (is(NORTH_PASSTHROUGH_MESSAGES_DONE_TYPE)
                        || self.is(Fault::TerminalOnDoneAlone)) =>
            {
                self.armed = true;
                if self.is(Fault::MessagesForwardDoneFrame) {
                    self.forward(&frame, data.as_ref());
                } else {
                    self.held.push(frame);
                }
            }
            Verdict::Events if self.armed && is(NORTH_PASSTHROUGH_MESSAGES_PING_TYPE) => {
                self.held.push(frame);
            }
            Verdict::Events if terminal && (self.armed || self.is(Fault::TerminalOnTypeAlone)) => {
                self.held.push(frame);
                self.release_held(at_end_of_input);
            }
            Verdict::Events if !self.armed && !terminal => {
                if !early {
                    self.forward(&frame, data.as_ref());
                }
            }
            // A trap, `Done` on any other frame or twice, and any other frame after `Done`.
            _ => self.fail(Step::MarkedDeliveryUnknown, None),
        }
    }

    /// `messages`: releases every withheld frame once the settlement persists, as [`Self::release`]
    /// does for one terminal frame.
    fn release_held(&mut self, at_end_of_input: bool) {
        use NorthPassthroughFaultV1 as Fault;
        use NorthPassthroughStepV1 as Step;
        let mut frames = std::mem::take(&mut self.held);
        if at_end_of_input
            && !self.is(Fault::NoTerminatorRepair)
            && let Some(last) = frames.last_mut()
        {
            let terminator = north_passthrough_terminator_v1(last);
            last.extend_from_slice(terminator);
        }
        if self.is(Fault::ReleaseTerminalBeforeSettlement) {
            self.steps.extend(frames.iter().cloned().map(Step::Forwarded));
        }
        match self.script.settlement() {
            NorthPassthroughSettlementV1::Persists => {
                self.steps.push(Step::SettlementPersisted);
                if !self.is(Fault::ReleaseTerminalBeforeSettlement) {
                    self.steps.extend(frames.into_iter().map(Step::Forwarded));
                }
                self.end();
            }
            NorthPassthroughSettlementV1::Fails => {
                if self.is(Fault::ForwardTerminalWhenSettlementFails) {
                    self.steps.extend(frames.into_iter().map(Step::Forwarded));
                }
                self.fail(Step::MarkedDeliveryUnknown, None);
            }
        }
    }

    /// Whether `data` marks the terminal frame of the run's protocol.
    fn is_terminal(&self, data: &Value) -> bool {
        if self.protocol == NorthPassthroughProtocolV2::GenerateContent
            && self.is(NorthPassthroughFaultV1::TerminalOnFirstCandidateOnly)
        {
            return !data["candidates"][0]["finishReason"].is_null();
        }
        north_passthrough_is_terminal_data_v2(self.protocol, data)
    }

    fn forward(&mut self, frame: &[u8], data: Option<&Value>) {
        use NorthPassthroughStepV1 as Step;
        if self.executor.keep_alive_at_boundaries {
            self.steps.push(Step::Forwarded(b": keep-alive\n\n".to_vec()));
        }
        if self.is(NorthPassthroughFaultV1::KeepAliveMidFrame) && frame.len() > 1 {
            let (head, tail) = frame.split_at(frame.len() / 2);
            self.steps.push(Step::Forwarded(head.to_vec()));
            self.steps.push(Step::Forwarded(b": keep-alive\n".to_vec()));
            self.steps.push(Step::Forwarded(tail.to_vec()));
        } else {
            self.steps.push(Step::Forwarded(frame.to_vec()));
        }
        if let Some(data) = data {
            if self.identity.is_none() {
                let response = &data["response"];
                if response.is_object() {
                    self.identity = Some((
                        response["id"].clone(),
                        response["model"].clone(),
                        response["created_at"].clone(),
                    ));
                }
            }
            if let Some(sequence) = data["sequence_number"].as_u64() {
                self.last_sequence = Some(sequence);
            }
        }
    }

    fn release(&mut self, frame: &[u8], at_end_of_input: bool) {
        use NorthPassthroughFaultV1 as Fault;
        use NorthPassthroughStepV1 as Step;
        let mut bytes = frame.to_vec();
        if at_end_of_input && !self.is(Fault::NoTerminatorRepair) {
            bytes.extend_from_slice(north_passthrough_terminator_v1(frame));
        }
        if self.is(Fault::ReleaseTerminalBeforeSettlement) {
            self.steps.push(Step::Forwarded(bytes.clone()));
        }
        match self.script.settlement() {
            NorthPassthroughSettlementV1::Persists => {
                self.steps.push(Step::SettlementPersisted);
                if !self.is(Fault::ReleaseTerminalBeforeSettlement) {
                    self.steps.push(Step::Forwarded(bytes));
                }
                self.end();
            }
            NorthPassthroughSettlementV1::Fails => {
                if self.is(Fault::ForwardTerminalWhenSettlementFails) {
                    self.steps.push(Step::Forwarded(bytes));
                }
                self.fail(Step::MarkedDeliveryUnknown, None);
            }
        }
    }

    fn finish(&mut self) {
        use NorthPassthroughStepV1 as Step;
        self.steps.push(Step::Finished);
        match self.script.finish() {
            NorthPassthroughFinishVerdictV1::Clean | NorthPassthroughFinishVerdictV1::Truncated
                if self.is(NorthPassthroughFaultV1::TruncationSettles) =>
            {
                self.steps.push(Step::SettlementPersisted);
                self.end();
            }
            // `Done` with no frame of its own, a clean end with no terminal, or a truncation.
            _ => self.fail(Step::MarkedDeliveryUnknown, None),
        }
    }

    fn fail(&mut self, funds: NorthPassthroughStepV1, failing: Option<&Value>) {
        if self.protocol == NorthPassthroughProtocolV2::GenerateContent {
            // No failure event exists: record the funds transition and end the stream.
            self.steps.push(funds);
            if self.is(NorthPassthroughFaultV1::GenerateContentFailureFrame) {
                let message = REFERENCE_NORTH_PASSTHROUGH_FAILURE_MESSAGE_V1;
                let data =
                    json!({"error": {"code": 500, "status": "INTERNAL", "message": message}});
                self.steps.push(NorthPassthroughStepV1::Forwarded(
                    format!("data: {data}\r\n\r\n").into_bytes(),
                ));
            }
            self.end();
            return;
        }
        if self.protocol == NorthPassthroughProtocolV2::Messages
            && self.is(NorthPassthroughFaultV1::MessagesNoFailureEvent)
        {
            self.steps.push(funds);
            self.end();
            return;
        }
        let event = if self.protocol == NorthPassthroughProtocolV2::Messages {
            self.messages_failure_event(failing)
        } else {
            self.failure_event(failing)
        };
        if self.is(NorthPassthroughFaultV1::FailureEventBeforeFunds) {
            self.steps.push(NorthPassthroughStepV1::Forwarded(event));
            self.steps.push(funds);
        } else {
            self.steps.push(funds);
            self.steps.push(NorthPassthroughStepV1::Forwarded(event));
        }
        self.end();
    }

    fn end(&mut self) {
        self.steps.push(NorthPassthroughStepV1::Ended);
        self.done = true;
    }

    /// The host's own Messages failure event: one `error` event with the fixed message.
    fn messages_failure_event(&self, failing: Option<&Value>) -> Vec<u8> {
        let upstream_text = failing.and_then(|data| data["error"]["message"].as_str());
        let message = match upstream_text {
            Some(text) if self.is(NorthPassthroughFaultV1::LeakUpstreamMessage) => text,
            _ => REFERENCE_NORTH_PASSTHROUGH_FAILURE_MESSAGE_V1,
        };
        let data = json!({"type": "error", "error": {"type": "api_error", "message": message}});
        format!("event: error\ndata: {data}\n\n").into_bytes()
    }

    /// The host's own failure event: one `response.failed` on a relayed stream, or `response.created`
    /// and `response.failed` when nothing was relayed.
    fn failure_event(&self, failing: Option<&Value>) -> Vec<u8> {
        let upstream_text = failing.and_then(|data| {
            data["error"]["message"]
                .as_str()
                .or_else(|| data["response"]["error"]["message"].as_str())
        });
        let message = match upstream_text {
            Some(text) if self.is(NorthPassthroughFaultV1::LeakUpstreamMessage) => text.to_owned(),
            _ => REFERENCE_NORTH_PASSTHROUGH_FAILURE_MESSAGE_V1.to_owned(),
        };
        let failed = |(id, model, created_at): Identity, sequence: u64| {
            let data = json!({"type": "response.failed", "sequence_number": sequence,
                "response": {"id": id, "object": "response", "created_at": created_at, "model": model,
                             "status": "failed", "output": [],
                             "error": {"type": "server_error", "code": "server_error",
                                       "message": message}}});
            format!("event: response.failed\ndata: {data}\n\n")
        };
        let skip = if self.is(NorthPassthroughFaultV1::WrongFailureSequence) { 2 } else { 1 };
        self.identity.as_ref().map_or_else(
            || {
                let identity = (json!("resp_host_minted"), json!("routed-model"), json!(0));
                let created = json!({"type": "response.created", "sequence_number": 0,
                    "response": {"id": identity.0, "object": "response", "created_at": identity.2,
                                 "model": identity.1, "status": "in_progress", "output": []}});
                format!("event: response.created\ndata: {created}\n\n{}", failed(identity, skip))
                    .into_bytes()
            },
            |identity| {
                let sequence = self.last_sequence.map_or(0, |last| last + skip);
                failed(identity.clone(), sequence).into_bytes()
            },
        )
    }
}
