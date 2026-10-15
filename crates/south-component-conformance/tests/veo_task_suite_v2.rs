//! `task-veo-v2` against the public task-v2 suite, and the per-family addressing and credential
//! that the Vertex family (design record `docs/design/2026-10-15-veo-vertex-task-component.md` §4)
//! adds beside the Gemini API family.

use serde_json::json;
use south_component_conformance::{
    TaskComponentV2, TaskFixturePackV2, reference_veo_task_v2::VeoTaskComponentV2,
    run_task_component_suite_v2, run_task_component_suite_v2_for_manifest,
};
use south_contracts::HostMintedValuesV1;
use south_provider_api::ComponentManifestV1;
use std::path::Path;
use token_station_protocol::{Auth, ProviderConfig};

fn pack() -> TaskFixturePackV2 {
    TaskFixturePackV2::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures-veo-task-v2")))
        .unwrap()
}

fn manifest() -> ComponentManifestV1 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).unwrap();
    serde_json::from_str(
        &std::fs::read_to_string(root.join("components/task-veo-v2/manifest.json")).unwrap(),
    )
    .unwrap()
}

fn config(provider: &str, base_url: &str) -> ProviderConfig {
    serde_json::from_value(json!({"provider": provider, "base_url": base_url,
        "auth": "provider_api_key"}))
    .unwrap()
}

const VERTEX_BASE: &str =
    "https://us-central1-aiplatform.example/v1/projects/sample-project/locations/us-central1";
const VERTEX_OPERATION: &str = "projects/sample-project/locations/us-central1/publishers/google/\
                                models/veo-3.1-generate-001/operations/op7";

#[test]
fn frozen_veo_fixtures_pass_the_public_suite() {
    let report = run_task_component_suite_v2(&VeoTaskComponentV2, &pack());
    assert!(report.is_passing(), "{:?}", report.failures().collect::<Vec<_>>());
}

#[test]
fn the_manifest_names_the_reference_and_passes_with_it() {
    let manifest = manifest();
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(manifest.metadata(), VeoTaskComponentV2.metadata());
    let report = run_task_component_suite_v2_for_manifest(&VeoTaskComponentV2, &pack(), &manifest);
    assert!(report.is_passing(), "{:?}", report.failures().collect::<Vec<_>>());
}

/// The rows the record and the task name for the Vertex family, so a pack that loses one fails.
#[test]
fn the_pack_carries_every_vertex_row() {
    let pack = pack();
    for row in [
        "task-v2.prepare.vertex-text",
        "task-v2.prepare.vertex-all-parameters",
        "task-v2.prepare.vertex-frames",
        "task-v2.prepare.vertex-storage-uri",
        "task-v2.prepare.unknown-family",
        "task-v2.created.vertex-accepted",
        "task-v2.created.vertex-name",
        "task-v2.observe.vertex-operation",
        "task-v2.observe.vertex-gemini-name",
        "task-v2.observe.vertex-gemini-locator",
        "task-v2.observe.gemini-vertex-locator",
        "task-v2.observe.vertex",
        "task-v2.observation.vertex-pending",
        "task-v2.observation.vertex-samples",
        "task-v2.observation.vertex-error",
        "task-v2.artifact.vertex-direct",
        "task-v2.artifact.vertex-gemini-locator",
        "task-v2.render.vertex-samples",
    ] {
        assert!(pack.cases().iter().any(|case| case.name == row), "the pack lacks {row}");
    }
}

/// Each family presents the credential its own way, and every descriptor stays under the bound
/// endpoint, so a host's `authorize` sends it: the Vertex poll leaves the location prefix's path
/// for the origin's `/v1/`, which is inside the endpoint because the operation name repeats the
/// bound project and location.
#[test]
fn each_family_addresses_and_authenticates_as_native_does() {
    let minted = HostMintedValuesV1::new("host-1", None).unwrap();
    let request = json!({"model": "veo-3.1-generate-001", "prompt": "p"});

    let vertex = config("vertex-veo-video", VERTEX_BASE);
    let prepared = VeoTaskComponentV2.build_submit_request(&vertex, &request, &minted).unwrap();
    vertex.authorize(&prepared.descriptor).unwrap();
    assert!(matches!(prepared.descriptor.auth, Some(Auth::Bearer { .. })));
    assert_eq!(
        prepared.descriptor.url,
        format!("{VERTEX_BASE}/publishers/google/models/veo-3.1-generate-001:predictLongRunning")
    );
    let poll = VeoTaskComponentV2
        .build_observe_request(&vertex, "veo", VERTEX_OPERATION, &prepared.locator)
        .unwrap();
    vertex.authorize(&poll).unwrap();
    assert!(matches!(poll.auth, Some(Auth::Bearer { .. })));
    assert_eq!(poll.url, format!("https://us-central1-aiplatform.example/v1/{VERTEX_OPERATION}"));

    let gemini = config("veo-video", "https://generativelanguage.example");
    let prepared = VeoTaskComponentV2.build_submit_request(&gemini, &request, &minted).unwrap();
    gemini.authorize(&prepared.descriptor).unwrap();
    assert!(
        matches!(&prepared.descriptor.auth, Some(Auth::Header { name, .. }) if name == "x-goog-api-key")
    );
    // A locator minted for one family is refused by the other.
    assert!(
        VeoTaskComponentV2
            .build_observe_request(&vertex, "veo", VERTEX_OPERATION, &prepared.locator)
            .is_err()
    );
}

/// A Vertex operation in another project or location is polled where native polls it, and a host
/// refuses to send that descriptor: the credential never leaves the bound location.
#[test]
fn a_vertex_operation_outside_the_bound_location_is_not_sendable() {
    let vertex = config("vertex-veo-video", VERTEX_BASE);
    let prepared = VeoTaskComponentV2
        .build_submit_request(
            &vertex,
            &json!({"model": "veo-3.1-generate-001", "prompt": "p"}),
            &HostMintedValuesV1::new("host-1", None).unwrap(),
        )
        .unwrap();
    let elsewhere = "projects/123456789/locations/us-central1/publishers/google/models/\
                     veo-3.1-generate-001/operations/op7";
    let poll = VeoTaskComponentV2
        .build_observe_request(&vertex, "veo", elsewhere, &prepared.locator)
        .unwrap();
    assert!(vertex.authorize(&poll).is_err());
}
