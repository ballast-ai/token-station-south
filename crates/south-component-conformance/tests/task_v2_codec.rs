//! A single strict JSON boundary for task-v2 values and guest/host shims.

use proptest::prelude::*;
use serde_json::json;
use south_component_conformance::{SubmitOutcomeV2, task_v2_json::*};
use south_contracts::{TaskObservationV2, TaskScalarV2};

proptest! {
    #[test]
    fn arbitrary_codec_input_never_panics_and_accepted_values_roundtrip(input in "(?s).{0,2048}") {
        if let Ok(value) = parse_locator_json(&input) {
            prop_assert_eq!(parse_locator_json(&locator_json(&value).to_string()), Ok(value));
        }
        if let Ok(value) = parse_observation_json(&input) {
            let wire=observation_json(&value).unwrap();
            prop_assert_eq!(parse_observation_json(&wire.to_string()), Ok(value));
        }
        if let Ok(value) = parse_submit_outcome_json(&input) {
            let wire=submit_outcome_json(&value).unwrap();
            prop_assert_eq!(parse_submit_outcome_json(&wire.to_string()), Ok(value));
        }
        if let Ok(value) = parse_prepared_task_json(&input) {
            let wire=prepared_task_json(&value).unwrap();
            prop_assert_eq!(parse_prepared_task_json(&wire.to_string()), Ok(value));
        }
        if let Ok(value) = parse_render_context_json(&input) {
            prop_assert_eq!(parse_render_context_json(&render_context_json(&value).to_string()), Ok(value));
        }
    }

    #[test]
    fn valid_scalar_and_locator_json_roundtrip(id in any::<u64>(), segment in "[a-zA-Z0-9_-]{1,64}") {
        let scalar=json!(id);
        prop_assert_eq!(scalar_json(&scalar_from_json(&scalar).unwrap()).unwrap(), scalar);
        let wire=json!({"schema_version":1,"route":format!("v1/{segment}")});
        prop_assert_eq!(locator_json(&parse_locator_json(&wire.to_string()).unwrap()), wire);
    }
}

#[test]
fn canonical_submit_frame_must_fit_before_decode_succeeds() {
    let empty = r#"{"outcome":"rejected","error":{"code":"internal","http_status":500,"message":"","x":1e8}}"#;
    let input = empty.replace(
        "\"message\":\"\"",
        &format!("\"message\":\"{}\"", "x".repeat(MAX_TASK_V2_FACT_JSON_BYTES - empty.len())),
    );
    assert_eq!(input.len(), MAX_TASK_V2_FACT_JSON_BYTES);
    assert!(
        parse_submit_outcome_json(&input).is_err(),
        "short exponent input expands when canonicalized; never admit an unencodable outcome"
    );
}

#[test]
fn canonical_prepared_frame_must_fit_before_decode_succeeds() {
    let empty = r#"{"descriptor":{"method":"POST","url":"https://upstream.example/tasks","body":{"padding":"","x":1e8}},"locator":{"schema_version":1,"route":"v1/tasks"},"request_estimate":{"requested_seconds":null,"milliunits_per_second":null}}"#;
    let limit = south_contracts::MAX_JSON_REQUEST_BODY_BYTES;
    let input = empty.replace(
        "\"padding\":\"\"",
        &format!("\"padding\":\"{}\"", "x".repeat(limit - empty.len())),
    );
    assert_eq!(input.len(), limit);
    assert!(
        parse_prepared_task_json(&input).is_err(),
        "the canonical descriptor frame must fit the same boundary"
    );
}

#[test]
fn empty_artifact_variant_still_refuses_unknown_fields() {
    let value = json!({"state":"succeeded","artifacts":{"kind":"none","secret":"PRIVATE"},
        "usage":{"seconds":null,"milliunits":null,"tokens":null}});
    assert!(parse_observation_json(&value.to_string()).is_err());
}

#[test]
fn encoding_revalidates_public_variants_and_terminal_meaning() {
    use south_contracts::{TaskArtifactRefV2, TaskUsageFactsV2};
    for artifacts in [TaskArtifactRefV2::Urls(Vec::new()), TaskArtifactRefV2::FileId(String::new())]
    {
        assert!(
            observation_json(&TaskObservationV2::Succeeded {
                artifacts,
                usage: TaskUsageFactsV2::default(),
            })
            .is_err()
        );
    }
    assert!(submit_outcome_json(&SubmitOutcomeV2::Accepted(String::new())).is_err());
    assert!(
        submit_outcome_json(&SubmitOutcomeV2::AcceptedTerminal(TaskObservationV2::Progress {
            running: true,
            status_word: "processing".into()
        }))
        .is_err()
    );
    assert!(parse_submit_outcome_json(r#"{"outcome":"accepted-terminal","observation":{"state":"unknown","reason":"not observed"}}"#).is_err());
}

#[test]
fn locator_wire_is_strict_versioned_and_bounded() {
    let input = r#"{"schema_version":1,"route":"v1/videos/image2video"}"#;
    let locator = parse_locator_json(input).unwrap();
    assert_eq!(locator_json(&locator), serde_json::from_str::<serde_json::Value>(input).unwrap());
    for bad in [
        r#"{"schema_version":2,"route":"v1/tasks"}"#,
        r#"{"schema_version":1,"route":"v1/tasks","prompt":"PRIVATE-PROMPT"}"#,
        r#"{"schema_version":1,"route":"v1/tasks","route":"v1/other"}"#,
        r#"{"schema_version":1,"route":"https://other.example/"}"#,
    ] {
        let error = parse_locator_json(bad).unwrap_err();
        assert!(!error.contains("PRIVATE-PROMPT"));
    }
    assert!(
        parse_locator_json(&format!("{{\"schema_version\":1,\"route\":\"{}\"}}", "x".repeat(4097)))
            .is_err()
    );
}

#[test]
fn observation_roundtrip_keeps_all_usage_and_scalar_categories() {
    let input = json!({"state":"succeeded",
        "artifacts":{"kind":"urls","items":[
            {"url":"https://media.example/1?sig=SECRET","id":18_446_744_073_709_551_615_u64,"duration":"4.50"},
            {"url":"https://media.example/2","id":null,"duration":4.5}
        ]}, "usage":{"seconds":4.5,"milliunits":1200,"tokens":0}});
    let observation = parse_observation_json(&input.to_string()).unwrap();
    assert_eq!(observation_json(&observation).unwrap(), input);
    assert!(!format!("{observation:?}").contains("SECRET"));
    assert!(matches!(observation, TaskObservationV2::Succeeded { usage, .. }
        if usage.seconds()==Some(4.5) && usage.milliunits()==Some(1200) && usage.tokens()==Some(0)));
    for input in [
        json!({"state":"progress","running":false,"status_word":"submitted"}),
        json!({"state":"progress","running":true,"status_word":"processing"}),
        json!({"state":"unknown","reason":"not observable"}),
        json!({"state":"failed","kind":"cancelled","code":null,"message":null}),
    ] {
        assert_eq!(
            observation_json(&parse_observation_json(&input.to_string()).unwrap()).unwrap(),
            input
        );
    }
}

#[test]
fn codec_refuses_arbitrary_artifact_objects_and_invalid_usage() {
    for scalar in [json!({"secret":"VALUE"}), json!([1]), json!(true)] {
        assert!(scalar_from_json(&scalar).is_err());
    }
    assert!(scalar_json(&TaskScalarV2::Float(f64::NAN)).is_err());
    let input = json!({"state":"succeeded","artifacts":{"kind":"none"},
        "usage":{"seconds":null,"milliunits":0,"tokens":null}});
    assert_eq!(
        observation_json(&parse_observation_json(&input.to_string()).unwrap()).unwrap(),
        input
    );
    let mut invalid = input.clone();
    invalid["usage"]["seconds"] = json!(-1);
    assert!(parse_observation_json(&invalid.to_string()).is_err());
    invalid = input;
    invalid["usage"]["private"] = json!("SECRET");
    assert!(parse_observation_json(&invalid.to_string()).is_err());
}

#[test]
fn scalar_numbers_do_not_round_large_integers_through_float() {
    for input in [json!(u64::MAX), json!(i64::MIN), json!(1.25), json!("001.25"), json!(null)] {
        assert_eq!(scalar_json(&scalar_from_json(&input).unwrap()).unwrap(), input);
    }
}

#[test]
fn submit_terminal_carries_typed_facts_and_unknown_fields_are_rejected() {
    let input = json!({"outcome":"accepted-terminal","observation":{
        "state":"succeeded","artifacts":{"kind":"none"},
        "usage":{"seconds":null,"milliunits":null,"tokens":null}}});
    let outcome = parse_submit_outcome_json(&input.to_string()).unwrap();
    assert!(matches!(outcome, SubmitOutcomeV2::AcceptedTerminal(_)));
    assert_eq!(submit_outcome_json(&outcome).unwrap(), input);
    assert!(parse_submit_outcome_json(r#"{"outcome":"unknown","body":{"secret":"x"}}"#).is_err());
}

#[test]
fn render_context_roundtrip_requires_explicit_host_metadata() {
    let input = json!({"task_id":"host-task","created":123,"model":"public/model",
        "provider":"p","upstream_task_id":null});
    assert_eq!(render_context_json(&parse_render_context_json(&input.to_string()).unwrap()), input);
    let mut invalid = input;
    invalid["callback_url"] = json!("https://host/callback?nonce=SECRET");
    assert!(parse_render_context_json(&invalid.to_string()).is_err());
}

#[test]
fn prepared_and_rejected_ir_values_roundtrip_with_existing_extensions() {
    let prepared = json!({"descriptor":{"method":"POST","url":"https://upstream.example/tasks", "body":{"x":1e8}},"locator":{"schema_version":1,"route":"v1/tasks"},"request_estimate":{"requested_seconds":null,"milliunits_per_second":null}});
    let prepared = parse_prepared_task_json(&prepared.to_string()).unwrap();
    assert_eq!(
        parse_prepared_task_json(&prepared_task_json(&prepared).unwrap().to_string()).unwrap(),
        prepared
    );
    let rejected = json!({"outcome":"rejected","error":{"code":"internal","http_status":500,"message":"upstream unavailable","x":1e8}});
    let rejected = parse_submit_outcome_json(&rejected.to_string()).unwrap();
    assert_eq!(
        parse_submit_outcome_json(&submit_outcome_json(&rejected).unwrap().to_string()).unwrap(),
        rejected
    );
}
