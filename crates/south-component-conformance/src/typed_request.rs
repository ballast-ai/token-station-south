//! Reading the request fields protocol 0.6.0 (canonical IR 4) promoted from `extensions` (R-Q15;
//! KC1 record `docs/design/2026-10-12-kc1-canonical-ir-4-south.md` §2.3).
//!
//! `reasoning_effort` and `parallel_tool_calls` kept their wire keys, so a deserialized request
//! carries them in the typed fields only and a reader of `extensions` would silently find nothing.
//! `responses_tool_strict` changed shape (a request-level map became `ToolDef::strict`) and is not
//! lifted at deserialization (kernel ruling 4), so the typed field wins and the map is the
//! transition fallback.

use serde_json::Value;
use token_station_protocol::{ChatRequest, ReasoningMode, ToolDef};

use crate::responses_vocabulary::extension;

/// The tool's `strict` flag: [`ToolDef::strict`], else the caller's entry in the
/// `responses_tool_strict` map a producer built before protocol 0.6.0 still writes.
pub(crate) fn tool_strict(request: &ChatRequest, tool: &ToolDef) -> Option<bool> {
    tool.strict.or_else(|| {
        request
            .extensions
            .get(extension::TOOL_STRICT)
            .and_then(|strict| strict.get(&tool.name))
            .and_then(Value::as_bool)
    })
}

/// Whether the caller turned Anthropic-style thinking on (`enabled` or `adaptive`): the typed
/// mode, or the `anthropic_thinking` extension an older producer wrote.
pub(crate) fn thinking_requested(request: &ChatRequest) -> bool {
    matches!(request.reasoning.mode, Some(ReasoningMode::Enabled | ReasoningMode::Adaptive))
        || request
            .extensions
            .get("anthropic_thinking")
            .and_then(|thinking| thinking.get("type"))
            .and_then(Value::as_str)
            .is_some_and(|kind| matches!(kind, "adaptive" | "enabled"))
}
