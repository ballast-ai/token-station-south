//! Outbound, streaming: canonical [`StreamEvent`]s -> a client's SSE chunks.
//!
//! # The codec owns one thing: this stream's protocol render state
//!
//! Block numbering, the message-level facts that may only be sent once, the
//! usage folded so far. It does **not** own the stream table, reclamation, or
//! the act of writing bytes — those are the host's — and it does not own the
//! *decode* state of whatever produced these events, which belongs to the
//! provider component. Keeping those three apart is what lets a host hold a
//! render state without being able to reach a decoder's fields, and vice versa.
//!
//! # Framing does not depend on how the caller batches
//!
//! A caller may hand over one event or twenty; the frames must come out the
//! same either way, because the alternative is a client-visible shape that
//! changes with upstream TCP timing. The one merge this wire needs —
//! `finish_reason` and `usage` on a single chunk — therefore holds the pending
//! finish *across* calls rather than flushing at the end of each one. The two
//! flush triggers both live in the event stream: a later renderable event
//! supersedes the pending finish, and `Done` flushes it.
//!
//! A consequence worth stating: a stream that ends without ever sending `Done`
//! produces **no** finish chunk. That stream did not finish, and inventing a
//! clean `finish_reason` for it would render "the upstream stopped talking" as
//! "the model completed normally".

use serde_json::{Value, json};
use token_station_protocol::{FinishReason, StreamEvent, Usage};

/// One stream's north-bound render state.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NorthSseState {
    /// The routed model name, supplied by the host.
    ///
    /// IR stream events deliberately do not carry it — the model is a routing
    /// decision, not stream content — so a renderer fed purely by component
    /// events has no other source for it.
    pub model: Option<String>,
    /// The message id, minted by the host (this crate reads no randomness).
    pub message_id: Option<String>,
    /// Set by an `Error` event: nothing is rendered afterwards.
    ///
    /// An error is the end of the stream. Emitting a normal terminal after it
    /// would tell the client the message completed, and the client would treat
    /// half an answer as a whole one.
    ///
    /// A *repeated* terminal is not handled here: the host's usage accumulator
    /// already refuses a second one and parks the request, and a second judge
    /// in the renderer would only give the same rule two places to drift.
    pub terminated: bool,
    /// A `finish_reason` waiting to see whether the next event is a `Usage` it
    /// should merge with. See the module docs.
    pub pending_finish: Option<Value>,
    /// Usage folded across however many reports the upstream sent.
    pub usage: Usage,
}

impl NorthSseState {
    /// A state with the routed model pre-set.
    #[must_use]
    pub fn for_model(model: impl Into<String>) -> Self {
        Self { model: Some(model.into()), ..Self::default() }
    }

    /// Attach the host-minted message id.
    #[must_use]
    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }
}

/// Canonical stream events -> `OpenAI` Chat SSE chunks (the `data:` payloads).
///
/// The `[DONE]` sentinel is not produced here: it is a transport concern, and
/// the host emits it after settlement so the client never sees the stream close
/// before the request is accounted for.
#[must_use]
pub fn openai_chat_frames(events: &[StreamEvent], state: &mut NorthSseState) -> Vec<Value> {
    let mut chunks = Vec::new();
    for event in events {
        if state.terminated {
            break;
        }
        match event {
            StreamEvent::ThinkingDelta { index, thinking_delta } => {
                flush_pending(state, &mut chunks);
                chunks.push(delta(*index, &json!({"reasoning_content": thinking_delta})));
            }
            StreamEvent::Delta { index, content } => {
                flush_pending(state, &mut chunks);
                chunks.push(delta(*index, &json!({"content": content})));
            }
            StreamEvent::ToolCallDelta { index, id, name, arguments_delta } => {
                flush_pending(state, &mut chunks);
                let mut function = json!({"arguments": arguments_delta});
                if let Some(name) = name {
                    function["name"] = json!(name);
                }
                let mut call = json!({"index": index, "function": function});
                if let Some(id) = id {
                    call["id"] = json!(id);
                }
                chunks.push(delta(0, &json!({"tool_calls": [call]})));
            }
            StreamEvent::Finish { finish_reason, .. } => {
                flush_pending(state, &mut chunks);
                state.pending_finish = Some(render_finish_reason(finish_reason.as_ref()));
            }
            StreamEvent::Usage { usage } => {
                // Multiple reports are folded rather than replaced: a provider
                // that sends input counts up front and the output count at the
                // end is reporting one usage in two instalments, and a client
                // that saw only the last instalment would see the first half
                // zeroed.
                state.usage.absorb(*usage);
                chunks.push(usage_chunk(state.pending_finish.take(), state.usage));
            }
            StreamEvent::Done { .. } => flush_pending(state, &mut chunks),
            StreamEvent::ThinkingSignatureDelta { .. } => {
                // No slot on this wire. Dropped here and nowhere else.
            }
            StreamEvent::Error { .. } => state.terminated = true,
        }
    }
    chunks
}

fn usage_chunk(pending_finish: Option<Value>, usage: Usage) -> Value {
    let mut chunk = pending_finish.map_or_else(
        // `stream_options.include_usage` shape: a trailing chunk with no choices.
        || json!({"object": "chat.completion.chunk", "choices": []}),
        // The wire puts finish_reason and usage on one chunk.
        |reason| {
            json!({
                "object": "chat.completion.chunk",
                "choices": [{"index": 0, "delta": {}, "finish_reason": reason}],
            })
        },
    );
    let mut reported = json!({
        "prompt_tokens": usage.input_tokens,
        "completion_tokens": usage.output_tokens,
        "total_tokens": usage.input_tokens + usage.output_tokens,
    });
    // Emitted only when non-zero, so a provider without cache or reasoning
    // reporting produces exactly the three fields it always did.
    if usage.cache_read_tokens > 0 {
        reported["prompt_tokens_details"] = json!({"cached_tokens": usage.cache_read_tokens});
    }
    if usage.reasoning_tokens > 0 {
        reported["completion_tokens_details"] = json!({"reasoning_tokens": usage.reasoning_tokens});
    }
    chunk["usage"] = reported;
    chunk
}

fn render_finish_reason(reason: Option<&FinishReason>) -> Value {
    reason.map_or(Value::Null, |reason| json!(reason))
}

fn delta(index: u32, delta: &Value) -> Value {
    json!({
        "object": "chat.completion.chunk",
        "choices": [{"index": index, "delta": delta.clone(), "finish_reason": Value::Null}],
    })
}

/// Send a pending finish as its own chunk (it did not get a `Usage` to merge with).
fn flush_pending(state: &mut NorthSseState, chunks: &mut Vec<Value>) {
    if let Some(reason) = state.pending_finish.take() {
        chunks.push(json!({
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {}, "finish_reason": reason}],
        }));
    }
}
