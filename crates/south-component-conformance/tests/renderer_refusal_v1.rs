//! Renderer refusal (0.15.0): a content part the IR carries verbatim
//! (`ContentPart::Unknown`) but a wire cannot spell is refused by name at
//! `build_http_request`, never pushed into the wire's content array.
//!
//! The IR's half of the contract is "never drop"; the renderer's half is
//! "never smuggle". These tests pin the boundary for the two wires that have
//! no spelling for foreign blocks, and pin that the Anthropic wire — whose own
//! blocks these are — keeps writing them back verbatim.

use serde_json::{Value, json};
use south_component_conformance::ProviderComponentV1;
use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::reference_anthropic::AnthropicReferenceV1;
use south_component_conformance::reference_gemini::GeminiReferenceV1;
use token_station_protocol::{ChatRequest, ErrorCode, ProviderConfig};

fn config(provider: &str, base_url: &str) -> ProviderConfig {
    serde_json::from_value(json!({"provider": provider, "base_url": base_url}))
        .expect("a provider config")
}

fn request_with_user_parts(parts: &Value) -> ChatRequest {
    serde_json::from_value(json!({
        "model": "m",
        "messages": [{"role": "user", "content": parts}],
        "sampling": {"max_output_tokens": 64}
    }))
    .expect("a canonical request")
}

const SERVER_TOOL_RESULT: &str = r#"{
    "type": "web_search_tool_result",
    "tool_use_id": "srvtoolu_1",
    "content": [{"type": "web_search_result", "title": "t", "url": "https://example.test"}]
}"#;

#[test]
fn openai_refuses_an_anthropic_server_tool_block_by_name() {
    let block: Value = serde_json::from_str(SERVER_TOOL_RESULT).expect("fixture JSON");
    let request = request_with_user_parts(&json!([{"type": "text", "text": "hi"}, block]));

    let error = OpenAiCompatibleReferenceV1
        .build_http_request(&request, &config("openai-compatible", "https://api.example/v1"))
        .expect_err("a server-tool block has no Chat Completions spelling");

    assert_eq!(error.code, ErrorCode::Capability);
    assert_eq!(error.http_status, 400);
    assert!(
        error.message.contains("`web_search_tool_result`"),
        "the refusal names the block: {}",
        error.message
    );
    assert!(
        !error.message.contains("example.test"),
        "the refusal carries no content: {}",
        error.message
    );
}

#[test]
fn openai_refuses_a_document_and_a_search_result_block() {
    for block in [
        json!({"type": "document", "source": {"type": "text", "media_type": "text/plain", "data": "d"}}),
        json!({"type": "search_result", "source": "s", "title": "t", "content": []}),
        json!({"no_type_at_all": true}),
    ] {
        let request = request_with_user_parts(&json!([block]));
        let error = OpenAiCompatibleReferenceV1
            .build_http_request(&request, &config("openai-compatible", "https://api.example/v1"))
            .expect_err("no Chat Completions spelling");
        assert_eq!(error.code, ErrorCode::Capability, "{}", error.message);
    }
}

#[test]
fn openai_still_forwards_its_own_unmodelled_part_types() {
    // `input_audio` is Chat Completions vocabulary the IR has no typed field
    // for yet; the upstream can read it, so it is forwarded, not refused.
    let audio = json!({"type": "input_audio", "input_audio": {"data": "AAAA", "format": "wav"}});
    let request = request_with_user_parts(&json!([audio]));

    let descriptor = OpenAiCompatibleReferenceV1
        .build_http_request(&request, &config("openai-compatible", "https://api.example/v1"))
        .expect("an OpenAI-native part renders");

    assert_eq!(descriptor.body.expect("a body")["messages"][0]["content"][0], audio);
}

#[test]
fn gemini_refuses_any_foreign_part() {
    let block: Value = serde_json::from_str(SERVER_TOOL_RESULT).expect("fixture JSON");
    let request = request_with_user_parts(&json!([block]));

    let error = GeminiReferenceV1
        .build_http_request(
            &request,
            &config("gemini", "https://generativelanguage.googleapis.com"),
        )
        .expect_err("Gemini parts have no discriminator to carry a foreign block");

    assert_eq!(error.code, ErrorCode::Capability);
    assert!(error.message.contains("`web_search_tool_result`"), "{}", error.message);
}

#[test]
fn anthropic_writes_its_own_blocks_back_verbatim() {
    let block: Value = serde_json::from_str(SERVER_TOOL_RESULT).expect("fixture JSON");
    let request = request_with_user_parts(&json!([block]));

    let descriptor = AnthropicReferenceV1
        .build_http_request(&request, &config("anthropic", "https://api.anthropic.com/v1"))
        .expect("the Anthropic wire spells its own blocks");

    assert_eq!(descriptor.body.expect("a body")["messages"][0]["content"][0], block);
}
