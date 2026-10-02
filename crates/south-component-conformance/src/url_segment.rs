//! One path segment of a request URL, shared by the references that place the model in the
//! path and by gate ②'s `RequestFactsHonoured`, so the check and the builders agree byte for
//! byte (B2, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §7.2).

/// Percent-encodes `value` as one path segment (RFC 3986 `pchar`): unreserved characters,
/// sub-delims, `:` and `@` stay; everything else — `/`, `?`, `#`, `%`, spaces and non-ASCII bytes
/// included — is encoded, so a model such as an inference-profile ARN cannot split the path.
pub fn encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";
