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
    host_capabilities: BTreeMap<String, BTreeMap<String, HostCapability>>,
}

/// A host's status for one capability, plus the size of the conformance table that status was
/// measured against.
///
/// `cases` exists because `status` alone silently decays. A suite may grow a case that no earlier
/// run could have passed, and a bare `"verified"` string cannot say which table it came from. The
/// freshness assertion in `south-provider-conformance` compares this number against the live
/// fixture table, so evidence that predates a suite extension fails CI instead of waiting for
/// someone to remember it.
///
/// `cases` is absent for a `not_verified` capability: no run happened, so there is no table to
/// name. Writing `0` there would assert a passing run against an empty table, which is a different
/// and false claim.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostCapability {
    status: String,
    #[serde(default)]
    cases: Option<usize>,
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
    header_auth_suite_id: String,
    header_auth_suite: u32,
    provider_quota_metadata_suite_id: String,
    provider_quota_metadata_suite: u32,
    controlled_query_suite_id: String,
    controlled_query_suite: u32,
    controlled_user_agent_suite_id: String,
    controlled_user_agent_suite: u32,
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

/// The expected `(capability, status, cases)` rows per host. `cases` is `None` exactly when the
/// status is not `verified`.
type ExpectedCapability = (&'static str, &'static str, Option<usize>);

fn expected_host_capabilities() -> BTreeMap<&'static str, [ExpectedCapability; 6]> {
    BTreeMap::from([
        (
            "token-station",
            [
                ("provider_call", "verified", Some(7)),
                // Token Station provider_stream verified 2026-08-18: the
                // explicit direct translated Bearer JSON POST canary passes
                // south.provider-stream.v1 9/9, real reqwest and production
                // gateway tests preserve the host-owned stream state machine,
                // and the successful validation CI covers the exact merged
                // Git tree. See the streaming design for immutable evidence
                // and the deliberately narrow scope.
                ("provider_stream", "verified", Some(9)),
                // token-station provider_quota_metadata demoted 2026-08-18. It was
                // flipped against the two-case table, and the suite has since
                // grown `CredentialSlotMismatch` (PR #20) — a case added
                // precisely because two success-path cases could not separate
                // correct wiring from a plausible shortcut. The old evidence is
                // therefore not merely stale but known-insufficient by the
                // reasoning that motivated the new case, so no honest `cases`
                // value exists for it. Re-run the three-case suite against this
                // host to restore the status.
                ("provider_quota_metadata", "not_verified", None),
                // Token Station header_auth verified 2026-08-18: its
                // compatibility-only adapter passes south.header-auth.v1 3/3,
                // real buffered and streaming reqwest loopbacks prove exact
                // sanctioned-header injection without Authorization, and the
                // existing production policy remains Bearer-only. See the
                // Header Auth design for immutable evidence and scope.
                ("header_auth", "verified", Some(3)),
                // controlled_query stays not_verified until this host runs its
                // own adoption slice against south.controlled-query.v1. The
                // suite existing in this repository is not adoption evidence.
                ("controlled_query", "not_verified", None),
                // controlled_user_agent stays not_verified for the same reason.
                ("controlled_user_agent", "not_verified", None),
            ],
        ),
        // token-station-server provider_stream verified 2026-08-17: the durable
        // chat sender's real seam runs streams through
        // open_streaming_provider_call_v1, the host's assembled executor passes
        // south.provider-stream.v1 9/9 with six-field evidence at real
        // boundaries, and an adversarial wiring review (one P1 — proxy-
        // environment fallback — fixed on-branch) plus a maintainer sign-off
        // closed the loop. The adoption record is held by that host's own
        // repository; this manifest records only the resulting status.
        (
            "token-station-server",
            [
                ("provider_call", "verified", Some(7)),
                ("provider_stream", "verified", Some(9)),
                // token-station-server provider_quota_metadata verified
                // 2026-08-18 against the three-case table at v0.5.0. Its
                // assembled executor runs south.provider-quota-metadata.v1 3/3
                // through execute_raw_with with the production one-shot
                // resolver. This replaces an earlier flip attempt that cited a
                // two-case run (closed PR #19): re-pinning that host to v0.5.0
                // failed to compile, because the fixture accessor changed shape
                // to express NotReached, so the run behind this status is a
                // genuinely new one rather than a re-labelled old one.
                //
                // What the new case bought, measured rather than assumed: the
                // mutation reporting literal (1, 1) evidence instead of reading
                // real counters survived the two-case table and now fails on
                // ResolverCallCount and TransportCallCount. What it did not
                // buy: an executor bypassing the host assembly layer to call
                // south-core directly still passes, because the binding check
                // that rejects the mismatched slot lives in south-core, so both
                // paths reach it and report the same failure with the same zero
                // counts. That axis remains a maintainer review item, and the
                // host records it as still-surviving rather than claiming the
                // gap closed.
                ("provider_quota_metadata", "verified", Some(3)),
                // token-station-server header_auth verified 2026-08-18: the
                // host's assembled executor runs south.header-auth.v1 3/3
                // through the same real seam as its two Bearer suites
                // (execute_raw_with / open_streaming_raw_with plus the
                // production one-shot resolver), with both wire-shape booleans
                // measured at the real transport boundary. A mutation check
                // declaring the Bearer arm instead fails the suite with
                // SanctionedHeader and AuthorizationPresence mismatches, so the
                // leg discriminates. An adversarial wiring review found no P1
                // (two P2 and one P3 fixed on-branch) and a maintainer
                // sign-off closed the loop. Scope is deliberately narrow and
                // describes the arm rather than that host's whole surface:
                // Anthropic is the only provider the arm unlocks there, and it
                // routes through south on the chat and responses surfaces only
                // — the messages surface keeps it on the legacy path because
                // that host maps it to an Anthropic response contract while the
                // south plan's guard admits the Chat contract alone. The
                // adoption record is held by that host's own repository; this
                // manifest records only the resulting status.
                ("header_auth", "verified", Some(3)),
                // token-station-server controlled_query verified 2026-08-18,
                // evidence refreshed against 0.4.1 after the suite grew its
                // fifth case. The original run passed the four-case table, but
                // that table could not tell an adapter that measures its wire
                // from one that reports success without looking — and the
                // re-run proved this host was the latter: its executor routed
                // the empty declaration through QueryStringV1::try_from_iter,
                // took the EmptyQuery error as a contract failure, and never
                // reached the transport, so QueryFreeRequestReachesTheWire
                // failed on three categories at once. Fixed on that host's
                // branch; the first verified status was not false so much as
                // unfalsifiable, which is exactly what the new case exists to
                // change.
                //
                // Current evidence: the assembled executor runs
                // south.controlled-query.v1 5/5 through the same real seam as
                // its other suites (execute_raw_with /
                // open_streaming_raw_with plus the production one-shot
                // resolver), and a wire-level test proves the declared query
                // reaches the upstream URL byte for byte with the path
                // untouched. Two mutations now fail that previously survived
                // or did not exist: a probe hardcoding its claim without
                // reading the prepared URL, and dropping the is_some() half of
                // the presence comparison. An adversarial review of the
                // adoption found no P1 beyond an evidence-reporting defect
                // fixed on-branch; two P2 and one P3 were fixed or documented.
                //
                // Scope: the arm unlocks Azure OpenAI and Azure AI Foundry on
                // that host's text and embeddings surfaces, plus Gemini's
                // native chat form, whose streaming ?alt=sse is the controlled
                // query. Gemini's OpenAI-compatible surfaces stay on legacy —
                // they send two auth headers and ProviderAuthV1 is single-arm.
                // The adoption record is held by that host's own repository;
                // this manifest records only the resulting status.
                ("controlled_query", "verified", Some(5)),
                // controlled_user_agent stays not_verified until that host runs
                // its own adoption slice against south.controlled-user-agent.v1
                // (its migration batches 3 and 4b are the expected consumers).
                // The suite existing in this repository is not adoption
                // evidence.
                ("controlled_user_agent", "not_verified", None),
            ],
        ),
    ])
}

// One flat assertion per manifest field is the point of this test: the manifest is a published
// contract, and a reader must be able to check it against the file line by line. Splitting it into
// helpers would hide which fields are actually pinned.
#[allow(clippy::too_many_lines)]
#[test]
fn compatibility_manifest_describes_the_library_slice() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compatibility.json");
    let contents = fs::read_to_string(path).unwrap();
    let manifest: CompatibilityManifest = serde_json::from_str(&contents).unwrap();

    assert_eq!(manifest.schema_version, 3);
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
    assert_eq!(manifest.conformance.header_auth_suite_id, "south.header-auth.v1");
    assert_eq!(manifest.conformance.header_auth_suite, 1);
    assert_eq!(
        manifest.conformance.provider_quota_metadata_suite_id,
        "south.provider-quota-metadata.v1"
    );
    assert_eq!(manifest.conformance.provider_quota_metadata_suite, 1);
    assert_eq!(manifest.conformance.controlled_query_suite_id, "south.controlled-query.v1");
    assert_eq!(manifest.conformance.controlled_query_suite, 1);
    assert_eq!(
        manifest.conformance.controlled_user_agent_suite_id,
        "south.controlled-user-agent.v1"
    );
    assert_eq!(manifest.conformance.controlled_user_agent_suite, 1);
    assert_eq!(manifest.provider_api.wit_version.as_deref(), Some("token-station:adapter@2.0.0"));
    assert!(manifest.provider_runtime.abi_version.is_none());
    let expected_crates = BTreeMap::from([
        (
            "south-contracts",
            "http_auth_error_stream_quota_metadata_header_auth_controlled_query_user_agent_v1",
        ),
        (
            "south-core",
            "buffered_streaming_provider_call_header_auth_controlled_query_user_agent_raw_prelude_v1",
        ),
        ("south-provider-api", "provider_adapter_v2_wit_manifest_v1"),
        (
            "south-provider-conformance",
            "provider_call_stream_quota_metadata_header_auth_controlled_query_user_agent_suites_v1",
        ),
        ("south-provider-runtime", "placeholder"),
        (
            "south-testkit",
            "provider_call_stream_quota_metadata_header_auth_controlled_query_user_agent_runners_raw_builder_v1",
        ),
        (
            "south-transport-reqwest",
            "buffered_streaming_json_post_quota_metadata_header_auth_user_agent_transport_pair_v1",
        ),
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
    // adversarial wiring review of the reported evidence. The adoption record is
    // held by that host's own repository; this manifest records only the status.
    assert_eq!(manifest.hosts.get("token-station-server").map(String::as_str), Some("verified"));
    // Per-capability annotations disambiguate the legacy per-host string: the
    // top-level `hosts` value mirrors `provider_call` (the first adopted
    // capability). Every newer capability is recorded independently and may
    // become verified only through its own adoption evidence. A capability
    // listed here must have a conformance suite in this manifest.
    let expected_capabilities = expected_host_capabilities();
    assert_eq!(manifest.host_capabilities.len(), expected_capabilities.len());
    for (host, capabilities) in expected_capabilities {
        let annotated = manifest.host_capabilities.get(host).unwrap_or_else(|| {
            panic!("missing host_capabilities entry for {host}");
        });
        assert_eq!(annotated.len(), capabilities.len(), "unexpected capability set for {host}");
        for (capability, expected_status, expected_cases) in capabilities {
            let annotation = annotated.get(capability).unwrap_or_else(|| {
                panic!("missing {host}/{capability} annotation");
            });
            assert_eq!(
                annotation.status, expected_status,
                "unexpected status for {host}/{capability}"
            );
            assert_eq!(
                annotation.cases, expected_cases,
                "unexpected case count for {host}/{capability}"
            );
            // `cases` records a passing run, so it is present exactly when one happened.
            assert_eq!(
                annotation.cases.is_some(),
                annotation.status == "verified",
                "{host}/{capability} must carry `cases` if and only if it is verified"
            );
        }
        // The legacy summary string must stay consistent with provider_call.
        assert_eq!(
            manifest.hosts.get(host).map(String::as_str),
            annotated.get("provider_call").map(|capability| capability.status.as_str()),
            "legacy host summary diverged from provider_call for {host}"
        );
    }
}
