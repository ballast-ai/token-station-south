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

/// The provider and embeddings worlds receive `declared` from this vocabulary; no task host path
/// builds it.
#[test]
fn credential_attributes_are_refused_in_the_task_worlds() {
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

/// The embeddings package `embeddings-gemini` with a service-account section that mints its slot
/// and exports `project_id`, and a `region` config key: what a Vertex-shaped embeddings family
/// declares (embeddings record §16).
fn embeddings_with_values() -> Value {
    let mut manifest = shipped("embeddings-gemini");
    manifest["config_schema"] = json!({
        "gemini": {
            "region": { "syntax": "aws_region", "required": true, "description": "A location." }
        }
    });
    manifest["credentials"] = json!({
        "schema": "south.credential-recipe.v1",
        "fields": {
            "service_account": { "secret": true, "required": true, "media": "application/json" },
            "project_id": { "secret": false, "required": true, "syntax": "gcp_project_id" }
        },
        "slots": { "provider_api_key": { "minted": "sa" } },
        "recipes": {
            "sa": {
                "steps": [
                    { "id": "assertion", "kind": "jwt_sign", "alg": "RS256",
                      "key": { "field": "service_account", "pointer": "/private_key" },
                      "claims": {
                          "iss": { "field": "service_account", "pointer": "/client_email" },
                          "aud": { "endpoint_of": "token" },
                          "iat": { "now_plus": 0 }, "exp": { "now_plus": 600 } } },
                    { "id": "token", "kind": "oauth2_token", "encoding": "form",
                      "endpoint": "https://oauth2.example.test/token",
                      "params": {
                          "grant_type": { "const": "urn:ietf:params:oauth:grant-type:jwt-bearer" },
                          "assertion": { "output": "assertion.jwt" } },
                      "extract": {
                          "access_token": { "pointer": "/access_token", "secret": true },
                          "expires_at": { "relative_seconds": "/expires_in" } } }
                ],
                "present": "token.access_token",
                "rotates_refresh_material": false,
                "attributes": { "project_id": { "field": "project_id", "export": true } }
            }
        }
    });
    manifest
}

/// An embeddings host builds `declared` per attempt as a provider host does, so the world admits
/// config keys and exported attributes, and both reach `declared_keys` / `declared_values`.
#[test]
fn the_embeddings_world_admits_config_keys_and_exported_attributes() {
    let manifest = parse(embeddings_with_values());
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(manifest.declared_keys("gemini"), BTreeSet::from(["project_id", "region"]));
    assert_eq!(
        manifest.declared_values(
            "gemini",
            &values(&[("region", "us-central1")]),
            &values(&[("project_id", "fake-project")])
        ),
        Ok(values(&[("project_id", "fake-project"), ("region", "us-central1")]))
    );
    assert_eq!(
        manifest.declared_values("gemini", &values(&[]), &values(&[])),
        Err(DeclaredValuesErrorV1::Config(ConfigErrorV1::MissingKey("region".to_owned())))
    );

    // Either half alone is admitted too.
    let mut keys_only = embeddings_with_values();
    keys_only.as_object_mut().unwrap().remove("credentials");
    assert_eq!(parse(keys_only).validate(), Ok(()));
    let mut attributes_only = embeddings_with_values();
    attributes_only.as_object_mut().unwrap().remove("config_schema");
    assert_eq!(parse(attributes_only).validate(), Ok(()));

    // The shipped embeddings packages declare neither, so nothing they serialize changes.
    let plain = parse(shipped("embeddings-gemini"));
    assert!(plain.declared_keys("gemini").is_empty());
    let wire = serde_json::to_value(&plain).unwrap();
    assert!(wire.get("config_schema").is_none() && wire.get("credentials").is_none());
}

/// What the embeddings world admits is held to the provider world's rules, and nothing beyond the
/// two value sources is widened: `endpoint` and `host_values` stay provider-only.
#[test]
fn the_embeddings_world_holds_its_values_to_the_provider_rules() {
    let refused = |edit: &dyn Fn(&mut Value)| {
        let mut manifest = embeddings_with_values();
        edit(&mut manifest);
        parse(manifest).validate()
    };
    assert!(matches!(
        refused(&|m| m["config_schema"]["gemini"]["Region"] =
            json!({ "syntax": "token", "description": "x" })),
        Err(ManifestErrorV1::InvalidEndpoint { .. })
    ));
    assert!(matches!(
        refused(&|m| m["config_schema"]["vertex"] = json!({})),
        Err(ManifestErrorV1::InvalidEndpoint { .. })
    ));
    assert!(matches!(
        refused(&|m| m["config_schema"]["gemini"]["region"] = json!({
            "syntax": "aws_region", "description": "x", "default": "Not A Region"
        })),
        Err(ManifestErrorV1::InvalidEndpoint { .. })
    ));
    // D8: one namespace.
    assert_eq!(
        refused(&|m| m["config_schema"]["gemini"]["project_id"] =
            json!({ "syntax": "gcp_project_id", "description": "x" })),
        Err(ManifestErrorV1::DeclaredValueNameCollision {
            family: "gemini".to_owned(),
            name: "project_id".to_owned(),
        })
    );
    // An attribute still needs a non-secret field with a syntax.
    assert!(matches!(
        refused(&|m| m["credentials"]["fields"]["project_id"] =
            json!({ "secret": false, "required": true })),
        Err(ManifestErrorV1::InvalidCredentials(_))
    ));
    assert_eq!(
        refused(&|m| m["endpoint"] = json!({ "gemini": "https://{region}-api.example.test" })),
        Err(ManifestErrorV1::EndpointIsAProviderWorldDeclaration)
    );
    assert_eq!(
        refused(&|m| m["host_values"] = json!(["attempt_id"])),
        Err(ManifestErrorV1::InstanceIsAProviderWorldDeclaration("host_values".to_owned()))
    );
}

/// The task worlds still refuse `config_schema`: the widening is the embeddings world's alone.
#[test]
fn the_task_worlds_still_refuse_config_keys() {
    for package in ["task-kling-v2", "task-kling"] {
        let mut task = shipped(package);
        task["config_schema"] = json!({
            "kling": { "region": { "syntax": "aws_region", "description": "A region." } }
        });
        assert_eq!(
            parse(task).validate(),
            Err(ManifestErrorV1::EndpointIsAProviderWorldDeclaration),
            "{package}"
        );
    }
}
