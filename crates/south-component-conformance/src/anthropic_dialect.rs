//! The per-model request dialect of Claude models, shared by the Anthropic
//! Messages and the Bedrock Converse components.
//!
//! Claude generations disagree on the request shape, and each disagreement is
//! a hard 400 rather than an ignored field (measured against the Messages API
//! on 2026-09-29):
//!
//! - Opus 4.7 and later reject `temperature` and `top_p` outright.
//! - Fable 5.1 and Opus 5.5 reject a forced `tool_choice` (`any` and `tool`).
//! - Opus 4.6 and later think through `thinking: {type: adaptive}` plus
//!   `output_config.effort`, and Opus 5 rejects the older
//!   `thinking: {type: enabled, budget_tokens}`; Haiku 4.5 only knows the
//!   budget form and rejects `effort`.
//! - `effort: xhigh` exists on Opus 4.7 and later but not on Sonnet 4.6.
//! - With thinking on, `temperature` must be 1 or unset and `top_p` at least
//!   0.95 or unset; the budget form also rejects a forced `tool_choice`.
//! - Opus 4.5 through Sonnet 4.6 accept `temperature` or `top_p` but reject
//!   both in one request.
//!
//! The component cannot tell these apart from the model name — hosts route
//! aliases, Bedrock IDs and private deployments — so the host declares the
//! dialect per model in `supported_parameters`, the same channel as
//! `reasoning_replay.claude.v1`. Declaring nothing keeps the request shape
//! exactly as it was before these words existed.

use serde_json::{Value, json};
use token_station_protocol::{
    ChatRequest, ErrorCode, ErrorEnvelope, ProviderConfig, ReasoningMode, ToolChoice,
};

use crate::component::ComponentResultV1;

/// The model rejects `temperature`, `top_p` and `top_k`; they are dropped, not sent.
pub const SAMPLING_NONE: &str = "anthropic.sampling.none";
/// The model rejects a forced tool choice; a request that forces one is refused.
pub const TOOL_CHOICE_AUTO_ONLY: &str = "anthropic.tool_choice.auto_only";
/// The model thinks through `thinking: {type: adaptive}` and `output_config.effort`.
pub const THINKING_ADAPTIVE: &str = "anthropic.thinking.adaptive";
/// The model thinks through `thinking: {type: enabled, budget_tokens}`.
pub const THINKING_BUDGET: &str = "anthropic.thinking.budget";
/// The model accepts `effort: xhigh`; without it `xhigh` is sent as `high`.
pub const EFFORT_XHIGH: &str = "anthropic.effort.xhigh";
/// The model accepts `temperature` or `top_p` but not both; when a request
/// carries both, `top_p` is dropped and `temperature` kept.
pub const SAMPLING_EXCLUSIVE: &str = "anthropic.sampling.exclusive";

/// Every dialect word, for hosts that validate the reserved namespace.
pub const DIALECT_PARAMETERS: [&str; 6] = [
    SAMPLING_NONE,
    TOOL_CHOICE_AUTO_ONLY,
    THINKING_ADAPTIVE,
    THINKING_BUDGET,
    EFFORT_XHIGH,
    SAMPLING_EXCLUSIVE,
];

/// The budget sent for each budget-form effort level. Anthropic's minimum
/// budget is 1024 tokens.
const BUDGET_LOW: u64 = 1024;
const BUDGET_MEDIUM: u64 = 4096;
const BUDGET_HIGH: u64 = 16384;

/// How a declared model is asked to think.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Thinking {
    /// `thinking: {type: adaptive}` with `output_config.effort`.
    Adaptive { effort: &'static str },
    /// `thinking: {type: enabled, budget_tokens}`.
    Budget { budget_tokens: u64 },
    /// The caller's own `thinking` configuration (protocol 0.6.0 `ChatRequest.reasoning` with a
    /// mode), written back unchanged: mode, budget and effort exactly as sent, whatever thinking
    /// shape the model declares (KC1 record §4.1, owner ruling K5). The upstream rules on it, as
    /// it does on the native pass-through row.
    Verbatim { mode: ReasoningMode, budget_tokens: Option<u32>, effort: Option<String> },
}

impl Thinking {
    /// Whether the model thinks under this form. A caller's `disabled` is a thinking form that
    /// leaves sampling alone.
    pub(crate) const fn is_on(&self) -> bool {
        !matches!(self, Self::Verbatim { mode: ReasoningMode::Disabled, .. })
    }

    /// The Messages-API fields this thinking mode adds to a request body.
    pub(crate) fn fields(&self) -> Vec<(&'static str, Value)> {
        match self {
            Self::Adaptive { effort } => vec![
                ("thinking", json!({"type": "adaptive"})),
                ("output_config", json!({"effort": effort})),
            ],
            Self::Budget { budget_tokens } => {
                vec![("thinking", json!({"type": "enabled", "budget_tokens": budget_tokens}))]
            }
            Self::Verbatim { mode, budget_tokens, effort } => {
                let mut thinking = json!({"type": match mode {
                    ReasoningMode::Enabled => "enabled",
                    ReasoningMode::Disabled => "disabled",
                    ReasoningMode::Adaptive => "adaptive",
                }});
                if let Some(budget) = budget_tokens {
                    thinking["budget_tokens"] = json!(budget);
                }
                let mut fields = vec![("thinking", thinking)];
                if let Some(effort) = effort {
                    fields.push(("output_config", json!({"effort": effort})));
                }
                fields
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Adaptive,
    Budget,
}

/// Which sampling parameters the model accepts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Sampling {
    /// `temperature` and `top_p`, together or alone.
    #[default]
    Any,
    /// Either one, but not both in one request.
    Exclusive,
    /// Neither.
    None,
}

/// What the host declared about the target model.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Dialect {
    sampling: Sampling,
    auto_only: bool,
    shape: Option<Shape>,
    xhigh: bool,
    /// The host declared at least one word of this dialect for the model: it is a Claude model.
    claude: bool,
}

fn capability(detail: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Capability, 400, detail)
}

/// Whether the request forces the model to call a tool. A named choice the
/// wire cannot read carries no name and is left out downstream, so it does
/// not force anything.
fn forces_tool(request: &ChatRequest) -> bool {
    match &request.tool_choice {
        Some(ToolChoice::Required) => !request.tools.is_empty(),
        Some(ToolChoice::Other(value)) => {
            !request.tools.is_empty()
                && value.get("function").and_then(|function| function.get("name")).is_some()
        }
        Some(ToolChoice::Auto | ToolChoice::None) | None => false,
    }
}

/// Whether a model declaring `supported_parameters` refuses this request's tool choice.
///
/// Hosts call it to refuse before routing under their own error code; the
/// components refuse exactly the same requests, so the two cannot disagree.
pub fn refuses_forced_tool<'a>(
    supported_parameters: impl IntoIterator<Item = &'a str>,
    request: &ChatRequest,
) -> bool {
    supported_parameters.into_iter().any(|word| word == TOOL_CHOICE_AUTO_ONLY)
        && forces_tool(request)
}

/// Refuses a forced tool choice when the host declared [`TOOL_CHOICE_AUTO_ONLY`] for
/// `request.model`, with the error `Dialect::refuse_forced_tool` gives.
///
/// For components that serve Claude models without reading the rest of this dialect
/// (`provider-kiro`, `provider-openai-compatible`; the family model parameters record §5): the
/// refusal moves from the host's pre-refusal into the component unchanged in meaning.
pub(crate) fn refuse_declared_forced_tool(
    request: &ChatRequest,
    config: &ProviderConfig,
) -> ComponentResultV1<()> {
    let declared = config
        .models
        .iter()
        .find(|model| model.model == request.model)
        .into_iter()
        .flat_map(|model| model.supported_parameters.iter().map(String::as_str));
    if refuses_forced_tool(declared, request) {
        return Err(capability(FORCED_TOOL_REFUSAL));
    }
    Ok(())
}

/// Why a forced tool choice is refused on a model declaring [`TOOL_CHOICE_AUTO_ONLY`].
const FORCED_TOOL_REFUSAL: &str = "the target model does not support forcing a tool call \
     (tool_choice required or a named tool); use tool_choice auto";

impl Dialect {
    /// The dialect the host declared for `request.model`; the empty dialect
    /// when the model is not listed.
    pub(crate) fn of(request: &ChatRequest, config: &ProviderConfig) -> ComponentResultV1<Self> {
        let Some(model) = config.models.iter().find(|model| model.model == request.model) else {
            return Ok(Self::default());
        };
        let declared = |word: &str| model.supported_parameters.contains(word);
        let shape = match (declared(THINKING_ADAPTIVE), declared(THINKING_BUDGET)) {
            (true, true) => {
                return Err(capability(format!(
                    "the model declares both {THINKING_ADAPTIVE} and {THINKING_BUDGET}"
                )));
            }
            (true, false) => Some(Shape::Adaptive),
            (false, true) => Some(Shape::Budget),
            (false, false) => None,
        };
        Ok(Self {
            sampling: if declared(SAMPLING_NONE) {
                Sampling::None
            } else if declared(SAMPLING_EXCLUSIVE) {
                Sampling::Exclusive
            } else {
                Sampling::Any
            },
            auto_only: declared(TOOL_CHOICE_AUTO_ONLY),
            shape,
            xhigh: declared(EFFORT_XHIGH),
            claude: DIALECT_PARAMETERS.iter().any(|word| declared(word)),
        })
    }

    /// Refuses a forced tool choice the model cannot honor. Downgrading it to
    /// `auto` would let the model answer without the tool the caller required.
    pub(crate) fn refuse_forced_tool(self, request: &ChatRequest) -> ComponentResultV1<()> {
        if self.auto_only && forces_tool(request) {
            return Err(capability(FORCED_TOOL_REFUSAL));
        }
        Ok(())
    }

    /// How to ask the model to think, from the caller's `reasoning_effort`.
    ///
    /// `None` when the model declares no thinking shape (the effort is ignored,
    /// as before these words existed), when the caller gave no effort, or when
    /// the budget form cannot be honored: the budget must stay below
    /// `max_tokens`, and a forced tool choice excludes it — the forced choice
    /// is a guarantee the caller asked for, the effort only a preference.
    ///
    /// A caller that sent its own thinking mode (Messages `thinking`, protocol 0.6.0) gets it back
    /// verbatim when `verbatim` allows it ([`Thinking::Verbatim`]); the declared shape decides only
    /// for an effort with no mode. A budget without a mode is not a form the caller wrote and is
    /// not invented into one.
    pub(crate) fn thinking(
        self,
        request: &ChatRequest,
        max_tokens: Option<u64>,
        verbatim: bool,
    ) -> ComponentResultV1<Option<Thinking>> {
        if verbatim && let Some(mode) = request.reasoning.mode {
            return Ok(Some(Thinking::Verbatim {
                mode,
                budget_tokens: request.reasoning.budget_tokens,
                effort: request.reasoning.effort.as_ref().map(|effort| effort.as_str().to_owned()),
            }));
        }
        let Some(shape) = self.shape else {
            return Ok(None);
        };
        // Typed since protocol 0.6.0 (R-Q15); before, the same wire key arrived in `extensions`.
        let Some(effort) = request.reasoning.effort.as_ref() else {
            return Ok(None);
        };
        let level = match effort.as_str() {
            "none" | "minimal" => Level::Floor,
            "low" => Level::Low,
            "medium" => Level::Medium,
            "high" => Level::High,
            "xhigh" => Level::Xhigh,
            other => {
                return Err(capability(format!(
                    "unsupported reasoning effort `{other}` (expected one of none, minimal, low, \
                     medium, high, xhigh)"
                )));
            }
        };
        Ok(match shape {
            // The adaptive form is never switched off: Fable 5.1 and Opus 5.5
            // reject `thinking: disabled`, so the lowest request is `low`.
            Shape::Adaptive => Some(Thinking::Adaptive {
                effort: match level {
                    Level::Floor | Level::Low => "low",
                    Level::Medium => "medium",
                    Level::Xhigh if self.xhigh => "xhigh",
                    Level::High | Level::Xhigh => "high",
                },
            }),
            Shape::Budget => {
                let budget = match level {
                    Level::Floor => return Ok(None),
                    Level::Low => BUDGET_LOW,
                    Level::Medium => BUDGET_MEDIUM,
                    Level::High | Level::Xhigh => BUDGET_HIGH,
                };
                let fits = max_tokens.is_some_and(|max| budget < max);
                (fits && !forces_tool(request))
                    .then_some(Thinking::Budget { budget_tokens: budget })
            }
        })
    }

    /// Whether `temperature`, `top_p` and `top_k` may be sent. The model may reject
    /// them outright, and with thinking on a non-default value is rejected
    /// too (and `top_k` at all); either way they are dropped rather than approximated.
    pub(crate) fn keeps_sampling(self, thinking: Option<&Thinking>) -> bool {
        !matches!(self.sampling, Sampling::None) && !thinking.is_some_and(Thinking::is_on)
    }

    /// Whether the host declared any word of this dialect for the model, which marks it as a
    /// Claude model on a wire that also serves others (Converse): only then are Claude's own
    /// request fields (a verbatim `thinking`, `top_k`) sent there (KC1 record §4.2, KC-Q7).
    pub(crate) const fn is_claude(self) -> bool {
        self.claude
    }

    /// Whether `top_p` may be sent next to a `temperature`. A model that takes
    /// only one of them keeps `temperature`, the knob callers set far more
    /// often, and drops `top_p` rather than refusing the request.
    pub(crate) const fn keeps_top_p_with_temperature(self) -> bool {
        !matches!(self.sampling, Sampling::Exclusive)
    }
}

#[derive(Debug, Clone, Copy)]
enum Level {
    Floor,
    Low,
    Medium,
    High,
    Xhigh,
}

#[cfg(test)]
mod tests {
    use super::*;
    use token_station_protocol::{
        Message, ModelCapability, ProviderEndpoint, Reasoning, ReasoningEffort, Role, ToolDef,
    };

    fn config(words: &[&str]) -> ProviderConfig {
        let mut config = ProviderConfig::new(
            "anthropic",
            ProviderEndpoint::try_new("https://api.anthropic.com").unwrap(),
        );
        config.models.push(ModelCapability {
            model: "m".into(),
            supported_parameters: words.iter().map(|word| (*word).to_owned()).collect(),
            ..ModelCapability::default()
        });
        config
    }

    fn request(effort: Option<&str>) -> ChatRequest {
        let mut request = ChatRequest::new("m", vec![Message::text(Role::User, "hi")]);
        if let Some(effort) = effort {
            request.reasoning.effort = Some(serde_json::from_value(json!(effort)).unwrap());
        }
        request
    }

    fn with_tool(mut request: ChatRequest, choice: ToolChoice) -> ChatRequest {
        request.tools.push(ToolDef::new("t", None, json!({"type": "object"})));
        request.tool_choice = Some(choice);
        request
    }

    fn thinking(words: &[&str], effort: Option<&str>, max: Option<u64>) -> Option<Thinking> {
        let request = request(effort);
        Dialect::of(&request, &config(words)).unwrap().thinking(&request, max, true).unwrap()
    }

    #[test]
    fn an_undeclared_model_keeps_todays_request() {
        let request = request(Some("high"));
        let dialect = Dialect::of(&request, &config(&[])).unwrap();
        assert_eq!(dialect.thinking(&request, Some(4096), true).unwrap(), None);
        assert!(dialect.keeps_sampling(None));
        let forced = with_tool(request, ToolChoice::Required);
        assert!(dialect.refuse_forced_tool(&forced).is_ok());
    }

    #[test]
    fn adaptive_effort_maps_the_openai_vocabulary() {
        let adaptive = [THINKING_ADAPTIVE];
        for (given, sent) in [
            ("none", "low"),
            ("minimal", "low"),
            ("low", "low"),
            ("medium", "medium"),
            ("high", "high"),
            ("xhigh", "high"),
        ] {
            assert_eq!(
                thinking(&adaptive, Some(given), Some(4096)),
                Some(Thinking::Adaptive { effort: sent }),
                "{given}"
            );
        }
        assert_eq!(
            thinking(&[THINKING_ADAPTIVE, EFFORT_XHIGH], Some("xhigh"), Some(4096)),
            Some(Thinking::Adaptive { effort: "xhigh" })
        );
        assert_eq!(thinking(&adaptive, None, Some(4096)), None, "no effort, no thinking fields");
    }

    #[test]
    fn budget_effort_stays_below_max_tokens_and_yields_to_a_forced_tool() {
        let budget = [THINKING_BUDGET];
        assert_eq!(thinking(&budget, Some("none"), Some(4096)), None);
        assert_eq!(
            thinking(&budget, Some("low"), Some(4096)),
            Some(Thinking::Budget { budget_tokens: 1024 })
        );
        assert_eq!(thinking(&budget, Some("medium"), Some(4096)), None, "4096 is not below 4096");
        assert_eq!(
            thinking(&budget, Some("xhigh"), Some(20000)),
            Some(Thinking::Budget { budget_tokens: 16384 })
        );
        assert_eq!(thinking(&budget, Some("low"), None), None, "no max, no safe budget");

        let forced = with_tool(request(Some("low")), ToolChoice::Required);
        let dialect = Dialect::of(&forced, &config(&budget)).unwrap();
        assert_eq!(dialect.thinking(&forced, Some(4096), true).unwrap(), None);
    }

    #[test]
    fn sampling_is_dropped_when_declared_or_when_thinking_is_on() {
        let request = request(None);
        let none = Dialect::of(&request, &config(&[SAMPLING_NONE])).unwrap();
        assert!(!none.keeps_sampling(None));
        let adaptive = Dialect::of(&request, &config(&[THINKING_ADAPTIVE])).unwrap();
        assert!(adaptive.keeps_sampling(None));
        assert!(!adaptive.keeps_sampling(Some(&Thinking::Adaptive { effort: "low" })));
    }

    #[test]
    fn exclusive_sampling_keeps_temperature_over_top_p() {
        let request = request(None);
        let exclusive = Dialect::of(&request, &config(&[SAMPLING_EXCLUSIVE])).unwrap();
        assert!(exclusive.keeps_sampling(None));
        assert!(!exclusive.keeps_top_p_with_temperature());
        let plain = Dialect::of(&request, &config(&[])).unwrap();
        assert!(plain.keeps_top_p_with_temperature());
    }

    #[test]
    fn a_forced_tool_is_refused_only_on_auto_only_models() {
        let dialect = Dialect::of(&request(None), &config(&[TOOL_CHOICE_AUTO_ONLY])).unwrap();
        let required = with_tool(request(None), ToolChoice::Required);
        assert!(dialect.refuse_forced_tool(&required).is_err());
        let named = with_tool(
            request(None),
            ToolChoice::Other(json!({"type": "function", "function": {"name": "t"}})),
        );
        assert!(dialect.refuse_forced_tool(&named).is_err());
        let auto = with_tool(request(None), ToolChoice::Auto);
        assert!(dialect.refuse_forced_tool(&auto).is_ok());
    }

    #[test]
    fn the_published_host_check_agrees_with_the_component() {
        let words = [TOOL_CHOICE_AUTO_ONLY];
        let forced = with_tool(request(None), ToolChoice::Required);
        let auto = with_tool(request(None), ToolChoice::Auto);
        let dialect = Dialect::of(&forced, &config(&words)).unwrap();
        for candidate in [&forced, &auto] {
            assert_eq!(
                refuses_forced_tool(words, candidate),
                dialect.refuse_forced_tool(candidate).is_err()
            );
        }
        assert!(!refuses_forced_tool([], &forced));
    }

    #[test]
    fn contradictory_or_malformed_declarations_are_refused() {
        let request = request(Some("high"));
        assert!(Dialect::of(&request, &config(&[THINKING_ADAPTIVE, THINKING_BUDGET])).is_err());
        let dialect = Dialect::of(&request, &config(&[THINKING_ADAPTIVE])).unwrap();
        let mut odd = request;
        odd.reasoning.effort = Some(ReasoningEffort::Other("max".to_owned()));
        assert!(
            dialect.thinking(&odd, Some(4096), true).is_err(),
            "max is not in the OpenAI vocabulary"
        );
    }

    /// KC1 §4.1: a caller's own mode is sent back unchanged whatever shape the model declares,
    /// only where the wire allows it; `disabled` leaves sampling alone.
    #[test]
    fn a_callers_mode_is_verbatim_and_disabled_keeps_sampling() {
        let mut request = request(Some("max"));
        request.reasoning.mode = Some(ReasoningMode::Adaptive);
        let dialect = Dialect::of(&request, &config(&[THINKING_BUDGET])).unwrap();
        let verbatim = dialect.thinking(&request, Some(4096), true).unwrap().unwrap();
        assert_eq!(
            verbatim,
            Thinking::Verbatim {
                mode: ReasoningMode::Adaptive,
                budget_tokens: None,
                effort: Some("max".to_owned()),
            }
        );
        assert_eq!(
            verbatim.fields(),
            vec![
                ("thinking", json!({"type": "adaptive"})),
                ("output_config", json!({"effort": "max"})),
            ]
        );
        assert!(!dialect.keeps_sampling(Some(&verbatim)));
        // Not allowed here: the effort rule decides, and `max` is not in its vocabulary.
        assert!(dialect.thinking(&request, Some(4096), false).is_err());

        request.reasoning =
            Reasoning { mode: Some(ReasoningMode::Disabled), effort: None, budget_tokens: None };
        let off = dialect.thinking(&request, Some(4096), true).unwrap().unwrap();
        assert!(!off.is_on());
        assert!(dialect.keeps_sampling(Some(&off)));
        assert_eq!(off.fields(), vec![("thinking", json!({"type": "disabled"}))]);
        assert!(dialect.is_claude());
        assert!(!Dialect::of(&request, &config(&[])).unwrap().is_claude());
    }
}
