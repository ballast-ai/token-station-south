//! Gate ① for the provider instances a package declares (B7a,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §10): query parameters, quota headers and
//! per-family user-agents. Every refusal has a row here.

use std::{collections::BTreeMap, path::Path};

use south_provider_api::{
    ComponentManifestV1, ManifestErrorV1, QueryParameterDeclarationV1, QueryValueSyntaxV1,
    QuotaHeaderDeclarationV1, is_user_agent_value,
};

fn manifest(component: &str) -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components")
        .join(component)
        .join("manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn provider() -> ComponentManifestV1 {
    manifest("provider-openai-compatible")
}

fn query(name: &str, syntax: QueryValueSyntaxV1) -> QueryParameterDeclarationV1 {
    QueryParameterDeclarationV1 { name: name.to_owned(), syntax }
}

fn quota(header: &str, field: &str) -> QuotaHeaderDeclarationV1 {
    QuotaHeaderDeclarationV1 { header: header.to_owned(), field: field.to_owned() }
}

fn with_query(parameters: Vec<QueryParameterDeclarationV1>) -> Result<(), ManifestErrorV1> {
    let mut manifest = provider();
    manifest.query_parameters = parameters;
    manifest.validate()
}

fn with_quota(headers: Vec<QuotaHeaderDeclarationV1>) -> Result<(), ManifestErrorV1> {
    let mut manifest = provider();
    manifest.quota_headers = headers;
    manifest.validate()
}

fn with_user_agent(family: &str, value: &str) -> Result<(), ManifestErrorV1> {
    let mut manifest = provider();
    manifest.user_agent = BTreeMap::from([(family.to_owned(), value.to_owned())]);
    manifest.validate()
}

fn enumeration(values: &[&str]) -> QueryValueSyntaxV1 {
    QueryValueSyntaxV1::Enum(values.iter().map(|value| (*value).to_owned()).collect())
}

#[test]
fn every_declaration_parses_from_its_manifest_shape_and_round_trips() {
    let json = serde_json::json!({
        "query_parameters": [
            {"name": "api_version_label", "syntax": "date"},
            {"name": "region-hint", "syntax": {"enum": ["us", "eu"]}},
            {"name": "page", "syntax": "digits"},
            {"name": "mode", "syntax": "token"}
        ],
        "quota_headers": [
            {"header": "x-acme-tokens-remaining", "field": "x-ratelimit-remaining-tokens"}
        ],
        "user_agent": {"openai-compatible": "acme-cli/1.2.3 (external, cli)"}
    });
    let mut value = serde_json::to_value(provider()).unwrap();
    for (key, declared) in json.as_object().unwrap() {
        value[key] = declared.clone();
    }
    let parsed: ComponentManifestV1 = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(parsed.validate(), Ok(()));
    assert_eq!(parsed.query_parameters[1].syntax, enumeration(&["us", "eu"]));
    assert_eq!(parsed.user_agent_for("openai-compatible"), Some("acme-cli/1.2.3 (external, cli)"));
    assert_eq!(parsed.user_agent_for("azure-openai-v1"), None);
    assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
}

#[test]
fn an_absent_declaration_changes_nothing() {
    // R5: a manifest that declares none of the three serializes without them and validates.
    // `provider-anthropic` declares none; `provider-openai-compatible` declares only the
    // `github-copilot` user-agent (host-zero-vendor-boundary §13.5 D7).
    let openai = provider();
    assert!(openai.query_parameters.is_empty());
    assert!(openai.quota_headers.is_empty());
    assert_eq!(openai.user_agent.keys().collect::<Vec<_>>(), ["github-copilot"]);
    let manifest = manifest("provider-anthropic");
    assert!(manifest.query_parameters.is_empty());
    assert!(manifest.quota_headers.is_empty());
    assert!(manifest.user_agent.is_empty());
    let value = serde_json::to_value(&manifest).unwrap();
    for key in ["query_parameters", "quota_headers", "user_agent"] {
        assert!(value.get(key).is_none(), "{key} must not serialize when absent");
    }
    assert_eq!(manifest.validate(), Ok(()));
}

#[test]
fn an_unknown_query_syntax_is_refused_when_the_manifest_is_read() {
    for syntax in [
        serde_json::json!("printable_ascii"),
        serde_json::json!("aws_region"),
        serde_json::json!("api_version_date"),
        serde_json::json!({"printable_ascii": 64}),
        serde_json::json!("Digits"),
    ] {
        let mut value = serde_json::to_value(provider()).unwrap();
        value["query_parameters"] = serde_json::json!([{"name": "page", "syntax": syntax}]);
        assert!(
            serde_json::from_value::<ComponentManifestV1>(value).is_err(),
            "{syntax} is not a query value syntax"
        );
    }
    let mut value = serde_json::to_value(provider()).unwrap();
    value["query_parameters"] =
        serde_json::json!([{"name": "page", "syntax": "digits", "source": "credential"}]);
    assert!(
        serde_json::from_value::<ComponentManifestV1>(value).is_err(),
        "a declaration names a parameter and a syntax, never where a value comes from"
    );
}

#[test]
fn query_names_with_bad_syntax_are_refused() {
    for name in [
        "",
        "1page",
        "-page",
        "_page",
        "pa ge",
        "pa&ge",
        "pa=ge",
        "pa%20ge",
        "pa#ge",
        "pa+ge",
        "pa/ge",
        "página",
        &"a".repeat(65),
    ] {
        assert!(
            matches!(
                with_query(vec![query(name, QueryValueSyntaxV1::Digits)]),
                Err(ManifestErrorV1::InvalidQueryParameter { .. })
            ),
            "{name:?} must be refused"
        );
    }
    assert_eq!(with_query(vec![query(&"a".repeat(64), QueryValueSyntaxV1::Digits)]), Ok(()));
    assert_eq!(with_query(vec![query("a.b_c-d~e", QueryValueSyntaxV1::Token)]), Ok(()));
}

#[test]
fn credential_shaped_and_sanctioned_query_names_are_reserved() {
    for name in [
        "key",
        "KEY",
        "api_key",
        "api-key",
        "apiKey",
        "x-api-key",
        "access_token",
        "token",
        "sig",
        "signature",
        "X-Amz-Signature",
        "X-Amz-Credential",
        "x-amz-security-token",
        "client_secret",
        "password",
        "auth",
        "code",
        "refresh_token",
        // A sanctioned name keeps its own grammar, in any spelling.
        "api-version",
        "api_version",
        "alt",
        "GroupId",
        "group_id",
        "task_id",
        "file_id",
    ] {
        let refused = with_query(vec![query(name, QueryValueSyntaxV1::Token)]);
        assert!(
            matches!(refused, Err(ManifestErrorV1::InvalidQueryParameter { .. })),
            "{name:?} must be reserved, got {refused:?}"
        );
    }
}

#[test]
fn malformed_enum_syntaxes_are_refused() {
    let too_many: Vec<String> = (0..17).map(|index| format!("v{index}")).collect();
    for syntax in [
        enumeration(&[]),
        enumeration(&["a", "a"]),
        enumeration(&["a b"]),
        enumeration(&["a&b"]),
        enumeration(&["a%26"]),
        enumeration(&[".."]),
        enumeration(&[""]),
        enumeration(&[&"a".repeat(65)]),
        QueryValueSyntaxV1::Enum(too_many),
    ] {
        assert!(
            matches!(
                with_query(vec![query("mode", syntax.clone())]),
                Err(ManifestErrorV1::InvalidQueryParameter { .. })
            ),
            "{syntax:?} must be refused"
        );
    }
}

#[test]
fn a_repeated_query_name_is_refused_and_the_count_is_bounded() {
    assert!(matches!(
        with_query(vec![
            query("page", QueryValueSyntaxV1::Digits),
            query("page", QueryValueSyntaxV1::Token),
        ]),
        Err(ManifestErrorV1::InvalidQueryParameter { .. })
    ));
    let many =
        (0..17).map(|index| query(&format!("p{index}"), QueryValueSyntaxV1::Digits)).collect();
    assert!(matches!(with_query(many), Err(ManifestErrorV1::InvalidQueryParameter { .. })));
    let sixteen =
        (0..16).map(|index| query(&format!("p{index}"), QueryValueSyntaxV1::Digits)).collect();
    assert_eq!(with_query(sixteen), Ok(()));
}

#[test]
fn query_value_syntaxes_admit_only_their_values() {
    let digits = QueryValueSyntaxV1::Digits;
    assert!(digits.admits("0") && digits.admits(&"9".repeat(32)));
    assert!(!digits.admits("") && !digits.admits(&"9".repeat(33)) && !digits.admits("1a"));
    let token = QueryValueSyntaxV1::Token;
    assert!(token.admits("v1") && token.admits("a.b_c-d~e") && token.admits(&"a".repeat(64)));
    for refused in ["", "..", "a b", "a&b", "a#b", "a%20", "a+b", "a=b", &"a".repeat(65)] {
        assert!(!token.admits(refused), "{refused:?}");
    }
    let date = QueryValueSyntaxV1::Date;
    assert!(date.admits("2024-10-21") && date.admits("2025-04-01-preview"));
    assert!(!date.admits("2024-1-21") && !date.admits("v1"));
    let choice = enumeration(&["sse", "json"]);
    assert!(choice.admits("sse") && !choice.admits("SSE") && !choice.admits("xml"));
}

#[test]
fn quota_headers_with_bad_names_are_refused() {
    for header in [
        "",
        "X-Acme-Remaining",
        "1-remaining",
        "-remaining",
        "remaining-",
        "acme_remaining",
        "acme remaining",
        "acme:remaining",
        &"a".repeat(129),
    ] {
        assert!(
            matches!(
                with_quota(vec![quota(header, "x-ratelimit-remaining-tokens")]),
                Err(ManifestErrorV1::InvalidQuotaHeader { .. })
            ),
            "{header:?} must be refused"
        );
    }
}

#[test]
fn reserved_response_headers_cannot_feed_a_quota_field() {
    for header in [
        "set-cookie",
        "set-cookie2",
        "authorization",
        "www-authenticate",
        "proxy-authenticate",
        "cookie",
        "content-length",
        "content-type",
        "retry-after",
        "transfer-encoding",
        "connection",
        "location",
    ] {
        assert!(
            matches!(
                with_quota(vec![quota(header, "x-ratelimit-limit-tokens")]),
                Err(ManifestErrorV1::InvalidQuotaHeader { .. })
            ),
            "{header} must be reserved"
        );
    }
}

#[test]
fn a_quota_field_outside_the_closed_set_is_refused() {
    for field in
        ["x-ratelimit-remaining-requests", "tokens_remaining", "", "X-RATELIMIT-LIMIT-TOKENS"]
    {
        assert!(
            matches!(
                with_quota(vec![quota("x-acme-remaining", field)]),
                Err(ManifestErrorV1::InvalidQuotaHeader { .. })
            ),
            "{field:?} is not a closed quota field"
        );
    }
}

#[test]
fn a_repeated_quota_header_or_field_is_refused() {
    assert!(matches!(
        with_quota(vec![
            quota("x-acme-remaining", "x-ratelimit-remaining-tokens"),
            quota("x-acme-remaining", "x-ratelimit-limit-tokens"),
        ]),
        Err(ManifestErrorV1::InvalidQuotaHeader { .. })
    ));
    assert!(matches!(
        with_quota(vec![
            quota("x-acme-remaining", "x-ratelimit-remaining-tokens"),
            quota("x-acme-left", "x-ratelimit-remaining-tokens"),
        ]),
        Err(ManifestErrorV1::InvalidQuotaHeader { .. })
    ));
    // A canonical name may feed its own field, or another one.
    assert_eq!(
        with_quota(vec![
            quota("x-ratelimit-limit-tokens", "x-ratelimit-limit-tokens"),
            quota("ratelimit-remaining", "x-ratelimit-remaining-tokens"),
        ]),
        Ok(())
    );
}

#[test]
fn a_declared_secret_header_cannot_feed_a_quota_field() {
    // Both grammars are lowercase-only, so an exact match is the only collision `validate` can
    // meet; the check itself folds case regardless.
    let mut manifest = provider();
    manifest.secret_headers = vec!["x-acme-key".to_owned()];
    manifest.quota_headers = vec![quota("x-acme-key", "x-ratelimit-limit-tokens")];
    assert!(matches!(
        manifest.validate(),
        Err(ManifestErrorV1::InvalidQuotaHeader { header, .. }) if header == "x-acme-key"
    ));
    manifest.quota_headers = vec![quota("x-acme-remaining", "x-ratelimit-remaining-tokens")];
    assert_eq!(manifest.validate(), Ok(()));
}

#[test]
fn user_agents_outside_the_grammar_are_refused() {
    for value in [
        "",
        " leading",
        "trailing ",
        "line\r\nbreak",
        "carriage\rreturn",
        "line\nfeed",
        "tab\there",
        "nul\0byte",
        "del\u{7f}byte",
        "non-ascii/é",
        &"a".repeat(257),
    ] {
        assert!(
            matches!(
                with_user_agent("openai-compatible", value),
                Err(ManifestErrorV1::InvalidUserAgent { .. })
            ),
            "{value:?} must be refused"
        );
        assert!(!is_user_agent_value(value));
    }
    for value in [
        "opencode/1.15.6",
        "GitHubCopilotChat/0.43.0",
        "aws-sdk-js/1.0.0 KiroIDE",
        "claude-cli/2.1.114 (external, cli)",
        &"a".repeat(256),
    ] {
        assert_eq!(with_user_agent("openai-compatible", value), Ok(()), "{value:?}");
    }
}

#[test]
fn a_user_agent_for_an_undeclared_family_is_refused() {
    assert!(matches!(
        with_user_agent("anthropic", "acme/1.0"),
        Err(ManifestErrorV1::InvalidUserAgent { family, .. }) if family == "anthropic"
    ));
}

#[test]
fn instances_are_provider_world_declarations() {
    let mut task = manifest("task-minimax-v2");
    assert_eq!(task.validate(), Ok(()));
    task.query_parameters = vec![query("page", QueryValueSyntaxV1::Digits)];
    assert_eq!(
        task.validate(),
        Err(ManifestErrorV1::InstanceIsAProviderWorldDeclaration("query_parameters".to_owned()))
    );

    let mut task = manifest("task-minimax-v2");
    task.quota_headers = vec![quota("x-acme-remaining", "x-ratelimit-remaining-tokens")];
    assert_eq!(
        task.validate(),
        Err(ManifestErrorV1::InstanceIsAProviderWorldDeclaration("quota_headers".to_owned()))
    );

    let mut task = manifest("task-minimax-v2");
    task.user_agent = BTreeMap::from([("minimax".to_owned(), "acme/1.0".to_owned())]);
    assert_eq!(
        task.validate(),
        Err(ManifestErrorV1::InstanceIsAProviderWorldDeclaration("user_agent".to_owned()))
    );
}
