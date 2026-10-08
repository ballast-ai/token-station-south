//! The component value channel (Q14, `docs/design/2026-09-30-host-zero-vendor-boundary.md`
//! §13.8): which keys a package declares into `ProviderConfig.declared` and
//! `ChatRequest.host_values`, how a host builds the per-attempt map, and what gate ① refuses.

// Endpoint templates are written with `{name}` parameters, which this lint mistakes for format
// arguments.
#![allow(clippy::literal_string_with_formatting_args)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};
use south_provider_api::{
    ComponentManifestV1, ConfigErrorV1, DeclaredValuesErrorV1, HOST_VALUE_ATTEMPT_ID, HOST_VALUES,
    ManifestErrorV1,
};

fn shipped(package: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../components/{package}/manifest.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn parse(manifest: Value) -> ComponentManifestV1 {
    serde_json::from_value(manifest).unwrap()
}

fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
}

const ARN: &str = "arn:aws:codewhisperer:us-east-1:123456789012:profile/ABCDEF";

/// The OpenAI-compatible package with a Kiro-like section that exports a field attribute (with
/// `persist`) and, from its selector, the recipe it chose.
fn exporting() -> Value {
    let refresh = |id: &str, endpoint: &str| {
        json!({
            "steps": [
                { "id": id, "kind": "http_exchange", "method": "POST", "encoding": "json",
                  "endpoint": endpoint,
                  "requires": ["refresh_token"],
                  "params": { "refreshToken": { "field": "refresh_token" } },
                  "extract": {
                      "access_token": { "pointer": "/accessToken", "secret": true },
                      "expires_at": { "relative_seconds": "/expiresIn" } } }
            ],
            "present": format!("{id}.access_token"),
            "rotates_refresh_material": false,
            "attributes": { "profile_arn": { "field": "profile_arn", "export": true, "persist": true } }
        })
    };
    let mut manifest = shipped("provider-openai-compatible");
    manifest["credentials"] = json!({
        "schema": "south.credential-recipe.v1",
        "fields": {
            "refresh_token": { "secret": true, "required": true },
            "auth_method": { "secret": false, "syntax": "token" },
            "profile_arn": { "secret": false, "syntax": "aws_arn" }
        },
        "slots": { "provider_api_key": { "minted": "pick" } },
        "recipes": {
            "pick": {
                "select": [
                    { "when": { "field_in": { "field": "auth_method", "values": ["idc"] } },
                      "recipe": "idc" },
                    { "recipe": "social" } ],
                "attributes": { "auth_flow": { "selected_recipe": true, "export": true } } },
            "social": refresh("social", "https://social.auth.example.com/refresh"),
            "idc": refresh("idc", "https://idc.auth.example.com/token")
        }
    });
    manifest
}

#[test]
fn the_host_value_vocabulary_is_closed_and_starts_with_the_attempt_id() {
    assert_eq!(HOST_VALUES, [HOST_VALUE_ATTEMPT_ID]);
    assert_eq!(HOST_VALUE_ATTEMPT_ID, "attempt_id");

    let mut manifest = shipped("provider-openai-compatible");
    manifest["host_values"] = json!(["attempt_id"]);
    let manifest = parse(manifest);
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(manifest.host_values, ["attempt_id"]);

    let mut unknown = shipped("provider-openai-compatible");
    unknown["host_values"] = json!(["request_id"]);
    assert_eq!(
        parse(unknown).validate(),
        Err(ManifestErrorV1::HostValueIsNotInTheVocabulary("request_id".to_owned()))
    );

    let mut twice = shipped("provider-openai-compatible");
    twice["host_values"] = json!(["attempt_id", "attempt_id"]);
    assert_eq!(
        parse(twice).validate(),
        Err(ManifestErrorV1::HostValueDeclaredTwice("attempt_id".to_owned()))
    );

    // `host_values` lives on `ChatRequest`, which only the provider world receives.
    let mut task = shipped("task-kling-v2");
    task["host_values"] = json!(["attempt_id"]);
    assert_eq!(
        parse(task).validate(),
        Err(ManifestErrorV1::InstanceIsAProviderWorldDeclaration("host_values".to_owned()))
    );

    // Absent means none, and it is not serialized, so every shipped manifest is unchanged.
    let plain = parse(shipped("provider-anthropic"));
    assert!(plain.host_values.is_empty());
    assert!(serde_json::to_value(&plain).unwrap().get("host_values").is_none());
}

/// Until Q14 a config key could only feed the endpoint; now the component may read it (§13.8).
#[test]
fn a_config_key_may_feed_the_component_alone() {
    let mut extra = shipped("provider-bedrock-converse-bearer");
    extra["config_schema"]["bedrock-bearer"]["inference_profile"] =
        json!({ "syntax": "aws_arn", "description": "An inference profile ARN." });
    assert_eq!(parse(extra).validate(), Ok(()));

    let mut no_endpoint = shipped("provider-bedrock-converse-bearer");
    no_endpoint.as_object_mut().unwrap().remove("endpoint");
    assert_eq!(parse(no_endpoint).validate(), Ok(()));

    // The endpoint's own rules are unchanged: a template parameter is still a required key or
    // one with a default.
    let mut optional_param = shipped("provider-bedrock-converse-bearer");
    optional_param["config_schema"]["bedrock-bearer"]["region"]["required"] = json!(false);
    assert!(matches!(
        parse(optional_param).validate(),
        Err(ManifestErrorV1::InvalidEndpoint { .. })
    ));
}

#[test]
fn declared_keys_are_the_family_config_keys_and_exported_attributes() {
    let manifest = parse(exporting());
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(
        manifest.declared_keys("github-copilot"),
        BTreeSet::from(["auth_flow", "plan", "profile_arn"])
    );
    assert_eq!(
        manifest.declared_keys("openai-compatible"),
        BTreeSet::from(["auth_flow", "profile_arn"])
    );
    assert!(manifest.declared_keys("not-a-family").is_empty());
    assert!(parse(shipped("provider-anthropic")).declared_keys("anthropic").is_empty());

    // A section scoped to one family exports nothing into another.
    let mut scoped = exporting();
    scoped["credentials"]["families"] = json!(["openai-compatible"]);
    let scoped = parse(scoped);
    assert_eq!(scoped.validate(), Ok(()));
    assert_eq!(scoped.declared_keys("github-copilot"), BTreeSet::from(["plan"]));
}

#[test]
fn declared_values_are_built_per_attempt_from_declared_keys_only() {
    let manifest = parse(exporting());
    let built = manifest
        .declared_values(
            "github-copilot",
            &values(&[("plan", "business")]),
            &values(&[("profile_arn", ARN), ("auth_flow", "idc")]),
        )
        .unwrap();
    assert_eq!(built, values(&[("auth_flow", "idc"), ("plan", "business"), ("profile_arn", ARN)]));

    // An absent optional key with a default takes it; with no default it is absent.
    let bearer = parse(shipped("provider-bedrock-converse-bearer"));
    assert_eq!(
        bearer.declared_values("bedrock-bearer", &values(&[("region", "eu-west-3")]), &values(&[])),
        Ok(values(&[("region", "eu-west-3")]))
    );

    // Config values go through the same checks as the endpoint's.
    assert_eq!(
        manifest.declared_values("github-copilot", &values(&[]), &values(&[])),
        Err(DeclaredValuesErrorV1::Config(ConfigErrorV1::MissingKey("plan".to_owned())))
    );
    assert_eq!(
        manifest.declared_values(
            "github-copilot",
            &values(&[("plan", "business"), ("stranger", "x")]),
            &values(&[])
        ),
        Err(DeclaredValuesErrorV1::Config(ConfigErrorV1::UnknownKey("stranger".to_owned())))
    );
    // An attribute the section does not export, or a value its declaration does not admit.
    assert_eq!(
        manifest.declared_values("openai-compatible", &values(&[]), &values(&[("plan", "x")])),
        Err(DeclaredValuesErrorV1::UndeclaredAttribute("plan".to_owned()))
    );
    assert_eq!(
        manifest.declared_values(
            "openai-compatible",
            &values(&[]),
            &values(&[("profile_arn", "not-an-arn")])
        ),
        Err(DeclaredValuesErrorV1::InvalidAttribute("profile_arn".to_owned()))
    );
    assert_eq!(
        manifest.declared_values(
            "openai-compatible",
            &values(&[]),
            &values(&[("auth_flow", "pick")])
        ),
        Err(DeclaredValuesErrorV1::InvalidAttribute("auth_flow".to_owned()))
    );
}

/// Config keys and attributes share one flat namespace in `declared` (D8).
#[test]
fn a_config_key_and_an_attribute_may_not_share_a_name() {
    let mut colliding = exporting();
    colliding["credentials"]["recipes"]["social"]["attributes"]["plan"] =
        json!({ "field": "auth_method", "export": true });
    assert_eq!(
        parse(colliding).validate(),
        Err(ManifestErrorV1::DeclaredValueNameCollision {
            family: "github-copilot".to_owned(),
            name: "plan".to_owned(),
        })
    );

    // Scoped away from the family that has the key, the same name is no collision.
    let mut scoped = exporting();
    scoped["credentials"]["families"] = json!(["openai-compatible"]);
    scoped["credentials"]["recipes"]["social"]["attributes"]["plan"] =
        json!({ "field": "auth_method", "export": true });
    assert_eq!(parse(scoped).validate(), Ok(()));
}

/// Only the provider world receives `declared` from this vocabulary; no task host path builds it.
#[test]
fn credential_attributes_are_a_provider_world_declaration() {
    let mut task = shipped("task-kling-v2");
    task["credentials"]["recipes"]["kling_jwt"]["attributes"] =
        json!({ "access_key": { "field": "access_key", "export": true } });
    assert_eq!(
        parse(task).validate(),
        Err(ManifestErrorV1::InstanceIsAProviderWorldDeclaration(
            "credential attributes".to_owned()
        ))
    );
}
