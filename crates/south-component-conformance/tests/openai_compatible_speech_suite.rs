//! `south.speech-component.v1` against the `speech-openai-compatible` native reference and its
//! shipped pack, and the behaviours of design record `2026-10-14-speech-v3-1-tts.md` §2 and §8
//! pinned by name: the generated 4,096-event stream and the `MAX_SPEECH_SEGMENTS` bound (SV-Q6).

use std::fmt::Write as _;

use serde_json::{Value, json};
use south_component_conformance::reference_openai_compatible_speech::OpenAiCompatibleSpeechReferenceV1;
use south_component_conformance::speech_fixture::{
    SpeechCaseV1, SpeechFamilyV1, SpeechFixturePackV1,
};
use south_component_conformance::speech_json::{
    MAX_SPEECH_OUTCOME_JSON_BYTES, parse_speech_outcome_json, speech_outcome_json,
};
use south_component_conformance::{
    CheckV1, SpeechComponentV1, SpeechDeliveryV1, SpeechOutcomeV1, run_speech_component_suite_v1,
};
use south_contracts::media::{
    BlobV1, MediaLimitsV1, PathPatternV1, SseEventFieldsV1, UpstreamRoundV1,
    build_media_sse_response_view_v1, decode_base64, encode_base64,
};
use south_contracts::speech::{
    MAX_SPEECH_SEGMENTS, SpeechArtifactFormV1, SpeechArtifactV1, SpeechCallContextV1,
};
use token_station_protocol::ProviderConfig;

#[path = "support/speech_v3_1.rs"]
mod support;

const PACKAGE: &str = "speech-openai-compatible";
const REFERENCE: OpenAiCompatibleSpeechReferenceV1 = OpenAiCompatibleSpeechReferenceV1;
/// The events of the generated case (record §8 plan, item 3).
const GENERATED_EVENTS: usize = 4096;
/// The `parse-response` payload bound, `RuntimeLimitsV1::default().max_payload_bytes` (record §6);
/// the sandbox parity test pins the two equal.
const PARSE_RESPONSE_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "openai-compatible",
        "base_url": "https://api.openai.com",
        "auth": "provider_api_key",
        "models": [{ "model": "gpt-4o-mini-tts" }],
    }))
    .unwrap()
}

fn token_context() -> SpeechCallContextV1 {
    serde_json::from_value(json!({
        "upstream_model": "gpt-4o-mini-tts",
        "operation": "synthesize",
        "metering_required": ["tokens"],
    }))
    .unwrap()
}

/// One `data:` line per event, as `OpenAI` writes a token-priced answer.
fn stream(deltas: &[Vec<u8>], usage: &Value) -> String {
    let mut body = String::new();
    for delta in deltas {
        let event = json!({ "type": "speech.audio.delta", "audio": encode_base64(delta) });
        let _ = write!(body, "data: {event}\n\n");
    }
    let done = json!({ "type": "speech.audio.done", "usage": usage });
    let _ = write!(body, "data: {done}\n\n");
    body
}

#[test]
fn the_manifest_passes_gate_one() {
    assert_eq!(support::manifest(PACKAGE).validate(), Ok(()));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let manifest = support::manifest(PACKAGE);
    let report = run_speech_component_suite_v1(&REFERENCE, &support::pack(&manifest), &manifest);
    let failures = support::failures(&report);
    assert!(failures.is_empty(), "{failures:#?}");
    let mut rows = support::APPLICABLE_ROWS.to_vec();
    rows.push(CheckV1::MissingMeterIsNotZero);
    support::assert_rows_ran(&report, &rows);
}

/// Record §8 plan item 3: a generated 4,096-event answer (built here, not hand-written) passes the
/// suite with one segment per delta, in event order.
#[test]
fn a_generated_4096_event_stream_passes_the_suite() {
    let manifest = support::manifest(PACKAGE);
    let full = support::pack(&manifest);
    let deltas: Vec<Vec<u8>> =
        (0..GENERATED_EVENTS).map(|index| format!("frame-{index:04}").into_bytes()).collect();
    let body = stream(&deltas, &json!({ "input_tokens": 11, "output_tokens": 4096 }));
    let items: Vec<Value> = (0..GENERATED_EVENTS)
        .map(|index| json!({ "pointer": format!("/{index}/data/audio"), "encoding": "base64" }))
        .collect();
    let generated = SpeechCaseV1 {
        name: "speech-v1.response.generated-4096-events".to_owned(),
        family: SpeechFamilyV1::Response,
        input: json!({
            "prepare_case": "speech-v1.prepare.tokens-wav",
            "response": { "status": 200, "content_type": "text/event-stream", "body_text": body },
        }),
        expected: json!({
            "outcome": "succeeded",
            "artifact": { "form": "segments", "items": items, "media_type": "audio/wav" },
            "metering": {
                "characters": null,
                "seconds": null,
                "tokens": { "input": 11, "output": 4096, "input_text": null, "input_audio": null },
            },
        }),
        missing: None,
    };
    let mut cases: Vec<SpeechCaseV1> = full.cases().to_vec();
    cases.push(generated);
    let report = run_speech_component_suite_v1(
        &REFERENCE,
        &SpeechFixturePackV1::from_cases(cases),
        &manifest,
    );
    let failures = support::failures(&report);
    assert!(failures.is_empty(), "{failures:#?}");
    assert!(
        report
            .outcomes()
            .iter()
            .any(|outcome| outcome.case == "speech-v1.response.generated-4096-events"
                && outcome.check == CheckV1::FixtureMatch),
        "the generated case ran"
    );
}

/// The `sse` view of a stream as a host hands it to `parse-response` (its JSON text), parsed, and
/// its blobs.
fn sse_view(body: &str, declared: &[PathPatternV1]) -> (String, Value, Vec<BlobV1>) {
    let events: Vec<(String, String)> = body
        .split("\n\n")
        .filter(|chunk| !chunk.is_empty())
        .map(|chunk| ("message".to_owned(), chunk.strip_prefix("data: ").unwrap().to_owned()))
        .collect();
    let fields: Vec<SseEventFieldsV1<'_>> =
        events.iter().map(|(event, data)| SseEventFieldsV1 { event, data }).collect();
    let (view, blobs) = build_media_sse_response_view_v1(
        UpstreamRoundV1 { status: 200, headers: &[], body: body.as_bytes(), content_type: None },
        &fields,
        &[],
        declared,
        &MediaLimitsV1::V1,
    )
    .unwrap();
    let text = view.to_json();
    let value = serde_json::from_str(&text).unwrap();
    (text, value, blobs)
}

/// Record §7 item 11 (verify then settle), as a host does it: each `segments` pointer lands on a
/// placeholder whose blob decodes as base64, and the decoded fragments concatenate to the audio
/// (native `media_arms.rs:1054`: `WAV1` + `WAV2`).
#[test]
fn the_segments_decode_to_the_concatenated_audio() {
    let prepared = REFERENCE
        .prepare(
            &config(),
            &json!({ "json": { "input": "hello world", "response_format": "wav" } }),
            &token_context(),
        )
        .unwrap();
    let body = stream(
        &[b"WAV1".to_vec(), b"WAV2".to_vec()],
        &json!({ "input_tokens": 5, "output_tokens": 40 }),
    );
    let (_, view, blobs) = sse_view(&body, &prepared.response_elision_paths);
    let state: Value = serde_json::from_str(&prepared.state).unwrap();
    let SpeechOutcomeV1::Succeeded { delivery: SpeechDeliveryV1::Audio(artifact), .. } =
        REFERENCE.parse_response(&state, &view).unwrap()
    else {
        panic!("a succeeded audio outcome");
    };
    let SpeechArtifactFormV1::Segments { items } = artifact.form else { panic!("segments") };
    let mut audio = Vec::new();
    for item in items {
        let id = view["body"]["sse"].pointer(item.pointer.as_str()).unwrap()["$south.blob"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let blob = blobs.iter().find(|blob| blob.id().as_str() == id).unwrap();
        audio.extend(decode_base64(std::str::from_utf8(blob.bytes()).unwrap()).unwrap());
    }
    assert_eq!(audio, b"WAV1WAV2");
    assert_eq!(artifact.media_type, "audio/wav");
}

/// Record §8 (SV-Q6): at exactly `MAX_SPEECH_SEGMENTS` events the view fits the `parse-response`
/// payload and the outcome fits its frame; one more event is refused by the artifact codec, and the
/// reference does not build such an artifact.
#[test]
fn max_speech_segments_fits_the_view_and_the_outcome() {
    let prepared = REFERENCE
        .prepare(&config(), &json!({ "json": { "input": "hello world" } }), &token_context())
        .unwrap();
    // Each delta's base64 is longer than the 64-byte placeholder head, as real audio deltas are.
    let deltas = vec![vec![0x55_u8; 96]; MAX_SPEECH_SEGMENTS];
    let body = stream(&deltas, &json!({ "input_tokens": 11, "output_tokens": 65_536 }));
    let (text, mut view, _) = sse_view(&body, &prepared.response_elision_paths);
    let state: Value = serde_json::from_str(&prepared.state).unwrap();
    let payload = text.len() + prepared.state.len();
    assert!(payload <= PARSE_RESPONSE_PAYLOAD_BYTES, "the view is {payload} bytes");

    let outcome = REFERENCE.parse_response(&state, &view).unwrap();
    let frame = speech_outcome_json(&outcome).unwrap();
    assert!(frame.len() <= MAX_SPEECH_OUTCOME_JSON_BYTES, "the outcome is {} bytes", frame.len());
    let SpeechOutcomeV1::Succeeded { delivery: SpeechDeliveryV1::Audio(artifact), .. } =
        parse_speech_outcome_json(&frame).unwrap()
    else {
        panic!("a succeeded audio outcome");
    };
    let SpeechArtifactFormV1::Segments { items } = &artifact.form else { panic!("segments") };
    assert_eq!(items.len(), MAX_SPEECH_SEGMENTS);

    // One more fragment: the artifact codec refuses it.
    let mut wire: Value = serde_json::to_value(&artifact).unwrap();
    wire["items"].as_array_mut().unwrap().push(json!({
        "pointer": format!("/{MAX_SPEECH_SEGMENTS}/data/audio"),
        "encoding": "base64",
    }));
    assert!(serde_json::from_value::<SpeechArtifactV1>(wire).is_err());
    // And the reference answers such a stream `unknown` rather than build it.
    let extra = view["body"]["sse"][0].clone();
    let events = view["body"]["sse"].as_array_mut().unwrap();
    events.insert(0, extra);
    assert!(matches!(
        REFERENCE.parse_response(&state, &view).unwrap(),
        SpeechOutcomeV1::Unknown { .. }
    ));
}

#[test]
fn every_break_turns_its_row_red() {
    let manifest = support::manifest(PACKAGE);
    support::assert_every_break_bites(&REFERENCE, &support::pack(&manifest), &manifest);
}
