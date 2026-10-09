//! Range admission and per-package isolation (B3,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §8.4, §8.5), against the real
//! `wasm32-wasip2` test guest: one bad package is refused on its own, and a family two packages
//! claim is served by neither unless the operator pins one by digest.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde_json::json;
use south_provider_api::{CompatibilityMismatchV2, HostRangeV1};
use south_provider_runtime::{
    ComponentRuntimeV1, LoadErrorV1, LoadedComponentV1, NoSecretsV1, PackageRefusalV1,
    RuntimeLimitsV1, SecretSignerV1, load_package_set,
};

/// The guest, built once per profile per test process: debug and release give two components
/// with different bytes and the same identity.
fn guest_wasm(release: bool) -> PathBuf {
    static BUILT: OnceLock<()> = OnceLock::new();
    let guest_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/guests/test-provider");
    BUILT.get_or_init(|| {
        // `scripts/prebuild-components.sh` (the nextest setup script) has already built both.
        if std::env::var_os("SOUTH_COMPONENTS_PREBUILT").is_some() {
            return;
        }
        for profile in [
            &["build", "--target", "wasm32-wasip2"][..],
            &["build", "--release", "--target", "wasm32-wasip2"][..],
        ] {
            let status = Command::new("cargo")
                .args(profile)
                .current_dir(&guest_dir)
                .status()
                .expect("cargo is on PATH");
            assert!(status.success(), "the guest must build; `rustup target add wasm32-wasip2`");
        }
    });
    guest_dir.join(format!(
        "target/wasm32-wasip2/{}/test_provider.wasm",
        if release { "release" } else { "debug" }
    ))
}

fn manifest(south_runtime: &str, runtime_abi: Option<u32>) -> String {
    let mut compatibility = json!({
        "ir_schema_id": "token-station-protocol@0.5.0/v0.4.0",
        "kernel_version": "0.4.0",
        "kernel_revision": "8e34f5a089d0b9c7273b49ddb6952dd87e960019",
        "wit_package": "token-station:adapter@2.0.0",
        "south_runtime": south_runtime,
        "kernel_contracts": { "canonical_ir": 3, "error_catalog": 1, "stream": 2 },
    });
    if let Some(abi) = runtime_abi {
        compatibility["runtime_abi"] = json!(abi);
    }
    json!({
        "name": "test-provider",
        "version": "1.0.0",
        "api_version": "provider-adapter-v2",
        "providers": ["test"],
        "capabilities": ["chat", "stream"],
        "auth_arms": ["bearer"],
        "permissions": { "network": false, "filesystem": false, "secrets": ["provider_api_key"] },
        "conformance": { "required_suite": "south.provider-component.v1", "fixtures": "fixtures/" },
        "compatibility": compatibility,
    })
    .to_string()
}

fn host() -> HostRangeV1 {
    HostRangeV1 {
        runtime_abi: 1,
        south_runtime_min: "0.43.0".to_owned(),
        south_runtime: "0.45.0".to_owned(),
        kernel_contracts: BTreeMap::from([
            ("canonical_ir".to_owned(), 3),
            ("error_catalog".to_owned(), 1),
            ("stream".to_owned(), 2),
        ]),
        contracts: BTreeMap::from([("task".to_owned(), BTreeSet::from([7]))]),
    }
}

fn root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("south-package-set-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir is writable");
    dir
}

fn add(root: &Path, name: &str, manifest: &str, wasm: Option<&Path>) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).expect("package dir");
    std::fs::write(dir.join("manifest.json"), manifest).expect("manifest writes");
    if let Some(wasm) = wasm {
        std::fs::copy(wasm, dir.join("component.wasm")).expect("wasm copies");
    }
}

fn runtime() -> ComponentRuntimeV1 {
    ComponentRuntimeV1::new(RuntimeLimitsV1 {
        memory_bytes: 64 * 1024 * 1024,
        call_timeout: Duration::from_millis(500),
        max_payload_bytes: 1024 * 1024,
    })
    .expect("engine builds")
}

fn signer() -> Arc<dyn SecretSignerV1 + Sync> {
    Arc::new(NoSecretsV1)
}

fn sha256_hex(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    Sha256::digest(std::fs::read(path).unwrap()).iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

#[test]
fn the_range_handshake_admits_through_the_existing_loader() {
    let dir = root("single");
    add(&dir, "ok", &manifest("0.44.0", Some(1)), Some(&guest_wasm(false)));
    let loaded = LoadedComponentV1::load(&runtime(), &dir.join("ok"), &host(), NoSecretsV1);
    assert!(loaded.is_ok(), "{:?}", loaded.err());

    add(&dir, "too-new", &manifest("0.46.0", Some(1)), Some(&guest_wasm(false)));
    let refused = LoadedComponentV1::load(&runtime(), &dir.join("too-new"), &host(), NoSecretsV1);
    assert!(matches!(
        refused,
        Err(LoadErrorV1::OutsideRange(CompatibilityMismatchV2::SouthRuntimeAboveHost { .. }))
    ));
}

#[test]
fn one_bad_package_is_refused_alone() {
    let dir = root("isolation");
    add(&dir, "a-good", &manifest("0.43.0", Some(1)), Some(&guest_wasm(false)));
    add(&dir, "b-stale", &manifest("0.42.0", None), Some(&guest_wasm(false)));
    add(&dir, "c-not-wasm", &manifest("0.43.0", Some(1)), None);
    std::fs::write(dir.join("c-not-wasm/component.wasm"), b"not a component").unwrap();
    add(&dir, "d-old-floor", &manifest("0.42.0", Some(1)), Some(&guest_wasm(false)));
    std::fs::create_dir_all(dir.join("e-empty")).unwrap();

    let report = load_package_set(&runtime(), &dir, &host(), &BTreeMap::new(), &signer());
    let admitted: Vec<&str> =
        report.admitted.iter().map(|package| package.directory.as_str()).collect();
    assert_eq!(admitted, ["a-good"]);
    assert_eq!(report.admitted[0].families, ["test"]);
    assert_eq!(report.admitted[0].component_sha256, sha256_hex(&guest_wasm(false)));

    let refused: Vec<(&str, String)> = report
        .refused
        .iter()
        .map(|package| (package.directory.as_str(), package.reason.to_string()))
        .collect();
    assert_eq!(refused.len(), 4, "{refused:#?}");
    assert!(matches!(
        &report.refused[0].reason,
        PackageRefusalV1::Load(LoadErrorV1::OutsideRange(
            CompatibilityMismatchV2::MissingRuntimeAbi
        ))
    ));
    assert!(matches!(
        &report.refused[1].reason,
        PackageRefusalV1::Load(LoadErrorV1::NotAComponent(_))
    ));
    assert!(matches!(
        &report.refused[2].reason,
        PackageRefusalV1::Load(LoadErrorV1::OutsideRange(
            CompatibilityMismatchV2::SouthRuntimeBelowMinimum { .. }
        ))
    ));
    assert!(matches!(
        &report.refused[3].reason,
        PackageRefusalV1::Load(LoadErrorV1::Unreadable { .. })
    ));
}

#[test]
fn a_contested_family_is_served_only_by_the_pinned_package() {
    let dir = root("contested");
    add(&dir, "debug-build", &manifest("0.43.0", Some(1)), Some(&guest_wasm(false)));
    add(&dir, "release-build", &manifest("0.43.0", Some(1)), Some(&guest_wasm(true)));

    // No pin: neither serves the family, whatever the directory order.
    let report = load_package_set(&runtime(), &dir, &host(), &BTreeMap::new(), &signer());
    assert!(report.admitted.is_empty());
    assert_eq!(report.refused.len(), 2);
    assert!(
        report
            .refused
            .iter()
            .all(|package| matches!(package.reason, PackageRefusalV1::EveryFamilyContested))
    );
    assert_eq!(report.contested.len(), 1);
    assert_eq!(report.contested[0].packages, ["debug-build", "release-build"]);
    assert_eq!(report.contested[0].pinned, None);

    // Pinned by digest: the later directory wins because the operator said so.
    let pins = BTreeMap::from([("test".to_owned(), sha256_hex(&guest_wasm(true)))]);
    let report = load_package_set(&runtime(), &dir, &host(), &pins, &signer());
    let admitted: Vec<&str> =
        report.admitted.iter().map(|package| package.directory.as_str()).collect();
    assert_eq!(admitted, ["release-build"]);
    assert_eq!(report.contested[0].pinned.as_deref(), Some("release-build"));

    // A pin that matches two identical packages selects neither.
    add(&dir, "release-copy", &manifest("0.43.0", Some(1)), Some(&guest_wasm(true)));
    let report = load_package_set(&runtime(), &dir, &host(), &pins, &signer());
    assert!(report.admitted.is_empty(), "an ambiguous pin must not fall back to load order");
}
