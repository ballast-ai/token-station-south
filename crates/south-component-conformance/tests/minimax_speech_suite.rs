//! `south.speech-component.v1` against the `speech-minimax` native reference and its shipped pack,
//! and the behaviours of design record `2026-10-14-speech-v3-2-tts.md` §3 pinned by name.

use serde_json::{Value, json};
use south_component_conformance::reference_minimax_speech::MiniMaxSpeechReferenceV1;
use south_component_conformance::{
    ComponentResultV1, SpeechComponentV1, SpeechDeliveryV1, SpeechOutcomeV1,
    run_speech_component_suite_v1,
};
use south_contracts::media::MediaBodyV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechArtifactFormV1, SpeechArtifactV1, SpeechCallContextV1,
    SpeechCharacterMeterV1, SpeechCharacterSourceV1, SpeechEncodingV1, SpeechMeteringV1,
    SpeechModelCapabilitiesV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

#[path = "support/speech_v3_1.rs"]
mod support;

const PACKAGE: &str = "speech-minimax";
const REFERENCE: MiniMaxSpeechReferenceV1 = MiniMaxSpeechReferenceV1;

fn config(group_id: Option<&str>) -> ProviderConfig {
    let mut config = json!({
        "provider": "minimax-speech",
        "base_url": "https://api.minimaxi.com",
        "auth": "provider_api_key",
        "models": [{ "model": "speech-2.6-hd" }],
    });
    if let Some(group_id) = group_id {
        config["declared"] = json!({ "group_id": group_id });
    }
    serde_json::from_value(config).unwrap()
}

fn context() -> SpeechCallContextV1 {
    serde_json::from_value(json!({
        "upstream_model": "speech-2.6-hd",
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

/// The upstream body `prepare` builds for this client body.
fn upstream_body(request: &Value) -> Value {
    let prepared =
        REFERENCE.prepare(&config(None), &json!({ "json": request }), &context()).unwrap();
    let MediaBodyV1::Json(template) = prepared.descriptor.body() else { panic!("a JSON body") };
    serde_json::from_str(template).unwrap()
}

/// The outcome of a 2xx view with this JSON body, for a call whose input is `input`.
fn answer(input: &str, body: &Value) -> SpeechOutcomeV1 {
    let prepared = REFERENCE
        .prepare(&config(None), &json!({ "json": { "input": input } }), &context())
        .unwrap();
    let state: Value = serde_json::from_str(&prepared.state).unwrap();
    let view = json!({ "status": 200, "headers": [], "body": { "json": body } });
    REFERENCE.parse_response(&state, &view).unwrap()
}

fn audio(usage: &Value) -> Value {
    json!({
        "data": { "audio": { "$south.blob": { "id": "b0", "bytes": 10, "head": "415544494f" } } },
        "extra_info": { "usage_characters": usage },
        "base_resp": { "status_code": 0, "status_msg": "success" },
    })
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
    support::assert_rows_ran(&report, &support::APPLICABLE_ROWS);
}

/// Record §3 (IM-Q3, P6): `?GroupId=` exactly when the family's `group_id` key is set; a value that
/// is not digits (the host refuses it at save) never reaches the wire, because the descriptor's
/// `GroupId` grammar is digits only.
#[test]
fn the_group_id_is_sent_only_when_configured_and_only_as_digits() {
    let body = json!({ "input": "hi" });
    let sent = prepare(&config(Some("1900001234")), &body).unwrap();
    assert_eq!(sent["descriptor"]["query"], json!([{ "name": "GroupId", "value": "1900001234" }]));
    let absent = prepare(&config(None), &body).unwrap();
    assert!(absent["descriptor"].get("query").is_none(), "{absent}");
    for value in ["19a", " 1900", "1900 ", "-1", "1e3"] {
        let refused = prepare(&config(Some(value)), &body).expect_err(value);
        assert_eq!(refused["code"], "internal", "{value}: {refused}");
    }
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../components/speech-minimax/manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["config_schema"]["minimax-speech"]["group_id"]["syntax"], "digits");
}

/// Record §3: the top-level `speed` is sent as a float, so a client `1` is `1.0` on the wire; a
/// client `voice_setting.speed` that is not `null` wins and is forwarded as sent.
#[test]
fn the_top_level_speed_is_sent_as_a_float() {
    let body = upstream_body(&json!({ "input": "x", "speed": 1 }));
    assert_eq!(body["voice_setting"]["speed"], json!(1.0));
    assert_ne!(body["voice_setting"]["speed"], json!(1));
    let body =
        upstream_body(&json!({ "input": "x", "speed": 1.5, "voice_setting": { "speed": 2 } }));
    assert_eq!(body["voice_setting"]["speed"], json!(2));
}

/// Record §3 and §8: `upper` is the weighted count (ASCII 1, any other scalar 2).
#[test]
fn the_upper_is_the_weighted_count() {
    for (input, count, upper) in [("hello world", 11, 11), ("안녕하세요", 5, 10), ("hi 안녕", 5, 7)]
    {
        let prepared = prepare(&config(None), &json!({ "input": input })).unwrap();
        assert_eq!(prepared["facts"]["characters"], json!({ "count": count, "upper": upper }));
    }
}

/// SV-Q7 (ruled 2026-10-14): a positive integer is the upstream's count, absent or `null` the
/// request's, and `0` or any other value `unknown`, never a $0 or a silently lower settlement.
#[test]
fn upstream_characters_follow_sv_q7() {
    let succeeded = |outcome: SpeechOutcomeV1| match outcome {
        SpeechOutcomeV1::Succeeded { delivery: SpeechDeliveryV1::Audio(artifact), metering } => {
            let SpeechArtifactFormV1::Inline { pointer, encoding } = &artifact.form else {
                panic!("an inline artifact")
            };
            assert_eq!(pointer.as_str(), "/data/audio");
            assert_eq!(*encoding, SpeechEncodingV1::Hex);
            metering.characters.expect("characters")
        }
        other => panic!("not a delivery: {other:?}"),
    };
    assert_eq!(
        succeeded(answer("안녕", &audio(&json!(9)))),
        SpeechCharacterMeterV1 { value: 9, source: SpeechCharacterSourceV1::Upstream }
    );
    assert_eq!(
        succeeded(answer("안녕", &audio(&Value::Null))),
        SpeechCharacterMeterV1 { value: 2, source: SpeechCharacterSourceV1::Request }
    );
    for value in [json!(0), json!(-1), json!(2.5), json!(9.0), json!("9"), json!(true), json!([9])]
    {
        assert!(
            matches!(answer("안녕", &audio(&value)), SpeechOutcomeV1::Unknown { error: None, .. }),
            "{value}"
        );
    }
}

/// Record §3 (I3-1 table, P4): every `base_resp` failure keeps the host's status; only the codes
/// meaning "refused before output" are `rejected`.
#[test]
fn a_body_detected_failure_carries_the_hosts_status() {
    for (code, status, error_code, rejected) in [
        (1004, 401, ErrorCode::Auth, true),
        (2049, 401, ErrorCode::Auth, true),
        (1026, 400, ErrorCode::ContentPolicy, false),
        (2013, 400, ErrorCode::InvalidRequest, true),
        (1002, 429, ErrorCode::RateLimit, true),
        (1008, 503, ErrorCode::UpstreamUnavailable, true),
        (2153, 503, ErrorCode::UpstreamUnavailable, true),
        (1000, 502, ErrorCode::ProviderProtocolError, false),
    ] {
        let outcome =
            answer("hi", &json!({ "base_resp": { "status_code": code, "status_msg": "m" } }));
        let ((SpeechOutcomeV1::Rejected { error }, true)
        | (SpeechOutcomeV1::Unknown { error: Some(error), .. }, false)) = (&outcome, rejected)
        else {
            panic!("{code}: {outcome:?}")
        };
        assert_eq!((error.http_status, error.code), (status, error_code), "{code}");
        assert_eq!(error.provider_message.as_deref(), Some("m"));
    }
}

#[test]
fn every_break_turns_its_row_red() {
    let manifest = support::manifest(PACKAGE);
    support::assert_every_break_bites(&REFERENCE, &support::pack(&manifest), &manifest);
}

/// A `MiniMax`-specific way to break the reference.
#[derive(Clone, Copy, Debug)]
enum Mutation {
    /// SV-Q7 dropped: a zero or malformed count settles at the request's count.
    FallBackOnBadCharacters,
    /// The `GroupId` never sent.
    DropGroupId,
    /// 1026 (possibly charged) released as `rejected`.
    ReleaseOutputViolation,
    /// An empty `data.audio` delivered.
    DeliverEmptyAudio,
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
        if matches!(self.0, Mutation::DropGroupId) {
            let mut value = serde_json::to_value(config).unwrap();
            if let Some(declared) = value.get_mut("declared").and_then(Value::as_object_mut) {
                declared.remove("group_id");
            }
            return REFERENCE.prepare(&serde_json::from_value(value).unwrap(), request, context);
        }
        REFERENCE.prepare(config, request, context)
    }
    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        let outcome = REFERENCE.parse_response(state, response)?;
        let characters = SpeechCharacterMeterV1 {
            value: state["characters"].as_u64().unwrap_or(0),
            source: SpeechCharacterSourceV1::Request,
        };
        let delivered = || SpeechOutcomeV1::Succeeded {
            delivery: SpeechDeliveryV1::Audio(SpeechArtifactV1 {
                form: SpeechArtifactFormV1::Inline {
                    pointer: south_contracts::JsonPointerV1::parse("/data/audio").unwrap(),
                    encoding: SpeechEncodingV1::Hex,
                },
                media_type: "audio/mpeg".to_owned(),
                container: None,
            }),
            metering: SpeechMeteringV1 {
                characters: Some(characters),
                ..SpeechMeteringV1::default()
            },
        };
        Ok(match (self.0, outcome) {
            (Mutation::FallBackOnBadCharacters, SpeechOutcomeV1::Unknown { reason, .. })
                if reason.contains("character") =>
            {
                delivered()
            }
            (
                Mutation::ReleaseOutputViolation,
                SpeechOutcomeV1::Unknown { error: Some(error), .. },
            ) if error.code == ErrorCode::ContentPolicy => SpeechOutcomeV1::Rejected { error },
            (Mutation::DeliverEmptyAudio, SpeechOutcomeV1::Unknown { reason, .. })
                if reason.contains("without audio")
                    && response.pointer("/body/json/data/audio/$south.blob/bytes")
                        == Some(&json!(0)) =>
            {
                delivered()
            }
            (_, outcome) => outcome,
        })
    }
}

/// Each `MiniMax`-specific mutation turns the shipped pack red on the cases that pin it.
#[test]
fn every_minimax_mutation_turns_the_pack_red() {
    let manifest = support::manifest(PACKAGE);
    let pack = support::pack(&manifest);
    for (mutation, cases) in [
        (
            Mutation::FallBackOnBadCharacters,
            &[
                "speech-v1.response.characters-zero",
                "speech-v1.response.characters-negative",
                "speech-v1.response.characters-fractional",
                "speech-v1.response.characters-string",
            ][..],
        ),
        (Mutation::DropGroupId, &["speech-v1.prepare.group-id-sent"][..]),
        (Mutation::ReleaseOutputViolation, &["speech-v1.response.base-resp-1026"][..]),
        (Mutation::DeliverEmptyAudio, &["speech-v1.response.empty-audio"][..]),
    ] {
        let report = run_speech_component_suite_v1(&Mutant(mutation), &pack, &manifest);
        let failed: Vec<String> = report.failures().map(|outcome| outcome.case.clone()).collect();
        for case in cases {
            assert!(failed.iter().any(|name| name == case), "{mutation:?}: {case} stayed green");
        }
    }
}
