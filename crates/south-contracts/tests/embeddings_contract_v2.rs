//! Golden vectors for embeddings contract 2, inline media inputs
//! (docs/design/2026-09-30-embeddings-contract.md §17): the contract 2 parse entry point, the two
//! request constructors, the `Media` wire shape and the versioning constants. Contract 1's own
//! vectors stay in `embeddings_contract_v1.rs`, untouched.

use proptest::prelude::*;
use serde_json::{Map, Value, json};
use south_contracts::{
    EMBEDDINGS_CONTRACT_VERSION, EMBEDDINGS_CONTRACT_VERSION_V2, EMBEDDINGS_CONTRACT_VERSIONS,
    EmbeddingInputV1, EmbeddingsRequestErrorV1, EmbeddingsRequestV1, InputShapeV1,
    MAX_EMBEDDING_INPUTS, MAX_EMBEDDINGS_REQUEST_VIEW_BYTES, media_type_of,
    parse_embeddings_request_v1, parse_embeddings_request_v2,
};

fn parse(body: &Value) -> Result<EmbeddingsRequestV1, EmbeddingsRequestErrorV1> {
    parse_embeddings_request_v2(body, "upstream-model")
}

fn parse_v1(body: &Value) -> Result<EmbeddingsRequestV1, EmbeddingsRequestErrorV1> {
    parse_embeddings_request_v1(body, "upstream-model")
}

fn text(value: &str) -> EmbeddingInputV1 {
    EmbeddingInputV1::Text(value.to_owned())
}

fn media(media_type: &str, data: &str) -> EmbeddingInputV1 {
    EmbeddingInputV1::Media { media_type: media_type.to_owned(), data: data.to_owned() }
}

#[test]
fn versioning_constants_are_pinned() {
    // Contract 1 keeps the name and the value it has always had.
    assert_eq!(EMBEDDINGS_CONTRACT_VERSION, 1);
    assert_eq!(EMBEDDINGS_CONTRACT_VERSION_V2, 2);
    assert_eq!(EMBEDDINGS_CONTRACT_VERSIONS, [1, 2]);
    // The runtime's payload limit, mirrored for the hosts (record §17.8).
    assert_eq!(MAX_EMBEDDINGS_REQUEST_VIEW_BYTES, 16 * 1024 * 1024);
}

// §17.2: a media string is a `Media` input split at the first `;base64,`, payload verbatim.
#[test]
fn a_media_string_is_a_media_input_of_shape_single() {
    let request = parse(&json!({"input": "data:image/png;base64,iVBORw0KGgo="})).unwrap();
    assert_eq!(request.inputs(), &[media("image/png", "iVBORw0KGgo=")]);
    assert_eq!(request.input_shape(), InputShapeV1::Single);
    assert!(request.carries_media());
    assert_eq!(request.minimum_contract_version(), 2);
}

#[test]
fn media_and_text_mix_in_an_array_and_keep_their_order() {
    let request = parse(&json!({
        "input": ["hello", "data:video/mp4;base64,AAAA", "data:text/plain,not media", "world"],
        "dimensions": 256,
    }))
    .unwrap();
    assert_eq!(
        request.inputs(),
        &[
            text("hello"),
            media("video/mp4", "AAAA"),
            text("data:text/plain,not media"),
            text("world")
        ]
    );
    assert_eq!(request.input_shape(), InputShapeV1::Array);
    assert_eq!(request.dimensions(), Some(256));
}

#[test]
fn the_payload_is_the_clients_text_after_the_first_marker_and_is_never_decoded() {
    for (uri, media_type, data) in [
        ("data:image/png;base64,", "image/png", ""),
        ("data:image/png;base64,abc;base64,def", "image/png", "abc;base64,def"),
        ("data:image/png;base64,!!not base64!!", "image/png", "!!not base64!!"),
        ("data:image/png;base64,AA==\n", "image/png", "AA==\n"),
        (
            "data:audio/L16;rate=16000;channels=1;base64,AAAA",
            "audio/L16;rate=16000;channels=1",
            "AAAA",
        ),
        ("data:IMAGE/PNG;base64,AAAA", "IMAGE/PNG", "AAAA"),
        (
            "data:application/pdf;name=a.pdf;base64,JVBERi0=",
            "application/pdf;name=a.pdf",
            "JVBERi0=",
        ),
    ] {
        let request = parse(&json!({"input": uri})).unwrap();
        assert_eq!(request.inputs(), &[media(media_type, data)], "{uri}");
        assert_eq!(media_type_of(uri), Some(media_type), "{uri}");
    }
}

#[test]
fn a_string_that_is_not_the_media_form_stays_text() {
    for input in [
        "data:text/plain,abc",
        "data:image/png;base64",
        "DATA:image/png;base64,AAAA",
        "data:image/png;BASE64,AAAA",
        "data:image;base64,AAAA",
        "data:image/png;name;base64,AAAA",
        "data:image/png; name=v;base64,AAAA",
        "data:image/png;name=\"v\";base64,AAAA",
        " data:image/png;base64,AAAA",
        "hello",
    ] {
        let request = parse(&json!({"input": input})).unwrap();
        assert_eq!(request.inputs(), &[text(input)], "{input}");
        assert!(!request.carries_media());
        assert_eq!(request.minimum_contract_version(), 1);
    }
}

// The one difference between the entry points: v1 refuses what v2 carries.
#[test]
fn contract_1_refuses_the_input_contract_2_carries() {
    let body = json!({"input": ["hello", "data:image/png;base64,AAAA"]});
    assert_eq!(parse_v1(&body), Err(EmbeddingsRequestErrorV1::MediaInputNotSupported));
    assert!(parse(&body).is_ok());
}

#[test]
fn refusals_and_their_order_are_those_of_contract_1() {
    let media = "data:image/png;base64,AAAA";
    let bodies = [
        json!({"input": vec![json!(media); MAX_EMBEDDING_INPUTS + 1]}),
        json!({"input": [media, ""]}),
        json!({"input": [media, 1]}),
        json!({"input": ""}),
        json!({"input": []}),
        json!({"input": [[1], [2]]}),
        json!({"input": media, "dimensions": 0}),
        json!({"input": media, "encoding_format": "hex"}),
        json!({"input": media, "user": 7}),
        json!({"input": media, "$south.x": 1}),
        json!([]),
        json!({}),
    ];
    for body in bodies {
        // Contract 1 refuses a media string right after the shape and the count; contract 2
        // carries it, so the checks that follow (dimensions, encoding, user, extras) speak.
        // Every other answer is the same for both contracts.
        let (v1, v2) = (parse_v1(&body), parse(&body));
        match v1 {
            Err(EmbeddingsRequestErrorV1::MediaInputNotSupported) => {
                assert_ne!(v2, Err(EmbeddingsRequestErrorV1::MediaInputNotSupported), "{body}");
            }
            other => assert_eq!(other, v2, "{body}"),
        }
    }
    for (body, refusal) in [
        (json!({"input": media, "dimensions": 0}), EmbeddingsRequestErrorV1::InvalidDimensions),
        (
            json!({"input": media, "encoding_format": "hex"}),
            EmbeddingsRequestErrorV1::InvalidEncodingFormat,
        ),
        (json!({"input": media, "user": 7}), EmbeddingsRequestErrorV1::InvalidUser),
        (json!({"input": media, "$south.x": 1}), EmbeddingsRequestErrorV1::ReservedExtraField),
    ] {
        assert_eq!(parse(&body), Err(refusal), "{body}");
        assert_eq!(
            parse_v1(&body),
            Err(EmbeddingsRequestErrorV1::MediaInputNotSupported),
            "{body}"
        );
    }
    // The media check comes after shape and count, as in contract 1: these refuse before any
    // media is classified.
    let too_many =
        json!({"input": vec![json!("data:image/png;base64,AAAA"); MAX_EMBEDDING_INPUTS + 1]});
    assert_eq!(parse(&too_many), Err(EmbeddingsRequestErrorV1::TooManyInputs));
    let exactly = json!({"input": vec![json!("data:image/png;base64,AAAA"); MAX_EMBEDDING_INPUTS]});
    assert_eq!(parse(&exactly).unwrap().inputs().len(), MAX_EMBEDDING_INPUTS);
    assert_eq!(
        parse(&json!({"input": ["data:image/png;base64,AAAA", ""]})),
        Err(EmbeddingsRequestErrorV1::InvalidInput)
    );
    assert_eq!(
        parse(&json!({"input": ["data:image/png;base64,AAAA", 1]})),
        Err(EmbeddingsRequestErrorV1::InvalidInput)
    );
}

#[test]
fn a_body_without_media_parses_to_the_same_request_under_both_contracts() {
    for body in [
        json!({"input": "hello"}),
        json!({"input": ["a", "b"], "dimensions": 8, "encoding_format": "base64", "user": "u"}),
        json!({"input": [1, 2, 3]}),
        json!({"input": [[1], [2, 3]], "input_type": "query", "truncate": "END"}),
        json!({"input": "data:text/plain,abc"}),
    ] {
        assert_eq!(parse_v1(&body).unwrap(), parse(&body).unwrap(), "{body}");
    }
}

// §17.3: the two constructors.
#[test]
fn the_contract_1_constructor_refuses_media_and_the_contract_2_constructor_carries_it() {
    let build = |inputs: Vec<EmbeddingInputV1>, shape| {
        (
            EmbeddingsRequestV1::new(
                "m".into(),
                inputs.clone(),
                shape,
                None,
                None,
                None,
                Map::new(),
            ),
            EmbeddingsRequestV1::new_v2("m".into(), inputs, shape, None, None, None, Map::new()),
        )
    };
    let (v1, v2) = build(vec![media("image/png", "AAAA")], InputShapeV1::Single);
    assert_eq!(v1, Err(EmbeddingsRequestErrorV1::MediaInputNotSupported));
    assert!(v2.is_ok());
    let (v1, v2) = build(vec![text("a"), media("image/png", "")], InputShapeV1::Array);
    assert_eq!(v1, Err(EmbeddingsRequestErrorV1::MediaInputNotSupported));
    assert!(v2.is_ok(), "an empty payload is carried, as the native arm carries it");
    // Without media the two agree.
    let (v1, v2) = build(vec![text("a")], InputShapeV1::Single);
    assert_eq!(v1, v2);
}

#[test]
fn the_contract_2_constructor_keeps_text_and_media_apart() {
    let build = |inputs: Vec<EmbeddingInputV1>| {
        EmbeddingsRequestV1::new_v2(
            "m".into(),
            inputs,
            InputShapeV1::Single,
            None,
            None,
            None,
            Map::new(),
        )
    };
    // The parser makes such a string a Media input; text holding one was built by hand.
    assert_eq!(
        build(vec![text("data:image/png;base64,AAAA")]),
        Err(EmbeddingsRequestErrorV1::InvalidInput)
    );
    // A media type that breaks the grammar would not split back out of its URI.
    for media_type in ["", "image", "image/", "/png", "image/png;", "image/png;a", "a b/c", "a/b,c"]
    {
        assert_eq!(
            build(vec![media(media_type, "AAAA")]),
            Err(EmbeddingsRequestErrorV1::InvalidInput),
            "{media_type:?}"
        );
    }
    assert!(build(vec![media("image/png;name=a.png", "AAAA")]).is_ok());
    assert_eq!(
        EmbeddingsRequestV1::new_v2(
            "m".into(),
            vec![media("image/png", "AAAA"); 2],
            InputShapeV1::Single,
            None,
            None,
            None,
            Map::new()
        ),
        Err(EmbeddingsRequestErrorV1::InvalidInput)
    );
}

// §17.2 wire: pinned because two hosts and the guests must agree byte for byte.
#[test]
fn the_media_wire_shape_is_pinned() {
    let input = media("image/png", "iVBORw0KGgo=");
    assert_eq!(
        serde_json::to_string(&input).unwrap(),
        r#"{"media":{"media_type":"image/png","data":"iVBORw0KGgo="}}"#
    );
    assert_eq!(
        serde_json::from_str::<EmbeddingInputV1>(
            r#"{"media":{"media_type":"image/png","data":"iVBORw0KGgo="}}"#
        )
        .unwrap(),
        input
    );
    // Contract 1's shapes are untouched.
    assert_eq!(serde_json::to_string(&text("a")).unwrap(), r#"{"text":"a"}"#);
    assert_eq!(
        serde_json::to_string(&EmbeddingInputV1::TokenIds(vec![1, 2])).unwrap(),
        r#"{"token_ids":[1,2]}"#
    );
    for refused in [
        r#"{"media":{"media_type":"image/png","data":"AA","extra":1}}"#,
        r#"{"media":{"media_type":"image/png"}}"#,
        r#"{"media":{"data":"AA"}}"#,
        r#"{"media":"data:image/png;base64,AA"}"#,
        r#"{"media":{"media_type":"image/png","data":1}}"#,
        r#"{"media":{"media_type":"image/png","data":"AA"},"text":"a"}"#,
    ] {
        assert!(serde_json::from_str::<EmbeddingInputV1>(refused).is_err(), "{refused}");
    }
}

#[test]
fn the_request_frame_decodes_with_the_contract_2_rules_and_round_trips() {
    let request =
        parse(&json!({"input": ["a", "data:image/png;base64,AAAA"], "dimensions": 4})).unwrap();
    let encoded = serde_json::to_string(&request).unwrap();
    assert_eq!(
        encoded,
        r#"{"model":"upstream-model","inputs":[{"text":"a"},{"media":{"media_type":"image/png","data":"AAAA"}}],"input_shape":"array","dimensions":4,"encoding_format":null,"user":null,"extra":{}}"#
    );
    assert_eq!(serde_json::from_str::<EmbeddingsRequestV1>(&encoded).unwrap(), request);
    // The decoder holds the invariants of the contract 2 constructor.
    let bad = encoded.replace("image/png", "image");
    assert!(serde_json::from_str::<EmbeddingsRequestV1>(&bad).is_err());
    let misclassified = r#"{"model":"m","inputs":[{"text":"data:image/png;base64,AA"}],"input_shape":"single","dimensions":null,"encoding_format":null,"user":null,"extra":{}}"#;
    assert!(serde_json::from_str::<EmbeddingsRequestV1>(misclassified).is_err());
}

#[test]
fn debug_prints_the_media_type_and_the_size_never_the_data() {
    let request =
        parse(&json!({"input": "data:image/png;base64,SECRETPAYLOAD", "user": "secret"})).unwrap();
    let debug = format!("{request:?}");
    assert!(debug.contains("image/png") && debug.contains("byte_count: 13"), "{debug}");
    assert!(!debug.contains("SECRET") && !debug.contains("secret"), "{debug}");
}

proptest! {
    #[test]
    fn a_media_input_rebuilds_the_clients_string(
        media_type in "[a-z]{1,8}/[a-z+.-]{1,8}(;[a-z]{1,5}=[a-z0-9.]{1,5}){0,2}",
        data in ".{0,64}",
    ) {
        let uri = format!("data:{media_type};base64,{data}");
        let parsed = parse(&json!({"input": &uri})).unwrap();
        prop_assert_eq!(parsed.inputs(), &[media(&media_type, &data)][..]);
        // Rebuilding the URI from the parts returns the client's string, and parsing that again
        // returns the same request.
        let EmbeddingInputV1::Media { media_type: found, data: payload } = &parsed.inputs()[0] else {
            panic!("a media input")
        };
        prop_assert_eq!(format!("data:{found};base64,{payload}"), uri);
        let encoded = serde_json::to_string(&parsed).unwrap();
        prop_assert_eq!(serde_json::from_str::<EmbeddingsRequestV1>(&encoded).unwrap(), parsed);
    }

    #[test]
    fn contract_2_refuses_nothing_contract_1_accepts_and_differs_only_on_media(
        input in ".{0,40}",
    ) {
        let body = json!({"input": input});
        match (parse_v1(&body), parse(&body)) {
            (Err(EmbeddingsRequestErrorV1::MediaInputNotSupported), Ok(request)) => {
                prop_assert!(request.carries_media());
                prop_assert!(media_type_of(&input).is_some());
            }
            (v1, v2) => prop_assert_eq!(v1, v2),
        }
    }
}
