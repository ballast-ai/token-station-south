//! Reading the request fields protocol 0.6.0 (canonical IR 4) promoted from `extensions` (R-Q15;
//! KC1 record `docs/design/2026-10-12-kc1-canonical-ir-4-south.md` §2.3).
//!
//! `reasoning_effort` and `parallel_tool_calls` kept their wire keys, so a deserialized request
//! carries them in the typed fields only and a reader of `extensions` would silently find nothing.
//! `responses_tool_strict` changed shape (a request-level map became `ToolDef::strict`) and is not
//! lifted at deserialization (kernel ruling 4), so the typed field wins and the map is the
//! transition fallback.

use serde_json::Value;
use token_station_protocol::{ChatRequest, Content, ContentPart, ReasoningMode, ToolDef};

use crate::responses_vocabulary::extension;

/// The tool's `strict` flag: [`ToolDef::strict`], else the caller's entry in the
/// `responses_tool_strict` map a producer built before protocol 0.6.0 still writes.
pub fn tool_strict(request: &ChatRequest, tool: &ToolDef) -> Option<bool> {
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
pub fn thinking_requested(request: &ChatRequest) -> bool {
    matches!(request.reasoning.mode, Some(ReasoningMode::Enabled | ReasoningMode::Adaptive))
        || request
            .extensions
            .get("anthropic_thinking")
            .and_then(|thinking| thinking.get("type"))
            .and_then(Value::as_str)
            .is_some_and(|kind| matches!(kind, "adaptive" | "enabled"))
}

/// The joined text of a text-only part list that is a list only because of prompt-cache markers.
///
/// The Messages decode keeps a marked system prompt or tool result as one text part per block
/// (KC1 record §3.1, K2); unmarked, the same content arrives as one string joined with `\n`. A
/// dialect without inline markers drops them (§4.3 to §4.5) and renders this string, so a marker
/// changes nothing else about the request it builds. `None` for any other content.
pub fn marker_only_text(content: Option<&Content>) -> Option<String> {
    let Some(Content::Parts(parts)) = content else { return None };
    let mut texts = Vec::with_capacity(parts.len());
    let mut marked = false;
    for part in parts {
        let ContentPart::Text { text, cache_control } = part else { return None };
        marked |= cache_control.is_some();
        texts.push(text.as_str());
    }
    marked.then(|| texts.join("\n"))
}

/// `content`, with a marker-only part list (see [`marker_only_text`]) replaced by its joined text.
pub fn without_markers(content: Option<&Content>) -> Option<std::borrow::Cow<'_, Content>> {
    marker_only_text(content).map_or_else(
        || content.map(std::borrow::Cow::Borrowed),
        |text| Some(std::borrow::Cow::Owned(Content::Text(text))),
    )
}
