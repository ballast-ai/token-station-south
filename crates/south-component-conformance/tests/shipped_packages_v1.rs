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
const OFFICIAL_COMPONENTS: [&str; 40] = [
    "embeddings-gemini",
    "embeddings-openai-compatible",
    "embeddings-vertex",
    "image-azure",
    "image-gemini",
    "image-ideogram",
    "image-minimax",
    "image-qwen",
    "image-reve",
    "image-stability",
    "image-vertex",
    "image-xai",
    "provider-anthropic",
    "provider-anthropic-bedrock-invoke",
    "provider-bedrock-converse",
    "provider-bedrock-converse-bearer",
    "provider-gemini",
    "provider-kiro",
    "provider-openai-compatible",
    "provider-openai-responses",
    "provider-vertex",
    "speech-azure",
    "speech-elevenlabs",
    "speech-groq",
    "speech-mimo",
    "speech-minimax",
    "speech-openai-compatible",
    "speech-qwen",
    "speech-vertex",
    "speech-xai",
    "task-kling",
    "task-kling-v2",
    "task-minimax-v2",
    "task-bailian-v2",
    "task-xai-v2",
    "task-byteplus-v2",
    "task-veo-v2",
    "task-wan-image-v2",
    "task-gmi-image-v2",
    "task-gemini-omni-v2",
];

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

/// Every directory under `components/` carrying a package manifest. Scanned
/// rather than listed, so a component added later is covered on the day it
/// lands instead of the day someone remembers this file. A manifest without its
/// guest crate is not a shipped package, and the first check below fails on it.
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
            triple(&manifest.compatibility.south_runtime) <= triple(host_range::south_release()),
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
/// The crates a component may link from this workspace, each with a version of its own
/// (host-zero-vendor-boundary §16 Q47).
///
/// A workspace-versioned crate's version moves with every South release, and a crate's version
/// enters the bytes of every component that links it: when this list was written, bumping the
/// workspace version alone changed all seventeen `component.wasm` files. So a component may link
/// only crates whose version moves when they change, and a release that changes none of them keeps
/// every package identity.
const GUEST_LINKED_CRATES: [&str; 3] =
    ["south-component-conformance", "south-contracts", "south-provider-api"];

/// Workspace crates that exist for hosts only: no component may link them, by any source.
///
/// `south-host-grammars` holds grammars only hosts call (`decode_sse_v1`). Putting such a grammar
/// in a component-linked crate would re-identify every package for code no package runs (boundary
/// record §13.13: an unused function changed 3 of 19 `component.wasm`, the Q47 bump all 19).
const HOST_ONLY_CRATES: [&str; 1] = ["south-host-grammars"];

/// The `[[package]]` entries of a lockfile that come from a path (no `source`), as
/// `(name, version)`.
fn locked_path_packages(lockfile: &str) -> Vec<(String, String)> {
    lockfile
        .split("[[package]]")
        .skip(1)
        .filter(|entry| !entry.lines().any(|line| line.starts_with("source = ")))
        .map(|entry| {
            let field = |key: &str| {
                entry
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix(key)?.strip_prefix(" = \"")?.strip_suffix('"')
                    })
                    .unwrap_or_else(|| panic!("a lockfile entry has no `{key}`"))
                    .to_owned()
            };
            (field("name"), field("version"))
        })
        .collect()
}

/// Every component links workspace crates only from [`GUEST_LINKED_CRATES`], at the version each
/// declares, and none of those crates takes the workspace version.
///
/// The first half keeps the Q47 property from eroding unseen: a component that starts linking a
/// workspace-versioned crate (the runtime, the north codec) would be re-identified by every release
/// again, and nothing else would say so until a host re-pinned. The second half keeps each
/// committed lockfile honest: the component builds are not `--locked`, so a stale entry would be
/// rewritten on the build machine and the committed file would describe other bytes.
#[test]
fn components_link_only_independently_versioned_workspace_crates() {
    let crates = repo_root().join("crates");
    for name in GUEST_LINKED_CRATES {
        let cargo_toml = std::fs::read_to_string(crates.join(name).join("Cargo.toml"))
            .expect("the crate manifest reads");
        assert!(
            !cargo_toml.lines().any(|line| line.trim() == "version.workspace = true"),
            "{name}: a component links this crate, so it carries its own version (Q47)"
        );
    }

    for package in shipped_packages() {
        let lockfile = std::fs::read_to_string(package.join("Cargo.lock"))
            .expect("every component commits its lockfile");
        let component = package.file_name().and_then(|name| name.to_str()).expect("a UTF-8 name");
        for (name, version) in locked_path_packages(&lockfile) {
            if name == component {
                continue;
            }
            assert!(
                GUEST_LINKED_CRATES.contains(&name.as_str()),
                "{}: links the workspace crate {name}, whose version is not its own (Q47)",
                package.display()
            );
            let declared = std::fs::read_to_string(crates.join(&name).join("Cargo.toml"))
                .expect("the crate manifest reads");
            assert_eq!(
                version,
                package_version(&declared),
                "{}: the lockfile records {name} {version}, the crate declares another version",
                package.display()
            );
        }
    }
}

/// No component lockfile names a host-only crate, whatever its source (path, registry or git), and
/// each host-only crate keeps the workspace version, so it is never mistaken for a guest-linked one.
///
/// The test above already refuses a path dependency on any crate outside
/// [`GUEST_LINKED_CRATES`]; this one also covers a component that took the crate by git tag, and
/// says why the crate must stay out.
#[test]
fn no_component_links_a_host_only_crate() {
    let crates = repo_root().join("crates");
    for name in HOST_ONLY_CRATES {
        let cargo_toml = std::fs::read_to_string(crates.join(name).join("Cargo.toml"))
            .expect("the host-only crate manifest reads");
        assert!(
            cargo_toml.lines().any(|line| line.trim() == "version.workspace = true"),
            "{name}: a host-only crate carries the workspace version"
        );
        assert!(!GUEST_LINKED_CRATES.contains(&name), "{name} is listed as both");
    }

    let shipped = shipped_packages();
    assert!(!shipped.is_empty(), "no component packages found");
    for package in shipped {
        let lockfile = std::fs::read_to_string(package.join("Cargo.lock"))
            .expect("every component commits its lockfile");
        for name in HOST_ONLY_CRATES {
            assert!(
                !lockfile.lines().any(|line| line.trim() == format!("name = \"{name}\"")),
                "{}: links the host-only crate {name}; only hosts may take it",
                package.display()
            );
        }
    }
}

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

/// A gate ② pass of the two embeddings packages does not establish host adoption of the world:
/// both hosts stay `not_verified` until each runs the world in production (the embeddings record
/// §14 E-Q5 asks the community host for a synchronous implementation).
#[test]
fn the_embeddings_world_has_independent_unverified_host_adoption_records() {
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("compatibility.json")).unwrap(),
    )
    .unwrap();
    let hosts = manifest["embeddings_component_capabilities"]
        .as_object()
        .expect("embeddings host capability table");
    assert_eq!(hosts.len(), 2);
    for host in ["token-station", "token-station-server"] {
        let capabilities = hosts[host].as_object().unwrap();
        assert_eq!(capabilities.len(), 1);
        assert_eq!(capabilities["embeddings_v1"]["status"], "not_verified");
        assert!(capabilities["embeddings_v1"].get("cases").is_none());
    }
    assert_eq!(
        manifest["conformance"]["embeddings_component_v1_suite_id"],
        "south.embeddings-component.v1"
    );
    assert_eq!(manifest["conformance"]["embeddings_component_v1_suite"], 1);
    // The newest contract the release decodes; the shipped contract 1 packages declare 1 and the
    // host range lists both (`host_range`).
    assert_eq!(
        manifest["contracts"]["embeddings"],
        u32::from(south_contracts::EMBEDDINGS_CONTRACT_VERSION_V2)
    );
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
        let expected_contracts: std::collections::BTreeMap<String, u32> = if manifest.name
            == "task-gemini-omni-v2"
        {
            // The one task package on contract 8: inline artifacts are named by pointers into
            // a view elided with the media vocabulary (the Omni record §13).
            [
                ("task".to_owned(), u32::from(south_contracts::TASK_CONTRACT_VERSION)),
                ("media".to_owned(), u32::from(south_contracts::media::MEDIA_CONTRACT_VERSION)),
            ]
            .into()
        } else if manifest.api_version.starts_with("task") {
            // Contract 8 is additive, so every other task package stays at the contract 7 it
            // was published with.
            [("task".to_owned(), u32::from(south_contracts::TASK_CONTRACT_VERSION_V7))].into()
        } else if manifest.api_version == south_provider_api::EMBEDDINGS_WORLD {
            // Only a package that accepts media speaks contract 2 (embeddings record §17.4);
            // the others stay at the contract 1 they were published with.
            let contract = if manifest.capabilities.contains("media") {
                south_contracts::EMBEDDINGS_CONTRACT_VERSION_V2
            } else {
                south_contracts::EMBEDDINGS_CONTRACT_VERSION
            };
            [("embeddings".to_owned(), u32::from(contract))].into()
        } else if manifest.api_version == south_provider_api::IMAGE_WORLD {
            // The image world speaks the shared media vocabulary and its own contract
            // (image record §15). Only the two packages over the shared Gemini image module,
            // `image-gemini` and `image-vertex`, name a contract 2 transform word
            // (`data_url_payload`, I3-2 GI-Q1); the others stay at the media 1 they were
            // published with.
            let media = if matches!(manifest.name.as_str(), "image-gemini" | "image-vertex") {
                south_contracts::media::MEDIA_CONTRACT_VERSION_V2
            } else {
                south_contracts::media::MEDIA_CONTRACT_VERSION
            };
            [
                ("media".to_owned(), u32::from(media)),
                ("image".to_owned(), u32::from(south_contracts::image::IMAGE_CONTRACT_VERSION)),
            ]
            .into()
        } else if manifest.api_version == south_provider_api::SPEECH_WORLD {
            // The speech world speaks the shared media vocabulary and its own contract, which
            // `compatibility.json` records from step S-S-5 (speech record §14, §18).
            // `speech-elevenlabs` carries a declared query parameter, which needs media 3
            // (V3-1 record §6, SV-Q5); the others declare media 1.
            let media = if manifest.name == "speech-elevenlabs" {
                south_contracts::media::MEDIA_CONTRACT_VERSION_V3
            } else {
                south_contracts::media::MEDIA_CONTRACT_VERSION
            };
            [
                ("media".to_owned(), u32::from(media)),
                ("speech".to_owned(), u32::from(south_contracts::speech::SPEECH_CONTRACT_VERSION)),
            ]
            .into()
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
/// The `south_runtime` each package declared when this test was written is no longer pinned: KC1
/// raised every package to 0.57.0 (`kc1_retires_every_r3_package_identity`).
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
        // The kernel tuple itself moved on with the KC1 re-pin (protocol 0.6.0, `canonical_ir` 4);
        // `kc1_retires_every_r3_package_identity` pins the current one.
    }
}

/// The embeddings world added modules to `south-contracts`, `south-provider-api` and
/// `south-component-conformance`, which every guest links, and a same-path rebuild of every package
/// before and after showed a different `component.wasm` for all fourteen, so every identity
/// published with 0.46.0 retires, or the release's digest-stability check would refuse it. Their
/// `south_runtime` stays 0.46.0: under the declared-runtime discipline it names the oldest runtime
/// a package needs, and none of them needs anything newer.
/// The `south_runtime` each package declared when this test was written is no longer pinned: KC1
/// raised every package to 0.57.0 (`kc1_retires_every_r3_package_identity`).
#[test]
fn the_embeddings_world_retires_every_published_046_package_identity() {
    for (name, published) in [
        ("provider-openai-compatible", "2.4.0"),
        ("provider-anthropic", "1.0.12"),
        ("provider-gemini", "1.1.8"),
        ("provider-bedrock-converse", "1.0.9"),
        ("provider-bedrock-converse-bearer", "1.0.1"),
        ("task-kling", "1.0.8"),
        ("task-kling-v2", "0.32.6"),
        ("task-minimax-v2", "0.31.5"),
        ("task-bailian-v2", "0.31.5"),
        ("task-xai-v2", "0.35.5"),
        ("task-byteplus-v2", "0.36.5"),
        ("task-veo-v2", "0.35.5"),
        ("task-wan-image-v2", "0.35.5"),
        ("task-gmi-image-v2", "0.35.5"),
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

/// The embeddings value channel and `embeddings-vertex` changed `south-provider-api` and
/// `south-component-conformance`, which every guest links, and a same-path rebuild at `v0.47.0` and
/// on the components commit (workspace still 0.47.0) gave a different `component.wasm` for all
/// sixteen packages published with 0.47.0, so each identity retires, or the release's
/// digest-stability check would refuse it. Each keeps its `south_runtime`: under the
/// declared-runtime discipline it names the oldest runtime a package needs, and none of them needs
/// anything newer than it did.
/// The `south_runtime` each package declared when this test was written is no longer pinned: KC1
/// raised every package to 0.57.0 (`kc1_retires_every_r3_package_identity`).
#[test]
fn the_embeddings_value_channel_retires_every_published_047_package_identity() {
    for (name, published) in [
        ("provider-openai-compatible", "2.4.1"),
        ("provider-anthropic", "1.0.13"),
        ("provider-gemini", "1.1.9"),
        ("provider-bedrock-converse", "1.0.10"),
        ("provider-bedrock-converse-bearer", "1.0.2"),
        ("task-kling", "1.0.9"),
        ("task-kling-v2", "0.32.7"),
        ("task-minimax-v2", "0.31.6"),
        ("task-bailian-v2", "0.31.6"),
        ("task-xai-v2", "0.35.6"),
        ("task-byteplus-v2", "0.36.6"),
        ("task-veo-v2", "0.35.6"),
        ("task-wan-image-v2", "0.35.6"),
        ("task-gmi-image-v2", "0.35.6"),
        ("embeddings-openai-compatible", "1.0.0"),
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

/// SF27 (host feedback: a Gemini stream's usage comes from its terminal chunk) changed
/// `south-component-conformance`, which every guest links, and a same-path rebuild at `v0.48.0` and
/// on the SF27 commit (workspace still 0.48.0) gave a different `component.wasm` for all seventeen
/// packages published with 0.48.0, so each identity retires, or the release's digest-stability
/// check would refuse it. Each keeps its `south_runtime`: under the declared-runtime discipline it
/// names the oldest runtime a package needs, and none of them needs anything newer than it did.
/// The `south_runtime` each package declared when this test was written is no longer pinned: KC1
/// raised every package to 0.57.0 (`kc1_retires_every_r3_package_identity`).
#[test]
fn host_feedback_sf27_retires_every_published_048_package_identity() {
    for (name, published) in [
        ("provider-openai-compatible", "2.4.2"),
        ("provider-anthropic", "1.0.14"),
        ("provider-gemini", "1.1.10"),
        ("provider-bedrock-converse", "1.0.11"),
        ("provider-bedrock-converse-bearer", "1.0.3"),
        ("task-kling", "1.0.10"),
        ("task-kling-v2", "0.32.8"),
        ("task-minimax-v2", "0.31.7"),
        ("task-bailian-v2", "0.31.7"),
        ("task-xai-v2", "0.35.7"),
        ("task-byteplus-v2", "0.36.7"),
        ("task-veo-v2", "0.35.7"),
        ("task-wan-image-v2", "0.35.7"),
        ("task-gmi-image-v2", "0.35.7"),
        ("embeddings-openai-compatible", "1.0.1"),
        ("embeddings-vertex", "1.0.0"),
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

/// Embeddings contract 2 changed `south-contracts`, `south-provider-api` and
/// `south-component-conformance`, which every guest links; under the Q47 rule (boundary record
/// §13.12) each of the three took a new version and every `component.wasm` changed, so every
/// identity published with 0.50.0 retires, or the release's digest-stability check would refuse
/// it. The sixteen other packages keep their `south_runtime`: under the declared-runtime
/// discipline it names the oldest runtime a package needs, and none of them needs anything newer
/// than it did. `embeddings-gemini` moved to 1.1.0 and is judged by
/// `embeddings_gemini_retires_its_published_identities_with_contract_2`.
///
/// B6-2 (`docs/design/2026-10-08-bedrock-invoke-anthropic-component.md` §15) changed
/// `south-component-conformance` again before any release, so it rides the same single bump over
/// v0.50.0 rather than a second one: `provider-anthropic` 1.0.16 also carries B6-2's behavior
/// changes (I-Q7, I-Q12), and the other packages' bytes change again under unchanged identities,
/// which no published release has seen.
/// The `south_runtime` each package declared when this test was written is no longer pinned: KC1
/// raised every package to 0.57.0 (`kc1_retires_every_r3_package_identity`).
#[test]
fn embeddings_contract_2_retires_every_published_050_package_identity() {
    for (name, published) in [
        ("provider-openai-compatible", "2.4.3"),
        ("provider-anthropic", "1.0.15"),
        ("provider-gemini", "1.1.11"),
        ("provider-bedrock-converse", "1.0.12"),
        ("provider-bedrock-converse-bearer", "1.0.4"),
        ("task-kling", "1.0.11"),
        ("task-kling-v2", "0.32.9"),
        ("task-minimax-v2", "0.31.8"),
        ("task-bailian-v2", "0.31.8"),
        ("task-byteplus-v2", "0.36.8"),
        ("task-xai-v2", "0.35.8"),
        ("task-veo-v2", "0.35.8"),
        ("task-wan-image-v2", "0.35.8"),
        ("task-gmi-image-v2", "0.35.8"),
        ("embeddings-openai-compatible", "1.0.2"),
        ("embeddings-vertex", "1.0.1"),
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
    // The Gemini package's published identity (1.0.2) is retired by its minor.
    let gemini: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/embeddings-gemini/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    assert_ne!(gemini.version, "1.0.2");
}

/// `provider-anthropic-bedrock-invoke` uses only declarations `provider-bedrock-converse` uses under
/// runtime 0.46.0 (design record §12), so 0.46.0 is the oldest runtime that admits it and the
/// package declares it; `scripts/check-declared-runtime.sh` loads it under that runtime's tag.
/// Since 0.57.0 every package declares 0.57.0, the first runtime that records `canonical_ir` 4
/// (KC1), which still refuses the older runtime below.
#[test]
fn the_invoke_package_declares_the_runtime_converse_declares() {
    let read = |name: &str| -> ComponentManifestV1 {
        serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let invoke = read("provider-anthropic-bedrock-invoke");
    // KC1 made 1.1.0, I3-1's Q47 bump 1.1.1, I3-2's 1.1.2, I3-4's 1.1.3, I3-5's 1.1.4, the Kiro
    // step's 1.1.5, task contract 8's 1.1.6, I3-3's 1.1.7, speech step S-S-1's 1.1.8, speech batch
    // V3-1's 1.1.9, provider-vertex's 1.1.10 and the speech Azure OpenAI families' Q47 bump 1.1.11
    // (`speech_azure_openai_retires_every_published_066_package_identity`).
    assert_eq!(invoke.version, "1.1.11");
    assert_eq!(invoke.compatibility.south_runtime, "0.57.0");
    assert_eq!(invoke.compatibility, read("provider-bedrock-converse").compatibility);
}

/// The image world changed `south-contracts`, `south-provider-api` and
/// `south-component-conformance`, which every guest links (their own versions moved 0.50.0 →
/// 0.51.0, Q47), and a same-path rebuild at `v0.51.1` and on the commit that moved them
/// (workspace still 0.51.1) gave a different `component.wasm` for all eighteen packages published
/// with 0.51.1, so each identity retires, or the release's digest-stability check would refuse it.
/// Each keeps its `south_runtime`: none needs anything newer than it did.
/// The `south_runtime` each package declared when this test was written is no longer pinned: KC1
/// raised every package to 0.57.0 (`kc1_retires_every_r3_package_identity`).
#[test]
fn the_image_world_retires_every_published_0511_package_identity() {
    for (name, published) in [
        ("provider-openai-compatible", "2.4.4"),
        ("provider-anthropic", "1.0.16"),
        ("provider-anthropic-bedrock-invoke", "1.0.0"),
        ("provider-gemini", "1.1.12"),
        ("provider-bedrock-converse", "1.0.13"),
        ("provider-bedrock-converse-bearer", "1.0.5"),
        ("task-kling", "1.0.12"),
        ("task-kling-v2", "0.32.10"),
        ("task-minimax-v2", "0.31.9"),
        ("task-bailian-v2", "0.31.9"),
        ("task-xai-v2", "0.35.9"),
        ("task-byteplus-v2", "0.36.9"),
        ("task-veo-v2", "0.35.9"),
        ("task-wan-image-v2", "0.35.9"),
        ("task-gmi-image-v2", "0.35.9"),
        ("embeddings-openai-compatible", "1.0.3"),
        ("embeddings-gemini", "1.1.0"),
        ("embeddings-vertex", "1.0.2"),
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

/// `image-azure` is the first package of the image world, which the 0.52.0 runtime is the first
/// to know, so that is the runtime it declares (boundary record §13.6).
/// Since 0.57.0 every package declares 0.57.0, the first runtime that records `canonical_ir` 4
/// (KC1), which still refuses the older runtime below.
#[test]
fn the_image_azure_package_declares_the_first_runtime_with_the_world() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/image-azure/manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.compatibility.south_runtime, "0.57.0");
    assert_eq!(manifest.api_version, south_provider_api::IMAGE_WORLD);
}

/// The embeddings world is first known to the 0.47.0 runtime, so that is the oldest runtime that
/// admits the contract 1 text package, and it declares it: a host that claims 0.46.0 refuses it
/// through the declared runtime alone, even with embeddings contract 1 in its range. (The Gemini
/// package declares a later runtime, the one whose gate 1 admits `media`.)
/// Since 0.57.0 every package declares 0.57.0, the first runtime that records `canonical_ir` 4
/// (KC1), which still refuses the older runtime below.
#[test]
fn the_embeddings_packages_declare_the_first_runtime_with_the_world() {
    let mut on_046 = host_range::host_range();
    "0.46.0".clone_into(&mut on_046.south_runtime);
    // `embeddings-gemini` moved on with contract 2 and declares a later runtime.
    let name = "embeddings-openai-compatible";
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components").join(name).join("manifest.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.compatibility.south_runtime, "0.57.0", "{name}");
    assert!(
        matches!(
            compatibility_admits(&manifest, &on_046),
            Err(CompatibilityMismatchV2::SouthRuntimeAboveHost { .. })
        ),
        "{name}"
    );
}

/// `embeddings-gemini` declares the `media` capability and embeddings contract 2, which gate ① first
/// admits in the 0.51.0 runtime (embeddings record §17.4), so that is the oldest runtime that admits
/// it and the package declares it: a host that claims 0.50.0 refuses it through the declared runtime
/// alone, even with contract 2 in its range. Contract 1 hosts refuse it through the contract number.
/// Since 0.57.0 every package declares 0.57.0, the first runtime that records `canonical_ir` 4
/// (KC1), which still refuses the older runtime below.
#[test]
fn the_gemini_package_declares_the_first_runtime_with_media_and_contract_2() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/embeddings-gemini/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.compatibility.south_runtime, "0.57.0");
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
    let mut on_050 = host_range::host_range();
    "0.50.0".clone_into(&mut on_050.south_runtime);
    assert!(matches!(
        compatibility_admits(&manifest, &on_050),
        Err(CompatibilityMismatchV2::SouthRuntimeAboveHost { .. })
    ));
    let mut contract_1_only = host_range::host_range();
    contract_1_only.contracts.insert("embeddings".to_owned(), BTreeSet::from([1]));
    assert!(matches!(
        compatibility_admits(&manifest, &contract_1_only),
        Err(CompatibilityMismatchV2::Contract { declared: 2, .. })
    ));
}

/// `embeddings-vertex` declares a family's `config_schema` and an exported credential attribute,
/// which gate ① first admits in the embeddings world in the 0.48.0 runtime (embeddings record §16),
/// so that is the oldest runtime that admits it and the package declares it: a host that claims
/// 0.47.0 refuses it through the declared runtime alone, even with embeddings contract 1 in its
/// range.
/// Since 0.57.0 every package declares 0.57.0, the first runtime that records `canonical_ir` 4
/// (KC1), which still refuses the older runtime below.
#[test]
fn the_vertex_package_declares_the_first_runtime_with_the_embeddings_value_channel() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root().join("components").join("embeddings-vertex").join("manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.compatibility.south_runtime, "0.57.0");
    let mut on_047 = host_range::host_range();
    "0.47.0".clone_into(&mut on_047.south_runtime);
    assert!(matches!(
        compatibility_admits(&manifest, &on_047),
        Err(CompatibilityMismatchV2::SouthRuntimeAboveHost { .. })
    ));
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
    assert_eq!(this_release.kernel_contracts.get("canonical_ir"), Some(&4));
    let mut previous = this_release.clone();
    previous.kernel_contracts.insert("canonical_ir".to_owned(), 3);
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
                    declared: Some(4),
                    expected: Some(3),
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

/// `embeddings-gemini` left the `south_runtime` those two retirement tests pin for it: it declares
/// `media`, which only a runtime whose gate 1 knows the word admits (embeddings record §17.4), and
/// every identity it published before (1.0.0 with 0.47.0, 1.0.1 with 0.48.0, 1.0.2 with 0.49.0)
/// stays retired.
#[test]
fn embeddings_gemini_retires_its_published_identities_with_contract_2() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/embeddings-gemini/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    for published in ["1.0.0", "1.0.1", "1.0.2"] {
        assert_ne!(manifest.version, published, "embeddings-gemini reused a published identity");
    }
    assert!(manifest.capabilities.contains("media"));
    assert_eq!(manifest.compatibility.contracts.get("embeddings"), Some(&2));
}

/// The `OpenAI` Responses package (record step R1) changed `south-provider-api` (the manifest gains
/// `immutable_body_paths` and `north_passthrough`) and `south-component-conformance` (the
/// reference, the shared vocabulary and the SSE splitter), which every guest links, so both took a
/// new version (0.51.0 → 0.52.0, Q47) and every `component.wasm` changes: each identity published
/// with 0.53.0 retires, or the release's digest-stability check would refuse it. Each keeps its
/// `south_runtime`: none needs anything newer than it did.
/// The `south_runtime` each package declared when this test was written is no longer pinned: KC1
/// raised every package to 0.57.0 (`kc1_retires_every_r3_package_identity`).
#[test]
fn the_responses_package_retires_every_published_053_package_identity() {
    for (name, published) in [
        ("provider-openai-compatible", "2.4.5"),
        ("provider-anthropic", "1.0.17"),
        ("provider-anthropic-bedrock-invoke", "1.0.1"),
        ("provider-gemini", "1.1.13"),
        ("provider-bedrock-converse", "1.0.14"),
        ("provider-bedrock-converse-bearer", "1.0.6"),
        ("task-kling", "1.0.13"),
        ("task-kling-v2", "0.32.11"),
        ("task-minimax-v2", "0.31.10"),
        ("task-bailian-v2", "0.31.10"),
        ("task-xai-v2", "0.35.10"),
        ("task-byteplus-v2", "0.36.10"),
        ("task-veo-v2", "0.35.10"),
        ("task-wan-image-v2", "0.35.10"),
        ("task-gmi-image-v2", "0.35.10"),
        ("embeddings-openai-compatible", "1.0.4"),
        ("embeddings-gemini", "1.1.1"),
        ("embeddings-vertex", "1.0.3"),
        ("image-azure", "1.0.0"),
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

/// `provider-openai-responses` declares `immutable_body_paths` and `north_passthrough`, which no
/// runtime before 0.54.0 admits (the manifest refuses unknown fields), so 0.54.0, the release that
/// ships this change, is the oldest runtime that admits it and the package declares it, as
/// `embeddings-gemini` declared 0.51.0 (#167): a host that claims 0.53.0 refuses it through the
/// declared runtime alone.
/// Since 0.57.0 every package declares 0.57.0, the first runtime that records `canonical_ir` 4
/// (KC1), which still refuses the older runtime below.
#[test]
fn the_responses_package_declares_the_runtime_that_first_admits_its_declarations() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root().join("components/provider-openai-responses/manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    // 1.0.0 shipped in 0.54.0; NC-1 made 1.1.0, R3 1.2.0, KC1 1.3.0, I3-1's Q47 bump 1.3.1,
    // I3-2's 1.3.2, I3-4's 1.3.3, I3-5's 1.3.4, the Kiro step's 1.3.5, task contract 8's 1.3.6, I3-3's 1.3.7, S-S-1's 1.3.8, V3-1's 1.3.9, provider-vertex's 1.3.10 (below) and the `bedrock-mantle` family's 1.4.0 (record `2026-10-15-bedrock-mantle.md` §11, a new family in the same cycle), and the speech Azure OpenAI families' Q47 bump 1.4.1. The Codex `credentials` section uses only forms the
    // 0.54.0 gate ① admits; `subscription_quota_families` (D7, 0.60.0) needs the 0.60.0 runtime.
    assert_eq!(manifest.version, "1.4.1");
    assert_eq!(manifest.compatibility.south_runtime, "0.60.0");
    assert!(!manifest.immutable_body_paths.is_empty() && !manifest.north_passthrough.is_empty());
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
    let mut on_053 = host_range::host_range();
    "0.53.0".clone_into(&mut on_053.south_runtime);
    assert!(matches!(
        compatibility_admits(&manifest, &on_053),
        Err(CompatibilityMismatchV2::SouthRuntimeAboveHost { .. })
    ));
}

/// NC-1 (design record `2026-10-11-responses-reasoning-replay-and-passthrough-suite.md` §5)
/// changed `south-component-conformance` (the `OpenAI` replay validator and vocabulary, the
/// Responses reference, the NP-Q4 refusals, new fixtures), which every guest links, so the crate
/// took a new version (0.52.0 → 0.53.0, Q47) and every `component.wasm` changes: each identity
/// published with 0.54.0 retires. Behaviour changes take a minor version (the Responses package's
/// replay, the two NP-Q4 refusals); every other package takes a patch. None needs a newer runtime:
/// the replay word is data in `supported_parameters`, not a manifest field.
/// The `south_runtime` each package declared when this test was written is no longer pinned: KC1
/// raised every package to 0.57.0 (`kc1_retires_every_r3_package_identity`).
#[test]
fn nc1_retires_every_published_054_package_identity() {
    for (name, published) in [
        ("provider-openai-compatible", "2.4.6"),
        ("provider-anthropic", "1.0.18"),
        ("provider-anthropic-bedrock-invoke", "1.0.2"),
        ("provider-gemini", "1.1.14"),
        ("provider-bedrock-converse", "1.0.15"),
        ("provider-bedrock-converse-bearer", "1.0.7"),
        ("provider-openai-responses", "1.0.0"),
        ("task-kling", "1.0.14"),
        ("task-kling-v2", "0.32.12"),
        ("task-minimax-v2", "0.31.11"),
        ("task-bailian-v2", "0.31.11"),
        ("task-xai-v2", "0.35.11"),
        ("task-byteplus-v2", "0.36.11"),
        ("task-veo-v2", "0.35.11"),
        ("task-wan-image-v2", "0.35.11"),
        ("task-gmi-image-v2", "0.35.11"),
        ("embeddings-openai-compatible", "1.0.5"),
        ("embeddings-gemini", "1.1.2"),
        ("embeddings-vertex", "1.0.4"),
        ("image-azure", "1.0.1"),
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

/// Step R3 (design record `2026-10-12-responses-r3-codex-and-request-fields.md` §6) changed
/// `south-component-conformance` (the Codex family, the four client fields, the stream form of
/// `UsageNeverDefaulted`), which every guest links, so the crate took a new version (0.53.0 →
/// 0.54.0, Q47) and every `component.wasm` changes: each identity published with 0.55.0 retires.
/// The Responses package's new family and body fields take a minor version; every other package a
/// patch. None needs a newer runtime: the Codex section uses only manifest forms 0.54.0 admits.
/// The `south_runtime` each package declared when this test was written is no longer pinned: KC1
/// raised every package to 0.57.0 (`kc1_retires_every_r3_package_identity`).
#[test]
fn r3_retires_every_published_055_package_identity() {
    for (name, published) in [
        ("provider-openai-compatible", "2.5.0"),
        ("provider-anthropic", "1.0.19"),
        ("provider-anthropic-bedrock-invoke", "1.0.3"),
        ("provider-gemini", "1.2.0"),
        ("provider-bedrock-converse", "1.0.16"),
        ("provider-bedrock-converse-bearer", "1.0.8"),
        ("provider-openai-responses", "1.1.0"),
        ("task-kling", "1.0.15"),
        ("task-kling-v2", "0.32.13"),
        ("task-minimax-v2", "0.31.12"),
        ("task-bailian-v2", "0.31.12"),
        ("task-xai-v2", "0.35.12"),
        ("task-byteplus-v2", "0.36.12"),
        ("task-veo-v2", "0.35.12"),
        ("task-wan-image-v2", "0.35.12"),
        ("task-gmi-image-v2", "0.35.12"),
        ("embeddings-openai-compatible", "1.0.6"),
        ("embeddings-gemini", "1.1.3"),
        ("embeddings-vertex", "1.0.5"),
        ("image-azure", "1.0.2"),
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

/// The runtime every speech package declares: 0.65.0, the first release that records
/// `contracts.speech` (S-S-5). The 0.65.0 release commit moved them to it from the interim
/// declaration of this tree's release.
const SPEECH_RUNTIME: &str = "0.65.0";

/// The `south_runtime` a package declares today. Most packages still need only the 0.57.0 runtime
/// (`canonical_ir` 4); the three that declare a manifest field added in 0.60.0
/// (`north_request_passthrough`, `subscription_quota_families`; D7, record
/// `2026-10-14-generate-content-request-passthrough.md`) need the first runtime that parses it, because
/// `ComponentManifestV1` refuses unknown fields; `image-gemini` needs the first runtime with media 2.
fn declared_runtime_now(name: &str) -> &'static str {
    match name {
        // `provider-anthropic` from 1.2.0: the Claude Code families declare
        // `subscription_quota_families`, which no runtime before 0.60.0 parses (Claude Code record
        // §9).
        "provider-anthropic"
        | "provider-gemini"
        | "provider-kiro"
        | "provider-openai-compatible"
        | "provider-openai-responses" => "0.60.0",
        "image-gemini" => "0.59.0",
        // V-Q8 (design record `2026-10-14-task-min-poll-interval.md`): the first package to declare
        // `min_poll_interval_seconds`, which no runtime before 0.63.0 parses (`ComponentManifestV1`
        // refuses unknown fields); 0.63.0 shipped it, so the literal.
        "task-minimax-v2" => "0.63.0",
        _ => "0.57.0",
    }
}

/// KC1 (design record `2026-10-12-kc1-canonical-ir-4-south.md`) re-pins the kernel to mirror
/// `v0.5.0` (protocol 0.6.0, `canonical_ir` 4) and changes `south-provider-api` (the
/// `end_user_hash` host value, 0.52.0 → 0.53.0) and `south-component-conformance` (0.54.0 →
/// 0.55.0, Q47). The tuple is package content and every `component.wasm` changes, so each identity
/// R3 left for 0.56.0 retires. The seven provider packages change behaviour (markers, `top_k`,
/// typed reasoning) and take a minor version; the others a patch. Every package needs
/// `canonical_ir` 4, which no runtime before 0.57.0 records, so under the declared-runtime
/// discipline each declares `south_runtime` 0.57.0 (record §2.4), as the 0.46.0 re-pin did.
/// The identity each package took in KC1 is no longer pinned: I3-1 gave every package one more
/// (`i3_1_retires_every_published_057_package_identity`); the kernel tuple still holds.
#[test]
fn kc1_retires_every_r3_package_identity() {
    for (name, r3) in [
        ("provider-openai-compatible", "2.5.1"),
        ("provider-anthropic", "1.0.20"),
        ("provider-anthropic-bedrock-invoke", "1.0.4"),
        ("provider-gemini", "1.2.1"),
        ("provider-bedrock-converse", "1.0.17"),
        ("provider-bedrock-converse-bearer", "1.0.9"),
        ("provider-openai-responses", "1.2.0"),
        ("task-kling", "1.0.16"),
        ("task-kling-v2", "0.32.14"),
        ("task-minimax-v2", "0.31.13"),
        ("task-bailian-v2", "0.31.13"),
        ("task-xai-v2", "0.35.13"),
        ("task-byteplus-v2", "0.36.13"),
        ("task-veo-v2", "0.35.13"),
        ("task-wan-image-v2", "0.35.13"),
        ("task-gmi-image-v2", "0.35.13"),
        ("embeddings-openai-compatible", "1.0.7"),
        ("embeddings-gemini", "1.1.4"),
        ("embeddings-vertex", "1.0.6"),
        ("image-azure", "1.0.3"),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, r3, "{name} reused its R3 identity");
        assert_eq!(
            manifest.compatibility.kernel_contracts.get("canonical_ir"),
            Some(&4),
            "{name}: built against protocol 0.6.0"
        );
        assert_eq!(
            manifest.compatibility.ir_schema_id, "token-station-protocol@0.6.0/v0.5.0",
            "{name}"
        );
        assert_eq!(manifest.compatibility.kernel_version, "0.5.0", "{name}");
        assert_eq!(
            manifest.compatibility.south_runtime,
            declared_runtime_now(name),
            "{name}: needs canonical_ir 4, which 0.57.0 is the first runtime to record"
        );
        assert_eq!(
            manifest.compatibility.kernel_revision, "c0bca10c2d9f9a439680865feb65c9edf598fc65",
            "{name}: the upstream kernel-v0.6.0 commit the mirror records"
        );
    }
}

/// Image world step I3-1 (design record `2026-10-12-image-i3-1-minimax-and-qwen.md` §7) changes
/// `south-component-conformance` (the `image-minimax` and `image-qwen` references and the Bailian
/// helpers the Wan task package now shares), which every guest links, so the crate takes a new
/// version (0.55.0 → 0.56.0, Q47) and every `component.wasm` changes: each identity published with
/// 0.57.0 retires. No published package changes behaviour, so each takes a patch and keeps its
/// `south_runtime`; the kernel tuple is the one KC1 pinned. The identity each package took in I3-1
/// is no longer pinned: I3-2 gave every package one more
/// (`i3_2_retires_every_published_058_package_identity`).
#[test]
fn i3_1_retires_every_published_057_package_identity() {
    for (name, published, _i3_1) in [
        ("provider-openai-compatible", "2.6.0", "2.6.1"),
        ("provider-anthropic", "1.1.0", "1.1.1"),
        ("provider-anthropic-bedrock-invoke", "1.1.0", "1.1.1"),
        ("provider-gemini", "1.3.0", "1.3.1"),
        ("provider-bedrock-converse", "1.1.0", "1.1.1"),
        ("provider-bedrock-converse-bearer", "1.1.0", "1.1.1"),
        ("provider-openai-responses", "1.3.0", "1.3.1"),
        ("task-kling", "1.0.17", "1.0.18"),
        ("task-kling-v2", "0.32.15", "0.32.16"),
        ("task-minimax-v2", "0.31.14", "0.31.15"),
        ("task-bailian-v2", "0.31.14", "0.31.15"),
        ("task-xai-v2", "0.35.14", "0.35.15"),
        ("task-byteplus-v2", "0.36.14", "0.36.15"),
        ("task-veo-v2", "0.35.14", "0.35.15"),
        ("task-wan-image-v2", "0.35.14", "0.35.15"),
        ("task-gmi-image-v2", "0.35.14", "0.35.15"),
        ("embeddings-openai-compatible", "1.0.8", "1.0.9"),
        ("embeddings-gemini", "1.1.5", "1.1.6"),
        ("embeddings-vertex", "1.0.7", "1.0.8"),
        ("image-azure", "1.0.4", "1.0.5"),
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
            manifest.compatibility.south_runtime,
            declared_runtime_now(name),
            "{name}: no runtime moves in that release"
        );
        assert_eq!(
            manifest.compatibility.ir_schema_id, "token-station-protocol@0.6.0/v0.5.0",
            "{name}"
        );
    }
}

/// Image world step I3-2 (design record `2026-10-13-image-i3-2-gemini.md` §10) adds the transform
/// word `data_url_payload` to `south-contracts` (`contracts.media` 2, 0.52.0 → 0.53.0) and the
/// `image-gemini` reference and the shared Gemini image module to `south-component-conformance`
/// (0.56.0 → 0.57.0), which every guest links, so every `component.wasm` changes: each identity
/// published with 0.58.0 retires. Each takes a patch and keeps its `south_runtime` and its
/// `contracts` (a package declaring `media` 1 stays at 1). No published package changes behaviour
/// except `image-minimax` 1.0.1, which also carries the I3-1 decision 1 fix (each `image_url`
/// array string its own character reference, as native). The identity each package took in I3-2
/// is no longer pinned: I3-4 gave every package one more
/// (`i3_4_retires_every_published_059_package_identity`).
#[test]
fn i3_2_retires_every_published_058_package_identity() {
    for (name, published, _i3_2) in [
        ("provider-openai-compatible", "2.6.1", "2.6.2"),
        ("provider-anthropic", "1.1.1", "1.1.2"),
        ("provider-anthropic-bedrock-invoke", "1.1.1", "1.1.2"),
        ("provider-gemini", "1.3.1", "1.3.2"),
        ("provider-bedrock-converse", "1.1.1", "1.1.2"),
        ("provider-bedrock-converse-bearer", "1.1.1", "1.1.2"),
        ("provider-openai-responses", "1.3.1", "1.3.2"),
        ("task-kling", "1.0.18", "1.0.19"),
        ("task-kling-v2", "0.32.16", "0.32.17"),
        ("task-minimax-v2", "0.31.15", "0.31.16"),
        ("task-bailian-v2", "0.31.15", "0.31.16"),
        ("task-xai-v2", "0.35.15", "0.35.16"),
        ("task-byteplus-v2", "0.36.15", "0.36.16"),
        ("task-veo-v2", "0.35.15", "0.35.16"),
        ("task-wan-image-v2", "0.35.15", "0.35.16"),
        ("task-gmi-image-v2", "0.35.15", "0.35.16"),
        ("embeddings-openai-compatible", "1.0.9", "1.0.10"),
        ("embeddings-gemini", "1.1.6", "1.1.7"),
        ("embeddings-vertex", "1.0.8", "1.0.9"),
        ("image-azure", "1.0.5", "1.0.6"),
        ("image-minimax", "1.0.0", "1.0.1"),
        ("image-qwen", "1.0.0", "1.0.1"),
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
            manifest.compatibility.south_runtime,
            declared_runtime_now(name),
            "{name}: no runtime moves in that release"
        );
        if manifest.api_version == south_provider_api::IMAGE_WORLD {
            assert_eq!(manifest.compatibility.contracts.get("media"), Some(&1), "{name}");
        }
        assert_eq!(
            manifest.compatibility.ir_schema_id, "token-station-protocol@0.6.0/v0.5.0",
            "{name}"
        );
    }
}

/// Image world step I3-4 (design record `2026-10-13-image-i3-4-stability.md` §9) adds the
/// `image-stability` reference to `south-component-conformance` (0.57.0 → 0.58.0), which every guest
/// links, so every `component.wasm` changes: each identity published with 0.59.0 retires. No
/// published package changes behaviour: each takes a patch and keeps its `south_runtime` and its
/// `contracts` (`image-gemini` stays at media 2 and the 0.59.0 runtime, every other image package
/// at media 1). The identity each package took in I3-4 is no longer pinned: I3-5 gave every
/// package one more (`i3_5_retires_every_published_060_package_identity`).
#[test]
fn i3_4_retires_every_published_059_package_identity() {
    for (name, published, _i3_4) in [
        ("provider-openai-compatible", "2.6.2", "2.6.3"),
        ("provider-anthropic", "1.1.2", "1.1.3"),
        ("provider-anthropic-bedrock-invoke", "1.1.2", "1.1.3"),
        ("provider-gemini", "1.3.2", "1.3.3"),
        ("provider-bedrock-converse", "1.1.2", "1.1.3"),
        ("provider-bedrock-converse-bearer", "1.1.2", "1.1.3"),
        ("provider-openai-responses", "1.3.2", "1.3.3"),
        ("task-kling", "1.0.19", "1.0.20"),
        ("task-kling-v2", "0.32.17", "0.32.18"),
        ("task-minimax-v2", "0.31.16", "0.31.17"),
        ("task-bailian-v2", "0.31.16", "0.31.17"),
        ("task-xai-v2", "0.35.16", "0.35.17"),
        ("task-byteplus-v2", "0.36.16", "0.36.17"),
        ("task-veo-v2", "0.35.16", "0.35.17"),
        ("task-wan-image-v2", "0.35.16", "0.35.17"),
        ("task-gmi-image-v2", "0.35.16", "0.35.17"),
        ("embeddings-openai-compatible", "1.0.10", "1.0.11"),
        ("embeddings-gemini", "1.1.7", "1.1.8"),
        ("embeddings-vertex", "1.0.9", "1.0.10"),
        ("image-azure", "1.0.6", "1.0.7"),
        ("image-minimax", "1.0.1", "1.0.2"),
        ("image-qwen", "1.0.1", "1.0.2"),
        ("image-gemini", "1.0.0", "1.0.1"),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
        let media = if name == "image-gemini" { 2 } else { 1 };
        assert_eq!(
            manifest.compatibility.south_runtime,
            declared_runtime_now(name),
            "{name}: I3-4 moves no runtime"
        );
        if manifest.api_version == south_provider_api::IMAGE_WORLD {
            assert_eq!(manifest.compatibility.contracts.get("media"), Some(&media), "{name}");
        }
        assert_eq!(
            manifest.compatibility.ir_schema_id, "token-station-protocol@0.6.0/v0.5.0",
            "{name}"
        );
    }
}

/// Image world step I3-5 (design record `2026-10-13-image-i3-5-ideogram.md` §9) adds the
/// `image-ideogram` reference to `south-component-conformance` (0.58.0 → 0.59.0), which every guest
/// links, so every `component.wasm` changes: each identity published with 0.60.0 retires. No
/// published package changes behaviour: each takes a patch and keeps its `south_runtime` (three
/// provider packages on 0.60.0, `image-gemini` on 0.59.0, the rest on 0.57.0) and its `contracts`
/// (`image-gemini` stays at media 2, every other image package at media 1). The identity each
/// package took in I3-5 is no longer pinned: the Kiro step gave every package one more
/// (`kiro_retires_every_published_061_package_identity`).
#[test]
fn i3_5_retires_every_published_060_package_identity() {
    for (name, published, _i3_5) in [
        ("provider-openai-compatible", "2.6.3", "2.6.4"),
        ("provider-anthropic", "1.1.3", "1.1.4"),
        ("provider-anthropic-bedrock-invoke", "1.1.3", "1.1.4"),
        ("provider-gemini", "1.3.3", "1.3.4"),
        ("provider-bedrock-converse", "1.1.3", "1.1.4"),
        ("provider-bedrock-converse-bearer", "1.1.3", "1.1.4"),
        ("provider-openai-responses", "1.3.3", "1.3.4"),
        ("task-kling", "1.0.20", "1.0.21"),
        ("task-kling-v2", "0.32.18", "0.32.19"),
        ("task-minimax-v2", "0.31.17", "0.31.18"),
        ("task-bailian-v2", "0.31.17", "0.31.18"),
        ("task-xai-v2", "0.35.17", "0.35.18"),
        ("task-byteplus-v2", "0.36.17", "0.36.18"),
        ("task-veo-v2", "0.35.17", "0.35.18"),
        ("task-wan-image-v2", "0.35.17", "0.35.18"),
        ("task-gmi-image-v2", "0.35.17", "0.35.18"),
        ("embeddings-openai-compatible", "1.0.11", "1.0.12"),
        ("embeddings-gemini", "1.1.8", "1.1.9"),
        ("embeddings-vertex", "1.0.10", "1.0.11"),
        ("image-azure", "1.0.7", "1.0.8"),
        ("image-minimax", "1.0.2", "1.0.3"),
        ("image-qwen", "1.0.2", "1.0.3"),
        ("image-gemini", "1.0.1", "1.0.2"),
        ("image-stability", "1.0.0", "1.0.1"),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
        let media = if name == "image-gemini" { 2 } else { 1 };
        assert_eq!(
            manifest.compatibility.south_runtime,
            declared_runtime_now(name),
            "{name}: I3-5 moves no runtime"
        );
        if manifest.api_version == south_provider_api::IMAGE_WORLD {
            assert_eq!(manifest.compatibility.contracts.get("media"), Some(&media), "{name}");
        }
        assert_eq!(
            manifest.compatibility.ir_schema_id, "token-station-protocol@0.6.0/v0.5.0",
            "{name}"
        );
    }
}

/// `image-gemini` (I3-2) started at 1.0.0 in the image world (I3-4's Q47 bump made it 1.0.1,
/// I3-5's 1.0.2 and the Kiro step's 1.0.3), family `gemini-image`, no config keys.
/// It names the transform word `data_url_payload`, so it declares `contracts.media` 2: a host whose
/// range lists media 1 only refuses it through the contract number. Its `south_runtime` must be the
/// first runtime that decodes media 2, the release that ships the word: 0.59.0 (the release commit
/// moved the declaration with the workspace version, as 0.51.0 did for `embeddings-gemini` 1.1.0).
#[test]
fn image_gemini_declares_media_2_which_a_media_1_host_refuses() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/image-gemini/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    // 1.0.3 after the Kiro step; task contract 8's Q47 bump made it 1.0.4, I3-3's 1.0.5, S-S-1's
    // 1.0.6, V3-1's 1.0.7, provider-vertex's 1.0.8 and the speech Azure OpenAI families' Q47 bump 1.0.9.
    assert_eq!(manifest.version, "1.0.9");
    assert_eq!(manifest.api_version, south_provider_api::IMAGE_WORLD);
    assert_eq!(manifest.providers.iter().collect::<Vec<_>>(), ["gemini-image"]);
    assert!(manifest.declared_keys("gemini-image").is_empty());
    assert_eq!(manifest.compatibility.contracts.get("media"), Some(&2));
    // Published with 0.59.0, so the literal: a later release bump must not move it.
    assert_eq!(manifest.compatibility.south_runtime, "0.59.0");
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
    let mut media_1_only = host_range::host_range();
    media_1_only.contracts.insert("media".to_owned(), BTreeSet::from([1]));
    assert!(matches!(
        compatibility_admits(&manifest, &media_1_only),
        Err(CompatibilityMismatchV2::Contract { declared: 2, .. })
    ));
    // The packages declaring media 1 still load on that host.
    let azure: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/image-azure/manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(compatibility_admits(&azure, &media_1_only), Ok(()));
}

/// The two packages of I3-1 started at 1.0.0 in the image world (published with 0.58.0; I3-2's Q47
/// bump made them 1.0.1, I3-4's 1.0.2, I3-5's 1.0.3 and the Kiro step's 1.0.4) and declare the oldest runtime that admits their declarations, 0.57.0
/// (the first to record `canonical_ir` 4, which every package of this tuple needs).
/// `image-minimax` declares one non-secret config key, `group_id` (IM-Q3); `image-qwen` declares
/// none.
#[test]
fn the_i3_1_image_packages_declare_the_runtime_and_values_of_the_record() {
    for (name, family, keys) in [
        ("image-minimax", "minimax-image", vec!["group_id"]),
        ("image-qwen", "bailian-qwen-image", vec![]),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        // 1.0.4 after the Kiro step; task contract 8's Q47 bump made it 1.0.5, I3-3's 1.0.6,
        // S-S-1's 1.0.7, V3-1's 1.0.8, provider-vertex's 1.0.9 and the speech Azure OpenAI families' Q47 bump 1.0.10.
        assert_eq!(manifest.version, "1.0.10", "{name}");
        assert_eq!(manifest.api_version, south_provider_api::IMAGE_WORLD, "{name}");
        assert_eq!(manifest.providers.iter().collect::<Vec<_>>(), [family], "{name}");
        assert_eq!(manifest.compatibility.south_runtime, "0.57.0", "{name}");
        assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()), "{name}");
        let declared: Vec<String> =
            manifest.declared_keys(family).into_iter().map(str::to_owned).collect();
        assert_eq!(declared, keys, "{name}");
    }
}

/// `image-stability` (I3-4, record §1) started at 1.0.0 in the image world (I3-5's Q47 bump made
/// it 1.0.1 and the Kiro step's 1.0.2), family
/// `stability-image`, no config keys, `bearer` only, `generate` only, and `contracts`
/// `{image: 1, media: 1}`: it names no word past media 1, so it declares the oldest runtime that
/// admits its declarations, 0.57.0, as the I3-1 packages do, and a media 1 host admits it.
#[test]
fn image_stability_declares_media_1_and_the_runtime_of_the_record() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/image-stability/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    // 1.0.2 after the Kiro step; task contract 8's Q47 bump made it 1.0.3, I3-3's 1.0.4, S-S-1's
    // 1.0.5, V3-1's 1.0.6, provider-vertex's 1.0.7 and the speech Azure OpenAI families' Q47 bump 1.0.8.
    assert_eq!(manifest.version, "1.0.8");
    assert_eq!(manifest.api_version, south_provider_api::IMAGE_WORLD);
    assert_eq!(manifest.providers.iter().collect::<Vec<_>>(), ["stability-image"]);
    assert!(manifest.declared_keys("stability-image").is_empty());
    assert_eq!(manifest.compatibility.contracts.get("media"), Some(&1));
    assert_eq!(manifest.compatibility.contracts.get("image"), Some(&1));
    assert_eq!(manifest.compatibility.south_runtime, "0.57.0");
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
    let mut media_1_only = host_range::host_range();
    media_1_only.contracts.insert("media".to_owned(), BTreeSet::from([1]));
    assert_eq!(compatibility_admits(&manifest, &media_1_only), Ok(()));
}

/// `image-ideogram` (I3-5, record §1) started at 1.0.0 in the image world (the Kiro step's Q47 bump
/// made it 1.0.1), family
/// `ideogram-image`, no config keys, `header_secret` only (the native `Api-Key` header), `generate`
/// only, and `contracts` `{image: 1, media: 1}`: it names no word past media 1, so it declares the
/// oldest runtime that admits its declarations, 0.57.0, as `image-azure` (which already uses
/// `header_secret` `api-key` in this world) does, and a media 1 host admits it.
#[test]
fn image_ideogram_declares_media_1_and_the_runtime_of_the_record() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/image-ideogram/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    // 1.0.1 after the Kiro step; task contract 8's Q47 bump made it 1.0.2, I3-3's 1.0.3, S-S-1's
    // 1.0.4, V3-1's 1.0.5, provider-vertex's 1.0.6 and the speech Azure OpenAI families' Q47 bump 1.0.7.
    assert_eq!(manifest.version, "1.0.7");
    assert_eq!(manifest.api_version, south_provider_api::IMAGE_WORLD);
    assert_eq!(manifest.providers.iter().collect::<Vec<_>>(), ["ideogram-image"]);
    assert!(manifest.declared_keys("ideogram-image").is_empty());
    assert_eq!(manifest.auth_arms.iter().collect::<Vec<_>>(), ["header_secret"]);
    assert_eq!(manifest.compatibility.contracts.get("media"), Some(&1));
    assert_eq!(manifest.compatibility.contracts.get("image"), Some(&1));
    assert_eq!(manifest.compatibility.south_runtime, "0.57.0");
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
    let mut media_1_only = host_range::host_range();
    media_1_only.contracts.insert("media".to_owned(), BTreeSet::from([1]));
    assert_eq!(compatibility_admits(&manifest, &media_1_only), Ok(()));
}

/// `image-xai` (I3-7, record `2026-10-15-image-i3-7-xai.md` §1) starts at 1.0.0 in the image
/// world: family `xai-image`, no config keys, `bearer` only (the native OpenAI-compatible
/// `Authorization` header), `generate` and `edit`, and `contracts` `{image: 1, media: 1}`. It names
/// no word past media 1 (its only transform is `as_is`), so it declares the oldest runtime that
/// admits its declarations, 0.57.0, as `image-azure` and `image-ideogram` do, and a media 1 host
/// admits it.
#[test]
fn image_xai_declares_media_1_bearer_and_the_runtime_of_the_record() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/image-xai/manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.name, "image-xai");
    // Published as 1.0.0 with 0.66.0; the speech Azure OpenAI families' Q47 bump made it 1.0.1.
    assert_eq!(manifest.version, "1.0.1");
    assert_eq!(manifest.api_version, south_provider_api::IMAGE_WORLD);
    assert_eq!(manifest.providers.iter().collect::<Vec<_>>(), ["xai-image"]);
    assert_eq!(manifest.capabilities.iter().collect::<Vec<_>>(), ["edit", "generate"]);
    assert!(manifest.declared_keys("xai-image").is_empty());
    assert_eq!(manifest.auth_arms.iter().collect::<Vec<_>>(), ["bearer"]);
    assert_eq!(manifest.compatibility.contracts.get("media"), Some(&1));
    assert_eq!(manifest.compatibility.contracts.get("image"), Some(&1));
    assert_eq!(manifest.compatibility.south_runtime, "0.57.0");
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
    let mut media_1_only = host_range::host_range();
    media_1_only.contracts.insert("media".to_owned(), BTreeSet::from([1]));
    assert_eq!(compatibility_admits(&manifest, &media_1_only), Ok(()));
}

/// `image-vertex` (I3-3, record §1) starts at 1.0.0 in the image world, family `vertex-ai-image`,
/// `bearer` only, `generate` only. It shares `image-gemini`'s module and so names the transform word
/// `data_url_payload`: it declares `contracts.media` 2, which a media 1 host refuses through the
/// contract number. It is the first image package with a credential recipe; its config keys and
/// `credentials` section are `embeddings-vertex`'s (record §4), which the image world has admitted
/// since 0.52.0 (record §2). Its `south_runtime` is 0.59.0, the oldest runtime it needs: that
/// release shipped `contracts.media` 2 (record, Implementation; `image-gemini` declares the same).
#[test]
fn image_vertex_declares_media_2_the_vertex_recipe_and_the_media_2_runtime() {
    let read = |name: &str| -> ComponentManifestV1 {
        serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let (manifest, embeddings) = (read("image-vertex"), read("embeddings-vertex"));
    // Published as 1.0.0 with 0.63.0; S-S-1's Q47 bump made it 1.0.1, V3-1's 1.0.2,
    // provider-vertex's 1.0.3 and the speech Azure OpenAI families' Q47 bump 1.0.4.
    assert_eq!(manifest.version, "1.0.4");
    assert_eq!(manifest.api_version, south_provider_api::IMAGE_WORLD);
    assert_eq!(manifest.providers.iter().collect::<Vec<_>>(), ["vertex-ai-image"]);
    assert_eq!(manifest.auth_arms.iter().collect::<Vec<_>>(), ["bearer"]);
    assert_eq!(
        manifest.declared_keys("vertex-ai-image"),
        BTreeSet::from(["project", "project_id", "region"])
    );
    assert_eq!(manifest.credentials, embeddings.credentials);
    assert_eq!(manifest.config_schema["vertex-ai-image"], embeddings.config_schema["vertex-ai"]);
    assert_eq!(manifest.compatibility.contracts.get("media"), Some(&2));
    assert_eq!(manifest.compatibility.contracts.get("image"), Some(&1));
    // The oldest runtime it needs: `contracts.media` 2 first shipped in 0.59.0 (as `image-gemini`),
    // and nothing else it declares is newer (the image world's value channel is 0.52.0).
    assert_eq!(manifest.compatibility.south_runtime, "0.59.0");
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
    let mut media_1_only = host_range::host_range();
    media_1_only.contracts.insert("media".to_owned(), BTreeSet::from([1]));
    assert!(matches!(
        compatibility_admits(&manifest, &media_1_only),
        Err(CompatibilityMismatchV2::Contract { declared: 2, .. })
    ));
}

/// `provider-vertex` (design record `2026-10-15-provider-vertex.md` §3) starts at 1.0.0 in the
/// provider world, family `vertex-ai` (VX-Q2), `bearer` only, `provider-gemini`'s capabilities. Its
/// config keys and `credentials` section are `image-vertex`'s, so one provider row and one
/// credential serve every Vertex package. It declares the oldest runtime that admits its
/// declarations (record §7): `north_request_passthrough` is a 0.60.0 manifest field, which an older
/// runtime refuses as unknown, so 0.60.0; the minted slot and the `vertex_sa` recipe are older.
#[test]
fn provider_vertex_declares_the_vertex_recipe_and_the_runtime_of_its_newest_declaration() {
    let read = |name: &str| -> ComponentManifestV1 {
        serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let (manifest, image) = (read("provider-vertex"), read("image-vertex"));
    // Published as 1.0.0 with 0.66.0; the speech Azure OpenAI families' Q47 bump made it 1.0.1.
    assert_eq!(manifest.version, "1.0.1");
    assert_eq!(manifest.api_version, south_provider_api::PROVIDER_WORLD);
    assert_eq!(manifest.providers.iter().collect::<Vec<_>>(), ["vertex-ai"]);
    assert_eq!(manifest.auth_arms.iter().collect::<Vec<_>>(), ["bearer"]);
    assert_eq!(manifest.capabilities, read("provider-gemini").capabilities);
    assert_eq!(
        manifest.declared_keys("vertex-ai"),
        BTreeSet::from(["project", "project_id", "region"])
    );
    assert_eq!(manifest.credentials, image.credentials);
    assert_eq!(manifest.config_schema["vertex-ai"], image.config_schema["vertex-ai-image"]);
    assert!(manifest.north_request_passthrough.contains_key("vertex-ai"));
    assert_eq!(manifest.compatibility.south_runtime, "0.60.0");
    assert!(manifest.compatibility.contracts.is_empty());
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
    let mut on_059 = host_range::host_range();
    "0.59.0".clone_into(&mut on_059.south_runtime);
    assert!(matches!(
        compatibility_admits(&manifest, &on_059),
        Err(CompatibilityMismatchV2::SouthRuntimeAboveHost { .. })
    ));
}

/// `image-reve` (I3-6, design record `2026-10-13-image-i3-6-reve.md` §1) starts at 1.0.0 in the
/// image world, family `reve-image` (RV-Q6), no config keys, `bearer` only (the native arm sends
/// `Authorization: Bearer`), `generate` and `edit` (RV-Q1), and `contracts` `{image: 1, media: 1}`:
/// the `credits` form and `charged_failure` exist since the world shipped (0.52.0) and it names no
/// media word past 1, so it declares the oldest runtime that admits its declarations, 0.57.0
/// (`canonical_ir` 4), as `image-azure` and `image-ideogram` do, and a media 1 host admits it.
#[test]
fn image_reve_declares_media_1_and_the_runtime_of_the_record() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/image-reve/manifest.json")).unwrap(),
    )
    .unwrap();
    // Published as 1.0.0 with 0.66.0; the speech Azure OpenAI families' Q47 bump made it 1.0.1.
    assert_eq!(manifest.version, "1.0.1");
    assert_eq!(manifest.api_version, south_provider_api::IMAGE_WORLD);
    assert_eq!(manifest.providers.iter().collect::<Vec<_>>(), ["reve-image"]);
    assert!(manifest.declared_keys("reve-image").is_empty());
    assert_eq!(manifest.auth_arms.iter().collect::<Vec<_>>(), ["bearer"]);
    assert_eq!(manifest.compatibility.contracts.get("media"), Some(&1));
    assert_eq!(manifest.compatibility.contracts.get("image"), Some(&1));
    assert_eq!(manifest.compatibility.south_runtime, "0.57.0");
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
    let mut media_1_only = host_range::host_range();
    media_1_only.contracts.insert("media".to_owned(), BTreeSet::from([1]));
    assert_eq!(compatibility_admits(&manifest, &media_1_only), Ok(()));
}

/// KC1 record §5: the two Anthropic-family packages read the host's per-tenant end-user hash
/// (`end_user_hash`, mapped to `metadata.user_id`); the Kiro record (§4.5, boundary §13.8) has
/// `provider-kiro` read the attempt id (`attempt_id`, its `conversationId`); no other package
/// declares a host value.
#[test]
fn only_the_records_that_decided_a_host_value_declare_one() {
    for package in shipped_packages() {
        let manifest: ComponentManifestV1 =
            serde_json::from_str(&std::fs::read_to_string(package.join("manifest.json")).unwrap())
                .unwrap();
        let expected: &[&str] = match manifest.name.as_str() {
            "provider-anthropic" | "provider-anthropic-bedrock-invoke" => &["end_user_hash"],
            "provider-kiro" => &["attempt_id"],
            _ => &[],
        };
        assert_eq!(manifest.host_values, expected, "{}", manifest.name);
    }
}

/// The Kiro step (design record `2026-09-30-kiro-provider-component.md`, accepted 2026-10-13) adds
/// the `provider-kiro` reference to `south-component-conformance` (0.59.0 → 0.60.0), which every
/// guest links, so every `component.wasm` changes: each identity published with 0.61.0 retires. No
/// published package changes behaviour: each takes a patch and keeps its `south_runtime` and its
/// `contracts`.
#[test]
fn kiro_retires_every_published_061_package_identity() {
    for (name, published, _kiro) in [
        ("provider-openai-compatible", "2.6.4", "2.6.5"),
        ("provider-anthropic", "1.1.4", "1.1.5"),
        ("provider-anthropic-bedrock-invoke", "1.1.4", "1.1.5"),
        ("provider-gemini", "1.3.4", "1.3.5"),
        ("provider-bedrock-converse", "1.1.4", "1.1.5"),
        ("provider-bedrock-converse-bearer", "1.1.4", "1.1.5"),
        ("provider-openai-responses", "1.3.4", "1.3.5"),
        ("task-kling", "1.0.21", "1.0.22"),
        ("task-kling-v2", "0.32.19", "0.32.20"),
        ("task-minimax-v2", "0.31.18", "0.31.19"),
        ("task-bailian-v2", "0.31.18", "0.31.19"),
        ("task-xai-v2", "0.35.18", "0.35.19"),
        ("task-byteplus-v2", "0.36.18", "0.36.19"),
        ("task-veo-v2", "0.35.18", "0.35.19"),
        ("task-wan-image-v2", "0.35.18", "0.35.19"),
        ("task-gmi-image-v2", "0.35.18", "0.35.19"),
        ("embeddings-openai-compatible", "1.0.12", "1.0.13"),
        ("embeddings-gemini", "1.1.9", "1.1.10"),
        ("embeddings-vertex", "1.0.11", "1.0.12"),
        ("image-azure", "1.0.8", "1.0.9"),
        ("image-minimax", "1.0.3", "1.0.4"),
        ("image-qwen", "1.0.3", "1.0.4"),
        ("image-gemini", "1.0.2", "1.0.3"),
        ("image-stability", "1.0.1", "1.0.2"),
        ("image-ideogram", "1.0.0", "1.0.1"),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
        let media = if name == "image-gemini" { 2 } else { 1 };
        assert_eq!(
            manifest.compatibility.south_runtime,
            declared_runtime_now(name),
            "{name}: the Kiro step moves no runtime"
        );
        if manifest.api_version == south_provider_api::IMAGE_WORLD {
            assert_eq!(manifest.compatibility.contracts.get("media"), Some(&media), "{name}");
        }
        assert_eq!(
            manifest.compatibility.ir_schema_id, "token-station-protocol@0.6.0/v0.5.0",
            "{name}"
        );
    }
}

/// `provider-kiro` (Kiro record §3) starts at 1.0.0, family `kiro`. It declares the oldest runtime
/// that admits its declarations: `subscription_quota_families` (the native arm counts Kiro among
/// the subscription families whose 429 raises the quota alert) is a 0.60.0 manifest field, which an
/// older runtime refuses as unknown, so 0.60.0 and not 0.57.0.
#[test]
fn provider_kiro_declares_the_runtime_of_its_newest_declaration() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/provider-kiro/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    // 1.0.0 as the Kiro step shipped it; task contract 8's Q47 bump made it 1.0.1, I3-3's 1.0.2,
    // S-S-1's 1.0.3 and V3-1's 1.0.4; the native rewrites (KR2–KR5, KD15) made it 1.1.0 and
    // provider-vertex's Q47 bump 1.1.1, then the speech Azure OpenAI families' Q47 bump 1.1.2.
    assert_eq!(manifest.version, "1.1.2");
    assert_eq!(manifest.api_version, south_provider_api::PROVIDER_WORLD);
    assert_eq!(manifest.providers.iter().collect::<Vec<_>>(), ["kiro"]);
    assert!(manifest.subscription_quota_families.contains("kiro"));
    assert_eq!(manifest.compatibility.south_runtime, "0.60.0");
    assert!(manifest.compatibility.contracts.is_empty());
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
    let mut on_059 = host_range::host_range();
    "0.59.0".clone_into(&mut on_059.south_runtime);
    assert!(matches!(
        compatibility_admits(&manifest, &on_059),
        Err(CompatibilityMismatchV2::SouthRuntimeAboveHost { .. })
    ));
}

/// `task-gemini-omni-v2` 0.1.0 (the Omni record): the first task package on contract 8, with the
/// media vocabulary its inline artifacts are named in. A host whose range lists task 7 only refuses
/// it by the contract number, while every contract 7 package stays admitted next to it. Its
/// `south_runtime` must be the first runtime that decodes task contract 8, the release that ships
/// it: 0.62.0 (the release commit moved the declaration with the workspace version, as 0.59.0 did
/// for `image-gemini`).
#[test]
fn task_gemini_omni_v2_declares_task_8_which_a_task_7_host_refuses() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/task-gemini-omni-v2/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    // Published as 0.1.0 with 0.62.0; I3-3's Q47 bump made it 0.1.1, S-S-1's 0.1.2, V3-1's
    // 0.1.3, provider-vertex's 0.1.4 and the speech Azure OpenAI families' Q47 bump 0.1.5.
    assert_eq!(manifest.version, "0.1.5");
    assert_eq!(manifest.api_version, south_provider_api::TASK_WORLD_V2);
    assert_eq!(manifest.compatibility.contracts.get("task"), Some(&8));
    assert_eq!(manifest.compatibility.contracts.get("media"), Some(&1));
    // Published with 0.62.0, so the literal: a later release bump must not move it.
    assert_eq!(manifest.compatibility.south_runtime, "0.62.0");
    assert_eq!(manifest.response_blob_paths, ["/steps/*/content/*/data", "/output_video/data"]);
    let this_release = host_range::host_range();
    assert_eq!(compatibility_admits(&manifest, &this_release), Ok(()));
    let mut task_7_only = this_release;
    task_7_only.contracts.insert("task".to_owned(), [7].into());
    assert!(compatibility_admits(&manifest, &task_7_only).is_err());
    let veo: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/task-veo-v2/manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(compatibility_admits(&veo, &task_7_only), Ok(()));
}

/// Task contract 8 (the Omni record §13) changes `south-contracts` (0.53.0 → 0.54.0) and
/// `south-provider-api` (0.54.0 → 0.55.0), and moves `south-component-conformance` one step past
/// the Kiro step (0.60.0 → 0.61.0); every guest links all three, so each identity the Kiro step
/// left (none of them published) takes one more patch with unchanged behaviour. None moves its
/// `south_runtime` or its contracts: contract 8 is additive, and every existing task package
/// stays at the contract 7 it was published with. The identity each package took here (published
/// with 0.62.0) is no longer pinned: I3-3 gave every package one more
/// (`i3_3_retires_every_published_062_package_identity`).
#[test]
fn task_contract_8_retires_every_kiro_step_package_identity() {
    for (name, kiro_step, _contract_8) in [
        ("embeddings-gemini", "1.1.10", "1.1.11"),
        ("embeddings-openai-compatible", "1.0.13", "1.0.14"),
        ("embeddings-vertex", "1.0.12", "1.0.13"),
        ("image-azure", "1.0.9", "1.0.10"),
        ("image-gemini", "1.0.3", "1.0.4"),
        ("image-ideogram", "1.0.1", "1.0.2"),
        ("image-minimax", "1.0.4", "1.0.5"),
        ("image-qwen", "1.0.4", "1.0.5"),
        ("image-stability", "1.0.2", "1.0.3"),
        ("provider-anthropic-bedrock-invoke", "1.1.5", "1.1.6"),
        ("provider-anthropic", "1.1.5", "1.1.6"),
        ("provider-bedrock-converse-bearer", "1.1.5", "1.1.6"),
        ("provider-bedrock-converse", "1.1.5", "1.1.6"),
        ("provider-gemini", "1.3.5", "1.3.6"),
        ("provider-kiro", "1.0.0", "1.0.1"),
        ("provider-openai-compatible", "2.6.5", "2.6.6"),
        ("provider-openai-responses", "1.3.5", "1.3.6"),
        ("task-bailian-v2", "0.31.19", "0.31.20"),
        ("task-byteplus-v2", "0.36.19", "0.36.20"),
        ("task-gmi-image-v2", "0.35.19", "0.35.20"),
        ("task-kling-v2", "0.32.20", "0.32.21"),
        ("task-kling", "1.0.22", "1.0.23"),
        ("task-minimax-v2", "0.31.19", "0.31.20"),
        ("task-veo-v2", "0.35.19", "0.35.20"),
        ("task-wan-image-v2", "0.35.19", "0.35.20"),
        ("task-xai-v2", "0.35.19", "0.35.20"),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, kiro_step, "{name} kept the Kiro step's identity");
        if name != "provider-kiro" {
            assert_eq!(
                manifest.compatibility.south_runtime,
                declared_runtime_now(name),
                "{name}: task contract 8 moves no runtime"
            );
        }
        if manifest.api_version.starts_with("task") {
            assert_eq!(manifest.compatibility.contracts.get("task"), Some(&7), "{name}");
        }
    }
}

/// Image world step I3-3 (design record `2026-10-13-image-i3-3-vertex.md`, accepted 2026-10-14) adds
/// the `image-vertex` reference and the shared `vertex_location` module to
/// `south-component-conformance` (0.61.0 → 0.62.0), which every guest links, so every
/// `component.wasm` changes: each identity published with 0.62.0 retires. No published package
/// changes behaviour (`embeddings-vertex` only calls the location rules from their new module): each
/// takes a patch and keeps its `south_runtime` and its `contracts`. The identity each package took
/// here (published with 0.63.0) is no longer pinned: speech step S-S-1 gave every package one more
/// (`s_s_1_retires_every_published_063_package_identity`).
#[test]
fn i3_3_retires_every_published_062_package_identity() {
    for (name, published, _i3_3) in [
        ("embeddings-gemini", "1.1.11", "1.1.12"),
        ("embeddings-openai-compatible", "1.0.14", "1.0.15"),
        ("embeddings-vertex", "1.0.13", "1.0.14"),
        ("image-azure", "1.0.10", "1.0.11"),
        ("image-gemini", "1.0.4", "1.0.5"),
        ("image-ideogram", "1.0.2", "1.0.3"),
        ("image-minimax", "1.0.5", "1.0.6"),
        ("image-qwen", "1.0.5", "1.0.6"),
        ("image-stability", "1.0.3", "1.0.4"),
        ("provider-anthropic-bedrock-invoke", "1.1.6", "1.1.7"),
        ("provider-anthropic", "1.1.6", "1.1.7"),
        ("provider-bedrock-converse-bearer", "1.1.6", "1.1.7"),
        ("provider-bedrock-converse", "1.1.6", "1.1.7"),
        ("provider-gemini", "1.3.6", "1.3.7"),
        ("provider-kiro", "1.0.1", "1.0.2"),
        ("provider-openai-compatible", "2.6.6", "2.6.7"),
        ("provider-openai-responses", "1.3.6", "1.3.7"),
        ("task-bailian-v2", "0.31.20", "0.31.21"),
        ("task-byteplus-v2", "0.36.20", "0.36.21"),
        ("task-gemini-omni-v2", "0.1.0", "0.1.1"),
        ("task-gmi-image-v2", "0.35.20", "0.35.21"),
        ("task-kling-v2", "0.32.21", "0.32.22"),
        ("task-kling", "1.0.23", "1.0.24"),
        ("task-minimax-v2", "0.31.20", "0.32.0"),
        ("task-veo-v2", "0.35.20", "0.35.21"),
        ("task-wan-image-v2", "0.35.20", "0.35.21"),
        ("task-xai-v2", "0.35.20", "0.35.21"),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
        let runtime =
            if name == "task-gemini-omni-v2" { "0.62.0" } else { declared_runtime_now(name) };
        assert_eq!(manifest.compatibility.south_runtime, runtime, "{name}: I3-3 moves no runtime");
        if manifest.api_version.starts_with("task") {
            let task = if name == "task-gemini-omni-v2" { 8 } else { 7 };
            assert_eq!(manifest.compatibility.contracts.get("task"), Some(&task), "{name}");
        }
        if manifest.api_version == south_provider_api::IMAGE_WORLD {
            let media = if name == "image-gemini" { 2 } else { 1 };
            assert_eq!(manifest.compatibility.contracts.get("media"), Some(&media), "{name}");
        }
    }
}

/// Speech world step S-S-1 (speech record §18) adds the `speech` contract module to
/// `south-contracts` (0.54.0 → 0.55.0), which every guest links, so every `component.wasm` changes:
/// each identity published with 0.63.0 retires. No published package changes behaviour: each takes
/// a patch and keeps its `south_runtime` and its `contracts`, except `task-minimax-v2`, which then
/// took a minor version for its immutable body paths
/// (`minimax_immutable_paths_retire_the_published_task_minimax_v2_identity`). Step S-S-2 moves
/// `south-provider-api` (0.56.0 → 0.57.0) in the same unreleased release; identities are compared
/// with the release, so the patch taken here covers it and no package takes a second one. The
/// identity each package took here (published with 0.64.0) is no longer pinned: speech batch V3-1
/// gave every package one more (`v3_1_retires_every_published_064_package_identity`).
#[test]
fn s_s_1_retires_every_published_063_package_identity() {
    for (name, published, _s_s_1) in [
        ("embeddings-gemini", "1.1.12", "1.1.13"),
        ("embeddings-openai-compatible", "1.0.15", "1.0.16"),
        ("embeddings-vertex", "1.0.14", "1.0.15"),
        ("image-azure", "1.0.11", "1.0.12"),
        ("image-gemini", "1.0.5", "1.0.6"),
        ("image-ideogram", "1.0.3", "1.0.4"),
        ("image-minimax", "1.0.6", "1.0.7"),
        ("image-qwen", "1.0.6", "1.0.7"),
        ("image-stability", "1.0.4", "1.0.5"),
        ("image-vertex", "1.0.0", "1.0.1"),
        ("provider-anthropic-bedrock-invoke", "1.1.7", "1.1.8"),
        ("provider-anthropic", "1.1.7", "1.1.8"),
        ("provider-bedrock-converse-bearer", "1.1.7", "1.1.8"),
        ("provider-bedrock-converse", "1.1.7", "1.1.8"),
        ("provider-gemini", "1.3.7", "1.3.8"),
        ("provider-kiro", "1.0.2", "1.0.3"),
        ("provider-openai-compatible", "2.6.7", "2.6.8"),
        ("provider-openai-responses", "1.3.7", "1.3.8"),
        ("task-bailian-v2", "0.31.21", "0.31.22"),
        ("task-byteplus-v2", "0.36.21", "0.36.22"),
        ("task-gemini-omni-v2", "0.1.1", "0.1.2"),
        ("task-gmi-image-v2", "0.35.21", "0.35.22"),
        ("task-kling-v2", "0.32.22", "0.32.23"),
        ("task-kling", "1.0.24", "1.0.25"),
        ("task-minimax-v2", "0.32.0", "0.33.0"),
        ("task-veo-v2", "0.35.21", "0.35.22"),
        ("task-wan-image-v2", "0.35.21", "0.35.22"),
        ("task-xai-v2", "0.35.21", "0.35.22"),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
        let runtime = match name {
            "task-gemini-omni-v2" => "0.62.0",
            "image-vertex" => "0.59.0",
            _ => declared_runtime_now(name),
        };
        assert_eq!(manifest.compatibility.south_runtime, runtime, "{name}: S-S-1 moves no runtime");
        if manifest.api_version.starts_with("task") {
            let task = if name == "task-gemini-omni-v2" { 8 } else { 7 };
            assert_eq!(manifest.compatibility.contracts.get("task"), Some(&task), "{name}");
        }
        if manifest.api_version == south_provider_api::IMAGE_WORLD {
            let media = if matches!(name, "image-gemini" | "image-vertex") { 2 } else { 1 };
            assert_eq!(manifest.compatibility.contracts.get("media"), Some(&media), "{name}");
        }
    }
}

/// The version every speech package carries: each was published as 1.0.0 with 0.65.0,
/// provider-vertex's Q47 bump made it 1.0.1 (published with 0.66.0) and the speech `azure-openai`
/// families' Q47 bump 1.0.2 (`speech_azure_openai_retires_every_published_066_package_identity`).
/// The tests below describe the batches that defined the packages, before those releases.
/// `speech-vertex` and `speech-openai-compatible` moved on to minors ([`speech_version`]).
const SPEECH_VERSION: &str = "1.0.2";

/// The version a speech package carries: [`SPEECH_VERSION`], except `speech-vertex`, which Q7's
/// refusal made 1.1.0 and the 0.67.0 cycle's Q47 bump 1.1.1
/// (`speech_vertex_1_1_refuses_q7_and_retires_its_published_identity`), and
/// `speech-openai-compatible`, which the `azure-openai` families made 1.1.0
/// (`speech_openai_compatible_1_1_adds_the_azure_openai_families`).
fn speech_version(name: &str) -> &'static str {
    match name {
        "speech-vertex" => "1.1.1",
        "speech-openai-compatible" => "1.1.0",
        _ => SPEECH_VERSION,
    }
}

/// Speech world batch V3-1 (record `2026-10-14-speech-v3-1-tts.md` §1, §6): four packages start at
/// 1.0.0 in the speech world, `synthesize` only, the secret slot `provider_api_key`, no config keys.
/// Three declare `contracts.media` 1, and a media 1 host admits them. `speech-elevenlabs` declares the
/// query parameter `output_format` and so `contracts.media` 3, which a media 2 host refuses. Since
/// step S-S-5 all four also declare `contracts.speech` 1, new in 0.65.0, and so that release
/// (the workspace version), which the release commit moves with the workspace version (the I3-2 /
/// `task-gemini-omni-v2` precedent).
#[test]
fn the_v3_1_speech_packages_declare_their_media_contract_and_runtime() {
    let read = |name: &str| -> ComponentManifestV1 {
        serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let mut media_1_only = host_range::host_range();
    media_1_only.contracts.insert("media".to_owned(), BTreeSet::from([1]));
    let mut media_2 = host_range::host_range();
    media_2.contracts.insert("media".to_owned(), BTreeSet::from([1, 2]));
    for (name, family, arm, media) in [
        ("speech-openai-compatible", "openai-compatible", "bearer", 1),
        ("speech-xai", "xai-speech", "bearer", 1),
        ("speech-groq", "groq-speech", "bearer", 1),
        ("speech-elevenlabs", "elevenlabs-speech", "header_secret", 3),
    ] {
        let manifest = read(name);
        assert_eq!(manifest.version, speech_version(name), "{name}");
        assert_eq!(manifest.api_version, south_provider_api::SPEECH_WORLD, "{name}");
        // `speech-openai-compatible` 1.1.0 adds the two Azure `OpenAI` families after its first
        // (`speech_openai_compatible_1_1_adds_the_azure_openai_families`).
        let (families, arms): (Vec<&str>, Vec<&str>) = match name {
            "speech-openai-compatible" => {
                (vec![family, "azure-openai-v1", "azure-openai-dated"], vec![arm, "header_secret"])
            }
            _ => (vec![family], vec![arm]),
        };
        assert_eq!(manifest.providers.iter().collect::<Vec<_>>(), families, "{name}");
        // Speech batch V3-3 adds recognition to three of the four at the same unpublished 1.0.0
        // (`the_v3_3_speech_packages_declare_their_operations_and_runtime`).
        let capabilities: &[&str] = match name {
            "speech-openai-compatible" => &["synthesize", "transcribe", "translate"],
            "speech-xai" | "speech-elevenlabs" => &["synthesize", "transcribe"],
            _ => &["synthesize"],
        };
        assert_eq!(manifest.capabilities.iter().collect::<Vec<_>>(), capabilities, "{name}");
        assert_eq!(manifest.auth_arms.iter().collect::<Vec<_>>(), arms, "{name}");
        assert!(manifest.declared_keys(family).is_empty(), "{name}");
        assert_eq!(
            manifest.compatibility.contracts,
            std::collections::BTreeMap::from([
                ("media".to_owned(), media),
                ("speech".to_owned(), 1)
            ]),
            "{name}"
        );
        assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()), "{name}");
        // `contracts.speech` arrived in 0.65.0 (S-S-5), so every speech package declares that
        // release (`the_speech_packages_declare_contracts_speech_1_and_this_release`).
        assert_eq!(manifest.compatibility.south_runtime, SPEECH_RUNTIME, "{name}");
        if media == 1 {
            assert!(manifest.query_parameters.is_empty(), "{name}");
            assert_eq!(compatibility_admits(&manifest, &media_1_only), Ok(()), "{name}");
        } else {
            assert_eq!(manifest.query_parameters.len(), 1, "{name}");
            assert_eq!(manifest.query_parameters[0].name, "output_format", "{name}");
            assert!(matches!(
                compatibility_admits(&manifest, &media_2),
                Err(CompatibilityMismatchV2::Contract { declared: 3, .. })
            ));
        }
    }
}

/// Speech world batch V3-2 (record `2026-10-14-speech-v3-2-tts.md` §1, §3): `speech-mimo` and
/// `speech-minimax` start at 1.0.0 in the speech world, `synthesize` only, `bearer` on the secret
/// slot `provider_api_key`, `contracts` `{media: 1, speech: 1}` (`speech` from step S-S-5) and so
/// 0.65.0; a media 1 host admits them. Neither declares a query
/// parameter: `GroupId` is a sanctioned name. `speech-minimax` declares one non-secret config key,
/// `group_id`, with `image-minimax`'s `digits` syntax and description (IM-Q3); `speech-mimo` none.
#[test]
fn the_v3_2_speech_packages_declare_their_runtime_and_values() {
    let read = |name: &str| -> ComponentManifestV1 {
        serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let mut media_1_only = host_range::host_range();
    media_1_only.contracts.insert("media".to_owned(), BTreeSet::from([1]));
    for (name, family, keys) in [
        ("speech-mimo", "mimo-speech", vec![]),
        ("speech-minimax", "minimax-speech", vec!["group_id"]),
    ] {
        let manifest = read(name);
        assert_eq!(manifest.version, speech_version(name), "{name}");
        assert_eq!(manifest.api_version, south_provider_api::SPEECH_WORLD, "{name}");
        assert_eq!(manifest.providers.iter().collect::<Vec<_>>(), [family], "{name}");
        assert_eq!(manifest.capabilities.iter().collect::<Vec<_>>(), ["synthesize"], "{name}");
        assert_eq!(manifest.auth_arms.iter().collect::<Vec<_>>(), ["bearer"], "{name}");
        assert!(manifest.query_parameters.is_empty(), "{name}");
        assert_eq!(
            manifest.compatibility.contracts,
            std::collections::BTreeMap::from([("media".to_owned(), 1), ("speech".to_owned(), 1)]),
            "{name}"
        );
        assert_eq!(manifest.compatibility.south_runtime, SPEECH_RUNTIME, "{name}");
        assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()), "{name}");
        assert_eq!(compatibility_admits(&manifest, &media_1_only), Ok(()), "{name}");
        let declared: Vec<String> =
            manifest.declared_keys(family).into_iter().map(str::to_owned).collect();
        assert_eq!(declared, keys, "{name}");
    }
    // The same key, syntax and description as `image-minimax`'s: one account, two families.
    let schema = |name: &str, family: &str| -> serde_json::Value {
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        manifest["config_schema"][family]["group_id"].clone()
    };
    let speech = schema("speech-minimax", "minimax-speech");
    assert_eq!(speech["syntax"], "digits");
    assert_eq!(speech, schema("image-minimax", "minimax-image"));
}

/// Speech world batch V3-3 (record `2026-10-14-speech-v3-3-asr.md` §1, §10, SV-Q1 (b)): recognition
/// arrives as a later version of the family's package. The three V3-1 packages it extends are
/// unpublished, so they gain `transcribe` (and `speech-openai-compatible` `translate`) at 1.0.0 with
/// no version move and keep their runtime and contracts; `speech-azure` starts at 1.0.0,
/// `transcribe` only, `header_secret`, no config keys and no query parameter (`api-version` is a
/// sanctioned name), `contracts.media` 1, which a media 1 host admits. No other speech package
/// changes. Step S-S-5 adds `contracts.speech` 1 and with it 0.65.0 as the runtime of
/// every speech package. Speech batch V4 then gives
/// `speech-azure` `synthesize` at the same unpublished 1.0.0
/// (`the_v4_speech_packages_declare_their_values_and_runtime`).
#[test]
fn the_v3_3_speech_packages_declare_their_operations_and_runtime() {
    let read = |name: &str| -> ComponentManifestV1 {
        serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let mut media_1_only = host_range::host_range();
    media_1_only.contracts.insert("media".to_owned(), BTreeSet::from([1]));
    for (name, capabilities, runtime) in [
        ("speech-openai-compatible", vec!["synthesize", "transcribe", "translate"], SPEECH_RUNTIME),
        ("speech-xai", vec!["synthesize", "transcribe"], SPEECH_RUNTIME),
        ("speech-elevenlabs", vec!["synthesize", "transcribe"], SPEECH_RUNTIME),
        ("speech-azure", vec!["synthesize", "transcribe"], SPEECH_RUNTIME),
        ("speech-groq", vec!["synthesize"], SPEECH_RUNTIME),
        ("speech-mimo", vec!["synthesize"], SPEECH_RUNTIME),
        ("speech-minimax", vec!["synthesize"], SPEECH_RUNTIME),
    ] {
        let manifest = read(name);
        assert_eq!(manifest.version, speech_version(name), "{name}");
        assert_eq!(manifest.api_version, south_provider_api::SPEECH_WORLD, "{name}");
        assert_eq!(manifest.capabilities.iter().collect::<Vec<_>>(), capabilities, "{name}");
        assert_eq!(manifest.compatibility.south_runtime, runtime, "{name}");
        assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()), "{name}");
    }
    let azure = read("speech-azure");
    assert_eq!(azure.providers.iter().collect::<Vec<_>>(), ["azure-speech"]);
    assert_eq!(azure.auth_arms.iter().collect::<Vec<_>>(), ["header_secret"]);
    assert!(azure.declared_keys("azure-speech").is_empty());
    assert!(azure.query_parameters.is_empty());
    assert_eq!(
        azure.compatibility.contracts,
        std::collections::BTreeMap::from([("media".to_owned(), 1), ("speech".to_owned(), 1)])
    );
    assert_eq!(compatibility_admits(&azure, &media_1_only), Ok(()));
}

/// Speech world batch V4 (record `2026-10-15-speech-v4-tts.md` §1, §2.6, §3.1, §4.1, §10):
/// `speech-qwen` and `speech-vertex` start at 1.0.0 in the speech world, `synthesize` only (Qwen also
/// `artifact_fetch`), `bearer` on the secret slot `provider_api_key`, `contracts` `{media: 1,
/// speech: 1}` (`speech` from step S-S-5) and so 0.65.0; a media 1 host admits them.
/// Neither declares a query parameter. `speech-vertex` declares `image-vertex`'s
/// configuration keys and credentials section byte for byte, so one row and one credential serve
/// the three Vertex packages; `speech-qwen` declares neither. `speech-azure` gains `synthesize` at
/// its unpublished 1.0.0 and keeps everything else.
#[test]
fn the_v4_speech_packages_declare_their_values_and_runtime() {
    let read = |name: &str| -> ComponentManifestV1 {
        serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let raw = |name: &str| -> serde_json::Value {
        serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let mut media_1_only = host_range::host_range();
    media_1_only.contracts.insert("media".to_owned(), BTreeSet::from([1]));
    for (name, family, capabilities, keys) in [
        ("speech-qwen", "bailian-qwen-speech", vec!["artifact_fetch", "synthesize"], vec![]),
        (
            "speech-vertex",
            "vertex-ai-speech",
            vec!["synthesize"],
            vec!["project", "project_id", "region"],
        ),
    ] {
        let manifest = read(name);
        assert_eq!(manifest.version, speech_version(name), "{name}");
        assert_eq!(manifest.api_version, south_provider_api::SPEECH_WORLD, "{name}");
        assert_eq!(manifest.providers.iter().collect::<Vec<_>>(), [family], "{name}");
        assert_eq!(manifest.capabilities.iter().collect::<Vec<_>>(), capabilities, "{name}");
        assert_eq!(manifest.auth_arms.iter().collect::<Vec<_>>(), ["bearer"], "{name}");
        assert_eq!(manifest.declared_keys(family), keys.into_iter().collect(), "{name}");
        assert!(manifest.query_parameters.is_empty(), "{name}");
        assert_eq!(manifest.compatibility.south_runtime, SPEECH_RUNTIME, "{name}");
        assert_eq!(
            manifest.compatibility.contracts,
            std::collections::BTreeMap::from([("media".to_owned(), 1), ("speech".to_owned(), 1)]),
            "{name}"
        );
        assert_eq!(compatibility_admits(&manifest, &media_1_only), Ok(()), "{name}");
    }
    assert_eq!(raw("speech-qwen").get("credentials"), None);
    assert_eq!(raw("speech-vertex")["credentials"], raw("image-vertex")["credentials"]);
    assert_eq!(
        raw("speech-vertex")["config_schema"]["vertex-ai-speech"],
        raw("image-vertex")["config_schema"]["vertex-ai-image"]
    );
    let azure = read("speech-azure");
    assert_eq!(azure.version, SPEECH_VERSION);
    assert_eq!(azure.capabilities.iter().collect::<Vec<_>>(), ["synthesize", "transcribe"]);
}

/// Speech world batch V3-1 (record `2026-10-14-speech-v3-1-tts.md` §13) changes `south-contracts`
/// (0.55.0 → 0.56.0, `contracts.media` 3), `south-provider-api` (0.57.0 → 0.58.0, SV-Q5) and
/// `south-component-conformance` (0.63.0 → 0.64.0, the four speech references), which every guest
/// links, so every `component.wasm` changes: each identity published with 0.64.0 retires. No
/// published package changes behaviour: each takes a patch and keeps its `south_runtime` and its
/// `contracts`. The identity each package took here (published with 0.65.0) is no longer pinned:
/// `provider-vertex` gave every package one more
/// (`provider_vertex_retires_every_published_065_package_identity`).
#[test]
fn v3_1_retires_every_published_064_package_identity() {
    for (name, published, _v3_1) in [
        ("embeddings-gemini", "1.1.13", "1.1.14"),
        ("embeddings-openai-compatible", "1.0.16", "1.0.17"),
        ("embeddings-vertex", "1.0.15", "1.0.16"),
        ("image-azure", "1.0.12", "1.0.13"),
        ("image-gemini", "1.0.6", "1.0.7"),
        ("image-ideogram", "1.0.4", "1.0.5"),
        ("image-minimax", "1.0.7", "1.0.8"),
        ("image-qwen", "1.0.7", "1.0.8"),
        ("image-stability", "1.0.5", "1.0.6"),
        ("image-vertex", "1.0.1", "1.0.2"),
        ("provider-anthropic-bedrock-invoke", "1.1.8", "1.1.9"),
        ("provider-anthropic", "1.1.8", "1.1.9"),
        ("provider-bedrock-converse-bearer", "1.1.8", "1.1.9"),
        ("provider-bedrock-converse", "1.1.8", "1.1.9"),
        ("provider-gemini", "1.3.8", "1.3.9"),
        // The Kiro native rewrites (KR2–KR5, KD15) change behaviour: a minor version.
        ("provider-kiro", "1.0.3", "1.1.0"),
        ("provider-openai-compatible", "2.6.8", "2.6.9"),
        ("provider-openai-responses", "1.3.8", "1.3.9"),
        ("task-bailian-v2", "0.31.22", "0.31.23"),
        ("task-byteplus-v2", "0.36.22", "0.36.23"),
        ("task-gemini-omni-v2", "0.1.2", "0.1.3"),
        ("task-gmi-image-v2", "0.35.22", "0.35.23"),
        ("task-kling-v2", "0.32.23", "0.32.24"),
        ("task-kling", "1.0.25", "1.0.26"),
        ("task-minimax-v2", "0.33.0", "0.33.1"),
        ("task-veo-v2", "0.35.22", "0.35.23"),
        ("task-wan-image-v2", "0.35.22", "0.35.23"),
        ("task-xai-v2", "0.35.22", "0.35.23"),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
        let runtime = match name {
            "task-gemini-omni-v2" => "0.62.0",
            "image-vertex" => "0.59.0",
            _ => declared_runtime_now(name),
        };
        assert_eq!(manifest.compatibility.south_runtime, runtime, "{name}: V3-1 moves no runtime");
        if manifest.api_version.starts_with("task") {
            let task = if name == "task-gemini-omni-v2" { 8 } else { 7 };
            assert_eq!(manifest.compatibility.contracts.get("task"), Some(&task), "{name}");
        }
        if manifest.api_version == south_provider_api::IMAGE_WORLD {
            let media = if matches!(name, "image-gemini" | "image-vertex") { 2 } else { 1 };
            assert_eq!(manifest.compatibility.contracts.get("media"), Some(&media), "{name}");
        }
    }
}

/// `provider-vertex` (design record `2026-10-15-provider-vertex.md`) moves the Gemini reference's
/// shared pieces into the module `gemini_text` and adds the `provider-vertex` reference to
/// `south-component-conformance` (0.64.0 → 0.65.0), which every guest links, so every
/// `component.wasm` changes: each identity published with 0.65.0 retires. No published package
/// changes behaviour (`provider-gemini` only calls the shared pieces from their new module): each
/// takes a patch and keeps its `south_runtime` and its `contracts`. The identity each package took
/// here (published with 0.66.0) is no longer pinned: the speech `azure-openai` families gave every
/// package one more (`speech_azure_openai_retires_every_published_066_package_identity`).
#[test]
fn provider_vertex_retires_every_published_065_package_identity() {
    for (name, published, _provider_vertex) in [
        ("embeddings-gemini", "1.1.14", "1.1.15"),
        ("embeddings-openai-compatible", "1.0.17", "1.0.18"),
        ("embeddings-vertex", "1.0.16", "1.0.17"),
        ("image-azure", "1.0.13", "1.0.14"),
        ("image-gemini", "1.0.7", "1.0.8"),
        ("image-ideogram", "1.0.5", "1.0.6"),
        ("image-minimax", "1.0.8", "1.0.9"),
        ("image-qwen", "1.0.8", "1.0.9"),
        ("image-stability", "1.0.6", "1.0.7"),
        ("image-vertex", "1.0.2", "1.0.3"),
        // The Claude Code families then took the minor 1.2.0 over this patch (Claude Code record
        // §9, `claude_code_conformance_v1::the_claude_code_families_retire_the_patch_identity`).
        ("provider-anthropic", "1.1.9", "1.2.0"),
        ("provider-anthropic-bedrock-invoke", "1.1.9", "1.1.10"),
        ("provider-bedrock-converse", "1.1.9", "1.1.10"),
        ("provider-bedrock-converse-bearer", "1.1.9", "1.1.10"),
        ("provider-gemini", "1.3.9", "1.3.10"),
        ("provider-kiro", "1.1.0", "1.1.1"),
        // The T9 families then took the minor 2.7.0 over this patch (T9 record §8,
        // `openai_compatible_t9_v1::the_t9_families_retire_the_patch_identity`).
        ("provider-openai-compatible", "2.6.9", "2.7.0"),
        // The `bedrock-mantle` family (record `2026-10-15-bedrock-mantle.md` §11) adds behaviour in
        // the same unreleased cycle: a minor version.
        ("provider-openai-responses", "1.3.9", "1.4.0"),
        ("speech-azure", "1.0.0", "1.0.1"),
        ("speech-elevenlabs", "1.0.0", "1.0.1"),
        ("speech-groq", "1.0.0", "1.0.1"),
        ("speech-mimo", "1.0.0", "1.0.1"),
        ("speech-minimax", "1.0.0", "1.0.1"),
        ("speech-openai-compatible", "1.0.0", "1.0.1"),
        ("speech-qwen", "1.0.0", "1.0.1"),
        ("speech-vertex", "1.0.0", "1.0.1"),
        ("speech-xai", "1.0.0", "1.0.1"),
        ("task-bailian-v2", "0.31.23", "0.31.24"),
        ("task-byteplus-v2", "0.36.23", "0.36.24"),
        ("task-gemini-omni-v2", "0.1.3", "0.1.4"),
        ("task-gmi-image-v2", "0.35.23", "0.35.24"),
        ("task-kling", "1.0.26", "1.0.27"),
        ("task-kling-v2", "0.32.24", "0.32.25"),
        ("task-minimax-v2", "0.33.1", "0.33.2"),
        ("task-veo-v2", "0.35.23", "0.35.24"),
        ("task-wan-image-v2", "0.35.23", "0.35.24"),
        ("task-xai-v2", "0.35.23", "0.35.24"),
    ] {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
        let runtime = match name {
            "task-gemini-omni-v2" => "0.62.0",
            "image-vertex" => "0.59.0",
            // Published with 0.65.0, the first runtime that records `contracts.speech` 1.
            _ if name.starts_with("speech-") => "0.65.0",
            _ => declared_runtime_now(name),
        };
        assert_eq!(
            manifest.compatibility.south_runtime, runtime,
            "{name}: provider-vertex moves no runtime"
        );
        if manifest.api_version.starts_with("task") {
            let task = if name == "task-gemini-omni-v2" { 8 } else { 7 };
            assert_eq!(manifest.compatibility.contracts.get("task"), Some(&task), "{name}");
        }
        if manifest.api_version == south_provider_api::IMAGE_WORLD {
            let media = if matches!(name, "image-gemini" | "image-vertex") { 2 } else { 1 };
            assert_eq!(manifest.compatibility.contracts.get("media"), Some(&media), "{name}");
        }
        if manifest.api_version == south_provider_api::SPEECH_WORLD {
            let media = if name == "speech-elevenlabs" { 3 } else { 1 };
            assert_eq!(
                manifest.compatibility.contracts,
                std::collections::BTreeMap::from([
                    ("media".to_owned(), media),
                    ("speech".to_owned(), 1)
                ]),
                "{name}"
            );
        }
    }
}

/// Declaring immutable body paths changes what `task-minimax-v2` prepares (task contract 6: the
/// host may now inject request extras outside those paths, design record
/// `2026-10-14-task-minimax-immutable-paths.md`), so the package takes a minor version: the
/// published 0.32.0 identity and the unpublished S-S-1 patch 0.32.1 both retire. The runtime and
/// the contracts stay: every task-v2 runtime since contract 6 reads the field, and the poll
/// interval still needs 0.63.0.
#[test]
fn minimax_immutable_paths_retire_the_published_task_minimax_v2_identity() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(
            repo_root().join("components").join("task-minimax-v2").join("manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for retired in ["0.32.0", "0.32.1"] {
        assert_ne!(manifest.version, retired, "a changed package cannot reuse an identity");
    }
    // Published as 0.33.0 with 0.64.0; speech batch V3-1's Q47 bump made it 0.33.1 and
    // provider-vertex's 0.33.2 (`provider_vertex_retires_every_published_065_package_identity`),
    // then the speech Azure OpenAI families' Q47 bump 0.33.3.
    assert_eq!(manifest.version, "0.33.3");
    assert_eq!(manifest.compatibility.south_runtime, declared_runtime_now("task-minimax-v2"));
    assert_eq!(manifest.compatibility.contracts.get("task"), Some(&7));
}

/// Speech world step S-S-5 (speech record §14, §18): `compatibility.json` records `contracts.speech`
/// 1, which includes the `url` artifact form's optional `upgrade_insecure` member (V4, SV-Q10), and
/// every speech package declares it beside `media`. No published runtime (0.64.0 and older) records
/// the key, so a host linking one refuses every speech package; each therefore declares this tree's
/// release, which the release commit moves with the workspace version (the I3-2 /
/// `task-gemini-omni-v2` precedent). The packages are unpublished, so they stay 1.0.0 and declaring
/// the contract re-identifies no other package. A host range without `speech`, or one listing only
/// another number, refuses each package by the `speech` contract; one listing `speech` 1 admits it.
#[test]
fn the_speech_packages_declare_contracts_speech_1_and_this_release() {
    let speech = u32::from(south_contracts::speech::SPEECH_CONTRACT_VERSION);
    assert_eq!(speech, 1);
    let mut without_speech = host_range::host_range();
    assert_eq!(without_speech.contracts.remove("speech"), Some(BTreeSet::from([speech])));
    let mut speech_2_only = host_range::host_range();
    speech_2_only.contracts.insert("speech".to_owned(), BTreeSet::from([2]));
    let mut seen = Vec::new();
    for package in shipped_packages() {
        let manifest: ComponentManifestV1 =
            serde_json::from_str(&std::fs::read_to_string(package.join("manifest.json")).unwrap())
                .unwrap();
        if manifest.api_version != south_provider_api::SPEECH_WORLD {
            assert_eq!(manifest.compatibility.contracts.get("speech"), None, "{}", manifest.name);
            continue;
        }
        let name = manifest.name.clone();
        assert_eq!(manifest.version, speech_version(&name), "{name}");
        assert_eq!(manifest.compatibility.contracts.get("speech"), Some(&speech), "{name}");
        assert_eq!(
            manifest.compatibility.south_runtime, SPEECH_RUNTIME,
            "{name}: declares 0.65.0, the first runtime that records contracts.speech"
        );
        assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()), "{name}");
        for refusing in [&without_speech, &speech_2_only] {
            match compatibility_admits(&manifest, refusing) {
                Err(CompatibilityMismatchV2::Contract { name: contract, declared, .. }) => {
                    assert_eq!((contract.as_str(), declared), ("speech", speech), "{name}");
                }
                other => panic!("{name}: a host without speech 1 admitted it: {other:?}"),
            }
        }
        seen.push(name);
    }
    assert_eq!(
        seen,
        [
            "speech-azure",
            "speech-elevenlabs",
            "speech-groq",
            "speech-mimo",
            "speech-minimax",
            "speech-openai-compatible",
            "speech-qwen",
            "speech-vertex",
            "speech-xai",
        ]
    );
}

/// `speech-vertex` 1.1.0 (V4 record `2026-10-15-speech-v4-tts.md` §3.4; lv's Q7 ruling of
/// 2026-09-30, moved into 0.66.0 by lv on 2026-10-15): `mp3`, `opus`, `aac` and `flac`, which 1.0.x
/// served as WAV as native does, are refused before dispatch. A behaviour change is a minor
/// version (the `provider-kiro` 1.1.0 precedent): the published 1.0.0 identity and the unpublished
/// `provider-vertex` patch 1.0.1 both retire. This release cycle already did its Q47 round, so
/// nothing else moves. The manifest changes nothing but its version, so the runtime stays 0.65.0,
/// the first that records `contracts.speech` 1, and the contracts stay `{media: 1, speech: 1}`.
#[test]
fn speech_vertex_1_1_refuses_q7_and_retires_its_published_identity() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/speech-vertex/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    for retired in ["1.0.0", "1.0.1"] {
        assert_ne!(manifest.version, retired, "a changed package cannot reuse an identity");
    }
    // Published as 1.1.0 with 0.66.0; the speech Azure OpenAI families' Q47 bump made it 1.1.1.
    assert_eq!(manifest.version, "1.1.1");
    assert_eq!(
        manifest.version,
        south_component_conformance::reference_vertex_speech::VERSION,
        "the guest reports the reference's version"
    );
    assert_eq!(manifest.providers, ["vertex-ai-speech"]);
    assert_eq!(manifest.capabilities.iter().collect::<Vec<_>>(), ["synthesize"]);
    assert_eq!(manifest.compatibility.south_runtime, "0.65.0");
    assert_eq!(
        manifest.compatibility.contracts,
        std::collections::BTreeMap::from([("media".to_owned(), 1), ("speech".to_owned(), 1)])
    );
    let mut media_1_only = host_range::host_range();
    media_1_only.contracts.insert("media".to_owned(), BTreeSet::from([1]));
    assert_eq!(compatibility_admits(&manifest, &media_1_only), Ok(()));
}

/// `task-veo-v2` gains the `vertex-veo-video` family (design record
/// `docs/design/2026-10-15-veo-vertex-task-component.md` §4, §7; lv 2026-10-15, VV-Q1 (b)): the
/// native arm's Vertex logic in the same package, with the host-minted token presented as `bearer`
/// beside the Gemini API's `header_secret`. A new family and auth arm change behaviour, so the
/// package takes a minor version past the unpublished `provider-vertex` patch 0.35.24 (the same
/// unreleased cycle, so no further Q47 round); the clip stays an upstream URI, so the task contract stays 7 and
/// a task-7-only host still admits it, and the runtime stays the oldest that admits the manifest.
#[test]
fn task_veo_v2_gains_the_vertex_family_and_bearer() {
    let manifest: ComponentManifestV1 = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("components/task-veo-v2/manifest.json")).unwrap(),
    )
    .unwrap();
    // 0.36.0 was published with 0.66.0; Q-VV1's poll prefix rewrite (lv 2026-10-15) is a
    // behaviour fix in the same family, so the package takes a patch
    // (`veo_task_suite_v2::a_project_number_operation_name_is_polled_under_the_binding_prefix`).
    for retired in ["0.35.23", "0.35.24", "0.36.0"] {
        assert_ne!(manifest.version, retired, "a changed package cannot reuse an identity");
    }
    assert_eq!(manifest.version, "0.36.1");
    assert_eq!(manifest.providers, ["veo-video", "vertex-veo-video"]);
    assert_eq!(manifest.auth_arms.iter().collect::<Vec<_>>(), ["bearer", "header_secret"]);
    assert_eq!(manifest.compatibility.contracts.get("task"), Some(&7));
    assert_eq!(manifest.compatibility.south_runtime, declared_runtime_now("task-veo-v2"));
    assert!(manifest.credentials.is_none(), "the host mints the Vertex token (record §2)");
    assert!(manifest.declared_keys("vertex-veo-video").is_empty(), "no config_schema (record §2)");
    let mut task_7_only = host_range::host_range();
    task_7_only.contracts.insert("task".to_owned(), [7].into());
    assert_eq!(compatibility_admits(&manifest, &task_7_only), Ok(()));
}

/// The speech `azure-openai` families (design record `2026-10-15-speech-azure-openai.md` §8) change
/// `south-component-conformance` (0.65.0 → 0.66.0) after release 0.66.0, which every guest links,
/// so every `component.wasm` changes: each identity published with 0.66.0 retires. Only
/// `speech-openai-compatible` changes behaviour, a minor (1.0.1 → 1.1.0); every other package takes
/// a patch. No package moves its `south_runtime` or its `contracts`.
/// A package, its identity published with 0.66.0, its identity now, its `south_runtime` and its
/// `contracts`.
type RetiredIdentity<'a> = (&'a str, &'a str, &'a str, &'a str, &'a [(&'a str, u32)]);

#[test]
fn speech_azure_openai_retires_every_published_066_package_identity() {
    let rows: [RetiredIdentity<'_>; 40] = [
        ("embeddings-gemini", "1.1.15", "1.1.16", "0.57.0", &[("embeddings", 2)]),
        ("embeddings-openai-compatible", "1.0.18", "1.0.19", "0.57.0", &[("embeddings", 1)]),
        ("embeddings-vertex", "1.0.17", "1.0.18", "0.57.0", &[("embeddings", 1)]),
        ("image-azure", "1.0.14", "1.0.15", "0.57.0", &[("image", 1), ("media", 1)]),
        ("image-gemini", "1.0.8", "1.0.9", "0.59.0", &[("image", 1), ("media", 2)]),
        ("image-ideogram", "1.0.6", "1.0.7", "0.57.0", &[("image", 1), ("media", 1)]),
        ("image-minimax", "1.0.9", "1.0.10", "0.57.0", &[("image", 1), ("media", 1)]),
        ("image-qwen", "1.0.9", "1.0.10", "0.57.0", &[("image", 1), ("media", 1)]),
        ("image-reve", "1.0.0", "1.0.1", "0.57.0", &[("image", 1), ("media", 1)]),
        ("image-stability", "1.0.7", "1.0.8", "0.57.0", &[("image", 1), ("media", 1)]),
        ("image-vertex", "1.0.3", "1.0.4", "0.59.0", &[("image", 1), ("media", 2)]),
        ("image-xai", "1.0.0", "1.0.1", "0.57.0", &[("image", 1), ("media", 1)]),
        ("provider-anthropic", "1.2.0", "1.2.1", "0.60.0", &[]),
        ("provider-anthropic-bedrock-invoke", "1.1.10", "1.1.11", "0.57.0", &[]),
        ("provider-bedrock-converse", "1.1.10", "1.1.11", "0.57.0", &[]),
        ("provider-bedrock-converse-bearer", "1.1.10", "1.1.11", "0.57.0", &[]),
        ("provider-gemini", "1.3.10", "1.3.11", "0.60.0", &[]),
        ("provider-kiro", "1.1.1", "1.1.2", "0.60.0", &[]),
        ("provider-openai-compatible", "2.7.0", "2.7.1", "0.60.0", &[]),
        ("provider-openai-responses", "1.4.0", "1.4.1", "0.60.0", &[]),
        ("provider-vertex", "1.0.0", "1.0.1", "0.60.0", &[]),
        ("speech-azure", "1.0.1", "1.0.2", "0.65.0", &[("media", 1), ("speech", 1)]),
        ("speech-elevenlabs", "1.0.1", "1.0.2", "0.65.0", &[("media", 3), ("speech", 1)]),
        ("speech-groq", "1.0.1", "1.0.2", "0.65.0", &[("media", 1), ("speech", 1)]),
        ("speech-mimo", "1.0.1", "1.0.2", "0.65.0", &[("media", 1), ("speech", 1)]),
        ("speech-minimax", "1.0.1", "1.0.2", "0.65.0", &[("media", 1), ("speech", 1)]),
        ("speech-openai-compatible", "1.0.1", "1.1.0", "0.65.0", &[("media", 1), ("speech", 1)]),
        ("speech-qwen", "1.0.1", "1.0.2", "0.65.0", &[("media", 1), ("speech", 1)]),
        ("speech-vertex", "1.1.0", "1.1.1", "0.65.0", &[("media", 1), ("speech", 1)]),
        ("speech-xai", "1.0.1", "1.0.2", "0.65.0", &[("media", 1), ("speech", 1)]),
        ("task-bailian-v2", "0.31.24", "0.31.25", "0.57.0", &[("task", 7)]),
        ("task-byteplus-v2", "0.36.24", "0.36.25", "0.57.0", &[("task", 7)]),
        ("task-gemini-omni-v2", "0.1.4", "0.1.5", "0.62.0", &[("media", 1), ("task", 8)]),
        ("task-gmi-image-v2", "0.35.24", "0.35.25", "0.57.0", &[("task", 7)]),
        ("task-kling", "1.0.27", "1.0.28", "0.57.0", &[("task", 7)]),
        ("task-kling-v2", "0.32.25", "0.32.26", "0.57.0", &[("task", 7)]),
        ("task-minimax-v2", "0.33.2", "0.33.3", "0.63.0", &[("task", 7)]),
        ("task-veo-v2", "0.36.0", "0.36.1", "0.57.0", &[("task", 7)]),
        ("task-wan-image-v2", "0.35.24", "0.35.25", "0.57.0", &[("task", 7)]),
        ("task-xai-v2", "0.35.24", "0.35.25", "0.57.0", &[("task", 7)]),
    ];
    let shipped: BTreeSet<&str> = rows.iter().map(|row| row.0).collect();
    assert_eq!(
        shipped,
        BTreeSet::from(OFFICIAL_COMPONENTS),
        "every official package takes the round"
    );
    for (name, published, now, runtime, contracts) in rows {
        let manifest: ComponentManifestV1 = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_ne!(manifest.version, published, "{name} reused its published identity");
        assert_eq!(manifest.version, now, "{name}");
        assert_eq!(
            manifest.compatibility.south_runtime, runtime,
            "{name}: the round moves no runtime"
        );
        let contracts: std::collections::BTreeMap<String, u32> =
            contracts.iter().map(|(world, version)| ((*world).to_owned(), *version)).collect();
        assert_eq!(manifest.compatibility.contracts, contracts, "{name}");
    }
}

/// `speech-openai-compatible` 1.1.0 (design record `2026-10-15-speech-azure-openai.md` §3, §7):
/// the families `azure-openai-v1` and `azure-openai-dated` after `openai-compatible`, the
/// `header_secret` arm beside `bearer`, and the dated family's `api_version` key, byte for byte the
/// provider world's. The runtime and the contracts stay: `header_secret`, a key with a `default`
/// and the `api_version_date` syntax all predate the speech world.
#[test]
fn speech_openai_compatible_1_1_adds_the_azure_openai_families() {
    let read = |name: &str| -> serde_json::Value {
        serde_json::from_str(
            &std::fs::read_to_string(
                repo_root().join("components").join(name).join("manifest.json"),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let manifest: ComponentManifestV1 =
        serde_json::from_value(read("speech-openai-compatible")).unwrap();
    for retired in ["1.0.0", "1.0.1", "1.0.2"] {
        assert_ne!(manifest.version, retired, "a changed package cannot reuse an identity");
    }
    assert_eq!(manifest.version, "1.1.0");
    assert_eq!(
        manifest.version,
        south_component_conformance::reference_openai_compatible_speech::VERSION,
        "the guest reports the reference's version"
    );
    assert_eq!(
        manifest.providers.iter().collect::<Vec<_>>(),
        ["openai-compatible", "azure-openai-v1", "azure-openai-dated"]
    );
    assert_eq!(manifest.auth_arms.iter().collect::<Vec<_>>(), ["bearer", "header_secret"]);
    assert!(manifest.declared_keys("openai-compatible").is_empty());
    assert!(manifest.declared_keys("azure-openai-v1").is_empty());
    assert_eq!(manifest.declared_keys("azure-openai-dated"), BTreeSet::from(["api_version"]));
    assert_eq!(
        read("speech-openai-compatible")["config_schema"]["azure-openai-dated"],
        read("provider-openai-compatible")["config_schema"]["azure-openai-dated"]
    );
    assert!(manifest.query_parameters.is_empty());
    assert_eq!(manifest.compatibility.south_runtime, "0.65.0");
    assert_eq!(compatibility_admits(&manifest, &host_range::host_range()), Ok(()));
}
