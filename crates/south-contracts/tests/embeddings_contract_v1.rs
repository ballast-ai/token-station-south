//! Golden vectors for embeddings contract 1 (docs/design/2026-09-30-embeddings-contract.md):
//! the §3 northbound parsing table under the §15 scope, the §5 locator, detection and erasure,
//! the §8 rendering conversions and the serde wire shapes two hosts and the guests share.

use proptest::prelude::*;
use serde_json::{Map, Value, json};
use south_contracts::{
    EMBEDDINGS_CONTRACT_VERSION, EmbeddingInputV1, EmbeddingsContractErrorV1, EmbeddingsEstimateV1,
    EmbeddingsFailureOutcomeV1, EmbeddingsParsedV1, EmbeddingsRequestErrorV1, EmbeddingsRequestV1,
    EmbeddingsUsageFactsV1, EncodingV1, InputShapeV1, JsonPointerV1,
    MAX_BINARY_RESPONSE_BODY_BYTES, MAX_EMBEDDING_INPUTS, MAX_EMBEDDINGS_EXTRA_BYTES,
    MAX_EMBEDDINGS_EXTRA_FIELDS, MAX_EMBEDDINGS_PARSE_CONTEXT_BYTES, MAX_VECTOR_POINTER_BYTES,
    UsageSourceV1, VectorLocatorV1, check_embeddings_response_v1, extract_vectors_v1,
    media_type_of, parse_embeddings_request_v1, render_vectors_v1,
};

fn parse(body: &Value) -> Result<EmbeddingsRequestV1, EmbeddingsRequestErrorV1> {
    parse_embeddings_request_v1(body, "upstream-model")
}

fn text(value: &str) -> EmbeddingInputV1 {
    EmbeddingInputV1::Text(value.to_owned())
}

fn pointer(value: &str) -> JsonPointerV1 {
    JsonPointerV1::parse(value).unwrap()
}

fn render_text(
    vectors: &[south_contracts::EmbeddingVectorV1],
    encoding: EncodingV1,
    model: &str,
    tokens: u64,
) -> String {
    String::from_utf8(render_vectors_v1(vectors, encoding, model, tokens).unwrap()).unwrap()
}

fn north_body(embeddings: &[Value], prompt_tokens: Option<u64>) -> Vec<u8> {
    let data: Vec<Value> = embeddings
        .iter()
        .enumerate()
        .map(|(index, embedding)| json!({"object":"embedding","index":index,"embedding":embedding}))
        .collect();
    let mut body = json!({"object":"list","data":data,"model":"upstream-echo"});
    if let Some(tokens) = prompt_tokens {
        body["usage"] = json!({"prompt_tokens":tokens,"total_tokens":tokens});
    }
    serde_json::to_vec(&body).unwrap()
}

#[test]
fn constants_match_the_record() {
    assert_eq!(EMBEDDINGS_CONTRACT_VERSION, 1);
    assert_eq!(MAX_EMBEDDING_INPUTS, 2048);
    assert_eq!(MAX_EMBEDDINGS_PARSE_CONTEXT_BYTES, 8 * 1024);
    assert_eq!(MAX_EMBEDDINGS_EXTRA_FIELDS, 32);
    assert_eq!(MAX_EMBEDDINGS_EXTRA_BYTES, 16 * 1024);
}

// §3 parsing table, row 1: a string is one input of shape `Single`.
#[test]
fn table_string_is_one_single_input() {
    let request = parse(&json!({"model":"north-name","input":"hello"})).unwrap();
    assert_eq!(request.inputs(), &[text("hello")]);
    assert_eq!(request.input_shape(), InputShapeV1::Single);
    assert_eq!(request.model(), "upstream-model");
    assert_eq!(request.encoding_format(), EncodingV1::Float);
    assert_eq!(request.dimensions(), None);
    assert_eq!(request.user(), None);
    assert!(request.extra().is_empty());
}

// Row 2: an array of strings is one input per string, in order, of shape `Array`.
#[test]
fn table_array_of_strings_is_one_input_each_in_order() {
    let request = parse(&json!({"input":["b","a","c"]})).unwrap();
    assert_eq!(request.inputs(), &[text("b"), text("a"), text("c")]);
    assert_eq!(request.input_shape(), InputShapeV1::Array);
    let one = parse(&json!({"input":["only"]})).unwrap();
    assert_eq!(one.inputs(), &[text("only")]);
    assert_eq!(one.input_shape(), InputShapeV1::Array);
}

// Row 3: an array of integers is ONE token-id input of shape `Single`.
#[test]
fn table_array_of_integers_is_one_token_id_input() {
    let request = parse(&json!({"input":[0, 7, 4_294_967_295_u64]})).unwrap();
    assert_eq!(request.inputs(), &[EmbeddingInputV1::TokenIds(vec![0, 7, u32::MAX])]);
    assert_eq!(request.input_shape(), InputShapeV1::Single);
    // The count bound is on inputs, not on ids within one sequence.
    let long = parse(&json!({"input": vec![1; MAX_EMBEDDING_INPUTS + 1]})).unwrap();
    assert_eq!(long.inputs().len(), 1);
}

// Row 4: an array of arrays of integers is one token-id input per inner array.
#[test]
fn table_array_of_integer_arrays_is_one_input_each() {
    let request = parse(&json!({"input":[[1,2],[3]]})).unwrap();
    assert_eq!(
        request.inputs(),
        &[EmbeddingInputV1::TokenIds(vec![1, 2]), EmbeddingInputV1::TokenIds(vec![3])]
    );
    assert_eq!(request.input_shape(), InputShapeV1::Array);
}

// Row 5: everything else is a 400 before admission.
#[test]
fn table_everything_else_is_refused() {
    let invalid = Err(EmbeddingsRequestErrorV1::InvalidInput);
    for input in [
        json!(""),
        json!([]),
        json!([[]]),
        json!([[1], []]),
        json!(["a", 1]),
        json!([1, "a"]),
        json!([[1], "a"]),
        json!(["a", ["b"]]),
        json!(["a", ""]),
        json!([1.5]),
        json!([1.0]),
        json!([[2.5]]),
        json!([-1]),
        json!([[-1]]),
        json!([4_294_967_296_u64]),
        json!([[4_294_967_296_u64]]),
        json!([true]),
        json!([null]),
        json!([{"text":"a"}]),
        json!(null),
        json!(7),
        json!({"text":"a"}),
    ] {
        assert_eq!(parse(&json!({"input": input})), invalid, "{input}");
    }
    assert_eq!(parse(&json!({"model":"m"})), invalid);
}

#[test]
fn table_more_than_the_input_bound_is_refused() {
    let at_bound = vec!["x"; MAX_EMBEDDING_INPUTS];
    assert_eq!(parse(&json!({"input": at_bound})).unwrap().inputs().len(), MAX_EMBEDDING_INPUTS);
    let too_many = Err(EmbeddingsRequestErrorV1::TooManyInputs);
    assert_eq!(parse(&json!({"input": vec!["x"; MAX_EMBEDDING_INPUTS + 1]})), too_many);
    assert_eq!(parse(&json!({"input": vec![[1]; MAX_EMBEDDING_INPUTS + 1]})), too_many);
}

#[test]
fn body_and_model_are_checked_first() {
    for body in [json!([]), json!("input"), json!(null)] {
        assert_eq!(parse(&body), Err(EmbeddingsRequestErrorV1::InvalidBody));
    }
    assert_eq!(
        parse_embeddings_request_v1(&json!({"input":"a"}), ""),
        Err(EmbeddingsRequestErrorV1::InvalidModel)
    );
}

#[test]
fn refusal_order_is_shape_then_count_then_media() {
    let mut items = vec![json!("data:image/png;base64,AAAA"); MAX_EMBEDDING_INPUTS + 1];
    assert_eq!(parse(&json!({"input": items})), Err(EmbeddingsRequestErrorV1::TooManyInputs));
    items = vec![json!("data:image/png;base64,AAAA"), json!("")];
    assert_eq!(parse(&json!({"input": items})), Err(EmbeddingsRequestErrorV1::InvalidInput));
    items = vec![json!("data:image/png;base64,AAAA"), json!(1)];
    assert_eq!(parse(&json!({"input": items})), Err(EmbeddingsRequestErrorV1::InvalidInput));
    let media_with_bad_dimensions = json!({"input":"data:image/png;base64,AA","dimensions":0});
    assert_eq!(
        parse(&media_with_bad_dimensions),
        Err(EmbeddingsRequestErrorV1::MediaInputNotSupported)
    );
}

#[test]
fn media_form_is_refused_distinctly() {
    let media = Err(EmbeddingsRequestErrorV1::MediaInputNotSupported);
    for input in [
        json!("data:image/png;base64,iVBORw0KGgo="),
        json!("data:image/png;base64,"),
        json!("data:audio/wav;rate=16000;base64,AAAA"),
        json!("data:IMAGE/PNG;base64,AAAA"),
        json!(["hello", "data:video/mp4;base64,AAAA"]),
    ] {
        assert_eq!(parse(&json!({"input": input})), media, "{input}");
    }
}

#[test]
fn media_type_detection_golden_vectors() {
    for (input, expected) in [
        ("data:image/png;base64,AAAA", Some("image/png")),
        ("data:image/png;base64,", Some("image/png")),
        ("data:image/png;name=a.png;base64,AAAA", Some("image/png;name=a.png")),
        ("data:audio/L16;rate=16000;channels=1;base64,AA", Some("audio/L16;rate=16000;channels=1")),
        ("data:IMAGE/PNG;base64,AAAA", Some("IMAGE/PNG")),
        ("data:application/vnd.api+json;base64,e30=", Some("application/vnd.api+json")),
        // The FIRST `;base64,` ends the media type; the payload is never decoded.
        ("data:image/png;base64,abc;base64,def", Some("image/png")),
        ("data:image/png;base64,!!not base64!!", Some("image/png")),
        // Not base64: text.
        ("data:text/plain,abc", None),
        ("data:text/plain;charset=utf-8,abc", None),
        ("data:image/png;base64", None),
        // The prefix and marker are case-sensitive, as the native arm matches them.
        ("DATA:image/png;base64,AAAA", None),
        ("data:image/png;BASE64,AAAA", None),
        // The media type must be `type/subtype` with `;name=value` token parameters.
        ("data:image;base64,AAAA", None),
        ("data:image/;base64,AAAA", None),
        ("data:/png;base64,AAAA", None),
        ("data:;base64,AAAA", None),
        ("data:image/png/x;base64,AAAA", None),
        ("data:image/png;name;base64,AAAA", None),
        ("data:image/png;name=;base64,AAAA", None),
        ("data:image/png;=v;base64,AAAA", None),
        ("data:image/png; name=v;base64,AAAA", None),
        ("data:image/png;name=\"v\";base64,AAAA", None),
        ("data:image/png;;base64,AAAA", None),
        ("data:ima(ge/png;base64,AAAA", None),
        ("data:image /png;base64,AAAA", None),
        ("data:image/p\u{e9}g;base64,AAAA", None),
        (" data:image/png;base64,AAAA", None),
        ("hello", None),
    ] {
        assert_eq!(media_type_of(input), expected, "{input}");
        let parsed = parse(&json!({"input": input}));
        if expected.is_some() {
            assert_eq!(parsed, Err(EmbeddingsRequestErrorV1::MediaInputNotSupported), "{input}");
        } else {
            assert_eq!(parsed.unwrap().inputs(), &[text(input)], "{input}");
        }
    }
}

#[test]
fn dimensions_must_be_a_positive_u32() {
    for (value, expected) in [
        (json!(1), Ok(Some(1))),
        (json!(u32::MAX), Ok(Some(u32::MAX))),
        (json!(null), Ok(None)),
        (json!(0), Err(EmbeddingsRequestErrorV1::InvalidDimensions)),
        (json!(-1), Err(EmbeddingsRequestErrorV1::InvalidDimensions)),
        (json!(8.0), Err(EmbeddingsRequestErrorV1::InvalidDimensions)),
        (json!(1.5), Err(EmbeddingsRequestErrorV1::InvalidDimensions)),
        (json!("8"), Err(EmbeddingsRequestErrorV1::InvalidDimensions)),
        (json!(u64::from(u32::MAX) + 1), Err(EmbeddingsRequestErrorV1::InvalidDimensions)),
    ] {
        let parsed = parse(&json!({"input":"a","dimensions":value}));
        assert_eq!(parsed.map(|request| request.dimensions()), expected, "{value}");
    }
}

#[test]
fn encoding_format_is_float_by_default_or_base64() {
    for (value, expected) in [
        (None, Ok(EncodingV1::Float)),
        (Some(json!(null)), Ok(EncodingV1::Float)),
        (Some(json!("float")), Ok(EncodingV1::Float)),
        (Some(json!("base64")), Ok(EncodingV1::Base64)),
        (Some(json!("FLOAT")), Err(EmbeddingsRequestErrorV1::InvalidEncodingFormat)),
        (Some(json!("int8")), Err(EmbeddingsRequestErrorV1::InvalidEncodingFormat)),
        (Some(json!(1)), Err(EmbeddingsRequestErrorV1::InvalidEncodingFormat)),
    ] {
        let mut body = json!({"input":"a"});
        if let Some(value) = value {
            body["encoding_format"] = value;
        }
        assert_eq!(parse(&body).map(|request| request.encoding_format()), expected);
    }
}

#[test]
fn user_is_a_string_when_present() {
    assert_eq!(parse(&json!({"input":"a","user":"u-1"})).unwrap().user(), Some("u-1"));
    assert_eq!(parse(&json!({"input":"a","user":""})).unwrap().user(), Some(""));
    assert_eq!(parse(&json!({"input":"a","user":null})).unwrap().user(), None);
    assert_eq!(parse(&json!({"input":"a","user":7})), Err(EmbeddingsRequestErrorV1::InvalidUser));
}

#[test]
fn extra_carries_every_unmodelled_field_unchanged() {
    let body = json!({
        "model":"north-name","input":"a","dimensions":8,"encoding_format":"base64","user":"u",
        "input_type":"query","truncate":"END","nested":{"$south.ref":1,"k":[1,2.5,null]}
    });
    let request = parse(&body).unwrap();
    let expected: Map<String, Value> = json!({
        "input_type":"query","truncate":"END","nested":{"$south.ref":1,"k":[1,2.5,null]}
    })
    .as_object()
    .unwrap()
    .clone();
    assert_eq!(request.extra(), &expected);
}

#[test]
fn extra_field_count_bound() {
    let body = |count: usize| {
        let mut body = json!({"input":"a"});
        for key in 0..count {
            body[format!("k{key}")] = json!(key);
        }
        body
    };
    assert_eq!(parse(&body(MAX_EMBEDDINGS_EXTRA_FIELDS)).unwrap().extra().len(), 32);
    assert_eq!(
        parse(&body(MAX_EMBEDDINGS_EXTRA_FIELDS + 1)),
        Err(EmbeddingsRequestErrorV1::ExtraTooLarge)
    );
}

#[test]
fn extra_byte_bound_is_the_compact_serialized_object() {
    // `{"k":"` + value + `"}` is eight bytes of framing.
    let body = |len: usize| json!({"input":"a","k":"v".repeat(len)});
    let at_bound = parse(&body(MAX_EMBEDDINGS_EXTRA_BYTES - 8)).unwrap();
    assert_eq!(serde_json::to_vec(at_bound.extra()).unwrap().len(), MAX_EMBEDDINGS_EXTRA_BYTES);
    assert_eq!(
        parse(&body(MAX_EMBEDDINGS_EXTRA_BYTES - 7)),
        Err(EmbeddingsRequestErrorV1::ExtraTooLarge)
    );
}

#[test]
fn extra_key_with_the_reserved_prefix_is_refused() {
    assert_eq!(
        parse(&json!({"input":"a","$south.ref":{}})),
        Err(EmbeddingsRequestErrorV1::ReservedExtraField)
    );
    assert!(parse(&json!({"input":"a","$southx":1,"south.ref":1})).is_ok());
}

#[test]
fn request_wire_shape_is_pinned() {
    let request = parse(&json!({
        "input":[[1,2],[3]],"dimensions":256,"encoding_format":"base64","user":"u","input_type":"query"
    }))
    .unwrap();
    let golden = r#"{"model":"upstream-model","inputs":[{"token_ids":[1,2]},{"token_ids":[3]}],"input_shape":"array","dimensions":256,"encoding_format":"base64","user":"u","extra":{"input_type":"query"}}"#;
    assert_eq!(serde_json::to_string(&request).unwrap(), golden);
    assert_eq!(serde_json::from_str::<EmbeddingsRequestV1>(golden).unwrap(), request);

    let minimal = parse(&json!({"input":"hi"})).unwrap();
    let golden = r#"{"model":"upstream-model","inputs":[{"text":"hi"}],"input_shape":"single","dimensions":null,"encoding_format":"float","user":null,"extra":{}}"#;
    assert_eq!(serde_json::to_string(&minimal).unwrap(), golden);
    assert_eq!(serde_json::from_str::<EmbeddingsRequestV1>(golden).unwrap(), minimal);
}

#[test]
fn request_decoding_rechecks_every_parser_invariant() {
    let base = json!({
        "model":"m","inputs":[{"text":"a"}],"input_shape":"single","dimensions":null,
        "encoding_format":"float","user":null,"extra":{}
    });
    assert!(serde_json::from_value::<EmbeddingsRequestV1>(base.clone()).is_ok());
    for (field, value) in [
        ("unknown", json!(1)),
        ("model", json!("")),
        ("inputs", json!([])),
        ("inputs", json!([{"text":""}])),
        ("inputs", json!([{"token_ids":[]}])),
        ("inputs", json!([{"text":"data:image/png;base64,AA"}])),
        ("inputs", json!([{"text":"a"},{"text":"b"}])),
        ("inputs", json!([{"media":"a"}])),
        ("input_shape", json!("batch")),
        ("dimensions", json!(0)),
        ("encoding_format", json!("int8")),
        ("extra", json!({"$south.x":1})),
        ("extra", json!({"input":"smuggled"})),
        ("extra", json!({"model":"smuggled"})),
    ] {
        let mut body = base.clone();
        body[field] = value;
        assert!(serde_json::from_value::<EmbeddingsRequestV1>(body.clone()).is_err(), "{body}");
    }
}

#[test]
fn request_constructor_reports_the_same_errors_as_the_parser() {
    let build = |inputs: Vec<EmbeddingInputV1>, shape| {
        EmbeddingsRequestV1::new(
            "m".into(),
            inputs,
            shape,
            None,
            EncodingV1::Float,
            None,
            Map::new(),
        )
    };
    assert!(build(vec![text("a")], InputShapeV1::Single).is_ok());
    assert!(build(vec![text("a"), text("b")], InputShapeV1::Array).is_ok());
    assert_eq!(
        build(vec![text("a"), text("b")], InputShapeV1::Single),
        Err(EmbeddingsRequestErrorV1::InvalidInput)
    );
    assert_eq!(
        build(vec![text("data:image/png;base64,")], InputShapeV1::Single),
        Err(EmbeddingsRequestErrorV1::MediaInputNotSupported)
    );
    assert_eq!(
        build(vec![text("a"); MAX_EMBEDDING_INPUTS + 1], InputShapeV1::Array),
        Err(EmbeddingsRequestErrorV1::TooManyInputs)
    );
}

#[test]
fn request_debug_never_prints_inputs() {
    let request =
        parse(&json!({"input":["secret text"],"user":"secret user","k":"secret"})).unwrap();
    let debug = format!("{request:?}");
    assert!(!debug.contains("secret"), "{debug}");
}

#[test]
fn small_enums_wire_shapes_are_pinned() {
    for (value, golden) in [
        (serde_json::to_string(&InputShapeV1::Single), r#""single""#),
        (serde_json::to_string(&InputShapeV1::Array), r#""array""#),
        (serde_json::to_string(&EncodingV1::Float), r#""float""#),
        (serde_json::to_string(&EncodingV1::Base64), r#""base64""#),
        (serde_json::to_string(&UsageSourceV1::Reported), r#""reported""#),
        (serde_json::to_string(&UsageSourceV1::NotReported), r#""not_reported""#),
        (serde_json::to_string(&EmbeddingsFailureOutcomeV1::Rejected), r#""rejected""#),
        (serde_json::to_string(&EmbeddingsFailureOutcomeV1::Unknown), r#""unknown""#),
        (serde_json::to_string(&text("a")), r#"{"text":"a"}"#),
        (serde_json::to_string(&EmbeddingInputV1::TokenIds(vec![1, 2])), r#"{"token_ids":[1,2]}"#),
    ] {
        assert_eq!(value.unwrap(), golden);
    }
    assert_eq!(serde_json::from_str::<EmbeddingsFailureOutcomeV1>(r#""not_reported""#).ok(), None);
    assert!(serde_json::from_str::<EncodingV1>(r#""Float""#).is_err());
}

#[test]
fn estimate_wire_shape_is_pinned() {
    let estimate = EmbeddingsEstimateV1::new(Some(12), None);
    let golden = r#"{"fallback_input_tokens":12,"max_input_tokens":null}"#;
    assert_eq!(serde_json::to_string(&estimate).unwrap(), golden);
    assert_eq!(serde_json::from_str::<EmbeddingsEstimateV1>(golden).unwrap(), estimate);
    assert_eq!(estimate.fallback_input_tokens(), Some(12));
    assert_eq!(estimate.max_input_tokens(), None);
    assert!(
        serde_json::from_str::<EmbeddingsEstimateV1>(
            r#"{"fallback_input_tokens":1,"max_input_tokens":1,"reserve":1}"#
        )
        .is_err()
    );
}

#[test]
fn json_pointer_syntax() {
    for valid in ["", "/", "/data", "/a~0b~1c", "/0/embedding", "/~01"] {
        assert_eq!(JsonPointerV1::parse(valid).unwrap().as_str(), valid);
    }
    for invalid in ["data", "a/b", "/a~", "/a~2", "~0", "/a~b"] {
        assert_eq!(JsonPointerV1::parse(invalid), Err(EmbeddingsContractErrorV1::InvalidPointer));
    }
    let at_bound = format!("/{}", "a".repeat(MAX_VECTOR_POINTER_BYTES - 1));
    assert!(JsonPointerV1::parse(&at_bound).is_ok());
    assert!(JsonPointerV1::parse(&format!("{at_bound}a")).is_err());
}

#[test]
fn locator_wire_shapes_are_pinned() {
    let forms = [
        (VectorLocatorV1::NorthIdentical, r#"{"kind":"north_identical"}"#),
        (
            VectorLocatorV1::Array {
                array: pointer("/embeddings"),
                vector: pointer("/values"),
                index: None,
            },
            r#"{"kind":"array","array":"/embeddings","vector":"/values","index":null}"#,
        ),
        (
            VectorLocatorV1::Array {
                array: pointer("/predictions"),
                vector: pointer("/embeddings/values"),
                index: Some(pointer("/i")),
            },
            r#"{"kind":"array","array":"/predictions","vector":"/embeddings/values","index":"/i"}"#,
        ),
        (
            VectorLocatorV1::Single { vector: pointer("/embedding/values") },
            r#"{"kind":"single","vector":"/embedding/values"}"#,
        ),
    ];
    for (locator, golden) in forms {
        assert_eq!(serde_json::to_string(&locator).unwrap(), golden);
        assert_eq!(serde_json::from_str::<VectorLocatorV1>(golden).unwrap(), locator);
    }
    for refused in [
        r#"{"kind":"single","vector":"embedding"}"#,
        r#"{"kind":"single","vector":"/v","extra":1}"#,
        r#"{"kind":"array","array":"/a"}"#,
        r#"{"kind":"northIdentical"}"#,
        r#"{"kind":"columns","vector":"/v"}"#,
        r#"{"vector":"/v"}"#,
    ] {
        assert!(serde_json::from_str::<VectorLocatorV1>(refused).is_err(), "{refused}");
    }
}

#[test]
fn usage_facts_rules_and_wire_shape() {
    let reported =
        EmbeddingsUsageFactsV1::new(UsageSourceV1::Reported, Some(7), Some(vec![3, 4])).unwrap();
    let golden = r#"{"source":"reported","input_tokens":7,"per_input_tokens":[3,4]}"#;
    assert_eq!(serde_json::to_string(&reported).unwrap(), golden);
    assert_eq!(serde_json::from_str::<EmbeddingsUsageFactsV1>(golden).unwrap(), reported);
    assert_eq!(reported.per_input_tokens(), Some(&[3, 4][..]));
    assert_eq!(
        serde_json::to_string(&EmbeddingsUsageFactsV1::not_reported()).unwrap(),
        r#"{"source":"not_reported","input_tokens":null,"per_input_tokens":null}"#
    );
    assert_eq!(
        serde_json::to_string(&EmbeddingsUsageFactsV1::reported(0)).unwrap(),
        r#"{"source":"reported","input_tokens":0,"per_input_tokens":null}"#
    );
    let invalid = Err(EmbeddingsContractErrorV1::InvalidUsage);
    for (source, total, each) in [
        (UsageSourceV1::Reported, None, None),
        (UsageSourceV1::Reported, None, Some(vec![1])),
        (UsageSourceV1::Reported, Some(7), Some(vec![3, 3])),
        (UsageSourceV1::Reported, Some(0), Some(vec![])),
        (UsageSourceV1::Reported, Some(u64::MAX), Some(vec![u64::MAX, 1])),
        (UsageSourceV1::Reported, Some(1), Some(vec![0; MAX_EMBEDDING_INPUTS + 1])),
        (UsageSourceV1::NotReported, Some(1), None),
        (UsageSourceV1::NotReported, None, Some(vec![1])),
    ] {
        assert_eq!(EmbeddingsUsageFactsV1::new(source, total, each), invalid);
    }
    for refused in [
        r#"{"source":"reported","input_tokens":null,"per_input_tokens":null}"#,
        r#"{"source":"reported","input_tokens":-1,"per_input_tokens":null}"#,
        r#"{"source":"reported","input_tokens":1,"per_input_tokens":null,"x":0}"#,
        r#"{"source":"absent","input_tokens":null,"per_input_tokens":null}"#,
    ] {
        assert!(serde_json::from_str::<EmbeddingsUsageFactsV1>(refused).is_err(), "{refused}");
    }
}

#[test]
fn parsed_wire_shape_is_pinned() {
    let parsed =
        EmbeddingsParsedV1::new(EmbeddingsUsageFactsV1::reported(5), 2, Some("m-1".into()));
    let golden = r#"{"usage":{"source":"reported","input_tokens":5,"per_input_tokens":null},"vector_count":2,"upstream_model":"m-1"}"#;
    assert_eq!(serde_json::to_string(&parsed).unwrap(), golden);
    assert_eq!(serde_json::from_str::<EmbeddingsParsedV1>(golden).unwrap(), parsed);
    assert_eq!(parsed.vector_count(), 2);
    assert_eq!(parsed.upstream_model(), Some("m-1"));
    assert!(
        serde_json::from_str::<EmbeddingsParsedV1>(
            r#"{"usage":{"source":"not_reported","input_tokens":3,"per_input_tokens":null},"vector_count":1,"upstream_model":null}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<EmbeddingsParsedV1>(
            r#"{"usage":{"source":"not_reported","input_tokens":null,"per_input_tokens":null},"vector_count":1,"upstream_model":null,"vectors":[]}"#
        )
        .is_err()
    );
}

#[test]
fn north_identical_extracts_erases_and_passes_float_through() {
    let body = north_body(&[json!([0.5, -1]), json!([0.25, 2])], Some(4));
    let extracted = extract_vectors_v1(&body, &VectorLocatorV1::NorthIdentical).unwrap();
    assert_eq!(extracted.len(), 2);
    assert_eq!(extracted.dimensions(), 2);
    assert_eq!(extracted.vectors()[0].to_f32(), vec![0.5, -1.0]);
    assert_eq!(extracted.vectors()[1].encoding(), EncodingV1::Float);
    assert_eq!(
        extracted.skeleton(),
        &json!({
            "object":"list",
            "data":[
                {"object":"embedding","index":0,"embedding":null},
                {"object":"embedding","index":1,"embedding":null}
            ],
            "model":"upstream-echo",
            "usage":{"prompt_tokens":4,"total_tokens":4}
        })
    );
    assert!(extracted.all_encoded_as(EncodingV1::Float));
    assert!(extracted.returns_upstream_bytes(EncodingV1::Float));
    assert!(!extracted.returns_upstream_bytes(EncodingV1::Base64));
}

#[test]
fn north_identical_orders_by_index_and_requires_exactly_zero_to_n() {
    let body = br#"{"data":[{"index":1,"embedding":[2]},{"index":0,"embedding":[1]}]}"#;
    let extracted = extract_vectors_v1(body, &VectorLocatorV1::NorthIdentical).unwrap();
    assert_eq!(extracted.vectors()[0].to_f32(), vec![1.0]);
    assert_eq!(extracted.vectors()[1].to_f32(), vec![2.0]);
    for body in [
        r#"{"data":[{"index":0,"embedding":[1]},{"index":0,"embedding":[2]}]}"#,
        r#"{"data":[{"index":0,"embedding":[1]},{"index":2,"embedding":[2]}]}"#,
        r#"{"data":[{"index":1,"embedding":[1]}]}"#,
        r#"{"data":[{"embedding":[1]}]}"#,
        r#"{"data":[{"index":-1,"embedding":[1]}]}"#,
        r#"{"data":[{"index":"0","embedding":[1]}]}"#,
        r#"{"data":[{"index":0.0,"embedding":[1]}]}"#,
    ] {
        assert_eq!(
            extract_vectors_v1(body.as_bytes(), &VectorLocatorV1::NorthIdentical),
            Err(EmbeddingsContractErrorV1::InvalidIndex),
            "{body}"
        );
    }
}

#[test]
fn encoding_is_detected_per_vector() {
    // 0.1f32 and 1.0f32, little-endian, then -2.5f32.
    let body = north_body(&[json!("zczMPQAAgD8="), json!([0.1, 1])], Some(2));
    let extracted = extract_vectors_v1(&body, &VectorLocatorV1::NorthIdentical).unwrap();
    assert_eq!(extracted.vectors()[0].encoding(), EncodingV1::Base64);
    assert_eq!(extracted.vectors()[0].to_f32(), vec![0.1_f32, 1.0]);
    assert_eq!(extracted.vectors()[1].encoding(), EncodingV1::Float);
    assert_eq!(extracted.vectors()[1].to_f32(), vec![0.1_f32, 1.0]);
    assert!(!extracted.all_encoded_as(EncodingV1::Float));
    assert!(!extracted.all_encoded_as(EncodingV1::Base64));
    assert!(!extracted.returns_upstream_bytes(EncodingV1::Float));
    assert!(!extracted.returns_upstream_bytes(EncodingV1::Base64));

    let base64 = north_body(&[json!("AAAgwA==")], Some(1));
    let extracted = extract_vectors_v1(&base64, &VectorLocatorV1::NorthIdentical).unwrap();
    assert_eq!(extracted.vectors()[0].to_f32(), vec![-2.5]);
    assert!(extracted.returns_upstream_bytes(EncodingV1::Base64));
}

#[test]
fn invalid_vectors_are_protocol_errors() {
    let invalid = Err(EmbeddingsContractErrorV1::InvalidVector);
    for vector in [
        json!([]),
        json!(""),
        json!("AAA="),             // two bytes: not a multiple of four
        json!("AAAAAAA="),         // five bytes
        json!("AAAAAA"),           // unpadded
        json!("AAAAAB=="),         // non-canonical trailing bits
        json!("AA=A"),             // padding inside the text
        json!("AAAAAA==AAAAAA=="), // padding before the last group
        json!("AAAA AA="),
        json!("AADAfw=="), // NaN
        json!("AACAfw=="), // infinity
        json!([1e39]),     // not a finite f32
        json!([1, "2"]),
        json!([null]),
        json!([[1]]),
        json!({"values":[1]}),
        json!(1),
        json!(null),
    ] {
        let body = north_body(std::slice::from_ref(&vector), Some(1));
        assert_eq!(
            extract_vectors_v1(&body, &VectorLocatorV1::NorthIdentical),
            invalid,
            "{vector}"
        );
    }
}

#[test]
fn vectors_must_share_one_length() {
    let body = north_body(&[json!([1, 2]), json!("AACAPw==")], Some(2));
    assert_eq!(
        extract_vectors_v1(&body, &VectorLocatorV1::NorthIdentical),
        Err(EmbeddingsContractErrorV1::VectorLengthMismatch)
    );
}

#[test]
fn locator_must_resolve_to_vectors() {
    let not_found = Err(EmbeddingsContractErrorV1::VectorNotFound);
    for body in
        [r#"{"data":[]}"#, r#"{"data":{}}"#, r#"{"items":[]}"#, "[]", r#"{"data":[{"index":0}]}"#]
    {
        assert_eq!(
            extract_vectors_v1(body.as_bytes(), &VectorLocatorV1::NorthIdentical),
            not_found,
            "{body}"
        );
    }
    let single = VectorLocatorV1::Single { vector: pointer("/embedding/values") };
    assert_eq!(extract_vectors_v1(br#"{"embedding":{}}"#, &single), not_found);
    assert_eq!(
        extract_vectors_v1(b"{not json", &VectorLocatorV1::NorthIdentical),
        Err(EmbeddingsContractErrorV1::InvalidJson)
    );
    assert_eq!(
        extract_vectors_v1(br#"{"data":[]} {}"#, &VectorLocatorV1::NorthIdentical),
        Err(EmbeddingsContractErrorV1::InvalidJson)
    );
    let oversized = vec![b' '; MAX_BINARY_RESPONSE_BODY_BYTES + 1];
    assert_eq!(
        extract_vectors_v1(&oversized, &VectorLocatorV1::NorthIdentical),
        Err(EmbeddingsContractErrorV1::BodyTooLarge)
    );
}

#[test]
fn array_locator_without_index_keeps_body_order() {
    let locator = VectorLocatorV1::Array {
        array: pointer("/embeddings"),
        vector: pointer("/values"),
        index: None,
    };
    let body = br#"{"embeddings":[{"values":[3,4]},{"values":[1,2]}],"other":true}"#;
    let extracted = extract_vectors_v1(body, &locator).unwrap();
    assert_eq!(extracted.vectors()[0].to_f32(), vec![3.0, 4.0]);
    assert_eq!(
        extracted.skeleton(),
        &json!({"embeddings":[{"values":null},{"values":null}],"other":true})
    );
    assert!(!extracted.returns_upstream_bytes(EncodingV1::Float));
}

#[test]
fn array_locator_with_nested_vector_and_declared_index() {
    let locator = VectorLocatorV1::Array {
        array: pointer("/predictions"),
        vector: pointer("/embeddings/values"),
        index: Some(pointer("/position")),
    };
    let body = br#"{"predictions":[
        {"position":1,"embeddings":{"values":[2],"statistics":{"token_count":3.0}}},
        {"position":0,"embeddings":{"values":[1],"statistics":{"token_count":2.0}}}
    ]}"#;
    let extracted = extract_vectors_v1(body, &locator).unwrap();
    assert_eq!(extracted.vectors()[0].to_f32(), vec![1.0]);
    assert_eq!(
        extracted.skeleton(),
        &json!({"predictions":[
            {"position":1,"embeddings":{"values":null,"statistics":{"token_count":3.0}}},
            {"position":0,"embeddings":{"values":null,"statistics":{"token_count":2.0}}}
        ]})
    );
}

#[test]
fn single_locator_and_whole_value_pointers() {
    let single = VectorLocatorV1::Single { vector: pointer("/embedding/values") };
    let extracted = extract_vectors_v1(br#"{"embedding":{"values":[0.5,0.25]}}"#, &single).unwrap();
    assert_eq!(extracted.skeleton(), &json!({"embedding":{"values":null}}));
    assert_eq!(extracted.dimensions(), 2);
    let root = VectorLocatorV1::Single { vector: pointer("") };
    let extracted = extract_vectors_v1(b"[1,2,3]", &root).unwrap();
    assert_eq!(extracted.skeleton(), &Value::Null);
    let rows = VectorLocatorV1::Array { array: pointer(""), vector: pointer(""), index: None };
    let extracted = extract_vectors_v1(b"[[1],[2]]", &rows).unwrap();
    assert_eq!(extracted.skeleton(), &json!([null, null]));
    assert_eq!(extracted.len(), 2);
}

#[test]
fn render_base64_to_float_emits_shortest_f32_decimals() {
    // 0.1f32 widened to f64 prints as 0.10000000149011612; the shortest f32 spelling is 0.1.
    let body = north_body(&[json!("zczMPQAAgD8="), json!("//9/fwEAAAA=")], Some(9));
    let (_, vectors) =
        extract_vectors_v1(&body, &VectorLocatorV1::NorthIdentical).unwrap().into_parts();
    assert_eq!(
        render_text(&vectors, EncodingV1::Float, "m", 9),
        r#"{"object":"list","data":[{"object":"embedding","index":0,"embedding":[0.1,1.0]},{"object":"embedding","index":1,"embedding":[3.4028235e+38,1e-45]}],"model":"m","usage":{"prompt_tokens":9,"total_tokens":9}}"#
    );
}

#[test]
fn render_float_to_base64_rounds_to_nearest_f32_little_endian() {
    // 0.10000000149011612 is 0.1f32 exactly; 16777217 is halfway and rounds to even (16777216).
    let body = north_body(&[json!([0.1, 1]), json!([0.100_000_001_490_116_12, 16_777_217])], None);
    let (_, vectors) =
        extract_vectors_v1(&body, &VectorLocatorV1::NorthIdentical).unwrap().into_parts();
    assert_eq!(
        render_text(&vectors, EncodingV1::Base64, "m", 3),
        r#"{"object":"list","data":[{"object":"embedding","index":0,"embedding":"zczMPQAAgD8="},{"object":"embedding","index":1,"embedding":"zczMPQAAgEs="}],"model":"m","usage":{"prompt_tokens":3,"total_tokens":3}}"#
    );
}

#[test]
fn render_same_encoding_emits_vectors_as_they_arrived() {
    let body = north_body(&[json!([0.1, 0, -1e-7, 2.50])], None);
    let (_, vectors) =
        extract_vectors_v1(&body, &VectorLocatorV1::NorthIdentical).unwrap().into_parts();
    assert_eq!(
        render_text(&vectors, EncodingV1::Float, "m", 1),
        r#"{"object":"list","data":[{"object":"embedding","index":0,"embedding":[0.1,0,-1e-7,2.5]}],"model":"m","usage":{"prompt_tokens":1,"total_tokens":1}}"#
    );
    let body = north_body(&[json!("AAAgwA==")], None);
    let (_, vectors) =
        extract_vectors_v1(&body, &VectorLocatorV1::NorthIdentical).unwrap().into_parts();
    assert_eq!(
        render_text(&vectors, EncodingV1::Base64, "m", 1),
        r#"{"object":"list","data":[{"object":"embedding","index":0,"embedding":"AAAgwA=="}],"model":"m","usage":{"prompt_tokens":1,"total_tokens":1}}"#
    );
}

#[test]
fn render_refuses_no_vectors_and_mixed_lengths() {
    assert_eq!(
        render_vectors_v1(&[], EncodingV1::Float, "m", 0),
        Err(EmbeddingsContractErrorV1::VectorLengthMismatch)
    );
    let one = extract_vectors_v1(b"[1]", &VectorLocatorV1::Single { vector: pointer("") }).unwrap();
    let two =
        extract_vectors_v1(b"[1,2]", &VectorLocatorV1::Single { vector: pointer("") }).unwrap();
    let mixed = [one.vectors()[0].clone(), two.vectors()[0].clone()];
    assert_eq!(
        render_vectors_v1(&mixed, EncodingV1::Float, "m", 0),
        Err(EmbeddingsContractErrorV1::VectorLengthMismatch)
    );
}

fn request_of(inputs: usize, dimensions: Option<u32>) -> EmbeddingsRequestV1 {
    let mut body = json!({"input": vec!["x"; inputs]});
    if let Some(dimensions) = dimensions {
        body["dimensions"] = json!(dimensions);
    }
    parse(&body).unwrap()
}

#[test]
fn response_checks_internal_consistency() {
    let request = request_of(2, Some(2));
    let body = north_body(&[json!([1, 2]), json!([3, 4])], Some(7));
    let extracted = extract_vectors_v1(&body, &VectorLocatorV1::NorthIdentical).unwrap();
    let per_input =
        EmbeddingsUsageFactsV1::new(UsageSourceV1::Reported, Some(7), Some(vec![3, 4])).unwrap();
    let ok = EmbeddingsParsedV1::new(per_input, 2, None);
    assert_eq!(check_embeddings_response_v1(&request, true, &extracted, &ok), Ok(()));

    let miscounted = EmbeddingsParsedV1::new(EmbeddingsUsageFactsV1::reported(7), 1, None);
    assert_eq!(
        check_embeddings_response_v1(&request, true, &extracted, &miscounted),
        Err(EmbeddingsContractErrorV1::VectorCountMismatch)
    );
    assert_eq!(
        check_embeddings_response_v1(&request_of(3, None), true, &extracted, &ok),
        Err(EmbeddingsContractErrorV1::InvalidUsage)
    );
    let three = EmbeddingsParsedV1::new(EmbeddingsUsageFactsV1::reported(7), 3, None);
    assert_eq!(
        check_embeddings_response_v1(&request_of(3, None), true, &extracted, &three),
        Err(EmbeddingsContractErrorV1::VectorCountMismatch)
    );
    let short = request_of(2, Some(3));
    assert_eq!(
        check_embeddings_response_v1(&short, true, &extracted, &ok),
        Err(EmbeddingsContractErrorV1::DimensionsMismatch)
    );
    // Without the `dimensions` capability the requested value does not bind the length.
    assert_eq!(check_embeddings_response_v1(&short, false, &extracted, &ok), Ok(()));
}

#[test]
fn north_identical_usage_must_equal_the_reported_count() {
    let request = request_of(1, None);
    let mismatch = Err(EmbeddingsContractErrorV1::NorthUsageMismatch);
    let check = |body: &[u8], usage: EmbeddingsUsageFactsV1| {
        let extracted = extract_vectors_v1(body, &VectorLocatorV1::NorthIdentical).unwrap();
        check_embeddings_response_v1(
            &request,
            false,
            &extracted,
            &EmbeddingsParsedV1::new(usage, 1, None),
        )
    };
    let body = north_body(&[json!([1])], Some(5));
    assert_eq!(check(&body, EmbeddingsUsageFactsV1::reported(5)), Ok(()));
    assert_eq!(check(&body, EmbeddingsUsageFactsV1::reported(4)), mismatch);
    assert_eq!(check(&body, EmbeddingsUsageFactsV1::not_reported()), mismatch);
    assert_eq!(
        check(&north_body(&[json!([1])], None), EmbeddingsUsageFactsV1::reported(0)),
        mismatch
    );
    let negative = br#"{"data":[{"index":0,"embedding":[1]}],"usage":{"prompt_tokens":-1}}"#;
    assert_eq!(check(negative, EmbeddingsUsageFactsV1::reported(0)), mismatch);
    // Other locators do not carry the northbound usage shape.
    let single =
        extract_vectors_v1(b"[1]", &VectorLocatorV1::Single { vector: pointer("") }).unwrap();
    let parsed = EmbeddingsParsedV1::new(EmbeddingsUsageFactsV1::not_reported(), 1, None);
    assert_eq!(check_embeddings_response_v1(&request, false, &single, &parsed), Ok(()));
}

fn finite_vectors() -> impl Strategy<Value = Vec<Vec<f32>>> {
    (1_usize..6, 1_usize..12).prop_flat_map(|(count, dimensions)| {
        proptest::collection::vec(
            proptest::collection::vec(
                any::<f32>().prop_filter("finite", |value| value.is_finite()),
                dimensions,
            ),
            count,
        )
    })
}

fn bits(vectors: &[Vec<f32>]) -> Vec<Vec<u32>> {
    vectors.iter().map(|vector| vector.iter().map(|value| value.to_bits()).collect()).collect()
}

proptest! {
    #[test]
    fn render_and_extract_round_trip_exact_f32_values(vectors in finite_vectors()) {
        // Upstream floats printed as shortest f32 decimals, as a careful upstream would.
        let floats: Vec<Value> =
            vectors.iter().map(|vector| serde_json::from_str(&serde_json::to_string(vector).unwrap()).unwrap()).collect();
        let body = north_body(&floats, Some(1));
        let extracted = extract_vectors_v1(&body, &VectorLocatorV1::NorthIdentical).unwrap();
        let decoded: Vec<Vec<f32>> = extracted.vectors().iter().map(south_contracts::EmbeddingVectorV1::to_f32).collect();
        prop_assert_eq!(bits(&decoded), bits(&vectors));
        prop_assert!(extracted.returns_upstream_bytes(EncodingV1::Float));

        // float -> base64 -> extract keeps every bit.
        let base64 = render_vectors_v1(extracted.vectors(), EncodingV1::Base64, "m", 1).unwrap();
        let from_base64 = extract_vectors_v1(&base64, &VectorLocatorV1::NorthIdentical).unwrap();
        prop_assert!(from_base64.returns_upstream_bytes(EncodingV1::Base64));
        let decoded: Vec<Vec<f32>> = from_base64.vectors().iter().map(south_contracts::EmbeddingVectorV1::to_f32).collect();
        prop_assert_eq!(bits(&decoded), bits(&vectors));

        // base64 -> float emits the same shortest decimals the f32 printer does.
        let back = render_vectors_v1(from_base64.vectors(), EncodingV1::Float, "m", 1).unwrap();
        let items: Vec<String> = vectors
            .iter()
            .enumerate()
            .map(|(index, vector)| {
                format!(
                    r#"{{"object":"embedding","index":{index},"embedding":{}}}"#,
                    serde_json::to_string(vector).unwrap()
                )
            })
            .collect();
        let expected = format!(
            r#"{{"object":"list","data":[{}],"model":"m","usage":{{"prompt_tokens":1,"total_tokens":1}}}}"#,
            items.join(",")
        );
        prop_assert_eq!(String::from_utf8(back.clone()).unwrap(), expected);
        let again = extract_vectors_v1(&back, &VectorLocatorV1::NorthIdentical).unwrap();
        let decoded: Vec<Vec<f32>> = again.vectors().iter().map(south_contracts::EmbeddingVectorV1::to_f32).collect();
        prop_assert_eq!(bits(&decoded), bits(&vectors));
    }

    #[test]
    fn any_string_input_is_text_media_or_empty(input in ".{0,40}", media in "[a-z]{1,8}/[a-z+.-]{1,8}") {
        let parsed = parse(&json!({"input": input}));
        match media_type_of(&input) {
            _ if input.is_empty() => prop_assert_eq!(parsed, Err(EmbeddingsRequestErrorV1::InvalidInput)),
            Some(_) => prop_assert_eq!(parsed, Err(EmbeddingsRequestErrorV1::MediaInputNotSupported)),
            None => prop_assert_eq!(parsed.unwrap().inputs().to_vec(), vec![text(&input)]),
        }
        let uri = format!("data:{media};base64,{input}");
        prop_assert_eq!(media_type_of(&uri), Some(media.as_str()));
    }
}
