//! `south.speech-component.v1` against the `speech-qwen` native reference and its shipped pack, and
//! the behaviours of design record `2026-10-15-speech-v4-tts.md` §2 pinned by name: the `url`
//! artifact with SV-Q10's upgrade, the weighted `upper`, SV-Q7's character rule, the 2xx `code`,
//! the `DashScope` error reader shared with `image-qwen`, and the `instruct` model class.

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
    upgrade_insecure_artifact_url_v1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

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

/// The rows the Qwen pack must show: V3-1's synthesis rows and the second hop.
fn rows() -> Vec<CheckV1> {
    let mut rows = support::APPLICABLE_ROWS.to_vec();
    rows.push(CheckV1::SecondHopArtifact);
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
    for row in ["speech-v1.second-hop", "speech-v1.metering.characters.request"] {
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
