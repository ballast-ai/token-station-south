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
    ]
}
