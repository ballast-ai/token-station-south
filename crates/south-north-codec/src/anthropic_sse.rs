//! Outbound, streaming: canonical [`StreamEvent`]s -> Anthropic Messages SSE.
//!
//! # What this state owns
//!
//! One stream's *render* state for this protocol: which content blocks are
//! open and under which numbers, whether the message has been announced, and
//! the usage folded so far. It does not own the stream table, reclamation, the
//! act of writing bytes, or the *decode* state of whatever produced these
//! events — the last one belongs to the provider component, and keeping the
//! two apart is what stops a host from reaching a decoder's fields.
//!
//! # Identity comes from the host, always
//!
//! This crate reads no clock and no randomness, so the message id on
//! `message_start` is supplied at construction rather than minted here. There
//! is deliberately no `Default`: an id-less state could only fall back to a
//! placeholder, and a placeholder that reaches a client is a message id that
//! identifies nothing.
//!
//! # The frames a stream does and does not get
//!
//! `message_start` is announced by the first event that renders something, not
//! by the first event of any kind — a stream that only ever reports usage has
//! not begun a message. The terminal trio (`content_block_stop`s,
//! `message_delta`, `message_stop`) is produced by `Done` and by nothing else,
//! so a stream that stops mid-flight leaves the message unclosed. That is the
//! honest render: inventing a clean `stop_reason` for it would tell the client
//! the model finished when the upstream simply stopped talking.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use token_station_protocol::{ErrorCode, FinishReason, StreamEvent, Usage};

use crate::anthropic::stop_reason;

/// One SSE frame: this wire names its event type as well as carrying a payload.
///
/// The event name is `&'static str` because this protocol's set of them is
/// closed — a frame with a name the protocol does not define is not a frame a
/// client can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicFrame {
    pub event: &'static str,
    pub data: Value,
}

/// One stream's Anthropic-bound render state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicSseState {
    /// The routed model name, supplied by the host: IR stream events do not
    /// carry it, because the model is a routing decision rather than stream
    /// content.
    pub model: String,
    /// The `msg_…` id the host minted for this message (see the module docs).
    pub message_id: String,
    /// Whether `message_start` has been sent. It may be sent only once.
    pub message_started: bool,
    /// The next content-block number to hand out.
    pub next_index: usize,
    pub thinking_index: Option<usize>,
    pub text_index: Option<usize>,
    /// IR tool slot (the wire `index` of a tool call) -> content-block number.
    pub tool_blocks: BTreeMap<u32, usize>,
    /// Usage folded across however many reports the upstream sent.
    pub usage: Usage,
    /// A `Finish` arrived ahead of `Done` and parked its stop semantics here.
    pub pending_finish_reason: Option<FinishReason>,
    pub pending_stop_sequence: Option<String>,
    /// Set by an `Error` event: nothing is rendered afterwards.
    ///
    /// An `error` frame is the end of a stream on this wire. Following it with
    /// `message_delta` + `message_stop` would tell the client the message
    /// completed normally, and the client would treat half an answer as whole.
    ///
    /// A *repeated* terminal is not judged here: the host's usage accumulator
    /// already refuses a second one and parks the request, and a second judge
    /// in the renderer would only give one rule two places to drift.
    pub terminated: bool,
}

impl AnthropicSseState {
    /// A fresh state for one stream, with the two facts only the host knows.
    #[must_use]
    pub fn new(model: impl Into<String>, message_id: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            message_id: message_id.into(),
            message_started: false,
            next_index: 0,
            thinking_index: None,
            text_index: None,
            tool_blocks: BTreeMap::new(),
            usage: Usage::default(),
            pending_finish_reason: None,
            pending_stop_sequence: None,
            terminated: false,
        }
    }

    /// Announce the message once, before the first block that needs it.
    fn ensure_message_start(&mut self, frames: &mut Vec<AnthropicFrame>) {
        if self.message_started {
            return;
        }
        self.message_started = true;
        // Whatever usage has been reported so far. A provider that reports its
        // input buckets up front (this protocol's own upstreams do) gets them
        // announced here; one that reports everything at the end gets zeros
        // here and the real numbers on `message_delta`.
        let mut usage = json!({"input_tokens": self.usage.input_tokens, "output_tokens": 0});
        if self.usage.cache_read_tokens > 0 {
            usage["cache_read_input_tokens"] = json!(self.usage.cache_read_tokens);
        }
        if self.usage.cache_write_tokens > 0 {
            usage["cache_creation_input_tokens"] = json!(self.usage.cache_write_tokens);
        }
        frames.push(AnthropicFrame {
            event: "message_start",
            data: json!({
                "type": "message_start",
                "message": {
                    "id": self.message_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": self.model,
                    "stop_reason": Value::Null,
                    "stop_sequence": Value::Null,
                    "usage": usage,
                }
            }),
        });
    }

    fn open_block(&mut self, content_block: &Value, frames: &mut Vec<AnthropicFrame>) -> usize {
        self.ensure_message_start(frames);
        let index = self.next_index;
        self.next_index += 1;
        frames.push(AnthropicFrame {
            event: "content_block_start",
            data: json!({
                "type": "content_block_start",
                "index": index,
                "content_block": content_block,
            }),
        });
        index
    }

    fn thinking_block(&mut self, frames: &mut Vec<AnthropicFrame>) -> usize {
        if let Some(index) = self.thinking_index {
            return index;
        }
        let index = self.open_block(&json!({"type": "thinking", "thinking": ""}), frames);
        self.thinking_index = Some(index);
        index
    }

    fn text_block(&mut self, frames: &mut Vec<AnthropicFrame>) -> usize {
        if let Some(index) = self.text_index {
            return index;
        }
        let index = self.open_block(&json!({"type": "text", "text": ""}), frames);
        self.text_index = Some(index);
        index
    }

    fn tool_block(
        &mut self,
        slot: u32,
        id: Option<&str>,
        name: Option<&str>,
        frames: &mut Vec<AnthropicFrame>,
    ) -> usize {
        if let Some(index) = self.tool_blocks.get(&slot) {
            return *index;
        }
        // A call with no id of its own is numbered from the message id and the
        // block number rather than from a random source, so the same stream
        // renders byte for byte the same every time.
        let derived =
            format!("toolu_{}_{}", self.message_id.trim_start_matches("msg_"), self.next_index);
        let block = json!({
            "type": "tool_use",
            "id": id.map_or(derived, str::to_owned),
            "name": name.map_or(Value::Null, |name| json!(name)),
            "input": {},
        });
        let index = self.open_block(&block, frames);
        self.tool_blocks.insert(slot, index);
        index
    }

    /// Close every block this message opened, in the order they were opened.
    fn close_open_blocks(&mut self, frames: &mut Vec<AnthropicFrame>) {
        let mut open: Vec<usize> = self.thinking_index.take().into_iter().collect();
        open.extend(self.text_index.take());
        open.extend(std::mem::take(&mut self.tool_blocks).into_values());
        for index in open {
            frames.push(AnthropicFrame {
                event: "content_block_stop",
                data: json!({"type": "content_block_stop", "index": index}),
            });
        }
    }

    /// Back to a state that can render another message, keeping the two facts
    /// that identify the stream rather than the message.
    fn reset_message(&mut self) {
        let model = std::mem::take(&mut self.model);
        let message_id = std::mem::take(&mut self.message_id);
        *self = Self::new(model, message_id);
    }
}

/// Canonical stream events -> Anthropic Messages SSE frames.
#[must_use]
pub fn anthropic_frames(
    events: &[StreamEvent],
    state: &mut AnthropicSseState,
) -> Vec<AnthropicFrame> {
    let mut frames = Vec::new();
    for event in events {
        if state.terminated {
            break;
        }
        match event {
            // The choice index is not consulted: this envelope carries one
            // message and the request that produced these events could not
            // have asked for a second answer, because this protocol has no `n`.
            StreamEvent::ThinkingDelta { thinking_delta, .. } => {
                let index = state.thinking_block(&mut frames);
                frames.push(block_delta(
                    index,
                    &json!({"type": "thinking_delta", "thinking": thinking_delta}),
                ));
            }
            StreamEvent::Delta { content, .. } => {
                let index = state.text_block(&mut frames);
                frames.push(block_delta(index, &json!({"type": "text_delta", "text": content})));
            }
            StreamEvent::ToolCallDelta { index, id, name, arguments_delta } => {
                let block = state.tool_block(*index, id.as_deref(), name.as_deref(), &mut frames);
                // An empty fragment is the call announcing itself; the block
                // start already carried the id and name, and an empty
                // `partial_json` would only add a frame with nothing in it.
                if !arguments_delta.is_empty() {
                    frames.push(block_delta(
                        block,
                        &json!({"type": "input_json_delta", "partial_json": arguments_delta}),
                    ));
                }
            }
            // The signature closes a thinking block. With no such block open
            // there is nothing to sign, and starting a message to say so would
            // announce a message that has no content and may never get one.
            StreamEvent::ThinkingSignatureDelta { signature_delta, .. } => {
                if let Some(index) = state.thinking_index {
                    frames.push(block_delta(
                        index,
                        &json!({"type": "signature_delta", "signature": signature_delta}),
                    ));
                }
            }
            // Folded, not rendered: this wire reports usage on the frames it
            // already has. Folding rather than replacing is what keeps a
            // provider's input-up-front, output-at-the-end pair from zeroing
            // its own first instalment.
            StreamEvent::Usage { usage } => state.usage.absorb(*usage),
            // A finish reason that arrived before the stream is over. It parks
            // here: the frames that carry stop semantics are the terminal ones,
            // and a final usage report may still be on its way.
            StreamEvent::Finish { finish_reason, stop_sequence } => {
                if finish_reason.is_some() {
                    state.pending_finish_reason.clone_from(finish_reason);
                }
                if stop_sequence.is_some() {
                    state.pending_stop_sequence.clone_from(stop_sequence);
                }
            }
            StreamEvent::Done { finish_reason, stop_sequence } => {
                // What `Done` carries wins; what a preceding `Finish` parked is
                // the fallback.
                let reason = finish_reason.clone().or_else(|| state.pending_finish_reason.take());
                let sequence = stop_sequence.clone().or_else(|| state.pending_stop_sequence.take());
                state.ensure_message_start(&mut frames);
                state.close_open_blocks(&mut frames);
                frames.push(AnthropicFrame {
                    event: "message_delta",
                    data: json!({
                        "type": "message_delta",
                        "delta": {
                            "stop_reason": stop_reason(reason.as_ref()),
                            "stop_sequence": sequence.map_or(Value::Null, Value::String),
                        },
                        "usage": terminal_usage(state.usage),
                    }),
                });
                frames.push(AnthropicFrame {
                    event: "message_stop",
                    data: json!({"type": "message_stop"}),
                });
                state.reset_message();
            }
            StreamEvent::Error { error } => {
                frames.push(AnthropicFrame {
                    event: "error",
                    data: json!({
                        "type": "error",
                        "error": {
                            "type": error_type(error.code),
                            "message": error.message,
                        }
                    }),
                });
                state.terminated = true;
            }
        }
    }
    frames
}

/// The usage on the terminal `message_delta`.
///
/// `output_tokens` is what this wire has always carried here. The input-side
/// buckets are repeated when they are known, because a provider that reports
/// everything at the end would otherwise have its input count announced as
/// zero on `message_start` and never corrected — the client would be told the
/// message cost no input at all. This wire's usage is cumulative for the
/// message, so a client reading the last report reads the whole truth.
fn terminal_usage(usage: Usage) -> Value {
    let mut reported = json!({"output_tokens": usage.output_tokens});
    if usage.input_tokens > 0 {
        reported["input_tokens"] = json!(usage.input_tokens);
    }
    if usage.cache_read_tokens > 0 {
        reported["cache_read_input_tokens"] = json!(usage.cache_read_tokens);
    }
    if usage.cache_write_tokens > 0 {
        reported["cache_creation_input_tokens"] = json!(usage.cache_write_tokens);
    }
    reported
}

fn block_delta(index: usize, delta: &Value) -> AnthropicFrame {
    AnthropicFrame {
        event: "content_block_delta",
        data: json!({"type": "content_block_delta", "index": index, "delta": delta}),
    }
}

/// Canonical error code -> this wire's `error.type` vocabulary.
const fn error_type(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Auth => "authentication_error",
        ErrorCode::RateLimit | ErrorCode::Capacity => "rate_limit_error",
        // This protocol has no billing or context type of its own: it reports
        // both low credit and an over-long prompt as an invalid request.
        ErrorCode::InvalidRequest
        | ErrorCode::Capability
        | ErrorCode::ContentPolicy
        | ErrorCode::PaymentRequired
        | ErrorCode::ContextLength => "invalid_request_error",
        // Truncation mid-body and a 2xx carrying something else are the
        // upstream's fault, not the caller's.
        ErrorCode::UpstreamUnavailable
        | ErrorCode::Timeout
        | ErrorCode::Internal
        | ErrorCode::TransportTruncated
        | ErrorCode::ProviderProtocolError => "api_error",
    }
}
