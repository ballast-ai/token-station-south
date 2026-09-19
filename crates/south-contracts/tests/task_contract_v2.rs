//! Public behavior of the task-v2 fact and recovery vocabulary.

use proptest::prelude::*;
use south_contracts::{
    MAX_ARTIFACT_REF_BYTES, MAX_ARTIFACT_URLS, MAX_RELATIVE_PATH_BYTES, TaskArtifactRefV2,
    TaskArtifactV2, TaskLocatorV2, TaskObservationV2, TaskRenderContextV2, TaskScalarV2,
    TaskUsageFactsV2,
};

proptest! {
    #[test]
    fn arbitrary_locator_input_uses_the_existing_relative_path_boundary(input in ".{0,3000}") {
        let existing = south_contracts::RelativePathV1::parse(&input);
        let locator = TaskLocatorV2::new(1, &input);
        prop_assert_eq!(locator.is_ok(), existing.is_ok());
        if let Ok(locator) = locator {
            prop_assert_eq!(locator.route(), input.as_str());
            prop_assert_eq!(locator.schema_version(), 1);
        }
    }

    #[test]
    fn arbitrary_usage_has_no_nonfinite_or_negative_accepted_value(
        seconds in any::<f64>(), milliunits in any::<i64>(), tokens in any::<i64>()
    ) {
        let result = TaskUsageFactsV2::new(Some(seconds), Some(milliunits), Some(tokens));
        prop_assert_eq!(result.is_ok(), seconds.is_finite() && seconds >= 0.0 && milliunits >= 0 && tokens >= 0);
    }
}

#[test]
fn locator_keeps_only_a_versioned_relative_route() {
    let locator = TaskLocatorV2::new(1, "v1/videos/image2video").unwrap();
    assert_eq!(locator.schema_version(), 1);
    assert_eq!(locator.route(), "v1/videos/image2video");
    for route in [
        "",
        "/root",
        "https://other.example/x",
        "../x",
        "v1/../x",
        "v1/x?key=secret",
        "v1/x#secret",
        "v1/%2fsecret",
        "v1/\nsecret",
    ] {
        assert!(TaskLocatorV2::new(1, route).is_err(), "invalid route must fail");
    }
    assert!(TaskLocatorV2::new(2, "v1/tasks").is_err());
    assert!(TaskLocatorV2::new(1, &"x".repeat(MAX_RELATIVE_PATH_BYTES + 1)).is_err());
}

#[test]
fn usage_keeps_simultaneous_facts_and_distinguishes_zero_from_missing() {
    let facts = TaskUsageFactsV2::new(Some(4.5), Some(1200), Some(0)).unwrap();
    assert_eq!(facts.seconds(), Some(4.5));
    assert_eq!(facts.milliunits(), Some(1200));
    assert_eq!(facts.tokens(), Some(0));
    assert_ne!(facts, TaskUsageFactsV2::default());
    assert_eq!(TaskUsageFactsV2::new(None, None, None).unwrap(), TaskUsageFactsV2::default());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1] {
        assert!(TaskUsageFactsV2::new(Some(value), None, None).is_err());
    }
    assert!(TaskUsageFactsV2::new(None, Some(-1), None).is_err());
    assert!(TaskUsageFactsV2::new(None, None, Some(-1)).is_err());
}

#[test]
fn artifacts_keep_scalar_types_but_do_not_debug_sensitive_urls() {
    let artifact = TaskArtifactV2::new(
        "https://media.example/x?sig=ARTIFACT-SECRET",
        TaskScalarV2::Unsigned(u64::MAX),
        TaskScalarV2::String("4.50".into()),
    )
    .unwrap();
    assert_eq!(artifact.id(), &TaskScalarV2::Unsigned(u64::MAX));
    assert_eq!(artifact.duration(), &TaskScalarV2::String("4.50".into()));
    assert!(!format!("{artifact:?}").contains("ARTIFACT-SECRET"));
    let facts = TaskObservationV2::Succeeded {
        artifacts: TaskArtifactRefV2::urls(vec![artifact.clone()]).unwrap(),
        usage: TaskUsageFactsV2::default(),
    };
    assert!(facts.validate().is_ok());
    assert!(!format!("{facts:?}").contains("ARTIFACT-SECRET"));
    assert!(TaskArtifactV2::new("", TaskScalarV2::Null, TaskScalarV2::Null).is_err());
    assert!(
        TaskArtifactV2::new(
            &"u".repeat(MAX_ARTIFACT_REF_BYTES + 1),
            TaskScalarV2::Null,
            TaskScalarV2::Null
        )
        .is_err()
    );
    assert!(TaskArtifactRefV2::Urls(vec![artifact; MAX_ARTIFACT_URLS + 1]).validate().is_err());
    assert!(TaskArtifactRefV2::FileId(String::new()).validate().is_err());
    assert!(TaskScalarV2::Float(f64::NAN).validate().is_err());
    assert!(TaskScalarV2::Float(f64::INFINITY).validate().is_err());
    assert!(TaskScalarV2::String("x".repeat(MAX_ARTIFACT_REF_BYTES + 1)).validate().is_err());
}

#[test]
fn render_context_is_explicit_and_does_not_require_an_upstream_id() {
    let context =
        TaskRenderContextV2::new("gateway-id", 123, "public/model", "provider", None).unwrap();
    assert_eq!(context.task_id(), "gateway-id");
    assert_eq!(context.created(), 123);
    assert_eq!(context.model(), "public/model");
    assert_eq!(context.provider(), "provider");
    assert_eq!(context.upstream_task_id(), None);
    assert!(TaskRenderContextV2::new("", 123, "m", "p", None).is_err());
}
