//! Endpoint templates and config keys (B2, `docs/design/2026-09-30-host-zero-vendor-boundary.md`
//! §7.3): both hosts fill and check a family's endpoint the same way, and gate ① refuses a
//! template that would let a parameter choose where a request goes.

// Endpoint templates are written with `{name}` parameters, which this lint mistakes for format
// arguments.
#![allow(clippy::literal_string_with_formatting_args)]

use std::collections::BTreeMap;
use std::path::Path;

use south_provider_api::{
    ComponentManifestV1, ConfigErrorV1, ConfigKeyV1, EndpointValuesErrorV1, ManifestErrorV1,
    ValueSyntaxV1,
};

fn converse() -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/provider-bedrock-converse/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn openai() -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/provider-openai-compatible/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
}

fn key(syntax: ValueSyntaxV1, required: bool) -> ConfigKeyV1 {
    ConfigKeyV1 { syntax, required, description: "test".to_owned(), default: None }
}

/// The Converse manifest with its `bedrock` family's endpoint and keys replaced.
fn declaring(template: &str, keys: &[(&str, ConfigKeyV1)]) -> ComponentManifestV1 {
    let mut manifest = converse();
    // Signing names the shipped template's `{region}`; these templates are about the endpoint only.
    manifest.signing = None;
    manifest.endpoint = BTreeMap::from([("bedrock".to_owned(), template.to_owned())]);
    manifest.config_schema = BTreeMap::from([(
        "bedrock".to_owned(),
        keys.iter().map(|(name, key)| ((*name).to_owned(), key.clone())).collect(),
    )]);
    if keys.is_empty() {
        manifest.config_schema.clear();
    }
    manifest
}

#[test]
fn the_shipped_converse_endpoint_fills_from_a_region() {
    let manifest = converse();
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(
        manifest.fill_endpoint("bedrock", &values(&[("region", "eu-west-3")])),
        Ok(Some("https://bedrock-runtime.eu-west-3.amazonaws.com".to_owned()))
    );
    assert_eq!(
        manifest.fill_endpoint("bedrock", &values(&[])),
        Err(ConfigErrorV1::MissingKey("region".to_owned()))
    );
    // A value that would carry the request to another domain never reaches the template.
    assert_eq!(
        manifest.fill_endpoint("bedrock", &values(&[("region", "x.attacker.example")])),
        Err(ConfigErrorV1::InvalidValue("region".to_owned()))
    );
    assert_eq!(
        manifest.validate_config_values(
            "bedrock",
            &values(&[("region", "us-east-1"), ("project", "p")])
        ),
        Err(ConfigErrorV1::UnknownKey("project".to_owned()))
    );
    assert_eq!(manifest.fill_endpoint("not-declared", &values(&[])), Ok(None));
}

#[test]
fn an_operator_url_is_checked_against_the_template() {
    let manifest = converse();
    assert!(manifest.endpoint_admits("bedrock", "https://bedrock-runtime.us-east-1.amazonaws.com"));
    assert!(
        manifest.endpoint_admits("bedrock", "https://bedrock-runtime.us-east-1.amazonaws.com/")
    );
    for url in [
        "https://bedrock-runtime.amazonaws.com",
        "https://bedrock-runtime.us-east-1.attacker.example",
        "https://bedrock-runtime.evil.com.amazonaws.com",
        "https://bedrock-runtime.US-EAST-1.amazonaws.com",
        "http://bedrock-runtime.us-east-1.amazonaws.com",
        "https://bedrock-runtime.us-east-1.amazonaws.com/extra",
    ] {
        assert!(!manifest.endpoint_admits("bedrock", url), "{url}");
    }
    // A family without a template keeps today's behavior: the operator's URL is the anchor.
    assert!(manifest.endpoint_admits("other", "https://anything.example"));
}

#[test]
fn path_parameters_are_encoded_as_one_segment() {
    let manifest = declaring(
        "https://aiplatform.googleapis.com/v1/projects/{project}/locations/{location}",
        &[
            ("project", key(ValueSyntaxV1::GcpProjectId, true)),
            (
                "location",
                key(ValueSyntaxV1::Enum(vec!["global".to_owned(), "a/b".to_owned()]), true),
            ),
        ],
    );
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(
        manifest
            .fill_endpoint("bedrock", &values(&[("project", "my-project-1"), ("location", "a/b")])),
        Ok(Some(
            "https://aiplatform.googleapis.com/v1/projects/my-project-1/locations/a%2Fb".to_owned()
        ))
    );
}

#[test]
fn gate_one_refuses_a_template_a_parameter_could_steer() {
    let region = || key(ValueSyntaxV1::AwsRegion, true);
    for (template, keys, why) in [
        ("https://{region}", vec![("region", region())], "a parameter chooses the whole host"),
        (
            "https://{region}.com",
            vec![("region", region())],
            "a one-label suffix is not a fixed domain",
        ),
        (
            "https://{name}.example.com",
            vec![("name", key(ValueSyntaxV1::PrintableAscii(64), true))],
            "a host parameter whose syntax admits dots",
        ),
        ("https://x.example.com:8443", Vec::new(), "a port"),
        ("http://x.example.com", Vec::new(), "plain http"),
        ("https://x.example.com/v1?key={region}", vec![("region", region())], "a query"),
        ("https://user@x.example.com", Vec::new(), "userinfo"),
        (
            "https://x.example.com/{region}",
            vec![("region", key(ValueSyntaxV1::AwsRegion, false))],
            "an optional parameter",
        ),
        ("https://x.example.com/{missing}", Vec::new(), "an undeclared parameter"),
        ("https://x.example.com/{Region}", Vec::new(), "a malformed parameter name"),
        (
            "https://x.example.com/{region}",
            vec![("region", key(ValueSyntaxV1::Enum(Vec::new()), true))],
            "an empty enum",
        ),
    ] {
        assert!(
            matches!(
                declaring(template, &keys).validate(),
                Err(ManifestErrorV1::InvalidEndpoint { .. })
            ),
            "{why}: {template}"
        );
    }

    // Since Q14 (§13.8) a key the endpoint does not use, and `config_schema` without an endpoint,
    // are admitted: the component reads such a key from `ProviderConfig.declared`
    // (`component_values_v1.rs`).
    assert_eq!(
        declaring("https://x.example.com/{region}", &[("region", region()), ("extra", region())])
            .validate(),
        Ok(())
    );
}

#[test]
fn value_syntaxes_admit_only_their_shape() {
    let cases: [(ValueSyntaxV1, &[&str], &[&str]); 7] = [
        (
            ValueSyntaxV1::AwsRegion,
            &["us-east-1", "ap-southeast-3"],
            &["", "US-EAST-1", "-us", "us.east", "a/b"],
        ),
        (
            ValueSyntaxV1::AwsArn,
            &["arn:aws:bedrock:us-east-1:123456789012:inference-profile/us.anthropic.claude-v1:0"],
            &["arn:aws:bedrock", "aws:bedrock:us-east-1:1:x:y", "arn:aws:b:r:a:x y"],
        ),
        (
            ValueSyntaxV1::GcpProjectId,
            &["my-project-1"],
            &["short", "1project", "Proj-ect", "my-project-"],
        ),
        (
            ValueSyntaxV1::ApiVersionDate,
            &["2024-10-21", "2025-01-01-preview"],
            &["2024-1-21", "v1", "2024-10-21-beta"],
        ),
        (ValueSyntaxV1::Digits, &["0", "123456789012"], &["", "12a", "-1"]),
        (ValueSyntaxV1::Token, &["abc_DEF-1.2"], &["", "a b", "a/b", "a\u{e9}"]),
        (ValueSyntaxV1::PrintableAscii(5), &["a b", "12345"], &["", "123456", "a\nb", "\u{e9}"]),
    ];
    for (syntax, good, bad) in cases {
        for value in good {
            assert!(syntax.admits(value), "{syntax:?} should admit {value:?}");
        }
        for value in bad {
            assert!(!syntax.admits(value), "{syntax:?} should refuse {value:?}");
        }
    }
    let one_of = ValueSyntaxV1::Enum(vec!["global".to_owned()]);
    assert!(one_of.admits("global") && !one_of.admits("Global"));
}

#[test]
fn endpoint_declarations_round_trip_and_belong_to_the_provider_world() {
    let manifest = converse();
    let wire = serde_json::to_value(&manifest).unwrap();
    assert_eq!(wire["endpoint"]["bedrock"], "https://bedrock-runtime.{region}.amazonaws.com");
    assert_eq!(wire["config_schema"]["bedrock"]["region"]["syntax"], "aws_region");
    assert_eq!(wire["stream_framing"], "aws-eventstream");
    let read: ComponentManifestV1 = serde_json::from_value(wire).unwrap();
    assert_eq!(read, manifest);

    let mut task = manifest;
    task.api_version = south_provider_api::TASK_WORLD.to_owned();
    task.conformance.required_suite = south_provider_api::TASK_BEHAVIOR_SUITE.to_owned();
    task.compatibility.wit_package = south_provider_api::TASK_WIT_PACKAGE.to_owned();
    task.capabilities = ["submit", "observe", "render"].into_iter().map(str::to_owned).collect();
    task.request_facts.clear();
    assert!(matches!(task.validate(), Err(ManifestErrorV1::InvalidSigning(_))));
    task.signing = None;
    assert_eq!(task.validate(), Err(ManifestErrorV1::StreamFramingIsAProviderWorldDeclaration));
    task.stream_framing = south_provider_api::StreamFramingV1::Bytes;
    assert_eq!(task.validate(), Err(ManifestErrorV1::EndpointIsAProviderWorldDeclaration));
}

#[test]
fn signing_names_its_scheme_service_region_and_inputs() {
    let manifest = converse();
    let signing = manifest.signing.expect("the shipped Converse manifest declares signing");
    assert_eq!(signing.scheme, south_provider_api::SigningSchemeV1::AwsSigv4);
    assert_eq!(signing.service, "bedrock");
    assert_eq!(signing.region.template_param, "region");

    let refused = |edit: &dyn Fn(&mut ComponentManifestV1)| {
        let mut manifest = converse();
        edit(&mut manifest);
        matches!(manifest.validate(), Err(ManifestErrorV1::InvalidSigning(_)))
    };
    assert!(
        refused(&|m| {
            m.auth_arms = ["bearer".to_owned()].into();
            m.emits.clear();
        }),
        "signing without host_signed"
    );
    assert!(
        refused(&|m| m.emits.retain(|header| header != "x-amz-date")),
        "a missing SigV4 header"
    );
    assert!(
        refused(&|m| m.signing.as_mut().unwrap().service = "Bedrock".to_owned()),
        "service syntax"
    );
    assert!(
        refused(&|m| {
            m.signing.as_mut().unwrap().credentials.remove("secret_access_key");
        }),
        "a missing secret key input"
    );
    assert!(
        refused(&|m| {
            m.signing.as_mut().unwrap().credentials.insert("password".to_owned(), "p".to_owned());
        }),
        "an input the scheme does not take"
    );
    assert!(
        refused(&|m| m.signing.as_mut().unwrap().region.template_param = "zone".to_owned()),
        "a region parameter the endpoint does not have"
    );
}

/// Host feedback SF13 (§5.4, §13.6): the fields `signing.credentials` names are declared by the
/// package as secret credential fields, so a host collects exactly those and never infers a field
/// set from the scheme. Gate ① promised this check in §5.4 and deferred it to B4 (§13.1).
#[test]
fn signing_inputs_name_declared_secret_fields() {
    let manifest = converse();
    let credentials =
        manifest.credentials_for("bedrock").expect("the Converse family declares its fields");
    let signing = manifest.signing.as_ref().expect("the Converse package signs");
    for (input, field) in &signing.credentials {
        let declared = credentials
            .fields
            .get(field)
            .unwrap_or_else(|| panic!("signing input `{input}` names an undeclared field"));
        assert!(declared.secret, "`{field}` is a signing credential, so it is secret");
        assert_eq!(
            declared.required,
            input != "session_token",
            "`{field}`: the two keys are required, the session token is optional"
        );
    }

    let refused = |edit: &dyn Fn(&mut ComponentManifestV1)| {
        let mut manifest = converse();
        edit(&mut manifest);
        matches!(manifest.validate(), Err(ManifestErrorV1::InvalidSigning(_)))
    };
    assert!(refused(&|m| m.credentials = None), "signing without declared fields");
    assert!(
        refused(&|m| {
            m.signing
                .as_mut()
                .unwrap()
                .credentials
                .insert("access_key_id".to_owned(), "undeclared".to_owned());
        }),
        "an input naming an undeclared field"
    );
    assert!(
        refused(&|m| {
            m.credentials.as_mut().unwrap().fields.get_mut("secret_access_key").unwrap().secret =
                false;
        }),
        "an input naming a non-secret field"
    );
    assert!(
        refused(&|m| {
            m.credentials.as_mut().unwrap().fields.get_mut("access_key_id").unwrap().required =
                false;
        }),
        "a required input naming an optional field"
    );
    assert!(
        refused(&|m| {
            m.providers.push("bedrock-eu".to_owned());
            m.endpoint.insert(
                "bedrock-eu".to_owned(),
                "https://bedrock-runtime.{region}.amazonaws.com".to_owned(),
            );
            let keys = m.config_schema["bedrock"].clone();
            m.config_schema.insert("bedrock-eu".to_owned(), keys);
            m.credentials.as_mut().unwrap().families = Some(vec!["bedrock".to_owned()]);
        }),
        "a signed family the credentials section does not cover"
    );
    // An optional session token may map to a required field: that is the operator's stricter
    // choice, not a contradiction.
    let mut strict = converse();
    strict.credentials.as_mut().unwrap().fields.get_mut("session_token").unwrap().required = true;
    assert_eq!(strict.validate(), Ok(()));
}

/// Host feedback SF17 (§13.6): the reverse of `fill_endpoint`. A host that has only an
/// operator-entered `base_url` recovers the template parameters it was filled from, so a signing
/// package's region need not be entered twice.
#[test]
fn endpoint_values_recover_the_parameters_of_a_base_url() {
    let manifest = converse();
    assert_eq!(
        manifest.endpoint_values("bedrock", "https://bedrock-runtime.eu-west-3.amazonaws.com/"),
        Ok(values(&[("region", "eu-west-3")]))
    );
    for base_url in [
        "https://bedrock-runtime.eu-west-3.amazonaws.com.evil.example",
        "https://bedrock-runtime..amazonaws.com",
        "https://bedrock-runtime.EU-WEST-3.amazonaws.com",
        "https://bedrock-runtime.eu.west.amazonaws.com",
        "http://bedrock-runtime.eu-west-3.amazonaws.com",
    ] {
        assert_eq!(
            manifest.endpoint_values("bedrock", base_url),
            Err(EndpointValuesErrorV1::NotThisEndpoint),
            "{base_url}"
        );
    }
    assert_eq!(
        manifest.endpoint_values("unknown", "https://example.com"),
        Err(EndpointValuesErrorV1::NoEndpoint)
    );

    // Copilot's plan is a required enum; the value is recovered as written.
    let copilot = openai();
    assert_eq!(
        copilot.endpoint_values("github-copilot", "https://api.business.githubcopilot.com"),
        Ok(values(&[("plan", "business")]))
    );
    // Every recovered value fills the template back to the same URL.
    for (family, base_url) in [
        ("bedrock", "https://bedrock-runtime.ap-southeast-2.amazonaws.com"),
        ("github-copilot", "https://api.enterprise.githubcopilot.com"),
    ] {
        let manifest = if family == "bedrock" { converse() } else { openai() };
        let recovered = manifest.endpoint_values(family, base_url).unwrap();
        assert_eq!(manifest.fill_endpoint(family, &recovered), Ok(Some(base_url.to_owned())));
    }
}

/// Two adjacent parameters can split one text in more than one way; a reverse that picked one
/// would invent a value, so it refuses instead.
#[test]
fn endpoint_values_refuse_an_ambiguous_split() {
    let manifest = declaring(
        "https://{first}-{second}.example.com",
        &[
            ("first", key(ValueSyntaxV1::AwsRegion, true)),
            ("second", key(ValueSyntaxV1::AwsRegion, true)),
        ],
    );
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(
        manifest.endpoint_values("bedrock", "https://a-b-c.example.com"),
        Err(EndpointValuesErrorV1::Ambiguous)
    );
    assert_eq!(
        manifest.endpoint_values("bedrock", "https://a-b.example.com"),
        Ok(values(&[("first", "a"), ("second", "b")]))
    );
}

/// Host feedback SF7 (§13.5 D7): Copilot's chat API host depends on the account's plan. The
/// family's `plan` key admits only GitHub's three documented hosts, so no value can leave the
/// domain. It is required and has no default (token-station-server feedback, 2026-10-07): with a
/// default of `individual`, a Business or Enterprise operator who left it empty was routed to the
/// Pro / Pro+ host and found out only when the upstream refused the request.
#[test]
fn the_shipped_copilot_endpoint_follows_the_plan_and_requires_it() {
    let manifest = openai();
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(
        manifest.fill_endpoint("github-copilot", &values(&[])),
        Err(ConfigErrorV1::MissingKey("plan".to_owned()))
    );
    for (plan, host) in [
        ("individual", "https://api.individual.githubcopilot.com"),
        ("business", "https://api.business.githubcopilot.com"),
        ("enterprise", "https://api.enterprise.githubcopilot.com"),
    ] {
        assert_eq!(
            manifest.fill_endpoint("github-copilot", &values(&[("plan", plan)])),
            Ok(Some(host.to_owned()))
        );
        assert!(manifest.endpoint_admits("github-copilot", host), "{host}");
    }
    for plan in ["Business", "free", "attacker.example", ""] {
        assert_eq!(
            manifest.fill_endpoint("github-copilot", &values(&[("plan", plan)])),
            Err(ConfigErrorV1::InvalidValue("plan".to_owned())),
            "{plan}"
        );
    }
    for url in ["https://api.githubcopilot.com", "https://api.attacker.githubcopilot.com"] {
        assert!(!manifest.endpoint_admits("github-copilot", url), "{url}");
    }
    // The package's other families declare no endpoint: the operator's URL stays their anchor.
    assert_eq!(manifest.fill_endpoint("openai-compatible", &values(&[])), Ok(None));
}

/// A config key's `default` (§13.5 D7) fills an absent value: it must have the key's syntax, a
/// required key has none, and an endpoint parameter is required or defaulted.
#[test]
fn a_config_default_fills_an_absent_value_and_gate_one_checks_it() {
    let label = || ValueSyntaxV1::Enum(vec!["one".to_owned(), "two".to_owned()]);
    let defaulted =
        |default: &str| ConfigKeyV1 { default: Some(default.to_owned()), ..key(label(), false) };
    let manifest = declaring("https://api.{tier}.example.com", &[("tier", defaulted("two"))]);
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(manifest.validate_config_values("bedrock", &values(&[])), Ok(()));
    assert_eq!(
        manifest.fill_endpoint("bedrock", &values(&[])),
        Ok(Some("https://api.two.example.com".to_owned()))
    );
    assert_eq!(
        manifest.fill_endpoint("bedrock", &values(&[("tier", "one")])),
        Ok(Some("https://api.one.example.com".to_owned()))
    );

    for (keys, why) in [
        (vec![("tier", defaulted("three"))], "a default without the key's syntax"),
        (
            vec![("tier", ConfigKeyV1 { default: Some("one".to_owned()), ..key(label(), true) })],
            "a default on a required key",
        ),
        (vec![("tier", key(label(), false))], "an optional parameter without a default"),
    ] {
        let manifest = declaring("https://api.{tier}.example.com", &keys);
        assert!(
            matches!(manifest.validate(), Err(ManifestErrorV1::InvalidEndpoint { .. })),
            "{why}"
        );
    }
}
