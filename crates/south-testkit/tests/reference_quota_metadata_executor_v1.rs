use std::time::Duration;

use south_testkit::{
    ReferenceAssembledProviderQuotaMetadataExecutorV1, run_provider_quota_metadata_conformance_v1,
};

#[tokio::test(flavor = "current_thread")]
async fn reference_executor_uses_real_core_for_every_metadata_case() {
    let executor = ReferenceAssembledProviderQuotaMetadataExecutorV1::new();
    let report = tokio::time::timeout(
        Duration::from_secs(5),
        run_provider_quota_metadata_conformance_v1(&executor),
    )
    .await
    .expect("structured metadata runner watchdog must not expire")
    .expect("reference executor should pass the metadata extension suite");

    assert_eq!(report.passed_case_ids().len(), 3);
}
