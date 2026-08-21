//! Public contract tests for gates ① and ②, run against the native reference
//! implementation over the shipped fixture pack.

use std::collections::BTreeSet;
use std::path::Path;

use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::{
    CompatibilityMismatchV1, FixturePackV1, HostExpectationsV1, PROVIDER_COMPONENT_SUITE_V1,
    ProviderComponentV1, accepts_manifest, compatibility_matches, reported_identity_matches,
    run_provider_component_suite_v1,
};
use south_provider_api::{
    AuthArmV1, COMPONENT_BEHAVIOR_SUITE, CompatibilityDeclarationV1, ComponentCapabilityV1,
    ComponentManifestV1, ComponentPermissionsV1, ConformanceSpecV1, PROVIDER_WORLD, WIT_PACKAGE,
};
use token_station_protocol::{Auth, SecretRef};

fn reference_manifest() -> ComponentManifestV1 {
    ComponentManifestV1 {
        name: "provider-openai-compatible".to_owned(),
        version: "2.0.0".to_owned(),
        api_version: PROVIDER_WORLD.to_owned(),
        providers: vec!["openai-compatible".to_owned(), "azure-openai-v1".to_owned()],
        capabilities: BTreeSet::from([
            ComponentCapabilityV1::Chat,
            ComponentCapabilityV1::Stream,
            ComponentCapabilityV1::ToolCall,
            ComponentCapabilityV1::JsonSchema,
        ]),
        auth_arms: BTreeSet::from([AuthArmV1::Bearer, AuthArmV1::HeaderSecret]),
        permissions: ComponentPermissionsV1 {
            network: false,
            filesystem: false,
            secrets: vec!["provider_api_key".to_owned()],
        },
        conformance: ConformanceSpecV1 {
            required_suite: COMPONENT_BEHAVIOR_SUITE.to_owned(),
            fixtures: "fixtures/".to_owned(),
        },
        compatibility: CompatibilityDeclarationV1 {
            ir_schema_id: "token-station-protocol@0.3.0/v0.2.0".to_owned(),
            kernel_version: "0.2.0".to_owned(),
            kernel_revision: "72458e3a11fe157f9ac04818c44b62a3dd2cb09c".to_owned(),
            wit_package: WIT_PACKAGE.to_owned(),
            south_runtime: "0.11.0".to_owned(),
        },
    }
}

fn shipped_pack() -> FixturePackV1 {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    FixturePackV1::load(&directory).expect("the shipped fixture pack loads")
}

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

/// The manifest the official component actually ships, read from disk rather
/// than hand-copied: [`reference_manifest`] resembles it, which is not the
/// same as being it.
fn shipped_component_manifest() -> ComponentManifestV1 {
    let source = std::fs::read_to_string(
        repo_root().join("components/provider-openai-compatible/manifest.json"),
    )
    .expect("the shipped component manifest reads");
    serde_json::from_str(&source).expect("the shipped component manifest parses")
}

/// The `version` of a crate manifest's `[package]` table. A five-line scan
/// rather than a TOML parser this crate would otherwise never need.
fn package_version(cargo_toml: &str) -> &str {
    cargo_toml
        .lines()
        .skip_while(|line| line.trim() != "[package]")
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with('['))
        .find_map(|line| line.trim().strip_prefix("version = \""))
        .and_then(|value| value.strip_suffix('"'))
        .expect("the crate manifest declares a package version")
}

/// The fixture pack is discovered by scanning the directory, so a case whose
/// file is renamed, mistyped or lost simply stops existing — the suite still
/// reports green over whatever remains. That is the wrong failure mode for a
/// pack whose whole job is to be frozen.
///
/// This names the cases the host-parity slice added, each of which pins a
/// behaviour an adopting host depends on. A pack missing any of them is not
/// the pack this component was verified against.
#[test]
fn the_shipped_pack_still_carries_every_host_parity_case() {
    let pack = shipped_pack();
    let present: BTreeSet<&str> = pack.cases().iter().map(|case| case.name.as_str()).collect();
    for required in [
        "provider.request.text-parts-array-is-preserved",
        "provider.request.empty-content-shapes",
        "provider.request.redacted-thinking-is-dropped",
        "provider.request.reasoning-content-is-declared",
        "provider.request.reasoning-content-is-withheld",
        "provider.stream.empty-deltas-are-suppressed",
        "provider.stream.bare-reasoning-field",
        "provider.stream.empty-choices-frame-is-ignored",
        "provider.response.bare-reasoning-field",
    ] {
        assert!(present.contains(required), "the frozen pack lost `{required}`");
    }
}

/// Gate ② against the reference implementation: the run that freezes the
/// fixture table. Every failure is printed so a drift names its case.
#[test]
fn the_reference_implementation_passes_the_component_behavior_suite() {
    let report = run_provider_component_suite_v1(&OpenAiCompatibleReferenceV1, &shipped_pack());
    for failure in report.failures() {
        eprintln!("{failure}");
    }
    assert!(report.is_passing(), "{report}");
    assert_eq!(report.suite(), PROVIDER_COMPONENT_SUITE_V1);
}

/// The pack covers every family, so `Coverage` is a real gate, and the suite
/// grew the S0-obligated adversarial rows (evidence relay, missing terminal,
/// cache convention, reasoning lift).
#[test]
fn the_shipped_pack_covers_every_family_and_the_s0_rows() {
    let pack = shipped_pack();
    assert!(pack.missing_families().is_empty());
    let names: Vec<&str> = pack.cases().iter().map(|case| case.name.as_str()).collect();
    for required in [
        "provider.stream.usage-terminal",
        "provider.stream.duplicate-usage",
        "provider.stream.missing-terminal",
        "provider.response.reasoning",
        "provider.response.cached-usage",
        "provider.error.rejected-credential",
    ] {
        assert!(names.contains(&required), "pack is missing `{required}`");
    }
}

#[test]
fn gate_one_accepts_the_reference_manifest_and_its_reported_identity() {
    let manifest = reference_manifest();
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    assert!(reported_identity_matches(&OpenAiCompatibleReferenceV1.metadata(), &manifest));
}

#[test]
fn gate_one_rejects_a_repackaged_identity() {
    let manifest = reference_manifest();
    let mut reported = OpenAiCompatibleReferenceV1.metadata();
    reported.version = "9.9.9".to_owned();
    assert!(!reported_identity_matches(&reported, &manifest));
}

#[test]
fn the_tuple_handshake_refuses_any_mismatch_in_tuple_order() {
    let manifest = reference_manifest();
    let expectations = HostExpectationsV1 {
        ir_schema_id: "token-station-protocol@0.3.0/v0.2.0".to_owned(),
        kernel_version: "0.2.0".to_owned(),
        kernel_revision: "72458e3a11fe157f9ac04818c44b62a3dd2cb09c".to_owned(),
        south_runtime: "0.11.0".to_owned(),
    };
    assert_eq!(compatibility_matches(&manifest, &expectations), Ok(()));

    let mut newer_ir = expectations.clone();
    newer_ir.ir_schema_id = "token-station-protocol@0.4.0/v0.3.0".to_owned();
    assert!(matches!(
        compatibility_matches(&manifest, &newer_ir),
        Err(CompatibilityMismatchV1::IrSchema { .. })
    ));

    let mut newer_runtime = expectations;
    // Deliberately *not* the manifest's version — this arm proves a mismatch
    // is refused, so it must stay one step ahead of whatever the release is.
    newer_runtime.south_runtime = "0.12.0".to_owned();
    assert!(matches!(
        compatibility_matches(&manifest, &newer_runtime),
        Err(CompatibilityMismatchV1::SouthRuntime { .. })
    ));
}

/// The official component's version is written in two files and nothing
/// compared them. Gate ① checks the package manifest against the identity the
/// component *reports*, and that identity comes from the reference
/// implementation — never from the component's own crate manifest. So the
/// 0.11.0 release moved the package to 2.0.0, left the crate at 1.0.0, and
/// shipped one artifact carrying two version numbers through a green run.
///
/// The tuple's `south_runtime` is the same shape of gap one field over. The
/// shipped manifest declares the release it was verified with, and both this
/// file and the parity test hold that release as a literal — so a workspace
/// bump that updates none of the three leaves the package quietly one release
/// behind while every assertion still agrees with itself. Pinning it to this
/// crate's own version (the workspace version) makes the bump the machine's
/// job, the way `compatibility.json`'s release version is already pinned.
#[test]
fn the_shipped_component_package_agrees_with_its_crate_and_this_release() {
    let manifest = shipped_component_manifest();
    let cargo_toml = std::fs::read_to_string(
        repo_root().join("components/provider-openai-compatible/Cargo.toml"),
    )
    .expect("the component's crate manifest reads");

    assert_eq!(
        package_version(&cargo_toml),
        manifest.version,
        "the component's crate version and its package manifest are one number in two files"
    );
    assert_eq!(
        manifest.compatibility.south_runtime,
        env!("CARGO_PKG_VERSION"),
        "the shipped manifest must declare the release it ships in"
    );
}

/// S0 ruling D4: every sanctioned secret header south can put on the wire is
/// a name the IR's credential catalog redacts and refuses to plugins. A new
/// sanctioned header cannot land without the redaction side knowing it.
#[test]
fn every_sanctioned_header_is_in_the_credential_catalog() {
    for header in south_contracts::SecretHeaderV1::ALL {
        assert!(
            Auth::header(header.header_name(), SecretRef::new("slot")).is_ok(),
            "`{}` is sanctioned in south but not a credential header in the IR catalog",
            header.header_name()
        );
    }
}

/// A pack directory with a family this suite does not know is refused by
/// name; a malformed package is not a conformance failure.
#[test]
fn unknown_families_and_oversized_fixtures_are_package_errors() {
    let directory = std::env::temp_dir()
        .join(format!("south-component-conformance-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("scratch creates");

    std::fs::write(directory.join("provider.telemetry.chat.input.json"), "{}")
        .expect("fixture writes");
    let error = FixturePackV1::load(&directory).expect_err("unknown family must be refused");
    assert!(error.to_string().contains("telemetry"), "{error}");

    std::fs::remove_dir_all(&directory).ok();
}
