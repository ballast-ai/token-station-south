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
    host_capabilities: BTreeMap<String, BTreeMap<String, String>>,
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
    provider_quota_metadata: u16,
    canonical_ir: Option<String>,
    header_limits: HeaderLimits,
    provider_quota_metadata_limits: ProviderQuotaMetadataLimits,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Conformance {
    suite_id: String,
    provider_call_suite: u32,
    provider_stream_suite_id: String,
    provider_stream_suite: u32,
    provider_quota_metadata_suite_id: String,
    provider_quota_metadata_suite: u32,
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
struct ProviderQuotaMetadataLimits {
    field_count: usize,
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

    assert_eq!(manifest.schema_version, 2);
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
    assert_eq!(
        manifest.contracts.provider_quota_metadata,
        south_contracts::PROVIDER_QUOTA_METADATA_CONTRACT_VERSION
    );
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
    assert_eq!(
        manifest.contracts.provider_quota_metadata_limits.field_count,
        south_contracts::PROVIDER_QUOTA_METADATA_FIELD_COUNT
    );
    assert_eq!(
        manifest.contracts.provider_quota_metadata_limits.value_bytes,
        south_contracts::MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES
    );
    assert_eq!(
        manifest.contracts.provider_quota_metadata_limits.total_bytes,
        south_contracts::MAX_PROVIDER_QUOTA_METADATA_TOTAL_BYTES
    );
    assert_eq!(manifest.conformance.suite_id, "south.provider-call.v1");
    assert_eq!(manifest.conformance.provider_call_suite, 1);
    assert_eq!(manifest.conformance.provider_stream_suite_id, "south.provider-stream.v1");
    assert_eq!(manifest.conformance.provider_stream_suite, 1);
    assert_eq!(
        manifest.conformance.provider_quota_metadata_suite_id,
        "south.provider-quota-metadata.v1"
    );
    assert_eq!(manifest.conformance.provider_quota_metadata_suite, 1);
    assert!(manifest.provider_api.wit_version.is_none());
    assert!(manifest.provider_runtime.abi_version.is_none());
    let expected_crates = BTreeMap::from([
        ("south-contracts", "http_auth_error_stream_quota_metadata_v1"),
        ("south-core", "buffered_streaming_provider_call_v1"),
        ("south-migration", "placeholder"),
        ("south-provider-api", "placeholder"),
        ("south-provider-conformance", "provider_call_stream_quota_metadata_suites_v1"),
        ("south-provider-runtime", "placeholder"),
        ("south-testkit", "provider_call_stream_quota_metadata_runners_v1"),
        ("south-transport-reqwest", "buffered_streaming_json_post_quota_metadata_v1"),
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
    // token-station verified 2026-08-17 for provider_call only: its non-test
    // diagnostic adapter functions, wrapped by CommunityConformanceExecutorV1, passed
    // south.provider-call.v1 7/7; real resolver/transport wiring received
    // adversarial review; and the final host PR CI passed at 63a3ceb. See the
    // community-host compatibility release design for the evidence and scope.
    assert_eq!(manifest.hosts.get("token-station").map(String::as_str), Some("verified"));
    // token-station-server verified 2026-08-17: real adapter at the embeddings
    // Bearer JSON POST call site, assembled-executor conformance 7/7, and an
    // adversarial wiring review of the reported evidence. See the enterprise
    // repo's adoption record (product-review #34) for the evidence trail.
    assert_eq!(manifest.hosts.get("token-station-server").map(String::as_str), Some("verified"));
    // Per-capability annotations disambiguate the legacy per-host string: the
    // top-level `hosts` value mirrors `provider_call` (the first adopted
    // capability). Every newer capability is recorded independently and may
    // become verified only through its own adoption evidence. A capability
    // listed here must have a conformance suite in this manifest.
    let expected_capabilities: BTreeMap<&str, [&str; 3]> = BTreeMap::from([
        ("token-station", ["provider_call", "provider_stream", "provider_quota_metadata"]),
        ("token-station-server", ["provider_call", "provider_stream", "provider_quota_metadata"]),
    ]);
    assert_eq!(manifest.host_capabilities.len(), expected_capabilities.len());
    for (host, capabilities) in expected_capabilities {
        let annotated = manifest.host_capabilities.get(host).unwrap_or_else(|| {
            panic!("missing host_capabilities entry for {host}");
        });
        assert_eq!(annotated.len(), capabilities.len(), "unexpected capability set for {host}");
        for capability in capabilities {
            let actual_status = annotated.get(capability).map(String::as_str);
            assert!(
                matches!(actual_status, Some("verified" | "not_verified")),
                "invalid status for {host}/{capability}"
            );
        }
        // The legacy summary string must stay consistent with provider_call.
        assert_eq!(
            manifest.hosts.get(host),
            annotated.get("provider_call"),
            "legacy host summary diverged from provider_call for {host}"
        );
    }
}
