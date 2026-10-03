//! How a test host linking this south release admits the shipped task packages
//! (host-zero-vendor-boundary §8.3, §8.6).
//!
//! A package whose content does not change keeps its `south_runtime` across releases, so a test
//! that loads a shipped package must not pin that field to the workspace version. This is the
//! runtime crate's copy of `south-component-conformance/tests/support/host_range.rs`, reduced to
//! what these tests use. This crate does not depend on `south-contracts`, so the task contract is
//! read from `compatibility.json`, whose `contracts.task` the `compatibility_manifest` test in
//! `south-contracts` pins to `TASK_CONTRACT_VERSION`.

#![allow(dead_code, reason = "each test binary mounting this module uses a different subset")]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;
use south_provider_api::{ComponentManifestV1, HostExpectationsV1, HostRangeV1};

/// The oldest `south_runtime` this test host admits: 0.42.0, the release that introduced the range
/// handshake. A floor policy of this test host, not a south rule: every host chooses its own
/// `south_runtime_min` (§8.6).
pub const SOUTH_RUNTIME_FLOOR: &str = "0.42.0";

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

fn as_u32(value: &Value, what: &str) -> u32 {
    value
        .as_u64()
        .and_then(|number| u32::try_from(number).ok())
        .unwrap_or_else(|| panic!("compatibility.json: {what} is not a u32"))
}

/// The range a host linking this south release accepts, read from `compatibility.json`.
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
        contracts: BTreeMap::from([(
            "task".to_owned(),
            BTreeSet::from([as_u32(&record["contracts"]["task"], "contracts.task")]),
        )]),
    }
}

/// The exact tuple of a host that accepts `manifest`'s own runtime declaration, for tests whose
/// purpose is the exact handshake.
pub fn exact_expectations_for(manifest: &str) -> HostExpectationsV1 {
    let manifest: ComponentManifestV1 = serde_json::from_str(manifest).expect("manifest parses");
    HostExpectationsV1 {
        ir_schema_id: "token-station-protocol@0.4.0/v0.3.0".to_owned(),
        kernel_version: "0.3.0".to_owned(),
        kernel_revision: "6822aab1dea54ef646cb2206595cd4955ff9764a".to_owned(),
        south_runtime: manifest.compatibility.south_runtime,
    }
}
