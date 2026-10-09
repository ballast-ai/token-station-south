use std::{fmt::Display, time::Duration};

use south_provider_conformance::{
    SAFE_FETCH_CONFORMANCE_SUITE_ID, SAFE_FETCH_CONFORMANCE_SUITE_VERSION, SafeFetchFixtureV1,
    safe_fetch_fixtures_v1,
};
use south_testkit::{
    ReferenceSafeFetchExecutorV1, SafeFetchArtifactV1, SafeFetchConformanceFailureV1,
    SafeFetchConformanceReportV1, SafeFetchExecutorV1, SafeFetchPortsV1,
    run_safe_fetch_conformance_v1,
};
use static_assertions::{assert_impl_all, assert_not_impl_any};

assert_impl_all!(ReferenceSafeFetchExecutorV1: Send, Sync);
assert_impl_all!(SafeFetchPortsV1: Send, Sync);
assert_not_impl_any!(SafeFetchArtifactV1: Display);
assert_not_impl_any!(SafeFetchPortsV1: Display);
assert_not_impl_any!(SafeFetchConformanceReportV1: Display);
assert_not_impl_any!(SafeFetchConformanceFailureV1: Display);

/// The reference executor passes its own suite, every row, under a structured watchdog.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_safe_fetch_conforms_under_a_structured_watchdog() {
    let executor = ReferenceSafeFetchExecutorV1::new();
    let dynamic: &dyn SafeFetchExecutorV1 = &executor;

    let report =
        tokio::time::timeout(Duration::from_secs(30), run_safe_fetch_conformance_v1(dynamic))
            .await
            .expect("safe fetch conformance watchdog expired")
            .expect("reference executor must conform");

    assert_eq!(report.suite_id(), SAFE_FETCH_CONFORMANCE_SUITE_ID);
    assert_eq!(report.suite_version(), SAFE_FETCH_CONFORMANCE_SUITE_VERSION);
    let canonical: Vec<_> =
        safe_fetch_fixtures_v1().iter().map(SafeFetchFixtureV1::case_id).collect();
    assert_eq!(report.passed_case_ids(), &canonical);
    assert_eq!(report.passed_case_ids().len(), 26);
}

/// The two stall rows end by the executor's own timeout, not the runner's: in paused time the
/// clock advances exactly to the case's total timeout.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn reference_stall_rows_end_at_their_own_total_timeout() {
    for fixture in safe_fetch_fixtures_v1() {
        if !matches!(
            fixture.case_id(),
            south_provider_conformance::SafeFetchCaseIdV1::ResolutionStallTimesOut
                | south_provider_conformance::SafeFetchCaseIdV1::BodyStallTimesOut
        ) {
            continue;
        }
        let ports = SafeFetchPortsV1::new(fixture);
        let started = tokio::time::Instant::now();
        let outcome = ReferenceSafeFetchExecutorV1::new().fetch(fixture.input(), &ports).await;
        assert_eq!(started.elapsed(), fixture.input().total_timeout());
        assert_eq!(
            outcome.expect_err("a stall times out"),
            south_provider_conformance::SafeFetchFailureCodeV1::Timeout
        );
    }
}

#[test]
fn diagnostics_never_echo_names_or_bodies() {
    let artifact = SafeFetchArtifactV1::new(
        "media-type-debug-sentinel".to_owned(),
        b"body-debug-sentinel".to_vec(),
        Some("content-type-debug-sentinel".to_owned()),
    );
    let debug = format!("{artifact:?}");
    for sentinel in
        ["media-type-debug-sentinel", "body-debug-sentinel", "content-type-debug-sentinel"]
    {
        assert!(!debug.contains(sentinel), "artifact echoed {sentinel}");
    }
    for fixture in safe_fetch_fixtures_v1() {
        let debug = format!("{:?}", SafeFetchPortsV1::new(fixture));
        for sentinel in ["cdn.example.com", "proxy.example.com", "password-debug-sentinel"] {
            assert!(!debug.contains(sentinel), "ports echoed {sentinel}");
        }
    }
}
