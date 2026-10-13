//! Outbound, streaming, with identity: canonical [`StreamEvent`]s -> complete `OpenAI` Chat chunks.
//!
//! [`crate::openai_chat_frames`] renders what the IR carries and nothing more, so every host that serves
//! this wire stamped `id`, `created` and `model` onto its chunks afterwards, decided which chunk was
//! terminal by probing `finish_reason`, and replaced that chunk's usage with its own rendering. Each of
//! those is a place where two hosts can drift. [`OpenAiChatStream`] does all three from a host-supplied
//! [`ChatStreamContext`] (render-gaps record G1, `docs/design/2026-09-30-north-codec-render-gaps.md`
//! §3); the older function and its state are left exactly as they were.
//!
//! # Rules
//!
//! - **Identity.** Every frame carries `id`, `object`, `created`, `model`, then `choices` (and `usage`),
//!   inserted in that order. Key order is not a codec contract (the record's N-Q5): renderings are
//!   compared as JSON values.
//! - **Kinds.** The chunk rendered from a `Finish` event is [`OpenAiChatFrameKind::Terminal`], whether it
//!   is flushed by a later event or by `Done`, and whether or not its `finish_reason` is null. The trailing
//!   `choices: []` chunk below is [`OpenAiChatFrameKind::Usage`]. Every other chunk is
//!   [`OpenAiChatFrameKind::Delta`].
//! - **Usage fold (N-Q2).** A `Usage` event never renders a chunk of its own: it is folded, and the folded
//!   usage is written on the terminal frame when that frame is emitted. When `Done` arrives and no
//!   `Finish` was ever seen, one `choices: []` frame carries the folded usage, if any was folded.
//! - **Role (N-Q4).** The first delta frame of each choice index carries `delta.role: "assistant"`,
//!   whatever else that delta carries (text, reasoning or a tool call). Terminal and usage frames never do.
//! - **Errors.** An `Error` event renders nothing and ends the stream, as on the older function. A usage
//!   total that does not fit in `u64` is [`CodecError::OutOfRange`] and also ends the stream: its terminal
//!   cannot be rendered, and a later call must not render one that pretends otherwise.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};
use token_station_protocol::{StreamEvent, Usage};

use crate::response::prefixed_id;
use crate::{CodecError, OpenAiChatSseState, openai_chat_frames};

const CHUNK_OBJECT: &str = "chat.completion.chunk";

/// The facts every chunk of one stream carries and the IR does not.
///
/// The host supplies them because only the host may mint an id or read a clock (see the crate docs). Which
/// model string clients see is the host's choice as well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatStreamContext {
    /// The stream's id. Gains the wire's `chatcmpl-` prefix unless it already has it.
    pub id: String,
    /// The model string written on every chunk.
    pub model: String,
    /// Unix seconds for every chunk's `created` field, read once per stream by the host.
    pub created: i64,
}

/// Which event produced a frame, so the host knows which frames to hold until settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiChatFrameKind {
    /// Content: text, reasoning or a tool-call fragment. Forwarded at once.
    Delta,
    /// The chunk rendered from a `Finish` event, null `finish_reason` included. Held until settlement.
    Terminal,
    /// The trailing `choices: []` chunk of a stream that never finished. Held until settlement.
    Usage,
}

/// One rendered chunk: its kind and its `data:` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiChatFrame {
    /// Which event produced this chunk.
    pub kind: OpenAiChatFrameKind,
    /// The complete chunk, identity included.
    pub data: Value,
}

/// One stream's `OpenAI` Chat render state with identity, kinds, the usage fold and the first-delta role.
///
/// Frames do not depend on how the caller batches events: each event is rendered on its own, and the one
/// merge this wire needs (a finish with its usage) is held across calls until `Done` or a later event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiChatStream {
    context: ChatStreamContext,
    /// The older render state, reused for the delta chunks and the pending finish.
    inner: OpenAiChatSseState,
    /// Choice indexes whose first delta frame has been emitted.
    roles_sent: BTreeSet<u32>,
    /// Whether any `Usage` event has been folded.
    usage_folded: bool,
    /// Whether any `Finish` event has been seen.
    finish_seen: bool,
    /// Whether the trailing `choices: []` frame has been emitted.
    usage_frame_sent: bool,
}

impl OpenAiChatStream {
    /// A fresh stream render state for one Chat stream.
    #[must_use]
    pub fn new(context: ChatStreamContext) -> Self {
        Self {
            context,
            inner: OpenAiChatSseState::default(),
            roles_sent: BTreeSet::new(),
            usage_folded: false,
            finish_seen: false,
            usage_frame_sent: false,
        }
    }

    /// Canonical stream events -> complete `OpenAI` Chat chunks (the `data:` payloads), with their kinds.
    ///
    /// The `[DONE]` sentinel is not produced here; the host writes it after settlement.
    ///
    /// # Errors
    ///
    /// [`CodecError::OutOfRange`] at `usage.total_tokens` when a frame's usage total does not fit in
    /// `u64`. The stream renders nothing afterwards.
    pub fn frames(&mut self, events: &[StreamEvent]) -> Result<Vec<OpenAiChatFrame>, CodecError> {
        let mut frames = Vec::new();
        for event in events {
            if self.inner.terminated {
                break;
            }
            if let Err(error) = self.render(event, &mut frames) {
                self.inner.terminated = true;
                return Err(error);
            }
        }
        Ok(frames)
    }

    fn render(
        &mut self,
        event: &StreamEvent,
        frames: &mut Vec<OpenAiChatFrame>,
    ) -> Result<(), CodecError> {
        match event {
            StreamEvent::Delta { index, .. } | StreamEvent::ThinkingDelta { index, .. } => {
                self.flush_terminal(frames)?;
                self.push_deltas(*index, event, frames);
            }
            // The older renderer puts every tool-call fragment on choice 0.
            StreamEvent::ToolCallDelta { .. } => {
                self.flush_terminal(frames)?;
                self.push_deltas(0, event, frames);
            }
            StreamEvent::RedactedThinking { .. } | StreamEvent::ThinkingSignatureDelta { .. } => {
                // No slot on this wire; the older renderer drops them, and they flush nothing.
            }
            StreamEvent::Finish { .. } => {
                self.flush_terminal(frames)?;
                self.finish_seen = true;
                // With no finish pending this renders nothing and only records the new one.
                let rendered = openai_chat_frames(std::slice::from_ref(event), &mut self.inner);
                debug_assert!(rendered.is_empty(), "a finish renders nothing on its own");
            }
            StreamEvent::Usage { usage } => {
                self.inner.usage.absorb(*usage);
                self.usage_folded = true;
            }
            StreamEvent::Done { .. } => {
                self.flush_terminal(frames)?;
                if !self.finish_seen && self.usage_folded && !self.usage_frame_sent {
                    let data = self.chunk(Vec::new(), Some(openai_chat_usage(&self.inner.usage)?));
                    self.usage_frame_sent = true;
                    frames.push(OpenAiChatFrame { kind: OpenAiChatFrameKind::Usage, data });
                }
            }
            StreamEvent::Error { .. } => self.inner.terminated = true,
        }
        Ok(())
    }

    /// Emit the pending finish, if any, as the terminal frame with the usage folded so far.
    fn flush_terminal(&mut self, frames: &mut Vec<OpenAiChatFrame>) -> Result<(), CodecError> {
        let Some(reason) = self.inner.pending_finish.take() else {
            return Ok(());
        };
        let usage =
            if self.usage_folded { Some(openai_chat_usage(&self.inner.usage)?) } else { None };
        let choice = json!({"index": 0, "delta": {}, "finish_reason": reason});
        let data = self.chunk(vec![choice], usage);
        frames.push(OpenAiChatFrame { kind: OpenAiChatFrameKind::Terminal, data });
        Ok(())
    }

    /// Render one content event through the older renderer, then add identity and the role.
    fn push_deltas(&mut self, choice: u32, event: &StreamEvent, frames: &mut Vec<OpenAiChatFrame>) {
        for mut chunk in openai_chat_frames(std::slice::from_ref(event), &mut self.inner) {
            let choices = match chunk.get_mut("choices").map(Value::take) {
                Some(Value::Array(choices)) => choices,
                _ => Vec::new(),
            };
            let choices = if self.roles_sent.insert(choice) {
                choices.into_iter().map(with_role).collect()
            } else {
                choices
            };
            let data = self.chunk(choices, None);
            frames.push(OpenAiChatFrame { kind: OpenAiChatFrameKind::Delta, data });
        }
    }

    /// One complete chunk, keys inserted in the record's order.
    fn chunk(&self, choices: Vec<Value>, usage: Option<Value>) -> Value {
        let mut chunk = Map::new();
        chunk.insert(
            "id".to_owned(),
            Value::String(prefixed_id(&self.context.id, "chatcmpl-", &self.context.id)),
        );
        chunk.insert("object".to_owned(), Value::String(CHUNK_OBJECT.to_owned()));
        chunk.insert("created".to_owned(), json!(self.context.created));
        chunk.insert("model".to_owned(), Value::String(self.context.model.clone()));
        chunk.insert("choices".to_owned(), Value::Array(choices));
        if let Some(usage) = usage {
            chunk.insert("usage".to_owned(), usage);
        }
        Value::Object(chunk)
    }
}

/// Put `role: "assistant"` first in a rendered choice's `delta`.
fn with_role(mut choice: Value) -> Value {
    if let Some(Value::Object(delta)) = choice.get_mut("delta") {
        let mut with = Map::new();
        with.insert("role".to_owned(), Value::String("assistant".to_owned()));
        with.extend(std::mem::take(delta));
        *delta = with;
    }
    choice
}

/// The one rendering of this wire's `usage` object.
///
/// `prompt_tokens`, `completion_tokens` and `total_tokens`, plus `prompt_tokens_details.cached_tokens`
/// and `completion_tokens_details.reasoning_tokens` when non-zero — the shape the codec's other Chat
/// renderers write. A host renders its settled usage with this after converting it to an IR [`Usage`].
///
/// # Errors
///
/// [`CodecError::OutOfRange`] at `usage.total_tokens` when the total does not fit in `u64`. Reported
/// rather than wrapped: a wrapped total is a plausible number that is not what was used.
pub fn openai_chat_usage(usage: &Usage) -> Result<Value, CodecError> {
    let total = usage.input_tokens.checked_add(usage.output_tokens).ok_or_else(|| {
        CodecError::out_of_range(
            "usage.total_tokens",
            format!("{} + {}", usage.input_tokens, usage.output_tokens),
            "u64",
        )
    })?;
    let mut reported = Map::new();
    reported.insert("prompt_tokens".to_owned(), json!(usage.input_tokens));
    reported.insert("completion_tokens".to_owned(), json!(usage.output_tokens));
    reported.insert("total_tokens".to_owned(), json!(total));
    if usage.cache_read_tokens > 0 {
        reported.insert(
            "prompt_tokens_details".to_owned(),
            json!({"cached_tokens": usage.cache_read_tokens}),
        );
    }
    if usage.reasoning_tokens > 0 {
        reported.insert(
            "completion_tokens_details".to_owned(),
            json!({"reasoning_tokens": usage.reasoning_tokens}),
        );
    }
    Ok(Value::Object(reported))
}
