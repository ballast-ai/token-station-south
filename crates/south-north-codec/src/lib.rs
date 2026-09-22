//! The north-bound codec: client wire <-> canonical IR, as pure mapping.
//!
//! Two products speak the same client-facing protocols and, until now, each
//! carried its own copy of the mapping between those protocols and the
//! canonical IR. One protocol fix therefore needed two implementations and two
//! reviews, and the two drifted in a dozen small ways that only showed up as
//! user-visible differences. This crate is the single implementation.
//!
//! # What lives here, and what deliberately does not
//!
//! Here: the protocol field mapping in both directions — inbound (client wire
//! -> [`ChatRequest`](token_station_protocol::ChatRequest)) and outbound
//! (canonical response/stream events -> client wire).
//!
//! Not here, on purpose:
//!
//! - **Product policy.** Admission, capability refusals, rate cards and
//!   amounts, retry budgets, disconnect handling, how an error maps onto an
//!   HTTP status. The codec can report that a conversion failed; it cannot
//!   decide whether that means money may be released or an upstream may be
//!   safely retried.
//! - **Ambient inputs.** No clock, no randomness, no network, no database, no
//!   tenant, no ledger, no credential source. Anything the wire needs that the
//!   IR does not carry — a response `created` stamp, a minted message id — is
//!   passed in by the caller as an explicit context. That is what makes the
//!   output of every function here a function of its inputs alone, and hence
//!   testable by equality rather than by pattern.
//! - **The upstream direction.** Rendering IR into a *provider's* wire, and
//!   parsing a provider's response back into IR, belong to the provider
//!   components. A north-bound codec that also spoke upstream would have no
//!   boundary left to defend.
//!
//! # Why the errors are typed
//!
//! A host has to turn a conversion failure into an HTTP status, a log line and
//! a funds decision. A string cannot be matched on, so every host ends up
//! re-parsing prose. [`CodecError`] carries a stable category and the field
//! path that failed; the host maps it, the codec never does.

use serde_json::Value;

pub mod anthropic;
pub mod anthropic_sse;
pub mod request;
pub mod response;
pub mod sse;

pub use anthropic::{anthropic_message_response, chat_request_from_anthropic_messages};
pub use anthropic_sse::{AnthropicFrame, AnthropicSseState, anthropic_frames};
pub use request::chat_request_from_openai_chat;
pub use response::{ResponseContext, openai_chat_response};
pub use sse::{OpenAiChatSseState, openai_chat_frames};

/// Why a conversion could not be completed.
///
/// Each variant names the wire field it is about, so a host can report the
/// offending field without re-parsing a message. Variants are added, never
/// repurposed: a host's mapping from category to HTTP status is a contract of
/// its own.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    /// A role, block type or enum value the protocol does not define.
    ///
    /// Refused rather than dropped: a silently discarded message is how a
    /// conversation loses a turn without anyone noticing.
    #[error("{field}: unknown value {value:?} (expected one of {expected})")]
    UnknownValue { field: String, value: String, expected: &'static str },
    /// A field whose value cannot be represented in the canonical IR.
    ///
    /// Reported rather than clamped. Clamping would substitute a number the
    /// client never asked for, and the client would learn about it only by
    /// seeing a truncated answer.
    #[error("{field}: {value} is outside the representable range ({limit})")]
    OutOfRange { field: String, value: String, limit: &'static str },
    /// The canonical response cannot be rendered onto this wire as-is.
    ///
    /// Two cases so far: a choice count the wire cannot express, and tool-call
    /// arguments that are not valid JSON. Both used to be papered over — the
    /// first by rendering only the first choice, the second by substituting an
    /// empty object — and both papered-over forms reach the client as a plausible
    /// answer that is not what the model produced.
    #[error("{field}: {detail}")]
    Unrenderable { field: String, detail: String },
}

impl CodecError {
    pub(crate) fn unknown_value(
        field: impl Into<String>,
        value: impl Into<String>,
        expected: &'static str,
    ) -> Self {
        Self::UnknownValue { field: field.into(), value: value.into(), expected }
    }

    pub(crate) fn out_of_range(
        field: impl Into<String>,
        value: impl Into<String>,
        limit: &'static str,
    ) -> Self {
        Self::OutOfRange { field: field.into(), value: value.into(), limit }
    }

    pub(crate) fn unrenderable(field: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Unrenderable { field: field.into(), detail: detail.into() }
    }

    /// The wire field this error is about, for a host that wants to echo it.
    #[must_use]
    pub fn field(&self) -> &str {
        match self {
            Self::UnknownValue { field, .. }
            | Self::OutOfRange { field, .. }
            | Self::Unrenderable { field, .. } => field,
        }
    }
}

/// A JSON value rendered as a short string for an error message.
///
/// Error text must not echo prompts or tool arguments back to the caller, so
/// this is deliberately lossy: scalars keep their value, containers report only
/// their kind.
pub(crate) fn describe(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(_) => "a string".to_owned(),
        Value::Array(_) => "an array".to_owned(),
        Value::Object(_) => "an object".to_owned(),
    }
}
