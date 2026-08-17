use http::StatusCode;
use south_contracts::{
    AUTH_CONTRACT_VERSION, BearerAuthV1, BufferedHttpResponseV1, ContractErrorV1, CredentialSlotV1,
    ERROR_CONTRACT_VERSION, HTTP_CONTRACT_VERSION, JsonBodyV1, JsonPostRequestV1,
    MAX_CREDENTIAL_SLOT_BYTES, MAX_ENDPOINT_BYTES, MAX_JSON_REQUEST_BODY_BYTES,
    MAX_RELATIVE_PATH_BYTES, MAX_RESPONSE_BODY_BYTES, MAX_RESPONSE_CONTENT_TYPE_BYTES,
    MAX_RESPONSE_RETRY_AFTER_BYTES, PreparationErrorV1, ProviderAuthV1, ProviderEndpointV1,
    RelativePathV1, STREAM_CONTRACT_VERSION, SafeHeaders, SecretHeaderV1, TransportErrorV1,
};

const SENTINEL: &str = "must-not-appear-7f23a";

/// Every sanctioned secret-bearing header in canonical table order. The match below is
/// deliberately exhaustive so adding a `SecretHeaderV1` variant fails compilation here until the
/// list, the reserved-header pin, and the conformance surface are all updated together.
const ALL_SECRET_HEADERS: [SecretHeaderV1; 5] = [
    SecretHeaderV1::ApiKey,
    SecretHeaderV1::XApiKey,
    SecretHeaderV1::XGoogApiKey,
    SecretHeaderV1::XiApiKey,
    SecretHeaderV1::OcpApimSubscriptionKey,
];

const fn assert_secret_header_listed(header: SecretHeaderV1) {
    match header {
        SecretHeaderV1::ApiKey
        | SecretHeaderV1::XApiKey
        | SecretHeaderV1::XGoogApiKey
        | SecretHeaderV1::XiApiKey
        | SecretHeaderV1::OcpApimSubscriptionKey => {}
    }
}

#[test]
fn contract_versions_are_independently_versioned() {
    assert_eq!(HTTP_CONTRACT_VERSION, 1);
    assert_eq!(AUTH_CONTRACT_VERSION, 2);
    assert_eq!(ERROR_CONTRACT_VERSION, 1);
    assert_eq!(STREAM_CONTRACT_VERSION, Some(1));
}

#[test]
fn secret_header_names_are_frozen_and_stay_on_the_reserved_list() {
    let expected_names =
        ["api-key", "x-api-key", "x-goog-api-key", "xi-api-key", "ocp-apim-subscription-key"];

    for (header, expected_name) in ALL_SECRET_HEADERS.into_iter().zip(expected_names) {
        assert_secret_header_listed(header);
        assert_eq!(header.header_name(), expected_name);

        // The sanctioned set must never drift off the plain-header blacklist: a provider must not
        // smuggle a secret-shaped header through the ordinary `SafeHeaders` channel.
        let error = SafeHeaders::try_from_iter([(header.header_name(), "must-not-appear")])
            .expect_err("every sanctioned secret header must stay reserved");
        assert_eq!(error.code(), "RESERVED_HEADER_FORBIDDEN");
    }
}

#[test]
fn provider_auth_carries_a_slot_declaration_under_both_arms() {
    let slot = CredentialSlotV1::parse("openai.primary").unwrap();
    let bearer: ProviderAuthV1 = BearerAuthV1::new(slot.clone()).into();
    let header_secret = ProviderAuthV1::HeaderSecret {
        header: SecretHeaderV1::XApiKey,
        slot: BearerAuthV1::new(slot.clone()),
    };

    assert!(matches!(bearer, ProviderAuthV1::Bearer(_)));
    assert_eq!(bearer.credential_slot(), &slot);
    assert_eq!(header_secret.credential_slot(), &slot);
}

#[test]
fn request_accepts_both_auth_scheme_declarations_through_one_constructor() {
    let request = JsonPostRequestV1::new(
        RelativePathV1::parse("v1/messages").unwrap(),
        SafeHeaders::try_from_iter([("content-type", "application/json")]).unwrap(),
        JsonBodyV1::parse("{\"input\":\"hello\"}").unwrap(),
        ProviderAuthV1::HeaderSecret {
            header: SecretHeaderV1::XApiKey,
            slot: BearerAuthV1::new(CredentialSlotV1::parse("anthropic.primary").unwrap()),
        },
    );

    let ProviderAuthV1::HeaderSecret { header, .. } = request.auth() else {
        panic!("the header-secret declaration must be preserved");
    };
    assert_eq!(header.header_name(), "x-api-key");
    assert_eq!(request.auth().credential_slot().as_str(), "anthropic.primary");
}

#[test]
fn endpoint_accepts_http_and_https_and_normalizes_the_base_path() {
    let https = ProviderEndpointV1::parse("https://EXAMPLE.com:443/provider/v1").unwrap();
    let http = ProviderEndpointV1::parse("http://127.0.0.1:8080").unwrap();

    assert_eq!(https.as_str(), "https://example.com/provider/v1/");
    assert_eq!(http.as_str(), "http://127.0.0.1:8080/");
}

#[test]
fn endpoint_enforces_its_size_boundary() {
    let prefix = "https://example.com/";
    let normalized_at_limit =
        format!("{prefix}{}", "a".repeat(MAX_ENDPOINT_BYTES - prefix.len() - 1));
    let overflow_after_normalization = format!("{normalized_at_limit}a");
    let already_normalized_at_limit =
        format!("{prefix}{}/", "a".repeat(MAX_ENDPOINT_BYTES - prefix.len() - 1));

    assert_eq!(
        ProviderEndpointV1::parse(&normalized_at_limit).unwrap().as_str().len(),
        MAX_ENDPOINT_BYTES
    );
    assert_eq!(
        ProviderEndpointV1::parse(&overflow_after_normalization),
        Err(ContractErrorV1::InvalidEndpoint)
    );
    assert_eq!(
        ProviderEndpointV1::parse(&already_normalized_at_limit).unwrap().as_str().len(),
        MAX_ENDPOINT_BYTES
    );
}

#[test]
fn endpoint_rejects_non_http_or_ambiguous_authorities() {
    let invalid = [
        "ftp://example.com/base",
        "mailto:user@example.com",
        "https:/base",
        "https:///base",
        "https://@example.com/base",
        "https://:@example.com/base",
        "https://user@example.com/base",
        "https://:password@example.com/base",
        "https://example.com/base?query=secret",
        "https://example.com/base#fragment",
    ];

    for input in invalid {
        assert_eq!(
            ProviderEndpointV1::parse(input),
            Err(ContractErrorV1::InvalidEndpoint),
            "unexpectedly accepted endpoint attack class: {input}"
        );
    }
}

#[test]
fn endpoint_rejects_unsafe_path_segments() {
    let invalid = [
        "https://example.com/a//b",
        "https://example.com//",
        "https://example.com/a/./b",
        "https://example.com/a/../b",
        "https://example.com/a\\b",
        "https://example.com/a b",
        "https://example.com/a/%2E/b",
        "https://example.com/a/%2f/b",
        "https://example.com/a/%5C/b",
        "https://example.com/a/%25/b",
        "https://example.com/a/%",
        "https://example.com/a/%2",
        "https://example.com/a/%gg",
        "https://example.com/a/%2g",
    ];

    for input in invalid {
        assert_eq!(
            ProviderEndpointV1::parse(input),
            Err(ContractErrorV1::InvalidEndpoint),
            "unexpectedly accepted unsafe endpoint path"
        );
    }
}

#[test]
fn relative_path_accepts_a_bounded_ascii_segment_sequence() {
    let path = RelativePathV1::parse("v1/models/model-1:invoke").unwrap();

    assert_eq!(path.as_str(), "v1/models/model-1:invoke");
}

#[test]
fn relative_path_enforces_its_size_boundary() {
    let at_limit = "a".repeat(MAX_RELATIVE_PATH_BYTES);
    let over_limit = "a".repeat(MAX_RELATIVE_PATH_BYTES + 1);

    assert!(RelativePathV1::parse(&at_limit).is_ok());
    assert_eq!(RelativePathV1::parse(&over_limit), Err(ContractErrorV1::InvalidRelativePath));
}

#[test]
fn relative_path_rejects_each_unsafe_path_class() {
    let invalid = [
        "",
        "/v1/models",
        "v1/models?query=secret",
        "v1/models#fragment",
        "v1//models",
        "v1/./models",
        "v1/../models",
        "v1\\models",
        "v1 models",
        "v1\nmodels",
        "https:example.com/path",
        "HtTpS:example.com/path",
        "v1/%2emodels",
        "v1/%2Fmodels",
        "v1/%5cmodels",
        "v1/%25models",
        "v1/%",
        "v1/%2",
        "v1/%gg",
        "v1/%2g",
        "\u{6a21}\u{578b}/v1",
    ];

    for input in invalid {
        assert_eq!(
            RelativePathV1::parse(input),
            Err(ContractErrorV1::InvalidRelativePath),
            "unexpectedly accepted unsafe relative path"
        );
    }
}

#[test]
fn relative_path_resolution_stays_within_the_normalized_binding() {
    let endpoint = ProviderEndpointV1::parse("https://EXAMPLE.com:443/providers/openai").unwrap();
    let path = RelativePathV1::parse("v1/responses").unwrap();

    let resolved = path.resolve_against(&endpoint).unwrap();

    assert_eq!(resolved.as_str(), "https://example.com/providers/openai/v1/responses");
    assert_eq!(resolved.scheme(), "https");
    assert_eq!(resolved.host_str(), Some("example.com"));
    assert_eq!(resolved.port_or_known_default(), Some(443));
}

#[test]
fn relative_path_resolution_preserves_allowed_percent_encodings() {
    let endpoint = ProviderEndpointV1::parse("https://example.com/providers%3Av1").unwrap();
    let path = RelativePathV1::parse("models%3Ainvoke").unwrap();

    let resolved = path.resolve_against(&endpoint).unwrap();

    assert_eq!(resolved.as_str(), "https://example.com/providers%3Av1/models%3Ainvoke");
}

#[test]
fn credential_slot_accepts_only_the_version_one_identifier_grammar() {
    for valid in ["a", "provider.primary_1", "a-b.c_d9"] {
        assert_eq!(CredentialSlotV1::parse(valid).unwrap().as_str(), valid);
    }

    for invalid in ["", "A", "1slot", ".slot", "slot/child", "slot secret", "slot\nsecret", "slöt"]
    {
        assert_eq!(CredentialSlotV1::parse(invalid), Err(ContractErrorV1::InvalidCredentialSlot));
    }
}

#[test]
fn credential_slot_enforces_its_size_boundary() {
    let at_limit = format!("a{}", "b".repeat(MAX_CREDENTIAL_SLOT_BYTES - 1));
    let over_limit = format!("{at_limit}b");

    assert!(CredentialSlotV1::parse(&at_limit).is_ok());
    assert_eq!(CredentialSlotV1::parse(&over_limit), Err(ContractErrorV1::InvalidCredentialSlot));
}

#[test]
fn json_body_preserves_the_exact_complete_json_value() {
    let input = " \n {\"number\":1.00,\"array\":[true,null]} \t";
    let body = JsonBodyV1::parse(input).unwrap();

    assert_eq!(body.as_str(), input);
    assert_eq!(body.len(), input.len());
}

#[test]
fn json_body_rejects_invalid_or_multiple_values() {
    for invalid in ["", "{", "null true", "{\"key\": NaN}"] {
        assert_eq!(JsonBodyV1::parse(invalid), Err(ContractErrorV1::InvalidJsonBody));
    }
}

#[test]
fn json_body_enforces_its_size_boundary() {
    let at_limit = format!("\"{}\"", "a".repeat(MAX_JSON_REQUEST_BODY_BYTES - 2));
    let over_limit = format!("{at_limit} ");

    assert!(JsonBodyV1::parse(&at_limit).is_ok());
    assert_eq!(JsonBodyV1::parse(&over_limit), Err(ContractErrorV1::RequestBodyTooLarge));
}

#[test]
fn json_body_accepts_a_node_dense_complete_value() {
    let node_count = 250_000;
    let mut input = String::with_capacity(node_count * 5 + 2);
    input.push('[');
    for index in 0..node_count {
        if index != 0 {
            input.push(',');
        }
        input.push_str("null");
    }
    input.push(']');

    let body = JsonBodyV1::parse(&input).unwrap();

    assert_eq!(body.as_str(), input);
}

#[test]
fn request_exposes_only_explicit_read_only_contract_fields() {
    let path = RelativePathV1::parse("v1/responses").unwrap();
    let headers = SafeHeaders::try_from_iter([("content-type", "application/json")]).unwrap();
    let body = JsonBodyV1::parse("{\"input\":\"hello\"}").unwrap();
    let auth = BearerAuthV1::new(CredentialSlotV1::parse("openai.primary").unwrap());
    let request = JsonPostRequestV1::new(path, headers, body, auth);

    assert_eq!(request.relative_path().as_str(), "v1/responses");
    assert_eq!(request.headers().get("content-type"), Some("application/json"));
    assert_eq!(request.body().as_str(), "{\"input\":\"hello\"}");
    assert_eq!(request.auth().credential_slot().as_str(), "openai.primary");
    assert_eq!(request.headers().iter().count(), 1);
    assert_eq!(request.headers().iter().next(), Some(("content-type", "application/json")));
}

#[test]
fn response_exposes_only_status_body_and_two_bounded_metadata_fields() {
    let response = BufferedHttpResponseV1::try_from_parts(
        StatusCode::TOO_MANY_REQUESTS,
        b"{\"error\":\"limited\"}".to_vec(),
        Some("application/json".to_owned()),
        Some("120".to_owned()),
    )
    .unwrap();

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.body(), "{\"error\":\"limited\"}");
    assert_eq!(response.content_type(), Some("application/json"));
    assert_eq!(response.retry_after(), Some("120"));
}

#[test]
fn response_enforces_body_encoding_and_size_boundaries() {
    let at_limit = vec![b'a'; MAX_RESPONSE_BODY_BYTES];
    let over_limit = vec![b'a'; MAX_RESPONSE_BODY_BYTES + 1];

    assert!(BufferedHttpResponseV1::try_from_parts(StatusCode::OK, at_limit, None, None).is_ok());
    assert_eq!(
        BufferedHttpResponseV1::try_from_parts(StatusCode::OK, over_limit, None, None),
        Err(TransportErrorV1::ResponseBodyTooLarge)
    );
    assert_eq!(
        BufferedHttpResponseV1::try_from_parts(StatusCode::OK, vec![0xff], None, None),
        Err(TransportErrorV1::ResponseBodyNotUtf8)
    );
}

#[test]
fn response_rejects_every_redirect_status_before_exposing_a_body() {
    for status in 300..400 {
        let status = StatusCode::from_u16(status).unwrap();
        assert_eq!(
            BufferedHttpResponseV1::try_from_parts(
                status,
                b"redirect-body".to_vec(),
                Some("text/plain".to_owned()),
                None,
            ),
            Err(TransportErrorV1::RedirectDenied)
        );
    }
}

#[test]
fn response_preserves_client_and_server_error_outcomes() {
    for status in [StatusCode::BAD_REQUEST, StatusCode::INTERNAL_SERVER_ERROR] {
        let response = BufferedHttpResponseV1::try_from_parts(
            status,
            b"upstream-error".to_vec(),
            Some("text/plain".to_owned()),
            None,
        )
        .unwrap();

        assert_eq!(response.status(), status);
        assert_eq!(response.body(), "upstream-error");
    }
}

#[test]
fn response_enforces_metadata_size_and_http_value_grammar() {
    let content_type_at_limit = "a".repeat(MAX_RESPONSE_CONTENT_TYPE_BYTES);
    let content_type_over_limit = "a".repeat(MAX_RESPONSE_CONTENT_TYPE_BYTES + 1);
    let retry_after_at_limit = "a".repeat(MAX_RESPONSE_RETRY_AFTER_BYTES);
    let retry_after_over_limit = "a".repeat(MAX_RESPONSE_RETRY_AFTER_BYTES + 1);

    assert!(
        BufferedHttpResponseV1::try_from_parts(
            StatusCode::OK,
            Vec::new(),
            Some(content_type_at_limit),
            None,
        )
        .is_ok()
    );
    assert_eq!(
        BufferedHttpResponseV1::try_from_parts(
            StatusCode::OK,
            Vec::new(),
            Some(content_type_over_limit),
            None,
        ),
        Err(TransportErrorV1::ResponseMetadataInvalid)
    );
    assert!(
        BufferedHttpResponseV1::try_from_parts(
            StatusCode::OK,
            Vec::new(),
            None,
            Some(retry_after_at_limit),
        )
        .is_ok()
    );
    assert_eq!(
        BufferedHttpResponseV1::try_from_parts(
            StatusCode::OK,
            Vec::new(),
            None,
            Some(retry_after_over_limit),
        ),
        Err(TransportErrorV1::ResponseMetadataInvalid)
    );
    assert_eq!(
        BufferedHttpResponseV1::try_from_parts(
            StatusCode::OK,
            Vec::new(),
            None,
            Some("120\r\nset-cookie: secret".to_owned()),
        ),
        Err(TransportErrorV1::ResponseMetadataInvalid)
    );
}

#[test]
fn stable_error_codes_are_exact_and_exhaustive() {
    let contract = [
        (ContractErrorV1::InvalidEndpoint, "INVALID_ENDPOINT"),
        (ContractErrorV1::InvalidRelativePath, "INVALID_RELATIVE_PATH"),
        (ContractErrorV1::InvalidCredentialSlot, "INVALID_CREDENTIAL_SLOT"),
        (ContractErrorV1::InvalidJsonBody, "INVALID_JSON_BODY"),
        (ContractErrorV1::RequestBodyTooLarge, "REQUEST_BODY_TOO_LARGE"),
    ];
    let preparation = [
        (PreparationErrorV1::UrlOutsideBinding, "URL_OUTSIDE_BINDING"),
        (PreparationErrorV1::CredentialBindingMismatch, "CREDENTIAL_BINDING_MISMATCH"),
        (PreparationErrorV1::CredentialResolutionFailed, "CREDENTIAL_RESOLUTION_FAILED"),
        (PreparationErrorV1::Cancelled, "CANCELLED"),
        (PreparationErrorV1::DeadlineExceeded, "DEADLINE_EXCEEDED"),
    ];
    let transport = [
        (TransportErrorV1::ClientBuildFailed, "CLIENT_BUILD_FAILED"),
        (TransportErrorV1::TransportTimeout, "TRANSPORT_TIMEOUT"),
        (TransportErrorV1::ConnectFailed, "CONNECT_FAILED"),
        (TransportErrorV1::RequestFailed, "REQUEST_FAILED"),
        (TransportErrorV1::ResponseReadFailed, "RESPONSE_READ_FAILED"),
        (TransportErrorV1::ResponseBodyTooLarge, "RESPONSE_BODY_TOO_LARGE"),
        (TransportErrorV1::ResponseBodyNotUtf8, "RESPONSE_BODY_NOT_UTF8"),
        (TransportErrorV1::ResponseMetadataInvalid, "RESPONSE_METADATA_INVALID"),
        (TransportErrorV1::RedirectDenied, "REDIRECT_DENIED"),
    ];

    for (error, code) in contract {
        assert_eq!(error.code(), code);
    }
    for (error, code) in preparation {
        assert_eq!(error.code(), code);
    }
    for (error, code) in transport {
        assert_eq!(error.code(), code);
    }
}

#[test]
fn debug_and_error_output_redact_all_untrusted_contract_values() {
    let endpoint = ProviderEndpointV1::parse(&format!("https://example.com/{SENTINEL}")).unwrap();
    let path = RelativePathV1::parse(&format!("v1/{SENTINEL}")).unwrap();
    let slot = CredentialSlotV1::parse(&format!("a.{SENTINEL}")).unwrap();
    let headers = SafeHeaders::try_from_iter([("x-sentinel", SENTINEL)]).unwrap();
    let body = JsonBodyV1::parse(&format!("\"{SENTINEL}\"")).unwrap();
    let header_secret_auth = ProviderAuthV1::HeaderSecret {
        header: SecretHeaderV1::XApiKey,
        slot: BearerAuthV1::new(slot.clone()),
    };
    let request = JsonPostRequestV1::new(path, headers, body, BearerAuthV1::new(slot));
    let response = BufferedHttpResponseV1::try_from_parts(
        StatusCode::OK,
        SENTINEL.as_bytes().to_vec(),
        Some(format!("application/{SENTINEL}")),
        Some(SENTINEL.to_owned()),
    )
    .unwrap();

    for output in [
        format!("{endpoint:?}"),
        format!("{:?}", request.relative_path()),
        format!("{:?}", request.auth().credential_slot()),
        format!("{:?}", request.auth()),
        format!("{header_secret_auth:?}"),
        format!("{:?}", request.body()),
        format!("{request:?}"),
        format!("{response:?}"),
    ] {
        assert!(!output.contains(SENTINEL));
    }

    let endpoint_error = ProviderEndpointV1::parse(SENTINEL).unwrap_err();
    let path_error = RelativePathV1::parse(&format!("{SENTINEL} query")).unwrap_err();
    let slot_error = CredentialSlotV1::parse(&format!("A{SENTINEL}")).unwrap_err();
    let body_error = JsonBodyV1::parse(&format!("{{{SENTINEL}")).unwrap_err();
    let error_outputs = [
        endpoint_error.to_string(),
        format!("{endpoint_error:?}"),
        path_error.to_string(),
        format!("{path_error:?}"),
        slot_error.to_string(),
        format!("{slot_error:?}"),
        body_error.to_string(),
        format!("{body_error:?}"),
        PreparationErrorV1::CredentialResolutionFailed.to_string(),
        format!("{:?}", PreparationErrorV1::CredentialResolutionFailed),
        TransportErrorV1::RequestFailed.to_string(),
        format!("{:?}", TransportErrorV1::RequestFailed),
    ];
    for output in error_outputs {
        assert!(!output.contains(SENTINEL));
    }
}
