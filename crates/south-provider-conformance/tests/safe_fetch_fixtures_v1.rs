use std::{
    fmt::Display,
    net::{IpAddr, SocketAddr},
};

use south_contracts::{
    MAX_BINARY_RESPONSE_BODY_BYTES,
    media::{ArtifactUrlV1, is_forbidden_egress_address},
};
use south_provider_conformance::{
    ProviderCallCountV1, SAFE_FETCH_CONFORMANCE_SUITE_ID, SAFE_FETCH_CONFORMANCE_SUITE_VERSION,
    SafeFetchCaseIdV1, SafeFetchExpectedOutcomeV1, SafeFetchFailureCodeV1, SafeFetchFixtureV1,
    SafeFetchResolutionV1, SafeFetchUpstreamBodyV1, safe_fetch_fixtures_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(SafeFetchFixtureV1: Display);
assert_not_impl_any!(south_provider_conformance::SafeFetchInputV1: Display);
assert_not_impl_any!(south_provider_conformance::SafeFetchEnvironmentV1: Display);
assert_not_impl_any!(south_provider_conformance::SafeFetchExpectedV1: Display);
assert_not_impl_any!(SafeFetchFailureCodeV1: Display);

const SENTINELS: &[&str] = &[
    "user-debug-sentinel",
    "password-debug-sentinel",
    "signature-debug-sentinel",
    "not-found-body-debug-sentinel",
    "cdn.example.com",
    "proxy.example.com",
];

fn fixture(case_id: SafeFetchCaseIdV1) -> &'static SafeFetchFixtureV1 {
    safe_fetch_fixtures_v1()
        .iter()
        .find(|fixture| fixture.case_id() == case_id)
        .expect("every case id has a fixture")
}

fn first_cdn_answer(fixture: &SafeFetchFixtureV1) -> &'static [IpAddr] {
    let entry = fixture.environment().dns()[0];
    match entry.resolution() {
        SafeFetchResolutionV1::Addresses(addresses) => addresses,
        SafeFetchResolutionV1::Rebinding { first, .. } => first,
        SafeFetchResolutionV1::Stall => &[],
    }
}

#[test]
fn suite_identity_and_canonical_case_order_are_frozen() {
    use SafeFetchCaseIdV1 as Case;
    assert_eq!(SAFE_FETCH_CONFORMANCE_SUITE_VERSION, 1);
    assert_eq!(SAFE_FETCH_CONFORMANCE_SUITE_ID, "south.safe-fetch.v1");

    let case_ids: Vec<_> =
        safe_fetch_fixtures_v1().iter().map(SafeFetchFixtureV1::case_id).collect();
    assert_eq!(
        case_ids,
        [
            Case::HttpRefused,
            Case::UserinfoRefused,
            Case::LocalhostRefused,
            Case::LoopbackLiteralRefused,
            Case::MetadataLiteralRefused,
            Case::ResolvesToPrivateRefused,
            Case::AnyResolvedAddressForbiddenRefused,
            Case::V4MappedRefused,
            Case::V4CompatibleRefused,
            Case::Nat64Refused,
            Case::SixToFourRefused,
            Case::SiteLocalRefused,
            Case::NoAddressesRefused,
            Case::ResolutionStallTimesOut,
            Case::RedirectToInternalRefused,
            Case::RedirectToPublicRefused,
            Case::NonSuccessStatusRefused,
            Case::PublicFetchSucceeds,
            Case::ConnectionPinnedToCheckedAddress,
            Case::SystemProxyIgnored,
            Case::DeclaredMediaTypeWins,
            Case::HostLimitExactSucceeds,
            Case::HostLimitExceededRefused,
            Case::BinaryCapBelowHostLimitRefused,
            Case::EmptyBodyRefused,
            Case::BodyStallTimesOut,
        ]
    );
    let mut sorted = case_ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted, case_ids, "the table is in declaration order with no duplicate");
}

/// The URL-grammar rows expect exactly what the pure parse says, and every other row's URL
/// parses — so the table and `ArtifactUrlV1::parse` cannot drift apart.
#[test]
fn url_rows_agree_with_the_pure_parse() {
    for fixture in safe_fetch_fixtures_v1() {
        let parsed = ArtifactUrlV1::parse(fixture.input().url());
        let case = fixture.case_id();
        match fixture.expected().evidence().resolver_calls() {
            ProviderCallCountV1::Zero => {
                let error = parsed.expect_err("a row that never resolves is refused by the parse");
                assert_eq!(
                    fixture.expected().outcome(),
                    &SafeFetchExpectedOutcomeV1::Failure {
                        code: SafeFetchFailureCodeV1::from_artifact_url_error(error)
                    },
                    "case {case:?}"
                );
            }
            ProviderCallCountV1::One => {
                let url = parsed.expect("a row that resolves has a valid URL");
                assert_eq!(url.host_str(), fixture.wire_target().server_name(), "case {case:?}");
                assert!(
                    url.as_str().ends_with(fixture.wire_target().path_and_query()),
                    "case {case:?}"
                );
            }
            ProviderCallCountV1::MoreThanOne => panic!("no row expects a second resolution"),
        }
    }
}

/// D8b's address vectors, as AAAA or A answers: each refused row's answer contains a forbidden
/// address, and every row that connects connects to an allowed one.
#[test]
fn address_rows_carry_the_record_vectors() {
    use SafeFetchCaseIdV1 as Case;
    let vectors = [
        (Case::ResolvesToPrivateRefused, "10.0.0.1"),
        (Case::AnyResolvedAddressForbiddenRefused, "192.168.1.1"),
        (Case::V4MappedRefused, "::ffff:10.0.0.1"),
        (Case::V4CompatibleRefused, "::10.0.0.1"),
        (Case::Nat64Refused, "64:ff9b::a00:1"),
        (Case::SixToFourRefused, "2002:a00:1::1"),
        (Case::SiteLocalRefused, "fec0::1"),
    ];
    for (case, vector) in vectors {
        let answer = first_cdn_answer(fixture(case));
        let vector: IpAddr = vector.parse().expect("a vector parses");
        assert!(answer.contains(&vector), "case {case:?}");
        assert!(is_forbidden_egress_address(vector), "case {case:?}");
        assert_eq!(
            fixture(case).expected().outcome(),
            &SafeFetchExpectedOutcomeV1::Failure { code: SafeFetchFailureCodeV1::ForbiddenAddress }
        );
    }
    // The mixed row puts the public address first, so a first-answer-only check is caught.
    let mixed = first_cdn_answer(fixture(Case::AnyResolvedAddressForbiddenRefused));
    assert_eq!(mixed.len(), 2);
    assert!(!is_forbidden_egress_address(mixed[0]));

    // The literal rows: refused by the parse, no resolution.
    for (case, host) in [
        (Case::LoopbackLiteralRefused, "127.0.0.1"),
        (Case::MetadataLiteralRefused, "169.254.169.254"),
    ] {
        assert!(fixture(case).input().url().starts_with(&format!("https://{host}/")));
        assert_eq!(fixture(case).expected().evidence().resolver_calls(), ProviderCallCountV1::Zero);
    }

    for fixture in safe_fetch_fixtures_v1() {
        if let Some(address) = fixture.expected().evidence().connected_to() {
            assert!(!is_forbidden_egress_address(address.ip()), "case {:?}", fixture.case_id());
            assert_eq!(address.port(), 443);
        }
    }
}

/// The rebinding row answers public first and private after; the proxy row's environment names a
/// proxy the resolver can resolve, so an executor that honours it really can route through it.
#[test]
fn pinning_and_proxy_rows_are_live_traps() {
    let pinned = fixture(SafeFetchCaseIdV1::ConnectionPinnedToCheckedAddress);
    let SafeFetchResolutionV1::Rebinding { first, later } =
        pinned.environment().dns()[0].resolution()
    else {
        panic!("the pinning row rebinds");
    };
    assert!(first.iter().all(|address| !is_forbidden_egress_address(*address)));
    assert!(later.iter().all(|address| is_forbidden_egress_address(*address)));
    assert_eq!(pinned.expected().evidence().connected_to(), Some(SocketAddr::new(first[0], 443)));

    let proxied = fixture(SafeFetchCaseIdV1::SystemProxyIgnored);
    let proxy = proxied.environment().system_proxy().expect("the proxy row configures a proxy");
    assert!(
        proxied
            .environment()
            .dns()
            .iter()
            .any(|entry| proxy.contains(entry.name()) && entry.name() != "cdn.example.com")
    );
    for fixture in safe_fetch_fixtures_v1() {
        if fixture.case_id() != SafeFetchCaseIdV1::SystemProxyIgnored {
            assert_eq!(fixture.environment().system_proxy(), None);
        }
    }
}

/// Every refused destination still answers if connected to, so skipping a check produces an
/// artifact rather than a transport failure that would hide it.
#[test]
fn refused_destinations_answer_if_reached() {
    for fixture in safe_fetch_fixtures_v1() {
        let target = fixture.wire_target().server_name();
        assert!(
            fixture.environment().servers().iter().any(|server| server.server_name() == target),
            "case {:?}",
            fixture.case_id()
        );
    }
}

#[test]
fn limit_rows_straddle_their_limits() {
    let chunks_total = |fixture: &SafeFetchFixtureV1| {
        let server = fixture.environment().servers()[0];
        match server.response().body() {
            SafeFetchUpstreamBodyV1::Chunks(chunks) => (
                chunks.len(),
                chunks.iter().map(|chunk| chunk.len()).sum::<usize>(),
                chunks.concat(),
            ),
            other => panic!("unexpected body {other:?}"),
        }
    };

    let exact = fixture(SafeFetchCaseIdV1::HostLimitExactSucceeds);
    let (count, total, bytes) = chunks_total(exact);
    assert_eq!(count, 2);
    assert_eq!(total, exact.input().host_byte_limit());
    let SafeFetchExpectedOutcomeV1::Artifact { body, upstream_content_type, .. } =
        exact.expected().outcome()
    else {
        panic!("the exact row succeeds");
    };
    assert_eq!(*body, bytes.as_slice());
    assert_eq!(*upstream_content_type, None);

    let exceeded = fixture(SafeFetchCaseIdV1::HostLimitExceededRefused);
    let (count, total, _) = chunks_total(exceeded);
    assert_eq!(count, 2);
    assert_eq!(total, exceeded.input().host_byte_limit() + 1);

    let cap = fixture(SafeFetchCaseIdV1::BinaryCapBelowHostLimitRefused);
    assert!(cap.input().host_byte_limit() > MAX_BINARY_RESPONSE_BODY_BYTES);
    let SafeFetchUpstreamBodyV1::Repeated { total_bytes, .. } =
        cap.environment().servers()[0].response().body()
    else {
        panic!("the cap row repeats");
    };
    assert_eq!(total_bytes, MAX_BINARY_RESPONSE_BODY_BYTES + 1);

    let (_, total, _) = chunks_total(fixture(SafeFetchCaseIdV1::EmptyBodyRefused));
    assert_eq!(total, 0);
}

#[test]
fn every_artifact_row_declares_png_and_the_media_type_row_disagrees_upstream() {
    for fixture in safe_fetch_fixtures_v1() {
        if let SafeFetchExpectedOutcomeV1::Artifact { media_type, .. } =
            fixture.expected().outcome()
        {
            assert_eq!(*media_type, fixture.input().declared_media_type());
        }
    }
    let row = fixture(SafeFetchCaseIdV1::DeclaredMediaTypeWins);
    let upstream = row.environment().servers()[0].response().content_type();
    assert_ne!(upstream, Some(row.input().declared_media_type()));
}

#[test]
fn stall_rows_have_short_timeouts() {
    for case in [SafeFetchCaseIdV1::ResolutionStallTimesOut, SafeFetchCaseIdV1::BodyStallTimesOut] {
        let row = fixture(case);
        assert!(row.input().total_timeout().as_secs() <= 1, "case {case:?}");
        assert_eq!(
            row.expected().outcome(),
            &SafeFetchExpectedOutcomeV1::Failure { code: SafeFetchFailureCodeV1::Timeout }
        );
    }
}

#[test]
fn diagnostics_never_echo_urls_names_or_bodies() {
    for fixture in safe_fetch_fixtures_v1() {
        let debug = format!("{fixture:?}");
        for sentinel in SENTINELS {
            assert!(!debug.contains(sentinel), "{:?} echoed {sentinel}", fixture.case_id());
        }
    }
}
