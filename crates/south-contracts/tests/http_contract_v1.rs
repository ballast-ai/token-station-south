use http::StatusCode;
use south_contracts::{
    AUTH_CONTRACT_VERSION, BearerAuthV1, BufferedHttpResponseV1, ContractErrorV1,
    ControlledUserAgentV1, CredentialSlotV1, ERROR_CONTRACT_VERSION, GetRequestV1,
    HTTP_CONTRACT_VERSION, JsonBodyV1, JsonPostRequestV1, MAX_CREDENTIAL_SLOT_BYTES,
    MAX_ENDPOINT_BYTES, MAX_JSON_REQUEST_BODY_BYTES, MAX_MULTIPART_BOUNDARY_BYTES,
    MAX_QUERY_VALUE_BYTES, MAX_RELATIVE_PATH_BYTES, MAX_RESPONSE_BODY_BYTES,
    MAX_RESPONSE_CONTENT_TYPE_BYTES, MAX_RESPONSE_RETRY_AFTER_BYTES, MAX_USER_AGENT_BYTES,
    MultipartBodyV1, MultipartBoundaryV1, MultipartPostRequestV1, PreparationErrorV1,
    ProviderAuthV1, ProviderEndpointV1, QueryParameterV1, QueryStringV1, RelativePathV1,
    STREAM_CONTRACT_VERSION, SafeHeaders, SecretHeaderV1, SignedHeaderSetErrorV1,
    SignedHeaderSetV1, SignedHeaderV1, TransportErrorV1,
};

const SENTINEL: &str = "must-not-appear-7f23a";

/// Every sanctioned secret-bearing header in canonical table order. The match below is
/// deliberately exhaustive so adding a `SecretHeaderV1` variant fails compilation here until the
/// list, the reserved-header pin, and the conformance surface are all updated together.
/// The published set is the single source of truth for every suite that must cover all sanctioned
/// headers; [`secret_header_all_covers_every_variant`] proves it is complete.
const ALL_SECRET_HEADERS: [SecretHeaderV1; 5] = SecretHeaderV1::ALL;

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
fn secret_header_all_covers_every_variant() {
    // Adding a variant without extending `SecretHeaderV1::ALL` would silently shrink the coverage
    // of every suite that iterates it, so prove completeness two ways: the exhaustive match rejects
    // an unlisted variant at compile time, and distinct wire names prove no slot is duplicated to
    // pad the array to its declared length.
    for header in SecretHeaderV1::ALL {
        assert_secret_header_listed(header);
    }
    let mut names: Vec<&str> =
        SecretHeaderV1::ALL.iter().map(SecretHeaderV1::header_name).collect();
    names.sort_unstable();
    let total = names.len();
    names.dedup();
    assert_eq!(names.len(), total, "SecretHeaderV1::ALL must not repeat a variant");
}

#[test]
fn contract_versions_are_independently_versioned() {
    assert_eq!(HTTP_CONTRACT_VERSION, 7);
    assert_eq!(AUTH_CONTRACT_VERSION, 4);
    assert_eq!(ERROR_CONTRACT_VERSION, 2);
    assert_eq!(STREAM_CONTRACT_VERSION, Some(2));
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
        (PreparationErrorV1::UnsupportedAuthShape, "UNSUPPORTED_AUTH_SHAPE"),
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

// ───────────────── controlled query (HTTP contract v2) ─────────────────

/// Every sanctioned query parameter, in canonical table order. The exhaustive match below fails
/// compilation when a variant is added, so the list, the value grammar, and the conformance
/// surface must all be updated together.
const ALL_QUERY_PARAMETERS: [QueryParameterV1; 4] = QueryParameterV1::ALL;

const fn assert_query_parameter_listed(parameter: QueryParameterV1) {
    match parameter {
        QueryParameterV1::ApiVersion
        | QueryParameterV1::Alt
        | QueryParameterV1::GroupId
        | QueryParameterV1::TaskId => (),
    }
}

#[test]
fn query_parameter_all_covers_every_variant() {
    for parameter in QueryParameterV1::ALL {
        assert_query_parameter_listed(parameter);
    }
    let mut names: Vec<&str> =
        ALL_QUERY_PARAMETERS.iter().map(QueryParameterV1::wire_name).collect();
    names.sort_unstable();
    let total = names.len();
    names.dedup();
    assert_eq!(names.len(), total, "QueryParameterV1::ALL must not repeat a variant");
}

#[test]
fn sanctioned_query_names_cannot_express_a_credential() {
    // The frozen set is the structural answer to query-borne secret exfiltration: a provider
    // cannot name `key`, `access_token`, or `sig` because those names do not exist in the type.
    for forbidden in ["key", "access_token", "sig", "signature", "password", "token"] {
        assert!(
            !ALL_QUERY_PARAMETERS.iter().any(|p| p.wire_name() == forbidden),
            "{forbidden} must never become a sanctioned query parameter"
        );
    }
}

#[test]
fn api_version_grammar_accepts_real_values_and_rejects_separators() {
    for accepted in ["2024-10-21", "2025-04-01-preview", "v1", "1.0", "a_b"] {
        assert!(
            QueryStringV1::try_from_iter([(QueryParameterV1::ApiVersion, accepted)]).is_ok(),
            "{accepted} is a real Azure api-version"
        );
    }
    // Separator-only values pass the character class but are meaningless as versions and read as
    // dot segments to anything that re-joins the URL.
    for rejected in
        ["", "a b", "a&b=c", "a%2Fb", "a#b", "a?b", "a/b", "a=b", ".", "..", "--", "._-"]
    {
        assert_eq!(
            QueryStringV1::try_from_iter([(QueryParameterV1::ApiVersion, rejected)]),
            Err(ContractErrorV1::InvalidQueryValue),
            "{rejected:?} must not survive the api-version grammar"
        );
    }
}

#[test]
fn alt_grammar_is_a_closed_value_set() {
    for accepted in ["sse", "json"] {
        assert!(QueryStringV1::try_from_iter([(QueryParameterV1::Alt, accepted)]).is_ok());
    }
    for rejected in ["SSE", "xml", "sse ", "", "sse&alt=json"] {
        assert_eq!(
            QueryStringV1::try_from_iter([(QueryParameterV1::Alt, rejected)]),
            Err(ContractErrorV1::InvalidQueryValue),
            "{rejected:?} is not a sanctioned alt value"
        );
    }
}

#[test]
fn group_id_grammar_is_a_bounded_digit_string() {
    // Real MiniMax group ids are decimal account identifiers; `19000` is the documentation
    // example and the nineteen-digit form is what the platform issues today.
    for accepted in ["19000", "1782000000000000000", "0"] {
        assert!(
            QueryStringV1::try_from_iter([(QueryParameterV1::GroupId, accepted)]).is_ok(),
            "{accepted} is a well-formed group id"
        );
    }
    // Anything that is not a bare digit string is refused: signs, separators, letters, a
    // smuggled second parameter, and the empty value.
    for rejected in ["", "-1", "+1", "1 9", "19000&alt=sse", "abc", "19000#", "１９", " 19000"] {
        assert_eq!(
            QueryStringV1::try_from_iter([(QueryParameterV1::GroupId, rejected)]),
            Err(ContractErrorV1::InvalidQueryValue),
            "{rejected:?} must not survive the GroupId grammar"
        );
    }
    let too_long = "9".repeat(MAX_QUERY_VALUE_BYTES + 1);
    assert_eq!(
        QueryStringV1::try_from_iter([(QueryParameterV1::GroupId, too_long.as_str())]),
        Err(ContractErrorV1::InvalidQueryValue)
    );
    // The wire name keeps the upstream's exact casing and sorts after the two older parameters.
    assert_eq!(QueryParameterV1::GroupId.wire_name(), "GroupId");
    let query = QueryStringV1::try_from_iter([
        (QueryParameterV1::GroupId, "19000"),
        (QueryParameterV1::ApiVersion, "v1"),
    ])
    .expect("two distinct sanctioned parameters construct");
    assert_eq!(query.as_str(), "api-version=v1&GroupId=19000");
}

// ───────────────── buffered GET request (HTTP contract v6) ─────────────────

#[test]
fn task_id_grammar_is_a_bounded_digit_string_appended_last() {
    // Real MiniMax task ids are decimal identifiers; the fifteen-digit form is what the platform
    // returns from `video_generation` today.
    for accepted in ["276843862449040", "0", "1"] {
        assert!(
            QueryStringV1::try_from_iter([(QueryParameterV1::TaskId, accepted)]).is_ok(),
            "{accepted} is a well-formed task id"
        );
    }
    // The grammar is digits only (buffered-GET record, D3): no signs, separators, letters, UUID
    // shapes, smuggled parameters, or the empty value. A host meeting a non-numeric task id
    // falls back rather than widening the grammar.
    for rejected in [
        "",
        "-1",
        "+1",
        "1 9",
        "1&alt=sse",
        "abc",
        "1#",
        "１９",
        " 1",
        "cgt-20240101-abc",
        "3f0a9c2e-1b4d-4c8e-9f2a-7d6b5c4a3e21",
    ] {
        assert_eq!(
            QueryStringV1::try_from_iter([(QueryParameterV1::TaskId, rejected)]),
            Err(ContractErrorV1::InvalidQueryValue),
            "{rejected:?} must not survive the task_id grammar"
        );
    }
    let too_long = "9".repeat(MAX_QUERY_VALUE_BYTES + 1);
    assert_eq!(
        QueryStringV1::try_from_iter([(QueryParameterV1::TaskId, too_long.as_str())]),
        Err(ContractErrorV1::InvalidQueryValue)
    );
    // The wire name is the upstream's snake case, and it sorts after every older parameter.
    assert_eq!(QueryParameterV1::TaskId.wire_name(), "task_id");
    let query = QueryStringV1::try_from_iter([
        (QueryParameterV1::TaskId, "276843862449040"),
        (QueryParameterV1::GroupId, "19000"),
        (QueryParameterV1::ApiVersion, "v1"),
    ])
    .expect("three distinct sanctioned parameters construct");
    assert_eq!(query.as_str(), "api-version=v1&GroupId=19000&task_id=276843862449040");
}

#[test]
fn get_request_is_the_post_field_set_minus_the_body() {
    let path = RelativePathV1::parse("v1/query/video_generation").unwrap();
    let headers = SafeHeaders::try_from_iter([("accept", "application/json")]).unwrap();
    let auth = BearerAuthV1::new(CredentialSlotV1::parse("minimax.primary").unwrap());
    let request = GetRequestV1::new(path, headers, auth);

    assert_eq!(request.relative_path().as_str(), "v1/query/video_generation");
    assert_eq!(request.headers().get("accept"), Some("application/json"));
    assert_eq!(request.headers().iter().count(), 1);
    assert_eq!(request.auth().credential_slot().as_str(), "minimax.primary");
    assert!(request.query().is_none(), "a GET declares no query until one is attached");
    assert!(request.user_agent().is_none());

    let query = QueryStringV1::try_from_iter([(QueryParameterV1::TaskId, "276843862449040")])
        .expect("the task id fixture satisfies its grammar");
    let user_agent = ControlledUserAgentV1::try_from_static("south-test/1.0").unwrap();
    let request = request.with_query(query.clone()).with_user_agent(user_agent);
    assert_eq!(request.query(), Some(&query));
    assert_eq!(
        request.user_agent().map(|agent| agent.as_str().to_owned()),
        Some("south-test/1.0".to_owned())
    );
}

#[test]
fn get_request_accepts_every_credential_arm_through_one_constructor() {
    let path = RelativePathV1::parse("v1/videos/276843862449040").unwrap();
    let headers = SafeHeaders::try_from_iter([("accept", "application/json")]).unwrap();
    let slot = CredentialSlotV1::parse("primary").unwrap();

    let bearer = GetRequestV1::new(path.clone(), headers.clone(), BearerAuthV1::new(slot.clone()));
    assert!(matches!(bearer.auth(), ProviderAuthV1::Bearer(_)));

    let header_secret = GetRequestV1::new(
        path.clone(),
        headers.clone(),
        ProviderAuthV1::HeaderSecret {
            header: SecretHeaderV1::XApiKey,
            slot: BearerAuthV1::new(slot.clone()),
        },
    );
    assert!(matches!(header_secret.auth(), ProviderAuthV1::HeaderSecret { .. }));

    let combined = GetRequestV1::new(
        path.clone(),
        headers.clone(),
        ProviderAuthV1::BearerAndHeaderSecret {
            header: SecretHeaderV1::XGoogApiKey,
            slot: BearerAuthV1::new(slot.clone()),
        },
    );
    assert!(matches!(combined.auth(), ProviderAuthV1::BearerAndHeaderSecret { .. }));

    let signed = GetRequestV1::new(
        path,
        headers,
        ProviderAuthV1::HostSigned {
            slot: BearerAuthV1::new(slot),
            emits: SignedHeaderSetV1::new(&[SignedHeaderV1::Authorization]).unwrap(),
        },
    );
    assert!(matches!(signed.auth(), ProviderAuthV1::HostSigned { .. }));
}

#[test]
fn get_request_debug_shows_shape_only() {
    let request = GetRequestV1::new(
        RelativePathV1::parse(&format!("v1/{SENTINEL}")).unwrap(),
        SafeHeaders::try_from_iter([("x-test", SENTINEL)]).unwrap(),
        BearerAuthV1::new(CredentialSlotV1::parse(SENTINEL).unwrap()),
    )
    .with_query(QueryStringV1::try_from_iter([(QueryParameterV1::TaskId, "7")]).unwrap());
    let rendered = format!("{request:?}");
    assert_eq!(
        rendered,
        format!(
            "GetRequestV1 {{ http_contract_version: {HTTP_CONTRACT_VERSION}, \
             auth_contract_version: {AUTH_CONTRACT_VERSION}, header_count: 1, has_query: true, \
             has_user_agent: false, .. }}"
        )
    );
    assert!(!rendered.contains(SENTINEL));
    assert!(!rendered.contains("task_id=7"));
}

#[test]
fn query_rejects_duplicate_parameters_rather_than_normalizing() {
    // Parameter pollution — gateway and upstream disagreeing on which duplicate wins — is the
    // classic failure of permissive query handling, so a duplicate is a contract error.
    assert_eq!(
        QueryStringV1::try_from_iter([
            (QueryParameterV1::ApiVersion, "v1"),
            (QueryParameterV1::ApiVersion, "2024-10-21"),
        ]),
        Err(ContractErrorV1::DuplicateQueryParameter)
    );
}

#[test]
fn query_serializes_in_canonical_declaration_order() {
    let query = QueryStringV1::try_from_iter([
        (QueryParameterV1::Alt, "sse"),
        (QueryParameterV1::ApiVersion, "v1"),
    ])
    .expect("both parameters are sanctioned");
    assert_eq!(query.as_str(), "api-version=v1&alt=sse");
}

#[test]
fn query_debug_never_reveals_values() {
    let query = QueryStringV1::try_from_iter([(QueryParameterV1::ApiVersion, SENTINEL_VERSION)])
        .expect("sentinel matches the grammar");
    let rendered = format!("{query:?}");
    assert!(!rendered.contains(SENTINEL_VERSION), "query Debug must not print values");
}

const SENTINEL_VERSION: &str = "must-not-appear-9c41b";

#[test]
fn request_without_query_is_unchanged_by_the_v2_contract() {
    let request = JsonPostRequestV1::new(
        RelativePathV1::parse("v1/chat/completions").unwrap(),
        SafeHeaders::try_from_iter([("accept", "application/json")]).unwrap(),
        JsonBodyV1::parse("{}").unwrap(),
        BearerAuthV1::new(CredentialSlotV1::parse("openai").unwrap()),
    );
    assert!(request.query().is_none(), "a v1 request is exactly a v2 request with no query");
}

#[test]
fn relative_path_grammar_still_rejects_query_and_fragment() {
    // The path grammar is deliberately untouched by this slice: a query attaches to the request,
    // never to the path, so the frozen grammar and its fuzz invariants stay intact.
    for rejected in ["v1/chat?api-version=v1", "v1/chat#frag"] {
        assert_eq!(RelativePathV1::parse(rejected), Err(ContractErrorV1::InvalidRelativePath));
    }
}

#[test]
fn a_query_with_no_parameters_is_a_contract_error() {
    // `Some(query)` on a request must mean "there is a query on the wire". An empty declaration
    // would serialize to `""` and produce a bare trailing `?`, which is a different URL from the
    // one the caller meant — so it is rejected at construction rather than normalized away.
    let empty: [(QueryParameterV1, &str); 0] = [];
    assert_eq!(QueryStringV1::try_from_iter(empty), Err(ContractErrorV1::EmptyQuery));
}

#[test]
fn set_query_is_an_identity_map_on_every_accepted_value() {
    // `resolve_against_with_query` proves the wire query equals the declaration byte for byte.
    // That check is only meaningful while the grammar admits solely query-safe bytes: the `url`
    // crate percent-encodes `#`, space, and NUL, and silently *deletes* CR and LF. No accepted
    // value hits any of those today, which is exactly why no positive test can falsify the check
    // — so pin the property the check depends on instead. Relaxing a grammar to admit `%`, `+`,
    // `:`, or whitespace (plausible when `GroupId`/`task_id` land) breaks this test first.
    let endpoint = ProviderEndpointV1::parse("https://example.com/base/").unwrap();
    let path = RelativePathV1::parse("v1/resource").unwrap();
    let accepted_values: [(QueryParameterV1, &[&str]); 4] = [
        (
            QueryParameterV1::ApiVersion,
            &["2024-10-21", "2025-04-01-preview", "v1", "1.0", "a_b", "A-Z.0_9"],
        ),
        (QueryParameterV1::Alt, &["sse", "json"]),
        (QueryParameterV1::GroupId, &["19000", "1782000000000000000"]),
        (QueryParameterV1::TaskId, &["0", "276843862449040"]),
    ];

    for (parameter, values) in accepted_values {
        for value in values {
            let query = QueryStringV1::try_from_iter([(parameter, *value)])
                .expect("fixture value must match the grammar");
            let resolved = path
                .resolve_against_with_query(&endpoint, Some(&query))
                .expect("an accepted query must resolve inside the binding");
            assert_eq!(
                resolved.query(),
                Some(query.as_str()),
                "{parameter:?}={value} must survive set_query unchanged"
            );
            // The query must never move the path, whatever the value.
            assert_eq!(resolved.path(), "/base/v1/resource");
            assert!(resolved.fragment().is_none());
        }
    }
}

#[test]
fn a_query_free_request_must_reach_the_wire_without_a_query() {
    // The `None` arm of the intactness check is the version-one regression guard: a path alone
    // must never acquire a query through the join.
    let endpoint = ProviderEndpointV1::parse("https://example.com/base/").unwrap();
    let path = RelativePathV1::parse("v1/resource").unwrap();
    let resolved = path.resolve_against(&endpoint).expect("a bare path must resolve");
    assert_eq!(resolved.query(), None);
    assert_eq!(resolved.as_str(), "https://example.com/base/v1/resource");
}

// ─────────────── controlled user-agent (HTTP contract v3) ───────────────

const SENTINEL_USER_AGENT: &str = "must-not-appear-4e87d/1.0";

/// The constructor is `const fn`, so a host builds its user-agents in `const` context and a
/// malformed literal fails at host compile time rather than at request time.
const CONST_CONTEXT_USER_AGENT: ControlledUserAgentV1 =
    match ControlledUserAgentV1::try_from_static("claude-cli/2.1.114 (external, cli)") {
        Ok(user_agent) => user_agent,
        Err(_) => panic!("a known-good literal must construct in const context"),
    };

#[test]
fn user_agent_constructs_in_const_context_and_round_trips() {
    assert_eq!(CONST_CONTEXT_USER_AGENT.as_str(), "claude-cli/2.1.114 (external, cli)");
}

#[test]
fn user_agent_grammar_accepts_every_audited_host_value() {
    // The four values measured in the adopting host's inventory (design record §1), plus the
    // shortest accepted shape. Product tokens, slashes, dots, parentheses, commas, and single
    // interior spaces must all survive verbatim.
    for accepted in [
        "opencode/1.15.6",
        "GitHubCopilotChat/0.43.0",
        "aws-sdk-js/1.0.0 KiroIDE",
        "claude-cli/2.1.114 (external, cli)",
        "a",
    ] {
        let user_agent = ControlledUserAgentV1::try_from_static(accepted)
            .expect("every audited inventory value must be accepted");
        assert_eq!(user_agent.as_str(), accepted);
    }
}

#[test]
fn user_agent_grammar_rejects_every_unsafe_byte_class() {
    // The grammar is deliberately narrower than an HTTP header value: control bytes and CR/LF
    // close header injection, non-ASCII closes encoding ambiguity, and edge spaces close
    // whitespace-trimming disagreements between intermediaries.
    for rejected in [
        "",
        " leading-space",
        "trailing-space ",
        " ",
        "line\nbreak",
        "line\rbreak",
        "tab\tseparated",
        "del\u{7f}byte",
        "ctl\u{1f}byte",
        "smart\u{201d}quote",
        "\u{6a21}\u{578b}/1.0",
    ] {
        assert_eq!(
            ControlledUserAgentV1::try_from_static(rejected),
            Err(ContractErrorV1::InvalidUserAgentValue),
            "{rejected:?} must not survive the user-agent grammar"
        );
    }
}

#[test]
fn user_agent_enforces_its_size_boundary() {
    // `String::leak` manufactures the `'static` inputs here; the design record names that escape
    // hatch as the reason `'static` provenance is a discipline claim, not a proof.
    let at_limit: &'static str = "a".repeat(MAX_USER_AGENT_BYTES).leak();
    let over_limit: &'static str = "a".repeat(MAX_USER_AGENT_BYTES + 1).leak();

    assert!(ControlledUserAgentV1::try_from_static(at_limit).is_ok());
    assert_eq!(
        ControlledUserAgentV1::try_from_static(over_limit),
        Err(ContractErrorV1::InvalidUserAgentValue)
    );
}

#[test]
fn request_without_user_agent_is_unchanged_by_the_v3_contract() {
    let request = JsonPostRequestV1::new(
        RelativePathV1::parse("v1/chat/completions").unwrap(),
        SafeHeaders::try_from_iter([("accept", "application/json")]).unwrap(),
        JsonBodyV1::parse("{}").unwrap(),
        BearerAuthV1::new(CredentialSlotV1::parse("openai").unwrap()),
    );
    assert!(
        request.user_agent().is_none(),
        "a v2 request is exactly a v3 request with no user-agent"
    );
}

#[test]
fn request_carries_the_declared_user_agent() {
    let user_agent = ControlledUserAgentV1::try_from_static("opencode/1.15.6").unwrap();
    let request = JsonPostRequestV1::new(
        RelativePathV1::parse("v1/chat/completions").unwrap(),
        SafeHeaders::try_from_iter([("accept", "application/json")]).unwrap(),
        JsonBodyV1::parse("{}").unwrap(),
        BearerAuthV1::new(CredentialSlotV1::parse("glm-coding").unwrap()),
    )
    .with_user_agent(user_agent);

    assert_eq!(request.user_agent().map(ControlledUserAgentV1::as_str), Some("opencode/1.15.6"));
}

#[test]
fn user_agent_stays_on_the_reserved_header_list() {
    // The sanctioned field is an opt-in, not a relaxation: the ordinary header channel must keep
    // refusing the name, which is what makes the wire's exactly-once property structural.
    let error = SafeHeaders::try_from_iter([("user-agent", "smuggled/1.0")])
        .expect_err("user-agent must stay reserved");
    assert_eq!(error.code(), "RESERVED_HEADER_FORBIDDEN");
}

#[test]
fn invalid_user_agent_error_code_is_stable() {
    assert_eq!(ContractErrorV1::InvalidUserAgentValue.code(), "INVALID_USER_AGENT_VALUE");
}

#[test]
fn user_agent_debug_never_reveals_the_value() {
    let user_agent = ControlledUserAgentV1::try_from_static(SENTINEL_USER_AGENT)
        .expect("the sentinel matches the grammar");
    let request = JsonPostRequestV1::new(
        RelativePathV1::parse("v1/chat/completions").unwrap(),
        SafeHeaders::try_from_iter([("accept", "application/json")]).unwrap(),
        JsonBodyV1::parse("{}").unwrap(),
        BearerAuthV1::new(CredentialSlotV1::parse("glm-coding").unwrap()),
    )
    .with_user_agent(user_agent);

    for output in [format!("{user_agent:?}"), format!("{request:?}")] {
        assert!(!output.contains("must-not-appear"), "debug output leaked the value: {output}");
    }
}

/// Every permitted signed header in canonical table order. Exhaustive on purpose: adding a
/// `SignedHeaderV1` variant fails compilation here until the list, the reserved-header pin, and
/// the host-signed conformance surface are updated together.
const ALL_SIGNED_HEADERS: [SignedHeaderV1; 4] = SignedHeaderV1::ALL;

const fn assert_signed_header_listed(header: SignedHeaderV1) {
    match header {
        SignedHeaderV1::Authorization
        | SignedHeaderV1::XAmzDate
        | SignedHeaderV1::XAmzContentSha256
        | SignedHeaderV1::XAmzSecurityToken => {}
    }
}

#[test]
fn signed_header_names_are_frozen_and_stay_on_the_reserved_list() {
    // Host-signed D1. The reserved-list half is the load-bearing one: a signed header that the
    // plain channel could also carry would let a provider set the very bytes the signature is
    // computed over, from a channel the signer never sees.
    let expected_names =
        ["authorization", "x-amz-date", "x-amz-content-sha256", "x-amz-security-token"];
    for (header, expected_name) in ALL_SIGNED_HEADERS.into_iter().zip(expected_names) {
        assert_signed_header_listed(header);
        assert_eq!(header.header_name(), expected_name);

        let error = SafeHeaders::try_from_iter([(header.header_name(), SENTINEL)])
            .expect_err("every permitted signed header must stay reserved");
        assert_eq!(error.code(), "RESERVED_HEADER_FORBIDDEN");
    }

    let mut names: Vec<&str> = ALL_SIGNED_HEADERS.iter().map(|h| h.header_name()).collect();
    names.sort_unstable();
    let total = names.len();
    names.dedup();
    assert_eq!(names.len(), total, "SignedHeaderV1::ALL must not repeat a variant");
}

#[test]
fn a_signed_header_set_normalizes_order_and_refuses_empty_or_duplicate() {
    // Normalization means two declarations naming the same headers are the same declaration.
    // Without it the allow-list diff would have to be order-insensitive at every comparison site,
    // and one site that forgot would reject a valid signer.
    let written_backwards =
        SignedHeaderSetV1::new(&[SignedHeaderV1::XAmzSecurityToken, SignedHeaderV1::Authorization])
            .expect("a two-header declaration is valid in any order");
    let written_forwards =
        SignedHeaderSetV1::new(&[SignedHeaderV1::Authorization, SignedHeaderV1::XAmzSecurityToken])
            .expect("same two headers");
    assert_eq!(written_backwards, written_forwards);
    assert_eq!(
        written_backwards.headers(),
        [SignedHeaderV1::Authorization, SignedHeaderV1::XAmzSecurityToken]
    );
    assert!(written_backwards.contains(SignedHeaderV1::Authorization));
    assert!(!written_backwards.contains(SignedHeaderV1::XAmzDate));
    assert_eq!(written_backwards.len(), 2);
    assert!(!written_backwards.is_empty());

    // An empty declaration would make the diff vacuous: emit nothing, satisfy everything.
    assert_eq!(SignedHeaderSetV1::new(&[]), Err(SignedHeaderSetErrorV1::Empty));
    assert_eq!(
        SignedHeaderSetV1::new(&[SignedHeaderV1::XAmzDate, SignedHeaderV1::XAmzDate]),
        Err(SignedHeaderSetErrorV1::Duplicate)
    );
}

#[test]
fn the_host_signed_arm_declares_a_slot_and_its_emitted_headers() {
    let slot = CredentialSlotV1::parse("aws.bedrock.primary").unwrap();
    let emits = SignedHeaderSetV1::new(&SignedHeaderV1::ALL).expect("full set is valid");
    let auth =
        ProviderAuthV1::HostSigned { slot: BearerAuthV1::new(slot.clone()), emits: emits.clone() };

    // The slot participates in the binding check exactly as the other two arms do, even though
    // South never resolves it (host-signed D2).
    assert_eq!(auth.credential_slot(), &slot);
    assert!(
        matches!(&auth, ProviderAuthV1::HostSigned { emits: declared, .. } if *declared == emits)
    );

    // Debug must not become a credential leak vector as the arm count grows.
    let rendered = format!("{auth:?}");
    assert!(rendered.contains("HostSigned"), "{rendered}");
    assert!(!rendered.contains(SENTINEL), "{rendered}");
}

#[test]
fn the_finalizer_preparation_errors_carry_stable_codes() {
    assert_eq!(PreparationErrorV1::RequestFinalizationFailed.code(), "REQUEST_FINALIZATION_FAILED");
    assert_eq!(
        PreparationErrorV1::RequestFinalizationRejected.code(),
        "REQUEST_FINALIZATION_REJECTED"
    );
}

// ───────────────── multipart request body (HTTP contract v7) ─────────────────

const SENTINEL_BOUNDARY: &str = "boundary-sentinel";

/// A well-formed body for `boundary`: one text part, opened and closed by the delimiter.
fn multipart_body_for(boundary: &str) -> Vec<u8> {
    format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nm\r\n--{boundary}--\r\n"
    )
    .into_bytes()
}

fn boundary(value: &str) -> MultipartBoundaryV1 {
    MultipartBoundaryV1::parse(value).expect("fixture boundary satisfies RFC 2046 §5.1.1")
}

#[test]
fn boundary_grammar_is_rfc_2046_unwidened() {
    // Every character class the RFC admits, including a space in an interior position.
    let longest = "b".repeat(MAX_MULTIPART_BOUNDARY_BYTES);
    for accepted in [
        "simple",
        "----WebKitFormBoundary7MA4YWxkTrZu0gW",
        "a",
        "0",
        "'()+_,-./:=?",
        "has space inside",
        longest.as_str(),
    ] {
        assert!(
            MultipartBoundaryV1::parse(accepted).is_ok(),
            "{accepted:?} is a well-formed boundary"
        );
    }
    // The grammar is not widened for convenience: no empty value, nothing over seventy bytes, no
    // character outside `bchars`, and no trailing space — that last one the `content-type`
    // parameter cannot represent unambiguously beside the value.
    let too_long = "b".repeat(MAX_MULTIPART_BOUNDARY_BYTES + 1);
    for rejected in [
        "",
        "trailing ",
        "quote\"inside",
        "semi;colon",
        "back\\slash",
        "at@sign",
        "percent%",
        "brace{",
        "tab\there",
        "new\nline",
        "café",
        too_long.as_str(),
    ] {
        assert_eq!(
            MultipartBoundaryV1::parse(rejected),
            Err(ContractErrorV1::InvalidMultipartBoundary),
            "{rejected:?} must not survive the boundary grammar"
        );
    }
    assert_eq!(MAX_MULTIPART_BOUNDARY_BYTES, 70, "RFC 2046 §5.1.1");
}

#[test]
fn a_multipart_body_must_be_delimited_by_the_boundary_it_declares() {
    let good = MultipartBodyV1::parse(multipart_body_for("edge"), boundary("edge"))
        .expect("a body delimited by its own boundary is valid");
    assert_eq!(good.boundary().as_str(), "edge");
    assert_eq!(good.content_type(), "multipart/form-data; boundary=edge");
    assert!(!good.is_empty());

    // The failure this type exists to catch: bytes delimited by a *different* boundary than the
    // one travelling in the media type. A splice that damaged the delimiter produces exactly
    // this, and nothing downstream could tell.
    assert_eq!(
        MultipartBodyV1::parse(multipart_body_for("other"), boundary("edge")),
        Err(ContractErrorV1::InvalidMultipartBody)
    );
    // Truncated: opens correctly, never closes.
    let truncated = b"--edge\r\nX: 1\r\n\r\nm\r\n".to_vec();
    assert_eq!(
        MultipartBodyV1::parse(truncated, boundary("edge")),
        Err(ContractErrorV1::InvalidMultipartBody)
    );
    assert_eq!(
        MultipartBodyV1::parse(Vec::new(), boundary("edge")),
        Err(ContractErrorV1::InvalidMultipartBody)
    );
    // Both line endings a real encoder emits after the closing delimiter, plus none at all.
    for tail in ["", "\r\n", "\n"] {
        let body = format!("--edge\r\nX: 1\r\n\r\nm\r\n--edge--{tail}").into_bytes();
        assert!(
            MultipartBodyV1::parse(body, boundary("edge")).is_ok(),
            "closing delimiter followed by {tail:?} must be accepted"
        );
    }
    // An RFC-legal epilogue is refused, deliberately: scanning a hundred-megabyte body for an
    // interior delimiter is quadratic, and every real encoder terminates at the end. Its host
    // falls back to the legacy path rather than South widening the rule.
    let with_epilogue = b"--edge\r\nX: 1\r\n\r\nm\r\n--edge--\r\nepilogue".to_vec();
    assert_eq!(
        MultipartBodyV1::parse(with_epilogue, boundary("edge")),
        Err(ContractErrorV1::InvalidMultipartBody)
    );
}

#[test]
fn a_multipart_body_shares_one_allocation_and_redacts_its_debug() {
    let bytes = multipart_body_for(SENTINEL_BOUNDARY);
    let expected = bytes.clone();
    let body = MultipartBodyV1::parse(bytes, boundary(SENTINEL_BOUNDARY)).expect("valid");
    assert_eq!(body.as_bytes(), expected.as_slice());
    assert_eq!(body.len(), expected.len());
    // The transport sends this allocation rather than a copy of it.
    let shared = body.shared_owner();
    assert_eq!(shared.as_ptr(), body.as_bytes().as_ptr());

    let rendered = format!("{body:?}");
    assert!(!rendered.contains(SENTINEL_BOUNDARY), "Debug must not leak the boundary: {rendered}");
    assert!(!rendered.contains("model"), "Debug must not leak body content: {rendered}");
}

#[test]
fn a_multipart_request_renders_its_own_media_type_and_refuses_a_second_source() {
    let path = RelativePathV1::parse("v1/audio/transcriptions").unwrap();
    let slot = CredentialSlotV1::parse("openai.primary").unwrap();

    let request = MultipartPostRequestV1::try_new(
        path.clone(),
        SafeHeaders::try_from_iter([("x-request-id", "req-1")]).unwrap(),
        MultipartBodyV1::parse(multipart_body_for("edge"), boundary("edge")).unwrap(),
        BearerAuthV1::new(slot.clone()),
    )
    .expect("ordinary headers without a content-type are accepted");
    assert_eq!(request.relative_path().as_str(), "v1/audio/transcriptions");
    assert_eq!(request.body().content_type(), "multipart/form-data; boundary=edge");
    assert_eq!(request.headers().get("content-type"), None);
    assert!(request.query().is_none() && request.user_agent().is_none());

    // The second source is refused under any casing: the shape renders its own media type, and
    // an opaque body gives no way to notice that a host-supplied one disagrees with the bytes.
    for name in ["content-type", "Content-Type", "CONTENT-TYPE"] {
        assert_eq!(
            MultipartPostRequestV1::try_new(
                path.clone(),
                SafeHeaders::try_from_iter([(name, "text/plain")]).unwrap(),
                MultipartBodyV1::parse(multipart_body_for("edge"), boundary("edge")).unwrap(),
                BearerAuthV1::new(slot.clone()),
            )
            .err(),
            Some(ContractErrorV1::ContentTypeHeaderNotPermitted),
            "{name} must be refused"
        );
    }
}

#[test]
fn a_multipart_request_carries_every_credential_arm_query_and_user_agent() {
    let build = |auth: ProviderAuthV1| {
        MultipartPostRequestV1::try_new(
            RelativePathV1::parse("v1/images/edits").unwrap(),
            SafeHeaders::default(),
            MultipartBodyV1::parse(multipart_body_for("edge"), boundary("edge")).unwrap(),
            auth,
        )
        .expect("valid")
    };
    let slot = CredentialSlotV1::parse("primary").unwrap();
    assert!(matches!(
        build(ProviderAuthV1::Bearer(BearerAuthV1::new(slot.clone()))).auth(),
        ProviderAuthV1::Bearer(_)
    ));
    assert!(matches!(
        build(ProviderAuthV1::HeaderSecret {
            header: SecretHeaderV1::ApiKey,
            slot: BearerAuthV1::new(slot.clone()),
        })
        .auth(),
        ProviderAuthV1::HeaderSecret { .. }
    ));
    assert!(matches!(
        build(ProviderAuthV1::BearerAndHeaderSecret {
            header: SecretHeaderV1::XGoogApiKey,
            slot: BearerAuthV1::new(slot.clone()),
        })
        .auth(),
        ProviderAuthV1::BearerAndHeaderSecret { .. }
    ));
    assert!(matches!(
        build(ProviderAuthV1::HostSigned {
            slot: BearerAuthV1::new(slot.clone()),
            emits: SignedHeaderSetV1::new(&[SignedHeaderV1::Authorization]).unwrap(),
        })
        .auth(),
        ProviderAuthV1::HostSigned { .. }
    ));

    let query =
        QueryStringV1::try_from_iter([(QueryParameterV1::ApiVersion, "2025-04-01-preview")])
            .unwrap();
    let agent = ControlledUserAgentV1::try_from_static("south-test/1.0").unwrap();
    let request = build(ProviderAuthV1::Bearer(BearerAuthV1::new(slot)))
        .with_query(query.clone())
        .with_user_agent(agent);
    assert_eq!(request.query(), Some(&query));
    assert_eq!(
        request.user_agent().map(|agent| agent.as_str().to_owned()),
        Some("south-test/1.0".to_owned())
    );
}

#[test]
fn multipart_request_debug_shows_shape_only() {
    let request = MultipartPostRequestV1::try_new(
        RelativePathV1::parse(&format!("v1/{SENTINEL}")).unwrap(),
        SafeHeaders::try_from_iter([("x-test", SENTINEL)]).unwrap(),
        MultipartBodyV1::parse(multipart_body_for(SENTINEL_BOUNDARY), boundary(SENTINEL_BOUNDARY))
            .unwrap(),
        BearerAuthV1::new(CredentialSlotV1::parse(SENTINEL).unwrap()),
    )
    .unwrap();
    let rendered = format!("{request:?}");
    assert!(rendered.starts_with("MultipartPostRequestV1 {"));
    assert!(!rendered.contains(SENTINEL));
    assert!(!rendered.contains(SENTINEL_BOUNDARY));
}
