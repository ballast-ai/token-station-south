//! `south.speech-component.v1` against the `speech-qwen` native reference and its shipped pack, and
//! the behaviours of design record `2026-10-15-speech-v4-tts.md` §2 pinned by name: the `url`
//! artifact with SV-Q10's upgrade, the weighted `upper`, SV-Q7's character rule, the 2xx `code`,
//! the `DashScope` error reader shared with `image-qwen`, and the `instruct` model class; and of
//! `2026-10-21-speech-qwen-asr.md` (Qwen-ASR transcription, 1.1.0): the data URL the host expands,
//! the audio media type, the 10 MB bound, the client formats, and seconds read from `usage.seconds`
//! with no fallback.

use serde_json::{Value, json};
use south_component_conformance::reference_qwen_speech::QwenSpeechReferenceV1;
use south_component_conformance::{
    CheckV1, ComponentResultV1, SpeechComponentV1, SpeechDeliveryV1, SpeechOutcomeV1,
    run_speech_component_suite_v1,
};
use south_contracts::media::{ArtifactUrlV1, MediaBodyV1};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechArtifactFormV1, SpeechArtifactV1, SpeechCallContextV1,
    SpeechCharacterMeterV1, SpeechCharacterSourceV1, SpeechMeteringV1, SpeechModelCapabilitiesV1,
    SpeechSecondsBasisV1, SpeechSecondsMeterV1, TranscriptDeliveryV1,
    upgrade_insecure_artifact_url_v1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

#[path = "support/speech_v3_3.rs"]
mod asr;
#[path = "support/speech_v3_1.rs"]
mod support;

const PACKAGE: &str = "speech-qwen";
const REFERENCE: QwenSpeechReferenceV1 = QwenSpeechReferenceV1;
const OSS_HTTP: &str = "http://dashscope-result-bj.oss-cn-beijing.aliyuncs.com/1d/ab/out.wav?Expires=1&OSSAccessKeyId=k&Signature=s%2B";

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "bailian-qwen-speech",
        "base_url": "https://dashscope.aliyuncs.com",
        "auth": "provider_api_key",
        "models": [{ "model": "qwen3-tts-flash" }, { "model": "qwen3-tts-instruct-flash" }],
    }))
    .unwrap()
}

fn context(model: &str) -> SpeechCallContextV1 {
    serde_json::from_value(json!({
        "upstream_model": model,
        "operation": "synthesize",
        "metering_required": ["characters"],
    }))
    .unwrap()
}

/// The prepared call as JSON, or the refusal as JSON.
fn prepare(model: &str, request: &Value) -> Result<Value, Value> {
    REFERENCE
        .prepare(&config(), &json!({ "json": request }), &context(model))
        .map(|prepared| serde_json::from_str(&prepared.to_json()).unwrap())
        .map_err(|error| serde_json::to_value(error).unwrap())
}

/// The upstream body `prepare` builds for this client body.
fn upstream_body(model: &str, request: &Value) -> Value {
    let prepared =
        REFERENCE.prepare(&config(), &json!({ "json": request }), &context(model)).unwrap();
    let MediaBodyV1::Json(template) = prepared.descriptor.body() else { panic!("a JSON body") };
    serde_json::from_str(template).unwrap()
}

/// The outcome of a view with this status and JSON body, for a call whose input is `input`.
fn answer(input: &str, status: u16, body: &Value) -> SpeechOutcomeV1 {
    let prepared = REFERENCE
        .prepare(&config(), &json!({ "json": { "input": input } }), &context("qwen3-tts-flash"))
        .unwrap();
    let state: Value = serde_json::from_str(&prepared.state).unwrap();
    let view = json!({ "status": status, "headers": [], "body": { "json": body } });
    REFERENCE.parse_response(&state, &view).unwrap()
}

fn delivered(url: &str, characters: &Value) -> Value {
    json!({ "output": { "audio": { "url": url } }, "usage": { "characters": characters } })
}

/// The rows the Qwen pack must show: V3-1's synthesis rows, the second hop and, from 1.1.0, the
/// missing duration of a transcription.
fn rows() -> Vec<CheckV1> {
    let mut rows = support::APPLICABLE_ROWS.to_vec();
    rows.push(CheckV1::SecondHopArtifact);
    rows.push(CheckV1::AbsentDurationIsNullOrUnknown);
    rows
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
    support::assert_rows_ran(&report, &rows());
    for row in [
        "speech-v1.second-hop",
        "speech-v1.metering.characters.request",
        "speech-v1.metering.seconds",
        "speech-v1.absent.seconds",
    ] {
        assert!(
            report.outcomes().iter().any(|outcome| outcome.case == row && !outcome.is_failure()),
            "{row} did not pass"
        );
    }
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

/// Record §2.2 step 4 and SV-Q10: the audio is a `url` artifact at `/output/audio/url` that asks
/// for the upgrade; the upstream's `http://` OSS link becomes an https URL the grammar admits.
#[test]
fn the_audio_is_an_upgradable_url_artifact() {
    let SpeechOutcomeV1::Succeeded { delivery: SpeechDeliveryV1::Audio(artifact), metering } =
        answer("hello world", 200, &delivered(OSS_HTTP, &json!(7)))
    else {
        panic!("a delivery")
    };
    let SpeechArtifactFormV1::Url { pointer, upgrade_insecure } = &artifact.form else {
        panic!("a url artifact")
    };
    assert_eq!(pointer.as_str(), "/output/audio/url");
    assert!(*upgrade_insecure);
    assert_eq!(artifact.media_type, "audio/wav");
    assert_eq!(artifact.container, None);
    assert_eq!(
        metering.characters,
        Some(SpeechCharacterMeterV1 { value: 7, source: SpeechCharacterSourceV1::Upstream })
    );
    assert!(ArtifactUrlV1::parse(OSS_HTTP).is_err(), "the raw link is refused");
    let upgraded = upgrade_insecure_artifact_url_v1(OSS_HTTP);
    assert!(upgraded.starts_with("https://dashscope-result-bj.oss-cn-beijing.aliyuncs.com/"));
    assert!(ArtifactUrlV1::parse(&upgraded).is_ok());
}

/// Record §2.3: `upper` is the weighted count (ASCII 1, any other scalar 2).
#[test]
fn the_upper_is_the_weighted_count() {
    for (input, count, upper) in [
        ("hello world", 11, 11),
        ("\u{c548}\u{b155}\u{d558}\u{c138}\u{c694}", 5, 10),
        ("hi \u{c548}\u{b155}", 5, 7),
    ] {
        let prepared = prepare("qwen3-tts-flash", &json!({ "input": input })).unwrap();
        assert_eq!(prepared["facts"]["characters"], json!({ "count": count, "upper": upper }));
    }
}

/// SV-Q7: a positive integer is the upstream's count, absent or `null` the request's, `0` or any
/// other value `unknown`; the characters are judged before the URL.
#[test]
fn upstream_characters_follow_sv_q7() {
    let characters = |outcome: SpeechOutcomeV1| match outcome {
        SpeechOutcomeV1::Succeeded { metering, .. } => metering.characters.expect("characters"),
        other => panic!("not a delivery: {other:?}"),
    };
    assert_eq!(
        characters(answer("hi", 200, &delivered(OSS_HTTP, &json!(3)))),
        SpeechCharacterMeterV1 { value: 3, source: SpeechCharacterSourceV1::Upstream }
    );
    assert_eq!(
        characters(answer("hi", 200, &delivered(OSS_HTTP, &Value::Null))),
        SpeechCharacterMeterV1 { value: 2, source: SpeechCharacterSourceV1::Request }
    );
    for value in [json!(0), json!(-1), json!(2.5), json!(2.0), json!("2"), json!(true)] {
        for url in [OSS_HTTP, ""] {
            assert!(
                matches!(
                    answer("hi", 200, &delivered(url, &value)),
                    SpeechOutcomeV1::Unknown { error: None, ref reason } if reason.contains("character")
                ),
                "{value} {url:?}"
            );
        }
    }
}

/// Record §2.2 step 2 (I3-1 decision 8): a 2xx with a non-empty string `code` is `unknown` with a
/// 502 envelope, whatever else it carries; an empty `code` is not one.
#[test]
fn a_2xx_carrying_a_code_is_unknown() {
    let mut body = delivered(OSS_HTTP, &json!(2));
    body["code"] = json!("InvalidParameter");
    body["message"] = json!("bad voice");
    let SpeechOutcomeV1::Unknown { error: Some(error), .. } = answer("hi", 200, &body) else {
        panic!("unknown with an envelope")
    };
    assert_eq!((error.code, error.http_status), (ErrorCode::ProviderProtocolError, 502));
    assert_eq!(error.provider_message.as_deref(), Some("InvalidParameter: bad voice"));
    body["code"] = json!("");
    assert!(matches!(answer("hi", 200, &body), SpeechOutcomeV1::Succeeded { .. }));
}

/// Record §2.2: the non-2xx reader is `image-qwen`'s, so a `DataInspection` code is a policy
/// refusal and the provider message is `code: message`.
#[test]
fn the_dashscope_error_reader_is_shared_with_image_qwen() {
    let SpeechOutcomeV1::Rejected { error } = answer(
        "hi",
        400,
        &json!({ "code": "DataInspectionFailed", "message": "Input data may contain inappropriate content." }),
    ) else {
        panic!("rejected")
    };
    assert_eq!(error.code, ErrorCode::ContentPolicy);
    assert_eq!(
        error.provider_message.as_deref(),
        Some("DataInspectionFailed: Input data may contain inappropriate content.")
    );
}

/// Record §2.1: `instructions` only on an `instruct` upstream id (case-sensitive, as native).
#[test]
fn instructions_follow_the_model_class() {
    let request = json!({ "input": "x", "instructions": "slowly" });
    assert_eq!(
        upstream_body("qwen3-tts-instruct-flash", &request)["input"]["instructions"],
        "slowly"
    );
    for model in ["qwen3-tts-flash", "qwen3-tts-INSTRUCT-flash"] {
        assert!(upstream_body(model, &request)["input"].get("instructions").is_none(), "{model}");
    }
}

/// A Qwen-specific way to break the reference.
#[derive(Clone, Copy, Debug)]
enum Mutation {
    /// SV-Q10 dropped: the `url` artifact does not ask for the upgrade.
    NoUpgrade,
    /// A 2xx carrying a `code` read on as a success when it has a URL.
    IgnoreCode,
    /// SV-Q7 dropped: a zero or malformed count settles at the request's count.
    FallBackOnBadCharacters,
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
        REFERENCE.prepare(config, request, context)
    }
    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        let has_url = response.pointer("/body/json/output/audio/url").is_some_and(Value::is_string);
        let url = |characters: SpeechCharacterMeterV1| SpeechOutcomeV1::Succeeded {
            delivery: SpeechDeliveryV1::Audio(SpeechArtifactV1 {
                form: SpeechArtifactFormV1::Url {
                    pointer: south_contracts::JsonPointerV1::parse("/output/audio/url").unwrap(),
                    upgrade_insecure: true,
                },
                media_type: "audio/wav".to_owned(),
                container: None,
            }),
            metering: SpeechMeteringV1 {
                characters: Some(characters),
                ..SpeechMeteringV1::default()
            },
        };
        let request = SpeechCharacterMeterV1 {
            value: state["characters"].as_u64().unwrap_or(0),
            source: SpeechCharacterSourceV1::Request,
        };
        Ok(match (self.0, REFERENCE.parse_response(state, response)?) {
            (
                Mutation::NoUpgrade,
                SpeechOutcomeV1::Succeeded {
                    delivery: SpeechDeliveryV1::Audio(artifact),
                    metering,
                },
            ) => SpeechOutcomeV1::Succeeded {
                delivery: SpeechDeliveryV1::Audio(SpeechArtifactV1 {
                    form: match artifact.form {
                        SpeechArtifactFormV1::Url { pointer, .. } => {
                            SpeechArtifactFormV1::Url { pointer, upgrade_insecure: false }
                        }
                        other => other,
                    },
                    ..artifact
                }),
                metering,
            },
            (Mutation::IgnoreCode, SpeechOutcomeV1::Unknown { reason, .. })
                if reason.contains("DashScope") && has_url =>
            {
                url(request)
            }
            (Mutation::FallBackOnBadCharacters, SpeechOutcomeV1::Unknown { reason, .. })
                if reason.contains("character") && has_url =>
            {
                url(request)
            }
            (_, outcome) => outcome,
        })
    }
}

/// Each Qwen-specific mutation turns the shipped pack red on the cases that pin it.
#[test]
fn every_qwen_mutation_turns_the_pack_red() {
    let manifest = support::manifest(PACKAGE);
    let pack = support::pack(&manifest);
    for (mutation, cases) in [
        (Mutation::NoUpgrade, &["speech-v1.response.native", "speech-v1.response.https-url"][..]),
        (Mutation::IgnoreCode, &["speech-v1.response.code-in-2xx-with-url"][..]),
        (
            Mutation::FallBackOnBadCharacters,
            &[
                "speech-v1.response.characters-zero",
                "speech-v1.response.characters-negative",
                "speech-v1.response.characters-string",
            ][..],
        ),
    ] {
        let report = run_speech_component_suite_v1(&Mutant(mutation), &pack, &manifest);
        let failed: Vec<String> = report.failures().map(|outcome| outcome.case.clone()).collect();
        for case in cases {
            assert!(failed.iter().any(|name| name == case), "{mutation:?}: {case} stayed green");
        }
    }
    // Without the upgrade, the second hop of the real `http://` shape fails its row.
    let report = run_speech_component_suite_v1(&Mutant(Mutation::NoUpgrade), &pack, &manifest);
    assert!(
        report.failures().any(|outcome| outcome.check == CheckV1::SecondHopArtifact
            && outcome.case == "speech-v1.response.native"),
        "{report}"
    );
}

// ---- Transcription (`2026-10-21-speech-qwen-asr.md`) ----

const ASR_MODEL: &str = "qwen3-asr-flash";

fn asr_config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "bailian-qwen-speech",
        "base_url": "https://ws-example.ap-southeast-1.maas.aliyuncs.com",
        "auth": "provider_api_key",
        "models": [{ "model": ASR_MODEL }],
    }))
    .unwrap()
}

fn asr_context() -> SpeechCallContextV1 {
    serde_json::from_value(json!({
        "upstream_model": ASR_MODEL,
        "operation": "transcribe",
        "metering_required": ["seconds"],
    }))
    .unwrap()
}

/// A transcription view: the file part (file name, media type, size) and text parts.
fn asr_request(
    filename: Option<&str>,
    media_type: Option<&str>,
    bytes: u64,
    texts: &[(&str, &str)],
) -> Value {
    let mut file = json!({ "kind": "file", "name": "file", "blob": "b0", "bytes": bytes });
    if let Some(filename) = filename {
        file["filename"] = json!(filename);
    }
    if let Some(media_type) = media_type {
        file["media_type"] = json!(media_type);
    }
    let mut parts = vec![file];
    for (name, value) in texts {
        parts.push(json!({ "kind": "text", "name": name, "value": value }));
    }
    json!({ "multipart": { "parts": parts } })
}

fn asr_prepare(
    request: &Value,
) -> Result<PreparedSpeechCallV1, token_station_protocol::ErrorEnvelope> {
    REFERENCE.prepare(&asr_config(), request, &asr_context())
}

fn asr_template(request: &Value) -> Value {
    let prepared = asr_prepare(request).unwrap();
    let MediaBodyV1::Json(template) = prepared.descriptor.body() else { panic!("a JSON body") };
    serde_json::from_str(template).unwrap()
}

/// The outcome of a transcription view with this status and JSON body, in `format`.
fn asr_answer(format: &str, status: u16, body: &Value) -> SpeechOutcomeV1 {
    let prepared =
        asr_prepare(&asr_request(Some("a.mp3"), None, 9, &[("response_format", format)])).unwrap();
    let state: Value = serde_json::from_str(&prepared.state).unwrap();
    let view = json!({ "status": status, "headers": [], "body": { "json": body } });
    REFERENCE.parse_response(&state, &view).unwrap()
}

fn transcript_body(text: &Value, seconds: &Value) -> Value {
    json!({
        "output": { "choices": [{ "message": { "role": "assistant", "content": [{ "text": text }] } }] },
        "usage": { "seconds": seconds },
    })
}

/// The shipped manifest declares `transcribe` beside synthesis, at 1.1.0, on the runtime and
/// contracts it already declared (`data_url` is a media contract 1 word).
#[test]
fn the_manifest_declares_transcription_at_1_1_0() {
    let manifest = support::manifest(PACKAGE);
    // Published as 1.1.0 with 0.76.0; the Gemini signatures re-pin made it 1.1.1
    // (`gemini_signatures_retires_every_published_076_package_identity`).
    // Published as 1.1.1 with 0.77.0; the Gemini placeholder signature round made it 1.1.2
    // (`gemini_placeholder_retires_every_published_077_package_identity`).
    // Published as 1.1.2 with 0.78.0; the 0.79.0 round made it 1.1.3
    // (`qwen_tier_and_responses_stop_retires_every_published_078_package_identity`).
    // Published as 1.1.3 with 0.79.0; the 0.80.0 round made it 1.1.4
    // (`veo_spelling_and_durationless_asr_retire_every_published_079_package_identity`).
    assert_eq!(manifest.version, "1.1.4");
    // Published as 1.1.3 with 0.79.0; the 0.80.0 round made it 1.1.4
    // (`veo_spelling_and_durationless_asr_retire_every_published_079_package_identity`).
    assert_eq!(REFERENCE.metadata().version, "1.1.4");
    assert!(manifest.capabilities.contains("transcribe"));
    assert!(!manifest.capabilities.contains("translate"));
    assert_eq!(manifest.compatibility.south_runtime, "0.77.0"); // release 0.77.0 raised every package (canonical_ir 5)
    assert_eq!(
        south_contracts::media::MediaTransformV1::DataUrl.minimum_contract_version(),
        south_contracts::media::MEDIA_CONTRACT_VERSION
    );
}

/// Every model declares both operations, `transcribe` metered in seconds, and never `translate`.
#[test]
fn every_model_transcribes_and_none_translates() {
    let models = REFERENCE.model_capabilities(&asr_config()).unwrap();
    let model = serde_json::to_value(&models[0]).unwrap();
    assert_eq!(model["operations"], json!(["synthesize", "transcribe"]));
    assert_eq!(model["metering_forms"], json!(["characters", "seconds"]));
}

/// Record §3.1: the audio reaches the upstream as a data URL the host expands from the file blob,
/// typed by the part's media type; the call reserves at most 300 seconds.
#[test]
fn the_audio_is_a_data_url_the_host_expands() {
    let prepared = asr_prepare(&asr_request(Some("a.mp3"), Some("audio/mpeg"), 3, &[])).unwrap();
    assert_eq!(prepared.facts.reservation.and_then(|bounds| bounds.max_seconds), Some(300));
    assert_eq!(prepared.immutable_body_paths, Some(vec!["model".to_owned(), "input".to_owned()]));
    let MediaBodyV1::Json(template) = prepared.descriptor.body() else { panic!("a JSON body") };
    let blobs = south_contracts::media::BlobSetV1::new([south_contracts::media::BlobV1::file(
        south_contracts::media::BlobIdV1::parse("b0").unwrap(),
        b"abc".to_vec(),
    )])
    .unwrap();
    let body: Value = serde_json::from_str(
        south_contracts::media::expand_json_template_v1(template, &blobs).unwrap().as_str(),
    )
    .unwrap();
    assert_eq!(
        body,
        json!({
            "model": ASR_MODEL,
            "input": { "messages": [{ "role": "user", "content": [{ "audio": "data:audio/mpeg;base64,YWJj" }] }] },
        })
    );
}

/// Record §3.2: `language` and `enable_itn` become `asr_options`, `prompt` the system message's
/// text; `temperature` is dropped; empty values are not sent.
#[test]
fn language_prompt_and_itn_map_onto_the_dashscope_body() {
    let body = asr_template(&asr_request(
        Some("a.mp3"),
        None,
        9,
        &[("language", "zh"), ("prompt", "Qwen"), ("enable_itn", "False"), ("temperature", "1")],
    ));
    assert_eq!(
        body["input"]["messages"][0],
        json!({ "role": "system", "content": [{ "text": "Qwen" }] })
    );
    assert_eq!(body["input"]["messages"][1]["role"], "user");
    assert_eq!(
        body["parameters"],
        json!({ "asr_options": { "language": "zh", "enable_itn": false } })
    );
    let bare =
        asr_template(&asr_request(Some("a.mp3"), None, 9, &[("language", ""), ("prompt", "")]));
    assert_eq!(bare["input"]["messages"].as_array().map(Vec::len), Some(1));
    assert!(bare.get("parameters").is_none());
}

/// Record §3.3: the part's own audio or video media type (lower-cased), else the file name's
/// extension when the part has none or `application/octet-stream`, else refused.
#[test]
fn the_media_type_comes_from_the_part_then_the_extension() {
    let media_type = |filename: Option<&str>, part: Option<&str>| {
        asr_prepare(&asr_request(filename, part, 9, &[])).map(|prepared| {
            let MediaBodyV1::Json(template) = prepared.descriptor.body() else { panic!() };
            let template: Value = serde_json::from_str(template).unwrap();
            template["input"]["messages"][0]["content"][0]["audio"]["$south.ref"]["media_type"]
                .as_str()
                .unwrap()
                .to_owned()
        })
    };
    for (filename, part, expected) in [
        (Some("x.bin"), Some("audio/x-m4a"), "audio/x-m4a"),
        (Some("x.mp3"), Some("Audio/MPEG"), "audio/mpeg"),
        (Some("x.mp4"), Some("video/mp4"), "video/mp4"),
        (Some("x.MP3"), None, "audio/mpeg"),
        (Some("x.wav"), Some("application/octet-stream"), "audio/wav"),
        (Some("x.m4a"), None, "audio/mp4"),
        (Some("x.aac"), None, "audio/aac"),
        (Some("x.flac"), None, "audio/flac"),
        (Some("x.ogg"), None, "audio/ogg"),
        (Some("x.opus"), None, "audio/opus"),
        (Some("x.webm"), None, "audio/webm"),
        (Some("x.amr"), None, "audio/amr"),
        (Some("x.mp4"), None, "video/mp4"),
    ] {
        assert_eq!(media_type(filename, part).as_deref(), Ok(expected), "{filename:?} {part:?}");
    }
    for (filename, part) in [
        (Some("x.txt"), None),
        (None, None),
        (Some("mp3"), None),
        (Some("x.mp3"), Some("image/png")),
        (Some("x.mp3"), Some("text/plain")),
    ] {
        let error = media_type(filename, part).unwrap_err();
        assert_eq!((error.code, error.http_status), (ErrorCode::InvalidRequest, 400));
    }
}

/// Record §3.4: the data URL may be at most 10,000,000 bytes (10 MB read as the smaller, decimal
/// unit), refused before dispatch above it.
#[test]
fn the_data_url_bound_is_ten_decimal_megabytes() {
    // `data:audio/mpeg;base64,` is 23 bytes; 7_499_982 bytes encode to 9_999_976 characters
    // (9_999_999 in all), 7_499_983 to 9_999_980 (10_000_003).
    assert!(asr_prepare(&asr_request(Some("a.mp3"), Some("audio/mpeg"), 7_499_982, &[])).is_ok());
    assert!(asr_prepare(&asr_request(Some("a.mp3"), Some("audio/mpeg"), 7_499_983, &[])).is_err());
    // `data:audio/wav;base64,` is 22 bytes: 9_999_998 in all.
    assert!(asr_prepare(&asr_request(Some("a.wav"), Some("audio/wav"), 7_499_982, &[])).is_ok());
    let error =
        asr_prepare(&asr_request(Some("a.wav"), Some("audio/wav"), 7_499_983, &[])).unwrap_err();
    assert_eq!((error.code, error.http_status), (ErrorCode::InvalidRequest, 400));
}

/// Record §3.5: `json` and `text` are served; the timestamped formats and anything else are refused
/// before dispatch, as is streaming.
#[test]
fn only_json_and_text_are_served() {
    for format in ["json", "text"] {
        assert!(
            asr_prepare(&asr_request(Some("a.mp3"), None, 9, &[("response_format", format)]))
                .is_ok()
        );
    }
    for format in ["verbose_json", "srt", "vtt", "xml", "JSON", ""] {
        let error =
            asr_prepare(&asr_request(Some("a.mp3"), None, 9, &[("response_format", format)]))
                .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{format}");
    }
    assert!(asr_prepare(&asr_request(Some("a.mp3"), None, 9, &[("stream", "true")])).is_err());
}

/// Record §3.6: the seconds are `usage.seconds` when it is a non-negative integer, reported; any
/// other value, or none, is `unknown` with no fallback.
#[test]
fn seconds_are_the_reported_integer_or_unknown() {
    let text = json!("hello");
    for (seconds, expected) in [(json!(0), 0.0), (json!(1), 1.0), (json!(300), 300.0)] {
        let SpeechOutcomeV1::Succeeded { metering, .. } =
            asr_answer("json", 200, &transcript_body(&text, &seconds))
        else {
            panic!("a delivery for {seconds}")
        };
        assert_eq!(
            metering.seconds,
            Some(SpeechSecondsMeterV1 { value: expected, basis: SpeechSecondsBasisV1::Reported })
        );
    }
    for seconds in [Value::Null, json!(-1), json!(1.5), json!(2.0), json!("2"), json!(true)] {
        assert!(
            matches!(
                asr_answer("json", 200, &transcript_body(&text, &seconds)),
                SpeechOutcomeV1::Unknown { error: None, .. }
            ),
            "{seconds}"
        );
    }
    let mut no_usage = transcript_body(&text, &json!(1));
    no_usage.as_object_mut().unwrap().remove("usage");
    assert!(matches!(asr_answer("text", 200, &no_usage), SpeechOutcomeV1::Unknown { .. }));
}

/// Record §3.6: `json` is `{"text"}`, `text` the plain transcript; a transcript the host elided is
/// a reference in `json` and `unknown` in `text`.
#[test]
fn the_transcript_is_rendered_in_the_client_format() {
    let body = transcript_body(&json!("hello"), &json!(4));
    let SpeechOutcomeV1::Succeeded { delivery: SpeechDeliveryV1::Transcript(json_doc), .. } =
        asr_answer("json", 200, &body)
    else {
        panic!("a transcript")
    };
    assert_eq!(json_doc, TranscriptDeliveryV1::Json { template: r#"{"text":"hello"}"#.to_owned() });
    let SpeechOutcomeV1::Succeeded { delivery: SpeechDeliveryV1::Transcript(text_doc), .. } =
        asr_answer("text", 200, &body)
    else {
        panic!("a transcript")
    };
    assert_eq!(
        text_doc,
        TranscriptDeliveryV1::Text {
            text: "hello".to_owned(),
            media_type: "text/plain; charset=utf-8".to_owned()
        }
    );
    let elided = json!({ "$south.blob": { "id": "r0", "bytes": 2_000_000, "head": "aaaa" } });
    let body = transcript_body(&elided, &json!(4));
    let SpeechOutcomeV1::Succeeded { delivery: SpeechDeliveryV1::Transcript(reference), .. } =
        asr_answer("json", 200, &body)
    else {
        panic!("a transcript")
    };
    assert_eq!(
        reference,
        TranscriptDeliveryV1::Json {
            template: r#"{"text":{"$south.ref":{"blob":"r0","transform":"as_is"}}}"#.to_owned()
        }
    );
    assert!(matches!(asr_answer("text", 200, &body), SpeechOutcomeV1::Unknown { .. }));
}

/// A 2xx carrying a `DashScope` code is `unknown` with a 502 envelope, as for synthesis.
#[test]
fn a_transcription_2xx_carrying_a_code_is_unknown() {
    let mut body = transcript_body(&json!("hello"), &json!(1));
    body["code"] = json!("InvalidParameter");
    let SpeechOutcomeV1::Unknown { error: Some(error), .. } = asr_answer("json", 200, &body) else {
        panic!("unknown with an envelope")
    };
    assert_eq!((error.code, error.http_status), (ErrorCode::ProviderProtocolError, 502));
}

/// Guessing a duration for a 2xx without `usage.seconds` (a fallback the record refuses) turns the
/// missing-duration cases red.
#[test]
fn a_guessed_duration_turns_the_pack_red() {
    struct GuessSeconds;
    impl SpeechComponentV1 for GuessSeconds {
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
            REFERENCE.prepare(config, request, context)
        }
        fn parse_response(
            &self,
            state: &Value,
            response: &Value,
        ) -> ComponentResultV1<SpeechOutcomeV1> {
            let mut response = response.clone();
            if let Some(usage) = response.pointer_mut("/body/json/usage")
                && usage.get("seconds").is_none_or(Value::is_null)
            {
                usage["seconds"] = json!(60);
            }
            REFERENCE.parse_response(state, &response)
        }
    }
    let manifest = support::manifest(PACKAGE);
    let report = run_speech_component_suite_v1(&GuessSeconds, &support::pack(&manifest), &manifest);
    let failed: Vec<(CheckV1, String)> =
        report.failures().map(|outcome| (outcome.check, outcome.case.clone())).collect();
    for case in ["speech-v1.response.asr-seconds-missing", "speech-v1.response.asr-seconds-null"] {
        assert!(
            failed.contains(&(CheckV1::AbsentDurationIsNullOrUnknown, case.to_owned())),
            "{case} stayed green: {failed:?}"
        );
    }
}
