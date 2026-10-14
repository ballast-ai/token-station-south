//! The speech world's host-obligation vectors (`tests/vectors/speech-v1.json`; speech record §6,
//! §10, §11, step S-S-3).
//!
//! A host that does not link this crate reproduces every output byte for byte: the WAV container a
//! `container` artifact asks for, and the `sse` response view built from the events
//! `decode_sse_v1` decoded. That the events are what `decode_sse_v1` gives for each `body` is
//! checked where that crate is linked, in `south-component-conformance`'s
//! `speech_host_obligations_v1` (this crate may not depend on it, boundary record §13.13). Each
//! expectation in the file was written by hand from the record, never generated from this
//! implementation.

use base64::Engine as _;
use serde_json::Value;
use south_contracts::media::{
    ElisionErrorV1, MediaLimitsV1, MediaResponseErrorV1, PathPatternV1, SseEventFieldsV1,
    UpstreamRoundV1, build_media_sse_response_view_v1, wav_pcm_s16le_v1,
};
use south_contracts::speech::SpeechContainerV1;

fn vectors() -> Value {
    serde_json::from_str(include_str!("vectors/speech-v1.json")).expect("the vector file is JSON")
}

fn base64(text: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(text).expect("vector base64")
}

fn rate_and_channels(case: &Value) -> (u32, u16) {
    (
        u32::try_from(case["sample_rate"].as_u64().expect("sample_rate")).expect("u32"),
        u16::try_from(case["channels"].as_u64().expect("channels")).expect("u16"),
    )
}

fn declared(case: &Value) -> Vec<PathPatternV1> {
    case["declared"]
        .as_array()
        .expect("declared")
        .iter()
        .map(|path| PathPatternV1::parse(path.as_str().expect("path")).expect("valid path"))
        .collect()
}

fn events(case: &Value) -> Vec<(String, String)> {
    case["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|event| {
            (
                event["event"].as_str().expect("event").to_owned(),
                event["data"].as_str().expect("data").to_owned(),
            )
        })
        .collect()
}

/// Each blob as its id and bytes.
type Blobs = Vec<(String, Vec<u8>)>;

fn view(
    events: &[(String, String)],
    declared: &[PathPatternV1],
) -> Result<(String, Blobs), MediaResponseErrorV1> {
    let fields: Vec<SseEventFieldsV1<'_>> =
        events.iter().map(|(event, data)| SseEventFieldsV1 { event, data }).collect();
    let (view, blobs) = build_media_sse_response_view_v1(
        UpstreamRoundV1 { status: 200, headers: &[], body: &[], content_type: None },
        &fields,
        &[],
        declared,
        &MediaLimitsV1::V1,
    )?;
    Ok((
        view.to_json(),
        blobs.iter().map(|blob| (blob.id().as_str().to_owned(), blob.bytes().to_vec())).collect(),
    ))
}

#[test]
fn wav_container_vectors() {
    let vectors = vectors();
    for case in vectors["wav_container"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let (sample_rate, channels) = rate_and_channels(case);
        let pcm = base64(case["pcm_base64"].as_str().expect("pcm"));
        let wav = base64(case["wav_base64"].as_str().expect("wav"));
        assert_eq!(wav_pcm_s16le_v1(&pcm, sample_rate, channels).as_ref(), Ok(&wav), "{name}");
        // The artifact's container names the same transform, so a host applying the declared
        // container gets the same bytes.
        let container: SpeechContainerV1 = serde_json::from_value(serde_json::json!({
            "wav_pcm_s16le": { "sample_rate": sample_rate, "channels": channels }
        }))
        .expect("a valid container");
        assert_eq!(container.transform().apply(&pcm, None).as_ref(), Ok(&wav), "{name}");
        assert_eq!(wav.len(), pcm.len() + 44, "{name}: a 44-byte header");
        assert_eq!(case["deliverable"].as_bool(), Some(!pcm.is_empty()), "{name}");
    }
}

#[test]
fn wav_container_refusal_vectors() {
    for case in vectors()["wav_container_refusals"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let (sample_rate, channels) = rate_and_channels(case);
        let pcm = base64(case["pcm_base64"].as_str().expect("pcm"));
        assert!(wav_pcm_s16le_v1(&pcm, sample_rate, channels).is_err(), "{name}");
    }
}

#[test]
fn sse_view_vectors() {
    for case in vectors()["sse_view"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let (view, blobs) = view(&events(case), &declared(case)).expect(name);
        assert_eq!(view, case["view"].as_str().expect("view"), "{name}");
        let expected: Blobs = case["blobs"]
            .as_array()
            .expect("blobs")
            .iter()
            .map(|blob| {
                (
                    blob["id"].as_str().expect("id").to_owned(),
                    blob["text"].as_str().expect("text").as_bytes().to_vec(),
                )
            })
            .collect();
        assert_eq!(blobs, expected, "{name}");
    }
}

#[test]
fn sse_view_refusal_vectors() {
    for case in vectors()["sse_view_refusals"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let error = match case["error"].as_str().expect("error") {
            "reserved_key" => ElisionErrorV1::ReservedKey,
            "duplicate_key" => ElisionErrorV1::DuplicateKey,
            other => panic!("{name}: unknown error word {other}"),
        };
        assert_eq!(
            view(&events(case), &declared(case)).map(|_| ()),
            Err(MediaResponseErrorV1::Elision(error)),
            "{name}"
        );
    }
}
