//! The S3 acceptance test: the official reference component, inside the
//! sandbox, passes gate ② byte-for-byte — the same suite, the same fixture
//! pack, the same frozen expectations that judged the native reference.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::sandbox::SandboxedComponentV1;
use south_component_conformance::{
    FixturePackV1, ProviderComponentV1, accepts_manifest, reported_identity_matches,
    run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::ComponentManifestV1;
use south_provider_api::{
    CompatibilityMismatchV1, HostExpectationsV1, HostRangeV1, compatibility_admits,
    compatibility_matches,
};
use south_provider_runtime::{
    ComponentRuntimeV1, LoadErrorV1, LoadedComponentV1, NoSecretsV1, RuntimeLimitsV1,
};

#[path = "support/gate2_report.rs"]
mod gate2_report;
#[path = "support/host_range.rs"]
mod host_range;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

/// Builds the official component once per test process.
fn component_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let status = Command::new("bash")
            .arg(repo_root().join("scripts/build-reference-component.sh"))
            .status()
            .expect("bash is on PATH");
        assert!(
            status.success(),
            "the official component must build; run `rustup target add wasm32-wasip2` if the \
             target is missing"
        );
        repo_root().join(
            "components/provider-openai-compatible/target/wasm32-wasip2/release/provider_openai_compatible.wasm",
        )
    })
}

fn shipped_manifest() -> (String, ComponentManifestV1) {
    let source = std::fs::read_to_string(
        repo_root().join("components/provider-openai-compatible/manifest.json"),
    )
    .expect("the shipped manifest reads");
    let manifest: ComponentManifestV1 =
        serde_json::from_str(&source).expect("the shipped manifest parses");
    (source, manifest)
}

fn sandboxed() -> SandboxedComponentV1 {
    SandboxedComponentV1::new(loaded())
}

fn loaded() -> LoadedComponentV1 {
    let runtime = ComponentRuntimeV1::new(RuntimeLimitsV1::default()).expect("engine builds");
    let wasm = std::fs::read(component_wasm()).expect("the component reads");
    let (source, _) = shipped_manifest();
    LoadedComponentV1::load_embedded(
        &runtime,
        &source,
        &wasm,
        &host_range::host_range(),
        NoSecretsV1,
    )
    .expect("the official package passes every load gate")
}

/// Host feedback SF12 (host-zero-vendor-boundary §13.6): a package declaring several families is
/// loaded once and every family's seam shares that one instance, instead of instantiating the
/// package once per family.
#[test]
fn one_loaded_package_serves_every_family_it_declares() {
    let loaded = std::sync::Arc::new(loaded());
    let seams: Vec<SandboxedComponentV1> = shipped_manifest()
        .1
        .providers
        .iter()
        .map(|_| SandboxedComponentV1::shared(std::sync::Arc::clone(&loaded)))
        .collect();
    assert_eq!(
        seams.len(),
        4,
        "openai-compatible, azure-openai-v1, github-copilot and gemini-openai-compatible"
    );
    assert_eq!(std::sync::Arc::strong_count(&loaded), 5, "one instance, four seams");
    for seam in &seams {
        assert_eq!(seam.metadata(), loaded.metadata());
    }
}

/// Gate ② inside the sandbox: the run that proves "the sandboxed output
/// equals the native output" — the suite's expectations were frozen against
/// the native reference, so a pass here is byte-identity per case.
#[test]
fn the_sandboxed_component_passes_gate_two_byte_for_byte() {
    let pack = FixturePackV1::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures"))
        .expect("the shipped fixture pack loads");
    let evidence = gate2_report::Evidence::capture("provider-openai-compatible", component_wasm());
    let component = sandboxed();

    let report =
        run_provider_component_suite_v1_for_manifest(&component, &pack, &shipped_manifest().1);
    evidence.record(&report);
    for failure in report.failures() {
        eprintln!("{failure}");
    }
    assert!(report.is_passing(), "{report}");
}

/// Gate ① against the shipped package: manifest, identity (native and
/// sandboxed agree, both with the manifest), the range handshake, and the
/// exact tuple handshake.
#[test]
fn the_shipped_package_passes_gate_one_and_the_tuple_handshake() {
    let (_, manifest) = shipped_manifest();
    assert_eq!(accepts_manifest(&manifest), Ok(()));

    let component = sandboxed();
    assert!(reported_identity_matches(&component.metadata(), &manifest));
    assert_eq!(component.metadata(), OpenAiCompatibleReferenceV1.metadata());

    // The range handshake a host linking this release uses admits the package.
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));

    // The exact handshake, still supported for one release. Its true values come from the
    // manifest's own runtime declaration, which may lag this release (§8.6).
    let expectations = host_range::exact_expectations_for(&manifest);
    let off_by_one_digit = HostExpectationsV1 {
        kernel_revision: "72458e3a11fe157f9ac04818c44b62a3dd2cb00c".to_owned(),
        ..expectations.clone()
    };
    // Deliberately one hex digit off first: the handshake must refuse …
    assert!(compatibility_matches(&manifest, &off_by_one_digit).is_err());
    // … and accept the true values.
    assert_eq!(compatibility_matches(&manifest, &expectations), Ok(()));
}

/// An unchanged package keeps its `south_runtime` across releases (§8.6).
///
/// This is the first release after the range handshake in which this package is not re-stamped:
/// the host links a runtime one minor release newer than the one the manifest declares. The range
/// handshake admits the unchanged package and the sandbox loads it; the exact handshake, holding
/// the newer release, refuses it, which is the re-stamp the range removes.
#[test]
fn a_package_that_keeps_an_older_runtime_is_admitted_by_the_range_but_not_the_tuple() {
    let (source, manifest) = shipped_manifest();
    let declared = manifest.compatibility.south_runtime.clone();
    let (major, minor, _) = host_range::triple(&declared);
    // Derived, not a literal: a literal one release ahead is indistinguishable from one that
    // should track the release, and a blanket version bump would collapse it into the declared one.
    let newer = format!("{major}.{}.0", minor + 1);
    let runtime = ComponentRuntimeV1::new(RuntimeLimitsV1::default()).expect("engine builds");
    let wasm = std::fs::read(component_wasm()).expect("the component reads");

    let range = HostRangeV1 { south_runtime: newer.clone(), ..host_range::host_range() };
    assert_eq!(compatibility_admits(&manifest, &range), Ok(()));
    let loaded = LoadedComponentV1::load_embedded(&runtime, &source, &wasm, &range, NoSecretsV1)
        .expect("a newer host admits the unchanged package");
    let component = SandboxedComponentV1::new(loaded);
    assert!(reported_identity_matches(&component.metadata(), &manifest));

    let exact = HostExpectationsV1 {
        south_runtime: newer.clone(),
        ..host_range::exact_expectations_for(&manifest)
    };
    assert_eq!(
        compatibility_matches(&manifest, &exact),
        Err(CompatibilityMismatchV1::SouthRuntime { declared, expected: newer })
    );
    let refused = LoadedComponentV1::load_embedded(&runtime, &source, &wasm, &exact, NoSecretsV1);
    assert!(
        matches!(
            refused,
            Err(LoadErrorV1::Incompatible(CompatibilityMismatchV1::SouthRuntime { .. }))
        ),
        "the exact handshake would have forced a re-stamp"
    );
}
