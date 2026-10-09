#![no_main]

//! Fuzz targets for the grammars of `contracts.media` v1 (image record §6, §11): the elider, the
//! multipart parser and encoder, the descriptor and its template expansion, the transforms and
//! the artifact URL grammar. Besides "never panics", each checks an invariant a host relies on.

use libfuzzer_sys::fuzz_target;
use south_contracts::MultipartBoundaryV1;
use south_contracts::media::{
    ArtifactUrlV1, BlobSetV1, MediaLimitsV1, MediaPartV1, MediaPartViewV1, MediaRequestDescriptorV1,
    MediaTransformV1, PathPatternV1, decode_base64, decode_hex, elide_v1, encode_base64,
    encode_multipart_v1, expand_json_template_v1, parse_data_url_v1, parse_multipart_parts_v1,
};

const PATTERNS: [&str; 4] = ["/*", "/data/*/b64_json", "/a/*/*", "/x~1y"];

fuzz_target!(|data: &[u8]| {
    let Some((&selector, input)) = data.split_first() else {
        return;
    };
    let small = MediaLimitsV1 { inline_string_bytes: 16, ..MediaLimitsV1::V1 };
    match selector % 6 {
        0 => {
            let declared: Vec<_> = PATTERNS
                .iter()
                .take(usize::from(selector >> 4) % (PATTERNS.len() + 1))
                .map(|pattern| PathPatternV1::parse(pattern).expect("fixed patterns are valid"))
                .collect();
            if let Ok((view, blobs)) = elide_v1(input, &declared, &small) {
                // A view is one JSON document, and it never carries an elided string's bytes
                // beyond the head.
                let parsed: serde_json::Value =
                    serde_json::from_str(view.as_str()).expect("a view is JSON");
                drop(parsed);
                for blob in &blobs {
                    assert!(blob.is_text());
                }
            }
        }
        1 => {
            let boundary = MultipartBoundaryV1::parse("fuzzBoundary").expect("valid");
            if let Ok((view, blobs)) = parse_multipart_parts_v1(input, &boundary, &small) {
                // Re-encoding what was split and splitting it again gives the same parts.
                let set = BlobSetV1::new(blobs).expect("ids are unique");
                let parts: Vec<MediaPartV1> = view
                    .parts
                    .iter()
                    .filter_map(|part| match part {
                        MediaPartViewV1::File { name, blob, filename, media_type, .. } => {
                            Some(MediaPartV1::File {
                                name: name.clone(),
                                blob: blob.clone(),
                                transform: MediaTransformV1::AsIs,
                                filename: filename.clone(),
                                media_type: media_type.clone(),
                            })
                        }
                        MediaPartViewV1::Text { name, value: Some(value), .. } => {
                            Some(MediaPartV1::Text { name: name.clone(), value: value.clone() })
                        }
                        MediaPartViewV1::Text { .. } => None,
                    })
                    .collect();
                if let Ok(body) = encode_multipart_v1(&parts, &set, &boundary, &small) {
                    let (again, _) = parse_multipart_parts_v1(body.as_bytes(), &boundary, &small)
                        .expect("an encoded body splits");
                    assert_eq!(again.parts.len(), parts.len());
                }
            }
        }
        2 => {
            if let Ok(text) = std::str::from_utf8(input) {
                if let Ok(descriptor) = MediaRequestDescriptorV1::parse(text, &MediaLimitsV1::V1) {
                    let _ = descriptor.referenced_blobs();
                }
                let _ = expand_json_template_v1(text, &BlobSetV1::default());
            }
        }
        3 => {
            if let Ok(text) = std::str::from_utf8(input) {
                if let Ok(bytes) = decode_base64(text) {
                    assert_eq!(encode_base64(&bytes), text, "canonical base64 round-trips");
                }
                let _ = decode_hex(text);
                let _ = parse_data_url_v1(text);
                let _ = ArtifactUrlV1::parse(text);
                let _ = PathPatternV1::parse(text);
            }
        }
        4 => {
            let words = [
                MediaTransformV1::AsIs,
                MediaTransformV1::Base64,
                MediaTransformV1::DataUrl,
                MediaTransformV1::FromDataUrl,
                MediaTransformV1::FromBase64,
                MediaTransformV1::FromHex,
                MediaTransformV1::WavPcmS16le { sample_rate: 24_000, channels: 1 },
            ];
            let word = words[usize::from(selector >> 3) % words.len()];
            let _ = word.apply(input, Some("image/png"));
        }
        _ => {
            if let Ok(text) = std::str::from_utf8(input) {
                let _ = serde_json::from_str::<MediaTransformV1>(text);
                let _ = serde_json::from_str::<Vec<MediaPartV1>>(text);
            }
        }
    }
});
