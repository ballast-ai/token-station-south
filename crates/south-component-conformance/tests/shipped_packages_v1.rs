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
const OFFICIAL_COMPONENTS: [&str; 13] = [
    "provider-anthropic",
    "provider-bedrock-converse",
    "provider-gemini",
    "provider-openai-compatible",
    "task-kling",
    "task-kling-v2",
    "task-minimax-v2",
    "task-bailian-v2",
    "task-xai-v2",
    "task-byteplus-v2",
    "task-veo-v2",
    "task-wan-image-v2",
    "task-gmi-image-v2",
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

/// Declaring immutable body paths changes what `task-kling-v2` prepares (task contract 6: the
/// host may now inject request extras outside those paths), so the published 0.31.0 identity
/// retires with it.
#[test]
fn kling_immutable_paths_retire_the_published_task_kling_v2_identity() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root().join("components").join("task-kling-v2").join("manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_ne!(manifest.version, "0.31.0", "a changed package cannot reuse its published identity");
}

/// Reporting the upstream's last frame as a `last_frame` artifact (task contract 7) changes what
/// `task-byteplus-v2` observes and renders, so the published 0.35.0 identity retires with it.
#[test]
fn byteplus_last_frame_retires_the_published_task_byteplus_v2_identity() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root().join("components").join("task-byteplus-v2").join("manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_ne!(manifest.version, "0.35.0", "a changed package cannot reuse its published identity");
}

/// The protocol-0.4 / kernel-0.3 compatibility tuple changes every package's bytes, so the
/// immutable package identities published with runtime 0.38.0 must all retire together.
#[test]
fn reasoning_replay_release_retires_every_published_038_package_identity() {
    for (name, published) in [
        ("provider-openai-compatible", "2.1.3"),
        ("provider-anthropic", "1.0.4"),
        ("provider-gemini", "1.1.3"),
        ("provider-bedrock-converse", "1.0.1"),
        ("task-kling", "1.0.3"),
        ("task-kling-v2", "0.32.0"),
        ("task-minimax-v2", "0.31.0"),
        ("task-bailian-v2", "0.31.0"),
        ("task-xai-v2", "0.35.0"),
        ("task-byteplus-v2", "0.36.0"),
        ("task-veo-v2", "0.35.0"),
        ("task-wan-image-v2", "0.35.0"),
        ("task-gmi-image-v2", "0.35.0"),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
        // Later runtimes redeclare `south_runtime`; what this release pinned is that
        // no package still declares the runtime it retired.
        assert_ne!(manifest.compatibility.south_runtime, "0.38.0");
    }
}

/// Reporting the whole prompt as the IR's `input_tokens` (the cache buckets partition it, kernel
/// `Usage::total`) changes what both packages parse, and the Converse package stops refusing a
/// `totalTokens` that counts the cache buckets, so the identities published with 0.39.0 retire.
#[test]
fn usage_partition_retires_the_published_anthropic_and_converse_identities() {
    for (name, published) in
        [("provider-anthropic", "1.0.5"), ("provider-bedrock-converse", "1.0.2")]
    {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
    }
}

/// The per-model Claude dialect (#120) changes the request both packages build for a model
/// whose host declares `anthropic.*` words, so the identities published with 0.40.0 retire.
#[test]
fn claude_dialect_retires_the_published_040_anthropic_and_converse_identities() {
    for (name, published) in
        [("provider-anthropic", "1.0.6"), ("provider-bedrock-converse", "1.0.3")]
    {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
    }
}

/// Exclusive sampling (P19 DP6) changes the request both packages build for a model whose
/// host declares `anthropic.sampling.exclusive`, so the identities published with 0.41.0 retire.
#[test]
fn exclusive_sampling_retires_the_published_041_anthropic_and_converse_identities() {
    for (name, published) in
        [("provider-anthropic", "1.0.7"), ("provider-bedrock-converse", "1.0.4")]
    {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
    }
}

/// B1 makes three references strict about usage (a missing or inconsistent report is a protocol
/// error, never a zero) and Gemini's output counts its thoughts, which changes what the three
/// packages parse; the Converse reference was already strict and is unchanged.
#[test]
fn usage_strictness_retires_the_three_published_usage_lenient_identities() {
    for (name, published) in [
        ("provider-openai-compatible", "2.1.4"),
        ("provider-anthropic", "1.0.8"),
        ("provider-gemini", "1.1.4"),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
    }
}

/// B2 seals requests against declared locations: the Gemini and Converse manifests declare their
/// `request_facts`, and both references now encode the model as one URL segment, which changes
/// what the Converse package builds for a model such as an inference-profile ARN.
#[test]
fn request_facts_retire_the_published_converse_identity() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root().join("components/provider-bedrock-converse/manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_ne!(
        manifest.version, "1.0.5",
        "provider-bedrock-converse reused its published identity"
    );
}
