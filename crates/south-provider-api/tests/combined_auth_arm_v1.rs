//! Gate ① for the combined `bearer_and_header_secret` arm (B7b,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §4.3 and §13.7): a provider manifest may
//! declare it, a task manifest may not, and it stands independent of the two arms it combines.

use std::collections::BTreeSet;
use std::path::Path;

use south_provider_api::{
    ComponentManifestV1, ManifestErrorV1, PROVIDER_AUTH_ARMS, TASK_AUTH_ARMS,
};

const COMBINED: &str = "bearer_and_header_secret";

fn shipped(package: &str) -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components")
        .join(package)
        .join("manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn arms(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn the_provider_vocabulary_names_the_combined_arm_and_the_task_one_does_not() {
    assert!(PROVIDER_AUTH_ARMS.contains(&COMBINED));
    assert!(!TASK_AUTH_ARMS.contains(&COMBINED));
    // The task vocabulary is the provider one minus the combined word, in the same order.
    let expected: Vec<&str> =
        PROVIDER_AUTH_ARMS.iter().copied().filter(|a| *a != COMBINED).collect();
    assert_eq!(TASK_AUTH_ARMS, expected.as_slice());
}

#[test]
fn a_provider_manifest_may_declare_the_combined_arm_alone_or_beside_the_others() {
    let mut manifest = shipped("provider-openai-compatible");
    for declared in [&[COMBINED][..], &["bearer", COMBINED], &["bearer", COMBINED, "header_secret"]]
    {
        manifest.auth_arms = arms(declared);
        assert_eq!(manifest.validate(), Ok(()), "{declared:?}");
        let decoded: ComponentManifestV1 =
            serde_json::from_str(&serde_json::to_string(&manifest).unwrap()).unwrap();
        assert_eq!(decoded.auth_arms, manifest.auth_arms, "the word round-trips");
    }
}

#[test]
fn a_task_manifest_may_not_declare_the_combined_arm() {
    for package in ["task-kling", "task-minimax-v2"] {
        let mut manifest = shipped(package);
        manifest.auth_arms = arms(&[COMBINED]);
        assert!(
            matches!(
                manifest.validate(),
                Err(ManifestErrorV1::AuthArmIsNotInTheWorldVocabulary { ref auth_arm, .. })
                    if auth_arm == COMBINED
            ),
            "{package}: {:?}",
            manifest.validate()
        );
    }
}

#[test]
fn host_signed_still_admits_no_other_arm() {
    let mut manifest = shipped("provider-bedrock-converse");
    assert!(manifest.auth_arms.contains("host_signed"));
    manifest.auth_arms.insert(COMBINED.to_owned());
    assert_eq!(manifest.validate(), Err(ManifestErrorV1::HostSignedAdmitsNoOtherArm));
}

#[test]
fn declaring_secret_headers_still_needs_the_header_secret_arm_itself() {
    // The combined arm carries only sanctioned names (Q35), so it does not stand in for the arm
    // that admits a package's own.
    let mut manifest = shipped("provider-openai-compatible");
    manifest.auth_arms = arms(&["bearer", COMBINED]);
    manifest.secret_headers = vec!["x-acme-key".to_owned()];
    assert_eq!(
        manifest.validate(),
        Err(ManifestErrorV1::SecretHeadersRequireTheHeaderSecretArm("x-acme-key".to_owned()))
    );
}
