//! `south.task-component.v1` run against the Kling reference and its pack.
//!
//! Two things are being proven here, and they are different:
//!
//! 1. the shipped pack passes against the shipped reference — the ordinary
//!    gate ② statement;
//! 2. the suite's checks **bite**. A conformance suite that cannot fail is
//!    decoration, so each mutation below breaks one rule and expects exactly
//!    that rule to report it.

use std::path::Path;

use south_component_conformance::{
    TASK_COMPONENT_SUITE_V1, TaskComponentV1, TaskFixturePackV1,
    reference_kling_task::KlingTaskReferenceV1, run_task_component_suite_v1,
};
use south_contracts::{HostMintedValuesV1, TaskObservationV1};
use token_station_protocol::{
    ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts, ProviderConfig,
};

fn pack() -> TaskFixturePackV1 {
    TaskFixturePackV1::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures-kling-task")))
        .expect("the shipped pack loads")
}

#[test]
fn the_shipped_pack_passes_against_the_shipped_reference() {
    let report = run_task_component_suite_v1(&KlingTaskReferenceV1, &pack());
    assert!(
        report.is_passing(),
        "gate ② must pass for the reference it was frozen against: {:?}",
        report.failures().collect::<Vec<_>>()
    );
    assert_eq!(report.suite(), TASK_COMPONENT_SUITE_V1);
}

/// Every family carries at least one case, so no check passes by never being
/// asked.
#[test]
fn the_pack_covers_every_family() {
    assert_eq!(pack().missing_families(), Vec::new());
}

// ── The suite's checks, each proven to bite ────────────────────────────────

/// A component wrapping the reference, with one behaviour replaced.
struct Mutant<F>(F);

impl<F> TaskComponentV1 for Mutant<F>
where
    F: Fn(&HttpResponseParts) -> Option<TaskObservationV1>,
{
    fn metadata(&self) -> south_provider_api::ComponentMetadataV1 {
        KlingTaskReferenceV1.metadata()
    }
    fn build_submit_request(
        &self,
        config: &ProviderConfig,
        request: &serde_json::Value,
        minted: &HostMintedValuesV1,
    ) -> Result<HttpRequestDescriptor, ErrorEnvelope> {
        KlingTaskReferenceV1.build_submit_request(config, request, minted)
    }
    fn parse_submit_response(
        &self,
        parts: &HttpResponseParts,
    ) -> Result<south_component_conformance::SubmitOutcomeV1, ErrorEnvelope> {
        KlingTaskReferenceV1.parse_submit_response(parts)
    }
    fn build_observe_request(
        &self,
        config: &ProviderConfig,
        model: &str,
        id: &str,
    ) -> Result<HttpRequestDescriptor, ErrorEnvelope> {
        KlingTaskReferenceV1.build_observe_request(config, model, id)
    }
    fn parse_observation(
        &self,
        parts: &HttpResponseParts,
    ) -> Result<TaskObservationV1, ErrorEnvelope> {
        (self.0)(parts).map_or_else(|| KlingTaskReferenceV1.parse_observation(parts), Ok)
    }
    fn build_artifact_request(
        &self,
        config: &ProviderConfig,
        observation: &TaskObservationV1,
    ) -> Result<Option<HttpRequestDescriptor>, ErrorEnvelope> {
        KlingTaskReferenceV1.build_artifact_request(config, observation)
    }
    fn render_success(
        &self,
        observation: &TaskObservationV1,
        fetched: Option<&HttpResponseParts>,
        minted: &HostMintedValuesV1,
    ) -> Result<serde_json::Value, ErrorEnvelope> {
        KlingTaskReferenceV1.render_success(observation, fetched, minted)
    }
    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV1,
    ) -> Result<ErrorEnvelope, ErrorEnvelope> {
        KlingTaskReferenceV1.map_terminal_failure(observation)
    }
}

/// **The ruling this world exists to protect.** A component that reads a 404
/// as "the task is gone" settles work that may still be running, and the host
/// releases a reservation against it.
#[test]
fn a_component_that_reads_a_404_as_terminal_is_caught() {
    let mutant = Mutant(|parts: &HttpResponseParts| {
        (parts.status == 404).then(|| TaskObservationV1::Failed {
            kind: south_contracts::TaskFailureKindV1::ProviderExpired,
            code: None,
            message: Some("gone".to_owned()),
        })
    });
    let report = run_task_component_suite_v1(&mutant, &pack());
    assert!(!report.is_passing(), "a terminal from a failed query must be caught");
    let detail = format!("{:?}", report.failures().collect::<Vec<_>>());
    assert!(
        detail.contains("TerminalOnlyFromTheWire"),
        "the failure must name the rule it broke, got: {detail}"
    );
}

/// A component that answers differently on the second call makes a pass
/// meaningless: the run that admitted it is not the run the host gets.
#[test]
fn a_non_deterministic_component_is_caught() {
    let counter = std::sync::atomic::AtomicUsize::new(0);
    let mutant = Mutant(move |parts: &HttpResponseParts| {
        (parts.status == 200).then(|| {
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            TaskObservationV1::Unknown { reason: format!("call {n}") }
        })
    });
    let report = run_task_component_suite_v1(&mutant, &pack());
    assert!(!report.is_passing());
    let detail = format!("{:?}", report.failures().collect::<Vec<_>>());
    assert!(detail.contains("Determinism"), "got: {detail}");
}

/// A pack missing the required non-2xx row fails rather than passes: a check
/// that never runs describes nothing.
#[test]
fn a_pack_without_a_failed_query_row_fails_the_ruling() {
    let dir = tempdir();
    for name in [
        "task.submit.text-to-video",
        "task.created.accepted",
        "task.observe.mirrors-creation-path",
        "task.observation.processing",
        "task.render.urls-become-host-paths",
        "task.failure.content-risk",
    ] {
        for half in ["input", "expected"] {
            let file = format!("{name}.{half}.json");
            std::fs::copy(
                Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures-kling-task")).join(&file),
                dir.join(&file),
            )
            .expect("copy");
        }
    }
    let trimmed = TaskFixturePackV1::load(&dir).expect("the trimmed pack loads");
    let report = run_task_component_suite_v1(&KlingTaskReferenceV1, &trimmed);
    assert!(!report.is_passing(), "a pack owing the 404 row must not pass");
    let detail = format!("{:?}", report.failures().collect::<Vec<_>>());
    assert!(detail.contains("every family owes one such row"), "got: {detail}");
}

fn tempdir() -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!(
        "south-task-pack-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&base).expect("temp dir");
    base
}
