//! `south.image-component.v1` against the `image-qwen` native reference and its shipped fixture
//! pack, and the behaviours of design record `2026-10-12-image-i3-1-minimax-and-qwen.md` §3 pinned
//! by name.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::image_fixture::ImageFixturePackV1;
use south_component_conformance::reference_qwen_image::QwenImageReferenceV1;
use south_component_conformance::{ImageComponentV1, ImageOutcomeV1, run_image_component_suite_v1};
use south_contracts::image::ImageCallContextV1;
use south_provider_api::ComponentManifestV1;
use token_station_protocol::ProviderConfig;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("../../components/image-qwen/manifest.json")).unwrap(),
    )
    .unwrap()
}

fn pack() -> ImageFixturePackV1 {
    ImageFixturePackV1::load(&root().join("fixtures-image-qwen")).unwrap()
}

fn config() -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": "bailian-qwen-image",
        "base_url": "https://dashscope.aliyuncs.com",
        "auth": "provider_api_key",
    }))
    .unwrap()
}

fn prepare(body: &Value) -> Value {
    let context: ImageCallContextV1 = serde_json::from_value(
        json!({"upstream_model": "qwen-image-plus", "metering_required": ["images"]}),
    )
    .unwrap();
    let prepared = QwenImageReferenceV1.prepare(&config(), &json!({ "json": body }), &context);
    serde_json::from_str(&prepared.unwrap().to_json()).unwrap()
}

fn answer(status: u16, body: &Value) -> ImageOutcomeV1 {
    let view = json!({"status": status, "headers": [], "body": {"json": body}});
    QwenImageReferenceV1.parse_response(&json!({}), &view).unwrap()
}

#[test]
fn the_shipped_manifest_passes_gate_one() {
    assert_eq!(manifest().validate(), Ok(()));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let report = run_image_component_suite_v1(&QwenImageReferenceV1, &pack(), &manifest());
    let failures: Vec<String> = report
        .failures()
        .map(|outcome| format!("{} {}: {}", outcome.check.as_str(), outcome.case, outcome.detail()))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// §3 and §5 item 3: a `size` fact only when the client sent one; the upstream's default would
/// move an unsized request into another price tier.
#[test]
fn the_size_fact_exists_only_when_the_client_sent_a_size() {
    assert!(prepare(&json!({"prompt": "x"}))["facts"].get("size").is_none());
    for size in ["1328x1328", "1328*1328", "1328X1328"] {
        let prepared = prepare(&json!({"prompt": "x", "size": size}));
        assert_eq!(prepared["facts"]["size"], json!({"width": 1328, "height": 1328}), "{size}");
        assert_eq!(
            prepared["descriptor"]["body"]["json"]["template"]["parameters"]["size"],
            json!("1328*1328"),
            "{size}"
        );
    }
    assert!(prepare(&json!({"prompt": "x", "size": "1K"}))["facts"].get("size").is_none());
}

/// South #297 (server 03 #142): a resolution tier word on every request, in the card's spelling,
/// so the host prices the model's tier price table and its `input_each` per input image.
#[test]
fn every_request_reports_a_resolution_tier_word() {
    let cases = [
        (None, "1K"),
        (Some("1328*1328"), "1K"),
        (Some("1664*928"), "1K"),
        (Some("2048x1024"), "1K"),
        (Some("2048*1152"), "2K"),
        (Some("2048*2048"), "2K"),
        (Some("1k"), "1K"),
        (Some("2k"), "2K"),
        (Some("2K"), "2K"),
        (Some("4K"), "4K"),
        (Some("large"), "1K"),
    ];
    for (size, word) in cases {
        let mut body = json!({"prompt": "x", "image_url": ["https://example.com/a.png"]});
        if let Some(size) = size {
            body["size"] = json!(size);
        }
        let facts = &prepare(&body)["facts"];
        assert_eq!(facts["tier"], json!({"resolution": word}), "{size:?}");
        assert_eq!(facts["inputs"]["input_image"], json!(1), "{size:?}");
    }
    let capabilities = QwenImageReferenceV1.model_capabilities(&config()).unwrap();
    let declared: Vec<Value> = capabilities
        .iter()
        .map(|model| serde_json::to_value(&model.tier_dimensions).unwrap())
        .collect();
    assert!(declared.iter().all(|dimensions| *dimensions == json!(["resolution"])), "{declared:?}");
}

/// §3: the synchronous endpoint, never the asynchronous header the Wan task package sends.
#[test]
fn the_request_is_synchronous() {
    let prepared = prepare(&json!({"prompt": "x", "X-DashScope-Async": "enable"}));
    let descriptor = &prepared["descriptor"];
    assert_eq!(descriptor["path"], "api/v1/services/aigc/multimodal-generation/generation");
    assert!(descriptor.get("headers").is_none_or(|headers| {
        !headers.to_string().to_ascii_lowercase().contains("x-dashscope-async")
    }));
    assert!(descriptor["body"]["json"]["template"].get("X-DashScope-Async").is_none());
}

/// §3 and record §6 item 4: a 2xx carrying a `DashScope` `code` failed, though images may follow.
#[test]
fn a_2xx_carrying_a_code_is_unknown() {
    let outcome = answer(
        200,
        &json!({
            "code": "InternalError",
            "message": "m",
            "output": {"choices": [{"message": {"content": [{"image": "https://example.com/a.png"}]}}]},
        }),
    );
    assert!(matches!(outcome, ImageOutcomeV1::Unknown { error: Some(_), .. }), "{outcome:?}");
}

/// §3: images flattened across choices as `url` pointers; `images_reported` as reported (Q9
/// keeps the "≤ delivered + 1" rule in the host).
#[test]
fn every_image_of_every_choice_is_a_url_artifact() {
    let outcome = answer(
        200,
        &json!({
            "output": {"choices": [
                {"message": {"content": [{"image": "https://example.com/a.png"}]}},
                {"message": {"content": [{"text": "t"}, {"image": "https://example.com/b.png"}]}},
            ]},
            "usage": {"image_count": 9},
        }),
    );
    let ImageOutcomeV1::Succeeded { artifacts, metering, .. } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        serde_json::to_value(&artifacts).unwrap(),
        json!([
            {"form": "url", "pointer": "/output/choices/0/message/content/0/image"},
            {"form": "url", "pointer": "/output/choices/1/message/content/1/image"},
        ])
    );
    assert_eq!(metering.images_reported, Some(9));
}

mod mutations {
    //! Each required row fails on a component that breaks it.

    use super::{manifest, pack};
    use serde_json::{Value, json};
    use south_component_conformance::reference_qwen_image::QwenImageReferenceV1;
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
        DanglingArtifact,
        InternalRefusal,
        CountForMissingEvidence,
    }

    struct Broken(Break);

    impl ImageComponentV1 for Broken {
        fn metadata(&self) -> ComponentMetadataV1 {
            QwenImageReferenceV1.metadata()
        }
        fn model_capabilities(
            &self,
            config: &ProviderConfig,
        ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
            QwenImageReferenceV1.model_capabilities(config)
        }
        fn prepare(
            &self,
            config: &ProviderConfig,
            request: &Value,
            context: &ImageCallContextV1,
        ) -> ComponentResultV1<PreparedImageCallV1> {
            QwenImageReferenceV1.prepare(config, request, context).map_err(|error| match self.0 {
                Break::InternalRefusal => ErrorEnvelope::new(ErrorCode::Internal, 500, "refused"),
                _ => error,
            })
        }
        fn parse_response(
            &self,
            state: &Value,
            response: &Value,
        ) -> ComponentResultV1<ImageOutcomeV1> {
            let outcome = QwenImageReferenceV1.parse_response(state, response)?;
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
            let rendered = QwenImageReferenceV1.render(state, outcomes, context)?;
            Ok(match self.0 {
                Break::DanglingArtifact => ImageRenderedV1 {
                    template: rendered.template.replace("\"index\":0", "\"index\":99"),
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
            (Break::DanglingArtifact, CheckV1::ReferenceIntegrity),
            (Break::InternalRefusal, CheckV1::PreDispatchRefusal),
            (Break::CountForMissingEvidence, CheckV1::EvidenceAbsentIsNull),
        ] {
            let report = run_image_component_suite_v1(&Broken(broken), &pack(), &manifest());
            let failing: Vec<CheckV1> = report.failures().map(|outcome| outcome.check).collect();
            assert!(failing.contains(&check), "{check:?} did not fail: {failing:?}");
        }
    }
}
