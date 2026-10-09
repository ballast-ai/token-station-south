//! The judge gate ② cannot be: every shipped provider reference, held to the
//! kernel's `Usage` contract rather than to itself.
//!
//! # Why this layer exists
//!
//! Gate ② proves "wasm ≡ native reference", and the two are one source. A
//! reference that maps a provider's usage wrongly therefore passes gate ② with
//! its wrong answer written into its own fixtures — which is how two of the
//! three non-OpenAI references came to report the *uncached* prompt as the IR's
//! `input_tokens`, and how the Converse reference came to refuse every real
//! cached response (its fixture's `totalTokens` was written to match the
//! reference, not AWS).
//!
//! The contract, from kernel `Usage::total`: cache counts **partition**
//! `input_tokens` rather than extend it. `input_tokens` is the whole prompt;
//! `cache_read_tokens` and `cache_write_tokens` are subsets of it.
//!
//! Two layers:
//!
//! 1. **Per provider, against the provider's own documentation.** Each case is
//!    a wire body whose numbers are chosen so that the documented prompt total
//!    differs from every single wire field — a mapping that copies the wrong
//!    field cannot land on it by accident. The expected total is derived from
//!    the provider's published semantics (cited per case), never from the
//!    reference.
//! 2. **Every shipped provider fixture.** The prompt total is recomputed from
//!    each fixture's *input wire* by the dialect's documented formula and must
//!    equal the expected IR `input_tokens`. A weaker "cache ≤ input" sweep was
//!    tried first and passed over the old, wrong fixtures (120 uncached vs 90 +
//!    30 cached) — a gate born green. This one fails on them.
//!
//! A third layer (B1, `docs/design/2026-09-30-host-zero-vendor-boundary.md`
//! §6.2 items 1 and 6) judges the output side and strictness: `reasoning_tokens`
//! is a subset of `output_tokens` in every dialect; Gemini's thoughts lie outside
//! its candidates (measured 2026-10-01, that record's §16 Q13), so its IR output
//! is candidates + thoughts; and a 2xx whose usage is missing or does not add up
//! is a protocol error, never a zero (provider-adapter.wit, `parse-response`).

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::{
    ProviderComponentV1,
    reference::OpenAiCompatibleReferenceV1,
    reference_anthropic::AnthropicReferenceV1,
    reference_anthropic_bedrock_invoke::AnthropicBedrockInvokeReferenceV1,
    reference_bedrock_converse::{BedrockConverseBearerReferenceV1, BedrockConverseReferenceV1},
    reference_gemini::GeminiReferenceV1,
};
use token_station_protocol::{HttpResponseParts, StreamEvent, Usage};

fn response(body: &Value) -> HttpResponseParts {
    serde_json::from_value(json!({"status":200,"headers":{},"body":body.to_string()})).unwrap()
}

/// The usage a consumer ends up with: every report folded the kernel's way.
fn folded(component: &dyn ProviderComponentV1, chunks: &[&str]) -> Usage {
    let mut parser = component.stream_parser();
    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(parser.parse_chunk(chunk.as_bytes()).expect("chunk parses"));
    }
    events.extend(parser.finish().expect("stream finishes"));
    let mut usage = Usage::default();
    for event in events {
        if let StreamEvent::Usage { usage: report } = event {
            usage.absorb(report);
        }
    }
    usage
}

/// The contract, and the documented prompt total.
fn assert_partitioned(usage: Usage, prompt: u64, read: u64, write: u64, what: &str) {
    assert_eq!(usage.input_tokens, prompt, "{what}: IR input_tokens must be the whole prompt");
    assert_eq!(usage.cache_read_tokens, read, "{what}: cache read bucket");
    assert_eq!(usage.cache_write_tokens, write, "{what}: cache write bucket");
    assert!(
        read + write <= usage.input_tokens,
        "{what}: the cache buckets partition input_tokens, they cannot exceed it"
    );
}

// ── OpenAI-compatible ────────────────────────────────────────────────────────
// OpenAI: `prompt_tokens` is the whole prompt; `prompt_tokens_details.
// cached_tokens` is the part served from cache (a subset).

#[test]
fn openai_prompt_tokens_already_contain_the_cached_part() {
    let body = json!({
        "id": "c", "object": "chat.completion", "model": "m",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"},
                     "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 1000, "completion_tokens": 7, "total_tokens": 1007,
                  "prompt_tokens_details": {"cached_tokens": 300}}
    });
    let usage = OpenAiCompatibleReferenceV1.parse_response(&response(&body)).unwrap().usage;
    assert_partitioned(usage, 1000, 300, 0, "openai non-stream");

    let usage = folded(
        &OpenAiCompatibleReferenceV1,
        &[
            "data: {\"id\":\"c\",\"object\":\"chat.completion.chunk\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"id\":\"c\",\"object\":\"chat.completion.chunk\",\"model\":\"m\",\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":7,\"total_tokens\":1007,\"prompt_tokens_details\":{\"cached_tokens\":300}}}\n\n",
            "data: [DONE]\n\n",
        ],
    );
    assert_partitioned(usage, 1000, 300, 0, "openai stream");
}

// ── Anthropic Messages ───────────────────────────────────────────────────────
// Anthropic: `input_tokens` counts only the tokens after the last cache
// breakpoint; `cache_read_input_tokens` and `cache_creation_input_tokens` sit
// beside it. Total input = input_tokens + cache_read + cache_creation.

#[test]
fn anthropic_input_tokens_exclude_both_cache_buckets() {
    let body = json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn", "stop_sequence": null,
        "usage": {"input_tokens": 500, "output_tokens": 20,
                  "cache_read_input_tokens": 300, "cache_creation_input_tokens": 200}
    });
    let usage = AnthropicReferenceV1.parse_response(&response(&body)).unwrap().usage;
    assert_partitioned(usage, 1000, 300, 200, "anthropic non-stream");
    assert_eq!(usage.output_tokens, 20);
}

#[test]
fn anthropic_stream_reports_the_whole_prompt_however_the_frames_split_it() {
    let start = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude\",\"content\":[],\"usage\":{\"input_tokens\":500,\"output_tokens\":1,\"cache_read_input_tokens\":300,\"cache_creation_input_tokens\":200}}}\n\n";
    let block = concat!(
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    );
    let stop = "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

    // The documented shape: output only on the terminal delta.
    let usage = folded(
        &AnthropicReferenceV1,
        &[
            start,
            block,
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":20}}\n\n",
            stop,
        ],
    );
    assert_partitioned(usage, 1000, 300, 200, "anthropic stream, output-only delta");
    assert_eq!(usage.output_tokens, 20);

    // A terminal delta that repeats `input_tokens` without the cache fields:
    // a per-frame sum would report 500 and, being nonzero, win the fold.
    let usage = folded(
        &AnthropicReferenceV1,
        &[
            start,
            block,
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"input_tokens\":500,\"output_tokens\":20}}\n\n",
            stop,
        ],
    );
    assert_partitioned(usage, 1000, 300, 200, "anthropic stream, delta repeats input only");
}

// Anthropic streaming documentation: the counts in `message_delta.usage` are cumulative for the
// whole message. A later report may repeat a count or raise it, never lower it; a count that
// shrinks is contradictory evidence and refused, as the host refuses it (its 03 #86 rule; I-Q7 of
// docs/design/2026-10-08-bedrock-invoke-anthropic-component.md).
#[test]
fn anthropic_stream_refuses_a_cumulative_count_that_shrinks() {
    let start = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1000,\"output_tokens\":1}}}\n\n";
    for (what, delta) in [
        ("input", "{\"input_tokens\":900,\"output_tokens\":20}"),
        ("output", "{\"output_tokens\":0}"),
    ] {
        let mut parser = AnthropicReferenceV1.stream_parser();
        parser.parse_chunk(start.as_bytes()).unwrap();
        let frame = format!(
            "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\"}},\"usage\":{delta}}}\n\n"
        );
        let outcome = parser.parse_chunk(frame.as_bytes());
        if what == "input" {
            assert_eq!(
                outcome.unwrap_err().code.as_str(),
                "provider_protocol_error",
                "a terminal input count below the start's must be refused"
            );
        } else {
            // Zero is "not filled in", never a shrink: the start's output stands.
            let events = outcome.unwrap();
            let usage = events.iter().find_map(|event| match event {
                StreamEvent::Usage { usage } => Some(*usage),
                _ => None,
            });
            assert_eq!(usage.map(|usage| usage.output_tokens), Some(1), "{events:?}");
        }
    }
}

// The cache-write total and its 5-minute / 1-hour split describe one bucket, so a later report
// replaces them together or not at all (the host's 03 #86 rule): a larger later total takes the
// later split, and an equal total takes it only when it brings a split the earlier report lacked.
// Folded field by field, a 300 / 0 start and a 0 / 500 terminal would bill 300 + 500 against a
// total of 500. Judged on the terminal report itself: each report carries the whole-so-far usage,
// and a consumer folding reports with the kernel's last-nonzero `Usage::absorb` cannot take a tier
// back to zero (design record §15.2).
#[test]
fn anthropic_stream_folds_the_cache_write_tiers_as_one_group() {
    let fold = |start: &Value, delta: &Value| -> Usage {
        let start = format!(
            "event: message_start\ndata: {}\n\n",
            json!({"type": "message_start", "message": {"usage": start}})
        );
        let delta = format!(
            "event: message_delta\ndata: {}\n\n",
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": delta})
        );
        let mut parser = AnthropicReferenceV1.stream_parser();
        let mut events = parser.parse_chunk(start.as_bytes()).expect("the start parses");
        events.extend(parser.parse_chunk(delta.as_bytes()).expect("the delta parses"));
        events
            .iter()
            .rev()
            .find_map(|event| match event {
                StreamEvent::Usage { usage } => Some(*usage),
                _ => None,
            })
            .expect("the terminal delta reports usage")
    };
    let tiers = |five: u64, hour: u64| json!({"ephemeral_5m_input_tokens": five, "ephemeral_1h_input_tokens": hour});
    let grown = fold(
        &json!({"input_tokens": 10, "output_tokens": 1, "cache_creation_input_tokens": 300,
                "cache_creation": tiers(300, 0)}),
        &json!({"output_tokens": 5, "cache_creation_input_tokens": 500,
                "cache_creation": tiers(0, 500)}),
    );
    assert_eq!(
        (grown.cache_write_tokens, grown.cache_write_5m_tokens, grown.cache_write_1h_tokens),
        (500, 0, 500)
    );
    let split_later = fold(
        &json!({"input_tokens": 10, "output_tokens": 1, "cache_creation_input_tokens": 300}),
        &json!({"output_tokens": 5, "cache_creation_input_tokens": 300,
                "cache_creation": tiers(100, 200)}),
    );
    assert_eq!((split_later.cache_write_5m_tokens, split_later.cache_write_1h_tokens), (100, 200));
    let kept = fold(
        &json!({"input_tokens": 10, "output_tokens": 1, "cache_creation_input_tokens": 300,
                "cache_creation": tiers(300, 0)}),
        &json!({"output_tokens": 5, "cache_creation_input_tokens": 300,
                "cache_creation": tiers(100, 200)}),
    );
    assert_eq!((kept.cache_write_5m_tokens, kept.cache_write_1h_tokens), (300, 0));
}

// ── Bedrock InvokeModel, Anthropic ───────────────────────────────────────────
// AWS (Anthropic Claude Messages API on Bedrock): the InvokeModel request and response bodies are
// the Messages API's, so the usage object and its formula are Anthropic's: total input =
// input_tokens + cache_read_input_tokens + cache_creation_input_tokens. A stream's `chunk` events
// carry each Anthropic stream event base64-encoded in `bytes` (`PayloadPart`).

fn invoke_chunk(event: &Value) -> String {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(event.to_string());
    format!("event: chunk\ndata: {}\n\n", json!({"bytes": encoded}))
}

#[test]
fn invoke_reads_the_messages_usage_and_its_whole_prompt() {
    let body = anthropic_body(&json!({"input_tokens": 500, "output_tokens": 20,
                                      "cache_read_input_tokens": 300,
                                      "cache_creation_input_tokens": 200}));
    let usage = AnthropicBedrockInvokeReferenceV1.parse_response(&response(&body)).unwrap().usage;
    assert_partitioned(usage, 1000, 300, 200, "invoke non-stream");

    let start = json!({"type": "message_start", "message": {"usage": {
        "input_tokens": 500, "output_tokens": 1, "cache_read_input_tokens": 300,
        "cache_creation_input_tokens": 200,
        "cache_creation": {"ephemeral_5m_input_tokens": 50, "ephemeral_1h_input_tokens": 150}}}});
    let block = [
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hi"}}),
        json!({"type": "content_block_stop", "index": 0}),
    ];
    let delta = json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
                       "usage": {"output_tokens": 20}});
    let chunks: Vec<String> =
        std::iter::once(&start).chain(&block).chain([&delta]).map(invoke_chunk).collect();
    let chunks: Vec<&str> = chunks.iter().map(String::as_str).collect();
    let usage = folded(&AnthropicBedrockInvokeReferenceV1, &chunks);
    assert_partitioned(usage, 1000, 300, 200, "invoke stream");
    assert_eq!(
        (usage.output_tokens, usage.cache_write_5m_tokens, usage.cache_write_1h_tokens),
        (20, 50, 150)
    );

    // The host's 03 #86 shape: input 0 at the start, the real input on the terminal delta.
    let start = json!({"type": "message_start", "message": {"usage": {"input_tokens": 0, "output_tokens": 0}}});
    let delta = json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
                       "usage": {"input_tokens": 700, "output_tokens": 20,
                                 "cache_read_input_tokens": 300}});
    let chunks: Vec<String> =
        std::iter::once(&start).chain(&block).chain([&delta]).map(invoke_chunk).collect();
    let chunks: Vec<&str> = chunks.iter().map(String::as_str).collect();
    let usage = folded(&AnthropicBedrockInvokeReferenceV1, &chunks);
    assert_partitioned(usage, 1000, 300, 0, "invoke stream, input on the terminal delta");

    let invoke = AnthropicBedrockInvokeReferenceV1;
    refused(&invoke, &anthropic_body(&Value::Null), "invoke, no usage object");
    refused(&invoke, &anthropic_body(&json!({"input_tokens": 10})), "invoke, no output_tokens");
}

// ── Gemini ───────────────────────────────────────────────────────────────────
// Gemini: `promptTokenCount` is the whole prompt; `cachedContentTokenCount` is
// the cached part of it (a subset).

#[test]
fn gemini_prompt_token_count_already_contains_the_cached_part() {
    let body = json!({
        "candidates": [{"content": {"role": "model", "parts": [{"text": "hi"}]},
                        "finishReason": "STOP"}],
        "usageMetadata": {"promptTokenCount": 1000, "candidatesTokenCount": 7,
                          "cachedContentTokenCount": 300, "totalTokenCount": 1007}
    });
    let usage = GeminiReferenceV1.parse_response(&response(&body)).unwrap().usage;
    assert_partitioned(usage, 1000, 300, 0, "gemini non-stream");

    let usage = folded(
        &GeminiReferenceV1,
        &[
            "data: {\"candidates\": [{\"content\": {\"role\": \"model\", \"parts\": [{\"text\": \"hi\"}]}, \"finishReason\": \"STOP\"}], \"usageMetadata\": {\"promptTokenCount\": 1000, \"candidatesTokenCount\": 7, \"cachedContentTokenCount\": 300, \"totalTokenCount\": 1007}}\n\n",
        ],
    );
    assert_partitioned(usage, 1000, 300, 0, "gemini stream");
}

// ── Bedrock Converse ─────────────────────────────────────────────────────────
// AWS (prompt caching guide): "`inputTokens` represents only the non-cached
// input tokens ... total input tokens = inputTokens + cacheReadInputTokens +
// cacheWriteInputTokens", and `totalTokens` is total input plus output — a
// real cached capture reads 10 + 4 + 5848 == 5862.

/// Both Converse packages, the signed one and its Bearer sibling (SF16): one wire, one judge.
const CONVERSE_PACKAGES: [&dyn ProviderComponentV1; 2] =
    [&BedrockConverseReferenceV1, &BedrockConverseBearerReferenceV1];

#[test]
fn converse_input_tokens_exclude_both_cache_buckets_and_total_counts_them() {
    let usage_wire = json!({"inputTokens": 500, "outputTokens": 20, "totalTokens": 1020,
                            "cacheReadInputTokens": 300, "cacheWriteInputTokens": 200});
    let body = json!({
        "output": {"message": {"role": "assistant", "content": [{"text": "hi"}]}},
        "stopReason": "end_turn",
        "usage": usage_wire,
    });
    for component in CONVERSE_PACKAGES {
        let usage = component.parse_response(&response(&body)).unwrap().usage;
        assert_partitioned(usage, 1000, 300, 200, "converse non-stream");

        let usage = folded(
            component,
            &[
                "event: messageStart\ndata: {\"role\": \"assistant\"}\n\n",
                "event: contentBlockDelta\ndata: {\"contentBlockIndex\": 0, \"delta\": {\"text\": \"hi\"}}\n\n",
                "event: contentBlockStop\ndata: {\"contentBlockIndex\": 0}\n\n",
                "event: messageStop\ndata: {\"stopReason\": \"end_turn\"}\n\n",
                &format!("event: metadata\ndata: {}\n\n", json!({"usage": usage_wire})),
            ],
        );
        assert_partitioned(usage, 1000, 300, 200, "converse stream");
    }
}

#[test]
fn converse_refuses_a_total_that_leaves_out_the_cache_buckets() {
    // The shape the reference once demanded. It never occurs with a cache
    // bucket set, and accepting it would mean the report is inconsistent.
    let body = json!({
        "output": {"message": {"role": "assistant", "content": [{"text": "hi"}]}},
        "stopReason": "end_turn",
        "usage": {"inputTokens": 500, "outputTokens": 20, "totalTokens": 520,
                  "cacheReadInputTokens": 300, "cacheWriteInputTokens": 200},
    });
    for component in CONVERSE_PACKAGES {
        assert!(component.parse_response(&response(&body)).is_err());
    }
}

// ── Every shipped provider fixture ───────────────────────────────────────────

/// How one dialect's wire states the whole prompt, per its documentation.
struct Dialect {
    dir: &'static str,
    /// The wire usage object's key.
    usage_key: &'static str,
    /// Wire fields that sum to the whole prompt.
    prompt: &'static [&'static str],
    /// Wire fields that carry a cache bucket.
    cache: &'static [&'static str],
}

const DIALECTS: [Dialect; 6] = [
    // `prompt_tokens` already contains `prompt_tokens_details.cached_tokens`.
    Dialect {
        dir: "fixtures",
        usage_key: "usage",
        prompt: &["prompt_tokens"],
        cache: &["cached_tokens"],
    },
    // `input_tokens` is the uncached remainder; the cache buckets sit beside it.
    Dialect {
        dir: "fixtures-anthropic",
        usage_key: "usage",
        prompt: &["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"],
        cache: &["cache_read_input_tokens", "cache_creation_input_tokens"],
    },
    // `promptTokenCount` already contains `cachedContentTokenCount`; the
    // tool-use prompt is billed as input beside it.
    Dialect {
        dir: "fixtures-gemini",
        usage_key: "usageMetadata",
        prompt: &["promptTokenCount", "toolUsePromptTokenCount"],
        cache: &["cachedContentTokenCount"],
    },
    // `inputTokens` is the uncached remainder (AWS prompt caching guide).
    Dialect {
        dir: "fixtures-bedrock-converse",
        usage_key: "usage",
        prompt: &["inputTokens", "cacheReadInputTokens", "cacheWriteInputTokens"],
        cache: &["cacheReadInputTokens", "cacheWriteInputTokens"],
    },
    // The Bearer sibling reads the same wire (SF16).
    Dialect {
        dir: "fixtures-bedrock-converse-bearer",
        usage_key: "usage",
        prompt: &["inputTokens", "cacheReadInputTokens", "cacheWriteInputTokens"],
        cache: &["cacheReadInputTokens", "cacheWriteInputTokens"],
    },
    // InvokeModel carries the Messages usage; stream events arrive base64-wrapped in `bytes`.
    Dialect {
        dir: "fixtures-anthropic-bedrock-invoke",
        usage_key: "usage",
        prompt: &["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"],
        cache: &["cache_read_input_tokens", "cache_creation_input_tokens"],
    },
];

/// The JSON payloads a fixture input puts on the wire: a response body, or
/// every `data:` line of every stream chunk, with an `InvokeModel` `{"bytes": <base64>}` envelope
/// opened (by the `base64` crate, not by the reference's decoder).
fn wire_payloads(input: &Value) -> Vec<Value> {
    use base64::Engine as _;
    if let Some(body) = input["body"].as_str() {
        return serde_json::from_str(body).into_iter().collect();
    }
    let chunks = input["chunks"].as_array().map(Vec::as_slice).unwrap_or_default();
    chunks
        .iter()
        .filter_map(Value::as_str)
        .flat_map(str::lines)
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
        .filter_map(|payload| {
            let Some(encoded) = payload["bytes"].as_str() else {
                return Some(payload);
            };
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .ok()
                .and_then(|decoded| serde_json::from_slice(&decoded).ok())
        })
        .collect()
}

/// Every object under `key`, anywhere in `value`, in document order.
fn objects_under(value: &Value, key: &str, out: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            for (name, child) in map {
                if name == key && child.is_object() {
                    out.push(child.clone());
                }
                objects_under(child, key, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|child| objects_under(child, key, out)),
        _ => {}
    }
}

/// A wire field anywhere in a usage object (`OpenAI` nests `cached_tokens`).
fn wire_count(usage: &Value, field: &str) -> u64 {
    match usage {
        Value::Object(map) => map.get(field).and_then(Value::as_u64).unwrap_or_else(|| {
            map.values().map(|child| wire_count(child, field)).find(|&n| n != 0).unwrap_or(0)
        }),
        _ => 0,
    }
}

/// Wire usage folded the way a stream reports it (last nonzero wins per
/// field), then summed per the dialect's documented prompt formula. `None`
/// when the input carries no usage at all.
fn documented_prompt(dialect: &Dialect, input: &Value) -> Option<(u64, u64)> {
    let mut reports = Vec::new();
    for payload in wire_payloads(input) {
        objects_under(&payload, dialect.usage_key, &mut reports);
    }
    if reports.is_empty() {
        return None;
    }
    let fold = |field: &str| {
        reports.iter().rev().map(|usage| wire_count(usage, field)).find(|&n| n != 0).unwrap_or(0)
    };
    let prompt = dialect.prompt.iter().map(|field| fold(field)).sum();
    let cache = dialect.cache.iter().map(|field| fold(field)).sum();
    Some((prompt, cache))
}

/// The IR usage a fixture expects, folded as a consumer folds it.
fn expected_input_tokens(expected: &Value) -> Option<u64> {
    let mut reports = Vec::new();
    objects_under(expected, "usage", &mut reports);
    let counts: Vec<u64> =
        reports.iter().filter_map(|usage| usage["input_tokens"].as_u64()).collect();
    if counts.is_empty() {
        return None;
    }
    Some(counts.into_iter().rev().find(|&n| n != 0).unwrap_or(0))
}

#[test]
fn every_shipped_provider_fixture_reports_the_documented_prompt_as_input_tokens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for dialect in &DIALECTS {
        let dir = root.join(dialect.dir);
        let mut inputs: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("{}: {error}", dialect.dir))
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                let name = path.file_name().unwrap().to_string_lossy();
                (name.starts_with("provider.response.") || name.starts_with("provider.stream."))
                    && name.ends_with(".input.json")
            })
            .collect();
        inputs.sort();
        let (mut checked, mut with_cache) = (0, 0);
        for input_path in inputs {
            let name = input_path.file_name().unwrap().to_string_lossy().into_owned();
            let expected_path = dir.join(name.replace(".input.json", ".expected.json"));
            let read = |path: &Path| -> Value {
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
            };
            let (input, expected) = (read(&input_path), read(&expected_path));
            let (Some((prompt, cache)), Some(reported)) =
                (documented_prompt(dialect, &input), expected_input_tokens(&expected))
            else {
                continue;
            };
            assert_eq!(
                reported,
                prompt,
                "{}/{name}: the IR's input_tokens must be the whole prompt as the provider \
                 documents it ({} summed), not a single wire field",
                dialect.dir,
                dialect.prompt.join(" + ")
            );
            checked += 1;
            with_cache += usize::from(cache > 0);
        }
        // A sweep that found nothing to judge proves nothing, and one that
        // never met a cache bucket cannot tell a partition from a sum.
        assert!(checked > 0, "{}: no fixture carried usage", dialect.dir);
        assert!(with_cache > 0, "{}: no fixture carried a cache bucket", dialect.dir);
    }
}

// ── Output, reasoning and strictness (B1) ────────────────────────────────────

fn openai_body(usage: &Value) -> Value {
    json!({
        "id": "c", "object": "chat.completion", "model": "m",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"},
                     "finish_reason": "stop"}],
        "usage": usage,
    })
}

fn anthropic_body(usage: &Value) -> Value {
    json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn", "stop_sequence": null,
        "usage": usage,
    })
}

fn gemini_body(usage: &Value) -> Value {
    json!({
        "candidates": [{"content": {"role": "model", "parts": [{"text": "hi"}]},
                        "finishReason": "STOP"}],
        "usageMetadata": usage,
    })
}

fn refused(component: &dyn ProviderComponentV1, body: &Value, what: &str) {
    let mut body = body.clone();
    if body["usage"].is_null() && body["usageMetadata"].is_null() {
        // The `null` placeholder of the builders stands for "no usage object".
        if let Some(map) = body.as_object_mut() {
            map.remove("usage");
            map.remove("usageMetadata");
        }
    }
    let error = component
        .parse_response(&response(&body))
        .err()
        .unwrap_or_else(|| panic!("{what}: a usage report that is not exact must be refused"));
    assert_eq!(error.code.as_str(), "provider_protocol_error", "{what}");
}

// OpenAI: `completion_tokens_details.reasoning_tokens` is the reasoning part of
// `completion_tokens`, and `total_tokens = prompt_tokens + completion_tokens`.
#[test]
fn openai_reasoning_is_a_subset_of_output_and_the_report_must_add_up() {
    let usage = OpenAiCompatibleReferenceV1
        .parse_response(&response(&openai_body(&json!({
            "prompt_tokens": 12, "completion_tokens": 50, "total_tokens": 62,
            "completion_tokens_details": {"reasoning_tokens": 41}
        }))))
        .unwrap()
        .usage;
    assert_eq!((usage.output_tokens, usage.reasoning_tokens), (50, 41));

    let ai = OpenAiCompatibleReferenceV1;
    refused(&ai, &openai_body(&Value::Null), "openai, no usage object");
    refused(
        &ai,
        &openai_body(&json!({"prompt_tokens": 12, "total_tokens": 12})),
        "openai, no completion_tokens",
    );
    refused(
        &ai,
        &openai_body(&json!({"prompt_tokens": 12, "completion_tokens": 50})),
        "openai, no total_tokens",
    );
    refused(
        &ai,
        &openai_body(&json!({"prompt_tokens": 12, "completion_tokens": 50, "total_tokens": 61})),
        "openai, total does not add up",
    );
    refused(
        &ai,
        &openai_body(&json!({"prompt_tokens": 12, "completion_tokens": 5, "total_tokens": 17,
                             "completion_tokens_details": {"reasoning_tokens": 6}})),
        "openai, reasoning larger than output",
    );
    refused(
        &ai,
        &openai_body(&json!({"prompt_tokens": 12, "completion_tokens": 5, "total_tokens": 17,
                             "prompt_tokens_details": {"cached_tokens": 13}})),
        "openai, cached larger than prompt",
    );
}

// Sakana Fugu reports sub-model consumption in `*_tokens_details.
// orchestration_*` on top of the top-level counts; its `total_tokens` is
// `prompt + completion + orchestration_input`. Bailian reports explicit cache
// writes as `prompt_tokens_details.cache_creation_input_tokens`.
#[test]
fn openai_compatible_folds_orchestration_and_reads_both_cache_write_keys() {
    let usage = OpenAiCompatibleReferenceV1
        .parse_response(&response(&openai_body(&json!({
            "prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 420,
            "prompt_tokens_details": {"orchestration_input_tokens": 300,
                                      "orchestration_input_cached_tokens": 50},
            "completion_tokens_details": {"orchestration_output_tokens": 40}
        }))))
        .unwrap()
        .usage;
    assert_eq!((usage.input_tokens, usage.output_tokens, usage.cache_read_tokens), (400, 60, 50));

    let usage = OpenAiCompatibleReferenceV1
        .parse_response(&response(&openai_body(&json!({
            "prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120,
            "prompt_tokens_details": {"cached_tokens": 30, "cache_creation_input_tokens": 60}
        }))))
        .unwrap()
        .usage;
    assert_partitioned(usage, 100, 30, 60, "bailian explicit cache");
}

// Anthropic: a message reports both sides; a stream reports input in
// `message_start` and output in `message_delta`. `cache_creation` splits
// `cache_creation_input_tokens` into the 5-minute and 1-hour tiers.
#[test]
fn anthropic_refuses_missing_counts_and_maps_the_cache_write_tiers() {
    let usage = AnthropicReferenceV1
        .parse_response(&response(&anthropic_body(&json!({
            "input_tokens": 10, "output_tokens": 3, "cache_creation_input_tokens": 300,
            "cache_creation": {"ephemeral_5m_input_tokens": 100, "ephemeral_1h_input_tokens": 200}
        }))))
        .unwrap()
        .usage;
    assert_partitioned(usage, 310, 0, 300, "anthropic cache tiers");
    assert_eq!((usage.cache_write_5m_tokens, usage.cache_write_1h_tokens), (100, 200));

    let anthropic = AnthropicReferenceV1;
    refused(&anthropic, &anthropic_body(&Value::Null), "anthropic, no usage object");
    refused(
        &anthropic,
        &anthropic_body(&json!({"input_tokens": 10})),
        "anthropic, no output_tokens",
    );
    refused(
        &anthropic,
        &anthropic_body(&json!({"output_tokens": 3})),
        "anthropic, no input_tokens",
    );
    refused(
        &anthropic,
        &anthropic_body(&json!({
            "input_tokens": 10, "output_tokens": 3, "cache_creation_input_tokens": 300,
            "cache_creation": {"ephemeral_5m_input_tokens": 100, "ephemeral_1h_input_tokens": 100}
        })),
        "anthropic, cache tiers do not add up",
    );

    let mut parser = AnthropicReferenceV1.stream_parser();
    parser
        .parse_chunk(b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude\",\"content\":[],\"usage\":{\"input_tokens\":5,\"output_tokens\":1}}}\n\n")
        .unwrap();
    assert!(
        parser
            .parse_chunk(b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":5}}\n\n")
            .is_err(),
        "anthropic stream, a terminal usage without output_tokens must be refused"
    );
}

// Gemini, measured on Vertex AI 2026-10-01 (`gemini-2.5-flash`, thinking budget
// 512): prompt 32, candidates 6, thoughts 286, total 324. Thoughts lie outside
// the candidates, so output is 6 + 286.
#[test]
fn gemini_output_counts_the_thoughts_the_candidates_leave_out() {
    let measured = json!({"promptTokenCount": 32, "candidatesTokenCount": 6,
                          "thoughtsTokenCount": 286, "totalTokenCount": 324});
    let usage = GeminiReferenceV1.parse_response(&response(&gemini_body(&measured))).unwrap().usage;
    assert_eq!((usage.input_tokens, usage.output_tokens, usage.reasoning_tokens), (32, 292, 286));

    let usage = folded(
        &GeminiReferenceV1,
        &[&format!(
            "data: {}\n\n",
            json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "hi"}]},
                                   "finishReason": "STOP"}],
                   "usageMetadata": measured})
        )],
    );
    assert_eq!((usage.output_tokens, usage.reasoning_tokens), (292, 286), "gemini stream");

    // The tool-use prompt is input; a zero candidates count is omitted on the
    // wire and is zero only when the total closes without it.
    let usage = GeminiReferenceV1
        .parse_response(&response(&gemini_body(&json!({
            "promptTokenCount": 32, "toolUsePromptTokenCount": 8,
            "thoughtsTokenCount": 512, "totalTokenCount": 552
        }))))
        .unwrap()
        .usage;
    assert_eq!((usage.input_tokens, usage.output_tokens, usage.reasoning_tokens), (40, 512, 512));

    let gemini = GeminiReferenceV1;
    refused(&gemini, &gemini_body(&Value::Null), "gemini, no usageMetadata");
    refused(
        &gemini,
        &gemini_body(&json!({"promptTokenCount": 32, "candidatesTokenCount": 6})),
        "gemini, no totalTokenCount",
    );
    refused(
        &gemini,
        &gemini_body(&json!({"promptTokenCount": 32, "candidatesTokenCount": 6,
                             "totalTokenCount": 38 + 286, "thoughtsTokenCount": 285})),
        "gemini, total does not add up",
    );
    refused(
        &gemini,
        &gemini_body(&json!({"promptTokenCount": 32, "thoughtsTokenCount": 286,
                             "totalTokenCount": 324})),
        "gemini, candidates unreported while the total says there were some",
    );
    refused(
        &gemini,
        &gemini_body(&json!({"promptTokenCount": 32, "candidatesTokenCount": 6,
                             "cachedContentTokenCount": 33, "totalTokenCount": 38})),
        "gemini, cached larger than prompt",
    );
}

/// Every shipped fixture's expected usage keeps reasoning inside output.
#[test]
fn every_shipped_provider_fixture_keeps_reasoning_inside_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut with_reasoning = 0;
    for dialect in &DIALECTS {
        for entry in std::fs::read_dir(root.join(dialect.dir)).unwrap() {
            let path = entry.unwrap().path();
            if !path.to_string_lossy().ends_with(".expected.json") {
                continue;
            }
            let expected: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let mut reports = Vec::new();
            objects_under(&expected, "usage", &mut reports);
            for usage in reports {
                let reasoning = usage["reasoning_tokens"].as_u64().unwrap_or(0);
                let output = usage["output_tokens"].as_u64().unwrap_or(0);
                assert!(
                    reasoning <= output,
                    "{}: reasoning_tokens {reasoning} exceeds output_tokens {output}",
                    path.display()
                );
                with_reasoning += usize::from(reasoning > 0);
            }
        }
    }
    assert!(with_reasoning > 0, "no shipped fixture carried reasoning tokens");
}
