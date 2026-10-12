//! A request with none of the canonical IR 4 inputs decodes to the same IR, byte for byte, as
//! before KC1 (record `docs/design/2026-10-12-kc1-canonical-ir-4-south.md` §7.1). The literals
//! were taken from the codecs at the re-pin commit, before the decode change.

use serde_json::json;
use south_north_codec::{chat_request_from_anthropic_messages, chat_request_from_openai_chat};

#[test]
fn an_unmarked_messages_request_is_unchanged_byte_for_byte() {
    let body = json!({
        "model": "claude-x", "max_tokens": 256, "temperature": 0.5, "stream": true,
        "system": [{"type": "text", "text": "rules"}, {"type": "text", "text": "more"}],
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "weather?"},
                {"type": "image", "source": {"type": "url", "url": "https://x.test/i.png"}}]},
            {"role": "assistant", "content": [{"type": "text", "text": "checking"},
                {"type": "tool_use", "id": "t1", "name": "w", "input": {"city": "Paris"}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1",
                "content": [{"type": "text", "text": "18C"}, {"type": "text", "text": "sunny"}]}]}
        ],
        "tools": [{"name": "w", "description": "weather", "input_schema": {"type": "object"}}],
        "tool_choice": {"type": "auto"}, "stop_sequences": ["END"]
    });
    let ir = serde_json::to_string(&chat_request_from_anthropic_messages(&body).unwrap()).unwrap();
    assert_eq!(ir, MESSAGES_GOLDEN);
}

#[test]
fn an_unmarked_chat_request_is_unchanged_byte_for_byte() {
    let body = json!({
        "model": "gpt-x", "max_completion_tokens": 64, "top_p": 0.9, "seed": 7,
        "messages": [{"role": "system", "content": "rules"},
                     {"role": "user", "content": [{"type": "text", "text": "hi"},
                        {"type": "image_url", "image_url": {"url": "https://x.test/i.png"}}]}],
        "tools": [{"type": "function", "function": {"name": "f", "parameters": {"type": "object"}}}],
        "tool_choice": "auto"
    });
    let ir = serde_json::to_string(&chat_request_from_openai_chat(&body).unwrap()).unwrap();
    assert_eq!(ir, CHAT_GOLDEN);
}

const MESSAGES_GOLDEN: &str = concat!(
    r#"{"model":"claude-x","messages":[{"role":"system","content":"rules\nmore"},{"role":"user",""#,
    r#"content":[{"type":"text","text":"weather?"},{"type":"image_url","image_url":{"url":"https:"#,
    r#"//x.test/i.png"}}]},{"role":"assistant","content":[{"type":"text","text":"checking"}],"too"#,
    r#"l_calls":[{"id":"t1","name":"w","arguments":"{\"city\":\"Paris\"}"}]},{"role":"tool","cont"#,
    r#"ent":"18C\nsunny","tool_call_id":"t1"}],"tools":[{"name":"w","description":"weather","para"#,
    r#"meters":{"type":"object"}}],"tool_choice":"auto","sampling":{"temperature":0.5,"max_output"#,
    r#"_tokens":256,"stop":["END"]},"stream":true}"#,
);
const CHAT_GOLDEN: &str = concat!(
    r#"{"model":"gpt-x","messages":[{"role":"system","content":"rules"},{"role":"user","content":"#,
    r#"[{"type":"text","text":"hi"},{"type":"image_url","image_url":{"url":"https://x.test/i.png""#,
    r#"}}]}],"tools":[{"name":"f","parameters":{"type":"object"}}],"tool_choice":"auto","sampling"#,
    r#"":{"top_p":0.9,"max_output_tokens":64},"stream":false,"seed":7}"#,
);
