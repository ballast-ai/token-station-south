//! The `OpenAI` Responses wire vocabulary this crate's components share (`OpenAI` Responses upstream
//! record §9; ruled R-Q13, 2026-10-10).
//!
//! One module, here, because guests already depend on this crate. `south-north-codec` maps the
//! same wire in the opposite direction and may depend only on the kernel IR, so it keeps its own
//! literals; the test `openai_responses_vocabulary_v1` depends on both crates and asserts that the
//! two spellings agree.

/// The `extensions` keys an `OpenAI` component acts on. Exactly the three the `OpenAI`-compatible
/// reference already reads (record §4.1, R-Q15): this module names them, it adds no use.
pub mod extension {
    /// Per tool name, the `strict` flag of a function tool.
    pub const TOOL_STRICT: &str = "responses_tool_strict";
    /// Whether the model may call several tools in one turn.
    pub const PARALLEL_TOOL_CALLS: &str = "parallel_tool_calls";
    /// The requested reasoning effort.
    pub const REASONING_EFFORT: &str = "reasoning_effort";
    /// The requested reasoning summary. The north codec writes it; no component acts on it in v1
    /// (record §4.1, R-Q15), so it is named here only for the agreement test.
    pub const REASONING_SUMMARY: &str = "responses_reasoning_summary";
    /// Marks the system message the north codec built from `instructions`. Named for the agreement
    /// test only; no component acts on it.
    pub const TRANSIENT_INSTRUCTIONS: &str = "responses_transient_instructions";
}

/// The `response.*` stream event types (record §6.1). The closed list a component accepts is
/// [`event::ACCEPTED`]; the failure frame `error` is recognised before it (record §6.4).
pub mod event {
    pub const CREATED: &str = "response.created";
    pub const IN_PROGRESS: &str = "response.in_progress";
    pub const QUEUED: &str = "response.queued";
    pub const COMPLETED: &str = "response.completed";
    pub const INCOMPLETE: &str = "response.incomplete";
    pub const FAILED: &str = "response.failed";
    pub const OUTPUT_ITEM_ADDED: &str = "response.output_item.added";
    pub const OUTPUT_ITEM_DONE: &str = "response.output_item.done";
    pub const CONTENT_PART_ADDED: &str = "response.content_part.added";
    pub const CONTENT_PART_DONE: &str = "response.content_part.done";
    pub const OUTPUT_TEXT_DELTA: &str = "response.output_text.delta";
    pub const OUTPUT_TEXT_DONE: &str = "response.output_text.done";
    pub const OUTPUT_TEXT_ANNOTATION_ADDED: &str = "response.output_text.annotation.added";
    pub const REFUSAL_DELTA: &str = "response.refusal.delta";
    pub const REFUSAL_DONE: &str = "response.refusal.done";
    pub const FUNCTION_CALL_ARGUMENTS_DELTA: &str = "response.function_call_arguments.delta";
    pub const FUNCTION_CALL_ARGUMENTS_DONE: &str = "response.function_call_arguments.done";
    pub const REASONING_SUMMARY_PART_ADDED: &str = "response.reasoning_summary_part.added";
    pub const REASONING_SUMMARY_PART_DONE: &str = "response.reasoning_summary_part.done";
    pub const REASONING_SUMMARY_TEXT_DELTA: &str = "response.reasoning_summary_text.delta";
    pub const REASONING_SUMMARY_TEXT_DONE: &str = "response.reasoning_summary_text.done";
    pub const REASONING_TEXT_DELTA: &str = "response.reasoning_text.delta";
    pub const REASONING_TEXT_DONE: &str = "response.reasoning_text.done";
    /// The stream failure frame that carries no `response` object.
    pub const ERROR: &str = "error";

    /// Every `response.*` type a component accepts, the host's closed list unchanged (record
    /// §6.2 rule 2). Anything else is a protocol error (R-Q9: strict).
    pub const ACCEPTED: [&str; 23] = [
        CREATED,
        IN_PROGRESS,
        QUEUED,
        COMPLETED,
        INCOMPLETE,
        FAILED,
        OUTPUT_ITEM_ADDED,
        OUTPUT_ITEM_DONE,
        CONTENT_PART_ADDED,
        CONTENT_PART_DONE,
        OUTPUT_TEXT_DELTA,
        OUTPUT_TEXT_DONE,
        OUTPUT_TEXT_ANNOTATION_ADDED,
        REFUSAL_DELTA,
        REFUSAL_DONE,
        FUNCTION_CALL_ARGUMENTS_DELTA,
        FUNCTION_CALL_ARGUMENTS_DONE,
        REASONING_SUMMARY_PART_ADDED,
        REASONING_SUMMARY_PART_DONE,
        REASONING_SUMMARY_TEXT_DELTA,
        REASONING_SUMMARY_TEXT_DONE,
        REASONING_TEXT_DELTA,
        REASONING_TEXT_DONE,
    ];
}

/// Output and input item types.
pub mod item {
    pub const MESSAGE: &str = "message";
    pub const FUNCTION_CALL: &str = "function_call";
    pub const FUNCTION_CALL_OUTPUT: &str = "function_call_output";
    pub const REASONING: &str = "reasoning";
}

/// Content part types.
pub mod part {
    pub const INPUT_TEXT: &str = "input_text";
    pub const INPUT_IMAGE: &str = "input_image";
    pub const INPUT_FILE: &str = "input_file";
    pub const INPUT_AUDIO: &str = "input_audio";
    pub const OUTPUT_TEXT: &str = "output_text";
    pub const REFUSAL: &str = "refusal";
    pub const SUMMARY_TEXT: &str = "summary_text";
    pub const REASONING_TEXT: &str = "reasoning_text";
}

/// `incomplete_details.reason` values that settle as a success (record §5, R-Q2 and its
/// 2026-10-01 extension).
pub mod incomplete_reason {
    pub const MAX_OUTPUT_TOKENS: &str = "max_output_tokens";
    pub const CONTENT_FILTER: &str = "content_filter";
}
