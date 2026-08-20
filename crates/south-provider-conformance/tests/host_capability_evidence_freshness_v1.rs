//! Guards `compatibility.json` against silently decaying evidence.
//!
//! A `verified` host capability records that some host passed a conformance suite. But suites grow:
//! `controlled_query` gained a fifth case and `provider_quota_metadata` a third, each added because
//! the frozen table could not separate correct wiring from a plausible shortcut. The status string
//! does not move when that happens, so a reader sees `verified` and reasonably assumes it means the
//! table as it stands today.
//!
//! The manifest therefore records the size of the table each status was measured against, and this
//! test compares that number against the live fixture tables. Extending a suite without re-running
//! a host now fails CI, which turns evidence freshness from something a person has to remember into
//! something the build states.
//!
//! This lives beside the fixtures rather than beside the rest of the manifest assertions because
//! only this crate can see the tables. `south-provider-conformance` depends on `south-contracts`,
//! so the reverse edge needed to read `provider_call_fixtures_v1()` from the manifest test would
//! close a dependency cycle.

use std::{collections::BTreeMap, fs, path::PathBuf};

use south_provider_conformance::{
    controlled_query_fixtures_v1, controlled_user_agent_fixtures_v1, header_auth_fixtures_v1,
    provider_call_fixtures_v1, provider_quota_metadata_fixtures_v1, provider_stream_fixtures_v1,
};

/// Every capability the manifest may annotate.
///
/// This is an enum rather than a string lookup so that adding a suite fails to compile until its
/// case table is wired into `case_count`. A capability that silently fell through to a default
/// would be exactly the blind spot this file exists to remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CapabilityV1 {
    ProviderCall,
    ProviderStream,
    HeaderAuth,
    ControlledQuery,
    ControlledUserAgent,
    ProviderQuotaMetadata,
}

impl CapabilityV1 {
    /// The manifest key for this capability.
    const fn manifest_key(self) -> &'static str {
        match self {
            Self::ProviderCall => "provider_call",
            Self::ProviderStream => "provider_stream",
            Self::HeaderAuth => "header_auth",
            Self::ControlledQuery => "controlled_query",
            Self::ControlledUserAgent => "controlled_user_agent",
            Self::ProviderQuotaMetadata => "provider_quota_metadata",
        }
    }

    /// The number of cases in this capability's live conformance table.
    const fn case_count(self) -> usize {
        match self {
            Self::ProviderCall => provider_call_fixtures_v1().len(),
            Self::ProviderStream => provider_stream_fixtures_v1().len(),
            Self::HeaderAuth => header_auth_fixtures_v1().len(),
            Self::ControlledQuery => controlled_query_fixtures_v1().len(),
            Self::ControlledUserAgent => controlled_user_agent_fixtures_v1().len(),
            Self::ProviderQuotaMetadata => provider_quota_metadata_fixtures_v1().len(),
        }
    }

    /// Every capability, so the test can prove the manifest annotates exactly this set.
    const fn all() -> [Self; 6] {
        [
            Self::ProviderCall,
            Self::ProviderStream,
            Self::HeaderAuth,
            Self::ControlledQuery,
            Self::ControlledUserAgent,
            Self::ProviderQuotaMetadata,
        ]
    }
}

/// Reads the manifest's `host_capabilities` as `host -> capability -> (status, cases)`.
fn read_host_capabilities() -> BTreeMap<String, BTreeMap<String, (String, Option<u64>)>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compatibility.json");
    let contents = fs::read_to_string(path).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&contents).unwrap();
    let hosts = manifest["host_capabilities"].as_object().unwrap();

    hosts
        .iter()
        .map(|(host, capabilities)| {
            let annotations = capabilities
                .as_object()
                .unwrap_or_else(|| panic!("{host} host_capabilities must be an object"))
                .iter()
                .map(|(capability, annotation)| {
                    let status = annotation["status"]
                        .as_str()
                        .unwrap_or_else(|| panic!("{host}/{capability} needs a status string"))
                        .to_owned();
                    let cases = annotation.get("cases").map(|cases| {
                        cases
                            .as_u64()
                            .unwrap_or_else(|| panic!("{host}/{capability} cases must be a number"))
                    });
                    (capability.clone(), (status, cases))
                })
                .collect();
            (host.clone(), annotations)
        })
        .collect()
}

/// The assertion this file exists for: a `verified` status must name the table it actually ran.
#[test]
fn verified_host_capabilities_cite_the_current_case_tables() {
    let host_capabilities = read_host_capabilities();
    assert!(!host_capabilities.is_empty(), "the manifest must annotate at least one host");

    for (host, annotations) in host_capabilities {
        for capability in CapabilityV1::all() {
            let key = capability.manifest_key();
            let (status, cases) = annotations
                .get(key)
                .unwrap_or_else(|| panic!("{host} is missing a {key} annotation"));

            match status.as_str() {
                "verified" => {
                    let cited = cases.unwrap_or_else(|| {
                        panic!("{host}/{key} is verified but does not say how many cases it ran")
                    });
                    let current = capability.case_count() as u64;
                    assert_eq!(
                        cited, current,
                        "{host}/{key} was verified against {cited} cases but \
                         south.{key}.v1 now has {current}. The suite grew after that run, so the \
                         status no longer describes the current table: re-run the host against the \
                         {current}-case suite and update `cases`, or record it as not_verified. \
                         Editing `cases` to match without re-running fabricates the evidence."
                    );
                }
                "not_verified" => assert!(
                    cases.is_none(),
                    "{host}/{key} is not_verified, so it has no passing run to cite"
                ),
                other => panic!("{host}/{key} has an unknown status {other:?}"),
            }
        }

        // Nothing may be annotated that this test does not know how to check, or a capability could
        // be added to the manifest with no suite behind it.
        let known: Vec<&str> = CapabilityV1::all().iter().map(|c| c.manifest_key()).collect();
        for capability in annotations.keys() {
            assert!(
                known.contains(&capability.as_str()),
                "{host} annotates unknown capability {capability}"
            );
        }
    }
}
