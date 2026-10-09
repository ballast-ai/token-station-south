//! Base64 (RFC 4648 §4 and §5), strict, for the references that read or write it.
//!
//! Crate-private on purpose: every component links this crate, and twenty lines of codec are
//! cheaper than a runtime dependency in every guest's lockfile and supply-chain checks
//! (`docs/design/2026-10-08-bedrock-invoke-anthropic-component.md` §13, I-Q4). The decoder is
//! strict in the ways that matter for evidence read off a wire: a symbol outside the alphabet, a
//! length no encoder produces, padding the mode does not allow, and non-zero bits after the last
//! whole byte are all refused, never repaired. Its decisions are pinned against the `base64` crate
//! (a dev-dependency only) by a property test below.

/// Which 64-symbol alphabet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alphabet {
    /// RFC 4648 §4: `+` and `/`.
    Standard,
    /// RFC 4648 §5: `-` and `_`.
    UrlSafe,
}

/// What a decoder accepts as trailing `=` padding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Padding {
    /// Exactly the padding that completes the last group of four symbols.
    Canonical,
    /// The canonical padding or less of it (RFC 7515 readers that meet both spellings).
    Indifferent,
}

impl Alphabet {
    const fn symbols(self) -> &'static [u8; 64] {
        match self {
            Self::Standard => b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
            Self::UrlSafe => b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
        }
    }

    fn sextet(self, symbol: u8) -> Option<u32> {
        self.symbols()
            .iter()
            .position(|candidate| *candidate == symbol)
            .and_then(|value| u32::try_from(value).ok())
    }
}

/// Encodes `bytes`, with canonical padding when `pad`.
pub fn encode(bytes: &[u8], alphabet: Alphabet, pad: bool) -> String {
    let symbols = alphabet.symbols();
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = chunk
            .iter()
            .enumerate()
            .fold(0usize, |group, (index, byte)| group | (usize::from(*byte) << (16 - 8 * index)));
        for index in 0..=chunk.len() {
            encoded.push(char::from(symbols[(group >> (18 - 6 * index)) & 0x3f]));
        }
        if pad {
            for _ in chunk.len()..3 {
                encoded.push('=');
            }
        }
    }
    encoded
}

/// Decodes `text`; `None` on anything an encoder in this mode would not have produced.
pub fn decode(text: &str, alphabet: Alphabet, padding: Padding) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let body_len = bytes.iter().rposition(|byte| *byte != b'=').map_or(0, |at| at + 1);
    let (body, pad) = bytes.split_at(body_len);
    let remainder = body.len() % 4;
    // One symbol carries six bits, less than a byte: no encoder ends a group there.
    if remainder == 1 {
        return None;
    }
    let canonical = if remainder == 0 { 0 } else { 4 - remainder };
    let pad_allowed = match padding {
        Padding::Canonical => pad.len() == canonical,
        Padding::Indifferent => pad.len() <= canonical,
    };
    if !pad_allowed {
        return None;
    }

    let mut decoded = Vec::with_capacity(body.len() * 3 / 4);
    let mut group = 0u32;
    let mut bits = 0u32;
    // A `=` inside the body is not in either alphabet, so it is refused here.
    for symbol in body {
        group = ((group << 6) | alphabet.sextet(*symbol)?) & 0xffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            decoded.push(u8::try_from((group >> bits) & 0xff).ok()?);
        }
    }
    // The bits after the last whole byte are zero in every canonical encoding; anything else is
    // a second spelling of the same bytes, which evidence must not have.
    if group & ((1 << bits) - 1) != 0 {
        return None;
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use super::{Alphabet, Padding, decode, encode};
    use base64::Engine as _;
    use base64::engine::general_purpose::{
        STANDARD, STANDARD_NO_PAD, STANDARD_PAD_INDIFFERENT, URL_SAFE, URL_SAFE_NO_PAD,
        URL_SAFE_PAD_INDIFFERENT,
    };
    use proptest::prelude::*;

    /// RFC 4648 §10.
    const VECTORS: [(&str, &str); 7] = [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ];

    #[test]
    fn the_rfc_4648_test_vectors_round_trip_in_both_alphabets() {
        for alphabet in [Alphabet::Standard, Alphabet::UrlSafe] {
            for (plain, encoded) in VECTORS {
                assert_eq!(encode(plain.as_bytes(), alphabet, true), encoded);
                assert_eq!(
                    decode(encoded, alphabet, Padding::Canonical).as_deref(),
                    Some(plain.as_bytes())
                );
                let bare = encoded.trim_end_matches('=');
                assert_eq!(encode(plain.as_bytes(), alphabet, false), bare);
                assert_eq!(
                    decode(bare, alphabet, Padding::Indifferent).as_deref(),
                    Some(plain.as_bytes())
                );
            }
        }
    }

    #[test]
    fn the_decoder_refuses_what_no_encoder_writes() {
        let standard = |text| decode(text, Alphabet::Standard, Padding::Canonical);
        // Non-zero trailing bits: `Zh==` and `Zm9=` spell `f` and `fo` a second way.
        assert_eq!(standard("Zh=="), None);
        assert_eq!(standard("Zm9="), None);
        // A length no group ends with, padding missing, padding in excess, padding inside.
        assert_eq!(standard("Z"), None);
        assert_eq!(standard("Zg"), None);
        assert_eq!(standard("Zg="), None);
        assert_eq!(standard("Zg==="), None);
        assert_eq!(standard("Zm9v="), None);
        assert_eq!(standard("Zg==Zg=="), None);
        assert_eq!(standard("===="), None);
        // The other alphabet's symbols, whitespace, and too much padding for the lenient mode.
        assert_eq!(standard("-_8="), None);
        assert_eq!(decode("+/8=", Alphabet::UrlSafe, Padding::Canonical), None);
        assert_eq!(standard("Zm9v\n"), None);
        assert_eq!(decode("Zm9v=", Alphabet::Standard, Padding::Indifferent), None);
        assert_eq!(standard("+/8=").as_deref(), Some(&b"\xfb\xff"[..]));
        assert_eq!(
            decode("-_8", Alphabet::UrlSafe, Padding::Indifferent).as_deref(),
            Some(&b"\xfb\xff"[..])
        );
    }

    /// Inputs drawn from both alphabets and the padding symbol, so the strings are often almost
    /// valid, which is where a decoder's decisions live.
    fn near_base64() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop::sample::select(
                b"AQgwZhm9v+/-_=08ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz\n ".to_vec(),
            ),
            0..24,
        )
        .prop_map(|symbols| symbols.into_iter().map(char::from).collect())
    }

    fn modes() -> [(Alphabet, Padding, &'static base64::engine::GeneralPurpose); 4] {
        [
            (Alphabet::Standard, Padding::Canonical, &STANDARD),
            (Alphabet::Standard, Padding::Indifferent, &STANDARD_PAD_INDIFFERENT),
            (Alphabet::UrlSafe, Padding::Canonical, &URL_SAFE),
            (Alphabet::UrlSafe, Padding::Indifferent, &URL_SAFE_PAD_INDIFFERENT),
        ]
    }

    proptest! {
        /// Every decision the decoder makes is the `base64` crate's, in every mode.
        #[test]
        fn the_decoder_agrees_with_the_base64_crate(text in near_base64()) {
            for (alphabet, padding, engine) in modes() {
                prop_assert_eq!(
                    decode(&text, alphabet, padding),
                    engine.decode(&text).ok(),
                    "{:?} {:?} on {:?}", alphabet, padding, text
                );
            }
        }

        /// Whatever the crate encodes, the decoder reads back, and the encoder writes the same.
        #[test]
        fn the_codec_round_trips_with_the_base64_crate(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
            for (alphabet, padding, engine) in modes() {
                let encoded = engine.encode(&bytes);
                let decoded = decode(&encoded, alphabet, padding);
                prop_assert_eq!(decoded.as_deref(), Some(bytes.as_slice()));
                if padding == Padding::Canonical {
                    prop_assert_eq!(encode(&bytes, alphabet, true), encoded);
                }
            }
            // The unpadded spelling RFC 7515 writes, which the lenient mode also reads.
            for (alphabet, engine) in [(Alphabet::Standard, &STANDARD_NO_PAD), (Alphabet::UrlSafe, &URL_SAFE_NO_PAD)] {
                let encoded = engine.encode(&bytes);
                prop_assert_eq!(encode(&bytes, alphabet, false), encoded.clone());
                let decoded = decode(&encoded, alphabet, Padding::Indifferent);
                prop_assert_eq!(decoded.as_deref(), Some(bytes.as_slice()));
            }
        }
    }
}
