//! `south.speech-component.v1` against the `speech-elevenlabs` native reference and its shipped pack,
//! and the behaviours of design record `2026-10-14-speech-v3-1-tts.md` §5, §6 and SV-Q4 pinned by
//! name; recognition (design record `2026-10-14-speech-v3-3-asr.md` §4, §10, SV-Q8, SV-Q9): the
//! recognition rows and breaks, one answer rendered in every client format, and the SV-Q8 cue
//! rules one at a time.

use serde_json::{Value, json};
use south_component_conformance::reference_elevenlabs_speech::{
    ElevenLabsSpeechReferenceV1, declared_query_parameters,
};
use south_component_conformance::{
    CheckV1, ComponentResultV1, DeclaredInstancesV1, SpeechComponentV1, SpeechDeliveryV1,
    SpeechOutcomeV1, run_speech_component_suite_v1,
};
use south_contracts::media::{
    MediaDescriptorErrorV1, MediaLimitsV1, MediaRequestDescriptorV1, ResponseBodyFormV1,
    UpstreamRoundV1, build_media_response_view_v1,
};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechContractErrorV1, SpeechModelCapabilitiesV1,
    SpeechSecondsBasisV1, TranscriptDeliveryV1, parse_prepared_speech_call_v1,
    parse_prepared_speech_call_with_declared_v1,
};
use south_contracts::{DeclaredQueryParameterV1, QueryValueSyntaxV1};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

#[path = "support/speech_v3_3.rs"]
mod asr;
#[path = "support/speech_v3_1.rs"]
mod support;

const PACKAGE: &str = "speech-elevenlabs";
const REFERENCE: ElevenLabsSpeechReferenceV1 = ElevenLabsSpeechReferenceV1;

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "elevenlabs-speech",
        "base_url": "https://api.elevenlabs.io",
        "auth": "provider_api_key",
    }))
    .unwrap()
}

fn context() -> SpeechCallContextV1 {
    serde_json::from_value(json!({
        "upstream_model": "eleven_multilingual_v2",
        "operation": "synthesize",
        "metering_required": ["characters"],
    }))
    .unwrap()
}

fn prepare(voice: &Value) -> ComponentResultV1<PreparedSpeechCallV1> {
    REFERENCE.prepare(&config(), &json!({ "json": { "input": "hi", "voice": voice } }), &context())
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
    // `client_format_rendering` judges per case and reports only a failure here: no row carries
    // `speech.verbose_json`, so the pack-wide row does not run (the per-case check is proven by
    // the `TextAsJson` break below).
    let mut rows = support::APPLICABLE_ROWS.to_vec();
    rows.extend(
        asr::RECOGNITION_ROWS.into_iter().filter(|row| *row != CheckV1::ClientFormatRendering),
    );
    support::assert_rows_ran(&report, &rows);
    for (check, row) in [
        (CheckV1::MeteringSample, "speech-v1.metering.seconds"),
        (CheckV1::AbsentDurationIsNullOrUnknown, "speech-v1.absent.seconds"),
    ] {
        assert!(asr::passed_row(&report, check, row), "{row} did not pass");
    }
}

/// Record §6: the reference declares exactly what its manifest declares, so the host's
/// declarations (built from the manifest) and the reference's own are one set.
#[test]
fn the_reference_declares_what_the_manifest_declares() {
    let instances = DeclaredInstancesV1::from_manifest(&support::manifest(PACKAGE)).unwrap();
    assert_eq!(instances.query_parameters(), declared_query_parameters().unwrap().as_slice());
}

/// Record §6: the prepared call carries the declared parameter; a host parses it only with the
/// package's declarations, and the descriptor needs `contracts.media` 3.
#[test]
fn the_prepared_query_needs_the_declaration_and_media_3() {
    let prepared = prepare(&json!("alloy")).unwrap();
    assert_eq!(
        prepared.descriptor.query().map(south_contracts::QueryStringV1::as_str),
        Some("output_format=mp3_44100_128")
    );
    assert_eq!(prepared.descriptor.path().as_str(), "v1/text-to-speech/9BWtsMINqrJLrRacOk9x");
    assert_eq!(prepared.descriptor.minimum_contract_version(), 3);
    let text = prepared.to_json();
    assert_eq!(
        parse_prepared_speech_call_v1(&text, &MediaLimitsV1::V1),
        Err(SpeechContractErrorV1::Descriptor(MediaDescriptorErrorV1::UnsanctionedQuery))
    );
    let declared = declared_query_parameters().unwrap();
    assert_eq!(
        parse_prepared_speech_call_with_declared_v1(&text, &MediaLimitsV1::V1, &declared),
        Ok(prepared)
    );
}

/// SV-Q4: a resolved voice id outside `[A-Za-z0-9._~-]{1,128}`, or a dot segment, is refused before
/// dispatch with `invalid_request`; every admitted id is one path segment.
#[test]
fn voice_ids_outside_the_grammar_are_refused() {
    let (too_long, longest) = ("x".repeat(129), "x".repeat(128));
    for bad in ["a/b", "a?b", "a#b", "a&b", "a%2Fb", "a b", ".", "..", "\u{e9}", too_long.as_str()]
    {
        let error = prepare(&json!(bad)).expect_err(bad);
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{bad}");
        assert!(error.message.contains("voice_id"), "{bad}: {}", error.message);
    }
    for good in ["...", "a.b", "A-z_0~9", longest.as_str(), "pNInz6obpgDQGcFmaJgB"] {
        let prepared = prepare(&json!(good)).expect(good);
        assert_eq!(prepared.descriptor.path().as_str(), format!("v1/text-to-speech/{good}"));
    }
}

/// A component that writes a query name its package does not declare.
struct UndeclaredQueryName;

impl SpeechComponentV1 for UndeclaredQueryName {
    fn metadata(&self) -> ComponentMetadataV1 {
        REFERENCE.metadata()
    }
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<SpeechModelCapabilitiesV1>> {
        REFERENCE.model_capabilities(config)
    }
    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        let prepared = REFERENCE.prepare(config, request, context)?;
        let mut descriptor: Value = serde_json::from_str(&prepared.descriptor.to_json()).unwrap();
        // A transcription carries no query (V3-3 record §4).
        if descriptor.get("query").is_none() {
            return Ok(prepared);
        }
        descriptor["query"][0]["name"] = json!("format");
        let elsewhere = DeclaredQueryParameterV1::try_new("format", QueryValueSyntaxV1::Token)
            .expect("a valid declaration");
        Ok(PreparedSpeechCallV1 {
            descriptor: MediaRequestDescriptorV1::parse_with_declared(
                &descriptor.to_string(),
                &MediaLimitsV1::V1,
                &[elsewhere],
            )
            .unwrap(),
            ..prepared
        })
    }
    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        REFERENCE.parse_response(state, response)
    }
}

/// Record §6 conformance: a mutation that writes an undeclared name turns the pack red, on the row
/// that judges what a host admits.
#[test]
fn an_undeclared_query_name_turns_the_pack_red() {
    let manifest = support::manifest(PACKAGE);
    let report =
        run_speech_component_suite_v1(&UndeclaredQueryName, &support::pack(&manifest), &manifest);
    assert!(
        report.failures().any(|outcome| outcome.check == CheckV1::EndpointConfinement),
        "{report}"
    );
}

#[test]
fn every_break_turns_its_row_red() {
    let manifest = support::manifest(PACKAGE);
    support::assert_every_break_bites(&REFERENCE, &support::pack(&manifest), &manifest);
}

#[test]
fn every_recognition_break_turns_its_row_red() {
    let manifest = support::manifest(PACKAGE);
    asr::assert_every_asr_break_bites(&REFERENCE, &support::pack(&manifest), &manifest);
}

fn transcription(format: &str) -> PreparedSpeechCallV1 {
    let config: ProviderConfig = serde_json::from_value(json!({
        "provider": "elevenlabs-speech",
        "base_url": "https://api.elevenlabs.io",
        "auth": "provider_api_key",
        "models": [{ "model": "scribe_v1" }],
    }))
    .unwrap();
    let context: SpeechCallContextV1 = serde_json::from_value(json!({
        "upstream_model": "scribe_v1",
        "operation": "transcribe",
        "metering_required": ["seconds"],
    }))
    .unwrap();
    let request = json!({ "multipart": { "parts": [
        { "kind": "file", "name": "file", "blob": "b0", "filename": "a.mp3", "bytes": 9 },
        { "kind": "text", "name": "response_format", "value": format },
    ] } });
    REFERENCE.prepare(&config, &request, &context).unwrap()
}

/// The outcome of one transcription in `format` answered with `body`.
fn transcribe(format: &str, body: &Value) -> SpeechOutcomeV1 {
    let prepared = transcription(format);
    let bytes = serde_json::to_vec(body).unwrap();
    let (view, _) = build_media_response_view_v1(
        UpstreamRoundV1 { status: 200, headers: &[], body: &bytes, content_type: None },
        &[],
        prepared.response_body_form,
        &prepared.response_elision_paths,
        &MediaLimitsV1::V1,
    )
    .unwrap();
    let view: Value = serde_json::from_str(&view.to_json()).unwrap();
    let state: Value = serde_json::from_str(&prepared.state).unwrap();
    REFERENCE.parse_response(&state, &view).unwrap()
}

/// The rendered `srt` document of `words`.
fn srt(words: &Value) -> String {
    match transcribe("srt", &json!({ "text": "", "words": words })) {
        SpeechOutcomeV1::Succeeded {
            delivery: SpeechDeliveryV1::Transcript(TranscriptDeliveryV1::Text { text, .. }),
            ..
        } => text,
        other => panic!("a rendered document, got {other:?}"),
    }
}

fn word(text: &str, start: f64, end: f64) -> Value {
    json!({ "text": text, "start": start, "end": end, "type": "word" })
}

fn spacing() -> Value {
    json!({ "text": " ", "type": "spacing" })
}

/// Record §4 (the `client_format_rendering` property, judged here since no `ElevenLabs` row carries
/// `speech.verbose_json`): one answer renders in every client format, each the document kind the
/// format names, all with the same `last_timestamp` seconds; the same upstream request serves them
/// all apart from the forwarded `response_format` part.
#[test]
fn one_answer_renders_in_every_client_format_with_the_same_seconds() {
    let body = json!({
        "language_code": "en",
        "text": "hi there",
        "words": [word("hi", 0.0, 0.4), spacing(), word("there", 0.5, 1.25)],
    });
    for format in ["json", "text", "srt", "vtt", "verbose_json"] {
        let SpeechOutcomeV1::Succeeded {
            delivery: SpeechDeliveryV1::Transcript(delivery),
            metering,
        } = transcribe(format, &body)
        else {
            panic!("{format}: a succeeded transcript");
        };
        let seconds = metering.seconds.expect("seconds");
        assert_eq!(seconds.basis, SpeechSecondsBasisV1::LastTimestamp, "{format}");
        assert!((seconds.value - 1.25).abs() < 1e-9, "{format}: {}", seconds.value);
        let json = matches!(delivery, TranscriptDeliveryV1::Json { .. });
        assert_eq!(json, matches!(format, "json" | "verbose_json"), "{format}");
        let prepared = transcription(format);
        assert_eq!(prepared.response_body_form, ResponseBodyFormV1::Json, "{format}");
    }
}

/// SV-Q8, one rule at a time.
#[test]
fn subtitle_cues_follow_sv_q8() {
    // Sentence punctuation, ASCII and full-width, closes a cue after the word.
    assert_eq!(
        srt(&json!([word("Hi!", 0.0, 0.3), spacing(), word("Yes", 0.4, 0.6)])),
        "1\n00:00:00,000 --> 00:00:00,300\nHi!\n\n2\n00:00:00,400 --> 00:00:00,600\nYes\n\n"
    );
    assert_eq!(
        srt(&json!([word("Hi\u{FF1F}", 0.0, 0.3), word("Yes", 0.4, 0.6)])),
        "1\n00:00:00,000 --> 00:00:00,300\nHi\u{FF1F}\n\n2\n00:00:00,400 --> 00:00:00,600\nYes\n\n"
    );
    // A gap of exactly 1 s closes; just under does not.
    assert_eq!(
        srt(&json!([word("a", 0.0, 0.5), spacing(), word("b", 1.5, 1.75)])),
        "1\n00:00:00,000 --> 00:00:00,500\na\n\n2\n00:00:01,500 --> 00:00:01,750\nb\n\n"
    );
    assert_eq!(
        srt(&json!([word("a", 0.0, 0.5), spacing(), word("b", 1.25, 1.75)])),
        "1\n00:00:00,000 --> 00:00:01,750\na b\n\n"
    );
    // A speaker change closes; the same speaker does not.
    let spoken = |speaker: &str, text: &str, start: f64, end: f64| json!({ "text": text, "start": start, "end": end, "type": "word", "speaker_id": speaker });
    assert_eq!(
        srt(&json!([spoken("s0", "a", 0.0, 0.2), spacing(), spoken("s1", "b", 0.3, 0.5)])),
        "1\n00:00:00,000 --> 00:00:00,200\na\n\n2\n00:00:00,300 --> 00:00:00,500\nb\n\n"
    );
    // A cue may last exactly 7 s; a word that would take it past closes it first.
    assert_eq!(
        srt(&json!([word("a", 0.0, 3.0), spacing(), word("b", 3.5, 7.0)])),
        "1\n00:00:00,000 --> 00:00:07,000\na b\n\n"
    );
    assert_eq!(
        srt(&json!([word("a", 0.0, 3.0), spacing(), word("b", 3.5, 7.25)])),
        "1\n00:00:00,000 --> 00:00:03,000\na\n\n2\n00:00:03,500 --> 00:00:07,250\nb\n\n"
    );
    // A cue may hold exactly 84 characters; one more closes it first.
    let (forty_one, forty_two) = ("x".repeat(41), "y".repeat(42));
    assert_eq!(
        srt(&json!([word(&forty_one, 0.0, 0.5), spacing(), word(&forty_two, 0.6, 1.0)])),
        format!("1\n00:00:00,000 --> 00:00:01,000\n{forty_one} {forty_two}\n\n")
    );
    let forty_three = "z".repeat(43);
    assert_eq!(
        srt(&json!([word(&forty_one, 0.0, 0.5), spacing(), word(&forty_three, 0.6, 1.0)])),
        format!(
            "1\n00:00:00,000 --> 00:00:00,500\n{forty_one}\n\n\
             2\n00:00:00,600 --> 00:00:01,000\n{forty_three}\n\n"
        )
    );
    // Audio events are skipped; spacing between cues is dropped.
    assert_eq!(
        srt(&json!([
            word("a.", 0.0, 0.2),
            spacing(),
            { "text": "(music)", "start": 0.2, "end": 0.4, "type": "audio_event" },
            word("b", 0.5, 0.6),
        ])),
        "1\n00:00:00,000 --> 00:00:00,200\na.\n\n2\n00:00:00,500 --> 00:00:00,600\nb\n\n"
    );
}
