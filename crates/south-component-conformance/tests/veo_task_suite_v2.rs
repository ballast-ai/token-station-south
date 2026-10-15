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
        "task-v2.observe.vertex-project-number",
        "task-v2.observe.vertex-other-shape",
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

fn vertex_submission() -> (ProviderConfig, south_contracts::TaskLocatorV2) {
    let vertex = config("vertex-veo-video", VERTEX_BASE);
    let prepared = VeoTaskComponentV2
        .build_submit_request(
            &vertex,
            &json!({"model": "veo-3.1-generate-001", "prompt": "p"}),
            &HostMintedValuesV1::new("host-1", None).unwrap(),
        )
        .unwrap();
    (vertex, prepared.locator)
}

/// Q-VV1 (lv 2026-10-15; **unverified**, no capture): Vertex may name the operation with the
/// project number where the binding carries the project id. A publisher-model operation name is
/// polled under the binding's own location prefix, the prefix the submit used, so a host's
/// `authorize` sends it; the operation's model and id are kept.
#[test]
fn a_project_number_operation_name_is_polled_under_the_binding_prefix() {
    let (vertex, locator) = vertex_submission();
    let by_number = "projects/123456789012/locations/us-central1/publishers/google/models/\
                     veo-3.1-generate-001/operations/op7";
    let poll =
        VeoTaskComponentV2.build_observe_request(&vertex, "veo", by_number, &locator).unwrap();
    assert_eq!(
        poll.url,
        format!("{VERTEX_BASE}/publishers/google/models/veo-3.1-generate-001/operations/op7")
    );
    vertex.authorize(&poll).unwrap();
    assert!(matches!(poll.auth, Some(Auth::Bearer { .. })));
    // Another location is replaced as well: the whole `projects/*/locations/*` prefix is the
    // binding's.
    let elsewhere = "projects/123456789012/locations/europe-west4/publishers/google/models/\
                     veo-3.1-generate-001/operations/op7";
    let poll =
        VeoTaskComponentV2.build_observe_request(&vertex, "veo", elsewhere, &locator).unwrap();
    assert_eq!(
        poll.url,
        format!("{VERTEX_BASE}/publishers/google/models/veo-3.1-generate-001/operations/op7")
    );
    vertex.authorize(&poll).unwrap();
}

/// A name that already repeats the bound project and location polls the URL native polls, byte
/// for byte: the rewrite changes nothing there.
#[test]
fn a_matching_operation_name_polls_the_native_url_unchanged() {
    let (vertex, locator) = vertex_submission();
    let poll = VeoTaskComponentV2
        .build_observe_request(&vertex, "veo", VERTEX_OPERATION, &locator)
        .unwrap();
    assert_eq!(poll.url, format!("https://us-central1-aiplatform.example/v1/{VERTEX_OPERATION}"));
    assert_eq!(
        poll.url,
        format!("{VERTEX_BASE}/publishers/google/models/veo-3.1-generate-001/operations/op7")
    );
    vertex.authorize(&poll).unwrap();
}

/// A `projects/…` name of any other shape is not rewritten: it is polled verbatim at the origin's
/// `/v1/{name}`, as before Q-VV1 and as native, so one outside the bound location yields a
/// descriptor a host's `authorize` refuses and the credential never leaves the location. A name
/// with an empty, `.` or `..` segment or a foreign byte is refused before any URL is built.
#[test]
fn an_operation_name_of_another_shape_is_polled_verbatim_or_refused() {
    let (vertex, locator) = vertex_submission();
    for other in [
        "projects/123456789012/locations/us-central1/operations/op7",
        "projects/123456789012/operations/op7",
        "projects/123456789012/locations/us-central1/publishers/google/models/\
         veo-3.1-generate-001/operations/op7/extra",
        "projects/123456789012/locations/us-central1/publishers/other/models/\
         veo-3.1-generate-001/operations/op7",
    ] {
        let poll =
            VeoTaskComponentV2.build_observe_request(&vertex, "veo", other, &locator).unwrap();
        assert_eq!(poll.url, format!("https://us-central1-aiplatform.example/v1/{other}"));
        assert!(vertex.authorize(&poll).is_err(), "{other} must stay outside the binding");
    }
    // The same other shape under the bound project and location is sendable, as before.
    let inside = "projects/sample-project/locations/us-central1/operations/op7";
    let poll = VeoTaskComponentV2.build_observe_request(&vertex, "veo", inside, &locator).unwrap();
    assert_eq!(poll.url, format!("{VERTEX_BASE}/operations/op7"));
    vertex.authorize(&poll).unwrap();
    for malformed in [
        "projects//locations/us-central1/publishers/google/models/veo/operations/op7",
        "projects/../locations/us-central1/publishers/google/models/veo/operations/op7",
        "projects/123/locations/us-central1/publishers/google/models/veo/operations/op?7",
    ] {
        let refused = VeoTaskComponentV2.build_observe_request(&vertex, "veo", malformed, &locator);
        assert_eq!(refused.unwrap_err().message, "invalid Veo operation name", "{malformed}");
    }
}
