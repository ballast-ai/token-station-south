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
const OFFICIAL_COMPONENTS: [&str; 2] = ["provider-anthropic", "provider-openai-compatible"];

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
