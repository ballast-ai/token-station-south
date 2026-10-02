//! Loading a directory of packages, each judged on its own (B3,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §8.5).
//!
//! A host used to aggregate per-package load errors into one process failure, so a single stale
//! package took down every family. Here each package independently passes gate ①, the range
//! handshake, the import scan and the identity probe; the report lists what was admitted and what
//! was refused and why, and never fails as a whole.
//!
//! Two rules travel with it:
//!
//! - **No fallback.** A refused or missing package makes only its own families unavailable. The
//!   native reference implementations are gate ②'s judges, not stand-ins (§16 Q8, ruled as
//!   recommended).
//! - **No tie broken by load order.** When two admitted packages of one world declare the same
//!   family, that family is unavailable from both unless the operator pins one package for it by
//!   its `component.wasm` digest.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use south_provider_api::HostRangeV1;

use crate::component::{LoadedComponentV1, SecretSignerV1};
use crate::loader::{
    HostCompatibilityV1, LoadErrorV1, MAX_COMPONENT_BYTES, MAX_MANIFEST_BYTES, UnreadableReasonV1,
    gate_manifest, parse_package, read_file_limited,
};
use crate::runtime::ComponentRuntimeV1;

/// One admitted package.
pub struct AdmittedPackageV1 {
    /// The package's directory name under the root.
    pub directory: String,
    /// Lowercase hex SHA-256 of `component.wasm`, the identity an operator pins.
    pub component_sha256: String,
    /// The families this package serves: its declared providers, minus any contested family it
    /// was not pinned for.
    pub families: Vec<String>,
    pub component: LoadedComponentV1,
}

/// Why one package was refused.
#[derive(Debug)]
pub enum PackageRefusalV1 {
    /// It failed a load gate.
    Load(LoadErrorV1),
    /// Every family it declares is claimed by another admitted package of the same world, and the
    /// operator pinned none of them to this package.
    EveryFamilyContested,
}

impl std::fmt::Display for PackageRefusalV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(error) => error.fmt(f),
            Self::EveryFamilyContested => f.write_str(
                "every family this package declares is also declared by another package; pin one \
                 by digest",
            ),
        }
    }
}

/// One refused package.
#[derive(Debug)]
pub struct RefusedPackageV1 {
    pub directory: String,
    pub reason: PackageRefusalV1,
}

/// A family two or more admitted packages of one world declare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestedFamilyV1 {
    pub world: String,
    pub family: String,
    /// The claimants' directories, sorted.
    pub packages: Vec<String>,
    /// The directory the operator's pin selected, if the pin matched a claimant.
    pub pinned: Option<String>,
}

/// The outcome of loading a package directory.
#[derive(Default)]
pub struct PackageSetReportV1 {
    /// Sorted by directory.
    pub admitted: Vec<AdmittedPackageV1>,
    /// Sorted by directory.
    pub refused: Vec<RefusedPackageV1>,
    /// Sorted by world, then family.
    pub contested: Vec<ContestedFamilyV1>,
}

/// Loads every package directory directly under `root`, each on its own.
///
/// `pins` maps a family to the `component.wasm` digest (lowercase hex SHA-256) of the package that
/// should serve it when more than one declares it. `signer` is shared by every admitted package.
/// A `root` that cannot be listed yields an empty report with no packages; the host decides what an
/// empty set means.
#[must_use]
pub fn load_package_set(
    runtime: &ComponentRuntimeV1,
    root: &Path,
    host: &HostRangeV1,
    pins: &BTreeMap<String, String>,
    signer: &Arc<dyn SecretSignerV1 + Sync>,
) -> PackageSetReportV1 {
    let mut directories: Vec<(String, PathBuf)> = std::fs::read_dir(root)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry.path())))
                .collect()
        })
        .unwrap_or_default();
    directories.sort();

    let mut report = PackageSetReportV1::default();
    let mut loaded = Vec::new();
    for (directory, path) in directories {
        match load_one(runtime, &path, host, signer) {
            Ok((component, component_sha256)) => {
                loaded.push((directory, component, component_sha256));
            }
            Err(error) => {
                report
                    .refused
                    .push(RefusedPackageV1 { directory, reason: PackageRefusalV1::Load(error) });
            }
        }
    }

    // Who claims what, per world.
    let mut claims: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (index, (_, component, _)) in loaded.iter().enumerate() {
        let manifest = component.manifest();
        for family in &manifest.providers {
            claims.entry((manifest.api_version.clone(), family.clone())).or_default().push(index);
        }
    }
    let mut withheld: Vec<Vec<String>> = vec![Vec::new(); loaded.len()];
    for ((world, family), claimants) in
        claims.into_iter().filter(|(_, claimants)| claimants.len() > 1)
    {
        // A pin selects a claimant only when exactly one has that digest; anything else would
        // fall back to load order.
        let pinned = pins.get(&family).and_then(|digest| {
            let mut matching =
                claimants.iter().copied().filter(|&index| loaded[index].2 == *digest);
            matching.next().filter(|_| matching.next().is_none())
        });
        for &index in &claimants {
            if Some(index) != pinned {
                withheld[index].push(family.clone());
            }
        }
        report.contested.push(ContestedFamilyV1 {
            world,
            family,
            packages: claimants.iter().map(|&index| loaded[index].0.clone()).collect(),
            pinned: pinned.map(|index| loaded[index].0.clone()),
        });
    }

    for ((directory, component, component_sha256), withheld) in loaded.into_iter().zip(withheld) {
        let families: Vec<String> = component
            .manifest()
            .providers
            .iter()
            .filter(|family| !withheld.contains(family))
            .cloned()
            .collect();
        if families.is_empty() {
            report.refused.push(RefusedPackageV1 {
                directory,
                reason: PackageRefusalV1::EveryFamilyContested,
            });
        } else {
            report.admitted.push(AdmittedPackageV1 {
                directory,
                component_sha256,
                families,
                component,
            });
        }
    }
    report.refused.sort_by(|left, right| left.directory.cmp(&right.directory));
    report
}

/// Gate ①, the range handshake, the import scan and the identity probe for one package directory,
/// and the digest of the bytes that passed them.
fn load_one(
    runtime: &ComponentRuntimeV1,
    dir: &Path,
    host: &HostRangeV1,
    signer: &Arc<dyn SecretSignerV1 + Sync>,
) -> Result<(LoadedComponentV1, String), LoadErrorV1> {
    let manifest_path = dir.join("manifest.json");
    let manifest_bytes = read_file_limited(&manifest_path, MAX_MANIFEST_BYTES)
        .map_err(|reason| LoadErrorV1::Unreadable { path: manifest_path.clone(), reason })?;
    let manifest_source = String::from_utf8(manifest_bytes).map_err(|error| {
        LoadErrorV1::Unreadable { path: manifest_path, reason: UnreadableReasonV1::NotUtf8(error) }
    })?;
    let wasm_path = dir.join("component.wasm");
    // The manifest is judged first, so a package built for another host is refused before its
    // bytes are read — the same order as `read_package`.
    host.admit(&gate_manifest(&manifest_source)?)?;
    let wasm = read_file_limited(&wasm_path, MAX_COMPONENT_BYTES)
        .map_err(|reason| LoadErrorV1::Unreadable { path: wasm_path, reason })?;
    let (manifest, component) = parse_package(runtime, &manifest_source, &wasm, host)?;
    let loaded = LoadedComponentV1::admit(runtime, manifest, component, Arc::clone(signer))?;
    Ok((loaded, hex_sha256(&wasm)))
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}
