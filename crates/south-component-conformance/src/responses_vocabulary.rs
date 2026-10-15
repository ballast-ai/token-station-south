//! The `OpenAI` Responses wire vocabulary this crate's components share (`OpenAI` Responses upstream
//! record §9; ruled R-Q13, 2026-10-10).
//!
//! One module, here, because guests already depend on this crate. `south-north-codec` maps the
//! same wire in the opposite direction and may depend only on the kernel IR, so it keeps its own
//! literals; the test `openai_responses_vocabulary_v1` depends on both crates and asserts that the
//! two spellings agree.

/// The `extensions` keys an `OpenAI` component acts on.
///
/// The three the `OpenAI`-compatible reference already reads (record §4.1, R-Q15), and since R3
/// the four client fields the IR has no typed slot for (R3 record
/// `2026-10-12-responses-r3-codex-and-request-fields.md` §4.3, RQ3-Q3).
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
    /// The client's end-user identifier. The raw Chat name, which the Chat codec copies verbatim
    /// and the Responses codec writes after validating it (RQ3-Q4).
    pub const USER: &str = "user";
    /// The client's string-valued metadata object (RQ3-Q4).
    pub const METADATA: &str = "metadata";
    /// The client's prompt-cache routing key (RQ3-Q4).
    pub const PROMPT_CACHE_KEY: &str = "prompt_cache_key";
    /// The Responses truncation strategy, `auto` or `disabled`. Responses only (R3 §4.3).
    pub const TRUNCATION: &str = "truncation";
    /// The four client fields, in the order a component writes them.
    pub const CLIENT_FIELDS: [&str; 4] = [USER, METADATA, PROMPT_CACHE_KEY, TRUNCATION];
    /// The most bytes [`USER`] and [`PROMPT_CACHE_KEY`] may hold (R3 §4.3).
    pub const MAX_FIELD_BYTES: usize = 512;
    /// The most entries [`METADATA`] may hold.
    pub const MAX_METADATA_ENTRIES: usize = 16;
    /// The most characters a [`METADATA`] key may hold.
    pub const MAX_METADATA_KEY_CHARS: usize = 64;
    /// The most characters a [`METADATA`] value may hold.
    pub const MAX_METADATA_VALUE_CHARS: usize = 512;
    /// The [`TRUNCATION`] values.
    pub const TRUNCATION_VALUES: [&str; 2] = ["auto", "disabled"];
}

/// The `openai-codex` family's spellings (record §3.3, §4.3, §10.3; R3 record §2).
pub mod codex {
    /// The family name.
    pub const FAMILY: &str = "openai-codex";
    /// The path below the configured base URL (record §3.3: `{base_url}/codex/responses`).
    pub const PATH: &str = "codex/responses";
    /// The `instructions` sent when the request has none (record §4.3).
    pub const DEFAULT_INSTRUCTIONS: &str = "You are a helpful assistant.";
    /// The credential attribute the recipe exports, read from `ProviderConfig.declared`.
    pub const ACCOUNT_ATTRIBUTE: &str = "account_id";
    /// The request header that carries it (record §10.3).
    pub const ACCOUNT_HEADER: &str = "chatgpt-account-id";
}

/// The `bedrock-mantle` family's spelling (design record `2026-10-15-bedrock-mantle.md`): AWS
/// Bedrock's OpenAI-compatible `bedrock-mantle` host, which speaks this wire unchanged.
pub mod mantle {
    /// The family name.
    pub const FAMILY: &str = "bedrock-mantle";
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

/// `OpenAI` encrypted reasoning replay through the IR.
///
/// NC-1, design record `2026-10-11-responses-reasoning-replay-and-passthrough-suite.md` §2.1–§2.4.
/// The north codec keeps its own literals; `openai_responses_vocabulary_v1` asserts they agree.
pub mod replay {
    /// The model capability a target declares in `supported_parameters` to accept the items.
    pub const CAPABILITY: &str = "reasoning_replay.openai.v1";
    /// The message extension naming a turn's replay family (shared with the Claude carrier).
    pub const FAMILY_KEY: &str = "reasoning_replay_protocol_family";
    /// The message extension holding a turn's layout (shared with the Claude carrier).
    pub const LAYOUT_KEY: &str = "reasoning_replay_block_layout";
    /// The family value of an `OpenAI` encrypted reasoning turn.
    pub const OPENAI_FAMILY: &str = "openai-encrypted-reasoning";
    /// The family value of a Claude carrier turn.
    pub const CLAUDE_FAMILY: &str = "claude-signed-thinking";
    /// The message extension holding the turn's replayable reasoning items, in input order.
    pub const ITEMS_KEY: &str = "responses_opaque_reasoning_items";
    /// The layout entry kind naming an item, by its index in the items list.
    pub const ITEM_KIND: &str = "reasoning_item";
    /// The `include` value that asks the upstream to return encrypted reasoning.
    pub const INCLUDE_ENCRYPTED_CONTENT: &str = "reasoning.encrypted_content";
}
