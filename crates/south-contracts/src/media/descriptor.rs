//! `MediaRequestDescriptorV1` (image record §6.3): the upstream request a media component
//! describes by reference, and the expansion of its JSON template.

use super::json::{self, Member, Node};
use super::multipart::MediaPartV1;
use super::transform::{MediaTransformV1, TransformErrorV1};
use super::{BlobIdV1, BlobSetV1, MediaLimitsV1, RESERVED_KEY_PREFIX};
use crate::{
    ContractErrorV1, CredentialSlotV1, HeaderPolicyError, JsonBodyV1, QueryParameterV1,
    QueryStringV1, RelativePathV1, SafeHeaders,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// The media types a `text` request body may carry (speech record D3a: SSML only).
pub const MEDIA_TEXT_BODY_MEDIA_TYPES: [&str; 1] = ["application/ssml+xml"];
/// The longest `text` request body, in bytes.
pub const MAX_MEDIA_TEXT_BODY_BYTES: usize = 1024 * 1024;
/// The key of a reference node in a JSON template.
pub const REFERENCE_KEY: &str = "$south.ref";
/// The key of a blob placeholder in a view; never valid in a component's output.
pub const BLOB_KEY: &str = "$south.blob";

/// Why a descriptor or a template is refused. Each is a pre-dispatch refusal: nothing was sent.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MediaDescriptorErrorV1 {
    /// The descriptor is not the JSON shape of §6.3.
    #[error("media descriptor is malformed: {0}")]
    Malformed(String),
    /// A method other than `POST`.
    #[error("media descriptor method must be POST")]
    Method,
    /// A path, query, slot or body value violates its contract grammar.
    #[error("media descriptor violates its contract: {0}")]
    Contract(ContractErrorV1),
    /// The headers violate the safe-header policy, or a multipart or text body carries
    /// `content-type`.
    #[error("media descriptor headers are refused: {0:?}")]
    Headers(Option<HeaderPolicyError>),
    /// A query parameter name outside the sanctioned set.
    #[error("media descriptor query parameter is not sanctioned")]
    UnsanctionedQuery,
    /// A header-secret arm names a header outside the header-name grammar.
    #[error("media descriptor auth header is invalid")]
    AuthHeader,
    /// A `text` body with a media type outside [`MEDIA_TEXT_BODY_MEDIA_TYPES`], or too long.
    #[error("media descriptor text body is refused")]
    TextBody,
    /// A multipart body with no parts or more than [`MediaLimitsV1::parts`].
    #[error("media descriptor multipart body has an invalid part count")]
    PartCount,
    /// A template uses `$south.blob`, another reserved key, or a malformed `$south.ref`.
    #[error("media template reference is invalid")]
    Reference,
    /// A reference names a blob the view does not hold.
    #[error("media template references an unknown blob")]
    UnknownBlob,
    /// A reference's transform refused its blob, or produced bytes that are not text.
    #[error("media template transform failed: {0}")]
    Transform(TransformErrorV1),
}

/// How the descriptor authenticates (§6.3).
///
/// It maps onto `ProviderAuthV1::Bearer` and `HeaderSecret` / `DeclaredHeaderSecret`. Whether
/// the manifest declares the arm and admits the header is decided by
/// `admit_media_descriptor_auth` in the conformance crate.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "arm", rename_all = "snake_case", deny_unknown_fields)]
pub enum MediaAuthV1 {
    /// `Authorization: Bearer <secret of slot>`.
    Bearer {
        /// The credential slot.
        slot: String,
    },
    /// `<header>: <secret of slot>`.
    HeaderSecret {
        /// The lower-case header name.
        header: String,
        /// The credential slot.
        slot: String,
    },
}

impl MediaAuthV1 {
    /// Returns the credential slot.
    #[must_use]
    pub fn slot(&self) -> &str {
        match self {
            Self::Bearer { slot } | Self::HeaderSecret { slot, .. } => slot,
        }
    }

    fn validate(&self) -> Result<(), MediaDescriptorErrorV1> {
        CredentialSlotV1::parse(self.slot()).map_err(MediaDescriptorErrorV1::Contract)?;
        if let Self::HeaderSecret { header, .. } = self {
            let valid = !header.is_empty()
                && header.len() <= crate::MAX_PROVIDER_HEADER_NAME_BYTES
                && header
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
            if !valid {
                return Err(MediaDescriptorErrorV1::AuthHeader);
            }
        }
        Ok(())
    }
}

impl fmt::Debug for MediaAuthV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer { slot } => formatter.debug_struct("Bearer").field("slot", slot).finish(),
            Self::HeaderSecret { header, slot } => formatter
                .debug_struct("HeaderSecret")
                .field("header", header)
                .field("slot", slot)
                .finish(),
        }
    }
}

/// A `text` request body (speech record D3a; no image component uses it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaTextBodyV1 {
    /// One of [`MEDIA_TEXT_BODY_MEDIA_TYPES`].
    pub media_type: String,
    /// The text.
    pub text: String,
}

/// The request body (§6.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaBodyV1 {
    /// `{"json": {"template": …}}`: JSON in which `$south.ref` nodes stand for blobs. Kept as
    /// compact JSON text, members in source order.
    Json(String),
    /// `{"multipart": {"parts": […]}}`.
    Multipart(Vec<MediaPartV1>),
    /// `{"text": {"media_type", "text"}}`.
    Text(MediaTextBodyV1),
    /// `"empty"`.
    Empty,
}

/// One `$south.ref` node: `{"blob": "<id>", "transform": <word>, "media_type"?: "<type>"}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaReferenceV1 {
    /// The blob.
    pub blob: BlobIdV1,
    /// The transform; never `concat`.
    pub transform: MediaTransformV1,
    /// The media type `data_url` writes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

/// A validated media request descriptor.
#[derive(Clone, PartialEq, Eq)]
pub struct MediaRequestDescriptorV1 {
    path: RelativePathV1,
    query: Option<QueryStringV1>,
    headers: SafeHeaders,
    auth: Option<MediaAuthV1>,
    body: MediaBodyV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DescriptorWire {
    method: String,
    path: String,
    #[serde(default)]
    query: Vec<NameValueWire>,
    #[serde(default)]
    headers: Vec<NameValueWire>,
    #[serde(default)]
    auth: Option<MediaAuthV1>,
    body: BodyWire,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameValueWire {
    name: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum BodyWire {
    Json {
        #[allow(dead_code)]
        template: serde::de::IgnoredAny,
    },
    Multipart {
        parts: Vec<MediaPartV1>,
    },
    Text(MediaTextBodyV1),
    Empty,
}

impl MediaRequestDescriptorV1 {
    /// Parses and validates a descriptor a component returned.
    ///
    /// Checks the shape, `POST`, the relative path grammar, sanctioned query parameters and their
    /// grammars, the safe-header policy (no `content-type` with a multipart or text body), the
    /// auth arm's slot and header grammars, the part count, and every `$south.ref` in a template
    /// (well formed, never `concat`, no other `$south.` key). Whether the manifest admits the
    /// auth arm is the conformance crate's `admit_media_descriptor_auth`.
    pub fn parse(json_text: &str, limits: &MediaLimitsV1) -> Result<Self, MediaDescriptorErrorV1> {
        let wire: DescriptorWire = serde_json::from_str(json_text)
            .map_err(|error| MediaDescriptorErrorV1::Malformed(error.to_string()))?;
        if wire.method != "POST" {
            return Err(MediaDescriptorErrorV1::Method);
        }
        let path = RelativePathV1::parse(&wire.path).map_err(MediaDescriptorErrorV1::Contract)?;
        let query = if wire.query.is_empty() {
            None
        } else {
            let mut parameters = Vec::with_capacity(wire.query.len());
            for entry in &wire.query {
                let parameter = QueryParameterV1::ALL
                    .iter()
                    .find(|candidate| candidate.wire_name() == entry.name)
                    .cloned()
                    .ok_or(MediaDescriptorErrorV1::UnsanctionedQuery)?;
                parameters.push((parameter, entry.value.as_str()));
            }
            Some(
                QueryStringV1::try_from_iter(parameters)
                    .map_err(MediaDescriptorErrorV1::Contract)?,
            )
        };
        let headers = SafeHeaders::try_from_iter(
            wire.headers.iter().map(|entry| (&entry.name, &entry.value)),
        )
        .map_err(|error| MediaDescriptorErrorV1::Headers(Some(error)))?;
        if let Some(auth) = &wire.auth {
            auth.validate()?;
        }
        let body = match wire.body {
            BodyWire::Json { .. } => {
                // The template's own text, members in source order and numbers as written: two
                // hosts must expand it to the same bytes, which a `serde_json::Value` round trip
                // (re-sorted keys, re-rendered numbers) would not guarantee.
                let text = template_text(json_text)?;
                Self::check_template(&text)?;
                MediaBodyV1::Json(text)
            }
            BodyWire::Multipart { parts } => {
                if parts.is_empty() || parts.len() > limits.parts {
                    return Err(MediaDescriptorErrorV1::PartCount);
                }
                if parts.iter().any(|part| {
                    matches!(part, MediaPartV1::File { transform: MediaTransformV1::Concat, .. })
                }) {
                    return Err(MediaDescriptorErrorV1::Transform(
                        TransformErrorV1::ConcatNeedsList,
                    ));
                }
                MediaBodyV1::Multipart(parts)
            }
            BodyWire::Text(text) => {
                if !MEDIA_TEXT_BODY_MEDIA_TYPES.contains(&text.media_type.as_str())
                    || text.text.len() > MAX_MEDIA_TEXT_BODY_BYTES
                {
                    return Err(MediaDescriptorErrorV1::TextBody);
                }
                MediaBodyV1::Text(text)
            }
            BodyWire::Empty => MediaBodyV1::Empty,
        };
        if matches!(body, MediaBodyV1::Multipart(_) | MediaBodyV1::Text(_))
            && headers.get("content-type").is_some()
        {
            return Err(MediaDescriptorErrorV1::Headers(None));
        }
        Ok(Self { path, query, headers, auth: wire.auth, body })
    }

    fn check_template(text: &str) -> Result<(), MediaDescriptorErrorV1> {
        let node = json::parse(text.as_bytes())
            .map_err(|_| MediaDescriptorErrorV1::Malformed("template is not JSON".into()))?;
        walk_references(&node, &mut |_| Ok(()))
    }

    /// The relative path.
    #[must_use]
    pub const fn path(&self) -> &RelativePathV1 {
        &self.path
    }

    /// The canonical query, when the descriptor has one.
    #[must_use]
    pub const fn query(&self) -> Option<&QueryStringV1> {
        self.query.as_ref()
    }

    /// The ordinary headers.
    #[must_use]
    pub const fn headers(&self) -> &SafeHeaders {
        &self.headers
    }

    /// The auth arm, when the descriptor has one.
    #[must_use]
    pub const fn auth(&self) -> Option<&MediaAuthV1> {
        self.auth.as_ref()
    }

    /// The body.
    #[must_use]
    pub const fn body(&self) -> &MediaBodyV1 {
        &self.body
    }

    /// Serializes the descriptor compactly in its wire shape: `method`, `path`, `query` (canonical
    /// order) and `headers` (lower-case names, sorted) when present, `auth` when present, and the
    /// body — a JSON template is copied as its source text. [`Self::parse`] of the result gives
    /// back an equal descriptor.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\"method\":\"POST\",\"path\":");
        json::write_string(self.path.as_str(), &mut out);
        if let Some(query) = &self.query {
            out.push_str(",\"query\":[");
            for (index, pair) in query.as_str().split('&').enumerate() {
                let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
                if index > 0 {
                    out.push(',');
                }
                out.push_str("{\"name\":");
                json::write_string(name, &mut out);
                out.push_str(",\"value\":");
                json::write_string(value, &mut out);
                out.push('}');
            }
            out.push(']');
        }
        if !self.headers.is_empty() {
            out.push_str(",\"headers\":[");
            for (index, (name, value)) in self.headers.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str("{\"name\":");
                json::write_string(name, &mut out);
                out.push_str(",\"value\":");
                json::write_string(value, &mut out);
                out.push('}');
            }
            out.push(']');
        }
        if let Some(auth) = &self.auth {
            out.push_str(",\"auth\":");
            // A two-field enum of strings always serializes.
            out.push_str(&serde_json::to_string(auth).unwrap_or_default());
        }
        out.push_str(",\"body\":");
        match &self.body {
            MediaBodyV1::Json(template) => {
                out.push_str("{\"json\":{\"template\":");
                out.push_str(template);
                out.push_str("}}");
            }
            MediaBodyV1::Multipart(parts) => {
                out.push_str("{\"multipart\":{\"parts\":");
                out.push_str(&serde_json::to_string(parts).unwrap_or_default());
                out.push_str("}}");
            }
            MediaBodyV1::Text(text) => {
                out.push_str("{\"text\":");
                out.push_str(&serde_json::to_string(text).unwrap_or_default());
                out.push('}');
            }
            MediaBodyV1::Empty => out.push_str("\"empty\""),
        }
        out.push('}');
        out
    }

    /// Every blob the descriptor references, in order of appearance.
    #[must_use]
    pub fn referenced_blobs(&self) -> Vec<BlobIdV1> {
        let mut ids = Vec::new();
        match &self.body {
            MediaBodyV1::Json(text) => {
                if let Ok(node) = json::parse(text.as_bytes()) {
                    let _ = walk_references(&node, &mut |reference| {
                        ids.push(reference.blob.clone());
                        Ok(())
                    });
                }
            }
            MediaBodyV1::Multipart(parts) => {
                for part in parts {
                    if let MediaPartV1::File { blob, .. } = part {
                        ids.push(blob.clone());
                    }
                }
            }
            MediaBodyV1::Text(_) | MediaBodyV1::Empty => {}
        }
        ids
    }
}

impl fmt::Debug for MediaRequestDescriptorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body = match &self.body {
            MediaBodyV1::Json(text) => format!("json({} bytes)", text.len()),
            MediaBodyV1::Multipart(parts) => format!("multipart({} parts)", parts.len()),
            MediaBodyV1::Text(text) => {
                format!("text({}, {} bytes)", text.media_type, text.text.len())
            }
            MediaBodyV1::Empty => "empty".to_owned(),
        };
        formatter
            .debug_struct("MediaRequestDescriptorV1")
            .field("path", &self.path)
            .field("query", &self.query)
            .field("headers", &self.headers)
            .field("auth", &self.auth)
            .field("body", &body)
            .finish()
    }
}

/// The compact source text of `/body/json/template`.
fn template_text(descriptor: &str) -> Result<String, MediaDescriptorErrorV1> {
    let malformed = || MediaDescriptorErrorV1::Malformed("template not found".into());
    let node = json::parse(descriptor.as_bytes()).map_err(|_| malformed())?;
    let member = |node: &Node, key: &str| match node {
        Node::Object(members) => members
            .iter()
            .find(|member| member.key.decoded == key)
            .map(|member| member.value.clone()),
        _ => None,
    };
    let template = member(&node, "body")
        .and_then(|body| member(&body, "json"))
        .and_then(|json| member(&json, "template"))
        .ok_or_else(malformed)?;
    let mut text = String::new();
    json::write_compact(&template, &mut text);
    Ok(text)
}

/// Visits every `$south.ref` node; refuses `$south.blob`, any other `$south.` key, and a
/// malformed reference.
fn walk_references(
    node: &Node,
    visit: &mut dyn FnMut(&MediaReferenceV1) -> Result<(), MediaDescriptorErrorV1>,
) -> Result<(), MediaDescriptorErrorV1> {
    match node {
        Node::Scalar(_) | Node::String(_) => Ok(()),
        Node::Array(items) => items.iter().try_for_each(|item| walk_references(item, visit)),
        Node::Object(members) => {
            if let Some(reference) = reference_of(members)? {
                return visit(&reference);
            }
            for Member { key, value } in members {
                if key.decoded.starts_with(RESERVED_KEY_PREFIX) {
                    return Err(MediaDescriptorErrorV1::Reference);
                }
                walk_references(value, visit)?;
            }
            Ok(())
        }
    }
}

/// `Some` when `members` is exactly one `$south.ref` member holding a valid reference.
fn reference_of(members: &[Member]) -> Result<Option<MediaReferenceV1>, MediaDescriptorErrorV1> {
    let Some(first) = members.first() else {
        return Ok(None);
    };
    if first.key.decoded != REFERENCE_KEY {
        return Ok(None);
    }
    if members.len() != 1 {
        return Err(MediaDescriptorErrorV1::Reference);
    }
    let mut text = String::new();
    json::write_compact(&first.value, &mut text);
    let reference: MediaReferenceV1 =
        serde_json::from_str(&text).map_err(|_| MediaDescriptorErrorV1::Reference)?;
    if reference.transform == MediaTransformV1::Concat {
        return Err(MediaDescriptorErrorV1::Reference);
    }
    Ok(Some(reference))
}

/// Expands a JSON template (§6.3).
///
/// Every `$south.ref` node becomes the JSON string its transform produces from its blob,
/// everything else is copied as written, and the result is checked as a [`JsonBodyV1`] (one JSON
/// value within the request bound).
///
/// A transform whose output is not UTF-8 text is refused: a JSON body can only carry text, so a
/// component that wants bytes in JSON names `base64` or `data_url`.
pub fn expand_json_template_v1(
    template: &str,
    blobs: &BlobSetV1,
) -> Result<JsonBodyV1, MediaDescriptorErrorV1> {
    let mut node = json::parse(template.as_bytes())
        .map_err(|_| MediaDescriptorErrorV1::Malformed("template is not JSON".into()))?;
    expand(&mut node, blobs)?;
    let mut out = String::with_capacity(template.len());
    json::write_compact(&node, &mut out);
    JsonBodyV1::parse(&out).map_err(MediaDescriptorErrorV1::Contract)
}

fn expand(node: &mut Node, blobs: &BlobSetV1) -> Result<(), MediaDescriptorErrorV1> {
    match node {
        Node::Scalar(_) | Node::String(_) => Ok(()),
        Node::Array(items) => items.iter_mut().try_for_each(|item| expand(item, blobs)),
        Node::Object(members) => {
            if let Some(reference) = reference_of(members)? {
                let blob = blobs.get(&reference.blob).ok_or(MediaDescriptorErrorV1::UnknownBlob)?;
                let bytes = reference
                    .transform
                    .apply(blob.bytes(), reference.media_type.as_deref())
                    .map_err(MediaDescriptorErrorV1::Transform)?;
                let text = String::from_utf8(bytes)
                    .map_err(|_| MediaDescriptorErrorV1::Transform(TransformErrorV1::NotText))?;
                let mut literal = String::with_capacity(text.len() + 2);
                json::write_string(&text, &mut literal);
                *node = Node::Scalar(literal);
                return Ok(());
            }
            for Member { key, value } in members.iter_mut() {
                if key.decoded.starts_with(RESERVED_KEY_PREFIX) {
                    return Err(MediaDescriptorErrorV1::Reference);
                }
                expand(value, blobs)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::BlobV1;

    fn parse(json: &str) -> Result<MediaRequestDescriptorV1, MediaDescriptorErrorV1> {
        MediaRequestDescriptorV1::parse(json, &MediaLimitsV1::V1)
    }

    #[test]
    fn a_json_descriptor_parses_and_expands() {
        let descriptor = parse(
            r#"{"method":"POST","path":"openai/deployments/d/images/generations",
                "query":[{"name":"api-version","value":"2025-04-01-preview"}],
                "auth":{"arm":"header_secret","header":"api-key","slot":"default"},
                "body":{"json":{"template":{"prompt":"x","image":{"$south.ref":{"blob":"b0","transform":"data_url","media_type":"image/png"}},"n":1}}}}"#,
        )
        .expect("valid");
        assert_eq!(descriptor.query().expect("query").as_str(), "api-version=2025-04-01-preview");
        assert_eq!(descriptor.referenced_blobs(), vec![BlobIdV1::from_index(0)]);
        let MediaBodyV1::Json(template) = descriptor.body() else { panic!("json") };
        let blobs =
            BlobSetV1::new([BlobV1::file(BlobIdV1::from_index(0), b"hi".to_vec())]).expect("ids");
        let body = expand_json_template_v1(template, &blobs).expect("expands");
        assert_eq!(body.as_str(), r#"{"prompt":"x","image":"data:image/png;base64,aGk=","n":1}"#);
    }

    #[test]
    fn as_is_text_blobs_expand_unchanged_and_bytes_are_refused() {
        let blobs = BlobSetV1::new([
            BlobV1::text(BlobIdV1::from_index(0), "a \"quoted\"\nline".into()),
            BlobV1::file(BlobIdV1::from_index(1), vec![0xff]),
        ])
        .expect("ids");
        let body = expand_json_template_v1(
            r#"[{"$south.ref":{"blob":"b0","transform":"as_is"}}]"#,
            &blobs,
        )
        .expect("expands");
        assert_eq!(body.as_str(), r#"["a \"quoted\"\nline"]"#);
        assert_eq!(
            expand_json_template_v1(r#"{"$south.ref":{"blob":"b1","transform":"as_is"}}"#, &blobs)
                .map(|_| ()),
            Err(MediaDescriptorErrorV1::Transform(TransformErrorV1::NotText))
        );
        assert_eq!(
            expand_json_template_v1(r#"{"$south.ref":{"blob":"b9","transform":"as_is"}}"#, &blobs)
                .map(|_| ()),
            Err(MediaDescriptorErrorV1::UnknownBlob)
        );
    }

    #[test]
    fn references_are_checked() {
        let base = |template: &str| {
            format!(
                r#"{{"method":"POST","path":"v1/x","body":{{"json":{{"template":{template}}}}}}}"#
            )
        };
        for bad in [
            r#"{"$south.blob":{"id":"b0","bytes":1,"head":""}}"#,
            r#"{"$south.ref":{"blob":"b0","transform":"concat"}}"#,
            r#"{"$south.ref":{"blob":"b0","transform":"as_is"},"x":1}"#,
            r#"{"$south.ref":{"blob":"x0","transform":"as_is"}}"#,
            r#"{"$south.ref":{"blob":"b0","transform":"as_is","extra":1}}"#,
            r#"{"a":{"$south.artifact":{"index":0}}}"#,
        ] {
            assert_eq!(
                parse(&base(bad)).map(|_| ()),
                Err(MediaDescriptorErrorV1::Reference),
                "{bad}"
            );
        }
    }

    #[test]
    fn descriptor_rules() {
        assert_eq!(
            parse(r#"{"method":"GET","path":"v1/x","body":"empty"}"#).map(|_| ()),
            Err(MediaDescriptorErrorV1::Method)
        );
        assert!(matches!(
            parse(r#"{"method":"POST","path":"/abs","body":"empty"}"#),
            Err(MediaDescriptorErrorV1::Contract(ContractErrorV1::InvalidRelativePath))
        ));
        assert_eq!(
            parse(r#"{"method":"POST","path":"v1/x","query":[{"name":"model","value":"x"}],"body":"empty"}"#).map(|_| ()),
            Err(MediaDescriptorErrorV1::UnsanctionedQuery)
        );
        assert_eq!(
            parse(r#"{"method":"POST","path":"v1/x","headers":[{"name":"content-type","value":"x"}],"body":{"multipart":{"parts":[{"name":"a","value":"b"}]}}}"#).map(|_| ()),
            Err(MediaDescriptorErrorV1::Headers(None))
        );
        assert_eq!(
            parse(r#"{"method":"POST","path":"v1/x","body":{"multipart":{"parts":[]}}}"#)
                .map(|_| ()),
            Err(MediaDescriptorErrorV1::PartCount)
        );
        assert_eq!(
            parse(r#"{"method":"POST","path":"v1/x","body":{"text":{"media_type":"text/plain","text":"x"}}}"#).map(|_| ()),
            Err(MediaDescriptorErrorV1::TextBody)
        );
        assert_eq!(
            parse(r#"{"method":"POST","path":"v1/x","auth":{"arm":"header_secret","header":"Api-Key","slot":"default"},"body":"empty"}"#).map(|_| ()),
            Err(MediaDescriptorErrorV1::AuthHeader)
        );
        assert!(matches!(
            parse(r#"{"method":"POST","path":"v1/x","body":"empty","extra":1}"#),
            Err(MediaDescriptorErrorV1::Malformed(_))
        ));
        assert!(parse(r#"{"method":"POST","path":"v1/x","body":{"text":{"media_type":"application/ssml+xml","text":"<speak/>"}}}"#).is_ok());
    }

    #[test]
    fn descriptors_serialize_back_to_what_they_parse_from() {
        for text in [
            r#"{"method":"POST","path":"openai/deployments/d/images/generations","query":[{"name":"api-version","value":"2025-04-01-preview"}],"headers":[{"name":"x-trace","value":"1"}],"auth":{"arm":"header_secret","header":"api-key","slot":"default"},"body":{"json":{"template":{"prompt":"x","n":1.0,"image":{"$south.ref":{"blob":"b0","transform":"as_is"}}}}}}"#,
            r#"{"method":"POST","path":"v1/images/edits","auth":{"arm":"bearer","slot":"default"},"body":{"multipart":{"parts":[{"name":"prompt","value":"x"},{"name":"image","blob":"b0","transform":"as_is","filename":"a.png"}]}}}"#,
            r#"{"method":"POST","path":"v1/x","body":{"text":{"media_type":"application/ssml+xml","text":"<speak/>"}}}"#,
            r#"{"method":"POST","path":"v1/x","body":"empty"}"#,
        ] {
            let descriptor = parse(text).expect("valid");
            assert_eq!(descriptor.to_json(), text);
            assert_eq!(parse(&descriptor.to_json()).expect("re-parses"), descriptor);
        }
    }
}
