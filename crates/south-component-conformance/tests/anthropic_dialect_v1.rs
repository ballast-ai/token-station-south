//! The refusals of the per-model Claude request dialect, through each
//! component's real `build_http_request`. The accepted shapes are pinned by
//! the `provider.request.dialect-*` fixtures; these are the requests that must
//! never reach the upstream.

use serde_json::{Value, json};
use south_component_conformance::{
    ProviderComponentV1, anthropic_dialect, reference_anthropic::AnthropicReferenceV1,
    reference_bedrock_converse::BedrockConverseReferenceV1,
};
use token_station_protocol::{ChatRequest, ErrorCode, ProviderConfig};

const MODEL: &str = "claude-x";

fn config(provider: &str, base_url: &str, words: &[&str]) -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": provider,
        "base_url": base_url,
        "models": [{"model": MODEL, "supported_parameters": words}],
    }))
    .unwrap()
}

fn anthropic(words: &[&str]) -> ProviderConfig {
    config("anthropic", "https://api.anthropic.com", words)
}

fn converse(words: &[&str]) -> ProviderConfig {
    config("bedrock", "https://bedrock-runtime.us-east-1.amazonaws.com", words)
}

fn request(extra: &Value) -> ChatRequest {
    let mut value = json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"name": "t", "parameters": {"type": "object"}}],
        "sampling": {"max_output_tokens": 4096},
    });
    for (key, field) in extra.as_object().unwrap() {
        value[key] = field.clone();
    }
    serde_json::from_value(value).unwrap()
}

fn refused(component: &dyn ProviderComponentV1, config: &ProviderConfig, request: &ChatRequest) {
    let error = component.build_http_request(request, config).expect_err("must be refused");
    assert_eq!(error.code, ErrorCode::Capability, "{error:?}");
    assert_eq!(error.http_status, 400, "{error:?}");
}

fn accepted(component: &dyn ProviderComponentV1, config: &ProviderConfig, request: &ChatRequest) {
    component.build_http_request(request, config).expect("must be accepted");
}

#[test]
fn a_forced_tool_is_refused_on_an_auto_only_model_by_both_components() {
    let auto_only = [anthropic_dialect::TOOL_CHOICE_AUTO_ONLY];
    let required = request(&json!({"tool_choice": "required"}));
    let named = request(&json!({"tool_choice": {"type": "function", "function": {"name": "t"}}}));
    let auto = request(&json!({"tool_choice": "auto"}));
    for (component, config) in [
        (&AnthropicReferenceV1 as &dyn ProviderComponentV1, anthropic(&auto_only)),
        (&BedrockConverseReferenceV1 as &dyn ProviderComponentV1, converse(&auto_only)),
    ] {
        refused(component, &config, &required);
        refused(component, &config, &named);
        accepted(component, &config, &auto);
    }
}

#[test]
fn a_forced_tool_still_reaches_a_model_that_did_not_declare_auto_only() {
    let required = request(&json!({"tool_choice": "required"}));
    accepted(&AnthropicReferenceV1, &anthropic(&[]), &required);
    accepted(&BedrockConverseReferenceV1, &converse(&[]), &required);
}

#[test]
fn both_thinking_forms_or_an_unknown_effort_are_refused() {
    let both = [anthropic_dialect::THINKING_ADAPTIVE, anthropic_dialect::THINKING_BUDGET];
    let plain = request(&json!({}));
    refused(&AnthropicReferenceV1, &anthropic(&both), &plain);
    refused(&BedrockConverseReferenceV1, &converse(&both), &plain);

    let adaptive = [anthropic_dialect::THINKING_ADAPTIVE];
    let odd = request(&json!({"reasoning_effort": "max"}));
    refused(&AnthropicReferenceV1, &anthropic(&adaptive), &odd);
    refused(&BedrockConverseReferenceV1, &converse(&adaptive), &odd);
    // Without a declared thinking form the effort is not read at all.
    accepted(&AnthropicReferenceV1, &anthropic(&[]), &odd);
}

#[test]
fn every_dialect_word_is_published_for_hosts() {
    assert_eq!(
        anthropic_dialect::DIALECT_PARAMETERS,
        [
            "anthropic.sampling.none",
            "anthropic.tool_choice.auto_only",
            "anthropic.thinking.adaptive",
            "anthropic.thinking.budget",
            "anthropic.effort.xhigh",
        ]
    );
}
