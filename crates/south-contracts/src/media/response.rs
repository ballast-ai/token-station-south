//! The views a host hands a media component: the request view (image record §6.1) and the
//! response view built from the declared `response_body_form` (§6.5; the `sse` form, speech
//! record §6).

use super::elide::{ElidedViewV1, ElisionErrorV1, PathPatternV1, elide_v1};
use super::json;
use super::multipart::{MultipartViewV1, view_json};
use super::{BlobIdV1, BlobV1, MediaLimitsV1};
use crate::RESPONSE_TRANSCRIPT_DENIED_HEADERS;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The response body forms a component may declare in `prepare` (§6.5).
///
/// `sse` is the speech world's (speech record §6): its view is built by
/// [`build_media_sse_response_view_v1`] from the events the host decoded with `decode_sse_v1`,
/// which ships in the host-only crate `south-host-grammars` (boundary record §13.13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseBodyFormV1 {
    /// A JSON document, elided under the declared response paths.
    Json,
    /// Bytes: the body becomes one opaque blob.
    Binary,
    /// UTF-8 text up to the fallback threshold.
    Text,
    /// Server-sent events: the decoded event list `[{"event", "data"}]`, elided as **one** JSON
    /// document whose root is the array, so a declared path addresses every event as
    /// `/*/data/…` (speech record §6).
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
    /// `{"sse": <elided event list>}`: a 2xx under the `sse` form (speech record §6). The list is
    /// `[{"event": "<type>", "data": <JSON value, or the data as a string when it is not JSON>}]`
    /// in stream order, elided as one document.
    Sse(ElidedViewV1),
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
    /// The `sse` form was asked of [`build_media_response_view_v1`], which has no decoded events;
    /// a host builds that form with [`build_media_sse_response_view_v1`].
    #[error("the sse response form is built from the decoded events")]
    SseUnavailable,
}

/// One server-sent event as the host decoded it.
///
/// These are the fields of `south-host-grammars`' `SseEventV1` (`event()`, `data()`), borrowed.
/// `south-contracts` does not link that crate (boundary record §13.13), so the host passes the two
/// strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SseEventFieldsV1<'a> {
    /// The event type (`message` when the stream named none).
    pub event: &'a str,
    /// The event's data, its `data:` lines joined with LF.
    pub data: &'a str,
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
///
/// The `sse` form needs the events the host decoded and is refused here
/// ([`MediaResponseErrorV1::SseUnavailable`]); [`build_media_sse_response_view_v1`] builds it.
pub fn build_media_response_view_v1(
    round: UpstreamRoundV1<'_>,
    declared_secret_headers: &[&str],
    form: ResponseBodyFormV1,
    declared: &[PathPatternV1],
    limits: &MediaLimitsV1,
) -> Result<(MediaResponseViewV1, Vec<BlobV1>), MediaResponseErrorV1> {
    build_view(round, declared_secret_headers, form, None, declared, limits)
}

/// Builds the response view of one round whose declared form is `sse` (speech record §6).
///
/// `events` are the 2xx body's events as `decode_sse_v1` returned them, in order; a host that
/// could not decode the body judges the round `unknown` without building a view. The 2xx body
/// becomes [`MediaResponseBodyV1::Sse`]: each event is `{"event", "data"}`, where `data` is the
/// event's data as a JSON value when it is one JSON document and as a string otherwise, and the
/// whole list is elided as one document whose root is the array, so a declared path such as
/// `/*/data/delta` matches that member of every event and blob ids run `b0`, `b1`, … across the
/// list in stream order. A `$south.` key in any event's data makes the round `unknown`, as in a
/// JSON body. A non-2xx body is not read as events: it follows the rule of
/// [`build_media_response_view_v1`], and `events` is ignored.
pub fn build_media_sse_response_view_v1(
    round: UpstreamRoundV1<'_>,
    events: &[SseEventFieldsV1<'_>],
    declared_secret_headers: &[&str],
    declared: &[PathPatternV1],
    limits: &MediaLimitsV1,
) -> Result<(MediaResponseViewV1, Vec<BlobV1>), MediaResponseErrorV1> {
    build_view(
        round,
        declared_secret_headers,
        ResponseBodyFormV1::Sse,
        Some(events),
        declared,
        limits,
    )
}

/// The decoded event list as one JSON document, `[{"event", "data"}]`, before elision.
fn sse_document(events: &[SseEventFieldsV1<'_>]) -> Result<String, MediaResponseErrorV1> {
    let mut out = String::from("[");
    for (index, SseEventFieldsV1 { event, data }) in events.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"event\":");
        json::write_string(event, &mut out);
        out.push_str(",\"data\":");
        match json::parse(data.as_bytes()) {
            // Carried as written: elision re-reads it, and keeps its member order and number text.
            Ok(_) => out.push_str(data),
            Err(json::ParseError::Syntax) => json::write_string(data, &mut out),
            Err(json::ParseError::TooDeep) => {
                return Err(MediaResponseErrorV1::Elision(ElisionErrorV1::TooDeep));
            }
        }
        out.push('}');
    }
    out.push(']');
    Ok(out)
}

fn build_view(
    round: UpstreamRoundV1<'_>,
    declared_secret_headers: &[&str],
    form: ResponseBodyFormV1,
    events: Option<&[SseEventFieldsV1<'_>]>,
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
            ResponseBodyFormV1::Sse => {
                let events = events.ok_or(MediaResponseErrorV1::SseUnavailable)?;
                let document = sse_document(events)?;
                let (view, elided) = elide_v1(document.as_bytes(), declared, limits)
                    .map_err(MediaResponseErrorV1::Elision)?;
                blobs = elided;
                MediaResponseBodyV1::Sse(view)
            }
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
            MediaResponseBodyV1::Sse(view) => {
                out.push_str("{\"sse\":");
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

    fn sse(
        status: u16,
        events: &[(&str, &str)],
        declared: &[&str],
    ) -> Result<(MediaResponseViewV1, Vec<BlobV1>), MediaResponseErrorV1> {
        let declared: Vec<_> =
            declared.iter().map(|path| PathPatternV1::parse(path).expect("valid")).collect();
        let events: Vec<SseEventFieldsV1<'_>> =
            events.iter().map(|(event, data)| SseEventFieldsV1 { event, data }).collect();
        build_media_sse_response_view_v1(
            UpstreamRoundV1 {
                status,
                headers: &[],
                body: b"{\"error\":{\"code\":\"x\"}}",
                content_type: Some("text/event-stream"),
            },
            &events,
            &[],
            &declared,
            &MediaLimitsV1::V1,
        )
    }

    #[test]
    fn an_sse_success_is_one_elided_array_document() {
        let (view, blobs) = sse(
            200,
            &[
                ("speech.audio.delta", r#"{"delta":"AAAA","n":1.0}"#),
                ("message", "[DONE]"),
                ("speech.audio.delta", r#"{"delta":"BBBB"}"#),
                ("message", ""),
            ],
            &["/*/data/delta"],
        )
        .expect("builds");
        assert_eq!(
            view.to_json(),
            r#"{"status":200,"headers":[],"body":{"sse":[{"event":"speech.audio.delta","data":{"delta":{"$south.blob":{"id":"b0","bytes":4,"head":"AAAA"}},"n":1.0}},{"event":"message","data":"[DONE]"},{"event":"speech.audio.delta","data":{"delta":{"$south.blob":{"id":"b1","bytes":4,"head":"BBBB"}}}},{"event":"message","data":""}]}}"#
        );
        assert_eq!(
            blobs.iter().map(BlobV1::bytes).collect::<Vec<_>>(),
            [b"AAAA".as_slice(), b"BBBB".as_slice()]
        );
        let (view, blobs) = sse(200, &[], &[]).expect("an empty stream builds");
        assert_eq!(
            view.body,
            MediaResponseBodyV1::Sse(elide_v1(b"[]", &[], &MediaLimitsV1::V1).expect("elides").0)
        );
        assert!(blobs.is_empty());
    }

    #[test]
    fn an_sse_round_refuses_forged_references_and_reads_errors_as_json() {
        assert_eq!(
            sse(200, &[("message", r#"{"$south.blob":{}}"#)], &[]).map(|_| ()),
            Err(MediaResponseErrorV1::Elision(ElisionErrorV1::ReservedKey))
        );
        assert_eq!(
            sse(200, &[("message", &format!("{}1{}", "[".repeat(200), "]".repeat(200)))], &[])
                .map(|_| ()),
            Err(MediaResponseErrorV1::Elision(ElisionErrorV1::TooDeep))
        );
        let (view, _) = sse(401, &[("message", "ignored")], &[]).expect("builds");
        assert!(matches!(view.body, MediaResponseBodyV1::Json(_)));
        assert_eq!(
            build(200, b"data: x\n\n", ResponseBodyFormV1::Sse, &[]).map(|_| ()),
            Err(MediaResponseErrorV1::SseUnavailable)
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
