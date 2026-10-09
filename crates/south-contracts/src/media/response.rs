//! The views a host hands a media component: the request view (image record §6.1) and the
//! response view built from the declared `response_body_form` (§6.5).

use super::elide::{ElidedViewV1, ElisionErrorV1, PathPatternV1, elide_v1};
use super::json;
use super::multipart::{MultipartViewV1, view_json};
use super::{BlobIdV1, BlobV1, MediaLimitsV1};
use crate::RESPONSE_TRANSCRIPT_DENIED_HEADERS;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The response body forms a component may declare in `prepare` (§6.5). `sse` is the speech
/// world's; its decoder ships separately (host plan B6, Q-B6-6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseBodyFormV1 {
    /// A JSON document, elided under the declared response paths.
    Json,
    /// Bytes: the body becomes one opaque blob.
    Binary,
    /// UTF-8 text up to the fallback threshold.
    Text,
    /// Server-sent events, one elided JSON document per event.
    Sse,
}

/// The request view (§6.1): `{"json": <elided view>}` or `{"multipart": {"parts": […]}}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaRequestViewV1 {
    /// A JSON request after [`elide_v1`].
    Json(ElidedViewV1),
    /// A multipart request after `parse_multipart_parts_v1`.
    Multipart(MultipartViewV1),
}

impl MediaRequestViewV1 {
    /// Serializes the view compactly, with the escaping every host reproduces.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        match self {
            Self::Json(view) => {
                out.push_str("{\"json\":");
                out.push_str(view.as_str());
                out.push('}');
            }
            Self::Multipart(view) => {
                out.push_str("{\"multipart\":");
                view_json(view, &mut out);
                out.push('}');
            }
        }
        out
    }
}

/// The body of a response view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaResponseBodyV1 {
    /// `{"json": <elided view>}`.
    Json(ElidedViewV1),
    /// `{"text": "…"}`, at most the fallback threshold.
    Text(String),
    /// `{"opaque": {"blob", "bytes", "media_type"}}`: binary, or text over the threshold.
    Opaque {
        /// The body's blob.
        blob: BlobIdV1,
        /// The body's length.
        bytes: u64,
        /// The upstream's `content-type` essence, when it sent a valid one.
        media_type: Option<String>,
    },
}

/// One upstream round as the component sees it: `{status, headers, body}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaResponseViewV1 {
    /// The HTTP status.
    pub status: u16,
    /// Response headers, lower-case names, transcript exclusions applied, in received order.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: MediaResponseBodyV1,
}

/// Why a response view could not be built. Every variant makes the round `unknown`.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum MediaResponseErrorV1 {
    /// The declared JSON form met a body that is not JSON, or that uses a `$south.` key
    /// (an upstream may not forge references), or another elision refusal.
    #[error("response body cannot be elided: {0}")]
    Elision(ElisionErrorV1),
    /// The declared text form met a body that is not UTF-8.
    #[error("response body is not UTF-8 text")]
    NotText,
    /// The `sse` form is built by the SSE decoder, which ships separately.
    #[error("the sse response form needs the SSE decoder")]
    SseUnavailable,
}

/// One upstream round as the transport returned it, before the view is built.
#[derive(Clone, Copy, Debug)]
pub struct UpstreamRoundV1<'a> {
    /// The HTTP status.
    pub status: u16,
    /// Response headers as received.
    pub headers: &'a [(String, String)],
    /// The buffered body.
    pub body: &'a [u8],
    /// The `content-type` header value, when present.
    pub content_type: Option<&'a str>,
}

/// Builds the response view of one round (§6.5).
///
/// A 2xx body follows the declared `form`. A non-2xx body is tried as UTF-8: JSON gives `json`
/// (elided with no declared paths), other text up to the threshold gives `text`, anything else
/// `opaque`. Headers in the transcript exclusion list and any `declared_secret_headers` are
/// dropped. The returned blobs back every reference in the view.
pub fn build_media_response_view_v1(
    round: UpstreamRoundV1<'_>,
    declared_secret_headers: &[&str],
    form: ResponseBodyFormV1,
    declared: &[PathPatternV1],
    limits: &MediaLimitsV1,
) -> Result<(MediaResponseViewV1, Vec<BlobV1>), MediaResponseErrorV1> {
    let UpstreamRoundV1 { status, headers, body, content_type } = round;
    let headers = headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
        .filter(|(name, _)| {
            !RESPONSE_TRANSCRIPT_DENIED_HEADERS.contains(&name.as_str())
                && !declared_secret_headers.iter().any(|secret| secret.eq_ignore_ascii_case(name))
        })
        .collect();
    let media_type = content_type
        .and_then(|value| value.split(';').next())
        .map(|essence| essence.trim().to_owned())
        .filter(|essence| super::is_media_type(essence));
    let opaque = |blobs: &mut Vec<BlobV1>| {
        let id = BlobIdV1::from_index(0);
        blobs.push(BlobV1::file(id.clone(), body.to_vec()));
        MediaResponseBodyV1::Opaque {
            blob: id,
            bytes: body.len() as u64,
            media_type: media_type.clone(),
        }
    };
    let mut blobs = Vec::new();
    let body = if (200..300).contains(&status) {
        match form {
            ResponseBodyFormV1::Json => {
                let (view, elided) =
                    elide_v1(body, declared, limits).map_err(MediaResponseErrorV1::Elision)?;
                blobs = elided;
                MediaResponseBodyV1::Json(view)
            }
            ResponseBodyFormV1::Binary => opaque(&mut blobs),
            ResponseBodyFormV1::Text => {
                let text = std::str::from_utf8(body).map_err(|_| MediaResponseErrorV1::NotText)?;
                if text.len() > limits.inline_string_bytes {
                    opaque(&mut blobs)
                } else {
                    MediaResponseBodyV1::Text(text.to_owned())
                }
            }
            ResponseBodyFormV1::Sse => return Err(MediaResponseErrorV1::SseUnavailable),
        }
    } else {
        match elide_v1(body, &[], limits) {
            Ok((view, elided)) => {
                blobs = elided;
                MediaResponseBodyV1::Json(view)
            }
            Err(ElisionErrorV1::ReservedKey) => {
                return Err(MediaResponseErrorV1::Elision(ElisionErrorV1::ReservedKey));
            }
            Err(_) => match std::str::from_utf8(body) {
                Ok(text) if text.len() <= limits.inline_string_bytes => {
                    MediaResponseBodyV1::Text(text.to_owned())
                }
                _ => opaque(&mut blobs),
            },
        }
    };
    Ok((MediaResponseViewV1 { status, headers, body }, blobs))
}

impl MediaResponseViewV1 {
    /// Serializes the view compactly: `{"status":…,"headers":[[name,value],…],"body":{…}}`.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\"status\":");
        out.push_str(&self.status.to_string());
        out.push_str(",\"headers\":[");
        for (index, (name, value)) in self.headers.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push('[');
            json::write_string(name, &mut out);
            out.push(',');
            json::write_string(value, &mut out);
            out.push(']');
        }
        out.push_str("],\"body\":");
        match &self.body {
            MediaResponseBodyV1::Json(view) => {
                out.push_str("{\"json\":");
                out.push_str(view.as_str());
                out.push('}');
            }
            MediaResponseBodyV1::Text(text) => {
                out.push_str("{\"text\":");
                json::write_string(text, &mut out);
                out.push('}');
            }
            MediaResponseBodyV1::Opaque { blob, bytes, media_type } => {
                out.push_str("{\"opaque\":{\"blob\":\"");
                out.push_str(blob.as_str());
                out.push_str("\",\"bytes\":");
                out.push_str(&bytes.to_string());
                if let Some(media_type) = media_type {
                    out.push_str(",\"media_type\":");
                    json::write_string(media_type, &mut out);
                }
                out.push_str("}}");
            }
        }
        out.push('}');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(
        status: u16,
        body: &[u8],
        form: ResponseBodyFormV1,
        declared: &[&str],
    ) -> Result<(MediaResponseViewV1, Vec<BlobV1>), MediaResponseErrorV1> {
        let declared: Vec<_> =
            declared.iter().map(|path| PathPatternV1::parse(path).expect("valid")).collect();
        build_media_response_view_v1(
            UpstreamRoundV1 {
                status,
                headers: &[
                    ("X-Request-Id".into(), "r1".into()),
                    ("set-cookie".into(), "s".into()),
                    ("x-secret".into(), "k".into()),
                ],
                body,
                content_type: Some("application/json; charset=utf-8"),
            },
            &["x-secret"],
            form,
            &declared,
            &MediaLimitsV1::V1,
        )
    }

    #[test]
    fn a_json_success_is_elided_under_the_declared_paths() {
        let (view, blobs) = build(
            200,
            br#"{"data":[{"b64_json":"AAAA"}],"usage":{"total_tokens":3}}"#,
            ResponseBodyFormV1::Json,
            &["/data/*/b64_json"],
        )
        .expect("builds");
        assert_eq!(
            view.to_json(),
            r#"{"status":200,"headers":[["x-request-id","r1"]],"body":{"json":{"data":[{"b64_json":{"$south.blob":{"id":"b0","bytes":4,"head":"AAAA"}}}],"usage":{"total_tokens":3}}}}"#
        );
        assert_eq!(blobs[0].bytes(), b"AAAA");
    }

    #[test]
    fn binary_text_and_error_bodies() {
        let (view, blobs) =
            build(200, &[0, 1, 2], ResponseBodyFormV1::Binary, &[]).expect("builds");
        assert_eq!(
            view.body,
            MediaResponseBodyV1::Opaque {
                blob: BlobIdV1::from_index(0),
                bytes: 3,
                media_type: Some("application/json".into())
            }
        );
        assert_eq!(blobs.len(), 1);
        let (view, _) =
            build(500, b"upstream exploded", ResponseBodyFormV1::Json, &[]).expect("builds");
        assert_eq!(view.body, MediaResponseBodyV1::Text("upstream exploded".into()));
        let (view, _) = build(400, br#"{"error":{"code":"x"}}"#, ResponseBodyFormV1::Binary, &[])
            .expect("builds");
        assert!(matches!(view.body, MediaResponseBodyV1::Json(_)));
        let (view, _) = build(502, &[0xff, 0xfe], ResponseBodyFormV1::Json, &[]).expect("builds");
        assert!(matches!(view.body, MediaResponseBodyV1::Opaque { .. }));
    }

    #[test]
    fn forged_references_and_bad_bodies_make_the_round_unknown() {
        assert_eq!(
            build(200, br#"{"$south.blob":{}}"#, ResponseBodyFormV1::Json, &[]).map(|_| ()),
            Err(MediaResponseErrorV1::Elision(ElisionErrorV1::ReservedKey))
        );
        assert_eq!(
            build(400, br#"{"x":{"$south.ref":{}}}"#, ResponseBodyFormV1::Json, &[]).map(|_| ()),
            Err(MediaResponseErrorV1::Elision(ElisionErrorV1::ReservedKey))
        );
        assert_eq!(
            build(200, b"not json", ResponseBodyFormV1::Json, &[]).map(|_| ()),
            Err(MediaResponseErrorV1::Elision(ElisionErrorV1::InvalidJson))
        );
        assert_eq!(
            build(200, &[0xff], ResponseBodyFormV1::Text, &[]).map(|_| ()),
            Err(MediaResponseErrorV1::NotText)
        );
    }

    #[test]
    fn request_views_serialize() {
        let (elided, _) = elide_v1(br#"{"prompt":"x"}"#, &[], &MediaLimitsV1::V1).expect("elides");
        assert_eq!(MediaRequestViewV1::Json(elided).to_json(), r#"{"json":{"prompt":"x"}}"#);
        assert_eq!(
            MediaRequestViewV1::Multipart(MultipartViewV1::default()).to_json(),
            r#"{"multipart":{"parts":[]}}"#
        );
    }
}
