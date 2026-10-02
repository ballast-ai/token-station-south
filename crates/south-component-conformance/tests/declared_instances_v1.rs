//! B7a (`docs/design/2026-09-30-host-zero-vendor-boundary.md` §10): gate ① in
//! `south-provider-api` and the contract constructors in `south-contracts` apply one set of rules
//! to declared query parameters, quota headers and user-agents. `south-provider-api` depends on no
//! other south crate, so it repeats the lists and the user-agent grammar; these tests are what
//! license the repetition, and they cover the bridge that turns an admitted manifest into contract
//! types.

use std::{collections::BTreeMap, path::Path};

use proptest::prelude::*;
use south_component_conformance::{
    DeclaredInstancesErrorV1, DeclaredInstancesV1, contract_query_syntax,
};
use south_contracts::{
    DeclaredQueryParameterV1, DeclaredUserAgentV1, ProviderQuotaHeaderMapV1,
    ProviderQuotaMetadataFieldV1, QueryParameterV1,
};
use south_provider_api::{
    ComponentManifestV1, ManifestErrorV1, QueryParameterDeclarationV1, QueryValueSyntaxV1,
    QuotaHeaderDeclarationV1,
};

fn provider() -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/provider-openai-compatible/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_repeated_lists_are_the_contract_lists() {
    let sanctioned: Vec<String> =
        QueryParameterV1::ALL.iter().map(|parameter| parameter.wire_name().to_owned()).collect();
    assert_eq!(south_provider_api::SANCTIONED_QUERY_NAMES, sanctioned.as_slice());
    assert_eq!(
        south_provider_api::DECLARED_QUERY_DENIED_NAMES,
        south_contracts::DECLARED_QUERY_DENIED_NAMES
    );
    assert_eq!(
        south_provider_api::DECLARED_QUERY_DENIED_FRAGMENTS,
        south_contracts::DECLARED_QUERY_DENIED_FRAGMENTS
    );
    assert_eq!(
        south_provider_api::MAX_DECLARED_QUERY_NAME_BYTES,
        south_contracts::MAX_DECLARED_QUERY_NAME_BYTES
    );
    assert_eq!(south_provider_api::MAX_QUERY_ENUM_VALUES, south_contracts::MAX_QUERY_ENUM_VALUES);
    let fields: Vec<&str> =
        ProviderQuotaMetadataFieldV1::ALL.iter().map(|field| field.as_header_name()).collect();
    assert_eq!(south_provider_api::QUOTA_METADATA_FIELDS, fields.as_slice());
    assert_eq!(
        south_provider_api::QUOTA_HEADER_DENIED_NAMES,
        south_contracts::PROVIDER_QUOTA_HEADER_DENIED_NAMES
    );
    assert_eq!(
        south_provider_api::MAX_QUOTA_HEADER_NAME_BYTES,
        south_contracts::MAX_QUOTA_HEADER_NAME_BYTES
    );
    assert_eq!(south_provider_api::MAX_USER_AGENT_BYTES, south_contracts::MAX_USER_AGENT_BYTES);
}

fn gate_one_admits_query(name: &str, syntax: &QueryValueSyntaxV1) -> bool {
    let mut manifest = provider();
    manifest.query_parameters =
        vec![QueryParameterDeclarationV1 { name: name.to_owned(), syntax: syntax.clone() }];
    manifest.validate().is_ok()
}

fn gate_one_admits_quota(header: &str) -> bool {
    let mut manifest = provider();
    manifest.quota_headers = vec![QuotaHeaderDeclarationV1 {
        header: header.to_owned(),
        field: "x-ratelimit-remaining-tokens".to_owned(),
    }];
    manifest.validate().is_ok()
}

fn contract_admits_quota(header: &str) -> bool {
    ProviderQuotaHeaderMapV1::try_from_iter([(
        header,
        ProviderQuotaMetadataFieldV1::XRateLimitRemainingTokens,
    )])
    .is_ok()
}

#[test]
fn every_single_byte_is_judged_alike_by_both_halves() {
    for byte in 0_u8..=255 {
        let Ok(text) = std::str::from_utf8(std::slice::from_ref(&byte)) else { continue };
        for candidate in [format!("a{text}b"), format!("{text}ab"), format!("ab{text}")] {
            assert_eq!(
                south_provider_api::is_user_agent_value(&candidate),
                DeclaredUserAgentV1::from_manifest_value(&candidate).is_ok(),
                "user-agent {candidate:?}"
            );
            assert_eq!(
                gate_one_admits_query(&candidate, &QueryValueSyntaxV1::Token),
                DeclaredQueryParameterV1::try_new(
                    &candidate,
                    south_contracts::QueryValueSyntaxV1::Token
                )
                .is_ok(),
                "query name {candidate:?}"
            );
            assert_eq!(
                gate_one_admits_quota(&candidate),
                contract_admits_quota(&candidate),
                "quota header {candidate:?}"
            );
            let syntax = QueryValueSyntaxV1::Token;
            assert_eq!(
                syntax.admits(&candidate),
                contract_query_syntax(&syntax).admits(&candidate),
                "token value {candidate:?}"
            );
        }
    }
}

#[test]
fn every_listed_name_is_refused_by_both_halves() {
    for name in south_contracts::DECLARED_QUERY_DENIED_NAMES
        .iter()
        .chain(south_contracts::DECLARED_QUERY_DENIED_FRAGMENTS)
        .chain(south_provider_api::SANCTIONED_QUERY_NAMES)
    {
        assert!(!gate_one_admits_query(name, &QueryValueSyntaxV1::Digits), "{name}");
        assert!(
            DeclaredQueryParameterV1::try_new(name, south_contracts::QueryValueSyntaxV1::Digits)
                .is_err(),
            "{name}"
        );
    }
    for header in south_contracts::PROVIDER_QUOTA_HEADER_DENIED_NAMES {
        assert!(!gate_one_admits_quota(header) && !contract_admits_quota(header), "{header}");
    }
}

proptest! {
    #[test]
    fn query_names_are_judged_alike(name in "[A-Za-z0-9._~ &=%-]{0,70}") {
        prop_assert_eq!(
            gate_one_admits_query(&name, &QueryValueSyntaxV1::Digits),
            DeclaredQueryParameterV1::try_new(&name, south_contracts::QueryValueSyntaxV1::Digits).is_ok()
        );
    }

    #[test]
    fn enum_syntaxes_are_judged_alike(values in proptest::collection::vec("[a-z.&% ]{0,4}", 0..18)) {
        let syntax = QueryValueSyntaxV1::Enum(values);
        prop_assert_eq!(
            gate_one_admits_query("mode", &syntax),
            DeclaredQueryParameterV1::try_new("mode", contract_query_syntax(&syntax)).is_ok()
        );
    }

    #[test]
    fn query_values_are_judged_alike(value in "\\PC{0,70}") {
        for syntax in [
            QueryValueSyntaxV1::Digits,
            QueryValueSyntaxV1::Token,
            QueryValueSyntaxV1::Date,
            QueryValueSyntaxV1::Enum(vec!["sse".to_owned(), "a.b".to_owned()]),
        ] {
            prop_assert_eq!(syntax.admits(&value), contract_query_syntax(&syntax).admits(&value));
        }
    }

    #[test]
    fn quota_header_names_are_judged_alike(header in "[a-z0-9_ A-Z-]{0,130}") {
        prop_assert_eq!(gate_one_admits_quota(&header), contract_admits_quota(&header));
    }

    #[test]
    fn user_agents_are_judged_alike(value in "\\PC{0,260}") {
        prop_assert_eq!(
            south_provider_api::is_user_agent_value(&value),
            DeclaredUserAgentV1::from_manifest_value(&value).is_ok()
        );
    }
}

#[test]
fn an_admitted_manifest_converts_to_contract_types() {
    let mut manifest = provider();
    manifest.query_parameters = vec![QueryParameterDeclarationV1 {
        name: "region-hint".to_owned(),
        syntax: QueryValueSyntaxV1::Enum(vec!["eu".to_owned(), "us".to_owned()]),
    }];
    manifest.quota_headers = vec![QuotaHeaderDeclarationV1 {
        header: "x-acme-left".to_owned(),
        field: "x-ratelimit-remaining-tokens".to_owned(),
    }];
    manifest.user_agent =
        BTreeMap::from([("openai-compatible".to_owned(), "acme-cli/1.0 (external)".to_owned())]);

    let instances = DeclaredInstancesV1::from_manifest(&manifest).unwrap();
    assert_eq!(instances.query_parameters().len(), 1);
    assert_eq!(instances.query_parameter("api-version"), Some(QueryParameterV1::ApiVersion));
    let declared = instances.query_parameter("region-hint").unwrap();
    assert_eq!(declared.wire_name(), "region-hint");
    assert_eq!(instances.query_parameter("region_hint"), None);
    assert_eq!(
        instances.quota_headers().iter().collect::<Vec<_>>(),
        [("x-acme-left", ProviderQuotaMetadataFieldV1::XRateLimitRemainingTokens)]
    );
    assert_eq!(
        instances.user_agent("openai-compatible").map(DeclaredUserAgentV1::as_str),
        Some("acme-cli/1.0 (external)")
    );
    assert_eq!(instances.user_agent("azure-openai-v1"), None);
}

#[test]
fn a_manifest_without_declarations_keeps_todays_behavior() {
    let instances = DeclaredInstancesV1::from_manifest(&provider()).unwrap();
    assert!(instances.query_parameters().is_empty());
    assert_eq!(instances.quota_headers(), &ProviderQuotaHeaderMapV1::canonical());
    assert_eq!(instances.user_agent("openai-compatible"), None);
}

#[test]
fn a_refused_manifest_yields_no_declared_value() {
    let mut manifest = provider();
    manifest.user_agent =
        BTreeMap::from([("openai-compatible".to_owned(), "acme/1.0\r\nx-injected: 1".to_owned())]);
    assert!(matches!(
        DeclaredInstancesV1::from_manifest(&manifest),
        Err(DeclaredInstancesErrorV1::Manifest(ManifestErrorV1::InvalidUserAgent { .. }))
    ));
    // Any gate ① refusal, not only the instance rules, withholds every declared value.
    let mut manifest = provider();
    manifest.user_agent = BTreeMap::from([("openai-compatible".to_owned(), "acme/1.0".to_owned())]);
    manifest.permissions.network = true;
    assert!(matches!(
        DeclaredInstancesV1::from_manifest(&manifest),
        Err(DeclaredInstancesErrorV1::Manifest(ManifestErrorV1::NetworkPermissionDenied))
    ));
}
