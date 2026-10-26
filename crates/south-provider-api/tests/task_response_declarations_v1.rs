//! Gate ① for task contract 8's package declarations (docs/design/2026-10-01-omni-task-component.md
//! §3.4, §6.2, §7.1): `response_blob_paths`, `token_buckets` and `required_token_buckets`.
//!
//! They are task-v2 declarations, they need task contract 8, the paths follow the media elision
//! grammar, the bucket words come from the closed task vocabulary, and every required bucket is a
//! declared one. Absent, a manifest validates exactly as before contract 8.

use south_provider_api::{
    ComponentManifestV1, ManifestErrorV1, TASK_INLINE_CONTRACT, TASK_TOKEN_BUCKETS,
};

fn task_manifest() -> ComponentManifestV1 {
    serde_json::from_str(
        r#"{
          "name": "task-example-v2",
          "version": "0.1.0",
          "api_version": "task-adapter-v2",
          "providers": ["example-video"],
          "capabilities": ["submit", "observe", "render"],
          "auth_arms": ["bearer"],
          "permissions": {"network": false, "filesystem": false, "secrets": ["provider_api_key"]},
          "conformance": {"required_suite": "south.task-component.v2", "fixtures": "fixtures/"},
          "compatibility": {
            "ir_schema_id": "token-station-protocol@0.7.0/v0.6.0",
            "kernel_version": "0.6.0",
            "kernel_revision": "a04925518b95c6557374252db7fbe7f026688bde",
            "wit_package": "token-station:task-adapter@2.0.0",
            "south_runtime": "0.61.0",
            "contracts": {"task": 7}
          }
        }"#,
    )
    .unwrap()
}

fn contract_8() -> ComponentManifestV1 {
    let mut manifest = task_manifest();
    manifest.compatibility.contracts.insert("task".to_owned(), TASK_INLINE_CONTRACT);
    manifest.response_blob_paths = vec!["/steps/*/content/*/data".to_owned()];
    manifest.token_buckets =
        ["total_input", "video_output", "total_output", "reasoning_output", "text_input"]
            .map(str::to_owned)
            .to_vec();
    manifest.required_token_buckets = ["total_input", "video_output"].map(str::to_owned).to_vec();
    manifest
}

#[test]
fn a_contract_7_package_without_the_declarations_validates_as_before() {
    assert_eq!(task_manifest().validate(), Ok(()));
    let json = serde_json::to_value(task_manifest()).unwrap();
    for key in ["response_blob_paths", "token_buckets", "required_token_buckets"] {
        assert!(json.get(key).is_none(), "{key} is not written when absent");
    }
}

#[test]
fn a_contract_8_package_declares_paths_and_buckets() {
    assert_eq!(contract_8().validate(), Ok(()));
    let back: ComponentManifestV1 =
        serde_json::from_value(serde_json::to_value(contract_8()).unwrap()).unwrap();
    assert_eq!(back, contract_8());
    assert_eq!(TASK_INLINE_CONTRACT, 8);
    assert_eq!(TASK_TOKEN_BUCKETS.len(), 14);
}

#[test]
fn the_declarations_need_task_contract_8() {
    for declared in [Some(7), None] {
        let mut manifest = contract_8();
        manifest.compatibility.contracts.remove("task");
        if let Some(contract) = declared {
            manifest.compatibility.contracts.insert("task".to_owned(), contract);
        }
        assert_eq!(
            manifest.validate(),
            Err(ManifestErrorV1::TaskResponseDeclarationsRequireContract8 { declared }),
            "{declared:?}"
        );
    }
}

#[test]
fn the_declarations_belong_to_the_task_v2_world() {
    let mut manifest = contract_8();
    manifest.api_version = "task-adapter-v1".to_owned();
    manifest.compatibility.wit_package = "token-station:task-adapter@1.0.0".to_owned();
    manifest.conformance.required_suite = "south.task-component.v1".to_owned();
    assert_eq!(
        manifest.validate(),
        Err(ManifestErrorV1::TaskResponseDeclarationsAreTaskV2Declarations)
    );
}

#[test]
fn elision_paths_follow_the_media_grammar() {
    for bad in ["", "steps", "/a~2b", "/a~", &format!("/{}", "a".repeat(256))] {
        let mut manifest = contract_8();
        manifest.response_blob_paths = vec![bad.to_owned()];
        assert!(
            matches!(manifest.validate(), Err(ManifestErrorV1::InvalidResponseBlobPaths(_))),
            "{bad}"
        );
    }
    let mut repeated = contract_8();
    repeated.response_blob_paths = vec!["/data".to_owned(), "/data".to_owned()];
    assert!(matches!(repeated.validate(), Err(ManifestErrorV1::InvalidResponseBlobPaths(_))));
    let mut too_many = contract_8();
    too_many.response_blob_paths = (0..33).map(|i| format!("/p{i}")).collect();
    assert!(matches!(too_many.validate(), Err(ManifestErrorV1::InvalidResponseBlobPaths(_))));
    let mut escaped = contract_8();
    escaped.response_blob_paths = vec!["/a~1b/*/c~0d".to_owned()];
    assert_eq!(escaped.validate(), Ok(()));
}

#[test]
fn buckets_come_from_the_closed_vocabulary_and_required_ones_are_declared() {
    let refuse = |declared: &[&str], required: &[&str]| {
        let mut manifest = contract_8();
        manifest.token_buckets = declared.iter().map(|word| (*word).to_owned()).collect();
        manifest.required_token_buckets = required.iter().map(|word| (*word).to_owned()).collect();
        assert!(
            matches!(manifest.validate(), Err(ManifestErrorV1::InvalidTokenBuckets(_))),
            "{declared:?} {required:?}"
        );
    };
    refuse(&["thought"], &[]);
    refuse(&["total_input", "total_input"], &[]);
    refuse(&["total_input"], &["video_output"]);
    refuse(&["total_input"], &["total_input", "total_input"]);
    refuse(&[], &["total_input"]);
    let mut every = contract_8();
    every.token_buckets = TASK_TOKEN_BUCKETS.iter().map(|word| (*word).to_owned()).collect();
    assert_eq!(every.validate(), Ok(()));
}
