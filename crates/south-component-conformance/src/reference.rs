//! The native reference implementation of the official OpenAI-compatible
//! provider component.
//!
//! Gate ② is built and frozen against this implementation (S2); the S3
//! runtime then proves a sandboxed build of the same logic gives identical
//! output, and S4 packages it as the first official component. Ported from
//! the community host's official adapter with two structural changes for the
//! v2 ABI: stream state lives in a per-stream parser instance instead of a
//! guest-global, and the SSE tail buffers **bytes** (S0 ruling D2) so a
//! socket split inside a UTF-8 sequence is buffered rather than corrupted.
//!
//! Everything here is protocol translation — routing, budget, billing and
//! the credential itself stay in the host.

use serde_json::{Map, Value, json};
use south_provider_api::{ComponentMetadataV1, PROVIDER_WORLD};
use token_station_protocol::{
    Auth, ChatRequest, ChatResponse, Choice, Content, ContentPart, ErrorCode, ErrorEnvelope,
    Extensions, FinishReason, HttpMethod, HttpRequestDescriptor, HttpResponseParts, Message,
    ProviderApi, ProviderConfig, ReasoningEffort, ReasoningMode, ResponseFormat, Role, SafeHeaders,
    StreamEvent, ToolCall, Usage,
};

use crate::component::{ComponentResultV1, ProviderComponentV1, StreamParserV1};
use crate::responses_vocabulary::extension;

/// The reference component. Stateless; each stream gets its own parser.
#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAiCompatibleReferenceV1;

// -- error plumbing ----------------------------------------------------------

fn internal(detail: impl std::fmt::Display) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string())
}

fn capability(detail: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Capability, 400, detail)
}

fn provider_protocol_error(message: &'static str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::ProviderProtocolError, 502, message)
}

// -- the github-copilot family -----------------------------------------------

/// The family whose bearer a credential recipe mints from a GitHub token (boundary §13.5 D7).
const GITHUB_COPILOT: &str = "github-copilot";

/// The identification headers GitHub's Copilot proxy expects on every chat request, as the
/// host's native arm sends them (server `upstream.rs`, `apply_github_copilot_auth`). The
/// `copilot-developer-cli` integration id is the one that unlocks the full model catalog; the
/// user-agent is the package's declared one (`user_agent`), since the ordinary channel refuses it.
const COPILOT_HEADERS: [(&str, &str); 6] = [
    ("copilot-integration-id", "copilot-developer-cli"),
    ("editor-plugin-version", "copilot-chat/0.43.0"),
    ("editor-version", "vscode/1.123.0"),
    ("openai-intent", "conversation-panel"),
    ("x-github-api-version", "2025-04-01"),
    ("x-vscode-user-agent-library-version", "electron-fetch"),
];

// -- the gemini-openai-compatible family ------------------------------------

/// The family for Gemini's OpenAI-compatible surface (`/v1beta/openai/chat/completions`): the same
/// Chat Completions body as `openai-compatible`, with the key presented twice, as
/// `Authorization: Bearer` and in `x-goog-api-key` (boundary §13.7 item 6).
const GEMINI_OPENAI_COMPATIBLE: &str = "gemini-openai-compatible";

// -- the glm-coding and azure-openai-dated families --------------------------

/// The Z.ai GLM Coding Plan family.
///
/// Design record `docs/design/2026-10-15-openai-compatible-t9.md` §3.1, transcribed from the
/// host's native `glm_coding` arm.
pub mod glm_coding {
    /// The family, as a host's provider rows name it.
    pub const FAMILY: &str = "glm-coding";
    /// The Chat Completions path below the operator's endpoint (an origin such as
    /// `https://api.z.ai`): native `build_upstream_url`'s `GlmCoding` arm, the path that is not
    /// `/v1/messages`.
    pub const CHAT_PATH: &str = "/api/coding/paas/v4/chat/completions";
    /// The user-agent the package declares for [`FAMILY`] (manifest `user_agent`): native
    /// `GLM_CODING_IMPERSONATION_UA`, an `OpenCode` release that Z.ai's Coding Plan whitelists.
    ///
    /// The component cannot send it: `user-agent` is reserved, and the host fills the controlled
    /// slot from the manifest. Here so a test can pin the manifest to the host's value.
    pub const USER_AGENT: &str = "opencode/1.15.6";
}

// -- the kimi-code family ----------------------------------------------------

/// The Kimi Code (Moonshot AI) coding subscription family.
///
/// Design record `docs/design/2026-10-16-kimi-code-family.md`, transcribed from the host's native
/// `kimi_code` arm. The wire is the `openai-compatible` Chat Completions wire; the family exists so
/// the host knows a 429 on such a row is the subscription's quota (`subscription_quota_families`,
/// native `is_coding_plan`), which configuration of `openai-compatible` cannot say.
pub mod kimi_code {
    /// The family, as a host's provider rows name it.
    pub const FAMILY: &str = "kimi-code";
    /// The Chat Completions path appended to the operator's endpoint.
    ///
    /// The endpoint is an origin such as `https://api.moonshot.cn` or carries a path: native
    /// `build_upstream_url` has no `KimiCode` arm, so its default arm appends the canonical path
    /// `/v1/chat/completions` to the trimmed endpoint.
    pub const CHAT_PATH: &str = "/v1/chat/completions";
    /// The endpoint native's type preset fills in for a new row, which an operator may edit.
    ///
    /// Not a manifest endpoint template: the family takes whatever endpoint the row names, as
    /// native does.
    pub const NATIVE_PRESET_ENDPOINT: &str = "https://api.moonshot.cn";
}

// -- the bailian-openai-compatible family ------------------------------------

/// Alibaba Bailian's (`DashScope`'s) OpenAI-compatible Chat surface, whose thinking models cap the
/// reasoning apart from the answer.
///
/// Design record `docs/design/2026-10-21-bailian-thinking-cap.md`. On these models `max_tokens`
/// bounds only the answer, while the billed `completion_tokens` include the reasoning. When a
/// model declares [`bailian_openai_compatible::THINKING_BUDGET_WORD`] and thinks, the family splits
/// the authorised cap N into `thinking_budget = B` and `max_tokens = N - B - CAP_OVERHEAD`, so the
/// measured worst case is N (`split_bailian_cap`); the manifest declares the split as
/// `request_facts.reasoning_cap`.
pub mod bailian_openai_compatible {
    /// The family, as a host's provider rows name it.
    pub const FAMILY: &str = "bailian-openai-compatible";
    /// The Chat Completions path appended to the operator's endpoint, the API root
    /// (`https://dashscope.aliyuncs.com/compatible-mode/v1` or a regional twin).
    pub const CHAT_PATH: &str = "/chat/completions";
    /// The model parameter word that opts a model into the split, and the body key of the
    /// reasoning cap (also the client's field the family consumes).
    pub const THINKING_BUDGET_WORD: &str = "thinking_budget";
    /// The client field that switches thinking, forwarded verbatim (`client_passthrough`).
    pub const ENABLE_THINKING: &str = "enable_thinking";
    /// Tokens the upstream emits beyond `thinking_budget + max_tokens` at most.
    ///
    /// Measured on 2026-10-21: an answer cut by `max_tokens` after thinking is `max_tokens + 3`
    /// (record §2). The manifest declares the same value as `request_facts.cap_overhead`.
    pub const CAP_OVERHEAD: u32 = 3;
    /// The smallest authorised cap that holds an answer cap of 1, a reasoning cap of 1 and the
    /// overhead.
    pub const MIN_SPLIT_CAP: u32 = CAP_OVERHEAD + 2;
}

/// Azure `OpenAI`'s dated, deployment-addressed surface.
///
/// Design record `docs/design/2026-10-15-openai-compatible-t9.md` §3.2, transcribed from the
/// host's native `azure_openai` arm for any effective API version other than `v1` (which is
/// `azure-openai-v1`).
pub mod azure_openai_dated {
    /// The family, as a host's provider rows name it.
    pub const FAMILY: &str = "azure-openai-dated";
    /// The configuration key that carries the API version (`ProviderConfig.declared`).
    pub const API_VERSION_KEY: &str = "api_version";
    /// The API version when the operator sets none: native `AZURE_OPENAI_DEFAULT_API_VERSION`.
    /// The manifest declares the same value as the key's `default`.
    pub const DEFAULT_API_VERSION: &str = "2024-10-21";
}

/// Client fields the IR does not type that native forwards on these families (design record
/// `docs/design/2026-10-15-openai-compatible-t9.md` §10, T9-Q9 (a)).
///
/// The host's native arm for every type these families serve (`glm`, `glm_coding`, Bailian,
/// `byteplus`, `kimi_code`, `deepinfra`, `github_copilot`, `azure_openai`, `azure_foundry`) sends the
/// client's Chat body itself, so a client's `thinking` (Z.ai) or `enable_thinking` (Bailian) reaches
/// the upstream unchanged. The Chat north codec keeps such keys verbatim in
/// `ChatRequest.extensions`; the package writes them back, only when the client sent them, as
/// `provider-openai-responses` writes its client fields (R3). Native builds the Gemini body through
/// the IR instead, so `gemini-openai-compatible` forwards nothing.
pub mod client_passthrough {
    /// The forwarded keys, each written under its own name.
    pub const FIELDS: [&str; 2] = ["thinking", "enable_thinking"];
    /// The families that forward them.
    ///
    /// `kimi-code` joined with the family itself (Kimi Code record §3): native sends a `kimi_code`
    /// row's client body too. `bailian-openai-compatible` joined with the family itself (the
    /// Bailian thinking cap record §5): Bailian rows were `openai-compatible` rows before.
    pub const FAMILIES: [&str; 7] = [
        "openai-compatible",
        "azure-openai-v1",
        "github-copilot",
        super::azure_openai_dated::FAMILY,
        super::glm_coding::FAMILY,
        super::kimi_code::FAMILY,
        super::bailian_openai_compatible::FAMILY,
    ];
}

/// Writes the client's [`client_passthrough::FIELDS`] into `body` verbatim (`null` included, as
/// native sends it) for a family in [`client_passthrough::FAMILIES`]. A key the package already
/// wrote is never overwritten: the package's own keys are the wire contract, and none of them
/// shares a name with a forwarded field.
fn forward_client_fields(request: &ChatRequest, config: &ProviderConfig, body: &mut Value) {
    if !client_passthrough::FAMILIES.contains(&config.provider.as_str()) {
        return;
    }
    let Value::Object(map) = body else {
        return;
    };
    for field in client_passthrough::FIELDS {
        if let Some(value) = request.extensions.get(field)
            && !map.contains_key(field)
        {
            map.insert(field.to_owned(), value.clone());
        }
    }
}

/// Renders the IR's `reasoning.mode` as Z.ai's `thinking` switch on [`glm_coding::FAMILY`] only
/// (design record `docs/design/2026-10-16-glm-coding-messages.md` §5 O1 and §6 Q-GM2 (a), lv
/// 2026-10-16; option A+ of `docs/design/2026-10-15-openai-compatible-t9.md` §10).
///
/// A Messages client of a `glm-coding` row reaches the Chat endpoint through the IR, where the
/// Anthropic north codec has consumed its `thinking` into `reasoning.mode`. `Enabled` and
/// `Disabled` become `thinking: {"type": "enabled" | "disabled"}`; `Adaptive` has no GLM spelling
/// and, like an absent mode, writes nothing, so the upstream default decides (Q-GM2 (a)).
/// `reasoning.budget_tokens` has no GLM carrier and is never written. A `thinking` already in the
/// body (a Chat client's own extension, forwarded by [`forward_client_fields`]) wins. Every other
/// family is unchanged.
fn render_glm_thinking(request: &ChatRequest, config: &ProviderConfig, body: &mut Value) {
    if config.provider != glm_coding::FAMILY {
        return;
    }
    let kind = match request.reasoning.mode {
        Some(ReasoningMode::Enabled) => "enabled",
        Some(ReasoningMode::Disabled) => "disabled",
        Some(ReasoningMode::Adaptive) | None => return,
    };
    if let Value::Object(map) = body
        && !map.contains_key("thinking")
    {
        map.insert("thinking".to_owned(), json!({"type": kind}));
    }
}

/// Splits the authorised cap on [`bailian_openai_compatible::FAMILY`] (the Bailian thinking cap
/// record §5); every other family is unchanged.
///
/// Applies only to a model that declares [`bailian_openai_compatible::THINKING_BUDGET_WORD`];
/// another model keeps the `openai-compatible` request. Thinking is on unless the client's own
/// `enable_thinking` (forwarded verbatim) is `false`, or the client sent none and the IR reasoning
/// mode is `disabled`: then the body says `enable_thinking: false`, because these models think by
/// default and would otherwise reason with no budget (BT-Q1). The client's switch wins over the IR
/// mode, so the split always follows what the upstream receives.
///
/// With thinking on and a cap N, the reasoning cap B is the client's `thinking_budget`, else the
/// IR's `reasoning.budget_tokens`, else `(N - 3) / 2`, clamped to `[1, N - 4]`; the body carries
/// `thinking_budget = B` and `max_tokens = N - B - 3`. N below
/// [`bailian_openai_compatible::MIN_SPLIT_CAP`] is a capability error, as is a client budget that is
/// not a non-negative integer (`null` counts as absent). Without a cap nothing is split, and the
/// client's budget is dropped, as on every other family.
fn split_bailian_cap(
    request: &ChatRequest,
    config: &ProviderConfig,
    body: &mut Value,
) -> ComponentResultV1<()> {
    use bailian_openai_compatible::{
        CAP_OVERHEAD, ENABLE_THINKING, FAMILY, MIN_SPLIT_CAP, THINKING_BUDGET_WORD,
    };
    if config.provider != FAMILY
        || !config.models.iter().any(|model| {
            model.model == request.model
                && model.supported_parameters.contains(THINKING_BUDGET_WORD)
        })
    {
        return Ok(());
    }
    let Value::Object(map) = body else {
        return Ok(());
    };
    let thinking = match map.get(ENABLE_THINKING) {
        Some(Value::Bool(on)) => *on,
        _ if request.reasoning.mode == Some(ReasoningMode::Disabled) => {
            map.insert(ENABLE_THINKING.to_owned(), Value::Bool(false));
            false
        }
        _ => true,
    };
    let Some(cap) = request.sampling.max_output_tokens.filter(|_| thinking) else {
        return Ok(());
    };
    if cap < MIN_SPLIT_CAP {
        return Err(capability(
            "this model reasons by default and its upstream caps the reasoning apart from the \
             answer; an output cap below 5 leaves no room for both",
        ));
    }
    let client = match request.extensions.get(THINKING_BUDGET_WORD) {
        None | Some(Value::Null) => None,
        Some(budget) => Some(
            budget
                .as_u64()
                .ok_or_else(|| capability("thinking_budget must be a non-negative integer"))?,
        ),
    };
    let room = cap - CAP_OVERHEAD - 1;
    let wanted = client
        .or_else(|| request.reasoning.budget_tokens.map(u64::from))
        .unwrap_or_else(|| u64::from((cap - CAP_OVERHEAD) / 2));
    // Clamped into `1..=room`, so it fits a `u32`.
    let budget = u32::try_from(wanted.clamp(1, u64::from(room))).unwrap_or(room);
    map.insert(THINKING_BUDGET_WORD.to_owned(), json!(budget));
    map.insert("max_tokens".to_owned(), json!(cap - CAP_OVERHEAD - budget));
    Ok(())
}

/// The output cap key of the two Azure `OpenAI` families, `azure-openai-v1` and
/// [`azure_openai_dated`].
///
/// Design record `docs/design/2026-10-15-openai-compatible-t9.md` §4 D6, transcribed from the
/// host's native `needs_max_tokens_rewrite`: an `azure_openai` (or `azure_foundry`) row sends the
/// cap as `max_completion_tokens` unless the row's `max_tokens_field` column says `max_tokens`, or,
/// with the column unset, the row clears `accepts_max_completion_tokens` (default on). The host
/// writes that per-row choice into this key when it migrates the row; absent, the native default
/// applies. Every other family of the package keeps `max_tokens`, as native does for the types it
/// serves.
pub mod azure_openai_cap_field {
    /// The families that read [`KEY`].
    pub const FAMILIES: [&str; 2] = ["azure-openai-v1", super::azure_openai_dated::FAMILY];
    /// The configuration key that carries the cap's wire name (`ProviderConfig.declared`), named
    /// after the native row column.
    pub const KEY: &str = "max_tokens_field";
    /// The modern wire name, native's default for an Azure `OpenAI` row.
    pub const MAX_COMPLETION_TOKENS: &str = "max_completion_tokens";
    /// The legacy wire name, the row's opt-out.
    pub const MAX_TOKENS: &str = "max_tokens";
    /// The wire name when the operator sets none. The manifest declares the same value as the
    /// key's `default`.
    pub const DEFAULT: &str = MAX_COMPLETION_TOKENS;
}

/// The body key the output cap is written under for `config`'s family (record §4 D6). A value
/// other than the two wire names is refused, as an undated `api_version` is, so a host that skipped
/// the manifest's `enum` check cannot put another key in the body.
fn cap_field(config: &ProviderConfig) -> ComponentResultV1<&'static str> {
    use azure_openai_cap_field::{DEFAULT, FAMILIES, KEY, MAX_COMPLETION_TOKENS, MAX_TOKENS};
    if !FAMILIES.contains(&config.provider.as_str()) {
        return Ok(MAX_TOKENS);
    }
    match config.declared.get(KEY).unwrap_or(DEFAULT) {
        MAX_COMPLETION_TOKENS => Ok(MAX_COMPLETION_TOKENS),
        MAX_TOKENS => Ok(MAX_TOKENS),
        _ => Err(internal(format!(
            "the `{KEY}` configuration value is neither `{MAX_COMPLETION_TOKENS}` nor `{MAX_TOKENS}`"
        ))),
    }
}

/// Whether `value` is a dated API version, `YYYY-MM-DD` with an optional `-preview`: the
/// `api_version_date` syntax the manifest declares for [`azure_openai_dated::API_VERSION_KEY`].
/// A host validates the value before it reaches the component; this keeps a host that skips the
/// check from putting anything else in the query. `speech-openai-compatible`'s
/// `azure-openai-dated` family applies the same check (`reference_openai_compatible_speech`).
pub(crate) fn is_dated_api_version(value: &str) -> bool {
    let date = value.strip_suffix("-preview").unwrap_or(value);
    date.len() == 10
        && date.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// The dated Azure `OpenAI` URL: `{base}/openai/deployments/{deployment}/chat/completions
/// ?api-version={version}`, as native `build_upstream_url` builds it, with the request's model
/// (the host's upstream model, the deployment name) as the deployment segment.
fn azure_dated_url(request: &ChatRequest, config: &ProviderConfig) -> ComponentResultV1<String> {
    let version = config
        .declared
        .get(azure_openai_dated::API_VERSION_KEY)
        .unwrap_or(azure_openai_dated::DEFAULT_API_VERSION);
    if !is_dated_api_version(version) {
        return Err(internal(format!(
            "the `{}` configuration value is not a dated API version",
            azure_openai_dated::API_VERSION_KEY
        )));
    }
    if request.model.is_empty() {
        return Err(capability(
            "Azure OpenAI addresses the deployment in the URL path, so a request without a model \
             has no deployment to send to",
        ));
    }
    Ok(format!(
        "{}/openai/deployments/{}/chat/completions?api-version={version}",
        config.base_url.as_str().trim_end_matches('/'),
        crate::url_segment::encode(&request.model)
    ))
}

// -- translation -------------------------------------------------------------

fn finish_reason(raw: Option<&str>) -> Option<FinishReason> {
    match raw? {
        "stop" => Some(FinishReason::Stop),
        "length" => Some(FinishReason::Length),
        "tool_calls" => Some(FinishReason::ToolCalls),
        "content_filter" => Some(FinishReason::ContentFilter),
        // Unknown reasons survive verbatim instead of vanishing.
        other => Some(FinishReason::Other(other.to_owned())),
    }
}

fn index_of(value: &Value) -> u32 {
    u32::try_from(value.as_u64().unwrap_or(0)).unwrap_or(0)
}

fn anthropic_reasoning_history(message: &Message) -> Option<String> {
    let blocks = message.extensions.get("anthropic_thinking_blocks")?.as_array()?;
    let thinking = blocks
        .iter()
        .filter_map(|entry| entry.pointer("/block/thinking").and_then(Value::as_str))
        .collect::<String>();
    (!thinking.is_empty()).then_some(thinking)
}

/// Whether this model wants the provider-private `reasoning_content` field
/// replayed on assistant turns.
///
/// R2 (host-parity slice). The field is not universally accepted: legacy
/// reasoner-style upstreams **reject** a request carrying it, which is why the
/// enterprise host gates it per model. Emitting it unconditionally whenever the
/// IR happens to hold a thinking block sends a field that is known to be
/// refused.
///
/// The gate is `ProviderConfig.models[].supported_parameters`, which is inside
/// the S0 §6 policy fence — a component may read it, so this needs no new IR
/// field and no `extensions` smuggling (S0 ruling D5).
///
/// Undeclared models keep the previous DeepSeek-prefix heuristic: a host that
/// ships no catalog (the community host's usual shape) sees no behaviour
/// change, while a host that declares one gets exact control. Declared wins:
/// a model listed with no `reasoning_content` parameter is a deliberate "no".
fn wants_reasoning_content(request: &ChatRequest, config: &ProviderConfig) -> bool {
    config.models.iter().find(|model| model.model == request.model).map_or_else(
        || request.model.get(..8).is_some_and(|prefix| prefix.eq_ignore_ascii_case("deepseek")),
        |model| model.supported_parameters.contains("reasoning_content"),
    )
}

/// Content-part `type`s this renderer forwards verbatim when the IR does not
/// model them. They are Chat Completions' own vocabulary, so the upstream can
/// read them; the IR merely has no typed field for them yet.
const OPENAI_NATIVE_UNKNOWN_PART_TYPES: &[&str] = &["input_audio", "file"];

/// The refusal for a part this wire cannot spell (renderer refusal, 0.15.0).
///
/// An unmodelled part that is not Chat Completions vocabulary — an Anthropic
/// server-tool block, a `document`, a `search_result`, a part invented by
/// another wire — used to be pushed into the `content` array as-is. The
/// upstream then either rejected the whole request with its own 400 or, worse,
/// read the block as nothing and answered as if the content had never been
/// there. The IR's contract is "never drop"; this renderer's half of it is
/// "never smuggle": a block it cannot express is refused by name so the host
/// can route the request to a provider that speaks that wire. This is the
/// opposite choice from R3 on purpose — `redacted_thinking` is side metadata
/// whose loss leaves the answer intact, whereas these blocks *are* the content
/// the answer depends on.
fn unmappable_part(wire: &str, value: &Value) -> ErrorEnvelope {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("<untyped>");
    capability(format!(
        "content block `{kind}` has no {wire} rendering; route the request to a provider \
         that speaks its wire"
    ))
}

/// Renders one canonical message into the `OpenAI` Chat Completions shape.
///
/// Fails only for a content part this wire cannot spell; see
/// [`unmappable_part`]. Everything else renders.
fn message_to_openai_for_model(
    message: &Message,
    requires_reasoning_content: bool,
) -> ComponentResultV1<Value> {
    let mut out = Map::new();
    out.insert("role".to_owned(), json!(message.role));
    // R5 (host-parity slice): the key is always written, `null` when the
    // canonical content is absent. Omitting it is a third spelling of "empty"
    // that some compatible providers treat differently from `null`.
    if message.content.is_none() {
        out.insert("content".to_owned(), Value::Null);
    }
    // This wire has no inline prompt-cache marker (KC1 record §4.3): a system prompt or tool
    // result that is a part list only for its markers renders as the string it would otherwise be.
    let content = if matches!(message.role, Role::System | Role::Tool) {
        crate::typed_request::without_markers(message.content.as_ref())
    } else {
        message.content.as_ref().map(std::borrow::Cow::Borrowed)
    };
    if let Some(content) = content.as_deref() {
        match content {
            Content::Text(text) => {
                out.insert("content".to_owned(), json!(text));
            }
            Content::Parts(parts) => {
                let mut reasoning = String::new();
                let mut multimodal = Vec::new();
                for part in parts {
                    match part {
                        ContentPart::Text { text, .. } => {
                            multimodal.push(json!({"type": "text", "text": text}));
                        }
                        ContentPart::ImageUrl { image_url, .. } => {
                            multimodal.push(json!({
                                "type": "image_url",
                                "image_url": image_url
                            }));
                        }
                        ContentPart::Thinking { thinking, .. } => {
                            reasoning.push_str(thinking);
                        }
                        // R3 (host-parity slice): an encrypted reasoning block
                        // is dropped, not refused. Refusing turned a request
                        // the enterprise host serves today into a 400, and a
                        // migration that only swaps implementations must not
                        // change which requests succeed. The block carries no
                        // OpenAI-compatible spelling, so dropping it is the
                        // same lossy-but-working choice the host already makes.
                        ContentPart::RedactedThinking { .. } => {}
                        ContentPart::Unknown(value) => {
                            let native =
                                value.get("type").and_then(Value::as_str).is_some_and(|kind| {
                                    OPENAI_NATIVE_UNKNOWN_PART_TYPES.contains(&kind)
                                });
                            if !native {
                                return Err(unmappable_part("OpenAI Chat Completions", value));
                            }
                            // A native part keeps its shape; a prompt-cache marker on it is
                            // dropped like any other on this wire (KC1 record §4.3).
                            let mut value = value.clone();
                            if let Some(object) = value.as_object_mut() {
                                object.remove("cache_control");
                            }
                            multimodal.push(value);
                        }
                    }
                }
                // R1 + R5 (host-parity slice): `Content::Parts` always renders
                // as an array and `Content::Text` always as a string, so the
                // Text/Parts distinction the IR draws survives the boundary.
                //
                // The previous shape collapsed a text-only parts array to a
                // bare string and an all-thinking parts array to `null`. Both
                // are lossy: the IR states the difference and this is the
                // adapter that must not lose it. The enterprise host has
                // rendered it this way in production all along; this adopts
                // its behaviour rather than inventing one.
                //
                // Consequence worth knowing: an assistant turn whose content
                // is nothing but thinking blocks now renders `content: []`.
                // OpenAI documents assistant `content` as string-or-null, so
                // hosts that previously saw `null` here see an empty array
                // instead. See the design record's migration note.
                out.insert("content".to_owned(), Value::Array(multimodal));
                // R2: same gate as the tool-call placeholder below — a model
                // that does not declare the field never receives it, however
                // much thinking the IR carries.
                if requires_reasoning_content && !reasoning.is_empty() {
                    out.insert("reasoning_content".to_owned(), json!(reasoning));
                }
            }
        }
    }
    if !message.tool_calls.is_empty() {
        // OpenAI permits assistant tool-call messages with `content: null`;
        // some compatible providers (DeepSeek) reject the same message when
        // the key is absent entirely. Canonical `None` means no assistant
        // text here, so writing null is lossless.
        if message.role == Role::Assistant && !out.contains_key("content") {
            out.insert("content".to_owned(), Value::Null);
        }
        if message.role == Role::Assistant
            && requires_reasoning_content
            && !out.contains_key("reasoning_content")
        {
            // DeepSeek thinking-mode tool continuations require this private
            // field on the historical assistant tool call. Restore genuine
            // Anthropic thinking when it survived the inbound translation; an
            // empty placeholder is the documented no-thinking history shape.
            out.insert(
                "reasoning_content".to_owned(),
                json!(anthropic_reasoning_history(message).unwrap_or_default()),
            );
        }
        let calls: Vec<Value> = message
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": {"name": call.name, "arguments": call.arguments},
                })
            })
            .collect();
        out.insert("tool_calls".to_owned(), Value::Array(calls));
    }
    if let Some(id) = &message.tool_call_id {
        out.insert("tool_call_id".to_owned(), json!(id));
    }
    if let Some(name) = &message.name {
        out.insert("name".to_owned(), json!(name));
    }
    Ok(Value::Object(out))
}

/// Fails through message rendering, for a part this wire cannot spell, and on a cap field value
/// that is not a wire name (record §4 D6).
fn body_of(request: &ChatRequest, config: &ProviderConfig) -> ComponentResultV1<Value> {
    let cap_key = cap_field(config)?;
    let mut body = Map::new();
    body.insert("model".to_owned(), json!(request.model));
    let requires_reasoning_content = wants_reasoning_content(request, config);
    let messages = request
        .messages
        .iter()
        .map(|message| message_to_openai_for_model(message, requires_reasoning_content))
        .collect::<ComponentResultV1<Vec<Value>>>()?;
    body.insert("messages".to_owned(), Value::Array(messages));
    if request.stream {
        body.insert("stream".to_owned(), json!(true));
        body.insert("stream_options".to_owned(), json!({"include_usage": true}));
    }
    if let Some(temperature) = request.sampling.temperature {
        body.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(top_p) = request.sampling.top_p {
        body.insert("top_p".to_owned(), json!(top_p));
    }
    if let Some(max_tokens) = request.sampling.max_output_tokens {
        body.insert(cap_key.to_owned(), json!(max_tokens));
    }
    if !request.sampling.stop.is_empty() {
        body.insert("stop".to_owned(), json!(request.sampling.stop));
    }
    if let Some(tool_choice) = &request.tool_choice {
        body.insert("tool_choice".to_owned(), json!(tool_choice));
    }
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                let mut function = Map::new();
                function.insert("name".to_owned(), json!(tool.name));
                function.insert("description".to_owned(), json!(tool.description));
                function.insert("parameters".to_owned(), tool.parameters.clone());
                if let Some(strict) = crate::typed_request::tool_strict(request, tool) {
                    function.insert("strict".to_owned(), json!(strict));
                }
                json!({"type": "function", "function": function})
            })
            .collect();
        body.insert("tools".to_owned(), Value::Array(tools));
        // Typed since protocol 0.6.0 (R-Q15); the wire only accepts it alongside `tools`.
        if let Some(parallel) = request.parallel_tool_calls {
            body.insert("parallel_tool_calls".to_owned(), json!(parallel));
        }
    }
    if let Some(format) = &request.response_format {
        let format = match format {
            ResponseFormat::Text => json!({"type": "text"}),
            ResponseFormat::JsonObject => json!({"type": "json_object"}),
            ResponseFormat::JsonSchema { json_schema } => {
                json!({"type": "json_schema", "json_schema": json_schema})
            }
        };
        body.insert("response_format".to_owned(), format);
    }
    Ok(Value::Object(body))
}

/// Whether `reasoning_effort` may be sent for the chosen model. Native
/// `OpenAI` clients keep the optimistic behavior when a model declares no
/// parameter set; a translated Anthropic adaptive/enabled thinking
/// preference requires an explicit declaration before receiving the field.
fn reasoning_effort_allowed(request: &ChatRequest, config: &ProviderConfig) -> bool {
    let requires_explicit_capability = crate::typed_request::thinking_requested(request);

    config.models.iter().find(|capability| capability.model == request.model).map_or(
        !requires_explicit_capability,
        |capability| {
            capability.supported_parameters.contains(extension::REASONING_EFFORT)
                || (!requires_explicit_capability && capability.supported_parameters.is_empty())
        },
    )
}

/// A required usage count: present, a non-negative integer.
fn required_count(raw: &Value, key: &str, missing: &'static str) -> ComponentResultV1<u64> {
    raw.get(key).and_then(Value::as_u64).ok_or_else(|| provider_protocol_error(missing))
}

/// An optional usage count: absent or `null` is zero; anything else must be a
/// non-negative integer.
fn optional_count(raw: Option<&Value>, key: &str, invalid: &'static str) -> ComponentResultV1<u64> {
    match raw.and_then(|raw| raw.get(key)) {
        None | Some(Value::Null) => Ok(0),
        Some(value) => value.as_u64().ok_or_else(|| provider_protocol_error(invalid)),
    }
}

/// A `*_tokens_details` object: absent or `null`, or an object.
fn usage_details<'a>(
    raw: &'a Value,
    key: &str,
    invalid: &'static str,
) -> ComponentResultV1<Option<&'a Value>> {
    match raw.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(details @ Value::Object(_)) => Ok(Some(details)),
        Some(_) => Err(provider_protocol_error(invalid)),
    }
}

fn checked_sum(left: u64, right: u64) -> ComponentResultV1<u64> {
    left.checked_add(right)
        .ok_or_else(|| provider_protocol_error("the upstream usage counts overflow"))
}

/// Normalizes the wire usage object in the `OpenAI` dialect's native cache
/// convention (S0 ruling D1): `cache_read_tokens` is the cached **subset** of
/// `input_tokens`, never a disjoint bucket.
///
/// Usage is funds evidence (provider-adapter.wit, `parse-response`): a missing
/// count, a total that does not add up or a subset larger than its whole is a
/// protocol error, never a zero (B1, host-zero-vendor-boundary §6.2 item 1).
///
/// - `prompt_tokens`, `completion_tokens` and `total_tokens` are required.
/// - Multi-agent orchestration (Sakana Fugu) reports sub-model consumption in
///   `*_tokens_details.orchestration_*` **on top of** the top-level counts, and
///   its `total_tokens` is `prompt + completion + orchestration_input`. Those
///   tokens are billable and fold into the IR buckets.
/// - The cached subset is `prompt_tokens_details.cached_tokens` or, for Kimi and
///   `DeepSeek`, the top-level `prompt_cache_hit_tokens`.
/// - The cache-write subset is `prompt_tokens_details.cache_write_tokens`
///   (`OpenAI`) or `prompt_tokens_details.cache_creation_input_tokens` (Bailian);
///   when both are present the Bailian key wins, matching the host's evidence.
/// - The presence of `prompt_tokens_details.cache_creation_input_tokens`, even
///   at 0, marks Bailian's explicit-cache mode: every hit of that response,
///   orchestration's cached subset included, is an explicit-cache read and is
///   reported as `explicit_cache_read_tokens` (a subset of `cache_read_tokens`,
///   which stays the total). Without the key every hit is implicit and the
///   explicit part is 0. The rule is keyed on the wire, not on the family,
///   exactly as the host's native evidence reads it (record
///   `docs/design/2026-10-17-explicit-cache-reads.md`).
fn usage_of(raw: &Value) -> ComponentResultV1<Usage> {
    if !raw.is_object() {
        return Err(provider_protocol_error("a chat completion must carry a usage object"));
    }
    let prompt = required_count(
        raw,
        "prompt_tokens",
        "the upstream usage lacks a valid prompt_tokens count",
    )?;
    let completion = required_count(
        raw,
        "completion_tokens",
        "the upstream usage lacks a valid completion_tokens count",
    )?;
    let total =
        required_count(raw, "total_tokens", "the upstream usage lacks a valid total_tokens count")?;
    let prompt_details = usage_details(
        raw,
        "prompt_tokens_details",
        "the upstream usage has an invalid prompt_tokens_details field",
    )?;
    let completion_details = usage_details(
        raw,
        "completion_tokens_details",
        "the upstream usage has an invalid completion_tokens_details field",
    )?;
    let invalid_count = "the upstream usage has an invalid token count";
    let orchestration_input =
        optional_count(prompt_details, "orchestration_input_tokens", invalid_count)?;
    let orchestration_cached =
        optional_count(prompt_details, "orchestration_input_cached_tokens", invalid_count)?;
    if orchestration_cached > orchestration_input {
        return Err(provider_protocol_error(
            "the upstream orchestration cached count exceeds its input count",
        ));
    }
    let orchestration_output =
        optional_count(completion_details, "orchestration_output_tokens", invalid_count)?;
    let plain_total = checked_sum(prompt, completion)?;
    if total != plain_total && total != checked_sum(plain_total, orchestration_input)? {
        return Err(provider_protocol_error(
            "the upstream total_tokens does not equal prompt_tokens plus completion_tokens",
        ));
    }
    let input_tokens = checked_sum(prompt, orchestration_input)?;
    let output_tokens = checked_sum(completion, orchestration_output)?;
    let cached = match prompt_details.and_then(|details| details.get("cached_tokens")) {
        Some(_) => optional_count(prompt_details, "cached_tokens", invalid_count)?,
        None => optional_count(Some(raw), "prompt_cache_hit_tokens", invalid_count)?,
    };
    let cache_read_tokens = checked_sum(cached, orchestration_cached)?;
    let explicit_cache =
        prompt_details.and_then(|details| details.get("cache_creation_input_tokens")).is_some();
    let cache_write_tokens = if explicit_cache {
        optional_count(prompt_details, "cache_creation_input_tokens", invalid_count)?
    } else {
        optional_count(prompt_details, "cache_write_tokens", invalid_count)?
    };
    if checked_sum(cache_read_tokens, cache_write_tokens)? > input_tokens {
        return Err(provider_protocol_error(
            "the upstream cached and cache-write counts exceed the prompt count",
        ));
    }
    let reasoning_tokens = optional_count(completion_details, "reasoning_tokens", invalid_count)?;
    if reasoning_tokens > output_tokens {
        return Err(provider_protocol_error(
            "the upstream reasoning_tokens exceeds the completion count",
        ));
    }
    Ok(Usage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        explicit_cache_read_tokens: if explicit_cache { cache_read_tokens } else { 0 },
        cache_write_tokens,
        reasoning_tokens,
        ..Usage::default()
    })
}

/// Tracks finish arrival separately from its canonical mapping, so a
/// downstream consumer never sees its stream close before the final
/// accounting event.
#[derive(Default)]
struct PendingFinish {
    seen: bool,
    reason: Option<FinishReason>,
    done_emitted: bool,
}

impl PendingFinish {
    fn record(&mut self, raw: &str) {
        self.seen = true;
        self.reason = finish_reason(Some(raw));
        self.done_emitted = false;
    }

    const fn take_finish(&mut self) -> Option<StreamEvent> {
        if !self.seen {
            return None;
        }
        self.seen = false;
        Some(StreamEvent::Finish {
            finish_reason: self.reason.take(),
            // The openai chat wire has no stop-sequence report slot.
            stop_sequence: None,
        })
    }

    fn finish_marker(&mut self) -> Vec<StreamEvent> {
        if self.done_emitted {
            self.done_emitted = false;
            return Vec::new();
        }
        let mut events = self.take_finish().into_iter().collect::<Vec<_>>();
        events.push(StreamEvent::Done { finish_reason: None, stop_sequence: None });
        self.seen = false;
        self.reason = None;
        self.done_emitted = true;
        events
    }

    fn flush_pending_finish(&mut self) -> Vec<StreamEvent> {
        if !self.seen {
            return Vec::new();
        }
        self.finish_marker()
    }
}

/// One complete SSE frame's worth of events. A finish reason is held until
/// cumulative usage arrives (or the provider emits `[DONE]`).
fn events_of_frame(
    payload: &str,
    pending_finish: &mut PendingFinish,
) -> ComponentResultV1<Vec<StreamEvent>> {
    let raw: Value = serde_json::from_str(payload)
        .map_err(|_| provider_protocol_error("the upstream emitted invalid SSE JSON"))?;
    if raw.get("error").is_some_and(|error| !error.is_null()) {
        return Err(provider_protocol_error(
            "the upstream embedded an error in a successful SSE response",
        ));
    }
    let mut events = Vec::new();

    let usage = match raw.get("usage") {
        None | Some(Value::Null) => None,
        Some(usage @ Value::Object(_)) => Some(usage),
        Some(_) => {
            return Err(provider_protocol_error(
                "the upstream SSE event has an invalid usage field",
            ));
        }
    };
    let choices = match raw.get("choices") {
        Some(Value::Array(choices)) => Some(choices),
        None if usage.is_some() => None,
        _ => {
            return Err(provider_protocol_error(
                "the upstream SSE event has an invalid choices field",
            ));
        }
    };
    if choices.is_some_and(Vec::is_empty) && usage.is_none() {
        // S5 (host-parity slice): an event with no choices and no usage is
        // ignored, not refused.
        //
        // Refusing it terminates the whole stream, and this exact shape is not
        // exotic: Azure OpenAI opens a stream with a `prompt_filter_results`
        // frame that carries an empty `choices` array — and `azure-openai-v1`
        // is a dialect this component's own manifest declares. Ordinary
        // keepalive frames look the same. The enterprise host has always
        // ignored them.
        return Ok(events);
    }
    for choice in choices.into_iter().flatten() {
        let index = index_of(&choice["index"]);
        let delta = &choice["delta"];

        // S4 (host-parity slice): the ecosystem spells the reasoning delta two
        // ways — `reasoning_content` (DeepSeek) and a bare `reasoning`
        // (Qwen, OpenWebUI and others). Reading only the first silently drops
        // every thinking token from the second family: no error, no event,
        // the client simply never sees the reasoning. Both are read here.
        //
        // S1 (host-parity slice): a zero-length delta carries nothing. OpenAI's
        // opening frame is `delta:{role:"assistant",content:""}` by convention,
        // and emitting an event for it makes a northbound renderer open an
        // empty content block, shifting every block index after it. Suppressed
        // for text and reasoning alike.
        let thinking = delta["reasoning_content"]
            .as_str()
            .or_else(|| delta["reasoning"].as_str())
            .filter(|delta| !delta.is_empty());
        if let Some(thinking) = thinking {
            events.push(StreamEvent::ThinkingDelta {
                index,
                block_index: 0,
                thinking_delta: thinking.to_owned(),
            });
        }
        if let Some(text) = delta["content"].as_str().filter(|text| !text.is_empty()) {
            events.push(StreamEvent::Delta { index, content: text.to_owned() });
        }
        for call in delta["tool_calls"].as_array().into_iter().flatten() {
            events.push(StreamEvent::ToolCallDelta {
                signature: None,
                // The tool call's own index, not the choice's: a single
                // choice may stream several calls at once.
                index: index_of(&call["index"]),
                id: call["id"].as_str().map(str::to_owned),
                name: call["function"]["name"].as_str().map(str::to_owned),
                arguments_delta: call["function"]["arguments"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            });
        }
        if let Some(reason) = choice["finish_reason"].as_str() {
            pending_finish.record(reason);
        }
    }

    if let Some(usage) = usage {
        let finish = pending_finish.take_finish();
        let has_finish = finish.is_some();
        if let Some(finish) = finish {
            events.push(finish);
        }
        events.push(StreamEvent::Usage { usage: usage_of(usage)? });
        if has_finish {
            events.push(StreamEvent::Done { finish_reason: None, stop_sequence: None });
            pending_finish.done_emitted = true;
        }
    }
    Ok(events)
}

/// Finds the earliest SSE frame separator in a **byte** buffer.
fn sse_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(left), Some(right)) if left <= right => Some((left, 2)),
        (_, Some(right)) => Some((right, 4)),
        (Some(left), None) => Some((left, 2)),
        (None, None) => None,
    }
}

/// One stream's parser: an unparsed byte tail and the held finish state.
#[derive(Default)]
struct OpenAiSseParser {
    tail: Vec<u8>,
    pending_finish: PendingFinish,
}

impl StreamParserV1 for OpenAiSseParser {
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>> {
        if chunk.is_empty() {
            return Ok(self.pending_finish.flush_pending_finish());
        }
        self.tail.extend_from_slice(chunk);

        let mut events = Vec::new();
        while let Some((end, separator)) = sse_frame_boundary(&self.tail) {
            // A complete frame must be UTF-8; a split mid-sequence never gets
            // here because the separator search would not have matched yet.
            let frame = match std::str::from_utf8(&self.tail[..end]) {
                Ok(frame) => frame.replace("\r\n", "\n"),
                Err(_) => {
                    return Err(provider_protocol_error(
                        "the upstream emitted a non-UTF-8 SSE frame",
                    ));
                }
            };
            self.tail.drain(..end + separator);

            let mut event_name = None;
            let mut data_lines = Vec::new();
            for line in frame.lines() {
                if line.starts_with(':') {
                    continue;
                }
                if let Some(value) = line.strip_prefix("event:") {
                    event_name = Some(value.strip_prefix(' ').unwrap_or(value));
                }
                if let Some(value) = line.strip_prefix("data:") {
                    data_lines.push(value.strip_prefix(' ').unwrap_or(value));
                }
            }
            if event_name == Some("error") {
                events.push(StreamEvent::Error {
                    error: provider_protocol_error("the upstream emitted an SSE error event"),
                });
                continue;
            }
            if data_lines.is_empty() {
                continue;
            }
            let payload = data_lines.join("\n");
            if payload == "[DONE]" {
                events.extend(self.pending_finish.finish_marker());
                continue;
            }
            events.extend(events_of_frame(&payload, &mut self.pending_finish)?);
        }
        Ok(events)
    }
}

impl ProviderComponentV1 for OpenAiCompatibleReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: "provider-openai-compatible".to_owned(),
            version: "2.13.5".to_owned(),
            api_version: PROVIDER_WORLD.to_owned(),
        }
    }

    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<token_station_protocol::ModelCapability>> {
        // No network, so the upstream's own catalog is unreachable. What its
        // operator declared is all there is.
        Ok(config.models.clone())
    }

    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        if request.messages.iter().any(|message| {
            message.extensions.get("reasoning_replay_protocol_family").and_then(Value::as_str)
                == Some("claude-signed-thinking")
        }) {
            return Err(capability(
                "the OpenAI-compatible dialect cannot consume Claude reasoning replay",
            ));
        }
        // NP-Q4: `OpenAI`'s encrypted reasoning items mean nothing to this wire; refused rather than
        // dropped, as the Claude carrier is (design record 2026-10-11 §2.6).
        if request.messages.iter().any(crate::reasoning_replay::is_openai_family) {
            return Err(capability(
                "the OpenAI-compatible dialect cannot consume OpenAI reasoning replay",
            ));
        }
        // A Claude model behind an OpenAI-compatible family that refuses a forced tool choice is
        // refused here, as the Anthropic components refuse it (the family model parameters
        // record §5).
        crate::anthropic_dialect::refuse_declared_forced_tool(request, config)?;
        let copilot = config.provider == GITHUB_COPILOT;
        let url = match config.provider.as_str() {
            // Copilot serves Chat Completions straight off its root: the kernel's `resolve` would
            // add `/v1` to an origin-only endpoint.
            GITHUB_COPILOT => format!("{}/chat/completions", config.base_url.as_str()),
            // The Coding Plan path sits below the operator's origin, as native appends it.
            glm_coding::FAMILY => format!(
                "{}{}",
                config.base_url.as_str().trim_end_matches('/'),
                glm_coding::CHAT_PATH
            ),
            // Native's default arm, `{endpoint}{api_path}`: the canonical path below whatever the
            // row names, an origin or a path. The kernel's `resolve` agrees on an origin only.
            kimi_code::FAMILY => format!(
                "{}{}",
                config.base_url.as_str().trim_end_matches('/'),
                kimi_code::CHAT_PATH
            ),
            // The row's endpoint is the API root, as on an `openai-compatible` row with a path.
            bailian_openai_compatible::FAMILY => format!(
                "{}{}",
                config.base_url.as_str().trim_end_matches('/'),
                bailian_openai_compatible::CHAT_PATH
            ),
            azure_openai_dated::FAMILY => azure_dated_url(request, config)?,
            _ => config.base_url.resolve(ProviderApi::ChatCompletions),
        };
        let mut descriptor = HttpRequestDescriptor::new(HttpMethod::Post, url);
        let identification: &[(&str, &str)] = if copilot { &COPILOT_HEADERS } else { &[] };
        descriptor.headers = SafeHeaders::try_new(
            std::iter::once(("content-type", "application/json"))
                .chain(identification.iter().copied()),
        )
        .map_err(internal)?;
        let mut body = body_of(request, config)?;
        // Typed since protocol 0.6.0 (R-Q15); rendered when the chosen model allows it.
        if let Some(effort) = request.reasoning.effort.as_ref().map(ReasoningEffort::as_str)
            && reasoning_effort_allowed(request, config)
            && let Value::Object(map) = &mut body
        {
            map.insert("reasoning_effort".to_owned(), json!(effort));
        }
        forward_client_fields(request, config, &mut body);
        render_glm_thinking(request, config, &mut body);
        split_bailian_cap(request, config, &mut body)?;
        descriptor.body = Some(body);
        // The host holds the value; this names the slot and the fixed
        // presentation selected by the trusted provider dialect.
        descriptor.auth = match (config.provider.as_str(), config.auth.clone()) {
            (
                "openai-compatible"
                | GITHUB_COPILOT
                | glm_coding::FAMILY
                | kimi_code::FAMILY
                | bailian_openai_compatible::FAMILY,
                secret,
            ) => secret.map(Auth::bearer),
            ("azure-openai-v1" | azure_openai_dated::FAMILY, Some(secret)) => {
                Some(Auth::header("api-key", secret).map_err(internal)?)
            }
            (GEMINI_OPENAI_COMPATIBLE, Some(secret)) => {
                Some(Auth::bearer_and_header("x-goog-api-key", secret).map_err(internal)?)
            }
            ("azure-openai-v1" | azure_openai_dated::FAMILY | GEMINI_OPENAI_COMPATIBLE, None) => {
                None
            }
            (dialect, _) => {
                return Err(capability(format!("unsupported provider dialect `{dialect}`")));
            }
        };
        Ok(descriptor)
    }

    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        let raw: Value = serde_json::from_str(&parts.body).map_err(|_| {
            provider_protocol_error("the upstream returned invalid JSON in a 2xx response")
        })?;
        if raw.get("error").is_some_and(|error| !error.is_null()) {
            return Err(provider_protocol_error(
                "the upstream embedded an error in a successful response",
            ));
        }
        let raw_choices = raw["choices"].as_array().ok_or_else(|| {
            provider_protocol_error("the upstream 2xx response has no choices array")
        })?;
        if raw_choices.is_empty() {
            return Err(provider_protocol_error("the upstream 2xx response contains no choices"));
        }

        let mut choices = Vec::new();
        for choice in raw_choices {
            let message = &choice["message"];
            let tool_calls = message["tool_calls"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|call| ToolCall {
                    signature: None,
                    id: call["id"].as_str().unwrap_or_default().to_owned(),
                    name: call["function"]["name"].as_str().unwrap_or_default().to_owned(),
                    arguments: call["function"]["arguments"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                })
                .collect();

            choices.push(Choice {
                index: index_of(&choice["index"]),
                // The openai chat wire has no stop-sequence report slot.
                stop_sequence: None,
                message: Message {
                    role: Role::Assistant,
                    // A reasoning report lifts content into parts form with
                    // the thinking block first; plain responses keep the
                    // bare-string shape.
                    // P1 (host-parity slice): the non-streaming twin of S4 —
                    // both reasoning spellings are read. Fixing only the
                    // streaming side would leave "thinking appears when
                    // streaming, vanishes when buffered", which is harder to
                    // diagnose than either failure alone.
                    content: match (
                        message["reasoning_content"]
                            .as_str()
                            .or_else(|| message["reasoning"].as_str())
                            .filter(|s| !s.is_empty()),
                        message["content"].as_str(),
                    ) {
                        (Some(thinking), text) => Some(Content::Parts({
                            let mut parts = vec![ContentPart::Thinking {
                                thinking: thinking.to_owned(),
                                signature: None,
                            }];
                            if let Some(text) = text.filter(|t| !t.is_empty()) {
                                parts.push(ContentPart::text(text.to_owned()));
                            }
                            parts
                        })),
                        (None, text) => text.map(|t| Content::Text(t.to_owned())),
                    },
                    tool_calls,
                    tool_call_id: None,
                    name: None,
                    extensions: Extensions::new(),
                },
                finish_reason: finish_reason(choice["finish_reason"].as_str()),
            });
        }

        let usage = usage_of(&raw["usage"])?;
        Ok(ChatResponse {
            id: raw["id"].as_str().unwrap_or_default().to_owned(),
            model: raw["model"].as_str().unwrap_or_default().to_owned(),
            choices,
            usage,
            extensions: Extensions::new(),
        })
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        let raw: Value = serde_json::from_str(&parts.body).unwrap_or(Value::Null);

        let provider_code = raw["error"]["code"].as_str().unwrap_or_default().to_ascii_lowercase();
        let code = if provider_code == "content_policy_violation" {
            ErrorCode::ContentPolicy
        } else if provider_code.contains("context_length")
            || provider_code.contains("maximum_context")
        {
            ErrorCode::ContextLength
        } else {
            match parts.status {
                400 | 404 | 422 => ErrorCode::InvalidRequest,
                401 | 403 => ErrorCode::Auth,
                402 => ErrorCode::PaymentRequired,
                408 => ErrorCode::Timeout,
                429 => ErrorCode::RateLimit,
                529 => ErrorCode::Capacity,
                500 | 502 | 503 | 504 => ErrorCode::UpstreamUnavailable,
                _ => ErrorCode::Internal,
            }
        };

        let message = match code {
            ErrorCode::InvalidRequest => "the upstream refused the request as malformed",
            ErrorCode::Auth => "the upstream rejected the credential",
            ErrorCode::PaymentRequired => {
                "the upstream requires payment or the account is out of funds"
            }
            ErrorCode::RateLimit => "the upstream rate limited this request",
            ErrorCode::ContentPolicy => "the upstream refused on content-policy grounds",
            ErrorCode::ContextLength => "the request exceeds the model's context window",
            ErrorCode::Timeout => "the upstream did not answer in time",
            ErrorCode::UpstreamUnavailable => "the upstream is unavailable",
            ErrorCode::TransportTruncated => "the upstream connection dropped mid-response",
            ErrorCode::ProviderProtocolError => "the upstream answered with an invalid body",
            ErrorCode::Capacity | ErrorCode::Capability | ErrorCode::Internal => {
                "the upstream failed"
            }
        };

        let mut envelope = ErrorEnvelope::new(code, parts.status, message);
        envelope.provider_message = raw["error"]["message"]
            .as_str()
            .filter(|message| message.chars().count() <= 256)
            .map(str::to_owned);
        envelope.retry_after_ms = parts
            .headers
            .get("retry-after")
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| seconds.saturating_mul(1000));
        Ok(envelope)
    }

    fn stream_parser(&self) -> Box<dyn StreamParserV1> {
        Box::new(OpenAiSseParser::default())
    }
}
