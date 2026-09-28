use serde_json::json;
use south_component_conformance::{
    ProviderComponentV1, reference::OpenAiCompatibleReferenceV1,
    reference_anthropic::AnthropicReferenceV1,
    reference_bedrock_converse::BedrockConverseReferenceV1, reference_gemini::GeminiReferenceV1,
};
use token_station_protocol::{
    ChatRequest, ContentPart, HttpResponseParts, ProviderConfig, StreamEvent,
};

const CAPABILITY: &str = "reasoning_replay.claude.v1";

fn request(model: &str) -> ChatRequest {
    serde_json::from_value(json!({
        "model": model,
        "messages": [{
            "role": "assistant",
            "content": [
                {"type":"thinking","thinking":"first","signature":"sig-1"},
                {"type":"text","text":"answer"},
                {"type":"redacted_thinking","data":"opaque"}
            ],
            "reasoning_replay_protocol_family":"claude-signed-thinking"
        }]
    }))
    .unwrap()
}

fn config(provider: &str, base_url: &str, model: &str, enabled: bool) -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": provider,
        "base_url": base_url,
        "models": [{
            "model": model,
            "supported_parameters": if enabled { json!([CAPABILITY]) } else { json!([]) }
        }]
    }))
    .unwrap()
}

#[test]
fn x1_anthropic_and_converse_require_explicit_replay_capability() {
    let anthropic_model = "claude-sonnet";
    let anthropic_request = request(anthropic_model);
    assert!(
        AnthropicReferenceV1
            .build_http_request(
                &anthropic_request,
                &config("anthropic", "https://api.anthropic.com", anthropic_model, false),
            )
            .is_err()
    );
    let anthropic = AnthropicReferenceV1
        .build_http_request(
            &anthropic_request,
            &config("anthropic", "https://api.anthropic.com", anthropic_model, true),
        )
        .unwrap()
        .body
        .unwrap();
    assert_eq!(anthropic["messages"][0]["content"][0]["type"], "thinking");
    assert_eq!(anthropic["messages"][0]["content"][0]["signature"], "sig-1");
    assert_eq!(anthropic["messages"][0]["content"][2]["type"], "redacted_thinking");

    let converse_model = "anthropic.claude-sonnet-4-v1:0";
    let converse_request = request(converse_model);
    assert!(
        BedrockConverseReferenceV1
            .build_http_request(
                &converse_request,
                &config(
                    "bedrock",
                    "https://bedrock-runtime.us-east-1.amazonaws.com",
                    converse_model,
                    false,
                ),
            )
            .is_err()
    );
    let converse = BedrockConverseReferenceV1
        .build_http_request(
            &converse_request,
            &config(
                "bedrock",
                "https://bedrock-runtime.us-east-1.amazonaws.com",
                converse_model,
                true,
            ),
        )
        .unwrap()
        .body
        .unwrap();
    assert_eq!(
        converse["messages"][0]["content"][0]["reasoningContent"]["reasoningText"],
        json!({"text":"first","signature":"sig-1"})
    );
    assert_eq!(
        converse["messages"][0]["content"][2]["reasoningContent"]["redactedContent"],
        "opaque"
    );
}

#[test]
fn x1_anthropic_and_converse_preserve_interleaved_replay_layout() {
    let request: ChatRequest = serde_json::from_value(json!({
        "model": "claude-sonnet",
        "messages": [{
            "role": "assistant",
            "content": [
                {"type":"thinking","thinking":"first","signature":"sig-1"},
                {"type":"text","text":"answer"},
                {"type":"redacted_thinking","data":"opaque"}
            ],
            "tool_calls": [{"id":"call-1","name":"lookup","arguments":"{}"}],
            "reasoning_replay_protocol_family":"claude-signed-thinking",
            "reasoning_replay_block_layout": [
                {"kind":"content","ordinal":0},
                {"kind":"content","ordinal":1},
                {"kind":"tool_call","call_id":"call-1"},
                {"kind":"content","ordinal":2}
            ]
        }]
    }))
    .unwrap();

    let anthropic = AnthropicReferenceV1
        .build_http_request(
            &request,
            &config("anthropic", "https://api.anthropic.com", "claude-sonnet", true),
        )
        .unwrap()
        .body
        .unwrap();
    let anthropic_blocks = anthropic["messages"][0]["content"].as_array().unwrap();
    assert_eq!(
        anthropic_blocks.iter().map(|block| block["type"].as_str().unwrap()).collect::<Vec<_>>(),
        ["thinking", "text", "tool_use", "redacted_thinking"]
    );

    let mut converse_request = request;
    converse_request.model = "anthropic.claude-sonnet-4-v1:0".into();
    let converse = BedrockConverseReferenceV1
        .build_http_request(
            &converse_request,
            &config(
                "bedrock",
                "https://bedrock-runtime.us-east-1.amazonaws.com",
                "anthropic.claude-sonnet-4-v1:0",
                true,
            ),
        )
        .unwrap()
        .body
        .unwrap();
    let converse_blocks = converse["messages"][0]["content"].as_array().unwrap();
    assert!(converse_blocks[0].get("reasoningContent").is_some());
    assert!(converse_blocks[1].get("text").is_some());
    assert!(converse_blocks[2].get("toolUse").is_some());
    assert!(converse_blocks[3].get("reasoningContent").is_some());
}

#[test]
fn x1_converse_response_preserves_thinking_signature_and_redacted_blocks() {
    let parts: HttpResponseParts = serde_json::from_value(json!({
        "status": 200,
        "headers": {},
        "body": json!({
            "output":{"message":{"content":[
                {"reasoningContent":{"reasoningText":{"text":"first","signature":"sig-1"}}},
                {"text":"answer"},
                {"reasoningContent":{"redactedContent":"opaque"}}
            ]}},
            "stopReason":"end_turn",
            "usage":{"inputTokens":1,"outputTokens":2,"totalTokens":3}
        }).to_string()
    }))
    .unwrap();
    let response = BedrockConverseReferenceV1.parse_response(&parts).unwrap();
    let token_station_protocol::Content::Parts(parts) =
        response.choices[0].message.content.as_ref().unwrap()
    else {
        panic!("reasoning response must preserve ordered parts");
    };
    assert!(
        matches!(&parts[0], ContentPart::Thinking { thinking, signature: Some(signature) } if thinking == "first" && signature == "sig-1")
    );
    assert!(matches!(&parts[1], ContentPart::Text { text } if text == "answer"));
    assert!(matches!(&parts[2], ContentPart::RedactedThinking { data } if data == "opaque"));
}

#[test]
fn x1_unmarked_reasoning_does_not_gain_replay_authority() {
    let mut request = request("anthropic.claude-sonnet-4-v1:0");
    request.messages[0].extensions.clear();
    let body = BedrockConverseReferenceV1
        .build_http_request(
            &request,
            &config(
                "bedrock",
                "https://bedrock-runtime.us-east-1.amazonaws.com",
                "anthropic.claude-sonnet-4-v1:0",
                true,
            ),
        )
        .unwrap()
        .body
        .unwrap();
    let rendered = body.to_string();
    assert!(!rendered.contains("sig-1"));
    assert!(!rendered.contains("opaque"));
}

#[test]
fn x1_openai_compatible_and_gemini_reject_claude_replay() {
    let openai = request("m");
    assert!(
        OpenAiCompatibleReferenceV1
            .build_http_request(
                &openai,
                &config("openai-compatible", "https://example.test/v1", "m", true),
            )
            .is_err()
    );
    let gemini = request("m");
    assert!(
        GeminiReferenceV1
            .build_http_request(&gemini, &config("gemini", "https://example.test", "m", true),)
            .is_err()
    );
}

#[test]
fn x5_anthropic_stream_preserves_block_identity_signature_and_redacted_data() {
    let mut parser = AnthropicReferenceV1.stream_parser();
    let events = parser
        .parse_chunk(concat!(
            "event: content_block_start\n",
            "data: {\"index\":2,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"index\":2,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"a\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"index\":2,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig\"}}\n\n",
            "event: content_block_start\n",
            "data: {\"index\":4,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"opaque\"}}\n\n"
        ).as_bytes())
        .unwrap();
    assert_eq!(
        events,
        vec![
            StreamEvent::ThinkingDelta { index: 0, block_index: 2, thinking_delta: String::new() },
            StreamEvent::ThinkingDelta { index: 0, block_index: 2, thinking_delta: "a".into() },
            StreamEvent::ThinkingSignatureDelta {
                index: 0,
                block_index: 2,
                signature_delta: "sig".into(),
            },
            StreamEvent::RedactedThinking { index: 0, block_index: 4, data: "opaque".into() },
        ]
    );
}

#[test]
fn x5_converse_stream_preserves_reasoning_variants_and_block_identity() {
    let mut parser = BedrockConverseReferenceV1.stream_parser();
    let events = parser
        .parse_chunk(concat!(
            "event: contentBlockStart\n",
            "data: {\"contentBlockIndex\":3,\"start\":{\"reasoningContent\":{}}}\n\n",
            "event: contentBlockDelta\n",
            "data: {\"contentBlockIndex\":3,\"delta\":{\"reasoningContent\":{\"text\":\"a\"}}}\n\n",
            "event: contentBlockDelta\n",
            "data: {\"contentBlockIndex\":3,\"delta\":{\"reasoningContent\":{\"signature\":\"sig\"}}}\n\n",
            "event: contentBlockStop\n",
            "data: {\"contentBlockIndex\":3}\n\n",
            "event: contentBlockStart\n",
            "data: {\"contentBlockIndex\":5,\"start\":{\"reasoningContent\":{}}}\n\n",
            "event: contentBlockDelta\n",
            "data: {\"contentBlockIndex\":5,\"delta\":{\"reasoningContent\":{\"redactedContent\":\"opaque\"}}}\n\n"
        ).as_bytes())
        .unwrap();
    assert_eq!(
        events,
        vec![
            StreamEvent::ThinkingDelta { index: 0, block_index: 3, thinking_delta: "a".into() },
            StreamEvent::ThinkingSignatureDelta {
                index: 0,
                block_index: 3,
                signature_delta: "sig".into(),
            },
            StreamEvent::RedactedThinking { index: 0, block_index: 5, data: "opaque".into() },
        ]
    );
}
