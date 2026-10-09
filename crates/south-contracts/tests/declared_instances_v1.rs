//! The contract half of the B7a instance declarations
//! (`docs/design/2026-09-30-host-zero-vendor-boundary.md` §10): declared query parameters under
//! HTTP contract version ten, the declared user-agent, and quota metadata contract version two.

use http::HeaderValue;
use proptest::prelude::*;
use south_contracts::{
    BearerAuthV1, ContractErrorV1, ControlledUserAgentV1, CredentialSlotV1,
    DeclaredQueryParameterV1, DeclaredUserAgentV1, HTTP_CONTRACT_VERSION, JsonBodyV1,
    JsonPostRequestV1, MAX_QUERY_TOTAL_BYTES, MAX_USER_AGENT_BYTES,
    PROVIDER_QUOTA_METADATA_CONTRACT_VERSION, PROVIDER_QUOTA_METADATA_FIELD_COUNT,
    ProviderEndpointV1, ProviderQuotaHeaderMapV1, ProviderQuotaMetadataFieldV1, QueryParameterV1,
    QueryStringV1, QueryValueSyntaxV1, RelativePathV1, SafeHeaders, UserAgentV1,
};

fn declared(name: &str, syntax: QueryValueSyntaxV1) -> QueryParameterV1 {
    QueryParameterV1::Declared(DeclaredQueryParameterV1::try_new(name, syntax).unwrap())
}

fn enumeration(values: &[&str]) -> QueryValueSyntaxV1 {
    QueryValueSyntaxV1::Enum(values.iter().map(|value| (*value).to_owned()).collect())
}

#[test]
fn the_contract_versions_moved() {
    assert_eq!(HTTP_CONTRACT_VERSION, 12);
    assert_eq!(PROVIDER_QUOTA_METADATA_CONTRACT_VERSION, 2);
}

// ───────────────── declared query parameters ─────────────────

#[test]
fn declared_names_are_refused_like_gate_one_refuses_them() {
    for name in [
        "",
        "1page",
        "-page",
        "pa ge",
        "pa&ge",
        "pa=ge",
        "pa%20",
        "pa#ge",
        "pa+ge",
        "pa/ge",
        "pá",
        "key",
        "api_key",
        "apiKey",
        "x-api-key",
        "access_token",
        "token",
        "sig",
        "signature",
        "X-Amz-Signature",
        "client_secret",
        "password",
        "auth",
        "code",
        "api-version",
        "api_version",
        "alt",
        "GroupId",
        "group_id",
        "task_id",
        "file_id",
    ] {
        assert_eq!(
            DeclaredQueryParameterV1::try_new(name, QueryValueSyntaxV1::Token),
            Err(ContractErrorV1::InvalidQueryDeclaration),
            "{name:?}"
        );
    }
    assert!(
        DeclaredQueryParameterV1::try_new(&"a".repeat(65), QueryValueSyntaxV1::Digits).is_err()
    );
    let accepted =
        DeclaredQueryParameterV1::try_new("a.b_c-d~E", QueryValueSyntaxV1::Date).unwrap();
    assert_eq!(accepted.name(), "a.b_c-d~E");
    assert_eq!(accepted.syntax(), &QueryValueSyntaxV1::Date);
}

#[test]
fn malformed_enum_syntaxes_are_refused() {
    let too_many: Vec<String> = (0..17).map(|index| format!("v{index}")).collect();
    for syntax in [
        enumeration(&[]),
        enumeration(&["a", "a"]),
        enumeration(&["a b"]),
        enumeration(&["a&b"]),
        enumeration(&[".."]),
        enumeration(&[&"a".repeat(65)]),
        QueryValueSyntaxV1::Enum(too_many),
    ] {
        assert_eq!(
            DeclaredQueryParameterV1::try_new("mode", syntax.clone()),
            Err(ContractErrorV1::InvalidQueryDeclaration),
            "{syntax:?}"
        );
    }
}

#[test]
fn a_declared_value_must_have_the_declared_syntax() {
    for (syntax, accepted, refused) in [
        (QueryValueSyntaxV1::Digits, "0042", "42a"),
        (QueryValueSyntaxV1::Token, "a.b_c-d~e", "a&key=x"),
        (QueryValueSyntaxV1::Date, "2025-04-01-preview", "2025-4-1"),
        (enumeration(&["sse", "json"]), "json", "xml"),
    ] {
        let parameter = declared("mode", syntax);
        assert_eq!(
            QueryStringV1::try_from_iter([(parameter.clone(), accepted)]).unwrap().as_str(),
            format!("mode={accepted}")
        );
        assert_eq!(
            QueryStringV1::try_from_iter([(parameter, refused)]),
            Err(ContractErrorV1::InvalidQueryValue)
        );
    }
}

#[test]
fn declared_parameters_follow_the_sanctioned_ones_in_name_order() {
    let zeta = declared("zeta", QueryValueSyntaxV1::Digits);
    let alpha = declared("alpha", QueryValueSyntaxV1::Token);
    let query = QueryStringV1::try_from_iter([
        (zeta.clone(), "9"),
        (QueryParameterV1::Alt, "sse"),
        (alpha.clone(), "a"),
        (QueryParameterV1::ApiVersion, "v1"),
    ])
    .unwrap();
    assert_eq!(query.as_str(), "api-version=v1&alt=sse&alpha=a&zeta=9");
    let reversed = QueryStringV1::try_from_iter([
        (QueryParameterV1::ApiVersion, "v1"),
        (alpha, "a"),
        (QueryParameterV1::Alt, "sse"),
        (zeta, "9"),
    ])
    .unwrap();
    assert_eq!(reversed, query);
}

#[test]
fn a_declared_query_that_carries_no_declared_parameter_serializes_as_in_version_nine() {
    let query = QueryStringV1::try_from_iter([
        (QueryParameterV1::FileId, "7"),
        (QueryParameterV1::GroupId, "19000"),
    ])
    .unwrap();
    assert_eq!(query.as_str(), "GroupId=19000&file_id=7");
}

#[test]
fn a_declared_name_repeated_is_a_duplicate() {
    let first = declared("page", QueryValueSyntaxV1::Digits);
    let second = declared("page", QueryValueSyntaxV1::Token);
    assert_eq!(
        QueryStringV1::try_from_iter([(first, "1"), (second, "a")]),
        Err(ContractErrorV1::DuplicateQueryParameter)
    );
}

#[test]
fn the_total_query_bound_still_holds() {
    let parameters: Vec<(QueryParameterV1, String)> = (0..5)
        .map(|index| (declared(&format!("p{index}"), QueryValueSyntaxV1::Token), "a".repeat(60)))
        .collect();
    let refused = QueryStringV1::try_from_iter(
        parameters.iter().map(|(name, value)| (name.clone(), value.as_str())),
    );
    assert_eq!(refused, Err(ContractErrorV1::QueryTooLarge));
    const { assert!(MAX_QUERY_TOTAL_BYTES < 5 * 64) };
}

#[test]
fn a_declared_query_lands_on_the_url_intact() {
    let endpoint = ProviderEndpointV1::parse("https://api.example.com/").unwrap();
    let path = RelativePathV1::parse("v1/resource").unwrap();
    let query = QueryStringV1::try_from_iter([
        (declared("region-hint", enumeration(&["eu", "us"])), "eu"),
        (QueryParameterV1::ApiVersion, "2024-10-21"),
    ])
    .unwrap();
    let url = path.resolve_against_with_query(&endpoint, Some(&query)).unwrap();
    assert_eq!(url.query(), Some("api-version=2024-10-21&region-hint=eu"));
}

// ───────────────── declared user-agent ─────────────────

#[test]
fn the_declared_grammar_is_the_controlled_grammar() {
    // Every single byte, both length edges, both space edges: the two constructors agree.
    for byte in 0_u8..=255 {
        let Ok(text) = std::str::from_utf8(std::slice::from_ref(&byte)) else { continue };
        let value = format!("a{text}b");
        let leaked: &'static str = Box::leak(value.clone().into_boxed_str());
        assert_eq!(
            DeclaredUserAgentV1::from_manifest_value(&value).is_ok(),
            ControlledUserAgentV1::try_from_static(leaked).is_ok(),
            "byte {byte:#04x}"
        );
    }
    for value in ["", " a", "a ", "a\r\nb", "a\nb", "a\tb", "é"] {
        assert_eq!(
            DeclaredUserAgentV1::from_manifest_value(value),
            Err(ContractErrorV1::InvalidUserAgentValue),
            "{value:?}"
        );
    }
    assert!(DeclaredUserAgentV1::from_manifest_value(&"a".repeat(MAX_USER_AGENT_BYTES)).is_ok());
    assert!(
        DeclaredUserAgentV1::from_manifest_value(&"a".repeat(MAX_USER_AGENT_BYTES + 1)).is_err()
    );
}

#[test]
fn a_declared_user_agent_fills_the_single_slot() {
    let request = JsonPostRequestV1::new(
        RelativePathV1::parse("v1/chat").unwrap(),
        SafeHeaders::default(),
        JsonBodyV1::parse("{}").unwrap(),
        BearerAuthV1::new(CredentialSlotV1::parse("slot").unwrap()),
    );
    let controlled = ControlledUserAgentV1::try_from_static("host/1.0").unwrap();
    let declared = DeclaredUserAgentV1::from_manifest_value("acme-cli/2.0 (external)").unwrap();

    // Whichever source fills it last, the slot holds exactly one value.
    let request = request.with_user_agent(controlled).with_user_agent(declared.clone());
    assert_eq!(request.user_agent(), Some(&UserAgentV1::Declared(declared)));
    assert_eq!(request.user_agent().map(UserAgentV1::as_str), Some("acme-cli/2.0 (external)"));
    let request = request.with_user_agent(controlled);
    assert_eq!(request.user_agent().map(UserAgentV1::as_str), Some("host/1.0"));

    // The ordinary channel still refuses the name.
    assert!(SafeHeaders::try_from_iter([("user-agent", "acme/1.0")]).is_err());
}

#[test]
fn a_declared_user_agent_prints_only_its_shape() {
    let value = "secretive-client/9.9";
    let agent = DeclaredUserAgentV1::from_manifest_value(value).unwrap();
    let direct = format!("{agent:?}");
    for debug in [direct, format!("{:?}", UserAgentV1::from(agent))] {
        assert!(!debug.contains(value), "{debug}");
    }
}

proptest! {
    #[test]
    fn an_accepted_declared_user_agent_is_a_valid_header_value(value in "\\PC{0,300}") {
        if let Ok(agent) = DeclaredUserAgentV1::from_manifest_value(&value) {
            prop_assert_eq!(agent.as_str(), value.as_str());
            prop_assert!(HeaderValue::from_str(agent.as_str()).is_ok());
            prop_assert!(!agent.as_str().contains(['\r', '\n']));
        }
    }

    #[test]
    fn an_accepted_declared_query_value_never_changes_the_query_shape(
        name in "[A-Za-z][A-Za-z0-9._~-]{0,70}",
        value in "\\PC{0,80}",
    ) {
        let Ok(parameter) = DeclaredQueryParameterV1::try_new(&name, QueryValueSyntaxV1::Token)
        else {
            return Ok(());
        };
        if let Ok(query) = QueryStringV1::try_from_iter([(QueryParameterV1::Declared(parameter), value.as_str())]) {
            prop_assert_eq!(query.as_str(), format!("{name}={value}"));
            prop_assert_eq!(query.as_str().matches('=').count(), 1);
            prop_assert!(!query.as_str().contains(['&', '#', '%', '+', ' ']));
        }
    }
}

// ───────────────── declared quota headers ─────────────────

#[test]
fn the_canonical_map_reads_each_field_from_its_own_header() {
    let canonical = ProviderQuotaHeaderMapV1::canonical();
    assert_eq!(canonical, ProviderQuotaHeaderMapV1::default());
    assert_eq!(canonical.iter().len(), PROVIDER_QUOTA_METADATA_FIELD_COUNT);
    for field in ProviderQuotaMetadataFieldV1::ALL {
        assert_eq!(canonical.header_for(field), Some(field.as_header_name()));
        assert_eq!(
            ProviderQuotaMetadataFieldV1::from_header_name(field.as_header_name()),
            Some(field)
        );
    }
    assert_eq!(
        ProviderQuotaMetadataFieldV1::from_header_name("x-ratelimit-remaining-requests"),
        None
    );
}

#[test]
fn a_declared_map_replaces_the_canonical_one() {
    let map = ProviderQuotaHeaderMapV1::try_from_iter([
        ("x-acme-left", ProviderQuotaMetadataFieldV1::XRateLimitRemainingTokens),
        ("x-acme-cap", ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens),
    ])
    .unwrap();
    assert_eq!(
        map.iter().collect::<Vec<_>>(),
        [
            ("x-acme-left", ProviderQuotaMetadataFieldV1::XRateLimitRemainingTokens),
            ("x-acme-cap", ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens),
        ]
    );
    assert_eq!(map.header_for(ProviderQuotaMetadataFieldV1::XRateLimitResetTokens), None);
}

#[test]
fn refused_quota_declarations() {
    use ProviderQuotaMetadataFieldV1::{
        XRateLimitLimitTokens as Limit, XRateLimitRemainingTokens as Left,
    };
    let refused: [&[(&str, ProviderQuotaMetadataFieldV1)]; 9] = [
        &[],
        &[("set-cookie", Left)],
        &[("authorization", Left)],
        &[("retry-after", Left)],
        &[("X-Acme-Left", Left)],
        &[("acme_left", Left)],
        &[("acme-left-", Left)],
        &[("x-acme-left", Left), ("x-acme-left", Limit)],
        &[("x-acme-left", Left), ("x-acme-other", Left)],
    ];
    for entries in refused {
        assert_eq!(
            ProviderQuotaHeaderMapV1::try_from_iter(entries.iter().copied()),
            Err(ContractErrorV1::InvalidQuotaHeaderDeclaration),
            "{entries:?}"
        );
    }
}
