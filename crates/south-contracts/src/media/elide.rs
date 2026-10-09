//! `elide_v1` (image record §6.2): the deterministic rule that takes large strings out of a JSON
//! document before it enters a sandbox, leaving a placeholder that describes each one.

use super::json::{self, Member, Node, ParseError};
use super::{BlobIdV1, BlobV1, MediaLimitsV1, RESERVED_KEY_PREFIX};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fmt};
use thiserror::Error;

/// One declared elision path: an RFC 6901 JSON Pointer in which a segment that is exactly `*`
/// matches any array index or object key.
///
/// It is not empty (the whole document is never one declared string), it starts with `/`, it is
/// at most [`MediaLimitsV1::elision_path_bytes`] long, and its escapes are `~0` and `~1` only. A
/// literal key `*` cannot be named; no upstream uses one.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PathPatternV1 {
    text: String,
    segments: Vec<Segment>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Segment {
    Any,
    Exact(String),
}

/// A refused elision path.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("invalid media elision path")]
pub struct InvalidPathPatternV1;

impl PathPatternV1 {
    /// Parses one pattern.
    pub fn parse(text: &str) -> Result<Self, InvalidPathPatternV1> {
        if text.is_empty()
            || text.len() > MediaLimitsV1::V1.elision_path_bytes
            || !text.starts_with('/')
        {
            return Err(InvalidPathPatternV1);
        }
        let segments = text[1..]
            .split('/')
            .map(|raw| {
                if raw == "*" {
                    return Ok(Segment::Any);
                }
                unescape_token(raw).map(Segment::Exact)
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { text: text.to_owned(), segments })
    }

    /// Returns the pattern as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    fn matches(&self, path: &[PathStep]) -> bool {
        self.segments.len() == path.len()
            && self.segments.iter().zip(path).all(|(segment, step)| match (segment, step) {
                (Segment::Any, _) => true,
                (Segment::Exact(key), PathStep::Key(actual)) => key == actual,
                (Segment::Exact(index), PathStep::Index(actual)) => index == &actual.to_string(),
            })
    }
}

/// Decodes one RFC 6901 reference token (`~1` is `/`, `~0` is `~`; any other `~` is invalid).
pub(super) fn unescape_token(raw: &str) -> Result<String, InvalidPathPatternV1> {
    let mut out = String::with_capacity(raw.len());
    let mut characters = raw.chars();
    while let Some(character) = characters.next() {
        if character == '~' {
            match characters.next() {
                Some('0') => out.push('~'),
                Some('1') => out.push('/'),
                _ => return Err(InvalidPathPatternV1),
            }
        } else {
            out.push(character);
        }
    }
    Ok(out)
}

impl TryFrom<String> for PathPatternV1 {
    type Error = InvalidPathPatternV1;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<PathPatternV1> for String {
    fn from(value: PathPatternV1) -> Self {
        value.text
    }
}

impl fmt::Debug for PathPatternV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("PathPatternV1").field(&self.text).finish()
    }
}

/// A view after elision: compact JSON, members in source order, numbers as their source text,
/// and each elided string replaced by `{"$south.blob": {"id": …, "bytes": …, "head": …}}`.
#[derive(Clone, PartialEq, Eq)]
pub struct ElidedViewV1 {
    json: String,
}

impl ElidedViewV1 {
    /// Returns the view's JSON text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.json
    }

    /// Returns the view's length in bytes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.json.len()
    }

    /// Whether the view is empty — never true for a view `elide_v1` produced.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.json.is_empty()
    }

    /// Consumes the view, returning its JSON text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.json
    }
}

impl fmt::Debug for ElidedViewV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ElidedViewV1").field("byte_count", &self.json.len()).finish()
    }
}

/// Why `elide_v1` refused a document. On the request side every variant is the host's 400 before
/// admission; on the response side the round is `unknown`.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ElisionErrorV1 {
    /// Not UTF-8, or not one RFC 8259 value.
    #[error("document is not valid JSON")]
    InvalidJson,
    /// Nested deeper than the parser's bound.
    #[error("document is nested too deeply")]
    TooDeep,
    /// An object repeats a key (compared after unescaping).
    #[error("document repeats an object key")]
    DuplicateKey,
    /// An object key is longer than the fallback threshold.
    #[error("document has an object key above the fallback threshold")]
    KeyTooLong,
    /// An object key starts with `$south.`, the placeholder namespace.
    #[error("document uses the reserved $south. key namespace")]
    ReservedKey,
    /// More declared paths than [`MediaLimitsV1::elision_paths`].
    #[error("too many declared elision paths")]
    TooManyPaths,
}

impl From<ParseError> for ElisionErrorV1 {
    fn from(error: ParseError) -> Self {
        match error {
            ParseError::Syntax => Self::InvalidJson,
            ParseError::TooDeep => Self::TooDeep,
        }
    }
}

#[derive(Debug, Clone)]
enum PathStep {
    Key(String),
    Index(usize),
}

/// Elides `document` under `declared` and `limits` (image record §6.2).
///
/// A JSON string is replaced when its path matches a declared pattern (whatever its length) or
/// when its decoded length exceeds [`MediaLimitsV1::inline_string_bytes`]. Traversal is depth
/// first in source order, so ids are `b0`, `b1`, … in document order. `bytes` is the decoded
/// length; `head` the first [`MediaLimitsV1::blob_head_bytes`] decoded bytes cut back to a UTF-8
/// boundary. The returned blobs hold each decoded string, in id order.
pub fn elide_v1(
    document: &[u8],
    declared: &[PathPatternV1],
    limits: &MediaLimitsV1,
) -> Result<(ElidedViewV1, Vec<BlobV1>), ElisionErrorV1> {
    elide_from(document, declared, limits, 0)
}

/// [`elide_v1`] with the id sequence starting at `first_id`, for a host that elides several
/// documents into one view (the `sse` response form, one document per event).
pub fn elide_from(
    document: &[u8],
    declared: &[PathPatternV1],
    limits: &MediaLimitsV1,
    first_id: u32,
) -> Result<(ElidedViewV1, Vec<BlobV1>), ElisionErrorV1> {
    if declared.len() > limits.elision_paths {
        return Err(ElisionErrorV1::TooManyPaths);
    }
    let mut node = json::parse(document)?;
    let mut state = State { declared, limits, blobs: Vec::new(), next_id: first_id };
    let mut path = Vec::new();
    state.visit(&mut node, &mut path)?;
    let mut out = String::with_capacity(document.len().min(1 << 20));
    json::write_compact(&node, &mut out);
    Ok((ElidedViewV1 { json: out }, state.blobs))
}

struct State<'a> {
    declared: &'a [PathPatternV1],
    limits: &'a MediaLimitsV1,
    blobs: Vec<BlobV1>,
    next_id: u32,
}

impl State<'_> {
    fn visit(&mut self, node: &mut Node, path: &mut Vec<PathStep>) -> Result<(), ElisionErrorV1> {
        match node {
            Node::Scalar(_) => Ok(()),
            Node::String(string) => {
                let declared = self.declared.iter().any(|pattern| pattern.matches(path));
                if declared || string.decoded.len() > self.limits.inline_string_bytes {
                    let decoded = std::mem::take(&mut string.decoded);
                    *node = self.placeholder(decoded);
                }
                Ok(())
            }
            Node::Array(items) => {
                for (index, item) in items.iter_mut().enumerate() {
                    path.push(PathStep::Index(index));
                    let result = self.visit(item, path);
                    path.pop();
                    result?;
                }
                Ok(())
            }
            Node::Object(members) => {
                let mut seen = HashSet::with_capacity(members.len());
                for Member { key, .. } in members.iter() {
                    if key.decoded.starts_with(RESERVED_KEY_PREFIX) {
                        return Err(ElisionErrorV1::ReservedKey);
                    }
                    if key.decoded.len() > self.limits.inline_string_bytes {
                        return Err(ElisionErrorV1::KeyTooLong);
                    }
                    if !seen.insert(key.decoded.as_str()) {
                        return Err(ElisionErrorV1::DuplicateKey);
                    }
                }
                for Member { key, value } in members.iter_mut() {
                    path.push(PathStep::Key(key.decoded.clone()));
                    let result = self.visit(value, path);
                    path.pop();
                    result?;
                }
                Ok(())
            }
        }
    }

    fn placeholder(&mut self, decoded: String) -> Node {
        let id = BlobIdV1::from_index(self.next_id);
        self.next_id += 1;
        let head = utf8_prefix(&decoded, self.limits.blob_head_bytes);
        let mut text = String::new();
        text.push_str("{\"$south.blob\":{\"id\":\"");
        text.push_str(id.as_str());
        text.push_str("\",\"bytes\":");
        text.push_str(&decoded.len().to_string());
        text.push_str(",\"head\":");
        json::write_string(head, &mut text);
        text.push_str("}}");
        self.blobs.push(BlobV1 { id, bytes: decoded.into_bytes(), text: true });
        Node::Scalar(text)
    }
}

/// The longest prefix of `value` of at most `limit` bytes that ends on a UTF-8 boundary.
pub(super) fn utf8_prefix(value: &str, limit: usize) -> &str {
    if value.len() <= limit {
        return value;
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns(texts: &[&str]) -> Vec<PathPatternV1> {
        texts.iter().map(|text| PathPatternV1::parse(text).expect("valid")).collect()
    }

    fn small_limits() -> MediaLimitsV1 {
        MediaLimitsV1 { inline_string_bytes: 8, ..MediaLimitsV1::V1 }
    }

    #[test]
    fn declared_paths_are_elided_whatever_their_length() {
        let (view, blobs) = elide_v1(
            br#"{"prompt":"a cat","images":[{"url":"x"},{"url":"data:image/png;base64,AAAA"}]}"#,
            &patterns(&["/images/*/url"]),
            &MediaLimitsV1::V1,
        )
        .expect("elides");
        assert_eq!(
            view.as_str(),
            r#"{"prompt":"a cat","images":[{"url":{"$south.blob":{"id":"b0","bytes":1,"head":"x"}}},{"url":{"$south.blob":{"id":"b1","bytes":26,"head":"data:image/png;base64,AAAA"}}}]}"#
        );
        assert_eq!(blobs.len(), 2);
        assert_eq!(blobs[1].id.as_str(), "b1");
        assert_eq!(blobs[1].bytes, b"data:image/png;base64,AAAA");
    }

    #[test]
    fn the_fallback_threshold_elides_any_long_string_and_counts_decoded_bytes() {
        // Nine decoded bytes behind fourteen source bytes: elided by the decoded length.
        let (view, blobs) =
            elide_v1(b"[\"\\u00e9\\u00e9\\u00e9abc\", \"12345678\"]", &[], &small_limits())
                .expect("elides");
        assert_eq!(
            view.as_str(),
            "[{\"$south.blob\":{\"id\":\"b0\",\"bytes\":9,\"head\":\"\u{e9}\u{e9}\u{e9}abc\"}},\"12345678\"]"
        );
        assert_eq!(blobs[0].bytes, "\u{e9}\u{e9}\u{e9}abc".as_bytes());
    }

    #[test]
    fn head_is_cut_back_to_a_character_boundary() {
        let value = format!("{}\u{e9}", "a".repeat(63));
        let document = serde_json::to_vec(&serde_json::json!([value])).expect("json");
        let (view, _) =
            elide_v1(&document, &patterns(&["/0"]), &MediaLimitsV1::V1).expect("elides");
        assert!(view.as_str().contains(&format!("\"head\":\"{}\"", "a".repeat(63))));
    }

    #[test]
    fn duplicate_reserved_and_long_keys_are_refused() {
        assert_eq!(
            elide_v1(br#"{"a":1,"a":2}"#, &[], &MediaLimitsV1::V1),
            Err(ElisionErrorV1::DuplicateKey)
        );
        assert_eq!(
            elide_v1(br#"{"x":{"$south.ref":1}}"#, &[], &MediaLimitsV1::V1),
            Err(ElisionErrorV1::ReservedKey)
        );
        assert_eq!(
            elide_v1(br#"{"123456789":1}"#, &[], &small_limits()),
            Err(ElisionErrorV1::KeyTooLong)
        );
        assert_eq!(elide_v1(b"{", &[], &MediaLimitsV1::V1), Err(ElisionErrorV1::InvalidJson));
    }

    #[test]
    fn index_segments_and_escapes_match() {
        let (view, _) = elide_v1(
            br#"{"a/b":["x","y"],"c~d":"z"}"#,
            &patterns(&["/a~1b/1", "/c~0d"]),
            &MediaLimitsV1::V1,
        )
        .expect("elides");
        assert_eq!(
            view.as_str(),
            r#"{"a/b":["x",{"$south.blob":{"id":"b0","bytes":1,"head":"y"}}],"c~d":{"$south.blob":{"id":"b1","bytes":1,"head":"z"}}}"#
        );
    }

    #[test]
    fn non_string_values_on_declared_paths_stay() {
        let (view, blobs) =
            elide_v1(br#"{"n":12.0,"o":{}}"#, &patterns(&["/n", "/o"]), &MediaLimitsV1::V1)
                .expect("elides");
        assert_eq!(view.as_str(), r#"{"n":12.0,"o":{}}"#);
        assert!(blobs.is_empty());
    }

    #[test]
    fn patterns_are_validated() {
        for bad in ["", "a", "/~2", "/~"] {
            assert!(PathPatternV1::parse(bad).is_err(), "{bad}");
        }
        assert!(PathPatternV1::parse(&format!("/{}", "a".repeat(256))).is_err());
        assert!(PathPatternV1::parse("/").is_ok());
        let too_many = vec![PathPatternV1::parse("/a").expect("valid"); 33];
        assert_eq!(
            elide_v1(b"{}", &too_many, &MediaLimitsV1::V1),
            Err(ElisionErrorV1::TooManyPaths)
        );
    }
}
