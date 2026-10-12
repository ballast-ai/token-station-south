//! `south.image-component.v1` against the `image-minimax` native reference and its shipped fixture
//! pack, and the behaviours of design record `2026-10-12-image-i3-1-minimax-and-qwen.md` §2 pinned
//! by name.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::image_fixture::ImageFixturePackV1;
use south_component_conformance::reference_minimax_image::MiniMaxImageReferenceV1;
use south_component_conformance::{ImageComponentV1, ImageOutcomeV1, run_image_component_suite_v1};
use south_contracts::image::ImageCallContextV1;
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{ErrorCode, ProviderConfig};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("../../components/image-minimax/manifest.json"))
            .unwrap(),
    )
    .unwrap()
}

fn pack() -> ImageFixturePackV1 {
    ImageFixturePackV1::load(&root().join("fixtures-image-minimax")).unwrap()
}

fn config(group_id: Option<&str>) -> ProviderConfig {
    let mut config = json!({
        "provider": "minimax-image",
        "base_url": "https://api.minimax.io",
        "auth": "provider_api_key",
    });
    if let Some(group_id) = group_id {
        config["declared"] = json!({ "group_id": group_id });
    }
    serde_json::from_value(config).unwrap()
}

fn context() -> ImageCallContextV1 {
    serde_json::from_value(json!({"upstream_model": "image-01", "metering_required": ["images"]}))
        .unwrap()
}

/// The prepared call as JSON, or the refusal.
fn prepare(config: &ProviderConfig, body: &Value) -> Result<Value, Value> {
    MiniMaxImageReferenceV1
        .prepare(config, &json!({ "json": body }), &context())
        .map(|prepared| serde_json::from_str(&prepared.to_json()).unwrap())
        .map_err(|error| serde_json::to_value(error).unwrap())
}

fn answer(status: u16, body: &Value) -> ImageOutcomeV1 {
    let view = json!({"status": status, "headers": [], "body": {"json": body}});
    MiniMaxImageReferenceV1.parse_response(&json!({}), &view).unwrap()
}

#[test]
fn the_shipped_manifest_passes_gate_one() {
    assert_eq!(manifest().validate(), Ok(()));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let report = run_image_component_suite_v1(&MiniMaxImageReferenceV1, &pack(), &manifest());
    let failures: Vec<String> = report
        .failures()
        .map(|outcome| format!("{} {}: {}", outcome.check.as_str(), outcome.case, outcome.detail()))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// IM-Q3: `?GroupId=` only when the family's `group_id` config key is set.
#[test]
fn the_group_id_is_sent_only_when_configured() {
    let body = json!({"prompt": "x"});
    let sent = prepare(&config(Some("19000")), &body).unwrap();
    assert_eq!(sent["descriptor"]["query"], json!([{"name": "GroupId", "value": "19000"}]));
    let absent = prepare(&config(None), &body).unwrap();
    assert!(absent["descriptor"].get("query").is_none(), "{absent}");
}

/// §2: `size` is dropped (`MiniMax` has no size) and is never a fact (§5 item 3).
#[test]
fn the_size_is_neither_forwarded_nor_a_fact() {
    let prepared = prepare(&config(None), &json!({"prompt": "x", "size": "1024x1024"})).unwrap();
    assert!(prepared["descriptor"]["body"]["json"]["template"].get("size").is_none());
    assert!(prepared["facts"].get("size").is_none(), "{prepared}");
}

/// §2 and IM-Q4 / IM-Q6: a 2xx whose `base_resp` names a failure carries the host's status for the
/// code; only the codes meaning "refused before output" are `rejected`.
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
        let outcome = answer(200, &json!({"base_resp": {"status_code": code, "status_msg": "m"}}));
        let ((ImageOutcomeV1::Rejected { error }, true)
        | (ImageOutcomeV1::Unknown { error: Some(error), .. }, false)) = (&outcome, rejected)
        else {
            panic!("{code}: {outcome:?}")
        };
        assert_eq!((error.http_status, error.code), (status, error_code), "{code}");
    }
}

/// IM-Q4: an output violation may have been charged, so it parks rather than releases.
#[test]
fn an_output_violation_is_unknown() {
    let outcome = answer(200, &json!({"base_resp": {"status_code": 1026}}));
    assert!(matches!(outcome, ImageOutcomeV1::Unknown { .. }), "{outcome:?}");
}

/// IM-Q5 and §2: URLs before inline images, inline images `image/png`; `images_reported` is the
/// upstream's count as reported, never capped here (Q9 keeps the rule in the host).
#[test]
fn artifacts_list_urls_first_and_the_count_is_reported_as_is() {
    let outcome = answer(
        200,
        &json!({
            "data": {
                "image_base64": [{"$south.blob": {"id": "b0", "bytes": 10, "head": "iVBO"}}],
                "image_urls": ["https://example.com/a.png"],
            },
            "metadata": {"success_count": "7"},
        }),
    );
    let ImageOutcomeV1::Succeeded { artifacts, metering, .. } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        serde_json::to_value(&artifacts).unwrap(),
        json!([
            {"form": "url", "pointer": "/data/image_urls/0"},
            {"form": "inline", "pointer": "/data/image_base64/0", "encoding": "base64", "media_type": "image/png"},
        ])
    );
    assert_eq!(metering.images_reported, Some(7));
}

mod mutations {
    //! Each required row fails on a component that breaks it, so a passing report means the row
    //! was checked, not skipped.

    use super::{manifest, pack};
    use serde_json::{Value, json};
    use south_component_conformance::reference_minimax_image::MiniMaxImageReferenceV1;
    use south_component_conformance::{
        CheckV1, ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1,
        run_image_component_suite_v1,
    };
    use south_contracts::image::{
        ImageCallContextV1, ImageModelCapabilitiesV1, ImageRenderContextV1, PreparedImageCallV1,
    };
    use south_provider_api::ComponentMetadataV1;
    use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

    #[derive(Clone, Copy)]
    enum Break {
        RetriableAuth,
        BlobInExtras,
        UrlDeliveryOfInline,
        InternalRefusal,
        CountForMissingEvidence,
    }

    struct Broken(Break);

    impl ImageComponentV1 for Broken {
        fn metadata(&self) -> ComponentMetadataV1 {
            MiniMaxImageReferenceV1.metadata()
        }
        fn model_capabilities(
            &self,
            config: &ProviderConfig,
        ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
            MiniMaxImageReferenceV1.model_capabilities(config)
        }
        fn prepare(
            &self,
            config: &ProviderConfig,
            request: &Value,
            context: &ImageCallContextV1,
        ) -> ComponentResultV1<PreparedImageCallV1> {
            MiniMaxImageReferenceV1.prepare(config, request, context).map_err(|error| {
                match self.0 {
                    Break::InternalRefusal => {
                        ErrorEnvelope::new(ErrorCode::Internal, 500, "refused")
                    }
                    _ => error,
                }
            })
        }
        fn parse_response(
            &self,
            state: &Value,
            response: &Value,
        ) -> ComponentResultV1<ImageOutcomeV1> {
            let outcome = MiniMaxImageReferenceV1.parse_response(state, response)?;
            Ok(match (self.0, outcome) {
                (Break::RetriableAuth, ImageOutcomeV1::Rejected { mut error })
                    if error.code == ErrorCode::Auth =>
                {
                    error.code = ErrorCode::UpstreamUnavailable;
                    ImageOutcomeV1::Rejected { error }
                }
                (Break::BlobInExtras, ImageOutcomeV1::Succeeded { artifacts, metering, .. }) => {
                    ImageOutcomeV1::Succeeded {
                        artifacts,
                        metering,
                        extras: json!({"$south.blob": {"id": "b0", "bytes": 1, "head": "x"}}),
                    }
                }
                (
                    Break::CountForMissingEvidence,
                    ImageOutcomeV1::Succeeded { artifacts, mut metering, extras },
                ) => {
                    metering.images_reported = metering.images_reported.or(Some(1));
                    ImageOutcomeV1::Succeeded { artifacts, metering, extras }
                }
                (_, outcome) => outcome,
            })
        }
        fn render(
            &self,
            state: &Value,
            outcomes: &[ImageOutcomeV1],
            context: &ImageRenderContextV1,
        ) -> ComponentResultV1<ImageRenderedV1> {
            let rendered = MiniMaxImageReferenceV1.render(state, outcomes, context)?;
            Ok(match self.0 {
                Break::UrlDeliveryOfInline => ImageRenderedV1 {
                    template: rendered.template.replace("\"as\":\"b64_json\"", "\"as\":\"url\""),
                },
                _ => rendered,
            })
        }
    }

    #[test]
    fn each_row_bites() {
        for (broken, check) in [
            (Break::RetriableAuth, CheckV1::AuthErrorsAreNotRetriable),
            (Break::BlobInExtras, CheckV1::ReferenceIntegrity),
            (Break::UrlDeliveryOfInline, CheckV1::ReferenceIntegrity),
            (Break::InternalRefusal, CheckV1::PreDispatchRefusal),
            (Break::CountForMissingEvidence, CheckV1::EvidenceAbsentIsNull),
        ] {
            let report = run_image_component_suite_v1(&Broken(broken), &pack(), &manifest());
            let failing: Vec<CheckV1> = report.failures().map(|outcome| outcome.check).collect();
            assert!(failing.contains(&check), "{check:?} did not fail: {failing:?}");
        }
    }
}
