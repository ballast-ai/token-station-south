//! Repository-level checks over the official component packages.
//!
//! Neither fact here is a gate ① or ② property. Gate ① compares a package
//! manifest against the identity a component *reports*, and nothing reports
//! either number below, so without this file they are declarations that no
//! assertion ever reads.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use south_provider_api::ComponentManifestV1;

/// The official components this repository ships. Named, so that an empty or
/// mistyped scan below cannot pass over nothing.
const OFFICIAL_COMPONENTS: [&str; 7] = [
    "provider-anthropic",
    "provider-gemini",
    "provider-openai-compatible",
    "task-kling",
    "task-kling-v2",
    "task-minimax-v2",
    "task-bailian-v2",
];

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

/// Every directory under `components/` carrying a package manifest. Scanned
/// rather than listed, so a component added later is covered on the day it
/// lands instead of the day someone remembers this file.
fn shipped_packages() -> Vec<PathBuf> {
    let mut packages: Vec<PathBuf> = std::fs::read_dir(repo_root().join("components"))
        .expect("the components directory reads")
        .map(|entry| entry.expect("the directory entry reads").path())
        .filter(|path| path.join("manifest.json").is_file())
        .collect();
    packages.sort();
    packages
}

/// The `version` of a crate manifest's `[package]` table.
///
/// Scoped to that table on purpose. A substring search for `version = "x"`
/// over the whole file would also accept the line of a dependency that
/// happens to be pinned at the same number, which is a check that passes for
/// the wrong reason.
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

/// A component's version is written twice — in its crate manifest and in its
/// package manifest — and nothing recomputes one from the other.
///
/// Gate ① cannot close this: it checks the package manifest against the
/// identity the component *reports*, and that identity comes from the
/// reference implementation, never from the crate manifest. So the two files
/// sit on opposite sides of no assertion at all. The 0.11.0 release moved
/// `provider-openai-compatible` to 2.0.0, left its crate at 1.0.0, and shipped
/// one artifact carrying two version numbers through a green run. (That
/// release number is history. It is not the current version and a release bump
/// must not sweep it along — this sentence has been rewritten by a blanket
/// version replacement twice already.)
///
/// The tuple's `south_runtime` is the same gap one field over: the package
/// manifest declares the release it was verified with, and the suites hold
/// that release as literals, so a workspace bump that updates none of them
/// leaves the package a release behind while every assertion still agrees with
/// itself. Pinning it to this crate's own version — the workspace version —
/// makes the bump the machine's job, the way `compatibility.json`'s release
/// version is already pinned.
#[test]
fn every_shipped_package_agrees_with_its_crate_and_names_this_release() {
    let mut seen = BTreeSet::new();
    for package in shipped_packages() {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(package.join("manifest.json"))
                .expect("the package manifest reads"),
        )
        .expect("the package manifest parses");
        let cargo_toml =
            std::fs::read_to_string(package.join("Cargo.toml")).expect("the crate manifest reads");

        assert_eq!(
            package_version(&cargo_toml),
            manifest.version,
            "{}: the crate version and the package manifest are one number in two files",
            manifest.name
        );
        assert_eq!(
            manifest.compatibility.south_runtime,
            env!("CARGO_PKG_VERSION"),
            "{}: the package manifest must declare the release it ships in",
            manifest.name
        );
        seen.insert(manifest.name);
    }

    assert_eq!(
        seen,
        OFFICIAL_COMPONENTS.iter().map(|name| (*name).to_owned()).collect::<BTreeSet<_>>(),
        "the scan must cover every official component package"
    );
}

/// Every official component is **built and packaged by the release workflow**.
///
/// The guard above pins each package's version against its crate. It cannot
/// see whether a release would actually ship the package — and that gap let
/// `task-kling` be added, registered, tested, and then published in v0.28.0
/// as a release that did not contain it. Nothing was red: the workflow names
/// its components in two hardcoded lists, and a name absent from both is
/// simply never built.
///
/// So this asserts the lists and the directory agree. A fifth component
/// breaks this test until its build script and its `package` line exist,
/// which is the only moment anyone is looking.
#[test]
fn the_release_workflow_ships_every_official_component() {
    let workflow = std::fs::read_to_string(repo_root().join(".github/workflows/release.yml"))
        .expect("the release workflow reads");

    for component in OFFICIAL_COMPONENTS {
        assert!(
            workflow.contains(&format!("package {component} ")),
            "`{component}` has no `package` line in release.yml, so a release would not \
             contain it — the failure mode v0.28.0 shipped with"
        );
    }

    // The build half, keyed off each package's own build script rather than a
    // second list to keep in step with this one.
    let scripts = repo_root().join("scripts");
    let mut built = 0;
    for entry in std::fs::read_dir(&scripts).expect("scripts/ reads") {
        let path = entry.expect("dir entry").path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with("build-") && name.ends_with("-component.sh") {
            assert!(
                workflow.contains(&format!("scripts/{name}")),
                "`{name}` builds a component the release workflow never runs"
            );
            built += 1;
        }
    }
    assert_eq!(
        built,
        OFFICIAL_COMPONENTS.len(),
        "every official component needs exactly one build script, and every build script \
         needs an official component"
    );
}

/// Rebuilt packages must not reuse the immutable identities published in 0.28.1.
#[test]
fn rebuilt_packages_retire_the_previous_release_identities() {
    let previous = [
        ("provider-openai-compatible", "2.1.0"),
        ("provider-anthropic", "1.0.1"),
        ("provider-gemini", "1.1.0"),
        ("task-kling", "1.0.0"),
        ("task-kling-v2", "0.28.1"),
    ];
    for (name, old_version) in previous {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, old_version, "{name} reused its prior content identity");
    }
}

/// Library suites and narrow host probes do not establish production task adoption.
#[test]
fn task_worlds_have_independent_unverified_host_adoption_records() {
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("compatibility.json")).unwrap(),
    )
    .unwrap();
    let hosts =
        manifest["task_component_capabilities"].as_object().expect("task host capability table");
    assert_eq!(hosts.len(), 2);
    for host in ["token-station", "token-station-server"] {
        let capabilities = hosts[host].as_object().unwrap();
        assert_eq!(capabilities.len(), 2);
        for world in ["task_v1", "task_v2"] {
            assert_eq!(capabilities[world]["status"], "not_verified");
            assert!(capabilities[world].get("cases").is_none());
        }
    }
    for (key, suite) in [
        ("task_component_v1", "south.task-component.v1"),
        ("task_component_v2", "south.task-component.v2"),
    ] {
        assert_eq!(manifest["conformance"][format!("{key}_suite_id")], suite);
        assert_eq!(manifest["conformance"][format!("{key}_suite")], 1);
    }
}

/// `FileId` introduces a new runtime capability; no 0.29.0 content identity is reused.
#[test]
fn file_id_release_retires_the_029_runtime_and_component_identities() {
    let previous = [
        ("provider-openai-compatible", "2.1.1"),
        ("provider-anthropic", "1.0.2"),
        ("provider-gemini", "1.1.1"),
        ("task-kling", "1.0.1"),
        ("task-kling-v2", "0.29.0"),
    ];
    for (name, version) in previous {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, version);
        assert_ne!(manifest.compatibility.south_runtime, "0.29.0");
    }
}

/// A seventh package requires a new release; published six-package identities stay immutable.
#[test]
fn bailian_release_retires_the_published_030_component_identities() {
    for (name, published) in [
        ("provider-openai-compatible", "2.1.2"),
        ("provider-anthropic", "1.0.3"),
        ("provider-gemini", "1.1.2"),
        ("task-kling", "1.0.2"),
        ("task-kling-v2", "0.30.0"),
        ("task-minimax-v2", "0.30.0"),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(
            manifest.version, published,
            "a changed package cannot reuse its published identity"
        );
        assert_ne!(manifest.compatibility.south_runtime, "0.30.0");
    }
}
