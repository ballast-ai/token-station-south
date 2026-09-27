use south_component_conformance::{
    TaskFixturePackV2, reference_wan_image_task_v2::WanImageTaskComponentV2,
    run_task_component_suite_v2,
};
use std::path::Path;
#[test]
fn frozen_wan_image_fixtures_pass_the_public_suite() {
    let pack = TaskFixturePackV2::load(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures-wan-image-task-v2"
    )))
    .unwrap();
    let report = run_task_component_suite_v2(&WanImageTaskComponentV2, &pack);
    assert!(report.is_passing(), "{:?}", report.failures().collect::<Vec<_>>());
}
