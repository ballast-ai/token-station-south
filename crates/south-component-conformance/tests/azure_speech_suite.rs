//! `south.speech-component.v1` against the `speech-azure` native reference and its shipped pack, and
//! the behaviours of design record `2026-10-14-speech-v3-3-asr.md` §5 pinned by name: native's
//! `definition` bytes, the sanctioned `api-version` query on media contract 1, and the subtitle and
//! streaming refusals; and of `2026-10-15-speech-v4-tts.md` §4 (SSML synthesis): native's SSML
//! template and escaper, the output-format header, the 1 MiB text body bound and the format table.

use serde_json::{Value, json};
use south_component_conformance::reference_azure_speech::{
    AzureSpeechReferenceV1, definition, ssml, voice_id, xml_escape,
};
use south_component_conformance::{
    CheckV1, ComponentResultV1, SpeechComponentV1, SpeechDeliveryV1, SpeechOutcomeV1,
    run_speech_component_suite_v1,
};
use south_contracts::QueryStringV1;
use south_contracts::media::{MAX_MEDIA_TEXT_BODY_BYTES, MediaBodyV1, MediaPartV1};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechArtifactFormV1, SpeechCallContextV1, SpeechModelCapabilitiesV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

#[path = "support/speech_v3_3.rs"]
mod asr;
#[path = "support/speech_v3_1.rs"]
mod support;

const PACKAGE: &str = "speech-azure";
const REFERENCE: AzureSpeechReferenceV1 = AzureSpeechReferenceV1;

/// The rows the package runs: the image world's carried-over rows, the recognition rows
/// (`client_format_rendering` judges per case and reports only a failure, since no Azure row
/// carries `speech.verbose_json`; the `TextAsJson` break proves it bites) and, from batch V4, the
/// synthesis rows `ssml_escaping` and `empty_audio_is_unknown`.
const ROWS: [CheckV1; 16] = [
    CheckV1::Coverage,
    CheckV1::FixtureMatch,
    CheckV1::Determinism,
    CheckV1::UnknownFieldTolerance,
    CheckV1::UndeclaredValuesIgnored,
    CheckV1::DescriptorAuthWithinManifest,
    CheckV1::EndpointConfinement,
    CheckV1::TerminalOnlyFromTheWire,
    CheckV1::AuthErrorsAreNotRetriable,
    CheckV1::PreDispatchRefusal,
    CheckV1::ReferenceIntegrity,
    CheckV1::MeteringSample,
    CheckV1::UndeclaredOperationRefused,
    CheckV1::AbsentDurationIsNullOrUnknown,
    CheckV1::SsmlEscaping,
    CheckV1::EmptyAudioIsUnknown,
];

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "azure-speech",
        "base_url": "https://eastus.cognitiveservices.azure.com",
        "auth": "provider_api_key",
        "models": [{ "model": "mai-transcribe-1.5" }],
    }))
    .unwrap()
}

fn context(model: &str) -> SpeechCallContextV1 {
    serde_json::from_value(json!({
        "upstream_model": model,
        "operation": "transcribe",
        "metering_required": ["seconds"],
    }))
    .unwrap()
}

fn request(extra: &[(&str, &str)]) -> Value {
    let mut parts = vec![
        json!({ "kind": "file", "name": "file", "blob": "b0", "filename": "a.wav", "bytes": 9 }),
    ];
    for (name, value) in extra {
        parts.push(json!({ "kind": "text", "name": name, "value": value }));
    }
    json!({ "multipart": { "parts": parts } })
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
    support::assert_rows_ran(&report, &ROWS);
    for (check, row) in [
        (CheckV1::MeteringSample, "speech-v1.metering.seconds"),
        (CheckV1::AbsentDurationIsNullOrUnknown, "speech-v1.absent.seconds"),
        (CheckV1::UndeclaredOperationRefused, "speech-v1.undeclared-operation"),
        (CheckV1::MeteringSample, "speech-v1.metering.characters"),
        (CheckV1::SsmlEscaping, "speech-v1.ssml.special-characters"),
        (CheckV1::EmptyAudioIsUnknown, "speech-v1.empty-audio.binary"),
    ] {
        assert!(asr::passed_row(&report, check, row), "{row} did not pass");
    }
}

#[test]
fn every_synthesis_break_turns_its_row_red() {
    let manifest = support::manifest(PACKAGE);
    support::assert_every_break_bites(&REFERENCE, &support::pack(&manifest), &manifest);
}

#[test]
fn every_recognition_break_turns_its_row_red() {
    let manifest = support::manifest(PACKAGE);
    asr::assert_every_asr_break_bites(&REFERENCE, &support::pack(&manifest), &manifest);
}

/// Record §5: the `definition` part carries native's bytes (`multipart.rs:315-318`, built with
/// `serde_json`), whatever the upstream id holds; the dual run compares part values as strings.
#[test]
fn the_definition_part_is_natives_bytes() {
    for id in [
        "mai-transcribe-1.5",
        "a\"b",
        "a\\b",
        "line\nbreak\ttab\u{1}",
        "</script>",
        "\u{c548}\u{b155}",
        "",
    ] {
        let native = json!({ "enhancedMode": { "enabled": true, "model": id } }).to_string();
        assert_eq!(definition(id), native, "{id:?}");
        let prepared = REFERENCE.prepare(&config(), &request(&[]), &context(id)).unwrap();
        let MediaBodyV1::Multipart(parts) = prepared.descriptor.body() else { panic!("multipart") };
        let Some(MediaPartV1::Text { name, value }) = parts.last() else { panic!("a text part") };
        assert_eq!((name.as_str(), value.as_str()), ("definition", native.as_str()), "{id:?}");
    }
}

/// Record §5: the `api-version` query is a sanctioned parameter, so the package needs only media
/// contract 1 and declares no query parameter.
#[test]
fn the_api_version_query_is_sanctioned() {
    let prepared =
        REFERENCE.prepare(&config(), &request(&[]), &context("mai-transcribe-1.5")).unwrap();
    assert_eq!(
        prepared.descriptor.query().map(QueryStringV1::as_str),
        Some("api-version=2025-10-15")
    );
    assert_eq!(prepared.descriptor.minimum_contract_version(), 1);
    assert!(support::manifest(PACKAGE).query_parameters.is_empty());
}

/// Record §5 and §1: subtitles are refused before dispatch with native's text whatever the padding,
/// and a streaming request before the format is read.
#[test]
fn subtitles_and_streaming_are_refused_before_dispatch() {
    for format in ["srt", "vtt", " srt", "vtt\t", "\nsrt\n"] {
        let error = REFERENCE
            .prepare(&config(), &request(&[("response_format", format)]), &context("m"))
            .expect_err(format);
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{format:?}");
        assert!(
            error
                .message
                .starts_with(&format!("response_format '{}' is not supported", format.trim())),
            "{format:?}: {}",
            error.message
        );
    }
    for stream in ["true", "TRUE", " true ", "1"] {
        let error = REFERENCE
            .prepare(
                &config(),
                &request(&[("stream", stream), ("response_format", "srt")]),
                &context("m"),
            )
            .expect_err(stream);
        assert!(
            error.message.starts_with("streaming transcription is not supported"),
            "{stream:?}"
        );
    }
    for stream in ["false", "0", "yes"] {
        assert!(
            REFERENCE.prepare(&config(), &request(&[("stream", stream)]), &context("m")).is_ok(),
            "{stream:?} is forwarded"
        );
    }
}

fn synthesis_config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "azure-speech",
        "base_url": "https://eastus.cognitiveservices.azure.com",
        "auth": "provider_api_key",
        "models": [{ "model": "MAI-Voice-2" }],
    }))
    .unwrap()
}

fn synthesis_context(model: &str) -> SpeechCallContextV1 {
    serde_json::from_value(json!({
        "upstream_model": model,
        "operation": "synthesize",
        "metering_required": ["characters"],
    }))
    .unwrap()
}

fn synthesize(request: &Value) -> ComponentResultV1<PreparedSpeechCallV1> {
    REFERENCE.prepare(
        &synthesis_config(),
        &json!({ "json": request }),
        &synthesis_context("MAI-Voice-2"),
    )
}

/// The SSML the descriptor carries.
fn ssml_of(prepared: &PreparedSpeechCallV1) -> String {
    let MediaBodyV1::Text(body) = prepared.descriptor.body() else { panic!("a text body") };
    assert_eq!(body.media_type, "application/ssml+xml");
    body.text.clone()
}

/// Record §4.2: native's `build_mai_ssml` template, exactly, with no declaration and no `xmlns`
/// (§8 item 8), and native's `xml_escape` (azure_tts_tests.rs:100-122).
#[test]
fn the_ssml_is_natives_template_and_escaping() {
    assert_eq!(
        ssml("hi", "en-US-June:MAI-Voice-2"),
        "<speak version='1.0' xml:lang='en-US'><voice name='en-US-June:MAI-Voice-2'>hi</voice></speak>"
    );
    assert_eq!(
        xml_escape("Tom & \"Jerry\" <x> it's"),
        "Tom &amp; &quot;Jerry&quot; &lt;x&gt; it&apos;s"
    );
    assert_eq!(xml_escape("alert\u{7}done\u{0}!"), "alertdone!");
    assert_eq!(xml_escape("line1\nline2\tend\r"), "line1\nline2\tend\r");
    let dropped: String = ('\u{0}'..='\u{1f}')
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .chain(['\u{fffe}', '\u{ffff}'])
        .collect();
    assert_eq!(xml_escape(&dropped), "");
    assert_eq!(xml_escape("\u{fffd}\u{10000}"), "\u{fffd}\u{10000}");
    let prepared =
        synthesize(&json!({ "input": "bonjour", "voice": "fr-FR-Vivienne:MAI-Voice-2" })).unwrap();
    assert_eq!(
        ssml_of(&prepared),
        "<speak version='1.0' xml:lang='fr-FR'><voice name='fr-FR-Vivienne:MAI-Voice-2'>bonjour</voice></speak>"
    );
}

/// Record §4.2: the voice rules of `map_openai_voice_to_mai` (azure_tts_tests.rs:7-99).
#[test]
fn the_voice_is_natives_mapping() {
    for (voice, model, id) in [
        ("alloy", "MAI-Voice-2", "en-US-June:MAI-Voice-2"),
        ("onyx", "MAI-Voice-2", "en-US-Reed:MAI-Voice-2"),
        ("coral", "MAI-Voice-2", "en-US-Iris:MAI-Voice-2"),
        ("sage", "MAI-Voice-2", "en-US-June:MAI-Voice-2"),
        ("ash", "MAI-Voice-2", "en-US-Grant:MAI-Voice-2"),
        ("June", "MAI-Voice-2", "en-US-June:MAI-Voice-2"),
        ("Alloy", "MAI-Voice-2", "en-US-Alloy:MAI-Voice-2"),
        ("en-US-June:MAI-Voice-2", "MAI-Voice-2", "en-US-June:MAI-Voice-2"),
        ("fr-FR-Vivienne:MAI-Voice-2", "MAI-Voice-2", "fr-FR-Vivienne:MAI-Voice-2"),
        ("alloy", "MAI-Voice-3", "en-US-June:MAI-Voice-3"),
        ("en-GB-Sonia", "MAI-Voice-2", "en-GB-Sonia:MAI-Voice-2"),
        ("zh-CN-liaoning-Xiaobei", "MAI-Voice-2", "zh-CN-liaoning-Xiaobei:MAI-Voice-2"),
        ("Mary-Jane-Extra", "MAI-Voice-2", "en-US-Mary-Jane-Extra:MAI-Voice-2"),
        ("EN-gb-Sonia", "MAI-Voice-2", "en-US-EN-gb-Sonia:MAI-Voice-2"),
    ] {
        assert_eq!(voice_id(voice, model), id, "{voice}");
    }
}

/// Record §4.2 (P1): an SSML document above `MAX_TEXT_REQUEST_BODY_BYTES` is refused before
/// dispatch; one exactly at the bound is sent.
#[test]
fn the_ssml_is_bounded_by_the_text_body_limit() {
    let wrap = ssml("", "en-US-June:MAI-Voice-2").len();
    let at_bound = "a".repeat(MAX_MEDIA_TEXT_BODY_BYTES - wrap);
    let prepared = synthesize(&json!({ "input": at_bound })).expect("a document at the bound");
    assert_eq!(ssml_of(&prepared).len(), MAX_MEDIA_TEXT_BODY_BYTES);
    // Escaping grows `&` to five bytes: a fifth of the bound in input passes it.
    let above = "&".repeat((MAX_MEDIA_TEXT_BODY_BYTES - wrap) / 5 + 1);
    assert!(above.len() < MAX_MEDIA_TEXT_BODY_BYTES / 4);
    let error = synthesize(&json!({ "input": above })).expect_err("above the bound");
    assert_eq!(
        (error.code, error.message.as_str()),
        (ErrorCode::InvalidRequest, "'input' is longer than the gateway accepts")
    );
    let error = synthesize(&json!({ "input": format!("{at_bound}b") })).expect_err("one byte over");
    assert_eq!(error.code, ErrorCode::InvalidRequest);
}

/// Record §4.2 and §4.3: the output format header from the table, matched exactly; the delivered
/// media type is the table's whatever the upstream says; no body path accepts request extras.
#[test]
fn the_format_table_sets_the_header_and_the_label() {
    for (format, token, media_type) in [
        (None, "audio-24khz-48kbitrate-mono-mp3", "audio/mpeg"),
        (Some("mp3"), "audio-24khz-48kbitrate-mono-mp3", "audio/mpeg"),
        (Some("opus"), "ogg-24khz-16bit-mono-opus", "audio/ogg"),
        (Some("wav"), "riff-24khz-16bit-mono-pcm", "audio/wav"),
        (Some("pcm"), "raw-24khz-16bit-mono-pcm", "audio/pcm"),
    ] {
        let mut request = json!({ "input": "hi" });
        if let Some(format) = format {
            request["response_format"] = json!(format);
        }
        let prepared = synthesize(&request).unwrap();
        let headers: Vec<(&str, &str)> = prepared.descriptor.headers().iter().collect();
        assert_eq!(headers, [("x-microsoft-outputformat", token)], "{format:?}");
        assert_eq!(prepared.immutable_body_paths, None, "{format:?}");
        let state: Value = serde_json::from_str(&prepared.state).unwrap();
        let view = json!({
            "status": 200,
            "headers": [["content-type", "audio/mpeg"]],
            "body": { "opaque": { "bytes": 3 } },
        });
        let SpeechOutcomeV1::Succeeded { delivery: SpeechDeliveryV1::Audio(artifact), .. } =
            REFERENCE.parse_response(&state, &view).unwrap()
        else {
            panic!("{format:?}: a delivery")
        };
        assert_eq!(artifact.form, SpeechArtifactFormV1::Body);
        assert_eq!(artifact.media_type, media_type, "{format:?}");
    }
    for format in ["MP3", "Wav", "flac", "aac", "", " mp3"] {
        let error =
            synthesize(&json!({ "input": "hi", "response_format": format })).expect_err(format);
        assert_eq!(
            error.message,
            format!(
                "response_format '{format}' is not supported for MAI-Voice-2 (Azure Speech); use \
                 mp3, opus, wav, or pcm"
            )
        );
    }
}

/// An Azure-synthesis-specific way to break the reference.
#[derive(Clone, Copy, Debug)]
enum Mutation {
    /// The 1 MiB refusal dropped: an oversized document is cut to the bound and sent.
    NoSsmlBound,
    /// The voice sent unescaped inside its attribute.
    UnescapedVoice,
    /// A forbidden character embedded as a numeric reference instead of dropped.
    ReferenceForbiddenCharacters,
    /// The delivered media type taken from the upstream's `content-type`.
    EchoContentType,
}

struct Mutant(Mutation);

impl SpeechComponentV1 for Mutant {
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
        let prepared = match (self.0, REFERENCE.prepare(config, request, context)) {
            (Mutation::NoSsmlBound, Err(error)) if error.message.starts_with("'input'") => {
                if request.pointer("/json/input").and_then(Value::as_str).is_none() {
                    return Err(error);
                }
                let mut shorter = request.clone();
                shorter["json"]["input"] = json!("short");
                return REFERENCE.prepare(config, &shorter, context);
            }
            (_, result) => result?,
        };
        let Some(input) = request.pointer("/json/input").and_then(Value::as_str) else {
            return Ok(prepared);
        };
        let MediaBodyV1::Text(_) = prepared.descriptor.body() else { return Ok(prepared) };
        let voice = request.pointer("/json/voice").and_then(Value::as_str).unwrap_or("alloy");
        let id = voice_id(voice.trim(), &context.upstream_model);
        let document = match self.0 {
            Mutation::UnescapedVoice => ssml(input, &id).replace(&xml_escape(&id), &id),
            Mutation::ReferenceForbiddenCharacters => {
                let referenced: String = input
                    .chars()
                    .map(|c| {
                        if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') {
                            format!("&#{};", c as u32)
                        } else {
                            xml_escape(&c.to_string())
                        }
                    })
                    .collect();
                ssml("", &id).replace("></voice>", &format!(">{referenced}</voice>"))
            }
            _ => return Ok(prepared),
        };
        let mut wire: Value = serde_json::from_str(&prepared.to_json()).unwrap();
        wire["descriptor"]["body"]["text"]["text"] = json!(document);
        south_contracts::speech::parse_prepared_speech_call_v1(
            &wire.to_string(),
            &south_contracts::media::MediaLimitsV1::V1,
        )
        .map_err(|error| ErrorEnvelope::new(ErrorCode::Internal, 500, error.to_string()))
    }
    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        let outcome = REFERENCE.parse_response(state, response)?;
        Ok(match (self.0, outcome) {
            (
                Mutation::EchoContentType,
                SpeechOutcomeV1::Succeeded {
                    delivery: SpeechDeliveryV1::Audio(mut artifact),
                    metering,
                },
            ) => {
                if let Some(content_type) = response["headers"]
                    .as_array()
                    .and_then(|headers| headers.iter().find(|pair| pair[0] == "content-type"))
                    .and_then(|pair| pair[1].as_str())
                {
                    content_type.clone_into(&mut artifact.media_type);
                }
                SpeechOutcomeV1::Succeeded { delivery: SpeechDeliveryV1::Audio(artifact), metering }
            }
            (_, outcome) => outcome,
        })
    }
}

/// Each Azure synthesis mutation turns the shipped pack red on the cases that pin it.
#[test]
fn every_azure_synthesis_mutation_turns_the_pack_red() {
    let manifest = support::manifest(PACKAGE);
    let pack = support::pack(&manifest);
    for (mutation, cases) in [
        (Mutation::NoSsmlBound, &["speech-v1.prepare.tts-ssml-above-the-text-body-bound"][..]),
        (
            Mutation::UnescapedVoice,
            &["speech-v1.prepare.tts-ssml-special-characters", "speech-v1.ssml.special-characters"]
                [..],
        ),
        (
            Mutation::ReferenceForbiddenCharacters,
            &[
                "speech-v1.prepare.tts-control-characters-dropped",
                "speech-v1.prepare.tts-every-forbidden-character-dropped",
            ][..],
        ),
        (Mutation::EchoContentType, &["speech-v1.response.tts-opus-relabelled"][..]),
    ] {
        let report = run_speech_component_suite_v1(&Mutant(mutation), &pack, &manifest);
        let failed: Vec<String> = report.failures().map(|outcome| outcome.case.clone()).collect();
        for case in cases {
            assert!(failed.iter().any(|name| name == case), "{mutation:?}: {case} stayed green");
        }
    }
}
