//! The Kling task reference, checked against the adopting host's frozen word
//! table.
//!
//! **This is the provenance test.** The wire knowledge in the reference was
//! transcribed from the host's production `normalize_kling`, not captured from
//! Kling, so the thing worth pinning is that the transcription is faithful:
//! every case below is one the host's own `kling_word_table` test pins, with
//! the same input and the equivalent expectation.
//!
//! A divergence here means the component and the host disagree about what the
//! upstream said — which, until someone captures real traffic, is the strongest
//! statement this suite can make.

use serde_json::{Value, json};
use south_component_conformance::{
    SubmitOutcomeV1, TaskComponentV1, reference_kling_task::KlingTaskReferenceV1,
};
use south_contracts::{
    HostMintedValuesV1, TaskArtifactRefV1, TaskFailureKindV1, TaskMeterV1, TaskObservationV1,
};
use token_station_protocol::{Auth, HttpMethod, HttpResponseParts, ProviderConfig, SecretRef};

fn parts(status: u16, body: &Value) -> HttpResponseParts {
    HttpResponseParts {
        status,
        headers: std::collections::BTreeMap::new(),
        body: body.to_string(),
        extensions: token_station_protocol::Extensions::default(),
    }
}

fn observe(body: &Value) -> TaskObservationV1 {
    KlingTaskReferenceV1.parse_observation(&parts(200, body)).expect("a 2xx body is parseable")
}

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "kling",
        "base_url": "https://api.kling.example",
        "models": [],
    }))
    .expect("a minimal provider config")
}

fn minted() -> HostMintedValuesV1 {
    HostMintedValuesV1::new("task-01J9ZK3V7Q", None).expect("a well-formed id")
}

// ── The host's frozen word table, case for case ────────────────────────────

#[test]
fn submitted_and_processing_are_running_with_the_upstreams_own_word() {
    for word in ["submitted", "processing"] {
        let observation = observe(&json!({"data": {"task_status": word}}));
        assert_eq!(
            observation,
            TaskObservationV1::Running { status_word: word.to_owned() },
            "`{word}` is non-terminal and keeps its word"
        );
    }
}

/// The host's own success case, including both meters it reads.
#[test]
fn succeed_carries_the_artifact_and_the_exact_deduction() {
    let observation = observe(&json!({
        "code": 0,
        "data": {
            "task_status": "succeed",
            "final_unit_deduction": "2.5",
            "task_result": {
                "videos": [{"id": "v1", "url": "https://cdn.kling.example/a.mp4", "duration": "5"}]
            }
        }
    }));
    let TaskObservationV1::Succeeded { artifact, meter } = observation else {
        panic!("succeed is terminal");
    };
    assert_eq!(
        artifact,
        TaskArtifactRefV1::urls(vec!["https://cdn.kling.example/a.mp4".to_owned()])
            .expect("one url")
    );
    // The exact deduction wins over the derived duration: 2.5 units = 2500
    // milliunits, matching the host's `kling_final_milliunits`.
    assert_eq!(meter, Some(TaskMeterV1::Milliunits(2_500)));
}

/// Without a deduction the duration is the meter — the same fallback order the
/// host uses.
#[test]
fn a_success_without_a_deduction_reports_the_duration_instead() {
    let observation = observe(&json!({
        "data": {
            "task_status": "succeed",
            "task_result": {"videos": [{"url": "https://cdn.kling.example/b.mp4", "duration": 8}]}
        }
    }));
    let TaskObservationV1::Succeeded { meter, .. } = observation else {
        panic!("succeed is terminal");
    };
    assert_eq!(meter, Some(TaskMeterV1::Seconds(8.0)));
}

#[test]
fn failed_carries_the_upstreams_message() {
    let observation =
        observe(&json!({"data": {"task_status": "failed", "task_status_msg": "content risk"}}));
    assert_eq!(
        observation,
        TaskObservationV1::Failed {
            kind: TaskFailureKindV1::Failed,
            code: None,
            message: Some("content risk".to_owned()),
        }
    );
}

/// D3 rule 1: an unrecognised word is `Unknown`, never a synthesised failure.
#[test]
fn an_unrecognised_word_falls_through_to_unknown() {
    assert!(matches!(
        observe(&json!({"data": {"task_status": "throttled"}})),
        TaskObservationV1::Unknown { .. }
    ));
}

/// The host's hardest-won case: `succeed` with nothing to deliver is not a
/// success. Fabricating one would settle a task whose artifact never arrives.
#[test]
fn succeed_without_a_url_is_unknown_not_a_success() {
    assert!(matches!(
        observe(&json!({"data": {"task_status": "succeed"}})),
        TaskObservationV1::Unknown { .. }
    ));
}

/// D3 rule 2, and the row the ruling requires of every family: a failed
/// *query* is not a failed *task*.
#[test]
fn a_404_query_is_unknown_because_the_observation_did_not_happen() {
    let observation = KlingTaskReferenceV1
        .parse_observation(&parts(404, &json!({"message": "not found"})))
        .expect("a non-2xx is still parseable");
    assert!(matches!(observation, TaskObservationV1::Unknown { .. }));
    assert!(!observation.is_terminal());
}

// ── Submit ─────────────────────────────────────────────────────────────────

/// The host's id must reach the wire as `external_task_id` — that field is the
/// upstream idempotency anchor, and a resubmission without it bills twice.
#[test]
fn the_submit_body_carries_the_host_minted_id_where_the_dialect_reads_it() {
    let descriptor = KlingTaskReferenceV1
        .build_submit_request(
            &config(),
            &json!({"model": "kling-v3", "prompt": "a cat"}),
            &minted(),
        )
        .expect("a well-formed request");
    let body = descriptor.body.expect("submit sends a body");
    assert_eq!(body.get("external_task_id").and_then(Value::as_str), Some("task-01J9ZK3V7Q"));
    assert_eq!(descriptor.method, HttpMethod::Post);
    assert!(descriptor.url.ends_with("/v1/videos/text2video"));
}

/// An image picks the other creation path — the reason the observe side needs
/// the model.
#[test]
fn an_image_request_goes_to_the_image_path() {
    let descriptor = KlingTaskReferenceV1
        .build_submit_request(
            &config(),
            &json!({"model": "kling-v3", "image": "https://input.example/a.png"}),
            &minted(),
        )
        .expect("a well-formed request");
    assert!(descriptor.url.ends_with("/v1/videos/image2video"));
}

#[test]
fn a_created_task_reports_its_upstream_id() {
    let outcome = KlingTaskReferenceV1
        .parse_submit_response(&parts(200, &json!({"code": 0, "data": {"task_id": "up-77"}})))
        .expect("parseable");
    assert_eq!(outcome, SubmitOutcomeV1::Accepted("up-77".to_owned()));
}

/// A 2xx with a non-zero business code took the call and declined the work.
#[test]
fn a_business_refusal_is_rejected_not_an_error() {
    let outcome = KlingTaskReferenceV1
        .parse_submit_response(&parts(200, &json!({"code": 1102, "message": "quota exhausted"})))
        .expect("parseable");
    assert!(matches!(outcome, SubmitOutcomeV1::Rejected(_)));
}

/// **The funds-critical case.** No id and no refusal: the upstream may have
/// taken the work and may be billing for it. Reporting failure here would tell
/// the host to release a reservation against work that may be running.
#[test]
fn a_creation_with_no_id_is_unknown_so_the_reservation_survives() {
    let outcome = KlingTaskReferenceV1
        .parse_submit_response(&parts(200, &json!({"code": 0, "data": {}})))
        .expect("parseable");
    assert_eq!(outcome, SubmitOutcomeV1::Unknown);
}

// ── Observe, render, failure ───────────────────────────────────────────────

/// The query path mirrors the creation path; there is no generic
/// `/v1/videos/{id}` route.
#[test]
fn the_observe_url_mirrors_the_creation_path() {
    let descriptor = KlingTaskReferenceV1
        .build_observe_request(&config(), "kling-v3", "up-77")
        .expect("a well-formed query");
    assert_eq!(descriptor.method, HttpMethod::Get);
    assert!(descriptor.url.ends_with("/v1/videos/text2video/up-77"), "{}", descriptor.url);
}

#[test]
fn an_authenticated_observation_keeps_the_bound_credential_reference() {
    let mut config = config();
    config.auth = Some(SecretRef::new("bound-task-slot"));
    let descriptor = KlingTaskReferenceV1
        .build_observe_request(&config, "kling-v3", "up-77")
        .expect("a well-formed query");
    config.authorize(&descriptor).expect("observation keeps the submitted credential slot");
    assert_eq!(descriptor.auth, Some(Auth::bearer(SecretRef::new("bound-task-slot"))));
    assert_eq!(descriptor.method, HttpMethod::Get);
    assert!(descriptor.body.is_none());
    assert!(!descriptor.url.contains("bound-task-slot"));
    assert!(!serde_json::to_string(&descriptor.headers).unwrap().contains("bound-task-slot"));
}

#[test]
fn an_unauthenticated_observation_does_not_invent_a_credential_reference() {
    let config = config();
    let descriptor = KlingTaskReferenceV1
        .build_observe_request(&config, "kling-v3", "up-77")
        .expect("a well-formed query");
    config.authorize(&descriptor).expect("an explicitly unauthenticated deployment is allowed");
    assert!(descriptor.auth.is_none());
}

/// Kling's terminal observation already carries its URLs, so the host is never
/// asked to fetch. `None` means "already have it".
#[test]
fn kling_never_asks_the_host_to_fetch_an_artifact() {
    let observation = observe(&json!({
        "data": {
            "task_status": "succeed",
            "task_result": {"videos": [{"url": "https://cdn.kling.example/a.mp4"}]}
        }
    }));
    let request = KlingTaskReferenceV1
        .build_artifact_request(&config(), &observation)
        .expect("no fetch needed");
    assert!(request.is_none());
}

/// Artifacts are addressed by the host's own path, built from the id the host
/// minted — placed, never invented.
#[test]
fn the_rendered_body_addresses_artifacts_by_the_hosts_own_path() {
    let observation = observe(&json!({
        "data": {
            "task_status": "succeed",
            "task_result": {"videos": [{"url": "https://cdn.kling.example/a.mp4"}]}
        }
    }));
    let body = KlingTaskReferenceV1
        .render_success(&observation, None, &minted())
        .expect("a terminal success renders");
    let first = &body["data"][0];
    assert_eq!(first["url"], "https://cdn.kling.example/a.mp4");
    assert_eq!(first["artifact"], "/v1/video/tasks/task-01J9ZK3V7Q/artifacts/art_0");
}

#[test]
fn a_terminal_failure_maps_onto_the_stable_catalog() {
    let observation =
        observe(&json!({"data": {"task_status": "failed", "task_status_msg": "content risk"}}));
    let envelope = KlingTaskReferenceV1.map_terminal_failure(&observation).expect("a failure maps");
    assert!(envelope.message.contains("content risk"));
}
