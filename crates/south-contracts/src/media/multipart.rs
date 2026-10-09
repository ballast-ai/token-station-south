//! The multipart part parser and encoder (image record §6.1, §6.3, §6.7).
//!
//! 0.25.0 kept multipart opaque at the transport layer, and [`MultipartBodyV1`] still is. The
//! world layer needs part names, file names and media types, so south supplies the one parser and
//! the one encoder both hosts run (§6.6 narrows 0.25.0's "no encoder" sentence to the transport
//! layer). The parser accepts what browsers and HTTP clients send (RFC 7578 over RFC 2046): no
//! preamble, CRLF line breaks, a `form-data` disposition with `name` and optional `filename`;
//! everything else is refused, never repaired.

use super::elide::utf8_prefix;
use super::json;
use super::transform::{MediaTransformV1, TransformErrorV1, is_media_type};
use super::{BlobIdV1, BlobSetV1, BlobV1, MediaLimitsV1};
use crate::{ContractErrorV1, MultipartBodyV1, MultipartBoundaryV1};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Why a multipart body could not be split, or a part list could not be encoded.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum MultipartErrorV1 {
    /// The body is not delimited by its boundary as RFC 2046 requires (no opening delimiter, a
    /// missing closing one, a delimiter not followed by CRLF or `--`, or bytes after the close).
    #[error("multipart body is not delimited by its boundary")]
    Framing,
    /// A part's header block is malformed, too long, or not UTF-8.
    #[error("multipart part has an invalid header block")]
    InvalidHeader,
    /// A part has no `form-data` disposition, or no `name`.
    #[error("multipart part has no form-data name")]
    MissingName,
    /// A part without a file name is not UTF-8 text.
    #[error("multipart text part is not UTF-8")]
    InvalidTextPart,
    /// A part declares a content transfer encoding other than identity.
    #[error("multipart part uses a content transfer encoding")]
    TransferEncoding,
    /// More parts than [`MediaLimitsV1::parts`].
    #[error("too many multipart parts")]
    TooManyParts,
    /// Encoding: a part names a blob the set does not hold.
    #[error("multipart part references an unknown blob")]
    UnknownBlob,
    /// Encoding: a part's transform refused its blob.
    #[error("multipart part transform failed: {0}")]
    Transform(TransformErrorV1),
    /// Encoding: a part's content contains the boundary.
    #[error("multipart content collides with the boundary")]
    BoundaryCollision,
    /// Encoding: a name, file name or media type is invalid.
    #[error("multipart part metadata is invalid")]
    InvalidMetadata,
    /// Encoding: the encoded body exceeds the multipart request bound.
    #[error("encoded multipart body is too large")]
    TooLarge,
}

/// One part of a split multipart request, as the component sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MediaPartViewV1 {
    /// A text field. When its value exceeds the fallback threshold it is elided: `value` is absent
    /// and `blob`, `bytes` and `head` describe it.
    Text {
        /// The field name.
        name: String,
        /// The value, when it was not elided.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<String>,
        /// The elided value's blob.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blob: Option<BlobIdV1>,
        /// The elided value's length in bytes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bytes: Option<u64>,
        /// The elided value's first bytes, cut back to a UTF-8 boundary.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        head: Option<String>,
    },
    /// A file part, by reference.
    File {
        /// The field name.
        name: String,
        /// The part's blob.
        blob: BlobIdV1,
        /// The part's file name, when it gave one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        /// The part's media type without parameters, when it gave one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
        /// The file's length in bytes.
        bytes: u64,
    },
}

/// The ordered part list of a multipart request view (`{"parts": [...]}`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultipartViewV1 {
    /// Parts in order of appearance; repeated names keep their order.
    pub parts: Vec<MediaPartViewV1>,
}

/// One part of a descriptor's multipart body: a text value, or a blob through a transform.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MediaPartV1 {
    /// A file part built from a blob.
    File {
        /// The field name.
        name: String,
        /// The blob to send.
        blob: BlobIdV1,
        /// How to turn the blob into the part's bytes.
        transform: MediaTransformV1,
        /// The part's file name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        /// The part's media type; `application/octet-stream` when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
    },
    /// A text part.
    Text {
        /// The field name.
        name: String,
        /// The value.
        value: String,
    },
}

/// Splits a `multipart/form-data` body into its ordered part list and the blobs behind it.
///
/// File parts (a `filename` or `filename*` parameter) always become blobs; text parts become
/// blobs only above [`MediaLimitsV1::inline_string_bytes`]. Ids run `b0`, `b1`, … over both, in
/// part order.
pub fn parse_multipart_parts_v1(
    body: &[u8],
    boundary: &MultipartBoundaryV1,
    limits: &MediaLimitsV1,
) -> Result<(MultipartViewV1, Vec<BlobV1>), MultipartErrorV1> {
    let dash = [b"--".as_slice(), boundary.as_str().as_bytes()].concat();
    if !body.starts_with(&dash) {
        return Err(MultipartErrorV1::Framing);
    }
    let mut position = dash.len();
    let mut view = MultipartViewV1::default();
    let mut blobs = Vec::new();
    loop {
        let rest = &body[position..];
        if let Some(epilogue) = rest.strip_prefix(b"--") {
            if matches!(epilogue, b"" | b"\r\n" | b"\n") {
                return Ok((view, blobs));
            }
            return Err(MultipartErrorV1::Framing);
        }
        if !rest.starts_with(b"\r\n") {
            return Err(MultipartErrorV1::Framing);
        }
        position += 2;
        if view.parts.len() == limits.parts {
            return Err(MultipartErrorV1::TooManyParts);
        }
        let header_end = find(&body[position..], b"\r\n\r\n")
            .filter(|end| *end <= limits.part_header_bytes)
            .ok_or(MultipartErrorV1::InvalidHeader)?;
        let headers = parse_headers(&body[position..position + header_end])?;
        position += header_end + 4;
        let delimiter = [b"\r\n".as_slice(), &dash].concat();
        let content_len = find(&body[position..], &delimiter).ok_or(MultipartErrorV1::Framing)?;
        let content = &body[position..position + content_len];
        position += content_len + delimiter.len();
        let next_id = u32::try_from(blobs.len()).map_err(|_| MultipartErrorV1::TooManyParts)?;
        if headers.is_file {
            let id = BlobIdV1::from_index(next_id);
            view.parts.push(MediaPartViewV1::File {
                name: headers.name,
                blob: id.clone(),
                filename: headers.filename,
                media_type: headers.media_type,
                bytes: content.len() as u64,
            });
            blobs.push(BlobV1::file(id, content.to_vec()));
        } else {
            let value =
                std::str::from_utf8(content).map_err(|_| MultipartErrorV1::InvalidTextPart)?;
            if value.len() > limits.inline_string_bytes {
                let id = BlobIdV1::from_index(next_id);
                view.parts.push(MediaPartViewV1::Text {
                    name: headers.name,
                    value: None,
                    blob: Some(id.clone()),
                    bytes: Some(value.len() as u64),
                    head: Some(utf8_prefix(value, limits.blob_head_bytes).to_owned()),
                });
                blobs.push(BlobV1::text(id, value.to_owned()));
            } else {
                view.parts.push(MediaPartViewV1::Text {
                    name: headers.name,
                    value: Some(value.to_owned()),
                    blob: None,
                    bytes: None,
                    head: None,
                });
            }
        }
    }
}

struct PartHeaders {
    name: String,
    filename: Option<String>,
    media_type: Option<String>,
    is_file: bool,
}

fn parse_headers(block: &[u8]) -> Result<PartHeaders, MultipartErrorV1> {
    let block = std::str::from_utf8(block).map_err(|_| MultipartErrorV1::InvalidHeader)?;
    let mut disposition = None;
    let mut media_type = None;
    for line in block.split("\r\n") {
        let (name, value) = line.split_once(':').ok_or(MultipartErrorV1::InvalidHeader)?;
        if name.is_empty() || name.bytes().any(|byte| !byte.is_ascii_graphic()) {
            return Err(MultipartErrorV1::InvalidHeader);
        }
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case("content-disposition") {
            if disposition.replace(parse_disposition(value)?).is_some() {
                return Err(MultipartErrorV1::InvalidHeader);
            }
        } else if name.eq_ignore_ascii_case("content-type") {
            let essence = value.split(';').next().unwrap_or_default().trim_matches([' ', '\t']);
            if !is_media_type(essence) || media_type.replace(essence.to_owned()).is_some() {
                return Err(MultipartErrorV1::InvalidHeader);
            }
        } else if name.eq_ignore_ascii_case("content-transfer-encoding")
            && !["binary", "8bit", "7bit"].iter().any(|word| value.eq_ignore_ascii_case(word))
        {
            return Err(MultipartErrorV1::TransferEncoding);
        }
    }
    let disposition = disposition.ok_or(MultipartErrorV1::MissingName)?;
    Ok(PartHeaders {
        name: disposition.name,
        is_file: disposition.filename.is_some() || disposition.extended_filename,
        filename: disposition.filename,
        media_type,
    })
}

struct Disposition {
    name: String,
    filename: Option<String>,
    extended_filename: bool,
}

fn parse_disposition(value: &str) -> Result<Disposition, MultipartErrorV1> {
    let mut rest = value;
    let kind_end = rest.find([';', ' ']).unwrap_or(rest.len());
    if !rest[..kind_end].eq_ignore_ascii_case("form-data") {
        return Err(MultipartErrorV1::MissingName);
    }
    rest = &rest[kind_end..];
    let mut name = None;
    let mut filename = None;
    let mut extended_filename = false;
    loop {
        rest = rest.trim_start_matches([' ', '\t']);
        if rest.is_empty() {
            break;
        }
        rest = rest.strip_prefix(';').ok_or(MultipartErrorV1::InvalidHeader)?;
        rest = rest.trim_start_matches([' ', '\t']);
        let equals = rest.find('=').ok_or(MultipartErrorV1::InvalidHeader)?;
        let parameter = rest[..equals].trim_end_matches([' ', '\t']).to_ascii_lowercase();
        rest = rest[equals + 1..].trim_start_matches([' ', '\t']);
        let (parsed, remainder) = parameter_value(rest)?;
        rest = remainder;
        let slot = match parameter.as_str() {
            "name" => &mut name,
            "filename" => &mut filename,
            "filename*" => {
                if extended_filename {
                    return Err(MultipartErrorV1::InvalidHeader);
                }
                extended_filename = true;
                continue;
            }
            _ => continue,
        };
        if slot.replace(parsed).is_some() {
            return Err(MultipartErrorV1::InvalidHeader);
        }
    }
    let name = name.filter(|name| !name.is_empty()).ok_or(MultipartErrorV1::MissingName)?;
    Ok(Disposition { name, filename, extended_filename })
}

/// A quoted string (`\` escapes the next character) or a token, and what follows it.
fn parameter_value(input: &str) -> Result<(String, &str), MultipartErrorV1> {
    if let Some(quoted) = input.strip_prefix('"') {
        let mut value = String::new();
        let mut characters = quoted.char_indices();
        while let Some((index, character)) = characters.next() {
            match character {
                '"' => return Ok((value, &quoted[index + 1..])),
                '\\' => {
                    let (_, escaped) = characters.next().ok_or(MultipartErrorV1::InvalidHeader)?;
                    value.push(escaped);
                }
                other => value.push(other),
            }
        }
        Err(MultipartErrorV1::InvalidHeader)
    } else {
        let end = input.find([';', ' ', '\t']).unwrap_or(input.len());
        let token = &input[..end];
        if token.is_empty() {
            return Err(MultipartErrorV1::InvalidHeader);
        }
        Ok((token.to_owned(), &input[end..]))
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let first = *needle.first()?;
    let mut from = 0;
    while let Some(offset) = haystack.get(from..)?.iter().position(|byte| *byte == first) {
        let at = from + offset;
        if haystack.get(at..at + needle.len()) == Some(needle) {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

/// Encodes a descriptor's part list into a multipart body under `boundary`.
///
/// Each part is `--<boundary>\r\n`, a `Content-Disposition: form-data; name="…"` line (with
/// `; filename="…"` for a file part), a `Content-Type` line for a file part (its media type, or
/// `application/octet-stream`), a blank line, the content and `\r\n`; the body ends with
/// `--<boundary>--\r\n`. In names and file names `"`, CR and LF are written `%22`, `%0D` and
/// `%0A` (the HTML form-data encoding). A content that contains `--<boundary>` is refused; the
/// caller retries with another boundary.
pub fn encode_multipart_v1(
    parts: &[MediaPartV1],
    blobs: &BlobSetV1,
    boundary: &MultipartBoundaryV1,
    limits: &MediaLimitsV1,
) -> Result<MultipartBodyV1, MultipartErrorV1> {
    if parts.len() > limits.parts {
        return Err(MultipartErrorV1::TooManyParts);
    }
    let dash = format!("--{}", boundary.as_str());
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(dash.as_bytes());
        body.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"");
        match part {
            MediaPartV1::Text { name, value } => {
                push_quoted(&mut body, name)?;
                body.extend_from_slice(b"\"\r\n\r\n");
                push_content(&mut body, value.as_bytes(), &dash)?;
            }
            MediaPartV1::File { name, blob, transform, filename, media_type } => {
                push_quoted(&mut body, name)?;
                body.push(b'"');
                if let Some(filename) = filename {
                    body.extend_from_slice(b"; filename=\"");
                    push_quoted(&mut body, filename)?;
                    body.push(b'"');
                }
                let media_type = media_type.as_deref().unwrap_or("application/octet-stream");
                if !is_media_type(media_type) {
                    return Err(MultipartErrorV1::InvalidMetadata);
                }
                body.extend_from_slice(b"\r\nContent-Type: ");
                body.extend_from_slice(media_type.as_bytes());
                body.extend_from_slice(b"\r\n\r\n");
                let source = blobs.get(blob).ok_or(MultipartErrorV1::UnknownBlob)?;
                let content = transform
                    .apply(source.bytes(), Some(media_type))
                    .map_err(MultipartErrorV1::Transform)?;
                push_content(&mut body, &content, &dash)?;
            }
        }
        body.extend_from_slice(b"\r\n");
        if body.len() > crate::MAX_MULTIPART_REQUEST_BODY_BYTES {
            return Err(MultipartErrorV1::TooLarge);
        }
    }
    body.extend_from_slice(dash.as_bytes());
    body.extend_from_slice(b"--\r\n");
    MultipartBodyV1::parse(body, boundary.clone()).map_err(|error| match error {
        ContractErrorV1::RequestBodyTooLarge => MultipartErrorV1::TooLarge,
        _ => MultipartErrorV1::Framing,
    })
}

fn push_quoted(body: &mut Vec<u8>, value: &str) -> Result<(), MultipartErrorV1> {
    if value.is_empty() {
        return Err(MultipartErrorV1::InvalidMetadata);
    }
    for byte in value.bytes() {
        match byte {
            b'"' => body.extend_from_slice(b"%22"),
            b'\r' => body.extend_from_slice(b"%0D"),
            b'\n' => body.extend_from_slice(b"%0A"),
            other => body.push(other),
        }
    }
    Ok(())
}

fn push_content(body: &mut Vec<u8>, content: &[u8], dash: &str) -> Result<(), MultipartErrorV1> {
    if find(content, dash.as_bytes()).is_some() {
        return Err(MultipartErrorV1::BoundaryCollision);
    }
    body.extend_from_slice(content);
    Ok(())
}

/// Serializes a part list view as `{"parts":[...]}` with the escaping every host reproduces.
pub(super) fn view_json(view: &MultipartViewV1, out: &mut String) {
    out.push_str("{\"parts\":[");
    for (index, part) in view.parts.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        match part {
            MediaPartViewV1::Text { name, value, blob, bytes, head } => {
                out.push_str("{\"kind\":\"text\",\"name\":");
                json::write_string(name, out);
                if let Some(value) = value {
                    out.push_str(",\"value\":");
                    json::write_string(value, out);
                }
                if let (Some(blob), Some(bytes), Some(head)) = (blob, bytes, head) {
                    out.push_str(",\"blob\":\"");
                    out.push_str(blob.as_str());
                    out.push_str("\",\"bytes\":");
                    out.push_str(&bytes.to_string());
                    out.push_str(",\"head\":");
                    json::write_string(head, out);
                }
                out.push('}');
            }
            MediaPartViewV1::File { name, blob, filename, media_type, bytes } => {
                out.push_str("{\"kind\":\"file\",\"name\":");
                json::write_string(name, out);
                out.push_str(",\"blob\":\"");
                out.push_str(blob.as_str());
                out.push('"');
                if let Some(filename) = filename {
                    out.push_str(",\"filename\":");
                    json::write_string(filename, out);
                }
                if let Some(media_type) = media_type {
                    out.push_str(",\"media_type\":");
                    json::write_string(media_type, out);
                }
                out.push_str(",\"bytes\":");
                out.push_str(&bytes.to_string());
                out.push('}');
            }
        }
    }
    out.push_str("]}");
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &[u8] = b"--XyZ\r\n\
Content-Disposition: form-data; name=\"prompt\"\r\n\r\n\
a \"cat\"\r\n\
--XyZ\r\n\
Content-Disposition: form-data; name=\"image[]\"; filename=\"a.png\"\r\n\
Content-Type: image/png; x=1\r\n\r\n\
\x89PNG\r\n\x1a\n\x00\r\n\
--XyZ\r\n\
content-disposition: FORM-DATA; filename=b.png; name=image[]\r\n\r\n\
BBB\r\n\
--XyZ--\r\n";

    fn boundary() -> MultipartBoundaryV1 {
        MultipartBoundaryV1::parse("XyZ").expect("valid")
    }

    #[test]
    fn splits_parts_in_order_with_one_id_sequence() {
        let (view, blobs) =
            parse_multipart_parts_v1(BODY, &boundary(), &MediaLimitsV1::V1).expect("splits");
        let mut json = String::new();
        view_json(&view, &mut json);
        assert_eq!(
            json,
            r#"{"parts":[{"kind":"text","name":"prompt","value":"a \"cat\""},{"kind":"file","name":"image[]","blob":"b0","filename":"a.png","media_type":"image/png","bytes":9},{"kind":"file","name":"image[]","blob":"b1","filename":"b.png","bytes":3}]}"#
        );
        assert_eq!(blobs[0].bytes(), b"\x89PNG\r\n\x1a\n\x00");
        assert_eq!(blobs[1].bytes(), b"BBB");
        assert!(!blobs[0].is_text());
    }

    #[test]
    fn long_text_parts_are_elided() {
        let limits = MediaLimitsV1 { inline_string_bytes: 4, ..MediaLimitsV1::V1 };
        let body = b"--XyZ\r\nContent-Disposition: form-data; name=\"p\"\r\n\r\n12345\r\n--XyZ--";
        let (view, blobs) = parse_multipart_parts_v1(body, &boundary(), &limits).expect("splits");
        assert_eq!(
            view.parts,
            vec![MediaPartViewV1::Text {
                name: "p".into(),
                value: None,
                blob: Some(BlobIdV1::from_index(0)),
                bytes: Some(5),
                head: Some("12345".into()),
            }]
        );
        assert!(blobs[0].is_text());
    }

    #[test]
    fn refuses_malformed_bodies() {
        let cases: [(&[u8], MultipartErrorV1); 9] = [
            (b"preamble\r\n--XyZ--", MultipartErrorV1::Framing),
            (b"--XyZ\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nx", MultipartErrorV1::Framing),
            (b"--XyZ--junk", MultipartErrorV1::Framing),
            (b"--XyZ\r\nContent-Type: text/plain\r\n\r\nx\r\n--XyZ--", MultipartErrorV1::MissingName),
            (b"--XyZ\r\nContent-Disposition: attachment; name=a\r\n\r\nx\r\n--XyZ--", MultipartErrorV1::MissingName),
            (b"--XyZ\r\nContent-Disposition: form-data; name=a; name=b\r\n\r\nx\r\n--XyZ--", MultipartErrorV1::InvalidHeader),
            (b"--XyZ\r\nContent-Disposition: form-data; name=a\r\n\r\n\xff\r\n--XyZ--", MultipartErrorV1::InvalidTextPart),
            (b"--XyZ\r\nContent-Disposition: form-data; name=a\r\nContent-Transfer-Encoding: base64\r\n\r\nx\r\n--XyZ--", MultipartErrorV1::TransferEncoding),
            (b"--XyZ\r\nContent-Disposition: form-data; name=a\r\n\r\nx\r\n--XyZz\r\n", MultipartErrorV1::Framing),
        ];
        for (body, expected) in cases {
            assert_eq!(
                parse_multipart_parts_v1(body, &boundary(), &MediaLimitsV1::V1).map(|_| ()),
                Err(expected),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
        let limits = MediaLimitsV1 { parts: 1, ..MediaLimitsV1::V1 };
        assert_eq!(
            parse_multipart_parts_v1(BODY, &boundary(), &limits).map(|_| ()),
            Err(MultipartErrorV1::TooManyParts)
        );
    }

    #[test]
    fn encode_then_parse_round_trips() {
        let blobs = BlobSetV1::new([
            BlobV1::file(BlobIdV1::from_index(0), b"\x89PNG".to_vec()),
            BlobV1::text(BlobIdV1::from_index(1), "data:image/png;base64,aGk=".into()),
        ])
        .expect("distinct ids");
        let parts = vec![
            MediaPartV1::Text { name: "prompt".into(), value: "a \"cat\"".into() },
            MediaPartV1::File {
                name: "image".into(),
                blob: BlobIdV1::from_index(0),
                transform: MediaTransformV1::AsIs,
                filename: Some("in\".png".into()),
                media_type: Some("image/png".into()),
            },
            MediaPartV1::File {
                name: "mask".into(),
                blob: BlobIdV1::from_index(1),
                transform: MediaTransformV1::FromDataUrl,
                filename: Some("mask.png".into()),
                media_type: None,
            },
        ];
        let body =
            encode_multipart_v1(&parts, &blobs, &boundary(), &MediaLimitsV1::V1).expect("encodes");
        assert_eq!(
            body.as_bytes(),
            b"--XyZ\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\na \"cat\"\r\n\
--XyZ\r\nContent-Disposition: form-data; name=\"image\"; filename=\"in%22.png\"\r\nContent-Type: image/png\r\n\r\n\x89PNG\r\n\
--XyZ\r\nContent-Disposition: form-data; name=\"mask\"; filename=\"mask.png\"\r\nContent-Type: application/octet-stream\r\n\r\nhi\r\n\
--XyZ--\r\n"
                .as_slice()
        );
        let (view, split) =
            parse_multipart_parts_v1(body.as_bytes(), &boundary(), &MediaLimitsV1::V1)
                .expect("splits");
        assert_eq!(view.parts.len(), 3);
        assert_eq!(split[1].bytes(), b"hi");
    }

    #[test]
    fn encoding_refuses_collisions_and_unknown_blobs() {
        let blobs = BlobSetV1::new([BlobV1::file(BlobIdV1::from_index(0), b"x--XyZ".to_vec())])
            .expect("ids");
        let file = |blob| MediaPartV1::File {
            name: "f".into(),
            blob,
            transform: MediaTransformV1::AsIs,
            filename: None,
            media_type: None,
        };
        assert_eq!(
            encode_multipart_v1(
                &[file(BlobIdV1::from_index(0))],
                &blobs,
                &boundary(),
                &MediaLimitsV1::V1
            )
            .map(|_| ()),
            Err(MultipartErrorV1::BoundaryCollision)
        );
        assert_eq!(
            encode_multipart_v1(
                &[file(BlobIdV1::from_index(9))],
                &blobs,
                &boundary(),
                &MediaLimitsV1::V1
            )
            .map(|_| ()),
            Err(MultipartErrorV1::UnknownBlob)
        );
    }

    #[test]
    fn descriptor_parts_parse_from_json() {
        let parts: Vec<MediaPartV1> = serde_json::from_str(
            r#"[{"name":"prompt","value":"x"},{"name":"image","blob":"b0","transform":"as_is","filename":"a.png"}]"#,
        )
        .expect("parses");
        assert!(matches!(parts[0], MediaPartV1::Text { .. }));
        assert!(matches!(parts[1], MediaPartV1::File { .. }));
    }
}
