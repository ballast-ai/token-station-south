//! Gate ① for `min_poll_interval_seconds`, the task package's minimum poll interval (the task poll
//! interval record, `docs/design/2026-10-14-task-min-poll-interval.md` §3; ruled V-Q8 (a)).
//!
//! The field is optional and absent means the host's own default; when present it is whole
//! seconds within the inclusive bounds, and only a task world (v1 or v2) may declare it. Absent, a
//! manifest validates and serializes exactly as before the field existed.

use std::path::Path;

use serde_json::{Value, json};
use south_provider_api::{
    ComponentManifestV1, MAX_TASK_POLL_INTERVAL_SECONDS, MIN_TASK_POLL_INTERVAL_SECONDS,
    ManifestErrorV1,
};

fn shipped(package: &str) -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components")
        .join(package)
        .join("manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn task_manifest_json() -> Value {
    json!({
      "name": "task-example-v2",
      "version": "0.1.0",
      "api_version": "task-adapter-v2",
      "providers": ["example-video"],
      "capabilities": ["submit", "observe", "render"],
      "auth_arms": ["bearer"],
      "permissions": {"network": false, "filesystem": false, "secrets": ["provider_api_key"]},
      "conformance": {"required_suite": "south.task-component.v2", "fixtures": "fixtures/"},
      "compatibility": {
        "ir_schema_id": "token-station-protocol@0.6.0/v0.5.0",
        "kernel_version": "0.5.0",
        "kernel_revision": "c0bca10c2d9f9a439680865feb65c9edf598fc65",
        "wit_package": "token-station:task-adapter@2.0.0",
        "south_runtime": "0.61.0",
        "contracts": {"task": 7}
      }
    })
}

fn task_manifest() -> ComponentManifestV1 {
    serde_json::from_value(task_manifest_json()).unwrap()
}

fn declaring(seconds: u32) -> ComponentManifestV1 {
    let mut manifest = task_manifest();
    manifest.min_poll_interval_seconds = Some(seconds);
    manifest
}

#[test]
fn absent_means_none_and_validates_and_serializes_as_before() {
    let manifest = task_manifest();
    assert_eq!(manifest.min_poll_interval_seconds, None);
    assert_eq!(manifest.validate(), Ok(()));
    let json = serde_json::to_value(&manifest).unwrap();
    assert!(json.get("min_poll_interval_seconds").is_none(), "not written when absent");
}

#[test]
fn a_declared_interval_parses_validates_and_round_trips() {
    let mut source = task_manifest_json();
    source["min_poll_interval_seconds"] = json!(12);
    let manifest: ComponentManifestV1 = serde_json::from_value(source).unwrap();
    assert_eq!(manifest.min_poll_interval_seconds, Some(12));
    assert_eq!(manifest.validate(), Ok(()));
    let json = serde_json::to_value(&manifest).unwrap();
    assert_eq!(json["min_poll_interval_seconds"], json!(12));
    assert_eq!(serde_json::from_value::<ComponentManifestV1>(json).unwrap(), manifest);
}

#[test]
fn the_bounds_are_inclusive() {
    assert_eq!(MIN_TASK_POLL_INTERVAL_SECONDS, 1);
    assert_eq!(MAX_TASK_POLL_INTERVAL_SECONDS, 600);
    for seconds in [MIN_TASK_POLL_INTERVAL_SECONDS, 5, 12, MAX_TASK_POLL_INTERVAL_SECONDS] {
        assert_eq!(declaring(seconds).validate(), Ok(()), "{seconds}");
    }
    for seconds in [0, MAX_TASK_POLL_INTERVAL_SECONDS + 1, 12_000, u32::MAX] {
        assert_eq!(
            declaring(seconds).validate(),
            Err(ManifestErrorV1::MinPollIntervalOutOfBounds(seconds)),
            "{seconds}"
        );
    }
}

#[test]
fn only_whole_non_negative_seconds_parse() {
    for value in [json!(-1), json!(1.5), json!("12"), json!(null), json!(4_294_967_296_u64)] {
        let mut source = task_manifest_json();
        source["min_poll_interval_seconds"] = value.clone();
        let parsed = serde_json::from_value::<ComponentManifestV1>(source);
        // `null` reads as absent, like every other optional field of the manifest.
        if value.is_null() {
            assert_eq!(parsed.unwrap().min_poll_interval_seconds, None);
        } else {
            assert!(parsed.is_err(), "{value}");
        }
    }
}

#[test]
fn the_task_v1_world_may_declare_it_too() {
    let mut manifest = shipped("task-kling");
    assert_eq!(manifest.validate(), Ok(()));
    manifest.min_poll_interval_seconds = Some(12);
    assert_eq!(manifest.validate(), Ok(()));
}

#[test]
fn only_a_task_world_declares_it() {
    for package in ["provider-openai-compatible", "embeddings-openai-compatible", "image-azure"] {
        let mut manifest = shipped(package);
        assert_eq!(manifest.validate(), Ok(()), "{package}");
        manifest.min_poll_interval_seconds = Some(12);
        assert_eq!(
            manifest.validate(),
            Err(ManifestErrorV1::MinPollIntervalIsATaskWorldDeclaration),
            "{package}"
        );
    }
}

/// V-Q8 (a): one package-level interval, 12 s for `task-minimax-v2` (the v1 Hailuo floor; H3's
/// officially suggested 10 s rises to it). No other shipped task package documents an upstream
/// cadence in this repository, so each leaves the field absent and gets the host's default.
#[test]
fn only_task_minimax_v2_declares_an_interval() {
    let minimax = shipped("task-minimax-v2");
    assert_eq!(minimax.min_poll_interval_seconds, Some(12));
    assert_eq!(minimax.validate(), Ok(()));
    for package in [
        "task-bailian-v2",
        "task-byteplus-v2",
        "task-gemini-omni-v2",
        "task-gmi-image-v2",
        "task-kling",
        "task-kling-v2",
        "task-veo-v2",
        "task-wan-image-v2",
        "task-xai-v2",
    ] {
        assert_eq!(shipped(package).min_poll_interval_seconds, None, "{package}");
    }
}
