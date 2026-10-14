#![no_main]

//! Fuzz targets for the grammars of `contracts.media` v1 (image record §6, §11): the elider, the
//! multipart parser and encoder, the descriptor and its template expansion, the transforms and
//! the artifact URL grammar, the speech contract's codecs and the `sse` response view (speech
//! record §6–§8). Besides "never panics", each checks an invariant a host relies on.

use libfuzzer_sys::fuzz_target;
use south_contracts::MultipartBoundaryV1;
use south_contracts::image::{
    DeliveredArtifactV1, ImageArtifactV1, ImageFactsV1, ImageMeteringV1, check_render_template_v1,
    fill_render_template_v1, parse_image_model_capabilities_v1, parse_prepared_image_call_v1,
};
use south_contracts::media::{
    ArtifactUrlV1, BlobSetV1, MediaLimitsV1, MediaPartV1, MediaPartViewV1,
    MediaRequestDescriptorV1, MediaTransformV1, PathPatternV1, SseEventFieldsV1, UpstreamRoundV1,
    build_media_sse_response_view_v1, decode_base64, decode_hex, elide_v1, encode_base64,
    encode_multipart_v1, expand_json_template_v1, parse_data_url_v1, parse_multipart_parts_v1,
};
use south_contracts::speech::{
    SpeechArtifactV1, SpeechFactsV1, SpeechMeteringV1, fill_transcript_template_v1,
    parse_prepared_speech_call_v1, parse_speech_model_capabilities_v1,
    parse_transcript_delivery_v1,
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
                // `contracts.media` 3: a declared query parameter (speech world batch V3-1 §6).
                // A descriptor parsed with the declarations re-parses to itself with them.
                let declared = [south_contracts::DeclaredQueryParameterV1::try_new(
                    "output_format",
                    south_contracts::QueryValueSyntaxV1::Token,
                )
                .expect("a valid declaration")];
                if let Ok(descriptor) = MediaRequestDescriptorV1::parse_with_declared(
                    text,
                    &MediaLimitsV1::V1,
                    &declared,
                ) {
                    let _ = descriptor.minimum_contract_version();
                    let again = MediaRequestDescriptorV1::parse_with_declared(
                        &descriptor.to_json(),
                        &MediaLimitsV1::V1,
                        &declared,
                    );
                    assert_eq!(again.ok(), Some(descriptor), "a declared descriptor round-trips");
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
                MediaTransformV1::DataUrlPayload,
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
                let _ = serde_json::from_str::<ImageFactsV1>(text);
                let _ = serde_json::from_str::<ImageMeteringV1>(text);
                let _ = parse_image_model_capabilities_v1(text);
                let _ = parse_prepared_image_call_v1(text, &MediaLimitsV1::V1);
                let _ = serde_json::from_str::<SpeechFactsV1>(text);
                let _ = serde_json::from_str::<SpeechMeteringV1>(text);
                let _ = parse_speech_model_capabilities_v1(text);
                if let Ok(artifact) = serde_json::from_str::<SpeechArtifactV1>(text) {
                    // A decoded artifact re-encodes to a document that decodes to it again.
                    let again = serde_json::to_string(&artifact).expect("an artifact serializes");
                    assert_eq!(
                        serde_json::from_str::<SpeechArtifactV1>(&again).ok(),
                        Some(artifact)
                    );
                }
                if let Ok(prepared) = parse_prepared_speech_call_v1(text, &MediaLimitsV1::V1) {
                    let again =
                        parse_prepared_speech_call_v1(&prepared.to_json(), &MediaLimitsV1::V1);
                    assert_eq!(again.ok(), Some(prepared), "a prepared speech call round-trips");
                }
                if let Ok(delivery) = parse_transcript_delivery_v1(text, &small) {
                    let again = parse_transcript_delivery_v1(&delivery.to_json(), &small);
                    assert_eq!(again.as_ref().ok(), Some(&delivery), "a delivery round-trips");
                    if let south_contracts::speech::TranscriptDeliveryV1::Json { template } =
                        &delivery
                    {
                        // A checked template fills without a structural error once its blobs exist.
                        let blobs =
                            BlobSetV1::new(delivery.referenced_blobs().into_iter().map(|id| {
                                south_contracts::media::BlobV1::text(id, "x".to_owned())
                            }));
                        if let Ok(blobs) = blobs {
                            fill_transcript_template_v1(template, &blobs)
                                .expect("a checked template fills");
                        }
                    }
                }
                // The `sse` view (speech record §6): each blank-line-separated chunk is one event's
                // data. The view is one JSON document, and every blob it references is returned.
                let events: Vec<SseEventFieldsV1<'_>> = text
                    .split("\n\n")
                    .map(|data| SseEventFieldsV1 { event: "message", data })
                    .collect();
                let declared = [PathPatternV1::parse("/*/data/delta").expect("a fixed pattern")];
                let round = UpstreamRoundV1 {
                    status: 200,
                    headers: &[],
                    body: &[],
                    content_type: Some("text/event-stream"),
                };
                if let Ok((view, blobs)) =
                    build_media_sse_response_view_v1(round, &events, &[], &declared, &small)
                {
                    let parsed: serde_json::Value =
                        serde_json::from_str(&view.to_json()).expect("an sse view is JSON");
                    assert_eq!(
                        parsed["body"]["sse"].as_array().map(Vec::len),
                        Some(events.len()),
                        "one entry per event"
                    );
                    assert!(blobs.iter().all(|blob| blob.is_text()));
                }
                let artifacts = [ImageArtifactV1::Body { media_type: "image/png".to_owned() }];
                if check_render_template_v1(text, &artifacts, &small).is_ok() {
                    // A template that passes the check fills without a structural error.
                    let delivered = [DeliveredArtifactV1::Bytes(vec![1, 2, 3])];
                    fill_render_template_v1(text, &delivered).expect("a checked template fills");
                }
            }
        }
    }
});
