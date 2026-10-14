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

#[test]
fn render_vectors() {
    use south_contracts::image::{
        DeliveredArtifactV1, ImageArtifactV1, ImageContractErrorV1, check_render_template_v1,
        fill_render_template_v1,
    };
    let vectors = vectors();
    let render = &vectors["render"];
    let artifacts: Vec<ImageArtifactV1> =
        serde_json::from_value(render["artifacts"].clone()).expect("artifacts");
    for case in render["fill"].as_array().expect("cases") {
        let template = case["template"].as_str().expect("template");
        check_render_template_v1(template, &artifacts, &MediaLimitsV1::V1).expect("valid template");
        let delivered: Vec<DeliveredArtifactV1> = case["delivered"]
            .as_array()
            .expect("delivered")
            .iter()
            .map(|item| {
                item["url"].as_str().map_or_else(
                    || {
                        DeliveredArtifactV1::Bytes(base64(
                            item["bytes_base64"].as_str().expect("bytes"),
                        ))
                    },
                    |url| DeliveredArtifactV1::Url(url.to_owned()),
                )
            })
            .collect();
        let body = fill_render_template_v1(template, &delivered).expect("fills");
        assert_eq!(body, case["body"].as_str().expect("body"), "{}", case["name"]);
    }
    for case in render["check_refusals"].as_array().expect("cases") {
        let expected = match case["error"].as_str().expect("error") {
            "delivery" => ImageContractErrorV1::Delivery,
            "reference_integrity" => ImageContractErrorV1::ReferenceIntegrity,
            other => panic!("unknown error {other}"),
        };
        assert_eq!(
            check_render_template_v1(
                case["template"].as_str().expect("template"),
                &artifacts,
                &MediaLimitsV1::V1
            ),
            Err(expected),
            "{}",
            case["name"]
        );
    }
}

/// The `syntax` word of a vector declaration, as a manifest spells it.
fn query_syntax(word: &str) -> south_contracts::QueryValueSyntaxV1 {
    match word {
        "token" => south_contracts::QueryValueSyntaxV1::Token,
        "digits" => south_contracts::QueryValueSyntaxV1::Digits,
        "date" => south_contracts::QueryValueSyntaxV1::Date,
        other => panic!("a vector syntax word: {other}"),
    }
}

/// The refusal a vector names, as the descriptor parser reports it.
fn descriptor_error(word: &str) -> south_contracts::media::MediaDescriptorErrorV1 {
    use south_contracts::ContractErrorV1;
    use south_contracts::media::MediaDescriptorErrorV1;
    match word {
        "unsanctioned_query" => MediaDescriptorErrorV1::UnsanctionedQuery,
        "invalid_query_value" => {
            MediaDescriptorErrorV1::Contract(ContractErrorV1::InvalidQueryValue)
        }
        "duplicate_query_parameter" => {
            MediaDescriptorErrorV1::Contract(ContractErrorV1::DuplicateQueryParameter)
        }
        other => panic!("a vector error word: {other}"),
    }
}

/// `contracts.media` 3 (speech world batch V3-1 record §6): a descriptor query entry may name a
/// parameter the package declares. Each case is parsed with and without the declarations.
#[test]
fn declared_query_vectors() {
    use south_contracts::DeclaredQueryParameterV1;
    use south_contracts::media::MediaRequestDescriptorV1;
    let vectors = vectors();
    let section = &vectors["descriptor_declared_query"];
    let cases = section["cases"].as_array().expect("cases");
    assert!(!cases.is_empty());
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let declared: Vec<DeclaredQueryParameterV1> = case["declarations"]
            .as_array()
            .expect("declarations")
            .iter()
            .map(|entry| {
                DeclaredQueryParameterV1::try_new(
                    entry["name"].as_str().expect("name"),
                    query_syntax(entry["syntax"].as_str().expect("syntax")),
                )
                .expect("a valid declaration")
            })
            .collect();
        let text = case["descriptor"].as_str().expect("descriptor");
        for (key, declarations) in
            [("with_declarations", declared.as_slice()), ("without_declarations", &[][..])]
        {
            let expected = &case[key];
            let parsed = MediaRequestDescriptorV1::parse_with_declared(
                text,
                &MediaLimitsV1::V1,
                declarations,
            );
            if let Some(error) = expected["error"].as_str() {
                assert_eq!(parsed.map(|_| ()), Err(descriptor_error(error)), "{name}: {key}");
            } else {
                let descriptor = parsed.unwrap_or_else(|error| panic!("{name}: {key}: {error}"));
                assert_eq!(
                    descriptor.query().map(south_contracts::QueryStringV1::as_str),
                    expected["query"].as_str(),
                    "{name}: {key}"
                );
                assert_eq!(
                    u64::from(descriptor.minimum_contract_version()),
                    expected["minimum_contract_version"].as_u64().expect("a version"),
                    "{name}: {key}"
                );
            }
        }
        // `parse` is the same call with no declarations.
        assert_eq!(
            MediaRequestDescriptorV1::parse(text, &MediaLimitsV1::V1),
            MediaRequestDescriptorV1::parse_with_declared(text, &MediaLimitsV1::V1, &[]),
            "{name}"
        );
    }
    for refused in section["declaration_refusals"].as_array().expect("refusals") {
        assert_eq!(
            DeclaredQueryParameterV1::try_new(
                refused["name"].as_str().expect("name"),
                query_syntax(refused["syntax"].as_str().expect("syntax")),
            )
            .map(|_| ()),
            Err(south_contracts::ContractErrorV1::InvalidQueryDeclaration),
            "{refused}"
        );
    }
}
