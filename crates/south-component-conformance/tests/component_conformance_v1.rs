//! Public contract tests for gates ① and ②, run against the native reference
//! implementation over the shipped fixture pack.

use std::collections::BTreeSet;
use std::path::Path;

use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::{
    FixturePackV1, PROVIDER_COMPONENT_SUITE_V1, ProviderComponentV1, accepts_manifest,
    reported_identity_matches, run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::{
    COMPONENT_BEHAVIOR_SUITE, CompatibilityDeclarationV1, ComponentManifestV1,
    ComponentPermissionsV1, ConformanceSpecV1, PROVIDER_WORLD, UsageEvidenceV1, WIT_PACKAGE,
};
use south_provider_api::{CompatibilityMismatchV1, HostExpectationsV1, compatibility_matches};
use token_station_protocol::{Auth, SecretRef};

/// The manifest's `emits` vocabulary (`SIGNED_HEADER_NAMES`) is the wire-name
/// projection of the host half's frozen `SignedHeaderV1` — repeated in
/// `south-provider-api` because that crate depends on no other south crate.
/// This is the test the repetition is licensed by.
#[test]
fn the_manifest_signed_header_vocabulary_is_the_host_halfs() {
    let host_half: Vec<&str> =
        south_contracts::SignedHeaderV1::ALL.iter().map(|header| header.header_name()).collect();
    assert_eq!(south_provider_api::SIGNED_HEADER_NAMES, host_half.as_slice());
}

/// Gate ①'s `secret_headers` rules are repeated in `south-provider-api` for the same reason
/// (B7a, host-zero-vendor-boundary §10): the bounds, the undeclarable list and the name syntax must
/// agree with the host half's `DeclaredSecretHeaderV1`, or a manifest gate ① admitted could carry
/// a name the host refuses, or the reverse.
#[test]
fn the_manifest_secret_header_rules_are_the_host_halfs() {
    assert_eq!(
        south_provider_api::UNDECLARABLE_SECRET_HEADER_NAMES,
        south_contracts::UNDECLARABLE_SECRET_HEADER_NAMES
    );
    assert_eq!(
        south_provider_api::MAX_SECRET_HEADER_NAME_BYTES,
        south_contracts::MAX_SECRET_HEADER_NAME_BYTES
    );
    assert_eq!(
        south_provider_api::MAX_SECRET_HEADERS,
        south_contracts::MAX_DECLARED_SECRET_HEADERS
    );

    let longest = "k".repeat(south_contracts::MAX_SECRET_HEADER_NAME_BYTES);
    let too_long = "k".repeat(south_contracts::MAX_SECRET_HEADER_NAME_BYTES + 1);
    let mut corpus = vec![
        "",
        "x-acme-key",
        "X-Acme-Key",
        "x acme",
        "x:acme",
        "x/acme",
        "x\"acme",
        "x(acme)",
        "x@acme",
        "x,acme",
        "x;acme",
        "x=acme",
        "x?acme",
        "x[acme]",
        "x{acme}",
        "x\tacme",
        "x\u{7f}acme",
        "\u{e9}",
        "!#$%&'*+-.^_`|~",
        "0",
        longest.as_str(),
        too_long.as_str(),
    ];
    corpus.extend(south_contracts::UNDECLARABLE_SECRET_HEADER_NAMES);
    for name in corpus {
        assert_eq!(
            south_provider_api::validate_secret_header_name(name).is_ok(),
            south_contracts::DeclaredSecretHeaderV1::parse(name).is_ok(),
            "gate ① and the host half disagree on {name:?}"
        );
    }
}

fn reference_manifest() -> ComponentManifestV1 {
    ComponentManifestV1 {
        name: "provider-openai-compatible".to_owned(),
        version: "2.3.0".to_owned(),
        api_version: PROVIDER_WORLD.to_owned(),
        providers: vec![
            "openai-compatible".to_owned(),
            "azure-openai-v1".to_owned(),
            "github-copilot".to_owned(),
        ],
        capabilities: BTreeSet::from([
            "chat".to_owned(),
            "stream".to_owned(),
            "tool_call".to_owned(),
            "json_schema".to_owned(),
        ]),
        auth_arms: BTreeSet::from(["bearer".to_owned(), "header_secret".to_owned()]),
        emits: Vec::new(),
        secret_headers: Vec::new(),
        usage_evidence: UsageEvidenceV1::Reported,
        stream_framing: south_provider_api::StreamFramingV1::Bytes,
        signing: None,
        credentials: None,
        request_facts: std::collections::BTreeMap::new(),
        endpoint: std::collections::BTreeMap::new(),
        config_schema: std::collections::BTreeMap::new(),
        query_parameters: Vec::new(),
        quota_headers: Vec::new(),
        user_agent: std::collections::BTreeMap::new(),
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
            ir_schema_id: "token-station-protocol@0.4.0/v0.3.0".to_owned(),
            kernel_version: "0.3.0".to_owned(),
            kernel_revision: "6822aab1dea54ef646cb2206595cd4955ff9764a".to_owned(),
            wit_package: WIT_PACKAGE.to_owned(),
            south_runtime: env!("CARGO_PKG_VERSION").to_owned(),
            runtime_abi: None,
            kernel_contracts: std::collections::BTreeMap::new(),
            contracts: std::collections::BTreeMap::new(),
        },
    }
}

/// The package's own `manifest.json`, which carries the per-family declarations and the
/// `credentials` section the hand-built tuple above leaves out.
fn shipped_manifest() -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/provider-openai-compatible/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("the manifest reads"))
        .expect("the manifest parses")
}

fn shipped_pack() -> FixturePackV1 {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    FixturePackV1::load(&directory).expect("the shipped fixture pack loads")
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

/// Gate ② against the reference implementation and the shipped manifest: the
/// run that freezes the fixture table, including the `github-copilot` family's
/// credential recipe (boundary §13.5 D7). Every failure is printed so a drift
/// names its case.
#[test]
fn the_reference_implementation_passes_the_component_behavior_suite() {
    let manifest = shipped_manifest();
    assert_eq!(manifest.validate(), Ok(()));
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &shipped_pack(),
        &manifest,
    );
    for failure in report.failures() {
        eprintln!("{failure}");
    }
    assert!(report.is_passing(), "{report}");
    assert_eq!(report.suite(), PROVIDER_COMPONENT_SUITE_V1);
    let credential_cases: Vec<&str> = report
        .outcomes()
        .iter()
        .filter(|row| row.check == south_component_conformance::CheckV1::CredentialRecipeMatch)
        .map(|row| row.case.as_str())
        .collect();
    assert_eq!(credential_cases.len(), 8, "{credential_cases:?}");
}

/// The Copilot recipe and user-agent apply to their own family only: the
/// package's other two families keep their static slot, the operator's key and
/// no user-agent (§13.5 D2, D7).
#[test]
fn the_copilot_declarations_apply_to_the_github_copilot_family_only() {
    let manifest = shipped_manifest();
    assert!(manifest.credentials_for("github-copilot").is_some());
    assert_eq!(manifest.credentials_for("openai-compatible"), None);
    assert_eq!(manifest.credentials_for("azure-openai-v1"), None);

    let instances = south_component_conformance::DeclaredInstancesV1::from_manifest(&manifest)
        .expect("the shipped manifest passes gate ①");
    assert_eq!(
        instances.user_agent("github-copilot").map(south_contracts::DeclaredUserAgentV1::as_str),
        Some("GitHubCopilotChat/0.43.0")
    );
    assert_eq!(instances.user_agent("openai-compatible"), None);
    assert_eq!(instances.user_agent("azure-openai-v1"), None);
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
        ir_schema_id: "token-station-protocol@0.4.0/v0.3.0".to_owned(),
        kernel_version: "0.3.0".to_owned(),
        kernel_revision: "6822aab1dea54ef646cb2206595cd4955ff9764a".to_owned(),
        south_runtime: env!("CARGO_PKG_VERSION").to_owned(),
    };
    assert_eq!(compatibility_matches(&manifest, &expectations), Ok(()));

    let mut newer_ir = expectations.clone();
    newer_ir.ir_schema_id = "token-station-protocol@99.99.99/v99.99.99".to_owned();
    assert!(matches!(
        compatibility_matches(&manifest, &newer_ir),
        Err(CompatibilityMismatchV1::IrSchema { .. })
    ));

    let mut newer_runtime = expectations;
    // Deliberately *not* the manifest's version — this arm proves a mismatch is refused, so the
    // value must never be a release number.
    //
    // It used to be "the next version", and a blanket version bump collapsed it into the matching
    // value twice: once during 0.14.0, and the `shipped_packages_v1` comment records an earlier
    // round. A sentinel no release can ever equal is the structural fix — the trap is not that
    // people are careless with `sed`, it is that a literal one step ahead of the release is
    // *indistinguishable* from a literal that should track the release.
    newer_runtime.south_runtime = "99.99.99".to_owned();
    assert!(matches!(
        compatibility_matches(&manifest, &newer_runtime),
        Err(CompatibilityMismatchV1::SouthRuntime { .. })
    ));
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
