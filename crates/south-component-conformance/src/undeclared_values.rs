//! Gate ②'s `UndeclaredValuesIgnored` row for the worlds whose only value source is
//! `ProviderConfig.declared` (Q14, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §13.8):
//! embeddings (`docs/design/2026-09-30-embeddings-contract.md` §16), image
//! (`docs/design/2026-09-30-image-world.md` §18.3) and speech
//! (`docs/design/2026-09-30-speech-world.md` §17.3, §18 S-S-2).
//!
//! The row is the other half of gate ①'s value-channel admission: a world whose manifest may
//! declare `config_schema` keys and exported credential attributes needs a check that its
//! component reads only those. Each of these worlds' functions takes the provider config at
//! `/provider_config` of a fixture input, so one implementation serves them all; the provider
//! world adds `ChatRequest.host_values`, which none of them has, and keeps its own row
//! (`suite.rs`).

use serde_json::Value;
use south_provider_api::ComponentManifestV1;

use crate::{CheckV1, OutcomeV1};

/// The prefix of the key injected to prove a component ignores a `declared` key its package does
/// not declare; a numeric suffix keeps it clear of any key the package declares.
const UNDECLARED_VALUE: &str = "conformance_undeclared_";

/// What one fixture input produced as JSON; `Err` is a fixture or boundary failure, never the
/// component's own refusal, which is a value (`{"error": ...}`).
pub type Invoked = Result<Value, String>;

/// Adds a key the manifest does not declare to `ProviderConfig.declared` of `input` and requires
/// the same answer as `first`, a refusal included.
///
/// The key is valid under the kernel's `ComponentValues` grammar, so only the declaration tells it
/// apart from one the component may read. The family whose declared keys are avoided is the one
/// the fixture names at `/provider_config/provider`.
pub fn undeclared_values_ignored(
    case: &str,
    input: &Value,
    first: &Invoked,
    manifest: &ComponentManifestV1,
    invoke: &dyn Fn(&Value) -> Invoked,
) -> OutcomeV1 {
    let check = CheckV1::UndeclaredValuesIgnored;
    let family = input.pointer("/provider_config/provider").and_then(Value::as_str);
    let declared = family.map(|family| manifest.declared_keys(family)).unwrap_or_default();
    // One more candidate than there are declared keys, so one is always free.
    let key = (0..=declared.len())
        .map(|index| format!("{UNDECLARED_VALUE}{index}"))
        .find(|key| !declared.contains(key.as_str()))
        .unwrap_or_default();
    let mut mutated = input.clone();
    let Some(Value::Object(config)) = mutated.pointer_mut("/provider_config") else {
        return OutcomeV1::failed(
            check,
            case,
            "fixture has no object at `/provider_config` to carry `declared`",
        );
    };
    let entry = config.entry("declared").or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Value::Object(values) = entry else {
        return OutcomeV1::failed(check, case, "`/provider_config/declared` is not an object");
    };
    values.insert(key.clone(), Value::String("undeclared".to_owned()));
    if invoke(&mutated) == *first {
        OutcomeV1::passed(check, case)
    } else {
        OutcomeV1::failed(
            check,
            case,
            format!(
                "adding the undeclared key `declared.{key}` changed the answer; a component reads \
                 only the keys its package declares"
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::VerdictV1;

    /// A speech-world manifest whose `vertex-tts` family declares the config key `region`, as a
    /// Vertex TTS package would (speech record §17.3).
    fn speech_manifest(extra_key: Option<&str>) -> ComponentManifestV1 {
        let mut keys = json!({
            "region": { "syntax": "aws_region", "required": true, "description": "A location." }
        });
        if let Some(extra) = extra_key {
            keys[extra] = json!({ "syntax": "token", "description": "Another key." });
        }
        let manifest: ComponentManifestV1 = serde_json::from_value(json!({
            "name": "speech-vertex",
            "version": "1.0.0",
            "api_version": "speech-adapter-v1",
            "providers": ["vertex-tts"],
            "capabilities": ["synthesize"],
            "auth_arms": ["bearer"],
            "permissions": { "network": false, "filesystem": false, "secrets": ["provider_api_key"] },
            "conformance": { "required_suite": "south.speech-component.v1", "fixtures": "fixtures/" },
            "compatibility": {
                "ir_schema_id": "token-station-protocol@0.6.0/v0.5.0",
                "kernel_version": "0.5.0",
                "kernel_revision": "c0bca10c2d9f9a439680865feb65c9edf598fc65",
                "wit_package": "token-station:speech-adapter@1.0.0",
                "south_runtime": "0.63.0"
            },
            "config_schema": { "vertex-tts": keys }
        }))
        .expect("the manifest parses");
        manifest.validate().expect("the speech world admits the value channel");
        manifest
    }

    /// A speech `prepare` fixture input: provider config, request view and call context.
    fn prepare_input() -> Value {
        json!({
            "provider_config": {
                "provider": "vertex-tts",
                "base_url": "https://us-central1-aiplatform.googleapis.com",
                "declared": { "region": "us-central1" }
            },
            "request": { "json": { "model": "gemini-2.5-flash-tts", "input": "hello" } },
            "context": { "upstream_model": "gemini-2.5-flash-tts", "operation": "synthesize" }
        })
    }

    /// A component that builds a body from the request and the `declared` keys it reads.
    fn component(reads_every_key: bool) -> impl Fn(&Value) -> Invoked {
        move |input: &Value| {
            let declared = input["provider_config"]["declared"].as_object().cloned();
            let mut body = json!({ "input": input["request"]["json"]["input"] });
            for (key, value) in declared.unwrap_or_default() {
                if reads_every_key || key == "region" {
                    body[key] = value;
                }
            }
            Ok(json!({ "descriptor": { "body": { "json": { "template": body } } } }))
        }
    }

    fn run(manifest: &ComponentManifestV1, reads_every_key: bool) -> OutcomeV1 {
        let invoke = component(reads_every_key);
        let input = prepare_input();
        let first = invoke(&input);
        undeclared_values_ignored("speech-v1.vertex-tts.prepare", &input, &first, manifest, &invoke)
    }

    #[test]
    fn a_component_reading_only_declared_keys_passes() {
        let outcome = run(&speech_manifest(None), false);
        assert_eq!(outcome.check, CheckV1::UndeclaredValuesIgnored);
        assert_eq!(outcome.verdict, VerdictV1::Passed);
    }

    #[test]
    fn a_component_acting_on_an_undeclared_key_is_red() {
        let outcome = run(&speech_manifest(None), true);
        let VerdictV1::Failed(detail) = outcome.verdict else {
            panic!("an undeclared key that changes the answer must fail the row");
        };
        assert!(detail.contains("`declared.conformance_undeclared_0`"), "{detail}");
    }

    /// The injected key avoids every key the family declares, so a component that reads a declared
    /// key of the reserved shape is not mistaken for one that reads an undeclared key.
    #[test]
    fn the_injected_key_is_never_a_declared_one() {
        let manifest = speech_manifest(Some("conformance_undeclared_0"));
        let VerdictV1::Failed(detail) = run(&manifest, true).verdict else {
            panic!("the reader still acts on the injected key");
        };
        assert!(detail.contains("`declared.conformance_undeclared_1`"), "{detail}");
    }

    /// A refusal must stay the same refusal, and a fixture without `/provider_config` is a fixture
    /// failure, never a pass.
    #[test]
    fn refusals_are_compared_and_a_shapeless_fixture_fails() {
        let manifest = speech_manifest(None);
        let refuse =
            |_: &Value| -> Invoked { Ok(json!({ "error": { "code": "invalid_request" } })) };
        let input = prepare_input();
        let outcome =
            undeclared_values_ignored("case", &input, &refuse(&input), &manifest, &refuse);
        assert_eq!(outcome.verdict, VerdictV1::Passed);

        let selective = |input: &Value| -> Invoked {
            let declared = &input["provider_config"]["declared"];
            if declared.as_object().is_some_and(|map| map.len() > 1) {
                Ok(json!({ "error": { "code": "invalid_request" } }))
            } else {
                Ok(json!({ "descriptor": {} }))
            }
        };
        let outcome =
            undeclared_values_ignored("case", &input, &selective(&input), &manifest, &selective);
        assert!(outcome.is_failure(), "refusing because of an undeclared key is acting on it");

        let shapeless = json!({ "request": {}, "context": {} });
        let outcome =
            undeclared_values_ignored("case", &shapeless, &Ok(json!({})), &manifest, &refuse);
        assert!(outcome.is_failure());
        let mut not_an_object = prepare_input();
        not_an_object["provider_config"]["declared"] = json!(["region"]);
        let outcome =
            undeclared_values_ignored("case", &not_an_object, &Ok(json!({})), &manifest, &refuse);
        assert!(outcome.is_failure());
    }
}
