//! Repository-level checks over the official component packages.
//!
//! Neither fact here is a gate ① or ② property. Gate ① compares a package
//! manifest against the identity a component *reports*, and nothing reports
//! either number below, so without this file they are declarations that no
//! assertion ever reads.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use south_provider_api::{CompatibilityMismatchV2, ComponentManifestV1, compatibility_admits};

#[path = "support/host_range.rs"]
mod host_range;

/// The official components this repository ships. Named, so that an empty or
/// mistyped scan below cannot pass over nothing.
const OFFICIAL_COMPONENTS: [&str; 14] = [
    "provider-anthropic",
    "provider-bedrock-converse",
    "provider-bedrock-converse-bearer",
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

/// Every directory under `components/` carrying a package manifest and its guest crate. Scanned
/// rather than listed, so a component added later is covered on the day it
/// lands instead of the day someone remembers this file.
///
/// A manifest without a guest crate is a package staged ahead of its build — the two embeddings
/// packages, whose gate ② suite and references land before their wasm guests — and is not shipped
/// until its `Cargo.toml` lands, when every check here starts to apply to it.
fn shipped_packages() -> Vec<PathBuf> {
    let mut packages: Vec<PathBuf> = std::fs::read_dir(repo_root().join("components"))
        .expect("the components directory reads")
        .map(|entry| entry.expect("the directory entry reads").path())
        .filter(|path| path.join("manifest.json").is_file() && path.join("Cargo.toml").is_file())
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
/// `south_runtime` is the release a package was last verified with. Under the
/// range handshake (B3, `docs/design/2026-09-30-host-zero-vendor-boundary.md`
/// §8.3, §8.6) a package whose content is unchanged keeps it — re-stamping
/// every package each release is exactly what the range removes — so it may
/// lag the workspace version but never exceed it: a package cannot have been
/// verified with a runtime that does not exist yet.
#[test]
fn every_shipped_package_agrees_with_its_crate_and_names_no_future_release() {
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
        let triple = |version: &str| -> Vec<u64> {
            version.split('.').map(|part| part.parse().expect("a numeric triple")).collect()
        };
        assert!(
            triple(&manifest.compatibility.south_runtime) <= triple(env!("CARGO_PKG_VERSION")),
            "{}: declares south runtime {}, newer than this release",
            manifest.name,
            manifest.compatibility.south_runtime
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

/// S3b (host-zero-vendor-boundary §13.5) adds the `github-copilot` family to
/// `provider-openai-compatible`, and changes the shared conformance and provider-api crates every
/// component links. A same-path rebuild of each package before and after showed a different
/// `component.wasm` for all thirteen (the strings are the same, their layout is not), so every
/// identity published with 0.43.0 retires, or the release's digest-stability check would refuse it.
#[test]
fn s3b_retires_every_published_043_package_identity() {
    for (name, published) in [
        ("provider-openai-compatible", "2.1.5"),
        ("provider-anthropic", "1.0.9"),
        ("provider-gemini", "1.1.5"),
        ("provider-bedrock-converse", "1.0.6"),
        ("task-kling", "1.0.5"),
        ("task-kling-v2", "0.32.3"),
        ("task-minimax-v2", "0.31.2"),
        ("task-bailian-v2", "0.31.2"),
        ("task-xai-v2", "0.35.2"),
        ("task-byteplus-v2", "0.36.2"),
        ("task-veo-v2", "0.35.2"),
        ("task-wan-image-v2", "0.35.2"),
        ("task-gmi-image-v2", "0.35.2"),
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

/// B3 (host-zero-vendor-boundary §8.3): every shipped manifest declares the runtime ABI epoch and
/// the kernel contract numbers this release distributes (`compatibility.json`), and every task
/// package the task contract it speaks, so a host can admit it by range instead of exact tuple.
#[test]
fn every_shipped_package_declares_the_range_handshake() {
    let compatibility: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("compatibility.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(compatibility["runtime_abi"], south_provider_api::RUNTIME_ABI);
    let kernel_contracts: std::collections::BTreeMap<String, u32> =
        serde_json::from_value(compatibility["kernel_contracts"].clone()).unwrap();
    let mut seen = 0;
    for package in shipped_packages() {
        let manifest: ComponentManifestV1 =
            serde_json::from_str(&std::fs::read_to_string(package.join("manifest.json")).unwrap())
                .unwrap();
        let declared = &manifest.compatibility;
        assert_eq!(
            declared.runtime_abi,
            Some(south_provider_api::RUNTIME_ABI),
            "{}",
            manifest.name
        );
        assert_eq!(declared.kernel_contracts, kernel_contracts, "{}", manifest.name);
        let expected_contracts: std::collections::BTreeMap<String, u32> =
            if manifest.api_version.starts_with("task") {
                [("task".to_owned(), u32::from(south_contracts::TASK_CONTRACT_VERSION))].into()
            } else {
                std::collections::BTreeMap::new()
            };
        assert_eq!(declared.contracts, expected_contracts, "{}", manifest.name);
        seen += 1;
    }
    assert_eq!(seen, OFFICIAL_COMPONENTS.len());
}

/// Declaring the range handshake changes every task manifest, so the task identities published
/// with 0.42.0 retire; the four provider packages already moved in B1 and B2 and are unreleased.
#[test]
fn the_range_handshake_retires_the_published_task_identities() {
    for (name, published) in [
        ("task-kling", "1.0.4"),
        ("task-kling-v2", "0.32.1"),
        ("task-minimax-v2", "0.31.1"),
        ("task-bailian-v2", "0.31.1"),
        ("task-xai-v2", "0.35.1"),
        ("task-byteplus-v2", "0.36.1"),
        ("task-veo-v2", "0.35.1"),
        ("task-wan-image-v2", "0.35.1"),
        ("task-gmi-image-v2", "0.35.1"),
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

/// Declaring Kling's credential recipe (B4, host-zero-vendor-boundary §3.3) changes the
/// `task-kling-v2` manifest, so its pending 0.32.2 identity (the B3 range handshake) retires with
/// it.
#[test]
fn the_kling_credential_recipe_retires_the_pending_task_kling_v2_identity() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root().join("components").join("task-kling-v2").join("manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(manifest.credentials.is_some(), "task-kling-v2 declares how its bearer is minted");
    assert_ne!(manifest.version, "0.32.2", "a changed package cannot reuse its identity");
}

/// Host feedback SF13 and SF14 (host-zero-vendor-boundary §13.6): Converse declares the fields
/// its signature reads and sends the native arm's `accept` and `x-amzn-bedrock-accept`, which
/// changes every request it builds, so the identity published with 0.44.0 retires.
#[test]
fn the_converse_credential_fields_and_headers_retire_the_published_044_identity() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root().join("components/provider-bedrock-converse/manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(manifest.credentials.is_some(), "Converse declares its signing credential fields");
    assert_ne!(
        manifest.version, "1.0.7",
        "provider-bedrock-converse reused its published identity"
    );
}

/// The shared conformance and provider-api crates changed for host feedback SF12–SF17
/// (host-zero-vendor-boundary §13.6), and a same-path rebuild of every package before and after
/// showed a different `component.wasm` for all thirteen, so every identity published with 0.44.0
/// retires, or the release's digest-stability check would refuse it. Under the declared-runtime
/// discipline their `south_runtime` stayed 0.44.0 at 0.45.0; the kernel re-pin then raised it to
/// 0.46.0 (see `the_kernel_repin_retires_every_published_045_package_identity`).
#[test]
fn host_feedback_sf12_to_sf17_retires_every_published_044_package_identity() {
    for (name, published) in [
        ("provider-openai-compatible", "2.2.0"),
        ("provider-anthropic", "1.0.10"),
        ("provider-gemini", "1.1.6"),
        ("provider-bedrock-converse", "1.0.7"),
        ("task-kling", "1.0.6"),
        ("task-kling-v2", "0.32.4"),
        ("task-minimax-v2", "0.31.3"),
        ("task-bailian-v2", "0.31.3"),
        ("task-xai-v2", "0.35.3"),
        ("task-byteplus-v2", "0.36.3"),
        ("task-veo-v2", "0.35.3"),
        ("task-wan-image-v2", "0.35.3"),
        ("task-gmi-image-v2", "0.35.3"),
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

/// The kernel re-pin to mirror `v0.4.0` (protocol 0.5.0, `canonical_ir` 3; design record
/// `docs/design/2026-10-08-kernel-repin-protocol-0.5.0.md`) changes every manifest's compatibility
/// declaration, so every identity published with 0.45.0 retires. `provider-openai-compatible` had
/// already moved to the unreleased 2.3.0 (#150), which this change does not retire again. Every
/// package now needs `canonical_ir` 3, which no runtime before 0.46.0 records, so under the
/// declared-runtime discipline each declares `south_runtime` 0.46.0.
#[test]
fn the_kernel_repin_retires_every_published_045_package_identity() {
    for (name, published) in [
        ("provider-openai-compatible", "2.2.1"),
        ("provider-anthropic", "1.0.11"),
        ("provider-gemini", "1.1.7"),
        ("provider-bedrock-converse", "1.0.8"),
        ("provider-bedrock-converse-bearer", "1.0.0"),
        ("task-kling", "1.0.7"),
        ("task-kling-v2", "0.32.5"),
        ("task-minimax-v2", "0.31.4"),
        ("task-bailian-v2", "0.31.4"),
        ("task-xai-v2", "0.35.4"),
        ("task-byteplus-v2", "0.36.4"),
        ("task-veo-v2", "0.35.4"),
        ("task-wan-image-v2", "0.35.4"),
        ("task-gmi-image-v2", "0.35.4"),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
        assert_eq!(
            manifest.compatibility.kernel_contracts.get("canonical_ir"),
            Some(&3),
            "{name}: built against protocol 0.5.0"
        );
        assert_eq!(
            manifest.compatibility.ir_schema_id, "token-station-protocol@0.5.0/v0.4.0",
            "{name}"
        );
        assert_eq!(
            manifest.compatibility.south_runtime, "0.46.0",
            "{name}: needs canonical_ir 3, which 0.46.0 is the first runtime to record"
        );
    }
}

/// Every package declares one kernel tuple, and its crate version is the one the workspace pins:
/// the tuple is package content, so a re-pin that forgets one manifest (or the dependency) fails
/// here rather than in a host.
#[test]
fn every_shipped_package_declares_the_kernel_the_workspace_pins() {
    let workspace = std::fs::read_to_string(repo_root().join("Cargo.toml")).unwrap();
    let pin = workspace
        .lines()
        .find(|line| line.starts_with("token-station-protocol = "))
        .expect("the workspace pins the protocol crate");
    let pinned_version = pin.split("version = \"").nth(1).and_then(|rest| rest.split('"').next());
    let pinned_version = pinned_version.expect("the pin names a version");

    let mut tuples = BTreeSet::new();
    for package in shipped_packages() {
        let manifest: ComponentManifestV1 =
            serde_json::from_str(&std::fs::read_to_string(package.join("manifest.json")).unwrap())
                .unwrap();
        let declared = manifest.compatibility;
        assert!(
            declared
                .ir_schema_id
                .starts_with(&format!("token-station-protocol@{pinned_version}/v")),
            "{}: `{}` is not built against the pinned protocol {pinned_version}",
            manifest.name,
            declared.ir_schema_id
        );
        assert_eq!(declared.kernel_revision.len(), 40, "{}", manifest.name);
        tuples.insert((declared.ir_schema_id, declared.kernel_version, declared.kernel_revision));
    }
    assert_eq!(tuples.len(), 1, "every package declares the same kernel tuple: {tuples:?}");
}

/// A host that records the previous `canonical_ir` refuses every shipped package, through the
/// contract number whichever runtime it claims to be; a host that records 3 admits them. This is
/// the one-way flag day of a kernel re-pin (design record section 13.7).
#[test]
fn a_host_recording_the_old_kernel_contract_refuses_every_shipped_package() {
    let this_release = host_range::host_range();
    assert_eq!(this_release.kernel_contracts.get("canonical_ir"), Some(&3));
    let mut previous = this_release.clone();
    previous.kernel_contracts.insert("canonical_ir".to_owned(), 2);
    let mut seen = 0;
    for package in shipped_packages() {
        let manifest: ComponentManifestV1 =
            serde_json::from_str(&std::fs::read_to_string(package.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(compatibility_admits(&manifest, &this_release), Ok(()), "{}", manifest.name);
        assert!(
            matches!(
                compatibility_admits(&manifest, &previous),
                Err(CompatibilityMismatchV2::KernelContract {
                    ref name,
                    declared: Some(3),
                    expected: Some(2),
                }) if name == "canonical_ir"
            ),
            "{}",
            manifest.name
        );
        seen += 1;
    }
    assert_eq!(seen, OFFICIAL_COMPONENTS.len());
}

/// `provider-openai-compatible` 2.3.0 was merged after 0.45.0 (#150) and never released; the
/// `gemini-openai-compatible` family changes its manifest and its `component.wasm`, so it moves on.
#[test]
fn the_gemini_family_moves_the_unreleased_openai_compatible_identity() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root().join("components/provider-openai-compatible/manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_ne!(manifest.version, "2.3.0");
    assert!(manifest.providers.iter().any(|family| family == "gemini-openai-compatible"));
}
