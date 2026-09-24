//! Gates ① and ② for the official AWS Bedrock Converse component, run against
//! its native reference implementation over its own frozen fixture pack.
//!
//! A second component means a second pack: the suite is the same, the cases are
//! not. Sharing one pack between dialects would freeze whichever dialect
//! happened to be written first.
//!
//! This is the first provider component on the `host_signed` arm, so gate ①
//! here is also the first time that arm's manifest rules are exercised by a
//! shipped package: exactly one arm, a non-empty `emits`, and every emitted name
//! drawn from the signed-header vocabulary.

use std::path::Path;

use south_component_conformance::reference_bedrock_converse::BedrockConverseReferenceV1;
use south_component_conformance::{
    FixturePackV1, ProviderComponentV1, accepts_manifest, reported_identity_matches,
    run_provider_component_suite_v1,
};
use south_provider_api::{
    ComponentManifestV1, HostExpectationsV1, PROVIDER_WORLD, SIGNED_HEADER_NAMES,
    compatibility_matches,
};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

/// The manifest the component actually ships, read from disk rather than
/// hand-copied: a hand-copy resembles it, which is not the same as being it.
fn shipped_manifest() -> ComponentManifestV1 {
    let source = std::fs::read_to_string(
        repo_root().join("components/provider-bedrock-converse/manifest.json"),
    )
    .expect("the shipped component manifest reads");
    serde_json::from_str(&source).expect("the shipped component manifest parses")
}

fn shipped_pack() -> FixturePackV1 {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures-bedrock-converse");
    FixturePackV1::load(&directory).expect("the shipped fixture pack loads")
}

fn host_expectations() -> HostExpectationsV1 {
    HostExpectationsV1 {
        ir_schema_id: "token-station-protocol@0.3.0/v0.2.0".to_owned(),
        kernel_version: "0.2.0".to_owned(),
        kernel_revision: "72458e3a11fe157f9ac04818c44b62a3dd2cb09c".to_owned(),
        south_runtime: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

/// Gate ①: the package the component ships is admissible, and the identity it
/// reports at runtime is the identity its manifest claims.
#[test]
fn gate_one_admits_the_shipped_package_and_its_reported_identity() {
    let manifest = shipped_manifest();
    assert!(accepts_manifest(&manifest).is_ok(), "the shipped manifest must pass gate ①");
    assert!(
        compatibility_matches(&manifest, &host_expectations()).is_ok(),
        "the shipped manifest must satisfy this release's compatibility tuple"
    );
    assert!(
        reported_identity_matches(&BedrockConverseReferenceV1.metadata(), &manifest),
        "the identity the component reports must be the one its manifest claims"
    );
    assert_eq!(manifest.api_version, PROVIDER_WORLD);
}

/// The `host_signed` arm's own rules, spelled out rather than left to gate ①'s
/// pass/fail — this is the first shipped package to use the arm, so the facts it
/// depends on are worth naming.
#[test]
fn the_manifest_declares_the_host_signed_arm_and_nothing_beside_it() {
    let manifest = shipped_manifest();
    assert!(manifest.auth_arms.contains("host_signed"));
    assert_eq!(
        manifest.auth_arms.len(),
        1,
        "the schema refuses any second arm alongside host_signed, and the dialect needs none: \
         both Bedrock credential forms are the host's to apply"
    );
    // A component that never holds a credential must not claim a secret slot.
    assert!(
        manifest.permissions.secrets.is_empty(),
        "a host_signed component is signed for, so it references no secret of its own"
    );
    assert!(!manifest.emits.is_empty(), "the arm requires the emitted header set to be named");
    for header in &manifest.emits {
        assert!(
            SIGNED_HEADER_NAMES.contains(&header.as_str()),
            "`{header}` is not a signed-header name the host can emit"
        );
    }
    // The full set, not the three a credential without an STS session token
    // produces: `emits` is declared once and statically, while the host's actual
    // set is a function of the credential. Declaring the maximum is the only
    // choice that stays correct for the recommended (temporary-credential)
    // deployment shape. See the plan's A1 note for why neither choice is
    // strictly right.
    assert!(
        manifest.emits.iter().any(|header| header == "x-amz-security-token"),
        "the STS session-token header must be declared, or an STS deployment's signer would \
         emit a header the manifest never named"
    );
}

/// Gate ②: the reference implementation answers its own frozen pack exactly.
#[test]
fn gate_two_passes_over_the_shipped_pack() {
    let report = run_provider_component_suite_v1(&BedrockConverseReferenceV1, &shipped_pack());
    let failures: Vec<String> = report.failures().map(|outcome| format!("{outcome:?}")).collect();
    assert!(failures.is_empty(), "{} gate ② failures:\n{}", failures.len(), failures.join("\n"));
}

/// The pack must keep exercising the two shapes this dialect gets wrong most
/// easily. A pack can pass while having quietly lost a case, and these two are
/// the ones whose absence would not be obvious.
#[test]
fn the_shipped_pack_still_carries_the_decided_behaviours() {
    let names: Vec<String> = shipped_pack().cases().iter().map(|case| case.name.clone()).collect();
    for required in [
        // Withholding the whole toolConfig, not just its toolChoice.
        "tool-choice-none-withholds-the-whole-config",
        // Parallel results in one user message, which Converse requires.
        "parallel-tool-results-share-one-user-message",
        // Done waits for metadata, so a cut stream cannot settle as complete.
        "text-ends-in-two-phases",
    ] {
        // `CaseV1::name` is the full `provider.<family>.<case>`, so the case
        // name is matched as the trailing segment.
        assert!(
            names.iter().any(|name| name.ends_with(required)),
            "the pack lost the `{required}` case; it is in the pack because the dialect is easy \
             to get wrong here, not because it was convenient to write"
        );
    }
}
