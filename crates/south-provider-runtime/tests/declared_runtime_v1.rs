//! The release check behind the declared-runtime discipline (host-zero-vendor-boundary §13.6,
//! SF10): a package's `compatibility.south_runtime` is the oldest south runtime it needs, so the
//! release proves that runtime really admits it.
//!
//! `scripts/check-declared-runtime.sh` copies this file into a checkout of each declared runtime's
//! release tag and runs it there over the packages that declare that runtime, so the loader doing
//! the judging is the old one, not this tree's. That is why the file is self-contained and uses
//! only API every runtime since the range handshake (0.43.0) has: `HostRangeV1`, `RUNTIME_ABI`,
//! `LoadedComponentV1::load` and `NoSecretsV1`. In this tree it is compiled by every Clippy and test
//! run, which keeps it building against the current API, and ignored, because it needs staged
//! packages.
//!
//! What it proves for each package, under the runtime it declares: gate ① (`gate_manifest`: the
//! manifest parses with that runtime's schema and passes its validation), the range handshake
//! (`compatibility_admits` with that runtime as both the floor and the ceiling), the import scan,
//! and the identity probe (instantiation and `metadata()`). It does not prove that the runtime's
//! link layer accepts every request the package builds; §13.6 says what covers that.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;
use south_provider_api::{HostRangeV1, RUNTIME_ABI};
use south_provider_runtime::{ComponentRuntimeV1, LoadedComponentV1, NoSecretsV1, RuntimeLimitsV1};

/// The directory of staged packages (`<package>/manifest.json` and `<package>/component.wasm`),
/// every one declaring this tree's runtime.
const PACKAGES_VARIABLE: &str = "SOUTH_DECLARED_RUNTIME_PACKAGES";

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

fn as_u32(value: &Value, what: &str) -> u32 {
    value
        .as_u64()
        .and_then(|number| u32::try_from(number).ok())
        .unwrap_or_else(|| panic!("compatibility.json: {what} is not a u32"))
}

/// The range of a host linking exactly this tree's runtime and accepting nothing older: the
/// kernel contracts and the task and embeddings contracts it decodes come from this tree's
/// `compatibility.json`. A runtime older than the embeddings world records no embeddings contract,
/// and this file also runs in checkouts of those runtimes, so that contract is read only where
/// the record has it.
fn this_runtime_only() -> HostRangeV1 {
    let record: Value = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("compatibility.json"))
            .expect("compatibility.json reads"),
    )
    .expect("compatibility.json parses");
    let kernel_contracts = record["kernel_contracts"]
        .as_object()
        .expect("compatibility.json records kernel_contracts")
        .iter()
        .map(|(name, number)| (name.clone(), as_u32(number, name)))
        .collect();
    let mut contracts = BTreeMap::from([(
        "task".to_owned(),
        BTreeSet::from([as_u32(&record["contracts"]["task"], "contracts.task")]),
    )]);
    if let Some(embeddings) = record["contracts"].get("embeddings") {
        contracts.insert(
            "embeddings".to_owned(),
            BTreeSet::from([as_u32(embeddings, "contracts.embeddings")]),
        );
    }
    HostRangeV1 {
        runtime_abi: RUNTIME_ABI,
        south_runtime_min: env!("CARGO_PKG_VERSION").to_owned(),
        south_runtime: env!("CARGO_PKG_VERSION").to_owned(),
        kernel_contracts,
        contracts,
    }
}

#[test]
#[ignore = "needs staged packages; run by scripts/check-declared-runtime.sh"]
fn every_package_loads_under_the_runtime_it_declares() {
    let root = PathBuf::from(
        std::env::var_os(PACKAGES_VARIABLE)
            .unwrap_or_else(|| panic!("{PACKAGES_VARIABLE} names the staged packages")),
    );
    let mut packages: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("{}: {error}", root.display()))
        .map(|entry| entry.expect("the directory entry reads").path())
        .filter(|path| path.is_dir())
        .collect();
    packages.sort();
    assert!(!packages.is_empty(), "{}: no staged package to judge", root.display());

    let runtime = ComponentRuntimeV1::new(RuntimeLimitsV1::default()).expect("engine builds");
    let range = this_runtime_only();
    let mut refused = Vec::new();
    for package in &packages {
        let name = package.file_name().unwrap_or_default().to_string_lossy().into_owned();
        match LoadedComponentV1::load(&runtime, package, &range, NoSecretsV1) {
            Ok(loaded) => {
                eprintln!("{name}: admitted by south runtime {}", env!("CARGO_PKG_VERSION"));
                assert_eq!(loaded.metadata().name, name, "the directory names the package");
            }
            Err(error) => refused.push(format!("{name}: {error}")),
        }
    }
    assert!(
        refused.is_empty(),
        "south runtime {} refuses packages that declare it as their minimum; raise their \
         south_runtime to the release they need, with a version bump:\n{}",
        env!("CARGO_PKG_VERSION"),
        refused.join("\n")
    );
}
