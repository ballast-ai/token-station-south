//! `task-gemini-omni-v2` against the public task-v2 suite with its manifest, so task contract 8's
//! checks run (docs/design/2026-10-01-omni-task-component.md §10), and the mutations those checks
//! exist to catch.

use serde_json::{Value, json};
use south_component_conformance::reference_gemini_omni_task_v2::GeminiOmniTaskComponentV2;
use south_component_conformance::{
    CheckV1, ComponentResultV1, PreparedTaskV2, ReportV1, SubmitOutcomeV2, TaskComponentV2,
    TaskFixturePackV2, run_task_component_suite_v2_for_manifest,
};
use south_contracts::image::ArtifactEncodingV1;
use south_contracts::{
    HostMintedValuesV1, TaskArtifactRefV2, TaskInlineArtifactV2, TaskLocatorV2, TaskObservationV2,
    TaskRenderContextV2, TaskTokenBucketV2, TaskTokenBucketsV2, TaskUsageFactsV2,
};
use south_provider_api::{ComponentManifestV1, ComponentMetadataV1};
use std::path::Path;
use token_station_protocol::{
    ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts, ProviderConfig,
};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).unwrap()
}

fn pack() -> TaskFixturePackV2 {
    TaskFixturePackV2::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures-gemini-omni-task-v2"),
    )
    .unwrap()
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("components/task-gemini-omni-v2/manifest.json"))
            .unwrap(),
    )
    .unwrap()
}

fn failed_checks(report: &ReportV1) -> Vec<CheckV1> {
    report.failures().map(|outcome| outcome.check).collect()
}

#[test]
fn frozen_omni_fixtures_pass_the_public_suite_with_the_contract_8_checks() {
    let manifest = manifest();
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(manifest.metadata(), GeminiOmniTaskComponentV2.metadata());
    let report =
        run_task_component_suite_v2_for_manifest(&GeminiOmniTaskComponentV2, &pack(), &manifest);
    assert!(report.is_passing(), "{:?}", report.failures().collect::<Vec<_>>());
    for check in [
        CheckV1::DeclaredBucketsOnly,
        CheckV1::ReferenceIntegrity,
        CheckV1::UsageNeverDefaulted,
        CheckV1::MissingMeterIsNotZero,
    ] {
        assert!(report.outcomes().iter().any(|outcome| outcome.check == check), "{check:?} ran");
    }
}

/// The rows the record requires by name (§10), so a pack that loses one fails here.
#[test]
fn the_pack_carries_every_row_the_record_names() {
    let pack = pack();
    for row in [
        "task-v2.prepare.text",
        "task-v2.prepare.aspect-ratio",
        "task-v2.prepare.image",
        "task-v2.prepare.refused-duration",
        "task-v2.prepare.refused-n",
        "task-v2.prepare.refused-frame",
        "task-v2.prepare.gemini-text",
        "task-v2.prepare.gemini-refused-image",
        "task-v2.created.accepted",
        "task-v2.created.accepted-completed",
        "task-v2.created.gemini-completed-uri",
        "task-v2.created.http-400-json",
        "task-v2.created.http-400-sse",
        "task-v2.created.http-500",
        "task-v2.observation.progress",
        "task-v2.observation.succeeded",
        "task-v2.observation.succeeded-get",
        "task-v2.observation.missing-usage",
        "task-v2.observation.missing-video-bucket",
        "task-v2.observation.missing-thoughts",
        "task-v2.observation.sum-mismatch",
        "task-v2.observation.unknown-modality",
        "task-v2.observation.no-video",
        "task-v2.render.direct",
    ] {
        assert!(pack.cases().iter().any(|case| case.name == row), "{row} is missing");
    }
    assert!(pack.usage_meta("task-v2.observation.succeeded").is_some());
}

/// The reference with one behaviour broken, to show each contract 8 check fires.
struct Broken(Break);
#[derive(Clone, Copy)]
enum Break {
    /// Reports zeros when usage is missing.
    DefaultUsage,
    /// Reports a bucket the manifest does not declare.
    StrayBucket,
    /// Points the artifact at a string that is not a blob.
    PointerOffBlob,
    /// Renders a marker for an artifact that does not exist.
    DanglingMarker,
}

fn zeros() -> TaskUsageFactsV2 {
    let buckets = TaskTokenBucketsV2::new(
        [
            TaskTokenBucketV2::TotalInput,
            TaskTokenBucketV2::TotalOutput,
            TaskTokenBucketV2::VideoOutput,
            TaskTokenBucketV2::ReasoningOutput,
        ]
        .map(|bucket| (bucket, 0)),
    )
    .unwrap();
    TaskUsageFactsV2::new(None, None, None).unwrap().with_token_buckets(Some(buckets)).unwrap()
}

impl TaskComponentV2 for Broken {
    fn metadata(&self) -> ComponentMetadataV1 {
        GeminiOmniTaskComponentV2.metadata()
    }
    fn build_submit_request(
        &self,
        config: &ProviderConfig,
        request: &Value,
        minted: &HostMintedValuesV1,
    ) -> ComponentResultV1<PreparedTaskV2> {
        GeminiOmniTaskComponentV2.build_submit_request(config, request, minted)
    }
    fn parse_submit_response(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<SubmitOutcomeV2> {
        GeminiOmniTaskComponentV2.parse_submit_response(parts)
    }
    fn build_observe_request(
        &self,
        config: &ProviderConfig,
        model: &str,
        id: &str,
        locator: &TaskLocatorV2,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        GeminiOmniTaskComponentV2.build_observe_request(config, model, id, locator)
    }
    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV2> {
        let observation = GeminiOmniTaskComponentV2.parse_observation(parts)?;
        let video = |pointer: &str| {
            TaskArtifactRefV2::inline(vec![
                TaskInlineArtifactV2::new(pointer, ArtifactEncodingV1::Base64, "video/mp4")
                    .unwrap(),
            ])
            .unwrap()
        };
        Ok(match (self.0, observation) {
            (Break::DefaultUsage, TaskObservationV2::Unknown { reason })
                if reason.contains("usage")
                    || reason.contains("thought")
                    || reason.contains("video output") =>
            {
                TaskObservationV2::Succeeded {
                    artifacts: video("/steps/1/content/0/data"),
                    usage: zeros(),
                }
            }
            (Break::StrayBucket, TaskObservationV2::Succeeded { artifacts, usage }) => {
                let mut entries: Vec<_> = usage.token_buckets().unwrap().iter().collect();
                entries.push((TaskTokenBucketV2::AudioOutput, 1));
                let usage = TaskUsageFactsV2::new(None, None, None)
                    .unwrap()
                    .with_token_buckets(Some(TaskTokenBucketsV2::new(entries).unwrap()))
                    .unwrap();
                TaskObservationV2::Succeeded { artifacts, usage }
            }
            (Break::PointerOffBlob, TaskObservationV2::Succeeded { usage, .. }) => {
                TaskObservationV2::Succeeded { artifacts: video("/steps/0/signature"), usage }
            }
            (_, observation) => observation,
        })
    }
    fn build_artifact_request(
        &self,
        config: &ProviderConfig,
        locator: &TaskLocatorV2,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        GeminiOmniTaskComponentV2.build_artifact_request(config, locator, observation)
    }
    fn render_success(
        &self,
        observation: &TaskObservationV2,
        fetched: Option<&HttpResponseParts>,
        context: &TaskRenderContextV2,
    ) -> ComponentResultV1<Value> {
        let mut body = GeminiOmniTaskComponentV2.render_success(observation, fetched, context)?;
        if matches!(self.0, Break::DanglingMarker) {
            body["data"][0]["url"] = json!({"$south.artifact": {"index": 1}});
        }
        Ok(body)
    }
    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV2,
    ) -> ComponentResultV1<ErrorEnvelope> {
        GeminiOmniTaskComponentV2.map_terminal_failure(observation)
    }
}

#[test]
fn each_contract_8_check_fails_the_mutation_it_exists_for() {
    for (mutation, check) in [
        (Break::DefaultUsage, CheckV1::UsageNeverDefaulted),
        (Break::DefaultUsage, CheckV1::MissingMeterIsNotZero),
        (Break::StrayBucket, CheckV1::DeclaredBucketsOnly),
        (Break::PointerOffBlob, CheckV1::ReferenceIntegrity),
        (Break::DanglingMarker, CheckV1::ReferenceIntegrity),
    ] {
        let report =
            run_task_component_suite_v2_for_manifest(&Broken(mutation), &pack(), &manifest());
        assert!(failed_checks(&report).contains(&check), "{check:?}");
    }
}

/// A required bucket the component stops reporting is a `DeclaredBucketsOnly` failure even when
/// every fixture still matches: the manifest, not the fixture, says what a success carries.
#[test]
fn a_manifest_that_requires_more_than_the_component_reports_fails() {
    let mut manifest = manifest();
    manifest.token_buckets.push("audio_output".to_owned());
    manifest.required_token_buckets.push("audio_output".to_owned());
    let report =
        run_task_component_suite_v2_for_manifest(&GeminiOmniTaskComponentV2, &pack(), &manifest);
    assert!(failed_checks(&report).contains(&CheckV1::DeclaredBucketsOnly));
    assert!(
        failed_checks(&report).contains(&CheckV1::MissingMeterIsNotZero),
        "no sidecar names a wire member for the new bucket"
    );
}
