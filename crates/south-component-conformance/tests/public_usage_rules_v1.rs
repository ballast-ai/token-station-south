//! The two usage rules a host's own component may call: the `OpenAI` Responses terminal
//! `response.usage` and Gemini `usageMetadata`.
//!
//! Design record: `docs/design/2026-11-26-public-usage-rules.md`. The references call the same
//! functions, so these tests fix the public contract: the counts that come out, and that every
//! refusal is a 502 `provider_protocol_error` whose text names the upstream or its wire, never a
//! product.

use serde_json::{Value, json};
use south_component_conformance::{reference_gemini, reference_openai_responses};
use token_station_protocol::{ErrorCode, ErrorEnvelope, Usage};

fn refused(result: Result<Usage, ErrorEnvelope>) -> String {
    let error = result.expect_err("the usage must be refused");
    assert_eq!(error.code, ErrorCode::ProviderProtocolError);
    assert_eq!(error.http_status, 502);
    assert!(
        error.message.starts_with("the upstream ")
            || error.message.starts_with("a Gemini response "),
        "{}",
        error.message
    );
    error.message
}

// -- OpenAI Responses --------------------------------------------------------

fn responses(usage: &Value) -> Result<Usage, ErrorEnvelope> {
    reference_openai_responses::usage_of(Some(usage))
}

#[test]
fn responses_usage_maps_every_count() {
    let usage = responses(&json!({
        "input_tokens": 100,
        "output_tokens": 40,
        "total_tokens": 140,
        "input_tokens_details": {"cached_tokens": 30, "cache_write_tokens": 20},
        "output_tokens_details": {"reasoning_tokens": 25}
    }))
    .expect("an exact report is accepted");
    assert_eq!(
        usage,
        Usage {
            input_tokens: 100,
            output_tokens: 40,
            cache_read_tokens: 30,
            cache_write_tokens: 20,
            reasoning_tokens: 25,
            ..Usage::default()
        }
    );
}

#[test]
fn responses_usage_reads_absent_or_null_details_as_zero() {
    for details in [json!(null), json!({}), json!({"cached_tokens": null})] {
        let usage = responses(&json!({
            "input_tokens": 3,
            "output_tokens": 2,
            "total_tokens": 5,
            "input_tokens_details": details,
            "output_tokens_details": null
        }))
        .expect("absent or null details read zero");
        assert_eq!((usage.cache_read_tokens, usage.reasoning_tokens), (0, 0));
    }
}

#[test]
fn responses_usage_refuses_what_it_cannot_count() {
    assert!(refused(reference_openai_responses::usage_of(None)).contains("no usage object"));
    assert!(refused(responses(&json!("12"))).contains("no usage object"));
    for (usage, names) in [
        (json!({"output_tokens": 2, "total_tokens": 2}), "input_tokens"),
        (json!({"input_tokens": 1, "total_tokens": 1}), "output_tokens"),
        (json!({"input_tokens": 1, "output_tokens": 2}), "total_tokens"),
        (json!({"input_tokens": -1, "output_tokens": 2, "total_tokens": 1}), "input_tokens"),
        (json!({"input_tokens": 1, "output_tokens": 2, "total_tokens": 4}), "total_tokens"),
        (
            json!({"input_tokens": 1, "output_tokens": 2, "total_tokens": 3,
                   "input_tokens_details": [1]}),
            "not an object",
        ),
        (
            json!({"input_tokens": 1, "output_tokens": 2, "total_tokens": 3,
                   "input_tokens_details": {"cached_tokens": "1"}}),
            "invalid token count",
        ),
        (
            json!({"input_tokens": 4, "output_tokens": 2, "total_tokens": 6,
                   "input_tokens_details": {"cached_tokens": 3, "cache_write_tokens": 2}}),
            "exceed input_tokens",
        ),
        (
            json!({"input_tokens": 1, "output_tokens": 2, "total_tokens": 3,
                   "output_tokens_details": {"reasoning_tokens": 3}}),
            "exceeds output_tokens",
        ),
    ] {
        let message = refused(responses(&usage));
        assert!(message.contains(names), "{usage}: {message}");
    }
}

// -- Gemini ------------------------------------------------------------------

#[test]
fn gemini_usage_moves_thoughts_into_output_and_tool_prompts_into_input() {
    let usage = reference_gemini::usage_of(&json!({
        "promptTokenCount": 100,
        "toolUsePromptTokenCount": 10,
        "cachedContentTokenCount": 60,
        "candidatesTokenCount": 30,
        "thoughtsTokenCount": 20,
        "totalTokenCount": 160
    }))
    .expect("an exact report is accepted");
    assert_eq!(
        usage,
        Usage {
            input_tokens: 110,
            output_tokens: 50,
            cache_read_tokens: 60,
            reasoning_tokens: 20,
            ..Usage::default()
        }
    );
}

#[test]
fn gemini_usage_reads_an_omitted_candidates_count_as_zero_only_when_the_total_closes() {
    let usage = reference_gemini::usage_of(&json!({
        "promptTokenCount": 7,
        "thoughtsTokenCount": null,
        "totalTokenCount": 7
    }))
    .expect("the total closes without candidates");
    assert_eq!((usage.input_tokens, usage.output_tokens), (7, 0));

    let message = refused(reference_gemini::usage_of(&json!({
        "promptTokenCount": 7,
        "totalTokenCount": 9
    })));
    assert!(message.contains("lacks candidatesTokenCount"), "{message}");
}

#[test]
fn gemini_usage_refuses_what_it_cannot_count() {
    for (meta, names) in [
        (json!(null), "must carry usageMetadata"),
        (json!({"totalTokenCount": 1}), "promptTokenCount or totalTokenCount"),
        (json!({"promptTokenCount": 1}), "promptTokenCount or totalTokenCount"),
        (
            json!({"promptTokenCount": 1, "candidatesTokenCount": "1", "totalTokenCount": 2}),
            "invalid token count",
        ),
        (
            json!({"promptTokenCount": 1, "cachedContentTokenCount": 2, "totalTokenCount": 1}),
            "exceeds promptTokenCount",
        ),
        (
            json!({"promptTokenCount": 1, "candidatesTokenCount": 1, "totalTokenCount": 3}),
            "does not equal",
        ),
        (
            json!({"promptTokenCount": u64::MAX, "thoughtsTokenCount": 1, "totalTokenCount": 1}),
            "overflow",
        ),
    ] {
        let message = refused(reference_gemini::usage_of(&meta));
        assert!(message.contains(names), "{meta}: {message}");
    }
}
