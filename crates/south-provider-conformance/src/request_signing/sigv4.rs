//! The suite's own AWS Signature Version 4 arithmetic, used only to verify what a host emitted.
//!
//! It is not a signer a host could adopt: it covers exactly the requests the suite's fixtures
//! build (`https` URLs without a query or dot segments, headers sent once) and nothing more. The
//! unit tests pin it against AWS's published `get-vanilla` vector and a POST vector, both computed
//! with Python's standard `hmac` and `hashlib`, and its HMAC against RFC 4231.

use sha2::{Digest, Sha256};

const HMAC_BLOCK_BYTES: usize = 64;

/// HMAC-SHA256 (RFC 2104) over `sha2`.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0_u8; HMAC_BLOCK_BYTES];
    if key.len() > HMAC_BLOCK_BYTES {
        let digest: [u8; 32] = Sha256::digest(key).into();
        for (slot, byte) in block.iter_mut().zip(digest) {
            *slot = byte;
        }
    } else {
        for (slot, byte) in block.iter_mut().zip(key) {
            *slot = *byte;
        }
    }
    let inner_key = block.map(|byte| byte ^ 0x36);
    let outer_key = block.map(|byte| byte ^ 0x5c);
    let inner = Sha256::new().chain_update(inner_key).chain_update(message).finalize();
    Sha256::new().chain_update(outer_key).chain_update(inner).finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

/// Lowercase hex SHA-256.
pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The `SigV4` timestamp `YYYYMMDDTHHMMSSZ` of a Unix time in seconds (UTC, proleptic Gregorian);
/// `None` before the epoch or after year 9999.
pub(super) fn amz_date(unix_seconds: i64) -> Option<String> {
    let days = unix_seconds.div_euclid(86_400);
    let seconds_of_day = unix_seconds.rem_euclid(86_400);
    // Days to civil date (H. Hinnant, `civil_from_days`).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    if !(1970..=9999).contains(&year) {
        return None;
    }
    Some(format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        seconds_of_day / 3600,
        seconds_of_day % 3600 / 60,
        seconds_of_day % 60
    ))
}

/// Splits an `https` URL without userinfo, query or fragment into its host (with any port) and
/// its raw path.
pub(super) fn split_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://")?;
    if rest.contains(['?', '#', '@']) {
        return None;
    }
    Some(rest.find('/').map_or((rest, ""), |slash| rest.split_at(slash)))
}

/// The canonical URI of a non-S3 service: the raw path, already percent-encoded once on the wire,
/// encoded again with every byte outside the unreserved set and `/` escaped. A `%3A` in the path
/// becomes `%253A`.
pub(super) fn canonical_uri(raw_path: &str) -> String {
    if raw_path.is_empty() {
        return "/".to_owned();
    }
    let mut encoded = String::with_capacity(raw_path.len());
    for byte in raw_path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push_str(&hex(&[byte]).to_ascii_uppercase());
        }
    }
    encoded
}

/// A header value as the canonical request carries it: trimmed, inner whitespace runs collapsed.
pub(super) fn canonical_header_value(value: &str) -> String {
    value.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

/// The canonical request. `headers` are the signed headers in `SignedHeaders` order, each with its
/// canonical value; the query string is always empty.
pub(super) fn canonical_request(
    method: &str,
    canonical_uri: &str,
    headers: &[(&str, String)],
    payload_hash: &str,
) -> String {
    let signed_headers: Vec<&str> = headers.iter().map(|(name, _)| *name).collect();
    let mut request = format!("{method}\n{canonical_uri}\n\n");
    for (name, value) in headers {
        request.push_str(name);
        request.push(':');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    request.push_str(&signed_headers.join(";"));
    request.push('\n');
    request.push_str(payload_hash);
    request
}

/// What a signature is computed over besides the canonical request.
pub(super) struct ScopeV1<'a> {
    pub(super) amz_date: &'a str,
    pub(super) region: &'a str,
    pub(super) service: &'a str,
}

/// The lowercase hex signature of `canonical_request` under `secret`.
pub(super) fn signature(secret: &str, scope: &ScopeV1<'_>, canonical_request: &str) -> String {
    let date = scope.amz_date.get(..8).unwrap_or_default();
    let credential_scope = format!("{date}/{}/{}/aws4_request", scope.region, scope.service);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{credential_scope}\n{}",
        scope.amz_date,
        sha256_hex(canonical_request.as_bytes())
    );
    let mut key = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    for part in [scope.region, scope.service, "aws4_request"] {
        key = hmac_sha256(&key, part.as_bytes());
    }
    hex(&hmac_sha256(&key, string_to_sign.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{
        ScopeV1, amz_date, canonical_request, canonical_uri, hex, hmac_sha256, sha256_hex,
        signature, split_url,
    };

    /// AWS's published example credentials from the `SigV4` test suite.
    const SECRET: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";

    #[test]
    fn hmac_matches_rfc_4231() {
        // Test case 1.
        assert_eq!(
            hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        // Test case 6: a key longer than the block is hashed first.
        assert_eq!(
            hex(&hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn amz_date_formats_utc() {
        assert_eq!(amz_date(1_440_938_160).as_deref(), Some("20150830T123600Z"));
        assert_eq!(amz_date(951_782_400).as_deref(), Some("20000229T000000Z"));
        assert_eq!(amz_date(4_102_444_799).as_deref(), Some("20991231T235959Z"));
        assert_eq!(amz_date(0).as_deref(), Some("19700101T000000Z"));
        assert_eq!(amz_date(-1), None);
    }

    /// AWS `SigV4` test suite, `get-vanilla`. The expected signature is AWS's published one, and
    /// Python's `hmac` / `hashlib` reproduce it from the same canonical request.
    #[test]
    fn get_vanilla_vector() {
        let request = canonical_request(
            "GET",
            &canonical_uri(""),
            &[
                ("host", "example.amazonaws.com".to_owned()),
                ("x-amz-date", "20150830T123600Z".to_owned()),
            ],
            &sha256_hex(b""),
        );
        let scope =
            ScopeV1 { amz_date: "20150830T123600Z", region: "us-east-1", service: "service" };
        assert_eq!(
            signature(SECRET, &scope, &request),
            "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    /// A POST with a JSON body to a path whose model segment carries an encoded `:`. Computed with
    /// Python's `hmac` and `hashlib` from the canonical request written out by hand, including the
    /// doubly encoded `%253A`.
    #[test]
    fn post_with_body_vector() {
        let body = br#"{"messages":[{"role":"user","content":[{"text":"hi"}]}]}"#;
        let url = "https://bedrock-runtime.us-east-1.amazonaws.com/model/anthropic.claude-3-5-sonnet-20241022-v2%3A0/converse";
        let (host, path) = split_url(url).unwrap();
        let uri = canonical_uri(path);
        assert_eq!(uri, "/model/anthropic.claude-3-5-sonnet-20241022-v2%253A0/converse");
        let payload_hash = sha256_hex(body);
        assert_eq!(
            payload_hash,
            "c3335e07d6e89650bf94cb98664be69f8774dbc3198df221f113db0a59ea14f0"
        );
        let request = canonical_request(
            "POST",
            &uri,
            &[
                ("content-type", "application/json".to_owned()),
                ("host", host.to_owned()),
                ("x-amz-content-sha256", payload_hash.clone()),
                ("x-amz-date", "20150830T123600Z".to_owned()),
            ],
            &payload_hash,
        );
        let scope =
            ScopeV1 { amz_date: "20150830T123600Z", region: "us-east-1", service: "bedrock" };
        assert_eq!(
            signature(SECRET, &scope, &request),
            "1a22e28de2a10019a5a1b6354ddc6a2308b477d3250d6c82adb1ef8d8db6df4f"
        );
    }

    #[test]
    fn urls_outside_the_fixture_shape_are_not_split() {
        assert_eq!(split_url("https://example.com"), Some(("example.com", "")));
        assert_eq!(split_url("http://example.com/"), None);
        assert_eq!(split_url("https://example.com/?a=b"), None);
        assert_eq!(split_url("https://user@example.com/"), None);
    }
}
