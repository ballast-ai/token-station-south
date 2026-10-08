//! The single strict JSON boundary for embeddings-v1 values and the guest ABI shims.

use proptest::prelude::*;
use serde_json::{Value, json};
use south_component_conformance::abi_embeddings::{
    build_embeddings_request_json, map_provider_error_json, parse_embeddings_response_json,
};
use south_component_conformance::embeddings_json::{
    MAX_EMBEDDINGS_FACT_JSON_BYTES, embeddings_parsed_json, parse_embeddings_parsed_json,
    parse_prepared_embeddings_json, parse_provider_error_json, prepared_embeddings_json,
    provider_error_json,
};
use south_component_conformance::reference_gemini_embeddings::GeminiEmbeddingsReferenceV1;
use south_component_conformance::reference_openai_compatible_embeddings::OpenAiCompatibleEmbeddingsReferenceV1;
use south_component_conformance::{EmbeddingsComponentV1, PreparedEmbeddingsV1};
use south_contracts::{
    EmbeddingsEstimateV1, EmbeddingsFailureOutcomeV1, EmbeddingsParsedV1, EmbeddingsRequestV1,
    EmbeddingsUsageFactsV1, JsonPointerV1, MAX_EMBEDDINGS_PARSE_CONTEXT_BYTES,
    MAX_JSON_REQUEST_BODY_BYTES, UsageSourceV1, VectorLocatorV1,
};
use token_station_protocol::{
    ErrorCode, ErrorEnvelope, HttpMethod, HttpRequestDescriptor, HttpResponseParts, ProviderConfig,
};

const PREPARED: &str = r#"{"descriptor":{"method":"POST","url":"https://api.openai.com/v1/embeddings","headers":{"content-type":"application/json"},"body":{"input":"hi","model":"m"},"auth":{"scheme":"bearer","secret":"provider_api_key"}},"estimate":{"fallback_input_tokens":null,"max_input_tokens":null},"vectors":{"kind":"north_identical"},"immutable_body_paths":["input","model"],"parse_context":null}"#;

fn golden(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn prepared(context: Value, paths: Option<Vec<String>>) -> PreparedEmbeddingsV1 {
    PreparedEmbeddingsV1 {
        descriptor: HttpRequestDescriptor::new(HttpMethod::Post, "https://upstream.example/e"),
        estimate: EmbeddingsEstimateV1::new(Some(3), Some(3)),
        vectors: VectorLocatorV1::Array {
            array: JsonPointerV1::parse("/embeddings").unwrap(),
            vector: JsonPointerV1::parse("/values").unwrap(),
            index: None,
        },
        immutable_body_paths: paths,
        parse_context: context,
    }
}

proptest! {
    #[test]
    fn arbitrary_codec_input_never_panics_and_accepted_values_roundtrip(input in "(?s).{0,2048}") {
        if let Ok(value) = parse_prepared_embeddings_json(&input) {
            let wire = prepared_embeddings_json(&value).unwrap();
            prop_assert_eq!(parse_prepared_embeddings_json(&wire.to_string()), Ok(value));
        }
        if let Ok(value) = parse_embeddings_parsed_json(&input) {
            let wire = embeddings_parsed_json(&value).unwrap();
            prop_assert_eq!(parse_embeddings_parsed_json(&wire.to_string()), Ok(value));
        }
        if let Ok((outcome, error)) = parse_provider_error_json(&input) {
            let wire = provider_error_json(outcome, &error).unwrap();
            prop_assert_eq!(parse_provider_error_json(&wire.to_string()), Ok((outcome, error)));
        }
    }

    #[test]
    fn parsed_facts_roundtrip(tokens in any::<u64>(), count in any::<u32>(), model in proptest::option::of("[a-z0-9-]{1,24}")) {
        let parsed = EmbeddingsParsedV1::new(EmbeddingsUsageFactsV1::reported(tokens), count, model);
        let wire = embeddings_parsed_json(&parsed).unwrap();
        prop_assert_eq!(parse_embeddings_parsed_json(&wire.to_string()), Ok(parsed));
    }
}

#[test]
fn prepared_wire_shape_is_pinned_and_roundtrips() {
    let value = parse_prepared_embeddings_json(PREPARED).unwrap();
    assert_eq!(value.vectors, VectorLocatorV1::NorthIdentical);
    assert_eq!(value.parse_context, Value::Null);
    assert_eq!(prepared_embeddings_json(&value).unwrap(), golden(PREPARED));
    let gemini = prepared(json!({"shape":"batch"}), Some(vec![]));
    let wire = prepared_embeddings_json(&gemini).unwrap();
    assert_eq!(
        wire["vectors"],
        json!({"kind":"array","array":"/embeddings","vector":"/values","index":null})
    );
    assert_eq!(wire["estimate"], json!({"fallback_input_tokens":3,"max_input_tokens":3}));
    assert_eq!(parse_prepared_embeddings_json(&wire.to_string()), Ok(gemini));
}

#[test]
fn prepared_frames_refuse_unknown_and_missing_keys() {
    let base: Value = serde_json::from_str(PREPARED).unwrap();
    let mut unknown = base.clone();
    unknown["reserve"] = json!(1);
    assert!(parse_prepared_embeddings_json(&unknown.to_string()).is_err());
    for key in ["descriptor", "estimate", "vectors", "immutable_body_paths", "parse_context"] {
        let mut missing = base.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(parse_prepared_embeddings_json(&missing.to_string()).is_err(), "{key}");
    }
    let mut estimate = base.clone();
    estimate["estimate"]["reserve"] = json!(1);
    assert!(parse_prepared_embeddings_json(&estimate.to_string()).is_err());
    let mut locator = base;
    locator["vectors"] = json!({"kind":"single","vector":"embedding"});
    assert!(parse_prepared_embeddings_json(&locator.to_string()).is_err());
}

#[test]
fn immutable_paths_use_contract_six_validation() {
    for invalid in [
        vec!["a..b".to_owned()],
        vec!["input".to_owned(), "input".to_owned()],
        vec!["requests[0]".to_owned()],
    ] {
        let value = prepared(Value::Null, Some(invalid.clone()));
        assert!(prepared_embeddings_json(&value).is_err(), "{invalid:?}");
        let mut wire: Value = serde_json::from_str(PREPARED).unwrap();
        wire["immutable_body_paths"] = json!(invalid);
        assert!(parse_prepared_embeddings_json(&wire.to_string()).is_err(), "{invalid:?}");
    }
    assert!(prepared_embeddings_json(&prepared(Value::Null, None)).is_ok());
    assert!(prepared_embeddings_json(&prepared(Value::Null, Some(vec!["a.b-c_1".into()]))).is_ok());
}

#[test]
fn parse_context_is_bounded_by_its_serialized_length() {
    // `"` + text + `"` is the serialized context.
    let at_bound = Value::String("x".repeat(MAX_EMBEDDINGS_PARSE_CONTEXT_BYTES - 2));
    assert_eq!(at_bound.to_string().len(), MAX_EMBEDDINGS_PARSE_CONTEXT_BYTES);
    let value = prepared(at_bound, None);
    let wire = prepared_embeddings_json(&value).unwrap();
    assert_eq!(parse_prepared_embeddings_json(&wire.to_string()), Ok(value));
    let over = Value::String("x".repeat(MAX_EMBEDDINGS_PARSE_CONTEXT_BYTES - 1));
    assert!(prepared_embeddings_json(&prepared(over.clone(), None)).is_err());
    let mut wire: Value = serde_json::from_str(PREPARED).unwrap();
    wire["parse_context"] = over;
    assert!(parse_prepared_embeddings_json(&wire.to_string()).is_err());
}

#[test]
fn canonical_prepared_frame_must_fit_before_decode_succeeds() {
    let empty = PREPARED.replace(r#""input":"hi""#, r#""input":"","x":1e8"#);
    let limit = MAX_JSON_REQUEST_BODY_BYTES;
    let input = empty
        .replace(r#""input":"""#, &format!(r#""input":"{}""#, "x".repeat(limit - empty.len())));
    assert_eq!(input.len(), limit);
    assert!(
        parse_prepared_embeddings_json(&input).is_err(),
        "the canonical descriptor frame must fit the same boundary"
    );
}

#[test]
fn parsed_frames_are_strict_and_bounded() {
    let golden = r#"{"usage":{"source":"not_reported","input_tokens":null,"per_input_tokens":null},"vector_count":3,"upstream_model":null}"#;
    let parsed = parse_embeddings_parsed_json(golden).unwrap();
    assert_eq!(parsed.usage().source(), UsageSourceV1::NotReported);
    assert_eq!(
        embeddings_parsed_json(&parsed).unwrap(),
        serde_json::from_str::<Value>(golden).unwrap()
    );
    for refused in [
        r#"{"usage":{"source":"reported","input_tokens":null,"per_input_tokens":null},"vector_count":1,"upstream_model":null}"#,
        r#"{"usage":{"source":"not_reported","input_tokens":null,"per_input_tokens":null},"vector_count":-1,"upstream_model":null}"#,
        r#"{"usage":{"source":"not_reported","input_tokens":null,"per_input_tokens":null},"vector_count":1,"upstream_model":null,"vectors":[]}"#,
    ] {
        assert!(parse_embeddings_parsed_json(refused).is_err(), "{refused}");
    }
    let padded = golden.replace(
        r#""upstream_model":null"#,
        &format!(r#""upstream_model":"{}""#, "m".repeat(MAX_EMBEDDINGS_FACT_JSON_BYTES)),
    );
    assert!(parse_embeddings_parsed_json(&padded).is_err());
}

#[test]
fn provider_error_frames_are_strict() {
    let golden = r#"{"outcome":"rejected","error":{"code":"auth","http_status":401,"message":"the upstream rejected the credential"}}"#;
    let (outcome, error) = parse_provider_error_json(golden).unwrap();
    assert_eq!(outcome, EmbeddingsFailureOutcomeV1::Rejected);
    assert_eq!(error.code, ErrorCode::Auth);
    assert_eq!(
        provider_error_json(outcome, &error).unwrap(),
        serde_json::from_str::<Value>(golden).unwrap()
    );
    for refused in [
        r#"{"outcome":"not_reported","error":{"code":"auth","http_status":401,"message":"m"}}"#,
        r#"{"outcome":"unknown"}"#,
        r#"{"outcome":"unknown","error":{"code":"quota_exhausted","http_status":402,"message":"m"}}"#,
        r#"{"outcome":"unknown","error":{"code":"internal","http_status":500,"message":"m"},"x":1}"#,
    ] {
        assert!(parse_provider_error_json(refused).is_err(), "{refused}");
    }
}

fn config() -> String {
    json!({"provider":"gemini","base_url":"https://generativelanguage.googleapis.com","auth":"provider_api_key"})
        .to_string()
}

#[test]
fn the_guest_shims_cross_the_boundary_through_the_codec() {
    let component = GeminiEmbeddingsReferenceV1;
    let request = r#"{"model":"gemini-embedding-001","inputs":[{"text":"abcd"}],"input_shape":"single","dimensions":null,"encoding_format":null,"user":null,"extra":{}}"#;
    let built = build_embeddings_request_json(&component, &config(), request).unwrap();
    let native = component
        .build_embeddings_request(
            &serde_json::from_str::<ProviderConfig>(&config()).unwrap(),
            &serde_json::from_str::<EmbeddingsRequestV1>(request).unwrap(),
        )
        .unwrap();
    assert_eq!(parse_prepared_embeddings_json(&built), Ok(native));

    let parts = json!({"status":200,"body":r#"{"embedding":{"values":null}}"#}).to_string();
    let parsed = parse_embeddings_response_json(&component, &parts, "null").unwrap();
    assert_eq!(parse_embeddings_parsed_json(&parsed).unwrap().vector_count(), 1);

    let failure = json!({"status":503,"body":""}).to_string();
    let mapped = map_provider_error_json(&component, &failure).unwrap();
    let (outcome, error) = parse_provider_error_json(&mapped).unwrap();
    assert_eq!(outcome, EmbeddingsFailureOutcomeV1::Unknown);
    assert_eq!(error.code, ErrorCode::UpstreamUnavailable);
}

#[test]
fn the_guest_shims_answer_bad_input_with_an_error_envelope() {
    let component = OpenAiCompatibleEmbeddingsReferenceV1;
    let envelope = |raw: String| serde_json::from_str::<ErrorEnvelope>(&raw).unwrap();
    // A request the contract refuses never reaches the component.
    let smuggled = r#"{"model":"m","inputs":[{"text":"a"}],"input_shape":"single","dimensions":null,"encoding_format":null,"user":null,"extra":{"$south.ref":1}}"#;
    let refused = build_embeddings_request_json(&component, &config(), smuggled).unwrap_err();
    assert_eq!(envelope(refused).code, ErrorCode::Internal);
    let refused = build_embeddings_request_json(&component, "{", smuggled).unwrap_err();
    assert_eq!(envelope(refused).code, ErrorCode::Internal);
    // A component refusal travels as its own envelope.
    let mixed = r#"{"model":"m","inputs":[{"text":"a"},{"token_ids":[1]}],"input_shape":"array","dimensions":null,"encoding_format":null,"user":null,"extra":{}}"#;
    let openai = json!({"provider":"openai-compatible","base_url":"https://api.openai.com/v1"});
    let refused =
        build_embeddings_request_json(&component, &openai.to_string(), mixed).unwrap_err();
    assert_eq!(envelope(refused).code, ErrorCode::Capability);
    // An oversized parse context is refused before the component sees it.
    let context = Value::String("x".repeat(MAX_EMBEDDINGS_PARSE_CONTEXT_BYTES)).to_string();
    let parts = json!({"status":200,"body":"{}"}).to_string();
    let refused = parse_embeddings_response_json(&component, &parts, &context).unwrap_err();
    assert_eq!(envelope(refused).code, ErrorCode::Internal);
    // A 2xx without usage is a provider protocol error, never a zero.
    let parts = json!({"status":200,"body":r#"{"data":[{"embedding":null}]}"#}).to_string();
    let refused = parse_embeddings_response_json(&component, &parts, "null").unwrap_err();
    assert_eq!(envelope(refused).code, ErrorCode::ProviderProtocolError);
    let _: HttpResponseParts = serde_json::from_str(&parts).unwrap();
}
