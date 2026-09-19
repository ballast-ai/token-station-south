//! Frozen pack and adversarial tests of the public task-v2 behavior gate.
use serde_json::Value;
use south_component_conformance::{
    CheckV1, ComponentResultV1, PreparedTaskV2, SubmitOutcomeV2, TaskComponentV2,
    TaskFixturePackV2, reference_kling_task_v2::KlingTaskReferenceV2, run_task_component_suite_v2,
};
use south_contracts::{HostMintedValuesV1, TaskLocatorV2, TaskObservationV2, TaskRenderContextV2};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use token_station_protocol::{
    ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts, ProviderConfig,
};

fn pack() -> TaskFixturePackV2 {
    TaskFixturePackV2::load(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures-kling-task-v2"
    )))
    .unwrap()
}
#[test]
fn frozen_v2_pack_passes_reference() {
    let report = run_task_component_suite_v2(&KlingTaskReferenceV2, &pack());
    assert!(report.is_passing(), "{report}");
    assert_eq!(report.suite(), "south.task-component.v2");
}
#[test]
fn v2_pack_covers_all_seven_operations() {
    assert!(pack().missing_families().is_empty());
}
struct Mutant {
    calls: AtomicUsize,
    terminal_on_404: bool,
}
impl TaskComponentV2 for Mutant {
    fn metadata(&self) -> south_provider_api::ComponentMetadataV1 {
        KlingTaskReferenceV2.metadata()
    }
    fn build_submit_request(
        &self,
        c: &ProviderConfig,
        r: &Value,
        m: &HostMintedValuesV1,
    ) -> ComponentResultV1<PreparedTaskV2> {
        KlingTaskReferenceV2.build_submit_request(c, r, m)
    }
    fn parse_submit_response(&self, p: &HttpResponseParts) -> ComponentResultV1<SubmitOutcomeV2> {
        KlingTaskReferenceV2.parse_submit_response(p)
    }
    fn build_observe_request(
        &self,
        c: &ProviderConfig,
        m: &str,
        id: &str,
        l: &TaskLocatorV2,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        KlingTaskReferenceV2.build_observe_request(c, m, id, l)
    }
    fn parse_observation(&self, p: &HttpResponseParts) -> ComponentResultV1<TaskObservationV2> {
        if self.terminal_on_404 {
            if p.status == 404 {
                return Ok(TaskObservationV2::Failed {
                    kind: south_contracts::TaskFailureKindV1::Failed,
                    code: None,
                    message: None,
                });
            }
            KlingTaskReferenceV2.parse_observation(p)
        } else {
            Ok(TaskObservationV2::Unknown {
                reason: format!("call {}", self.calls.fetch_add(1, Ordering::Relaxed)),
            })
        }
    }
    fn build_artifact_request(
        &self,
        c: &ProviderConfig,
        l: &TaskLocatorV2,
        o: &TaskObservationV2,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        KlingTaskReferenceV2.build_artifact_request(c, l, o)
    }
    fn render_success(
        &self,
        o: &TaskObservationV2,
        f: Option<&HttpResponseParts>,
        c: &TaskRenderContextV2,
    ) -> ComponentResultV1<Value> {
        KlingTaskReferenceV2.render_success(o, f, c)
    }
    fn map_terminal_failure(&self, o: &TaskObservationV2) -> ComponentResultV1<ErrorEnvelope> {
        KlingTaskReferenceV2.map_terminal_failure(o)
    }
}
#[test]
fn v2_suite_catches_terminal_from_failed_query() {
    let report = run_task_component_suite_v2(
        &Mutant { calls: AtomicUsize::new(0), terminal_on_404: true },
        &pack(),
    );
    assert!(report.failures().any(|row| row.check == CheckV1::TerminalOnlyFromTheWire));
}
#[test]
fn v2_suite_catches_nondeterminism() {
    let report = run_task_component_suite_v2(
        &Mutant { calls: AtomicUsize::new(0), terminal_on_404: false },
        &pack(),
    );
    assert!(report.failures().any(|row| row.check == CheckV1::Determinism));
}
#[test]
fn empty_v2_pack_cannot_pass_vacuously() {
    let report = run_task_component_suite_v2(&KlingTaskReferenceV2, &TaskFixturePackV2::default());
    assert!(report.failures().any(|row| row.check == CheckV1::Coverage));
    assert!(report.failures().any(|row| row.check == CheckV1::TerminalOnlyFromTheWire));
}
