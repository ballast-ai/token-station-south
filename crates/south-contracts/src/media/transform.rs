//! The closed transforms of `contracts.media` v1 (image record §6.4, speech record §6) and the
//! codecs under them.
//!
//! Every word names a public standard and is implemented once, here, as a pure function. The set
//! is closed: a new word is a `contracts.media` version change. The codecs are crate-private
//! rather than a `base64` dependency because every component links this crate (the
//! `south-component-conformance` base64 module makes the same choice, Invoke record I-Q4); their
//! decisions are pinned against the `base64` crate, a dev-dependency only, by a property test.

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// The longest media type a transform or descriptor carries.
pub const MAX_MEDIA_TYPE_BYTES: usize = 128;
/// The highest sample rate `wav_pcm_s16le` accepts, in hertz.
pub const MAX_WAV_SAMPLE_RATE: u32 = 384_000;
/// The most channels `wav_pcm_s16le` accepts.
pub const MAX_WAV_CHANNELS: u16 = 8;

/// One word of the closed transform vocabulary.
///
/// On the wire every word but one is a JSON string (`"as_is"`, `"base64"`, …); the parameterised
/// `wav_pcm_s16le` is `{"wav_pcm_s16le": {"sample_rate": …, "channels": …}}`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaTransformV1 {
    /// The original string or bytes, unchanged.
    AsIs,
    /// Bytes to a standard base64 string with canonical padding (RFC 4648 §4).
    Base64,
    /// Bytes to `data:<media_type>;base64,<base64>` (RFC 2397); needs a media type.
    DataUrl,
    /// A base64 `data:` URL to its bytes.
    FromDataUrl,
    /// A standard base64 string with canonical padding to its bytes.
    FromBase64,
    /// A hexadecimal string (either case, even length) to its bytes.
    FromHex,
    /// Several byte strings to their concatenation, in order. It takes a list, so it is never the
    /// transform of a single reference.
    Concat,
    /// Raw signed 16-bit little-endian PCM to a WAV file (RIFF, 44-byte header).
    WavPcmS16le {
        /// Samples per second, from 1 to [`MAX_WAV_SAMPLE_RATE`].
        sample_rate: u32,
        /// Interleaved channels, from 1 to [`MAX_WAV_CHANNELS`].
        channels: u16,
    },
}

impl fmt::Debug for MediaTransformV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AsIs => formatter.write_str("as_is"),
            Self::Base64 => formatter.write_str("base64"),
            Self::DataUrl => formatter.write_str("data_url"),
            Self::FromDataUrl => formatter.write_str("from_data_url"),
            Self::FromBase64 => formatter.write_str("from_base64"),
            Self::FromHex => formatter.write_str("from_hex"),
            Self::Concat => formatter.write_str("concat"),
            Self::WavPcmS16le { sample_rate, channels } => {
                write!(formatter, "wav_pcm_s16le({sample_rate} Hz, {channels} ch)")
            }
        }
    }
}

/// Why a transform refused its input. Diagnostics never echo the input.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum TransformErrorV1 {
    /// A word that reads text met bytes that are not UTF-8.
    #[error("transform input is not UTF-8 text")]
    NotText,
    /// Not standard base64 with canonical padding.
    #[error("transform input is not canonical base64")]
    InvalidBase64,
    /// Not an even-length hexadecimal string.
    #[error("transform input is not hexadecimal")]
    InvalidHex,
    /// Not a base64 `data:` URL.
    #[error("transform input is not a base64 data URL")]
    InvalidDataUrl,
    /// `data_url` without a media type, or a media type outside the grammar.
    #[error("transform needs a valid media type")]
    InvalidMediaType,
    /// `wav_pcm_s16le` parameters out of range, or PCM that is not whole frames, or too long for a
    /// RIFF size field.
    #[error("invalid wav_pcm_s16le input")]
    InvalidPcm,
    /// `concat` applied to one reference.
    #[error("concat takes a list, not one reference")]
    ConcatNeedsList,
}

impl MediaTransformV1 {
    /// Applies a single-input word to `input`.
    ///
    /// `media_type` is read by `data_url` only, and required there.
    pub fn apply(
        &self,
        input: &[u8],
        media_type: Option<&str>,
    ) -> Result<Vec<u8>, TransformErrorV1> {
        match self {
            Self::AsIs => Ok(input.to_vec()),
            Self::Base64 => Ok(encode_base64(input).into_bytes()),
            Self::DataUrl => {
                let media_type = media_type.ok_or(TransformErrorV1::InvalidMediaType)?;
                if !is_media_type(media_type) {
                    return Err(TransformErrorV1::InvalidMediaType);
                }
                Ok(format!("data:{media_type};base64,{}", encode_base64(input)).into_bytes())
            }
            Self::FromDataUrl => Ok(parse_data_url_v1(text(input)?)?.1),
            Self::FromBase64 => decode_base64(text(input)?),
            Self::FromHex => decode_hex(text(input)?),
            Self::Concat => Err(TransformErrorV1::ConcatNeedsList),
            Self::WavPcmS16le { sample_rate, channels } => {
                wav_pcm_s16le_v1(input, *sample_rate, *channels)
            }
        }
    }

    /// Whether the word reads its input as text (and therefore refuses bytes that are not UTF-8).
    #[must_use]
    pub const fn reads_text(&self) -> bool {
        matches!(self, Self::FromDataUrl | Self::FromBase64 | Self::FromHex)
    }

    /// Whether the word always produces text (so its result can stand as a JSON string).
    #[must_use]
    pub const fn produces_text(&self) -> bool {
        matches!(self, Self::Base64 | Self::DataUrl)
    }
}

fn text(input: &[u8]) -> Result<&str, TransformErrorV1> {
    std::str::from_utf8(input).map_err(|_| TransformErrorV1::NotText)
}

/// Concatenates byte strings in order (`concat`).
#[must_use]
pub fn concat_v1<T: AsRef<[u8]>>(parts: &[T]) -> Vec<u8> {
    let total = parts.iter().map(|part| part.as_ref().len()).sum();
    let mut out = Vec::with_capacity(total);
    for part in parts {
        out.extend_from_slice(part.as_ref());
    }
    out
}

/// Splits a base64 `data:` URL into its media type (when it names one) and its bytes.
///
/// The scheme and the `;base64` marker match ASCII case-insensitively (RFC 2397 §3, RFC 3986
/// §3.1). Parameters between the media type and `;base64` are allowed and dropped. A URL without
/// `;base64` (percent-encoded data) is refused: no image or speech client sends one, and decoding
/// it is a second grammar this contract would have to carry.
pub fn parse_data_url_v1(url: &str) -> Result<(Option<String>, Vec<u8>), TransformErrorV1> {
    let rest = strip_prefix_ignore_case(url, "data:").ok_or(TransformErrorV1::InvalidDataUrl)?;
    let (header, payload) = rest.split_once(',').ok_or(TransformErrorV1::InvalidDataUrl)?;
    let mut segments = header.split(';');
    let media_type = segments.next().unwrap_or_default();
    let mut base64 = false;
    for segment in segments {
        if segment.eq_ignore_ascii_case("base64") {
            base64 = true;
        } else if base64 {
            // `;base64` must be the last segment.
            return Err(TransformErrorV1::InvalidDataUrl);
        }
    }
    if !base64 {
        return Err(TransformErrorV1::InvalidDataUrl);
    }
    let media_type = if media_type.is_empty() {
        None
    } else if is_media_type(media_type) {
        Some(media_type.to_owned())
    } else {
        return Err(TransformErrorV1::InvalidDataUrl);
    };
    let bytes = decode_base64(payload).map_err(|_| TransformErrorV1::InvalidDataUrl)?;
    Ok((media_type, bytes))
}

fn strip_prefix_ignore_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &value[prefix.len()..])
}

/// A media type `type/subtype` of RFC 7230 `tchar`s, at most [`MAX_MEDIA_TYPE_BYTES`], without
/// parameters. Case is kept as written.
#[must_use]
pub fn is_media_type(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_MEDIA_TYPE_BYTES {
        return false;
    }
    let Some((kind, subtype)) = value.split_once('/') else {
        return false;
    };
    let token = |part: &str| !part.is_empty() && part.bytes().all(is_tchar);
    token(kind) && token(subtype)
}

const fn is_tchar(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// Wraps raw signed 16-bit little-endian PCM in a 44-byte RIFF/WAVE header.
pub fn wav_pcm_s16le_v1(
    pcm: &[u8],
    sample_rate: u32,
    channels: u16,
) -> Result<Vec<u8>, TransformErrorV1> {
    if !(1..=MAX_WAV_SAMPLE_RATE).contains(&sample_rate)
        || !(1..=MAX_WAV_CHANNELS).contains(&channels)
    {
        return Err(TransformErrorV1::InvalidPcm);
    }
    let block_align = 2 * u32::from(channels);
    let data_len = u32::try_from(pcm.len()).map_err(|_| TransformErrorV1::InvalidPcm)?;
    if !data_len.is_multiple_of(block_align) {
        return Err(TransformErrorV1::InvalidPcm);
    }
    let riff_len = data_len.checked_add(36).ok_or(TransformErrorV1::InvalidPcm)?;
    let byte_rate = sample_rate.checked_mul(block_align).ok_or(TransformErrorV1::InvalidPcm)?;
    let mut out = Vec::with_capacity(pcm.len() + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_len.to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    // `block_align` is at most 16, so it fits.
    out.extend_from_slice(&u16::try_from(block_align).unwrap_or(u16::MAX).to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    Ok(out)
}

const BASE64_SYMBOLS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with canonical padding (RFC 4648 §4).
#[must_use]
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |group, (index, byte)| group | (u32::from(*byte) << (16 - 8 * index)));
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(char::from(BASE64_SYMBOLS[((group >> (18 - 6 * index)) & 0x3f) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

const fn base64_value(symbol: u8) -> Option<u32> {
    match symbol {
        b'A'..=b'Z' => Some((symbol - b'A') as u32),
        b'a'..=b'z' => Some((symbol - b'a') as u32 + 26),
        b'0'..=b'9' => Some((symbol - b'0') as u32 + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decodes standard base64 with canonical padding; anything an encoder would not have produced
/// (whitespace, a missing or extra `=`, non-zero trailing bits, the URL-safe alphabet) is refused.
pub fn decode_base64(text: &str) -> Result<Vec<u8>, TransformErrorV1> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(TransformErrorV1::InvalidBase64);
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let groups = bytes.len() / 4;
    for (index, group) in bytes.chunks_exact(4).enumerate() {
        let last = index + 1 == groups;
        let padding = group.iter().rev().take_while(|symbol| **symbol == b'=').count();
        if padding > 2 || (padding > 0 && !last) {
            return Err(TransformErrorV1::InvalidBase64);
        }
        let mut value = 0_u32;
        for symbol in &group[..4 - padding] {
            value = (value << 6) | base64_value(*symbol).ok_or(TransformErrorV1::InvalidBase64)?;
        }
        value <<= 6 * padding;
        let produced = 3 - padding;
        // Canonical: the bits after the last whole byte are zero.
        let unused: u32 = if padding == 2 {
            16
        } else if padding == 1 {
            8
        } else {
            0
        };
        if unused > 0 && value & ((1 << unused) - 1) != 0 {
            return Err(TransformErrorV1::InvalidBase64);
        }
        for shift in 0..produced {
            out.push(((value >> (16 - 8 * shift)) & 0xff) as u8);
        }
    }
    Ok(out)
}

/// Decodes an even-length hexadecimal string, either case.
pub fn decode_hex(text: &str) -> Result<Vec<u8>, TransformErrorV1> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err(TransformErrorV1::InvalidHex);
    }
    bytes
        .chunks_exact(2)
        .map(|pair| {
            let high = char::from(pair[0]).to_digit(16).ok_or(TransformErrorV1::InvalidHex)?;
            let low = char::from(pair[1]).to_digit(16).ok_or(TransformErrorV1::InvalidHex)?;
            // Two hex digits make one byte.
            Ok(u8::try_from((high << 4) | low).unwrap_or(u8::MAX))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use proptest::prelude::*;

    #[test]
    fn words_serialize_as_specified() {
        let words = [
            (MediaTransformV1::AsIs, r#""as_is""#),
            (MediaTransformV1::Base64, r#""base64""#),
            (MediaTransformV1::DataUrl, r#""data_url""#),
            (MediaTransformV1::FromDataUrl, r#""from_data_url""#),
            (MediaTransformV1::FromBase64, r#""from_base64""#),
            (MediaTransformV1::FromHex, r#""from_hex""#),
            (MediaTransformV1::Concat, r#""concat""#),
            (
                MediaTransformV1::WavPcmS16le { sample_rate: 24_000, channels: 1 },
                r#"{"wav_pcm_s16le":{"sample_rate":24000,"channels":1}}"#,
            ),
        ];
        for (word, json) in words {
            assert_eq!(serde_json::to_string(&word).expect("serializable"), json);
            assert_eq!(serde_json::from_str::<MediaTransformV1>(json).expect("parses"), word);
        }
        assert!(serde_json::from_str::<MediaTransformV1>(r#""gzip""#).is_err());
    }

    #[test]
    fn data_urls() {
        assert_eq!(
            parse_data_url_v1("DATA:image/png;param=1;BASE64,aGk=").expect("valid"),
            (Some("image/png".to_owned()), b"hi".to_vec())
        );
        assert_eq!(parse_data_url_v1("data:;base64,").expect("valid"), (None, Vec::new()));
        for bad in [
            "data:image/png,aGk=",
            "data:image/png;base64;x=1,aGk=",
            "data:image png;base64,aGk=",
            "http:image/png;base64,aGk=",
            "data:image/png;base64,aGk",
        ] {
            assert_eq!(parse_data_url_v1(bad), Err(TransformErrorV1::InvalidDataUrl), "{bad}");
        }
        assert_eq!(
            MediaTransformV1::DataUrl.apply(b"hi", Some("image/png")).expect("applies"),
            b"data:image/png;base64,aGk=".to_vec()
        );
        assert_eq!(
            MediaTransformV1::DataUrl.apply(b"hi", None),
            Err(TransformErrorV1::InvalidMediaType)
        );
    }

    #[test]
    fn hex_and_concat() {
        assert_eq!(decode_hex("00ffA0").expect("valid"), vec![0, 255, 160]);
        assert_eq!(decode_hex("0"), Err(TransformErrorV1::InvalidHex));
        assert_eq!(decode_hex("zz"), Err(TransformErrorV1::InvalidHex));
        assert_eq!(concat_v1(&[b"ab".as_slice(), b"", b"c"]), b"abc".to_vec());
        assert_eq!(
            MediaTransformV1::Concat.apply(b"x", None),
            Err(TransformErrorV1::ConcatNeedsList)
        );
    }

    #[test]
    fn wav_header_matches_the_riff_layout() {
        let wav = wav_pcm_s16le_v1(&[1, 0, 2, 0], 24_000, 1).expect("valid");
        assert_eq!(wav.len(), 48);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().expect("4")), 40);
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().expect("4")), 24_000);
        assert_eq!(u32::from_le_bytes(wav[28..32].try_into().expect("4")), 48_000);
        assert_eq!(u16::from_le_bytes(wav[32..34].try_into().expect("2")), 2);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().expect("4")), 4);
        assert_eq!(&wav[44..], &[1, 0, 2, 0]);
        assert_eq!(wav_pcm_s16le_v1(&[1], 24_000, 1), Err(TransformErrorV1::InvalidPcm));
        assert_eq!(wav_pcm_s16le_v1(&[1, 0], 24_000, 2), Err(TransformErrorV1::InvalidPcm));
        assert_eq!(wav_pcm_s16le_v1(&[], 0, 1), Err(TransformErrorV1::InvalidPcm));
        assert_eq!(wav_pcm_s16le_v1(&[], 8000, 9), Err(TransformErrorV1::InvalidPcm));
    }

    #[test]
    fn base64_refuses_non_canonical_text() {
        for bad in ["a", "aGk", "aGk==", "aG=k", "aGl=", "aGk=aGk=", "a Gk=", "aGk-", "===="] {
            assert_eq!(decode_base64(bad), Err(TransformErrorV1::InvalidBase64), "{bad}");
        }
        assert_eq!(decode_base64("").expect("empty"), Vec::<u8>::new());
    }

    proptest! {
        #[test]
        fn base64_agrees_with_the_base64_crate(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
            let reference = base64::engine::general_purpose::STANDARD.encode(&bytes);
            prop_assert_eq!(&encode_base64(&bytes), &reference);
            prop_assert_eq!(decode_base64(&reference).expect("canonical"), bytes);
        }

        #[test]
        fn base64_decisions_agree_with_the_base64_crate(text in "[A-Za-z0-9+/=]{0,16}") {
            let reference = base64::engine::general_purpose::STANDARD.decode(&text).ok();
            prop_assert_eq!(decode_base64(&text).ok(), reference);
        }
    }
}
