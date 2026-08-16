use std::{collections::BTreeMap, fs, path::PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityManifest {
    schema_version: u16,
    release: Release,
    contracts: Contracts,
    conformance: Conformance,
    provider_api: ProviderApi,
    provider_runtime: ProviderRuntime,
    crates: BTreeMap<String, String>,
    hosts: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Release {
    version: String,
    stability: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Contracts {
    reserved_header_policy: u16,
    http: u16,
    auth: u16,
    error: u16,
    stream: Option<u16>,
    canonical_ir: Option<String>,
    header_limits: HeaderLimits,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Conformance {
    suite_id: String,
    provider_call_suite: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderLimits {
    count: usize,
    name_bytes: usize,
    value_bytes: usize,
    total_bytes: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderApi {
    wit_version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderRuntime {
    abi_version: Option<String>,
}

#[test]
fn compatibility_manifest_describes_the_library_slice() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compatibility.json");
    let contents = fs::read_to_string(path).unwrap();
    let manifest: CompatibilityManifest = serde_json::from_str(&contents).unwrap();

    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.release.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest.release.stability, "library_slice");
    assert_eq!(
        manifest.contracts.reserved_header_policy,
        south_contracts::RESERVED_HEADER_POLICY_VERSION
    );
    assert_eq!(manifest.contracts.http, south_contracts::HTTP_CONTRACT_VERSION);
    assert_eq!(manifest.contracts.auth, south_contracts::AUTH_CONTRACT_VERSION);
    assert_eq!(manifest.contracts.error, south_contracts::ERROR_CONTRACT_VERSION);
    assert_eq!(manifest.contracts.stream, south_contracts::STREAM_CONTRACT_VERSION);
    assert!(manifest.contracts.canonical_ir.is_none());
    assert_eq!(manifest.contracts.header_limits.count, south_contracts::MAX_PROVIDER_HEADER_COUNT);
    assert_eq!(
        manifest.contracts.header_limits.name_bytes,
        south_contracts::MAX_PROVIDER_HEADER_NAME_BYTES
    );
    assert_eq!(
        manifest.contracts.header_limits.value_bytes,
        south_contracts::MAX_PROVIDER_HEADER_VALUE_BYTES
    );
    assert_eq!(
        manifest.contracts.header_limits.total_bytes,
        south_contracts::MAX_PROVIDER_HEADER_TOTAL_BYTES
    );
    assert_eq!(manifest.conformance.suite_id, "south.provider-call.v1");
    assert_eq!(manifest.conformance.provider_call_suite, 1);
    assert!(manifest.provider_api.wit_version.is_none());
    assert!(manifest.provider_runtime.abi_version.is_none());
    let expected_crates = BTreeMap::from([
        ("south-contracts", "http_auth_error_v1"),
        ("south-core", "buffered_provider_call_v1"),
        ("south-migration", "placeholder"),
        ("south-provider-api", "placeholder"),
        ("south-provider-conformance", "provider_call_suite_v1"),
        ("south-provider-runtime", "placeholder"),
        ("south-testkit", "provider_call_runner_v1"),
        ("south-transport-reqwest", "buffered_json_post_v1"),
        ("south-transport-ureq", "placeholder"),
    ]);
    assert_eq!(manifest.crates.len(), expected_crates.len());
    for (name, expected_status) in expected_crates {
        assert_eq!(
            manifest.crates.get(name).map(String::as_str),
            Some(expected_status),
            "unexpected status for {name}"
        );
    }
    assert_eq!(manifest.hosts.get("token-station").map(String::as_str), Some("not_verified"));
    assert_eq!(
        manifest.hosts.get("token-station-server").map(String::as_str),
        Some("not_verified")
    );
}
