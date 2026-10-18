//! Declared model-parameter words that a host validates, exported under a path that names no
//! provider.
//!
//! A host checks a model row's `supported_parameters` against the words a component honours (a
//! misspelt word in a reserved namespace is refused at save time). The words themselves belong to
//! the component that reads them; this module only re-exports them, so a host can import the list
//! without naming a provider module.

/// The prompt-cache checkpoint words a model row can declare
/// ([`crate::reference_bedrock_converse::CACHE_POINT_PARAMETERS`]). They share one reserved
/// namespace, the prefix every word starts with.
pub const CACHE_POINT_PARAMETERS: [&str; 3] =
    crate::reference_bedrock_converse::CACHE_POINT_PARAMETERS;

/// The `model_parameters` words of each shipped provider family.
///
/// Sorted as a manifest lists them (the family model parameters record,
/// `docs/design/2026-10-18-family-model-parameters.md` §6); `family_model_parameters_v1` pins every
/// manifest to these.
pub mod family {
    /// `anthropic`, `claude-code`, `claude-code-setup-token` and `anthropic-bedrock-invoke`: the
    /// Claude dialect and the Claude replay carrier.
    pub const CLAUDE: [&str; 7] = [
        "anthropic.effort.xhigh",
        "anthropic.sampling.exclusive",
        "anthropic.sampling.none",
        "anthropic.thinking.adaptive",
        "anthropic.thinking.budget",
        "anthropic.tool_choice.auto_only",
        "reasoning_replay.claude.v1",
    ];
    /// `bedrock` and `bedrock-bearer`: the Claude dialect, the cache points and the Claude replay
    /// carrier.
    pub const BEDROCK_CONVERSE: [&str; 10] = [
        "anthropic.effort.xhigh",
        "anthropic.sampling.exclusive",
        "anthropic.sampling.none",
        "anthropic.thinking.adaptive",
        "anthropic.thinking.budget",
        "anthropic.tool_choice.auto_only",
        "bedrock.converse.cache_point",
        "bedrock.converse.cache_point.tools",
        "bedrock.converse.cache_point.ttl_1h",
        "reasoning_replay.claude.v1",
    ];
    /// The two Claude thinking forms a model declares at most one of, for every family that reads
    /// the Claude dialect.
    pub const CLAUDE_EXCLUSIVE: [&str; 2] =
        ["anthropic.thinking.adaptive", "anthropic.thinking.budget"];
    /// `openai-compatible`: the forced-tool refusal, `reasoning_content` and `reasoning_effort`.
    pub const OPENAI_COMPATIBLE: [&str; 3] =
        ["anthropic.tool_choice.auto_only", "reasoning_content", "reasoning_effort"];
    /// The other `provider-openai-compatible` families. The component reads `reasoning_content` on
    /// them too; declaring it, which lets a host derive it for them, is a separate measured change
    /// (MP-Q3).
    pub const OPENAI_COMPATIBLE_OTHER: [&str; 2] =
        ["anthropic.tool_choice.auto_only", "reasoning_effort"];
    /// Every `provider-openai-responses` family: the forced-tool refusal (lv, 2026-10-18), the
    /// effort and the `OpenAI` replay carrier.
    pub const OPENAI_RESPONSES: [&str; 3] =
        ["anthropic.tool_choice.auto_only", "reasoning_effort", "reasoning_replay.openai.v1"];
    /// `gemini` and `vertex-ai`: the forced-tool refusal only (lv, 2026-10-18: no family is
    /// exempt by its wire).
    pub const GEMINI_TEXT: [&str; 1] = ["anthropic.tool_choice.auto_only"];
    /// `kiro`: the forced-tool refusal only (MP-Q7).
    pub const KIRO: [&str; 1] = ["anthropic.tool_choice.auto_only"];
}

#[cfg(test)]
mod tests {
    use super::CACHE_POINT_PARAMETERS;
    use super::family;
    use crate::{anthropic_dialect, responses_vocabulary};

    fn sorted(words: &[&str]) -> bool {
        words.windows(2).all(|pair| pair[0] < pair[1])
    }

    #[test]
    fn the_family_lists_are_sorted_and_spelled_by_their_readers() {
        let lists: [&[&str]; 7] = [
            &family::GEMINI_TEXT,
            &family::CLAUDE,
            &family::BEDROCK_CONVERSE,
            &family::OPENAI_COMPATIBLE,
            &family::OPENAI_COMPATIBLE_OTHER,
            &family::OPENAI_RESPONSES,
            &family::KIRO,
        ];
        for list in lists {
            assert!(sorted(list), "{list:?}");
        }
        assert!(sorted(&family::CLAUDE_EXCLUSIVE));
        for word in anthropic_dialect::DIALECT_PARAMETERS {
            assert!(family::CLAUDE.contains(&word) && family::BEDROCK_CONVERSE.contains(&word));
        }
        for word in CACHE_POINT_PARAMETERS {
            assert!(family::BEDROCK_CONVERSE.contains(&word), "{word}");
        }
        for word in family::CLAUDE_EXCLUSIVE {
            assert!(family::CLAUDE.contains(&word));
        }
        assert!(family::CLAUDE_EXCLUSIVE.contains(&anthropic_dialect::THINKING_ADAPTIVE));
        assert!(family::CLAUDE_EXCLUSIVE.contains(&anthropic_dialect::THINKING_BUDGET));
        assert!(family::OPENAI_RESPONSES.contains(&responses_vocabulary::replay::CAPABILITY));
        let effort = responses_vocabulary::extension::REASONING_EFFORT;
        assert!(family::OPENAI_RESPONSES.contains(&effort));
        assert!(family::OPENAI_COMPATIBLE.contains(&effort));
        for list in lists {
            assert!(list.contains(&anthropic_dialect::TOOL_CHOICE_AUTO_ONLY), "{list:?}");
        }
    }

    #[test]
    fn the_cache_point_words_share_one_namespace() {
        let namespace = CACHE_POINT_PARAMETERS[0]
            .rsplit_once('.')
            .map(|(head, _)| head)
            .expect("a dotted word");
        assert!(
            CACHE_POINT_PARAMETERS.iter().all(|word| word.starts_with(namespace)),
            "{CACHE_POINT_PARAMETERS:?}"
        );
    }
}
