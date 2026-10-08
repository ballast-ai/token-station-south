//! How a test host linking this south release admits the shipped packages
//! (host-zero-vendor-boundary §8.3, §8.6).
//!
//! A package whose content does not change keeps its identity and its `south_runtime` across
//! releases, so that field may lag the workspace version. A test that admits a shipped package with
//! an exact tuple pinned to `env!("CARGO_PKG_VERSION")` would turn red on the first release where a
//! package keeps an older runtime, and force the re-stamp the range handshake exists to remove.
//! Tests that load a shipped package therefore admit it the way a host does: through
//! [`host_range`].
//!
//! The exact handshake (`HostExpectationsV1`) stays supported for one release. Tests whose purpose
//! is that handshake use [`exact_expectations_for`], which takes the runtime from the manifest's
//! own declaration instead of from the workspace version.

#![allow(dead_code, reason = "each test binary mounting this module uses a different subset")]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;
use south_provider_api::{ComponentManifestV1, HostExpectationsV1, HostRangeV1};

/// The oldest `south_runtime` this test host admits: 0.43.0, the release that introduced the range
/// handshake.
///
/// This is a floor policy of this test host, not a south rule. Every host chooses its own
/// `south_runtime_min`, and raises it when it wants a release that tightened gate ① or ② enforced
/// (§8.6). This one admits every package built for the range handshake at all; no earlier release
/// wrote the fields that handshake reads.
pub const SOUTH_RUNTIME_FLOOR: &str = "0.43.0";

/// The IR, kernel release and kernel revision this release distributes. The range handshake records
/// them for provenance only; the exact handshake still compares them.
const IR_SCHEMA_ID: &str = "token-station-protocol@0.5.0/v0.4.0";
const KERNEL_VERSION: &str = "0.4.0";
const KERNEL_REVISION: &str = "8e34f5a089d0b9c7273b49ddb6952dd87e960019";

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

fn as_u32(value: &Value, what: &str) -> u32 {
    value
        .as_u64()
        .and_then(|number| u32::try_from(number).ok())
        .unwrap_or_else(|| panic!("compatibility.json: {what} is not a u32"))
}

/// The range a host linking this south release accepts.
///
/// `runtime_abi` and `kernel_contracts` are read from `compatibility.json`, the record this release
/// publishes, so the test host and the published record cannot drift apart. `contracts` holds the
/// task and embeddings contracts this release's codecs decode, and `south_runtime` is the release
/// itself.
pub fn host_range() -> HostRangeV1 {
    let source = std::fs::read_to_string(repo_root().join("compatibility.json"))
        .expect("compatibility.json reads");
    let record: Value = serde_json::from_str(&source).expect("compatibility.json parses");
    let kernel_contracts = record["kernel_contracts"]
        .as_object()
        .expect("compatibility.json records kernel_contracts")
        .iter()
        .map(|(name, number)| (name.clone(), as_u32(number, name)))
        .collect();
    HostRangeV1 {
        runtime_abi: as_u32(&record["runtime_abi"], "runtime_abi"),
        south_runtime_min: SOUTH_RUNTIME_FLOOR.to_owned(),
        south_runtime: env!("CARGO_PKG_VERSION").to_owned(),
        kernel_contracts,
        contracts: BTreeMap::from([
            (
                "task".to_owned(),
                BTreeSet::from([u32::from(south_contracts::TASK_CONTRACT_VERSION)]),
            ),
            // EMBEDDINGS_CONTRACT_VERSION, added with the contract types
            ("embeddings".to_owned(), BTreeSet::from([1])),
        ]),
    }
}

/// The exact tuple of a host that accepts `manifest`'s own runtime declaration.
///
/// For tests whose purpose is the exact handshake: the IR and kernel values are this release's,
/// but `south_runtime` is the one the manifest declares, so the test does not depend on the
/// workspace version equalling it.
pub fn exact_expectations_for(manifest: &ComponentManifestV1) -> HostExpectationsV1 {
    HostExpectationsV1 {
        ir_schema_id: IR_SCHEMA_ID.to_owned(),
        kernel_version: KERNEL_VERSION.to_owned(),
        kernel_revision: KERNEL_REVISION.to_owned(),
        south_runtime: manifest.compatibility.south_runtime.clone(),
    }
}

/// `major.minor.patch` as numbers, so `0.10.0` sorts above `0.9.0`.
pub fn triple(version: &str) -> (u64, u64, u64) {
    let parts: Vec<u64> = version
        .split('.')
        .map(|part| part.parse().unwrap_or_else(|_| panic!("`{version}` is not numeric")))
        .collect();
    let [major, minor, patch] = parts[..] else {
        panic!("`{version}` is not a major.minor.patch triple")
    };
    (major, minor, patch)
}

/// The shipped-package rule for `south_runtime` (as in `shipped_packages_v1`): it may lag the
/// workspace version but never exceed it, and it may not fall below this host's floor.
pub fn assert_released_runtime(manifest: &ComponentManifestV1) {
    let declared = &manifest.compatibility.south_runtime;
    assert!(
        triple(declared) <= triple(env!("CARGO_PKG_VERSION")),
        "{}: declares south runtime {declared}, newer than this release",
        manifest.name
    );
    assert!(
        triple(declared) >= triple(SOUTH_RUNTIME_FLOOR),
        "{}: declares south runtime {declared}, older than this host's floor {SOUTH_RUNTIME_FLOOR}",
        manifest.name
    );
}
