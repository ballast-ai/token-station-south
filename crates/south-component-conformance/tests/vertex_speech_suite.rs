//! `south.speech-component.v1` against the `speech-vertex` native reference and its shipped pack,
//! and the behaviours of design record `2026-10-15-speech-v4-tts.md` §3 pinned by name: the
//! configuration refusals (500s, outside the pack), the configuration and credentials copied from
//! `image-vertex`, the Q7 downgrade, the WAV container and the audio-less answers.

use serde_json::{Value, json};
use south_component_conformance::reference_vertex_speech::VertexSpeechReferenceV1;
use south_component_conformance::{
    ComponentResultV1, SpeechComponentV1, SpeechDeliveryV1, SpeechOutcomeV1,
    run_speech_component_suite_v1,
};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechArtifactFormV1, SpeechCallContextV1, SpeechContainerV1,
    SpeechEncodingV1, SpeechModelCapabilitiesV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

#[path = "support/speech_v3_1.rs"]
mod support;

const PACKAGE: &str = "speech-vertex";
const REFERENCE: VertexSpeechReferenceV1 = VertexSpeechReferenceV1;
const MODEL: &str = "gemini-2.5-flash-preview-tts";

fn config(base_url: &str, declared: &Value) -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "vertex-ai-speech",
        "base_url": base_url,
        "auth": "provider_api_key",
        "declared": declared,
        "models": [{ "model": MODEL }],
    }))
    .unwrap()
}

fn located() -> ProviderConfig {
    config(
        "https://us-central1-aiplatform.googleapis.com",
        &json!({ "project_id": "test-project", "region": "us-central1" }),
    )
}

fn context() -> SpeechCallContextV1 {
    serde_json::from_value(json!({
        "upstream_model": MODEL,
        "operation": "synthesize",
        "metering_required": ["characters"],
    }))
    .unwrap()
}

/// The prepared call as JSON, or the refusal as JSON.
fn prepare(config: &ProviderConfig, request: &Value) -> Result<Value, Value> {
    REFERENCE
        .prepare(config, &json!({ "json": request }), &context())
        .map(|prepared| serde_json::from_str(&prepared.to_json()).unwrap())
        .map_err(|error| serde_json::to_value(error).unwrap())
}

/// The outcome of a 2xx view with this JSON body for a call with this `response_format`.
fn answer(format: &str, body: &Value) -> SpeechOutcomeV1 {
    let prepared = REFERENCE
        .prepare(
            &located(),
            &json!({ "json": { "input": "hi", "response_format": format } }),
            &context(),
        )
        .unwrap();
    let state: Value = serde_json::from_str(&prepared.state).unwrap();
    let view = json!({ "status": 200, "headers": [], "body": { "json": body } });
    REFERENCE.parse_response(&state, &view).unwrap()
}

fn audio_body() -> Value {
    json!({ "candidates": [{ "content": { "parts": [
        { "inlineData": { "mimeType": "audio/L16;codec=pcm;rate=24000",
                          "data": { "$south.blob": { "id": "b0", "bytes": 4, "head": "50434d21" } } } }
    ] } }] })
}

fn manifest_json(name: &str) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../components")
                .join(name)
                .join("manifest.json"),
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn the_manifest_passes_gate_one() {
    assert_eq!(support::manifest(PACKAGE).validate(), Ok(()));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let manifest = support::manifest(PACKAGE);
    let pack = support::pack(&manifest);
    assert_eq!(pack.credentials().cases().len(), 5, "the five credential cases of image-vertex");
    let report = run_speech_component_suite_v1(&REFERENCE, &pack, &manifest);
    let failures = support::failures(&report);
    assert!(failures.is_empty(), "{failures:#?}");
    support::assert_rows_ran(&report, &support::APPLICABLE_ROWS);
}

#[test]
fn every_break_turns_its_row_red() {
    let manifest = support::manifest(PACKAGE);
    support::assert_every_break_bites(&REFERENCE, &support::pack(&manifest), &manifest);
}

/// Record §3.1: the configuration keys and the credentials section are `image-vertex`'s byte for
/// byte, so one provider row and one credential serve all three Vertex packages.
#[test]
fn configuration_and_credentials_are_image_vertexs() {
    let speech = manifest_json(PACKAGE);
    let image = manifest_json("image-vertex");
    assert_eq!(
        speech["config_schema"]["vertex-ai-speech"],
        image["config_schema"]["vertex-ai-image"]
    );
    assert_eq!(speech["credentials"], image["credentials"]);
}

/// The configuration refusals of record §3.1, by name, with the texts `image-vertex` gives.
pub const CONFIGURATION_REFUSALS: [(&str, &str, &str, &str); 4] = [
    (
        "missing-region",
        "https://us-central1-aiplatform.googleapis.com",
        r#"{"project_id": "test-project"}"#,
        "the provider configuration carries no `region` for Vertex AI",
    ),
    (
        "missing-project",
        "https://us-central1-aiplatform.googleapis.com",
        r#"{"region": "us-central1"}"#,
        "the provider configuration carries no `project_id` for Vertex AI",
    ),
    (
        "region-mismatch",
        "https://us-central1-aiplatform.googleapis.com",
        r#"{"project_id": "test-project", "region": "europe-west4"}"#,
        "the base URL https://us-central1-aiplatform.googleapis.com is not the Vertex AI endpoint \
         of location `europe-west4`, which is https://europe-west4-aiplatform.googleapis.com",
    ),
    (
        "global-prefixed-host",
        "https://global-aiplatform.googleapis.com",
        r#"{"project_id": "test-project", "region": "global"}"#,
        "the base URL https://global-aiplatform.googleapis.com is not the Vertex AI endpoint of \
         location `global`, which is https://aiplatform.googleapis.com",
    ),
];

/// Record §3.1 (P12): a missing `region` or project, and a Vertex AI base URL the region does not
/// select, are a 500 naming the value, reported before the refusal of an empty `input`.
#[test]
fn a_configuration_error_is_a_500_before_any_request_refusal() {
    for (name, base_url, declared, message) in CONFIGURATION_REFUSALS {
        let config = config(base_url, &serde_json::from_str(declared).unwrap());
        let error = REFERENCE
            .prepare(&config, &json!({ "json": { "input": "" } }), &context())
            .expect_err(name);
        assert_eq!((error.code, error.http_status), (ErrorCode::Internal, 500), "{name}");
        assert_eq!(error.message, message, "{name}");
    }
    let error = prepare(&located(), &json!({ "input": "" })).unwrap_err();
    assert_eq!(error["http_status"], 400, "{error}");
}

/// Record §3.4 (Q7, 1.0.0): mp3, opus, aac and flac are served as WAV, any case; an unknown value
/// is refused with native's text.
#[test]
fn q7_transcode_only_formats_degrade_to_wav() {
    for format in ["mp3", "MP3", "opus", "aac", "flac", "wav", "Wav"] {
        let prepared = prepare(&located(), &json!({ "input": "x", "response_format": format }))
            .unwrap_or_else(|error| panic!("{format}: {error}"));
        assert_eq!(prepared["facts"]["output_media_type"], "audio/wav", "{format}");
        assert_eq!(prepared["state"]["format"], "wav", "{format}");
    }
    let prepared = prepare(&located(), &json!({ "input": "x", "response_format": "pcm" })).unwrap();
    assert_eq!(prepared["facts"]["output_media_type"], "audio/L16; rate=24000");
    for format in ["ogg", "pcm16", "", " wav"] {
        let error = prepare(&located(), &json!({ "input": "x", "response_format": format }))
            .expect_err(format);
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .ends_with("supported: wav, pcm (mp3/opus/aac/flac degrade to wav)."),
            "{format}: {error}"
        );
    }
}

/// Record §3.3: WAV is the base64 PCM inside the 24000 Hz mono container; `pcm` is the bare
/// samples labelled `audio/L16; rate=24000`.
#[test]
fn the_container_follows_the_format() {
    for (format, media_type, container) in [
        (
            "wav",
            "audio/wav",
            Some(SpeechContainerV1::WavPcmS16le { sample_rate: 24_000, channels: 1 }),
        ),
        (
            "mp3",
            "audio/wav",
            Some(SpeechContainerV1::WavPcmS16le { sample_rate: 24_000, channels: 1 }),
        ),
        ("pcm", "audio/L16; rate=24000", None),
    ] {
        let SpeechOutcomeV1::Succeeded { delivery: SpeechDeliveryV1::Audio(artifact), .. } =
            answer(format, &audio_body())
        else {
            panic!("{format}: a delivery")
        };
        assert_eq!(artifact.media_type, media_type, "{format}");
        assert_eq!(artifact.container, container, "{format}");
        assert!(
            matches!(&artifact.form, SpeechArtifactFormV1::Inline { pointer, encoding: SpeechEncodingV1::Base64 }
                if pointer.as_str() == "/candidates/0/content/parts/0/inlineData/data"),
            "{format}"
        );
    }
}

/// Record §3.3 step 3 (I3-2 GI-Q5): a blocked prompt is `rejected`, a policy stop `unknown`, both
/// with a `content_policy` envelope; another audio-less answer is `unknown` without one.
#[test]
fn audio_less_answers_follow_gi_q5() {
    let SpeechOutcomeV1::Rejected { error } =
        answer("wav", &json!({ "promptFeedback": { "blockReason": "SAFETY" } }))
    else {
        panic!("rejected")
    };
    assert_eq!(error.code, ErrorCode::ContentPolicy);
    let SpeechOutcomeV1::Unknown { error: Some(error), .. } =
        answer("wav", &json!({ "candidates": [{ "finishReason": "PROHIBITED_CONTENT" }] }))
    else {
        panic!("unknown with an envelope")
    };
    assert_eq!(error.code, ErrorCode::ContentPolicy);
    assert!(matches!(
        answer("wav", &json!({ "candidates": [{ "finishReason": "STOP" }] })),
        SpeechOutcomeV1::Unknown { error: None, .. }
    ));
}

/// A Vertex-specific way to break the reference.
#[derive(Clone, Copy, Debug)]
enum Mutation {
    /// Q7 dropped early: mp3, opus, aac and flac refused in 1.0.0.
    RefuseTranscodeFormats,
    /// The container left off a WAV delivery.
    NoContainer,
    /// A blocked prompt answered `unknown` rather than `rejected`.
    BlockedAsUnknown,
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
        let format = request.pointer("/json/response_format").and_then(Value::as_str);
        if matches!(self.0, Mutation::RefuseTranscodeFormats)
            && let Some(format) = format.map(str::to_lowercase)
            && ["mp3", "opus", "aac", "flac"].contains(&format.as_str())
        {
            return Err(token_station_protocol::ErrorEnvelope::new(
                ErrorCode::InvalidRequest,
                400,
                format!(
                    "Gemini TTS on Vertex cannot emit response_format='{format}'. Supported: wav, pcm."
                ),
            ));
        }
        REFERENCE.prepare(config, request, context)
    }
    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        Ok(match (self.0, REFERENCE.parse_response(state, response)?) {
            (
                Mutation::NoContainer,
                SpeechOutcomeV1::Succeeded {
                    delivery: SpeechDeliveryV1::Audio(mut artifact),
                    metering,
                },
            ) => {
                artifact.container = None;
                SpeechOutcomeV1::Succeeded { delivery: SpeechDeliveryV1::Audio(artifact), metering }
            }
            (Mutation::BlockedAsUnknown, SpeechOutcomeV1::Rejected { error })
                if error.code == ErrorCode::ContentPolicy =>
            {
                SpeechOutcomeV1::Unknown { reason: "blocked".to_owned(), error: Some(error) }
            }
            (_, outcome) => outcome,
        })
    }
}

/// Each Vertex-specific mutation turns the shipped pack red on the cases that pin it.
#[test]
fn every_vertex_mutation_turns_the_pack_red() {
    let manifest = support::manifest(PACKAGE);
    let pack = support::pack(&manifest);
    for (mutation, cases) in [
        (
            Mutation::RefuseTranscodeFormats,
            &[
                "speech-v1.prepare.format-mp3-degrades-to-wav",
                "speech-v1.prepare.format-mp3-upper-degrades-to-wav",
                "speech-v1.prepare.format-opus-degrades-to-wav",
                "speech-v1.prepare.format-aac-degrades-to-wav",
                "speech-v1.prepare.format-flac-degrades-to-wav",
            ][..],
        ),
        (
            Mutation::NoContainer,
            &["speech-v1.response.native", "speech-v1.response.mp3-served-as-wav"][..],
        ),
        (Mutation::BlockedAsUnknown, &["speech-v1.response.prompt-blocked"][..]),
    ] {
        let report = run_speech_component_suite_v1(&Mutant(mutation), &pack, &manifest);
        let failed: Vec<String> = report.failures().map(|outcome| outcome.case.clone()).collect();
        for case in cases {
            assert!(failed.iter().any(|name| name == case), "{mutation:?}: {case} stayed green");
        }
    }
}
