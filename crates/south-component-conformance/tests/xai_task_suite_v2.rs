use south_component_conformance::{
    TaskFixturePackV2, reference_xai_task_v2::XaiTaskComponentV2, run_task_component_suite_v2,
};
use std::path::Path;
#[test]
fn frozen_xai_fixtures_pass_the_public_suite() {
    let pack = TaskFixturePackV2::load(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures-xai-task-v2"
    )))
    .unwrap();
    let report = run_task_component_suite_v2(&XaiTaskComponentV2, &pack);
    assert!(report.is_passing(), "{:?}", report.failures().collect::<Vec<_>>());
}
