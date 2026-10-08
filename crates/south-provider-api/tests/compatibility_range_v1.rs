//! The range handshake (B3, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §8.3, §8.7): every
//! bound refuses on the side it guards, and nothing outside the declared items decides.

// Each case edits one field of a fixture; a plain assignment says which more clearly than
// `clone_into`.
#![allow(clippy::assigning_clones)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use south_provider_api::{
    CompatibilityMismatchV2, ComponentManifestV1, HostRangeV1, RUNTIME_ABI, compatibility_admits,
};

fn manifest() -> ComponentManifestV1 {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../components/task-kling-v2/manifest.json");
    let mut manifest: ComponentManifestV1 =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    manifest.compatibility.south_runtime = "0.44.0".to_owned();
    manifest
}

fn host() -> HostRangeV1 {
    HostRangeV1 {
        runtime_abi: RUNTIME_ABI,
        south_runtime_min: "0.43.0".to_owned(),
        south_runtime: "0.45.0".to_owned(),
        kernel_contracts: BTreeMap::from([
            ("canonical_ir".to_owned(), 3),
            ("error_catalog".to_owned(), 1),
            ("stream".to_owned(), 2),
        ]),
        contracts: BTreeMap::from([("task".to_owned(), BTreeSet::from([6, 7]))]),
    }
}

fn refused(edit: impl Fn(&mut ComponentManifestV1, &mut HostRangeV1)) -> CompatibilityMismatchV2 {
    let (mut manifest, mut host) = (manifest(), host());
    edit(&mut manifest, &mut host);
    compatibility_admits(&manifest, &host).expect_err("the edit must be refused")
}

#[test]
fn a_component_inside_the_range_is_admitted_at_both_bounds() {
    assert_eq!(compatibility_admits(&manifest(), &host()), Ok(()));
    for south_runtime in ["0.43.0", "0.45.0"] {
        let mut at_bound = manifest();
        at_bound.compatibility.south_runtime = south_runtime.to_owned();
        assert_eq!(compatibility_admits(&at_bound, &host()), Ok(()), "{south_runtime}");
    }
    // Provenance does not decide: a different kernel revision or IR id is still admitted.
    let mut provenance = manifest();
    provenance.compatibility.kernel_revision = "0".repeat(40);
    provenance.compatibility.ir_schema_id = "token-station-protocol@0.5.1/v0.4.1".to_owned();
    assert_eq!(compatibility_admits(&provenance, &host()), Ok(()));
}

#[test]
fn every_bound_refuses_on_its_own_side() {
    assert_eq!(
        refused(|m, _| m.compatibility.runtime_abi = None),
        CompatibilityMismatchV2::MissingRuntimeAbi
    );
    assert!(matches!(
        refused(|m, _| m.compatibility.runtime_abi = Some(RUNTIME_ABI + 1)),
        CompatibilityMismatchV2::RuntimeAbi { .. }
    ));
    assert!(matches!(
        refused(|m, _| m.compatibility.south_runtime = "0.42.9".to_owned()),
        CompatibilityMismatchV2::SouthRuntimeBelowMinimum { .. }
    ));
    assert!(matches!(
        refused(|m, _| m.compatibility.south_runtime = "0.45.1".to_owned()),
        CompatibilityMismatchV2::SouthRuntimeAboveHost { .. }
    ));
    // Compared as numbers, not text: 0.100.0 is newer than 0.45.0.
    assert!(matches!(
        refused(|m, _| m.compatibility.south_runtime = "0.100.0".to_owned()),
        CompatibilityMismatchV2::SouthRuntimeAboveHost { .. }
    ));
    assert!(matches!(
        refused(|m, _| m.compatibility.south_runtime = "0.44".to_owned()),
        CompatibilityMismatchV2::InvalidSouthRuntime(_)
    ));
}

#[test]
fn kernel_contracts_must_match_exactly_in_both_directions() {
    let differs = refused(|m, _| {
        m.compatibility.kernel_contracts.insert("stream".to_owned(), 3);
    });
    assert!(
        matches!(differs, CompatibilityMismatchV2::KernelContract { ref name, .. } if name == "stream")
    );
    let missing = refused(|m, _| {
        m.compatibility.kernel_contracts.remove("error_catalog");
    });
    assert!(matches!(missing, CompatibilityMismatchV2::KernelContract { declared: None, .. }));
    let extra = refused(|m, _| {
        m.compatibility.kernel_contracts.insert("router_config".to_owned(), 1);
    });
    assert!(matches!(extra, CompatibilityMismatchV2::KernelContract { expected: None, .. }));
}

#[test]
fn a_contract_version_must_be_one_the_host_decodes() {
    assert!(matches!(
        refused(|_, h| {
            h.contracts.insert("task".to_owned(), BTreeSet::from([8]));
        }),
        CompatibilityMismatchV2::Contract { declared: 7, .. }
    ));
    assert!(matches!(
        refused(|_, h| h.contracts.clear()),
        CompatibilityMismatchV2::Contract { ref accepted, .. } if accepted.is_empty()
    ));
}
