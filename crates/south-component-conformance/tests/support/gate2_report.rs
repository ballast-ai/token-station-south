//! The gate ② report a release publishes beside each package (host-zero-vendor-boundary §9.2).
//!
//! Each sandbox parity test runs gate ② against the built `component.wasm`. This module turns that
//! run into a small JSON record naming the exact bytes it judged, so the release index can carry
//! `gate2_report_sha256` for a package and a host can show which report its component passed.
//!
//! The record is written only when `SOUTH_GATE2_REPORT_DIR` is set (release CI sets it); otherwise
//! the record is still built, so the code path runs in every CI run, and then dropped.
//!
//! Binding to the bytes: the component's digest is taken before the sandbox loads it and again
//! after the suite ran, and the two must agree, so the recorded digest is the digest of the bytes
//! the suite judged. `scripts/release_index.py` then refuses a report whose digest differs from
//! the `component.wasm` inside the archive.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use south_component_conformance::ReportV1;

/// The schema id of the record this module writes.
const SCHEMA: &str = "south.gate2-report.v1";

/// The environment variable naming the directory the record is written into.
const REPORT_DIR_ENV: &str = "SOUTH_GATE2_REPORT_DIR";

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

fn sha256_of(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    Sha256::digest(&bytes).iter().fold(String::with_capacity(64), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// The package's identity and digests, taken before the sandbox loads the component.
pub struct Evidence {
    package: String,
    wasm: PathBuf,
    manifest: Value,
    manifest_sha256: String,
    component_sha256: String,
}

impl Evidence {
    /// `package` is the directory under `components/`; `wasm` is the built component.
    pub fn capture(package: &str, wasm: &Path) -> Self {
        let manifest_path = repo_root().join("components").join(package).join("manifest.json");
        let source = std::fs::read_to_string(&manifest_path).expect("the shipped manifest reads");
        let manifest: Value = serde_json::from_str(&source).expect("the shipped manifest parses");
        assert_eq!(manifest["name"], package, "the package directory names the package");
        Self {
            package: package.to_owned(),
            wasm: wasm.to_owned(),
            manifest,
            manifest_sha256: sha256_of(&manifest_path),
            component_sha256: sha256_of(wasm),
        }
    }

    /// Builds the record for `report`, and writes it when `SOUTH_GATE2_REPORT_DIR` is set.
    ///
    /// Call it before asserting the report passes, so a failing run also leaves its record.
    pub fn record(&self, report: &ReportV1) {
        assert_eq!(
            sha256_of(&self.wasm),
            self.component_sha256,
            "{}: component.wasm changed while gate ② ran; the report would not name the bytes it \
             judged",
            self.package
        );
        assert_eq!(
            self.manifest["conformance"]["required_suite"],
            report.suite(),
            "{}: the suite that ran is not the one the manifest requires",
            self.package
        );

        let mut outcomes: Vec<(&str, &str, &str)> = report
            .outcomes()
            .iter()
            .map(|outcome| (outcome.check.as_str(), outcome.case.as_str(), outcome.detail()))
            .collect();
        // Sorted, so the record's bytes do not depend on the order the suite visited its cases.
        outcomes.sort_unstable();
        let outcomes: Vec<Value> = outcomes
            .into_iter()
            .map(|(check, case, detail)| {
                if detail.is_empty() {
                    json!({ "check": check, "case": case, "verdict": "passed" })
                } else {
                    json!({ "check": check, "case": case, "verdict": "failed", "detail": detail })
                }
            })
            .collect();
        let failed = report.failures().count();

        let record = json!({
            "schema": SCHEMA,
            "suite": report.suite(),
            "south_release": crate::host_range::south_release(),
            "name": self.manifest["name"],
            "version": self.manifest["version"],
            "world": self.manifest["api_version"],
            "manifest_sha256": self.manifest_sha256,
            "component_sha256": self.component_sha256,
            "passed": report.is_passing(),
            "checks": outcomes.len(),
            "failed": failed,
            "outcomes": outcomes,
        });

        if let Some(dir) = std::env::var_os(REPORT_DIR_ENV) {
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).expect("the report directory is creatable");
            let mut text = serde_json::to_string_pretty(&record).expect("the record serializes");
            text.push('\n');
            let path = dir.join(format!("{}.gate2.json", self.package));
            std::fs::write(&path, text)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        }
    }
}
