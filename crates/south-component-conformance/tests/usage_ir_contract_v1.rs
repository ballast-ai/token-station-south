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

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::{
    ProviderComponentV1, reference::OpenAiCompatibleReferenceV1,
    reference_anthropic::AnthropicReferenceV1,
    reference_bedrock_converse::BedrockConverseReferenceV1, reference_gemini::GeminiReferenceV1,
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

#[test]
fn converse_input_tokens_exclude_both_cache_buckets_and_total_counts_them() {
    let usage_wire = json!({"inputTokens": 500, "outputTokens": 20, "totalTokens": 1020,
                            "cacheReadInputTokens": 300, "cacheWriteInputTokens": 200});
    let body = json!({
        "output": {"message": {"role": "assistant", "content": [{"text": "hi"}]}},
        "stopReason": "end_turn",
        "usage": usage_wire,
    });
    let usage = BedrockConverseReferenceV1.parse_response(&response(&body)).unwrap().usage;
    assert_partitioned(usage, 1000, 300, 200, "converse non-stream");

    let usage = folded(
        &BedrockConverseReferenceV1,
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
    assert!(BedrockConverseReferenceV1.parse_response(&response(&body)).is_err());
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

const DIALECTS: [Dialect; 4] = [
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
    // `promptTokenCount` already contains `cachedContentTokenCount`.
    Dialect {
        dir: "fixtures-gemini",
        usage_key: "usageMetadata",
        prompt: &["promptTokenCount"],
        cache: &["cachedContentTokenCount"],
    },
    // `inputTokens` is the uncached remainder (AWS prompt caching guide).
    Dialect {
        dir: "fixtures-bedrock-converse",
        usage_key: "usage",
        prompt: &["inputTokens", "cacheReadInputTokens", "cacheWriteInputTokens"],
        cache: &["cacheReadInputTokens", "cacheWriteInputTokens"],
    },
];

/// The JSON payloads a fixture input puts on the wire: a response body, or
/// every `data:` line of every stream chunk.
fn wire_payloads(input: &Value) -> Vec<Value> {
    if let Some(body) = input["body"].as_str() {
        return serde_json::from_str(body).into_iter().collect();
    }
    let chunks = input["chunks"].as_array().map(Vec::as_slice).unwrap_or_default();
    chunks
        .iter()
        .filter_map(Value::as_str)
        .flat_map(str::lines)
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str(data.trim()).ok())
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
