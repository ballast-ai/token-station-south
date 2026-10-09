//! Canonical fixtures for the safe fetch host suite (image record §11, D8b; §12.3 item 3).
//!
//! An absolute-URL second hop — an image the upstream left at a URL, a speech result behind a
//! link — is not a provider call: South never executes it. The host does, with one executor shared
//! by every media world, and this table holds that executor to the seven obligations of §11:
//!
//! 1. `https` only, no userinfo, a host name required, no `localhost` or `*.localhost`;
//! 2. resolve before connecting, under a timeout; refuse outright if **any** resolved address is
//!    forbidden; then pin the connection to the checked address;
//! 3. the forbidden ranges of [`south_contracts::media::is_forbidden_egress_address`], including
//!    every IPv6 form that embeds an IPv4 address;
//! 4. no redirect is followed (a 3xx is a failure) and the system proxy is disabled;
//! 5. only `Accept` is sent — no credential header, no cookie;
//! 6. a total timeout, and a byte limit of the smaller of the host limit and
//!    [`south_contracts::MAX_BINARY_RESPONSE_BODY_BYTES`]; an empty body is a failure;
//! 7. the result's media type is the one the component declared; the upstream `content-type` is
//!    only recorded.
//!
//! Each fixture describes a whole fake network: what the resolver answers for each name, what each
//! server answers, and what the system proxy environment says. The runner in `south-testkit`
//! builds the fakes from the fixture, hands them to the host's executor, and judges what the
//! executor did through them — so every piece of boundary evidence here is **observed by the
//! runner**, not reported by the adapter. The refused destinations all answer when connected to:
//! an executor that skips a check gets a convincing artifact back, exactly as it would from an
//! internal service, and fails on that.

use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use south_contracts::{MAX_BINARY_RESPONSE_BODY_BYTES, media::ArtifactUrlErrorV1};

use crate::ProviderCallCountV1;

/// The safe fetch conformance suite version.
pub const SAFE_FETCH_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The stable identifier for safe fetch conformance version one.
pub const SAFE_FETCH_CONFORMANCE_SUITE_ID: &str = "south.safe-fetch.v1";

/// The closed set of canonical safe fetch cases.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SafeFetchCaseIdV1 {
    /// An `http` URL, refused before any resolution (obligation 1).
    HttpRefused,
    /// A URL carrying userinfo, refused before any resolution (obligation 1). The fixture's
    /// password is a sentinel no diagnostic may echo.
    UserinfoRefused,
    /// `localhost`, refused by name before any resolution (obligation 1).
    LocalhostRefused,
    /// The literal `127.0.0.1`, refused without a resolution or a connection.
    LoopbackLiteralRefused,
    /// The literal cloud metadata address `169.254.169.254`, refused without a connection.
    MetadataLiteralRefused,
    /// A name that resolves to `10.0.0.1`, refused after one resolution and before any connection.
    ResolvesToPrivateRefused,
    /// A name that resolves to a public address **and** `192.168.1.1`, public first.
    ///
    /// Any forbidden address refuses the whole fetch. An executor that checks only the first
    /// answer, or that filters the forbidden one out and connects to the rest, fails here.
    AnyResolvedAddressForbiddenRefused,
    /// A name whose AAAA answer is the v4-mapped `::ffff:10.0.0.1`.
    V4MappedRefused,
    /// A name whose AAAA answer is the v4-compatible `::10.0.0.1`.
    V4CompatibleRefused,
    /// A name whose AAAA answer is the NAT64 `64:ff9b::a00:1`, which a NAT64 gateway delivers to
    /// `10.0.0.1`.
    Nat64Refused,
    /// A name whose AAAA answer is the 6to4 `2002:a00:1::1`, whose bits 16–47 are `10.0.0.1`.
    SixToFourRefused,
    /// A name whose AAAA answer is the deprecated site-local `fec0::1`.
    SiteLocalRefused,
    /// A name the resolver has no address for.
    NoAddressesRefused,
    /// A resolver that never answers: the executor's own timeout ends the fetch and nothing is
    /// connected to (obligation 2, "with a timeout").
    ResolutionStallTimesOut,
    /// A public server answering `302` to `https://10.0.0.1/…` (obligation 4).
    ///
    /// The redirect is the failure, not the target: an executor that follows and then refuses
    /// the internal address reaches the right verdict for the wrong reason, and fails on the code.
    RedirectToInternalRefused,
    /// A public server answering `301` to another public server that would serve the artifact.
    ///
    /// Following this redirect is safe as far as addresses go, so only an executor that refuses
    /// every 3xx passes.
    RedirectToPublicRefused,
    /// A public server answering `404` with a body: not an artifact.
    NonSuccessStatusRefused,
    /// The plain success: one resolution, one connection to the checked address, the declared
    /// media type, the upstream `content-type` recorded beside it.
    PublicFetchSucceeds,
    /// A resolver that answers a public address first and a private one on every later query.
    ///
    /// The DNS-rebinding row: the executor must connect to the address it checked, not resolve
    /// again when it connects.
    ConnectionPinnedToCheckedAddress,
    /// A system proxy is configured in the environment the executor is given; the fetch must still
    /// go direct to the checked address (obligation 4: behind a proxy, the proxy re-resolves and
    /// the pin is void).
    SystemProxyIgnored,
    /// The upstream says `text/html`; the artifact is still the declared `image/png`, and the
    /// upstream value is only recorded (obligation 7). An executor that validates the upstream
    /// value, or adopts it, fails here.
    DeclaredMediaTypeWins,
    /// A body of exactly the host limit, in two chunks and with no upstream `content-type`: the
    /// limit is inclusive and a missing value is recorded as missing.
    HostLimitExactSucceeds,
    /// A body one byte over the host limit, arriving in two chunks: the limit is enforced across
    /// chunks.
    HostLimitExceededRefused,
    /// A host limit above 64 MiB and a body of 64 MiB plus one byte: the binary response cap still
    /// applies (obligation 6, "the minimum of").
    BinaryCapBelowHostLimitRefused,
    /// A `200` whose body is one empty chunk and then the end (obligation 6).
    EmptyBodyRefused,
    /// A server that sends its headers and a first chunk, then stalls: the timeout is total, so it
    /// covers the body as well as the connection.
    BodyStallTimesOut,
}

fixed_debug!(SafeFetchCaseIdV1 {
    HttpRefused => "HttpRefused",
    UserinfoRefused => "UserinfoRefused",
    LocalhostRefused => "LocalhostRefused",
    LoopbackLiteralRefused => "LoopbackLiteralRefused",
    MetadataLiteralRefused => "MetadataLiteralRefused",
    ResolvesToPrivateRefused => "ResolvesToPrivateRefused",
    AnyResolvedAddressForbiddenRefused => "AnyResolvedAddressForbiddenRefused",
    V4MappedRefused => "V4MappedRefused",
    V4CompatibleRefused => "V4CompatibleRefused",
    Nat64Refused => "Nat64Refused",
    SixToFourRefused => "SixToFourRefused",
    SiteLocalRefused => "SiteLocalRefused",
    NoAddressesRefused => "NoAddressesRefused",
    ResolutionStallTimesOut => "ResolutionStallTimesOut",
    RedirectToInternalRefused => "RedirectToInternalRefused",
    RedirectToPublicRefused => "RedirectToPublicRefused",
    NonSuccessStatusRefused => "NonSuccessStatusRefused",
    PublicFetchSucceeds => "PublicFetchSucceeds",
    ConnectionPinnedToCheckedAddress => "ConnectionPinnedToCheckedAddress",
    SystemProxyIgnored => "SystemProxyIgnored",
    DeclaredMediaTypeWins => "DeclaredMediaTypeWins",
    HostLimitExactSucceeds => "HostLimitExactSucceeds",
    HostLimitExceededRefused => "HostLimitExceededRefused",
    BinaryCapBelowHostLimitRefused => "BinaryCapBelowHostLimitRefused",
    EmptyBodyRefused => "EmptyBodyRefused",
    BodyStallTimesOut => "BodyStallTimesOut",
});

/// The closed reasons a safe fetch fails.
///
/// The first six mirror [`ArtifactUrlErrorV1`] one for one, so a host built on
/// [`south_contracts::media::ArtifactUrlV1::parse`] maps them with
/// [`SafeFetchFailureCodeV1::from_artifact_url_error`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SafeFetchFailureCodeV1 {
    /// The URL is longer than the artifact URL bound.
    UrlTooLong,
    /// Not an absolute URL.
    InvalidUrl,
    /// Not `https`.
    NotHttps,
    /// The URL carries a user name or password.
    Userinfo,
    /// No host, `localhost`, or a name under `.localhost`.
    ForbiddenHost,
    /// A literal or resolved address in a forbidden range.
    ForbiddenAddress,
    /// The resolver failed or answered no address.
    ResolutionFailed,
    /// The total timeout elapsed, during resolution or during the exchange.
    Timeout,
    /// The connection or exchange failed.
    TransportFailed,
    /// The upstream answered 3xx; redirects are never followed.
    Redirect,
    /// The upstream answered with a status that is neither 2xx nor 3xx.
    UpstreamStatus,
    /// The 2xx body was empty.
    EmptyBody,
    /// The body exceeded the smaller of the host limit and the binary response cap.
    BodyTooLarge,
}

fixed_debug!(SafeFetchFailureCodeV1 {
    UrlTooLong => "UrlTooLong",
    InvalidUrl => "InvalidUrl",
    NotHttps => "NotHttps",
    Userinfo => "Userinfo",
    ForbiddenHost => "ForbiddenHost",
    ForbiddenAddress => "ForbiddenAddress",
    ResolutionFailed => "ResolutionFailed",
    Timeout => "Timeout",
    TransportFailed => "TransportFailed",
    Redirect => "Redirect",
    UpstreamStatus => "UpstreamStatus",
    EmptyBody => "EmptyBody",
    BodyTooLarge => "BodyTooLarge",
});

impl SafeFetchFailureCodeV1 {
    /// Maps a refusal of the pure URL grammar onto the suite's closed code.
    #[must_use]
    pub const fn from_artifact_url_error(error: ArtifactUrlErrorV1) -> Self {
        match error {
            ArtifactUrlErrorV1::TooLong => Self::UrlTooLong,
            ArtifactUrlErrorV1::Invalid => Self::InvalidUrl,
            ArtifactUrlErrorV1::NotHttps => Self::NotHttps,
            ArtifactUrlErrorV1::Userinfo => Self::Userinfo,
            ArtifactUrlErrorV1::ForbiddenHost => Self::ForbiddenHost,
            ArtifactUrlErrorV1::ForbiddenAddress => Self::ForbiddenAddress,
        }
    }
}

/// What the executor is asked to fetch.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SafeFetchInputV1 {
    url: &'static str,
    declared_media_type: &'static str,
    host_byte_limit: usize,
    total_timeout: Duration,
}

impl SafeFetchInputV1 {
    /// Returns the raw URL the component pointed at.
    #[must_use]
    pub const fn url(&self) -> &'static str {
        self.url
    }

    /// Returns the media type the component declared for the artifact.
    #[must_use]
    pub const fn declared_media_type(&self) -> &'static str {
        self.declared_media_type
    }

    /// Returns the host's own byte limit for this fetch, before the binary response cap.
    #[must_use]
    pub const fn host_byte_limit(&self) -> usize {
        self.host_byte_limit
    }

    /// Returns the total timeout the executor must apply, resolution included.
    #[must_use]
    pub const fn total_timeout(&self) -> Duration {
        self.total_timeout
    }
}

impl fmt::Debug for SafeFetchInputV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchInputV1")
            .field("url_byte_count", &self.url.len())
            .field("declared_media_type_byte_count", &self.declared_media_type.len())
            .field("host_byte_limit", &self.host_byte_limit)
            .field("total_timeout", &self.total_timeout)
            .finish()
    }
}

/// What the fake resolver answers for one name.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SafeFetchResolutionV1 {
    /// The same addresses on every query, in this order. Empty means "no address".
    Addresses(&'static [IpAddr]),
    /// `first` on the first query for the name and `later` on every later one.
    Rebinding {
        /// The first answer.
        first: &'static [IpAddr],
        /// Every later answer.
        later: &'static [IpAddr],
    },
    /// The query never completes.
    Stall,
}

impl fmt::Debug for SafeFetchResolutionV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Addresses(addresses) => {
                formatter.debug_tuple("Addresses").field(addresses).finish()
            }
            Self::Rebinding { first, later } => formatter
                .debug_struct("Rebinding")
                .field("first", first)
                .field("later", later)
                .finish(),
            Self::Stall => formatter.write_str("Stall"),
        }
    }
}

/// One name the fake resolver knows. A name it does not know fails to resolve.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SafeFetchDnsEntryV1 {
    name: &'static str,
    resolution: SafeFetchResolutionV1,
}

impl SafeFetchDnsEntryV1 {
    /// Returns the name, compared ASCII case-insensitively.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Returns what the resolver answers for it.
    #[must_use]
    pub const fn resolution(&self) -> SafeFetchResolutionV1 {
        self.resolution
    }
}

impl fmt::Debug for SafeFetchDnsEntryV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchDnsEntryV1")
            .field("name_byte_count", &self.name.len())
            .field("resolution", &self.resolution)
            .finish()
    }
}

/// How a fake server delivers its body.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SafeFetchUpstreamBodyV1 {
    /// These chunks in order, then the end. An empty chunk is delivered as one.
    Chunks(&'static [&'static [u8]]),
    /// `total_bytes` bytes of a fill pattern in chunks of `chunk_bytes`, then the end. Lets a row
    /// exceed the 64 MiB cap without a 64 MiB table entry.
    Repeated {
        /// The size of every chunk but the last.
        chunk_bytes: usize,
        /// The total size.
        total_bytes: usize,
    },
    /// This first chunk, then a read that never completes.
    StallAfter(&'static [u8]),
}

impl fmt::Debug for SafeFetchUpstreamBodyV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Chunks(chunks) => formatter
                .debug_struct("Chunks")
                .field("chunk_count", &chunks.len())
                .field("byte_count", &chunks.iter().map(|chunk| chunk.len()).sum::<usize>())
                .finish(),
            Self::Repeated { chunk_bytes, total_bytes } => formatter
                .debug_struct("Repeated")
                .field("chunk_bytes", chunk_bytes)
                .field("total_bytes", total_bytes)
                .finish(),
            Self::StallAfter(prefix) => {
                formatter.debug_struct("StallAfter").field("byte_count", &prefix.len()).finish()
            }
        }
    }
}

/// What one fake server answers, whoever connects to it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SafeFetchUpstreamResponseV1 {
    status: u16,
    content_type: Option<&'static str>,
    location: Option<&'static str>,
    body: SafeFetchUpstreamBodyV1,
}

impl SafeFetchUpstreamResponseV1 {
    /// Returns the raw HTTP status.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Returns the optional raw `content-type`.
    #[must_use]
    pub const fn content_type(&self) -> Option<&'static str> {
        self.content_type
    }

    /// Returns the optional raw `location`.
    #[must_use]
    pub const fn location(&self) -> Option<&'static str> {
        self.location
    }

    /// Returns how the body is delivered.
    #[must_use]
    pub const fn body(&self) -> SafeFetchUpstreamBodyV1 {
        self.body
    }
}

impl fmt::Debug for SafeFetchUpstreamResponseV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchUpstreamResponseV1")
            .field("status", &self.status)
            .field("has_content_type", &self.content_type.is_some())
            .field("has_location", &self.location.is_some())
            .field("body", &self.body)
            .finish()
    }
}

/// One fake server, found by the TLS server name the exchange names.
///
/// The fake answers by name rather than by address on purpose: a connection to the wrong address
/// still gets an answer, so a mispinned or proxied exchange is caught by the address evidence
/// rather than hidden behind a transport failure.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SafeFetchServerV1 {
    server_name: &'static str,
    response: SafeFetchUpstreamResponseV1,
}

impl SafeFetchServerV1 {
    /// Returns the server name, compared ASCII case-insensitively.
    #[must_use]
    pub const fn server_name(&self) -> &'static str {
        self.server_name
    }

    /// Returns the response every exchange with it receives.
    #[must_use]
    pub const fn response(&self) -> &SafeFetchUpstreamResponseV1 {
        &self.response
    }
}

impl fmt::Debug for SafeFetchServerV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchServerV1")
            .field("server_name_byte_count", &self.server_name.len())
            .field("response", &self.response)
            .finish()
    }
}

/// The whole fake network one case runs in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SafeFetchEnvironmentV1 {
    dns: &'static [SafeFetchDnsEntryV1],
    servers: &'static [SafeFetchServerV1],
    system_proxy: Option<&'static str>,
}

impl SafeFetchEnvironmentV1 {
    /// Returns every name the resolver knows.
    #[must_use]
    pub const fn dns(&self) -> &'static [SafeFetchDnsEntryV1] {
        self.dns
    }

    /// Returns every server that answers.
    #[must_use]
    pub const fn servers(&self) -> &'static [SafeFetchServerV1] {
        self.servers
    }

    /// Returns the system proxy the environment configures, as an `HTTPS_PROXY` value would.
    #[must_use]
    pub const fn system_proxy(&self) -> Option<&'static str> {
        self.system_proxy
    }
}

impl fmt::Debug for SafeFetchEnvironmentV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchEnvironmentV1")
            .field("dns", &self.dns)
            .field("servers", &self.servers)
            .field("has_system_proxy", &self.system_proxy.is_some())
            .finish()
    }
}

/// The only target a correct exchange names: the URL's host as the TLS server name and its path
/// and query as the request target.
///
/// Pinning changes *where* the executor connects, never *whom* it talks to: the server name stays
/// the URL's host, so the certificate is still checked against it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SafeFetchWireTargetV1 {
    server_name: &'static str,
    path_and_query: &'static str,
}

impl SafeFetchWireTargetV1 {
    /// Returns the expected TLS server name and `host`.
    #[must_use]
    pub const fn server_name(&self) -> &'static str {
        self.server_name
    }

    /// Returns the expected request target.
    #[must_use]
    pub const fn path_and_query(&self) -> &'static str {
        self.path_and_query
    }
}

impl fmt::Debug for SafeFetchWireTargetV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchWireTargetV1")
            .field("server_name_byte_count", &self.server_name.len())
            .field("path_and_query_byte_count", &self.path_and_query.len())
            .finish()
    }
}

/// The exact expected terminal shape of one canonical safe fetch case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SafeFetchExpectedOutcomeV1 {
    /// A fetched artifact matched field by field.
    Artifact {
        /// Expected body bytes.
        body: &'static [u8],
        /// Expected media type: always the declared one.
        media_type: &'static str,
        /// Expected recorded upstream `content-type`, preserving presence.
        upstream_content_type: Option<&'static str>,
    },
    /// A known closed failure.
    Failure {
        /// Expected closed failure code.
        code: SafeFetchFailureCodeV1,
    },
}

impl fmt::Debug for SafeFetchExpectedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Artifact { body, media_type, upstream_content_type } => formatter
                .debug_struct("Artifact")
                .field("body_byte_count", &body.len())
                .field("media_type_byte_count", &media_type.len())
                .field("has_upstream_content_type", &upstream_content_type.is_some())
                .finish(),
            Self::Failure { code } => {
                formatter.debug_struct("Failure").field("code", code).finish()
            }
        }
    }
}

/// Expected runner-observed boundary evidence.
///
/// Two further claims hold on **every** exchange of every case and so are not tabulated: the
/// exchange named [`SafeFetchFixtureV1::wire_target`], and its only request header was `accept`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SafeFetchExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    connected_to: Option<SocketAddr>,
}

impl SafeFetchExpectedEvidenceV1 {
    /// Returns the expected resolver query category.
    #[must_use]
    pub const fn resolver_calls(&self) -> ProviderCallCountV1 {
        self.resolver_calls
    }

    /// Returns the one address the executor must connect to, or `None` for no connection at all.
    #[must_use]
    pub const fn connected_to(&self) -> Option<SocketAddr> {
        self.connected_to
    }
}

impl fmt::Debug for SafeFetchExpectedEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchExpectedEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("connected_to", &self.connected_to)
            .finish()
    }
}

/// The expected outcome and boundary evidence for one safe fetch fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SafeFetchExpectedV1 {
    outcome: SafeFetchExpectedOutcomeV1,
    evidence: SafeFetchExpectedEvidenceV1,
}

impl SafeFetchExpectedV1 {
    /// Returns the expected terminal shape.
    #[must_use]
    pub const fn outcome(&self) -> &SafeFetchExpectedOutcomeV1 {
        &self.outcome
    }

    /// Returns the expected boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &SafeFetchExpectedEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for SafeFetchExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchExpectedV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// One immutable canonical safe fetch fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SafeFetchFixtureV1 {
    case_id: SafeFetchCaseIdV1,
    input: SafeFetchInputV1,
    environment: SafeFetchEnvironmentV1,
    wire_target: SafeFetchWireTargetV1,
    expected: SafeFetchExpectedV1,
}

impl SafeFetchFixtureV1 {
    /// Returns the stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> SafeFetchCaseIdV1 {
        self.case_id
    }

    /// Returns what the executor is asked to fetch.
    #[must_use]
    pub const fn input(&self) -> &SafeFetchInputV1 {
        &self.input
    }

    /// Returns the fake network the case runs in.
    #[must_use]
    pub const fn environment(&self) -> &SafeFetchEnvironmentV1 {
        &self.environment
    }

    /// Returns the only target any exchange of this case may name.
    #[must_use]
    pub const fn wire_target(&self) -> &SafeFetchWireTargetV1 {
        &self.wire_target
    }

    /// Returns the exact expected outcome and evidence.
    #[must_use]
    pub const fn expected(&self) -> &SafeFetchExpectedV1 {
        &self.expected
    }
}

impl fmt::Debug for SafeFetchFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchFixtureV1")
            .field("case_id", &self.case_id)
            .field("input", &self.input)
            .field("environment", &self.environment)
            .field("wire_target", &self.wire_target)
            .field("expected", &self.expected)
            .finish()
    }
}

// Names and addresses. Public addresses come from the documentation ranges (RFC 5737), which
// `is_forbidden_egress_address` allows, so no row depends on a real host.

const CDN: &str = "cdn.example.com";
const MIRROR: &str = "mirror.example.com";
const PROXY: &str = "proxy.example.com";
const ARTIFACT_PATH: &str = "/artifacts/a.png?sig=signature-debug-sentinel";
const CDN_URL: &str = "https://cdn.example.com/artifacts/a.png?sig=signature-debug-sentinel";

const PUBLIC: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10));
const MIRROR_PUBLIC: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 20));
const PROXY_PUBLIC: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 99));
const PRIVATE: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
const PRIVATE_HOME: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
const V4_MAPPED: IpAddr = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x0a00, 0x0001));
const V4_COMPATIBLE: IpAddr = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0x0a00, 0x0001));
const NAT64: IpAddr = IpAddr::V6(Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0x0a00, 0x0001));
const SIX_TO_FOUR: IpAddr = IpAddr::V6(Ipv6Addr::new(0x2002, 0x0a00, 0x0001, 0, 0, 0, 0, 1));
const SITE_LOCAL: IpAddr = IpAddr::V6(Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 1));

const PUBLIC_443: SocketAddr = SocketAddr::new(PUBLIC, 443);

const PNG: &str = "image/png";
const UPSTREAM_HTML: &str = "text/html; charset=utf-8";
const PROXY_ENVIRONMENT: &str = "http://proxy.example.com:3128";

/// A PNG signature followed by bytes no decoder accepts: the artifact every answering server
/// returns, refused destinations included.
const ARTIFACT: &[u8] = &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0xfe, 0xff];
const ARTIFACT_CHUNKS: &[&[u8]] = &[ARTIFACT];

/// Sixteen bytes in two chunks of ten and six; [`HOST_LIMIT_SMALL`] is sixteen.
const SIXTEEN: &[u8] = b"sixteen-bytes-ok";
const SIXTEEN_CHUNKS: &[&[u8]] = &[b"sixteen-by", b"tes-ok"];
/// Seventeen bytes in two chunks, neither over the limit alone.
const SEVENTEEN_CHUNKS: &[&[u8]] = &[b"seventeen-", b"bytes!!"];
const HOST_LIMIT_SMALL: usize = 16;

const NOT_FOUND_CHUNKS: &[&[u8]] = &[b"not-found-body-debug-sentinel"];
const EMPTY_CHUNKS: &[&[u8]] = &[b""];

/// Generous for rows that complete at once; the runner adds its own grace on top.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Short for the two rows that only end by timeout, so a host running in real time waits at most
/// this long for each.
const STALL_TIMEOUT: Duration = Duration::from_secs(1);

const fn input(url: &'static str) -> SafeFetchInputV1 {
    SafeFetchInputV1 {
        url,
        declared_media_type: PNG,
        host_byte_limit: 1024 * 1024,
        total_timeout: TIMEOUT,
    }
}

const fn ok(
    content_type: Option<&'static str>,
    chunks: &'static [&'static [u8]],
) -> SafeFetchUpstreamResponseV1 {
    SafeFetchUpstreamResponseV1 {
        status: 200,
        content_type,
        location: None,
        body: SafeFetchUpstreamBodyV1::Chunks(chunks),
    }
}

const ARTIFACT_RESPONSE: SafeFetchUpstreamResponseV1 = ok(Some(PNG), ARTIFACT_CHUNKS);

/// `cdn.example.com` serves the artifact, whatever it resolves to.
const CDN_SERVES_ARTIFACT: &[SafeFetchServerV1] =
    &[SafeFetchServerV1 { server_name: CDN, response: ARTIFACT_RESPONSE }];

const fn environment(
    dns: &'static [SafeFetchDnsEntryV1],
    servers: &'static [SafeFetchServerV1],
) -> SafeFetchEnvironmentV1 {
    SafeFetchEnvironmentV1 { dns, servers, system_proxy: None }
}

/// No resolver entry: the URL is refused before the resolver could matter.
const NO_DNS: &[SafeFetchDnsEntryV1] = &[];

macro_rules! cdn_resolves_to {
    ($($address:expr),* $(,)?) => {
        environment(
            &[SafeFetchDnsEntryV1 {
                name: CDN,
                resolution: SafeFetchResolutionV1::Addresses(&[$($address),*]),
            }],
            CDN_SERVES_ARTIFACT,
        )
    };
}

const fn wire(server_name: &'static str, path_and_query: &'static str) -> SafeFetchWireTargetV1 {
    SafeFetchWireTargetV1 { server_name, path_and_query }
}

const CDN_TARGET: SafeFetchWireTargetV1 = wire(CDN, ARTIFACT_PATH);

const fn refused_unresolved(code: SafeFetchFailureCodeV1) -> SafeFetchExpectedV1 {
    SafeFetchExpectedV1 {
        outcome: SafeFetchExpectedOutcomeV1::Failure { code },
        evidence: SafeFetchExpectedEvidenceV1 {
            resolver_calls: ProviderCallCountV1::Zero,
            connected_to: None,
        },
    }
}

const fn refused_after_resolution(code: SafeFetchFailureCodeV1) -> SafeFetchExpectedV1 {
    SafeFetchExpectedV1 {
        outcome: SafeFetchExpectedOutcomeV1::Failure { code },
        evidence: SafeFetchExpectedEvidenceV1 {
            resolver_calls: ProviderCallCountV1::One,
            connected_to: None,
        },
    }
}

const fn refused_after_exchange(code: SafeFetchFailureCodeV1) -> SafeFetchExpectedV1 {
    SafeFetchExpectedV1 {
        outcome: SafeFetchExpectedOutcomeV1::Failure { code },
        evidence: SafeFetchExpectedEvidenceV1 {
            resolver_calls: ProviderCallCountV1::One,
            connected_to: Some(PUBLIC_443),
        },
    }
}

const fn fetched(
    body: &'static [u8],
    upstream_content_type: Option<&'static str>,
) -> SafeFetchExpectedV1 {
    SafeFetchExpectedV1 {
        outcome: SafeFetchExpectedOutcomeV1::Artifact {
            body,
            media_type: PNG,
            upstream_content_type,
        },
        evidence: SafeFetchExpectedEvidenceV1 {
            resolver_calls: ProviderCallCountV1::One,
            connected_to: Some(PUBLIC_443),
        },
    }
}

const fn fixture(
    case_id: SafeFetchCaseIdV1,
    input: SafeFetchInputV1,
    environment: SafeFetchEnvironmentV1,
    wire_target: SafeFetchWireTargetV1,
    expected: SafeFetchExpectedV1,
) -> SafeFetchFixtureV1 {
    SafeFetchFixtureV1 { case_id, input, environment, wire_target, expected }
}

/// A public `cdn.example.com` whose one server answers with `response`.
macro_rules! public_cdn_answering {
    ($response:expr) => {
        environment(
            &[SafeFetchDnsEntryV1 {
                name: CDN,
                resolution: SafeFetchResolutionV1::Addresses(&[PUBLIC]),
            }],
            &[SafeFetchServerV1 { server_name: CDN, response: $response }],
        )
    };
}

const SAFE_FETCH_FIXTURES: &[SafeFetchFixtureV1] = &[
    fixture(
        SafeFetchCaseIdV1::HttpRefused,
        input("http://cdn.example.com/artifacts/a.png?sig=signature-debug-sentinel"),
        cdn_resolves_to!(PUBLIC),
        CDN_TARGET,
        refused_unresolved(SafeFetchFailureCodeV1::NotHttps),
    ),
    fixture(
        SafeFetchCaseIdV1::UserinfoRefused,
        input(
            "https://user-debug-sentinel:password-debug-sentinel@cdn.example.com/artifacts/a.png?sig=signature-debug-sentinel",
        ),
        cdn_resolves_to!(PUBLIC),
        CDN_TARGET,
        refused_unresolved(SafeFetchFailureCodeV1::Userinfo),
    ),
    fixture(
        SafeFetchCaseIdV1::LocalhostRefused,
        input("https://localhost/artifacts/a.png?sig=signature-debug-sentinel"),
        environment(
            &[SafeFetchDnsEntryV1 {
                name: "localhost",
                resolution: SafeFetchResolutionV1::Addresses(&[PUBLIC]),
            }],
            &[SafeFetchServerV1 { server_name: "localhost", response: ARTIFACT_RESPONSE }],
        ),
        wire("localhost", ARTIFACT_PATH),
        refused_unresolved(SafeFetchFailureCodeV1::ForbiddenHost),
    ),
    fixture(
        SafeFetchCaseIdV1::LoopbackLiteralRefused,
        input("https://127.0.0.1/artifacts/a.png?sig=signature-debug-sentinel"),
        environment(
            NO_DNS,
            &[SafeFetchServerV1 { server_name: "127.0.0.1", response: ARTIFACT_RESPONSE }],
        ),
        wire("127.0.0.1", ARTIFACT_PATH),
        refused_unresolved(SafeFetchFailureCodeV1::ForbiddenAddress),
    ),
    fixture(
        SafeFetchCaseIdV1::MetadataLiteralRefused,
        input("https://169.254.169.254/latest/meta-data/"),
        environment(
            NO_DNS,
            &[SafeFetchServerV1 { server_name: "169.254.169.254", response: ARTIFACT_RESPONSE }],
        ),
        wire("169.254.169.254", "/latest/meta-data/"),
        refused_unresolved(SafeFetchFailureCodeV1::ForbiddenAddress),
    ),
    fixture(
        SafeFetchCaseIdV1::ResolvesToPrivateRefused,
        input(CDN_URL),
        cdn_resolves_to!(PRIVATE),
        CDN_TARGET,
        refused_after_resolution(SafeFetchFailureCodeV1::ForbiddenAddress),
    ),
    fixture(
        SafeFetchCaseIdV1::AnyResolvedAddressForbiddenRefused,
        input(CDN_URL),
        cdn_resolves_to!(PUBLIC, PRIVATE_HOME),
        CDN_TARGET,
        refused_after_resolution(SafeFetchFailureCodeV1::ForbiddenAddress),
    ),
    fixture(
        SafeFetchCaseIdV1::V4MappedRefused,
        input(CDN_URL),
        cdn_resolves_to!(V4_MAPPED),
        CDN_TARGET,
        refused_after_resolution(SafeFetchFailureCodeV1::ForbiddenAddress),
    ),
    fixture(
        SafeFetchCaseIdV1::V4CompatibleRefused,
        input(CDN_URL),
        cdn_resolves_to!(V4_COMPATIBLE),
        CDN_TARGET,
        refused_after_resolution(SafeFetchFailureCodeV1::ForbiddenAddress),
    ),
    fixture(
        SafeFetchCaseIdV1::Nat64Refused,
        input(CDN_URL),
        cdn_resolves_to!(NAT64),
        CDN_TARGET,
        refused_after_resolution(SafeFetchFailureCodeV1::ForbiddenAddress),
    ),
    fixture(
        SafeFetchCaseIdV1::SixToFourRefused,
        input(CDN_URL),
        cdn_resolves_to!(SIX_TO_FOUR),
        CDN_TARGET,
        refused_after_resolution(SafeFetchFailureCodeV1::ForbiddenAddress),
    ),
    fixture(
        SafeFetchCaseIdV1::SiteLocalRefused,
        input(CDN_URL),
        cdn_resolves_to!(SITE_LOCAL),
        CDN_TARGET,
        refused_after_resolution(SafeFetchFailureCodeV1::ForbiddenAddress),
    ),
    fixture(
        SafeFetchCaseIdV1::NoAddressesRefused,
        input(CDN_URL),
        cdn_resolves_to!(),
        CDN_TARGET,
        refused_after_resolution(SafeFetchFailureCodeV1::ResolutionFailed),
    ),
    fixture(
        SafeFetchCaseIdV1::ResolutionStallTimesOut,
        SafeFetchInputV1 { total_timeout: STALL_TIMEOUT, ..input(CDN_URL) },
        environment(
            &[SafeFetchDnsEntryV1 { name: CDN, resolution: SafeFetchResolutionV1::Stall }],
            CDN_SERVES_ARTIFACT,
        ),
        CDN_TARGET,
        refused_after_resolution(SafeFetchFailureCodeV1::Timeout),
    ),
    fixture(
        SafeFetchCaseIdV1::RedirectToInternalRefused,
        input(CDN_URL),
        public_cdn_answering!(SafeFetchUpstreamResponseV1 {
            status: 302,
            content_type: None,
            location: Some("https://10.0.0.1/artifacts/a.png?sig=signature-debug-sentinel"),
            body: SafeFetchUpstreamBodyV1::Chunks(EMPTY_CHUNKS),
        }),
        CDN_TARGET,
        refused_after_exchange(SafeFetchFailureCodeV1::Redirect),
    ),
    fixture(
        SafeFetchCaseIdV1::RedirectToPublicRefused,
        input(CDN_URL),
        environment(
            &[
                SafeFetchDnsEntryV1 {
                    name: CDN,
                    resolution: SafeFetchResolutionV1::Addresses(&[PUBLIC]),
                },
                SafeFetchDnsEntryV1 {
                    name: MIRROR,
                    resolution: SafeFetchResolutionV1::Addresses(&[MIRROR_PUBLIC]),
                },
            ],
            &[
                SafeFetchServerV1 {
                    server_name: CDN,
                    response: SafeFetchUpstreamResponseV1 {
                        status: 301,
                        content_type: None,
                        location: Some(
                            "https://mirror.example.com/artifacts/a.png?sig=signature-debug-sentinel",
                        ),
                        body: SafeFetchUpstreamBodyV1::Chunks(EMPTY_CHUNKS),
                    },
                },
                SafeFetchServerV1 { server_name: MIRROR, response: ARTIFACT_RESPONSE },
            ],
        ),
        CDN_TARGET,
        refused_after_exchange(SafeFetchFailureCodeV1::Redirect),
    ),
    fixture(
        SafeFetchCaseIdV1::NonSuccessStatusRefused,
        input(CDN_URL),
        public_cdn_answering!(SafeFetchUpstreamResponseV1 {
            status: 404,
            content_type: Some(UPSTREAM_HTML),
            location: None,
            body: SafeFetchUpstreamBodyV1::Chunks(NOT_FOUND_CHUNKS),
        }),
        CDN_TARGET,
        refused_after_exchange(SafeFetchFailureCodeV1::UpstreamStatus),
    ),
    fixture(
        SafeFetchCaseIdV1::PublicFetchSucceeds,
        input(CDN_URL),
        cdn_resolves_to!(PUBLIC),
        CDN_TARGET,
        fetched(ARTIFACT, Some(PNG)),
    ),
    fixture(
        SafeFetchCaseIdV1::ConnectionPinnedToCheckedAddress,
        input(CDN_URL),
        environment(
            &[SafeFetchDnsEntryV1 {
                name: CDN,
                resolution: SafeFetchResolutionV1::Rebinding {
                    first: &[PUBLIC],
                    later: &[PRIVATE],
                },
            }],
            CDN_SERVES_ARTIFACT,
        ),
        CDN_TARGET,
        fetched(ARTIFACT, Some(PNG)),
    ),
    fixture(
        SafeFetchCaseIdV1::SystemProxyIgnored,
        input(CDN_URL),
        SafeFetchEnvironmentV1 {
            dns: &[
                SafeFetchDnsEntryV1 {
                    name: CDN,
                    resolution: SafeFetchResolutionV1::Addresses(&[PUBLIC]),
                },
                SafeFetchDnsEntryV1 {
                    name: PROXY,
                    resolution: SafeFetchResolutionV1::Addresses(&[PROXY_PUBLIC]),
                },
            ],
            servers: CDN_SERVES_ARTIFACT,
            system_proxy: Some(PROXY_ENVIRONMENT),
        },
        CDN_TARGET,
        fetched(ARTIFACT, Some(PNG)),
    ),
    fixture(
        SafeFetchCaseIdV1::DeclaredMediaTypeWins,
        input(CDN_URL),
        public_cdn_answering!(ok(Some(UPSTREAM_HTML), ARTIFACT_CHUNKS)),
        CDN_TARGET,
        fetched(ARTIFACT, Some(UPSTREAM_HTML)),
    ),
    fixture(
        SafeFetchCaseIdV1::HostLimitExactSucceeds,
        SafeFetchInputV1 { host_byte_limit: HOST_LIMIT_SMALL, ..input(CDN_URL) },
        public_cdn_answering!(ok(None, SIXTEEN_CHUNKS)),
        CDN_TARGET,
        fetched(SIXTEEN, None),
    ),
    fixture(
        SafeFetchCaseIdV1::HostLimitExceededRefused,
        SafeFetchInputV1 { host_byte_limit: HOST_LIMIT_SMALL, ..input(CDN_URL) },
        public_cdn_answering!(ok(Some(PNG), SEVENTEEN_CHUNKS)),
        CDN_TARGET,
        refused_after_exchange(SafeFetchFailureCodeV1::BodyTooLarge),
    ),
    fixture(
        SafeFetchCaseIdV1::BinaryCapBelowHostLimitRefused,
        SafeFetchInputV1 { host_byte_limit: 2 * MAX_BINARY_RESPONSE_BODY_BYTES, ..input(CDN_URL) },
        public_cdn_answering!(SafeFetchUpstreamResponseV1 {
            status: 200,
            content_type: Some(PNG),
            location: None,
            body: SafeFetchUpstreamBodyV1::Repeated {
                chunk_bytes: 1024 * 1024,
                total_bytes: MAX_BINARY_RESPONSE_BODY_BYTES + 1,
            },
        }),
        CDN_TARGET,
        refused_after_exchange(SafeFetchFailureCodeV1::BodyTooLarge),
    ),
    fixture(
        SafeFetchCaseIdV1::EmptyBodyRefused,
        input(CDN_URL),
        public_cdn_answering!(ok(Some(PNG), EMPTY_CHUNKS)),
        CDN_TARGET,
        refused_after_exchange(SafeFetchFailureCodeV1::EmptyBody),
    ),
    fixture(
        SafeFetchCaseIdV1::BodyStallTimesOut,
        SafeFetchInputV1 { total_timeout: STALL_TIMEOUT, ..input(CDN_URL) },
        public_cdn_answering!(SafeFetchUpstreamResponseV1 {
            status: 200,
            content_type: Some(PNG),
            location: None,
            body: SafeFetchUpstreamBodyV1::StallAfter(ARTIFACT),
        }),
        CDN_TARGET,
        refused_after_exchange(SafeFetchFailureCodeV1::Timeout),
    ),
];

/// Returns the immutable canonical safe fetch fixture table.
#[must_use]
pub const fn safe_fetch_fixtures_v1() -> &'static [SafeFetchFixtureV1] {
    SAFE_FETCH_FIXTURES
}
