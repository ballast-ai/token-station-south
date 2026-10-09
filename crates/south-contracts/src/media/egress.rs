//! The pure halves of the safe fetch executor (image record §11, D8b): the artifact URL grammar
//! and the forbidden destination ranges. Execution — DNS resolution, address pinning, the
//! connection itself — stays in the host; `south.safe-fetch.v1` (gate ③) holds the host to these
//! two functions.

use super::MediaLimitsV1;
use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};
use thiserror::Error;
use url::{Host, Url};

/// An absolute `https` URL a host may fetch an artifact from.
///
/// No userinfo, a host name or address, at most [`MediaLimitsV1::artifact_url_bytes`], and not
/// `localhost` or `*.localhost`. A literal address in a forbidden range is refused here; a name is
/// checked against [`is_forbidden_egress_address`] after the host resolves it.
#[derive(Clone, PartialEq, Eq)]
pub struct ArtifactUrlV1 {
    url: Url,
}

/// Why an artifact URL is refused. Diagnostics never echo the URL.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ArtifactUrlErrorV1 {
    /// Longer than the bound.
    #[error("artifact URL is too long")]
    TooLong,
    /// Not an absolute URL.
    #[error("artifact URL is not an absolute URL")]
    Invalid,
    /// Not `https`.
    #[error("artifact URL is not https")]
    NotHttps,
    /// Carries a user name or password.
    #[error("artifact URL carries userinfo")]
    Userinfo,
    /// No host, `localhost`, or a name under `.localhost`.
    #[error("artifact URL host is not allowed")]
    ForbiddenHost,
    /// A literal address in a forbidden range.
    #[error("artifact URL address is not allowed")]
    ForbiddenAddress,
}

impl ArtifactUrlV1 {
    /// Parses and checks one artifact URL.
    pub fn parse(input: &str) -> Result<Self, ArtifactUrlErrorV1> {
        if input.len() > MediaLimitsV1::V1.artifact_url_bytes {
            return Err(ArtifactUrlErrorV1::TooLong);
        }
        let url = Url::parse(input).map_err(|_| ArtifactUrlErrorV1::Invalid)?;
        if url.scheme() != "https" {
            return Err(ArtifactUrlErrorV1::NotHttps);
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(ArtifactUrlErrorV1::Userinfo);
        }
        match url.host() {
            None => return Err(ArtifactUrlErrorV1::ForbiddenHost),
            Some(Host::Domain(name)) => {
                let name = name.trim_end_matches('.');
                if name.is_empty()
                    || name.eq_ignore_ascii_case("localhost")
                    || name.to_ascii_lowercase().ends_with(".localhost")
                {
                    return Err(ArtifactUrlErrorV1::ForbiddenHost);
                }
            }
            Some(Host::Ipv4(address)) => {
                if is_forbidden_egress_address(IpAddr::V4(address)) {
                    return Err(ArtifactUrlErrorV1::ForbiddenAddress);
                }
            }
            Some(Host::Ipv6(address)) => {
                if is_forbidden_egress_address(IpAddr::V6(address)) {
                    return Err(ArtifactUrlErrorV1::ForbiddenAddress);
                }
            }
        }
        Ok(Self { url })
    }

    /// Returns the URL as the host fetches it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }

    /// Returns the host name or literal address.
    #[must_use]
    pub fn host_str(&self) -> &str {
        self.url.host_str().unwrap_or_default()
    }

    /// Returns the explicit or default port.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.url.port_or_known_default().unwrap_or(443)
    }
}

impl fmt::Debug for ArtifactUrlV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ArtifactUrlV1")
            .field("byte_count", &self.url.as_str().len())
            .finish_non_exhaustive()
    }
}

/// Whether a host must refuse to connect to `address` (image record §11 item 3).
///
/// IPv4: loopback, RFC 1918, link-local (the metadata address included), unspecified, broadcast,
/// multicast, `100.64.0.0/10`, `0.0.0.0/8`, `192.0.0.0/24`, `198.18.0.0/15` and `240.0.0.0/4`.
/// IPv6: loopback, unspecified, multicast, `fc00::/7`, `fe80::/10`, `fec0::/10`, and every form
/// that embeds an IPv4 address — v4-mapped `::ffff:0:0/96`, v4-compatible `::/96`, NAT64
/// `64:ff9b::/96` and `64:ff9b:1::/48`, and 6to4 `2002::/16` — whose embedded address is checked
/// as IPv4.
#[must_use]
pub fn is_forbidden_egress_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => forbidden_v4(address),
        IpAddr::V6(address) => forbidden_v6(address),
    }
}

fn forbidden_v4(address: Ipv4Addr) -> bool {
    let [first, second, third, _] = address.octets();
    address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_unspecified()
        || address.is_broadcast()
        || address.is_multicast()
        || first == 0
        || (first == 100 && (64..=127).contains(&second))
        || (first == 192 && second == 0 && third == 0)
        || (first == 198 && (second == 18 || second == 19))
        || first >= 240
}

fn forbidden_v6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    if address.is_loopback() || address.is_unspecified() || address.is_multicast() {
        return true;
    }
    if (segments[0] & 0xfe00) == 0xfc00 // fc00::/7
        || (segments[0] & 0xffc0) == 0xfe80 // fe80::/10
        || (segments[0] & 0xffc0) == 0xfec0
    // fec0::/10
    {
        return true;
    }
    let embedded = |high: u16, low: u16| {
        let [a, b] = high.to_be_bytes();
        let [c, d] = low.to_be_bytes();
        Ipv4Addr::new(a, b, c, d)
    };
    // v4-mapped ::ffff:a.b.c.d and v4-compatible ::a.b.c.d. The address is never permitted on
    // the strength of the embedding: a v4-mapped or v4-compatible destination is refused if its
    // embedded address is.
    if segments[..5] == [0; 5] && (segments[5] == 0xffff || segments[5] == 0) {
        return forbidden_v4(embedded(segments[6], segments[7]));
    }
    // NAT64 64:ff9b::/96 (well-known prefix) and 64:ff9b:1::/48 (local-use prefix).
    if segments[0] == 0x64 && segments[1] == 0xff9b {
        if segments[2..6] == [0; 4] {
            return forbidden_v4(embedded(segments[6], segments[7]));
        }
        if segments[2] == 1 {
            // RFC 8215 local use: the embedding may sit anywhere after the /48; refuse the whole
            // prefix rather than guess where a translator put the address.
            return true;
        }
    }
    // 6to4 2002::/16: the IPv4 address is bits 16–47.
    if segments[0] == 0x2002 {
        return forbidden_v4(embedded(segments[1], segments[2]));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forbidden(text: &str) -> bool {
        is_forbidden_egress_address(text.parse().expect("an address"))
    }

    #[test]
    fn the_record_vectors_are_refused() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "0.1.2.3",
            "255.255.255.255",
            "224.0.0.1",
            "100.64.0.1",
            "100.127.255.255",
            "192.0.0.1",
            "198.18.0.1",
            "198.19.255.255",
            "240.0.0.1",
            "::1",
            "::",
            "ff02::1",
            "fc00::1",
            "fd12::1",
            "fe80::1",
            "fec0::1",
            "::ffff:10.0.0.1",
            "::ffff:127.0.0.1",
            "::10.0.0.1",
            "64:ff9b::a00:1",
            "64:ff9b:1::1",
            "2002:a00:1::1",
            "2002:7f00:1::",
        ] {
            assert!(forbidden(address), "{address}");
        }
    }

    #[test]
    fn public_destinations_are_allowed() {
        for address in [
            "8.8.8.8",
            "1.1.1.1",
            "100.63.255.255",
            "100.128.0.0",
            "198.17.255.255",
            "198.20.0.0",
            "192.0.1.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:808:808::",
        ] {
            assert!(!forbidden(address), "{address}");
        }
    }

    #[test]
    fn artifact_urls() {
        let url = ArtifactUrlV1::parse("https://cdn.example.com/a.png?x=1").expect("valid");
        assert_eq!(url.host_str(), "cdn.example.com");
        assert_eq!(url.port(), 443);
        let cases = [
            ("http://cdn.example.com/a.png", ArtifactUrlErrorV1::NotHttps),
            ("https://user:pw@cdn.example.com/a", ArtifactUrlErrorV1::Userinfo),
            ("https://localhost/a", ArtifactUrlErrorV1::ForbiddenHost),
            ("https://x.LOCALHOST./a", ArtifactUrlErrorV1::ForbiddenHost),
            ("https://127.0.0.1/a", ArtifactUrlErrorV1::ForbiddenAddress),
            ("https://169.254.169.254/latest", ArtifactUrlErrorV1::ForbiddenAddress),
            ("https://[::ffff:10.0.0.1]/a", ArtifactUrlErrorV1::ForbiddenAddress),
            ("https://[64:ff9b::a00:1]/a", ArtifactUrlErrorV1::ForbiddenAddress),
            ("/relative", ArtifactUrlErrorV1::Invalid),
        ];
        for (input, expected) in cases {
            assert_eq!(ArtifactUrlV1::parse(input), Err(expected), "{input}");
        }
        let long = format!("https://cdn.example.com/{}", "a".repeat(8 * 1024));
        assert_eq!(ArtifactUrlV1::parse(&long), Err(ArtifactUrlErrorV1::TooLong));
    }
}
