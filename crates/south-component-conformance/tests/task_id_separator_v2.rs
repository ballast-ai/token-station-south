//! Task ids never travel as `%2F` (kernel re-pin to protocol 0.5.0,
//! `docs/design/2026-10-08-kernel-repin-protocol-0.5.0.md` §4).
//!
//! Every task-v2 component that encodes an upstream task id as one observe path segment refuses an
//! id containing `/` before it builds a request. Protocol 0.5.0 made the kernel's endpoint gate
//! admit `%2F` inside one segment, for ARN model ids; no task id needs a `/`, and many upstreams
//! decode `%2F` as a path separator, so the components keep the refusal the 0.4.0 gate gave them.

use serde_json::json;
use south_component_conformance::TaskComponentV2;
use south_component_conformance::reference_bailian_task_v2::BailianTaskComponentV2;
use south_component_conformance::reference_byteplus_task_v2::BytePlusTaskComponentV2;
use south_component_conformance::reference_gmi_image_task_v2::GmiImageTaskComponentV2;
use south_component_conformance::reference_kling_task_v2::KlingTaskReferenceV2;
use south_component_conformance::reference_minimax_task_v2::MiniMaxTaskReferenceV2;
use south_component_conformance::reference_wan_image_task_v2::WanImageTaskComponentV2;
use south_component_conformance::reference_xai_task_v2::XaiTaskComponentV2;
use south_contracts::TaskLocatorV2;
use token_station_protocol::ProviderConfig;

fn config(provider: &str, base_url: &str) -> ProviderConfig {
    serde_json::from_value(
        json!({ "provider": provider, "base_url": base_url, "auth": "provider_api_key" }),
    )
    .unwrap()
}

#[test]
fn every_segment_encoding_task_component_refuses_a_slash_in_the_task_id() {
    let cases: [(&str, &dyn TaskComponentV2, ProviderConfig, &str); 7] = [
        (
            "task-kling-v2",
            &KlingTaskReferenceV2,
            config("kling", "https://api.kling.example"),
            "v1/videos/text2video",
        ),
        (
            "task-minimax-v2",
            &MiniMaxTaskReferenceV2,
            config("minimax", "https://api.minimax.io"),
            "v2/query/video_generation",
        ),
        (
            "task-bailian-v2",
            &BailianTaskComponentV2,
            config("bailian", "https://dashscope.example"),
            "api/v1/tasks",
        ),
        (
            "task-byteplus-v2",
            &BytePlusTaskComponentV2,
            config("byteplus", "https://ark.example"),
            "api/v3/contents/generations/tasks",
        ),
        ("task-xai-v2", &XaiTaskComponentV2, config("xai", "https://api.x.example"), "v1/videos"),
        (
            "task-wan-image-v2",
            &WanImageTaskComponentV2,
            config("bailian", "https://dashscope.example"),
            "api/v1/tasks",
        ),
        (
            "task-gmi-image-v2",
            &GmiImageTaskComponentV2,
            config("gmi", "https://console.gmi.example"),
            "api/v1/ie/requestqueue/apikey/requests",
        ),
    ];
    for (name, component, config, route) in cases {
        let locator = TaskLocatorV2::new(1, route).unwrap();
        let plain = component
            .build_observe_request(&config, "model", "task-1", &locator)
            .unwrap_or_else(|error| panic!("{name}: a plain id builds: {error:?}"));
        config.authorize(&plain).unwrap_or_else(|error| panic!("{name}: {error:?}"));
        for id in ["a/b", "/a", "a/", "a%2Fb/c"] {
            assert!(
                component.build_observe_request(&config, "model", id, &locator).is_err(),
                "{name}: `{id}` must be refused before a request is built"
            );
        }
    }
}
