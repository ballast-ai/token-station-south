//! The runner judged against deliberately broken executors.
//!
//! [`FaultyExecutor`] is the reference algorithm with one switch thrown at a time — the shapes a
//! real host executor gets wrong. Each must fail exactly the rows that switch is about and no
//! other, which proves both that every row bites and that no row is noise.

use std::{
    cmp::min,
    collections::BTreeSet,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use south_contracts::{
    MAX_BINARY_RESPONSE_BODY_BYTES,
    media::{ArtifactUrlV1, is_forbidden_egress_address},
};
use south_provider_conformance::{
    SAFE_FETCH_CONFORMANCE_SUITE_ID, SAFE_FETCH_CONFORMANCE_SUITE_VERSION, SafeFetchCaseIdV1,
    SafeFetchExpectedOutcomeV1, SafeFetchFailureCodeV1, SafeFetchFixtureV1, SafeFetchInputV1,
    safe_fetch_fixtures_v1,
};
use south_testkit::{
    MAX_SAFE_FETCH_MISMATCHES_V1, SafeFetchArtifactV1, SafeFetchExecutorV1, SafeFetchFutureV1,
    SafeFetchMismatchCategoryV1, SafeFetchPortsV1, SafeFetchWireRequestV1, SafeFetchWireResponseV1,
    run_safe_fetch_conformance_v1,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    None,
    FollowsRedirects,
    ReResolvesToConnect,
    SkipsEmbeddedV4Check,
    ChecksFirstAddressOnly,
    HonoursSystemProxy,
    SendsCredentials,
    HostLimitOnly,
    AdoptsUpstreamMediaType,
    DropsUpstreamContentType,
    CorruptsBody,
    AcceptsEmptyBody,
    NoTimeout,
}

struct FaultyExecutor(Fault);

impl SafeFetchExecutorV1 for FaultyExecutor {
    fn fetch<'a>(
        &'a self,
        input: &'a SafeFetchInputV1,
        ports: &'a SafeFetchPortsV1,
    ) -> SafeFetchFutureV1<'a> {
        Box::pin(async move {
            if self.0 == Fault::NoTimeout {
                return faulty_fetch(self.0, input, ports).await;
            }
            tokio::time::timeout(input.total_timeout(), faulty_fetch(self.0, input, ports))
                .await
                .unwrap_or(Err(SafeFetchFailureCodeV1::Timeout))
        })
    }
}

/// Whether an IPv6 address is one of the IPv4-embedding forms.
fn embeds_v4(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    (segments[..5] == [0; 5] && (segments[5] == 0xffff || segments[5] == 0))
        || (segments[0] == 0x64 && segments[1] == 0xff9b)
        || segments[0] == 0x2002
}

/// The range check most hosts started with: every range but the embedded-IPv4 recheck.
fn forbidden_without_embedding(address: IpAddr) -> bool {
    match address {
        IpAddr::V6(v6) if embeds_v4(v6) => false,
        _ => is_forbidden_egress_address(address),
    }
}

fn literal_address(host: &str) -> Option<IpAddr> {
    host.strip_prefix('[').and_then(|host| host.strip_suffix(']')).unwrap_or(host).parse().ok()
}

fn path_and_query(url: &ArtifactUrlV1) -> String {
    let text = url.as_str();
    let after_scheme = text.strip_prefix("https://").unwrap_or(text);
    let target = after_scheme.find('/').map_or("/", |start| &after_scheme[start..]);
    target.split('#').next().unwrap_or(target).to_owned()
}

async fn resolve(
    ports: &SafeFetchPortsV1,
    host: &str,
) -> Result<Vec<IpAddr>, SafeFetchFailureCodeV1> {
    match literal_address(host) {
        Some(address) => Ok(vec![address]),
        None => ports.resolve(host).await.map_err(|_| SafeFetchFailureCodeV1::ResolutionFailed),
    }
}

async fn faulty_fetch(
    fault: Fault,
    input: &SafeFetchInputV1,
    ports: &SafeFetchPortsV1,
) -> Result<SafeFetchArtifactV1, SafeFetchFailureCodeV1> {
    let mut current = input.url().to_owned();
    for _hop in 0..3 {
        let url = ArtifactUrlV1::parse(&current)
            .map_err(SafeFetchFailureCodeV1::from_artifact_url_error)?;
        let host = url.host_str().to_owned();
        let addresses = resolve(ports, &host).await?;
        let Some(&first) = addresses.first() else {
            return Err(SafeFetchFailureCodeV1::ResolutionFailed);
        };
        let forbidden = |address: IpAddr| {
            if fault == Fault::SkipsEmbeddedV4Check {
                forbidden_without_embedding(address)
            } else {
                is_forbidden_egress_address(address)
            }
        };
        let refused = if fault == Fault::ChecksFirstAddressOnly {
            forbidden(first)
        } else {
            addresses.iter().copied().any(forbidden)
        };
        if refused {
            return Err(SafeFetchFailureCodeV1::ForbiddenAddress);
        }

        let connect_to = if fault == Fault::ReResolvesToConnect {
            // Checked one answer, connects by name: the connector resolves again.
            let again = resolve(ports, &host).await?;
            let address = again.first().ok_or(SafeFetchFailureCodeV1::ResolutionFailed)?;
            SocketAddr::new(*address, url.port())
        } else {
            SocketAddr::new(first, url.port())
        };
        let connect_to = match (fault, ports.system_proxy()) {
            (Fault::HonoursSystemProxy, Some(proxy)) => {
                let authority = proxy.strip_prefix("http://").unwrap_or(proxy);
                let (proxy_host, proxy_port) =
                    authority.split_once(':').unwrap_or((authority, "80"));
                let proxy_address = *resolve(ports, proxy_host)
                    .await?
                    .first()
                    .ok_or(SafeFetchFailureCodeV1::ResolutionFailed)?;
                SocketAddr::new(proxy_address, proxy_port.parse().unwrap_or(80))
            }
            _ => connect_to,
        };

        let mut headers = vec![("accept".to_owned(), input.declared_media_type().to_owned())];
        if fault == Fault::SendsCredentials {
            headers.push(("authorization".to_owned(), "Bearer fake-credential".to_owned()));
            headers.push(("cookie".to_owned(), "session=fake-cookie".to_owned()));
        }
        let request = SafeFetchWireRequestV1::new(host, path_and_query(&url), headers);
        let response = ports
            .exchange(connect_to, request)
            .await
            .map_err(|_| SafeFetchFailureCodeV1::TransportFailed)?;
        match response.status() {
            200..=299 => {}
            300..=399 => {
                if fault == Fault::FollowsRedirects
                    && let Some(location) = response.location()
                {
                    location.clone_into(&mut current);
                    continue;
                }
                return Err(SafeFetchFailureCodeV1::Redirect);
            }
            _ => return Err(SafeFetchFailureCodeV1::UpstreamStatus),
        }

        return read_artifact(fault, input, response).await;
    }
    Err(SafeFetchFailureCodeV1::Redirect)
}

/// Reads the body of a 2xx exchange into an artifact.
async fn read_artifact(
    fault: Fault,
    input: &SafeFetchInputV1,
    mut response: SafeFetchWireResponseV1,
) -> Result<SafeFetchArtifactV1, SafeFetchFailureCodeV1> {
    let limit = if fault == Fault::HostLimitOnly {
        input.host_byte_limit()
    } else {
        min(input.host_byte_limit(), MAX_BINARY_RESPONSE_BODY_BYTES)
    };
    let mut body = Vec::new();
    while let Some(chunk) = response.body_mut().next_chunk().await {
        if chunk.len() > limit - body.len() {
            return Err(SafeFetchFailureCodeV1::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    if body.is_empty() && fault != Fault::AcceptsEmptyBody {
        return Err(SafeFetchFailureCodeV1::EmptyBody);
    }
    if fault == Fault::CorruptsBody
        && let Some(byte) = body.first_mut()
    {
        *byte ^= 0xff;
    }
    let media_type = match (fault, response.content_type()) {
        (Fault::AdoptsUpstreamMediaType, Some(upstream)) => upstream.to_owned(),
        _ => input.declared_media_type().to_owned(),
    };
    let upstream_content_type = if fault == Fault::DropsUpstreamContentType {
        None
    } else {
        response.content_type().map(str::to_owned)
    };
    Ok(SafeFetchArtifactV1::new(media_type, body, upstream_content_type))
}

type Expected = BTreeSet<(SafeFetchCaseIdV1, SafeFetchMismatchCategoryV1)>;

fn rows_where(
    predicate: impl Fn(&SafeFetchFixtureV1) -> bool,
    category: SafeFetchMismatchCategoryV1,
) -> Expected {
    safe_fetch_fixtures_v1()
        .iter()
        .filter(|fixture| predicate(fixture))
        .map(|fixture| (fixture.case_id(), category))
        .collect()
}

const fn connects(fixture: &SafeFetchFixtureV1) -> bool {
    fixture.expected().evidence().connected_to().is_some()
}

const fn fetches_artifact(fixture: &SafeFetchFixtureV1) -> bool {
    matches!(fixture.expected().outcome(), SafeFetchExpectedOutcomeV1::Artifact { .. })
}

const fn records_upstream_content_type(fixture: &SafeFetchFixtureV1) -> bool {
    matches!(
        fixture.expected().outcome(),
        SafeFetchExpectedOutcomeV1::Artifact { upstream_content_type: Some(_), .. }
    )
}

async fn mismatches_of(fault: Fault) -> Expected {
    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        run_safe_fetch_conformance_v1(&FaultyExecutor(fault)),
    )
    .await
    .expect("safe fetch conformance watchdog expired");
    match outcome {
        Ok(report) => {
            assert_eq!(report.passed_case_ids().len(), safe_fetch_fixtures_v1().len());
            Expected::new()
        }
        Err(failure) => {
            assert_eq!(failure.suite_id(), SAFE_FETCH_CONFORMANCE_SUITE_ID);
            assert_eq!(failure.suite_version(), SAFE_FETCH_CONFORMANCE_SUITE_VERSION);
            assert_eq!(failure.evaluated_case_count(), safe_fetch_fixtures_v1().len());
            assert!(failure.mismatches().len() <= MAX_SAFE_FETCH_MISMATCHES_V1);
            let set: Expected = failure
                .mismatches()
                .iter()
                .map(|mismatch| (mismatch.case_id(), mismatch.category()))
                .collect();
            assert_eq!(set.len(), failure.mismatches().len(), "no mismatch is reported twice");
            set
        }
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_unbroken_executor_passes_every_row() {
    let executor = FaultyExecutor(Fault::None);
    let dynamic: &dyn SafeFetchExecutorV1 = &executor;
    let ports = SafeFetchPortsV1::new(&safe_fetch_fixtures_v1()[0]);
    let future = dynamic.fetch(safe_fetch_fixtures_v1()[0].input(), &ports);
    assert_send(&future);
    drop(future);

    assert_eq!(mismatches_of(Fault::None).await, Expected::new());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn each_fault_fails_exactly_its_rows() {
    use SafeFetchCaseIdV1 as Case;
    use SafeFetchMismatchCategoryV1 as Cat;

    let pairs = |rows: &[(Case, Cat)]| rows.iter().copied().collect::<Expected>();
    let mut re_resolves = rows_where(connects, Cat::ResolverCallCount);
    re_resolves.insert((Case::ConnectionPinnedToCheckedAddress, Cat::ConnectedAddresses));

    let expectations = [
        (
            Fault::FollowsRedirects,
            // Following to the internal address and then refusing it reaches the right verdict
            // for the wrong reason; following to a public mirror succeeds outright.
            pairs(&[
                (Case::RedirectToInternalRefused, Cat::ErrorCode),
                (Case::RedirectToPublicRefused, Cat::OutcomeKind),
                (Case::RedirectToPublicRefused, Cat::ResolverCallCount),
                (Case::RedirectToPublicRefused, Cat::ConnectedAddresses),
                (Case::RedirectToPublicRefused, Cat::WireTarget),
            ]),
        ),
        (Fault::ReResolvesToConnect, re_resolves),
        (
            Fault::SkipsEmbeddedV4Check,
            pairs(&[
                (Case::V4MappedRefused, Cat::OutcomeKind),
                (Case::V4MappedRefused, Cat::ConnectedAddresses),
                (Case::V4CompatibleRefused, Cat::OutcomeKind),
                (Case::V4CompatibleRefused, Cat::ConnectedAddresses),
                (Case::Nat64Refused, Cat::OutcomeKind),
                (Case::Nat64Refused, Cat::ConnectedAddresses),
                (Case::SixToFourRefused, Cat::OutcomeKind),
                (Case::SixToFourRefused, Cat::ConnectedAddresses),
            ]),
        ),
        (
            Fault::ChecksFirstAddressOnly,
            pairs(&[
                (Case::AnyResolvedAddressForbiddenRefused, Cat::OutcomeKind),
                (Case::AnyResolvedAddressForbiddenRefused, Cat::ConnectedAddresses),
            ]),
        ),
        (
            Fault::HonoursSystemProxy,
            pairs(&[
                (Case::SystemProxyIgnored, Cat::ResolverCallCount),
                (Case::SystemProxyIgnored, Cat::ConnectedAddresses),
            ]),
        ),
        (Fault::SendsCredentials, rows_where(connects, Cat::WireHeaders)),
        (Fault::HostLimitOnly, pairs(&[(Case::BinaryCapBelowHostLimitRefused, Cat::OutcomeKind)])),
        (Fault::AdoptsUpstreamMediaType, pairs(&[(Case::DeclaredMediaTypeWins, Cat::MediaType)])),
        (
            Fault::DropsUpstreamContentType,
            rows_where(records_upstream_content_type, Cat::UpstreamContentType),
        ),
        (Fault::CorruptsBody, rows_where(fetches_artifact, Cat::Body)),
        (Fault::AcceptsEmptyBody, pairs(&[(Case::EmptyBodyRefused, Cat::OutcomeKind)])),
        (
            Fault::NoTimeout,
            pairs(&[
                (Case::ResolutionStallTimesOut, Cat::Deadline),
                (Case::BodyStallTimesOut, Cat::Deadline),
            ]),
        ),
    ];

    let mut categories_exercised = BTreeSet::new();
    for (fault, expected) in expectations {
        assert!(!expected.is_empty(), "{fault:?} must fail some row");
        let observed = mismatches_of(fault).await;
        assert_eq!(observed, expected, "{fault:?}");
        categories_exercised.extend(observed.iter().map(|(_, category)| *category));
    }

    // Every closed category is reachable by some realistic fault.
    assert_eq!(
        categories_exercised,
        [
            Cat::OutcomeKind,
            Cat::ErrorCode,
            Cat::Body,
            Cat::MediaType,
            Cat::UpstreamContentType,
            Cat::Deadline,
            Cat::ResolverCallCount,
            Cat::ConnectedAddresses,
            Cat::WireHeaders,
            Cat::WireTarget,
        ]
        .into_iter()
        .collect()
    );
}

/// The pinning row reaches a second resolution only through a re-resolving executor, and the
/// address it then connects to is the private one the resolver rebinds to.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn rebinding_is_what_the_pinning_row_measures() {
    let fixture = safe_fetch_fixtures_v1()
        .iter()
        .find(|fixture| fixture.case_id() == SafeFetchCaseIdV1::ConnectionPinnedToCheckedAddress)
        .expect("the pinning row exists");
    let ports = SafeFetchPortsV1::new(fixture);
    let first = ports.resolve("cdn.example.com").await.expect("first answer");
    let second = ports.resolve("CDN.EXAMPLE.COM").await.expect("second answer");
    assert!(!is_forbidden_egress_address(first[0]));
    assert!(is_forbidden_egress_address(second[0]));
}

const fn assert_send<T: Send>(_: &T) {}
