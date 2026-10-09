//! `contracts.media` v1: the vocabulary both media worlds share (image record §6, speech record
//! §6), and the host-side pure functions that keep bytes out of the sandbox.
//!
//! Normative record: `docs/design/2026-09-30-image-world.md` §6 and §11. The host holds the bytes;
//! a component sees a view in which large strings and file parts are replaced by references
//! ([`elide_v1`], [`parse_multipart_parts_v1`]), and describes the upstream request by reference
//! ([`MediaRequestDescriptorV1`]); the host expands the references
//! ([`expand_json_template_v1`], [`encode_multipart_v1`]) with the closed transforms
//! ([`MediaTransformV1`]). Every function here is pure: no I/O, no clock, no randomness (a
//! multipart boundary is the caller's). The SSE decoder the speech world needs ships separately
//! (host plan B6, Q-B6-6).

mod descriptor;
mod egress;
mod elide;
pub(crate) mod json;
mod multipart;
mod response;
mod transform;

pub use descriptor::{
    BLOB_KEY, MAX_MEDIA_TEXT_BODY_BYTES, MEDIA_TEXT_BODY_MEDIA_TYPES, MediaAuthV1, MediaBodyV1,
    MediaDescriptorErrorV1, MediaReferenceV1, MediaRequestDescriptorV1, MediaTextBodyV1,
    REFERENCE_KEY, expand_json_template_v1,
};
pub use egress::{ArtifactUrlErrorV1, ArtifactUrlV1, is_forbidden_egress_address};
pub use elide::{
    ElidedViewV1, ElisionErrorV1, InvalidPathPatternV1, PathPatternV1, elide_from, elide_v1,
};
pub use json::MAX_MEDIA_JSON_DEPTH;
pub use multipart::{
    MediaPartV1, MediaPartViewV1, MultipartErrorV1, MultipartViewV1, encode_multipart_v1,
    parse_multipart_parts_v1,
};
pub use response::{
    MediaRequestViewV1, MediaResponseBodyV1, MediaResponseErrorV1, MediaResponseViewV1,
    ResponseBodyFormV1, UpstreamRoundV1, build_media_response_view_v1,
};
pub use transform::{
    MAX_MEDIA_TYPE_BYTES, MAX_WAV_CHANNELS, MAX_WAV_SAMPLE_RATE, MediaTransformV1,
    TransformErrorV1, concat_v1, decode_base64, decode_hex, encode_base64, is_media_type,
    parse_data_url_v1, wav_pcm_s16le_v1,
};

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};
use thiserror::Error;

/// `contracts.media` v1: the whole vocabulary of both media worlds, released once (image record
/// §6.4).
pub const MEDIA_CONTRACT_VERSION: u16 = 1;

/// Keys under this prefix are the placeholder namespace (`$south.blob`, `$south.ref`,
/// `$south.artifact`); a document that uses one is refused (image record §6.1, §6.5).
pub const RESERVED_KEY_PREFIX: &str = "$south.";

/// The bounds of `contracts.media` v1 (`compatibility.json` `media_limits`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaLimitsV1 {
    /// A string, text part or response text longer than this many decoded bytes is elided
    /// wherever it stands (§6.2, the fallback threshold).
    pub inline_string_bytes: usize,
    /// The most elision paths a component declares on one side (§6.2).
    pub elision_paths: usize,
    /// The longest elision path, in bytes (§6.2).
    pub elision_path_bytes: usize,
    /// The bytes of each blob's `head` (§6.2).
    pub blob_head_bytes: usize,
    /// The most rounds one prepared call may repeat (§7, `repeat`).
    pub repeat: u8,
    /// The longest artifact URL, in bytes (§10.1; the same value as the task world's
    /// `MAX_ARTIFACT_REF_BYTES`).
    pub artifact_url_bytes: usize,
    /// The most parts a multipart request or descriptor carries.
    pub parts: usize,
    /// The bytes of one part's header block (multipart parsing).
    pub part_header_bytes: usize,
    /// The serialized `state` a component hands back through the host (§7).
    pub state_bytes: usize,
}

impl MediaLimitsV1 {
    /// The limits of `contracts.media` v1.
    pub const V1: Self = Self {
        inline_string_bytes: 1024 * 1024,
        elision_paths: 32,
        elision_path_bytes: 256,
        blob_head_bytes: 64,
        repeat: 10,
        artifact_url_bytes: 8 * 1024,
        parts: 64,
        part_header_bytes: 8 * 1024,
        state_bytes: 8 * 1024,
    };
}

impl Default for MediaLimitsV1 {
    fn default() -> Self {
        Self::V1
    }
}

/// A blob id local to one view: `b0`, `b1`, … (no leading zeros).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BlobIdV1 {
    text: String,
}

/// A malformed blob id.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("invalid media blob id")]
pub struct InvalidBlobIdV1;

impl BlobIdV1 {
    /// The id of the `index`th blob.
    #[must_use]
    pub fn from_index(index: u32) -> Self {
        Self { text: format!("b{index}") }
    }

    /// Parses `b<decimal>` with no leading zeros and an index that fits a `u32`.
    pub fn parse(text: &str) -> Result<Self, InvalidBlobIdV1> {
        let digits = text.strip_prefix('b').ok_or(InvalidBlobIdV1)?;
        let well_formed = !digits.is_empty()
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && (digits == "0" || !digits.starts_with('0'))
            && digits.parse::<u32>().is_ok();
        if well_formed { Ok(Self { text: text.to_owned() }) } else { Err(InvalidBlobIdV1) }
    }

    /// Returns the id as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl TryFrom<String> for BlobIdV1 {
    type Error = InvalidBlobIdV1;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<BlobIdV1> for String {
    fn from(value: BlobIdV1) -> Self {
        value.text
    }
}

impl fmt::Debug for BlobIdV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text)
    }
}

/// The bytes behind one reference, held by the host and never handed to the component.
#[derive(Clone, PartialEq, Eq)]
pub struct BlobV1 {
    pub(crate) id: BlobIdV1,
    pub(crate) bytes: Vec<u8>,
    /// Whether the bytes came from text (a JSON string or a text part) rather than a file part.
    pub(crate) text: bool,
}

impl BlobV1 {
    /// A blob of file bytes, for a host that splits its own parts.
    #[must_use]
    pub const fn file(id: BlobIdV1, bytes: Vec<u8>) -> Self {
        Self { id, bytes, text: false }
    }

    /// A blob of text.
    #[must_use]
    pub const fn text(id: BlobIdV1, value: String) -> Self {
        Self { id, bytes: value.into_bytes(), text: true }
    }

    /// Returns the blob's id.
    #[must_use]
    pub const fn id(&self) -> &BlobIdV1 {
        &self.id
    }

    /// Returns the blob's bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether the blob holds text (a JSON string or a text part).
    #[must_use]
    pub const fn is_text(&self) -> bool {
        self.text
    }
}

impl fmt::Debug for BlobV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BlobV1")
            .field("id", &self.id)
            .field("byte_count", &self.bytes.len())
            .field("text", &self.text)
            .finish()
    }
}

/// The blobs of one view, by id, for expansion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlobSetV1 {
    blobs: BTreeMap<BlobIdV1, BlobV1>,
}

impl BlobSetV1 {
    /// Collects blobs; a repeated id is refused.
    pub fn new(blobs: impl IntoIterator<Item = BlobV1>) -> Result<Self, InvalidBlobIdV1> {
        let mut set = BTreeMap::new();
        for blob in blobs {
            if set.insert(blob.id.clone(), blob).is_some() {
                return Err(InvalidBlobIdV1);
            }
        }
        Ok(Self { blobs: set })
    }

    /// Looks a blob up by id.
    #[must_use]
    pub fn get(&self, id: &BlobIdV1) -> Option<&BlobV1> {
        self.blobs.get(id)
    }

    /// Whether the set holds a blob with this id.
    #[must_use]
    pub fn contains(&self, id: &BlobIdV1) -> bool {
        self.blobs.contains_key(id)
    }

    /// The number of blobs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_ids() {
        assert_eq!(BlobIdV1::from_index(12).as_str(), "b12");
        for good in ["b0", "b7", "b4294967295"] {
            assert!(BlobIdV1::parse(good).is_ok(), "{good}");
        }
        for bad in ["", "b", "b01", "b-1", "B1", "b4294967296", "b1 "] {
            assert!(BlobIdV1::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn limits_serialize_for_compatibility_json() {
        let json = serde_json::to_value(MediaLimitsV1::V1).expect("serializable");
        assert_eq!(json["inline_string_bytes"], 1_048_576);
        assert_eq!(json["repeat"], 10);
        assert_eq!(json["parts"], 64);
    }
}
