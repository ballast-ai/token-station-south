//! `south.image-component.v1` against the `image-azure` native reference and its shipped fixture
//! pack (image record §12.1, §19 S-I-6).

use std::path::Path;

use south_component_conformance::image_fixture::ImageFixturePackV1;
use south_component_conformance::reference_azure_image::AzureImageReferenceV1;
use south_component_conformance::run_image_component_suite_v1;
use south_provider_api::ComponentManifestV1;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest() -> ComponentManifestV1 {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("../../components/image-azure/manifest.json"))
            .unwrap(),
    )
    .unwrap()
}

fn pack() -> ImageFixturePackV1 {
    ImageFixturePackV1::load(&root().join("fixtures-image-azure")).unwrap()
}

#[test]
fn the_shipped_manifest_passes_gate_one() {
    assert_eq!(manifest().validate(), Ok(()));
}

#[test]
fn the_reference_passes_its_own_pack() {
    let report = run_image_component_suite_v1(&AzureImageReferenceV1, &pack(), &manifest());
    let failures: Vec<String> = report
        .failures()
        .map(|outcome| format!("{} {}: {}", outcome.check.as_str(), outcome.case, outcome.detail()))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

mod mutations {
    //! Each required row fails on a component that breaks it, so a passing report means the row
    //! was checked, not skipped.

    use super::{manifest, pack};
    use serde_json::{Value, json};
    use south_component_conformance::reference_azure_image::AzureImageReferenceV1;
    use south_component_conformance::{
        CheckV1, ComponentResultV1, ImageComponentV1, ImageOutcomeV1, ImageRenderedV1,
        run_image_component_suite_v1,
    };
    use south_contracts::image::{
        ImageCallContextV1, ImageMeteringV1, ImageModelCapabilitiesV1, ImageRenderContextV1,
        ImageTokenUsageV1, PreparedImageCallV1,
    };
    use south_provider_api::ComponentMetadataV1;
    use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

    #[derive(Clone, Copy)]
    enum Break {
        ZeroForMissingUsage,
        RetriableAuth,
        BlobInExtras,
        UrlDelivery,
        InternalRefusal,
    }

    struct Broken(Break);

    impl ImageComponentV1 for Broken {
        fn metadata(&self) -> ComponentMetadataV1 {
            AzureImageReferenceV1.metadata()
        }
        fn model_capabilities(
            &self,
            config: &ProviderConfig,
        ) -> ComponentResultV1<Vec<ImageModelCapabilitiesV1>> {
            AzureImageReferenceV1.model_capabilities(config)
        }
        fn prepare(
            &self,
            config: &ProviderConfig,
            request: &Value,
            context: &ImageCallContextV1,
        ) -> ComponentResultV1<PreparedImageCallV1> {
            AzureImageReferenceV1.prepare(config, request, context).map_err(|error| match self.0 {
                Break::InternalRefusal => ErrorEnvelope::new(ErrorCode::Internal, 500, "refused"),
                _ => error,
            })
        }
        fn parse_response(
            &self,
            state: &Value,
            response: &Value,
        ) -> ComponentResultV1<ImageOutcomeV1> {
            let outcome = AzureImageReferenceV1.parse_response(state, response)?;
            Ok(match (self.0, outcome) {
                (Break::ZeroForMissingUsage, ImageOutcomeV1::Unknown { reason, .. })
                    if reason.contains("declared token bucket") =>
                {
                    ImageOutcomeV1::Succeeded {
                        artifacts: serde_json::from_value(json!([{"form":"inline","pointer":"/data/0/b64_json","encoding":"base64","media_type":"image/png"}])).unwrap(),
                        metering: ImageMeteringV1 {
                            tokens: Some(ImageTokenUsageV1 { total_input: Some(0), total_output: Some(0), ..ImageTokenUsageV1::default() }),
                            ..ImageMeteringV1::default()
                        },
                        extras: Value::Null,
                    }
                }
                (Break::RetriableAuth, ImageOutcomeV1::Rejected { mut error }) if error.code == ErrorCode::Auth => {
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
                (_, outcome) => outcome,
            })
        }
        fn render(
            &self,
            state: &Value,
            outcomes: &[ImageOutcomeV1],
            context: &ImageRenderContextV1,
        ) -> ComponentResultV1<ImageRenderedV1> {
            let rendered = AzureImageReferenceV1.render(state, outcomes, context)?;
            Ok(match self.0 {
                Break::UrlDelivery => ImageRenderedV1 {
                    template: rendered.template.replace("\"as\":\"b64_json\"", "\"as\":\"url\""),
                },
                _ => rendered,
            })
        }
    }

    fn failing_checks(broken: Break) -> Vec<CheckV1> {
        let report = run_image_component_suite_v1(&Broken(broken), &pack(), &manifest());
        report.failures().map(|outcome| outcome.check).collect()
    }

    #[test]
    fn each_row_bites() {
        for (broken, check) in [
            (Break::ZeroForMissingUsage, CheckV1::MissingMeterIsNotZero),
            (Break::RetriableAuth, CheckV1::AuthErrorsAreNotRetriable),
            (Break::BlobInExtras, CheckV1::ReferenceIntegrity),
            (Break::UrlDelivery, CheckV1::ReferenceIntegrity),
            (Break::InternalRefusal, CheckV1::PreDispatchRefusal),
        ] {
            let failing = failing_checks(broken);
            assert!(failing.contains(&check), "{check:?} did not fail: {failing:?}");
        }
    }
}
