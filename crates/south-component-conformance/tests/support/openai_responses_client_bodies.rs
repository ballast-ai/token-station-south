/// The `OpenAI` Responses client bodies the codec-derived request rows of
/// `fixtures-openai-responses/` come from: each row's `chat_request` is what `south-north-codec`
/// parses from the body (the record §9: "a request fixture for this package is a north codec
/// output"). Shared by `openai_responses_vocabulary_v1`, which re-derives the rows.
fn client_bodies() -> Vec<(&'static str, serde_json::Value)> {
    use serde_json::json;
    let weather = json!({"type": "function", "name": "get_weather",
        "description": "Current weather for a city.",
        "parameters": {"type": "object", "properties": {"city": {"type": "string"}},
                       "required": ["city"]}});
    let mut strict_weather = weather.clone();
    strict_weather["strict"] = json!(true);
    vec![
        (
            "text",
            json!({"model": "gpt-5.5", "input": "Weather in Beijing?", "temperature": 0.2,
                   "max_output_tokens": 256}),
        ),
        (
            "stream-with-cap",
            json!({"model": "gpt-5.5", "input": "Tell me a joke.", "max_output_tokens": 128,
                   "stream": true}),
        ),
        (
            "instructions-and-history",
            json!({"model": "gpt-5.5", "instructions": "You are terse.",
                   "input": [
                       {"role": "developer", "content": "Answer in English."},
                       {"role": "user", "content": "Hi"},
                       {"role": "assistant",
                        "content": [{"type": "output_text", "text": "Hello! How can I help?"}]},
                       {"role": "system", "content": "Keep it under fifty words."},
                       {"role": "user", "content": [{"type": "input_text", "text": "Summarize "},
                                                    {"type": "input_text", "text": "our chat."}]}
                   ]}),
        ),
        (
            "tools-and-tool-choice",
            json!({"model": "gpt-5.5", "input": "Weather and time in Paris?",
                   "tools": [strict_weather,
                             {"type": "function", "name": "get_time", "strict": false,
                              "parameters": {"type": "object",
                                             "properties": {"city": {"type": "string"}}}}],
                   "tool_choice": {"type": "function", "name": "get_weather"},
                   "parallel_tool_calls": false}),
        ),
        (
            "structured-output",
            json!({"model": "gpt-5.5", "input": "Alice and Bob meet on Friday for lunch.",
                   "text": {"format": {"type": "json_schema", "name": "calendar_event",
                                       "description": "One calendar entry.", "strict": true,
                                       "schema": {"type": "object",
                                                  "properties": {"name": {"type": "string"},
                                                                 "date": {"type": "string"}},
                                                  "required": ["name", "date"],
                                                  "additionalProperties": false}}}}),
        ),
        (
            "json-object-output",
            json!({"model": "gpt-5.5", "input": "Return the colors as JSON.",
                   "text": {"format": {"type": "json_object"}}, "tool_choice": "none",
                   "tools": [weather]}),
        ),
        (
            "reasoning-effort",
            json!({"model": "gpt-5.5", "input": "Prove that there are infinitely many primes.",
                   "reasoning": {"effort": "high", "summary": "auto"}, "top_p": 0.9}),
        ),
        // R3 record §4.3: the four client fields the IR has no typed slot for.
        (
            "request-fields",
            json!({"model": "gpt-5.5", "input": "Summarize our last meeting.", "user": "user-42",
                   "metadata": {"tenant": "acme", "trace": "t-1"},
                   "prompt_cache_key": "meetings-acme", "truncation": "auto"}),
        ),
    ]
}

/// The `OpenAI` Responses client bodies the codec-derived NC-1 replay rows come from (design
/// record `2026-10-11-responses-reasoning-replay-and-passthrough-suite.md` §7 items 6 and 7). The
/// rows' model declares `reasoning_replay.openai.v1`, so their `provider_config` is the declared
/// one plus that word; `openai_responses_vocabulary_v1` re-derives the rows.
fn replay_client_bodies() -> Vec<(&'static str, serde_json::Value)> {
    use serde_json::json;
    let reasoning = |id: &str, summary: &str, encrypted: &str| {
        json!({"type": "reasoning", "id": id,
               "summary": [{"type": "summary_text", "text": summary}],
               "encrypted_content": encrypted})
    };
    vec![
        (
            "reasoning-replay",
            json!({"model": "gpt-5.5", "input": [
                {"role": "user", "content": "Is 91 prime?"},
                reasoning("rs_68af0a1d1a2b8190b3c4d5e6f7a8b9c0", "7 x 13 = 91.",
                          "gAAAAABo-replay-one"),
                {"type": "message", "role": "assistant",
                 "content": [{"type": "output_text", "text": "No, 91 = 7 x 13."}]},
                {"role": "user", "content": "And 97?"}
            ]}),
        ),
        (
            "reasoning-replay-interleaved",
            json!({"model": "gpt-5.5", "input": [
                {"role": "user", "content": "Weather in Paris, then summarize."},
                reasoning("rs_1", "Need the weather first.", "gAAAAABo-interleaved-one"),
                {"type": "function_call", "call_id": "call_1", "name": "get_weather",
                 "arguments": "{\"city\":\"Paris\"}"},
                reasoning("rs_2", "Also the time.", "gAAAAABo-interleaved-two"),
                {"type": "function_call", "call_id": "call_2", "name": "get_time",
                 "arguments": "{\"city\":\"Paris\"}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "18C, clear"},
                {"type": "function_call_output", "call_id": "call_2", "output": "14:05"},
                reasoning("rs_3", "Both answers are in.", "gAAAAABo-interleaved-three"),
                {"type": "message", "role": "assistant",
                 "content": [{"type": "output_text", "text": "Paris: 18C and clear at 14:05."}]},
                {"role": "user", "content": "Thanks."}
            ]}),
        ),
        (
            "reasoning-replay-multi-turn",
            json!({"model": "gpt-5.5", "input": [
                {"role": "user", "content": "Pick a prime."},
                reasoning("rs_a", "Any prime works.", "gAAAAABo-turn-one"),
                {"type": "message", "role": "assistant",
                 "content": [{"type": "output_text", "text": "13."}]},
                {"role": "user", "content": "Another one, larger."},
                {"type": "message", "role": "assistant",
                 "content": [{"type": "output_text", "text": "Let me think."}]},
                reasoning("rs_b", "Larger than 13.", "gAAAAABo-turn-two"),
                {"type": "message", "role": "assistant",
                 "content": [{"type": "output_text", "text": "17."}]},
                {"role": "user", "content": "Their product?"}
            ]}),
        ),
        (
            "reasoning-replay-include-only",
            json!({"model": "gpt-5.5", "input": "Is 97 prime?"}),
        ),
    ]
}
