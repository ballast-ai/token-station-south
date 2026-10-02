//! Endpoint templates and config keys (B2, `docs/design/2026-09-30-host-zero-vendor-boundary.md`
//! §7.3): both hosts fill and check a family's endpoint the same way, and gate ① refuses a
//! template that would let a parameter choose where a request goes.

// Endpoint templates are written with `{name}` parameters, which this lint mistakes for format
// arguments.
#![allow(clippy::literal_string_with_formatting_args)]

use std::collections::BTreeMap;
use std::path::Path;

use south_provider_api::{
    ComponentManifestV1, ConfigErrorV1, ConfigKeyV1, ManifestErrorV1, ValueSyntaxV1,
};

fn converse() -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/provider-bedrock-converse/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
}

fn key(syntax: ValueSyntaxV1, required: bool) -> ConfigKeyV1 {
    ConfigKeyV1 { syntax, required, description: "test".to_owned() }
}

/// The Converse manifest with its `bedrock` family's endpoint and keys replaced.
fn declaring(template: &str, keys: &[(&str, ConfigKeyV1)]) -> ComponentManifestV1 {
    let mut manifest = converse();
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
        (
            "https://x.example.com/{region}",
            vec![("region", region()), ("unused", region())],
            "a key the endpoint does not use (§16 Q14)",
        ),
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

    let mut orphan = converse();
    orphan.endpoint.clear();
    assert!(
        matches!(orphan.validate(), Err(ManifestErrorV1::InvalidEndpoint { .. })),
        "config_schema without an endpoint"
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
    assert_eq!(task.validate(), Err(ManifestErrorV1::StreamFramingIsAProviderWorldDeclaration));
    task.stream_framing = south_provider_api::StreamFramingV1::Bytes;
    assert_eq!(task.validate(), Err(ManifestErrorV1::EndpointIsAProviderWorldDeclaration));
}
