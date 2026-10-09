//! `contracts.media` v1 against its golden vectors (`tests/vectors/media-v1.json`).
//!
//! The vectors are the cross-host contract (image record §12.3 items 1 and 2): a host that does
//! not link this crate reproduces every output byte for byte. Each expectation in the file was
//! written by hand from the record, never generated from this implementation.

use base64::Engine as _;
use serde_json::Value;
use south_contracts::MultipartBoundaryV1;
use south_contracts::media::{
    ArtifactUrlErrorV1, ArtifactUrlV1, BlobIdV1, BlobSetV1, BlobV1, ElisionErrorV1, MediaLimitsV1,
    MediaPartV1, MediaRequestViewV1, MediaTransformV1, PathPatternV1, elide_v1,
    encode_multipart_v1, expand_json_template_v1, is_forbidden_egress_address,
    parse_multipart_parts_v1,
};
use std::net::IpAddr;

fn vectors() -> Value {
    serde_json::from_str(include_str!("vectors/media-v1.json")).expect("the vector file is JSON")
}

fn base64(text: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(text).expect("vector base64")
}

fn bytes_of(case: &Value, field: &str) -> Vec<u8> {
    if let Some(text) = case[format!("{field}_text")].as_str() {
        return text.as_bytes().to_vec();
    }
    base64(case[format!("{field}_base64")].as_str().expect("a text or base64 field"))
}

fn limits(case: &Value) -> MediaLimitsV1 {
    case["inline_string_bytes"].as_u64().map_or(MediaLimitsV1::V1, |bytes| MediaLimitsV1 {
        inline_string_bytes: usize::try_from(bytes).expect("small"),
        ..MediaLimitsV1::V1
    })
}

fn blob_set(case: &Value) -> BlobSetV1 {
    BlobSetV1::new(case["blobs"].as_array().expect("blobs").iter().map(|blob| {
        let id = BlobIdV1::parse(blob["id"].as_str().expect("id")).expect("valid id");
        blob["text"].as_str().map_or_else(
            || BlobV1::file(id.clone(), base64(blob["base64"].as_str().expect("base64"))),
            |text| BlobV1::text(id.clone(), text.to_owned()),
        )
    }))
    .expect("distinct ids")
}

#[test]
fn elide_vectors() {
    for case in vectors()["elide"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let declared: Vec<_> = case["declared"]
            .as_array()
            .expect("declared")
            .iter()
            .map(|path| PathPatternV1::parse(path.as_str().expect("path")).expect("valid path"))
            .collect();
        let (view, blobs) =
            elide_v1(&bytes_of(case, "document"), &declared, &limits(case)).expect(name);
        assert_eq!(view.as_str(), case["view"].as_str().expect("view"), "{name}");
        let expected: Vec<&str> = case["blobs_text"]
            .as_array()
            .expect("blobs")
            .iter()
            .map(|blob| blob.as_str().expect("text"))
            .collect();
        let actual: Vec<&str> = blobs
            .iter()
            .map(|blob| std::str::from_utf8(blob.bytes()).expect("text blob"))
            .collect();
        assert_eq!(actual, expected, "{name}");
        // A view is always one JSON document a component can parse.
        serde_json::from_str::<Value>(view.as_str()).expect(name);
    }
}

#[test]
fn elide_refusal_vectors() {
    for case in vectors()["elide_refusals"].as_array().expect("cases") {
        let expected = match case["error"].as_str().expect("error") {
            "duplicate_key" => ElisionErrorV1::DuplicateKey,
            "reserved_key" => ElisionErrorV1::ReservedKey,
            "invalid_json" => ElisionErrorV1::InvalidJson,
            other => panic!("unknown error {other}"),
        };
        assert_eq!(
            elide_v1(&bytes_of(case, "document"), &[], &MediaLimitsV1::V1).map(|_| ()),
            Err(expected),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn multipart_parse_vectors() {
    for case in vectors()["multipart_parse"].as_array().expect("cases") {
        let boundary = MultipartBoundaryV1::parse(case["boundary"].as_str().expect("boundary"))
            .expect("valid");
        let (view, blobs) =
            parse_multipart_parts_v1(&bytes_of(case, "body"), &boundary, &MediaLimitsV1::V1)
                .expect("splits");
        assert_eq!(
            MediaRequestViewV1::Multipart(view).to_json(),
            case["view"].as_str().expect("view")
        );
        let expected: Vec<Vec<u8>> = case["blobs_base64"]
            .as_array()
            .expect("blobs")
            .iter()
            .map(|blob| base64(blob.as_str().expect("base64")))
            .collect();
        let actual: Vec<Vec<u8>> = blobs.iter().map(|blob| blob.bytes().to_vec()).collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn multipart_encode_vectors() {
    for case in vectors()["multipart_encode"].as_array().expect("cases") {
        let boundary = MultipartBoundaryV1::parse(case["boundary"].as_str().expect("boundary"))
            .expect("valid");
        let parts: Vec<MediaPartV1> = serde_json::from_value(case["parts"].clone()).expect("parts");
        let body = encode_multipart_v1(&parts, &blob_set(case), &boundary, &MediaLimitsV1::V1)
            .expect("encodes");
        assert_eq!(body.as_bytes(), bytes_of(case, "body").as_slice(), "{}", case["name"]);
    }
}

#[test]
fn template_vectors() {
    for case in vectors()["template_expand"].as_array().expect("cases") {
        let body =
            expand_json_template_v1(case["template"].as_str().expect("template"), &blob_set(case))
                .expect("expands");
        assert_eq!(body.as_str(), case["body"].as_str().expect("body"), "{}", case["name"]);
    }
}

#[test]
fn transform_vectors() {
    let vectors = vectors();
    for case in vectors["transforms"].as_array().expect("cases") {
        let transform: MediaTransformV1 =
            serde_json::from_value(case["transform"].clone()).expect("word");
        let output = transform
            .apply(&bytes_of(case, "input"), case["media_type"].as_str())
            .expect("applies");
        assert_eq!(output, bytes_of(case, "output"), "{transform:?}");
    }
    for case in vectors["transform_refusals"].as_array().expect("cases") {
        let transform: MediaTransformV1 =
            serde_json::from_value(case["transform"].clone()).expect("word");
        assert!(transform.apply(&bytes_of(case, "input"), None).is_err(), "{case}");
    }
}

#[test]
fn egress_vectors() {
    let vectors = vectors();
    for address in vectors["forbidden_addresses"].as_array().expect("addresses") {
        let parsed: IpAddr = address.as_str().expect("text").parse().expect("an address");
        assert!(is_forbidden_egress_address(parsed), "{address}");
    }
    for address in vectors["allowed_addresses"].as_array().expect("addresses") {
        let parsed: IpAddr = address.as_str().expect("text").parse().expect("an address");
        assert!(!is_forbidden_egress_address(parsed), "{address}");
    }
    for case in vectors["artifact_urls"].as_array().expect("urls") {
        let result = ArtifactUrlV1::parse(case["url"].as_str().expect("url")).map(|_| ());
        let expected = match case["error"].as_str() {
            None => Ok(()),
            Some("not_https") => Err(ArtifactUrlErrorV1::NotHttps),
            Some("userinfo") => Err(ArtifactUrlErrorV1::Userinfo),
            Some("forbidden_host") => Err(ArtifactUrlErrorV1::ForbiddenHost),
            Some("forbidden_address") => Err(ArtifactUrlErrorV1::ForbiddenAddress),
            Some(other) => panic!("unknown error {other}"),
        };
        assert_eq!(result, expected, "{}", case["url"]);
    }
}
