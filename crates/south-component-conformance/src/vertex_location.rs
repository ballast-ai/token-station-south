//! The location rules the two Vertex AI references share: `embeddings-vertex` (embeddings record
//! §16) and `image-vertex` (image world step I3-3, design record
//! `docs/design/2026-10-13-image-i3-3-vertex.md` §4).
//!
//! Transcribed from token-station-server's `build_vertex_media_url` / `vertex_host_for_region` in
//! `gateway/src/modules/inference/engine/vertex.rs`. `region` and the project come from
//! `ProviderConfig.declared`: the `region` config key, and the `project` config key when the
//! operator set that override, else the `project_id` the credential exports. `base_url` is the
//! location's API origin; when it is a Vertex AI API origin it must be the one the region selects:
//! `https://{region}-aiplatform.googleapis.com`, except `https://aiplatform.googleapis.com` for
//! `global`, whose prefixed host answers 404. Any other base URL (a proxy, a test server) is used
//! as given. A missing value is a 500 naming the key, before any request refusal.
//!
//! One module, so the two references cannot drift.

use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

use crate::ComponentResultV1;

/// The `declared` keys a Vertex reference reads: the `region` and `project` config keys and the
/// `project_id` the credential exports.
const REGION: &str = "region";
const PROJECT_OVERRIDE: &str = "project";
const PROJECT_ID: &str = "project_id";

/// The location whose API host carries no region prefix.
const GLOBAL: &str = "global";
/// The suffix every Vertex AI API host shares.
const API_DOMAIN: &str = "aiplatform.googleapis.com";

fn internal(detail: impl std::fmt::Display) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string())
}

/// The API origin a location selects.
fn origin_of(region: &str) -> String {
    if region == GLOBAL {
        format!("https://{API_DOMAIN}")
    } else {
        format!("https://{region}-{API_DOMAIN}")
    }
}

/// The base URL the request goes under: `base_url`, which must be the region's origin when it is a
/// Vertex AI API origin at all.
pub fn base_of(config: &ProviderConfig, region: &str) -> ComponentResultV1<String> {
    let base = config.base_url.as_str();
    let base = base.trim_end_matches('/');
    let authority = base.split_once("://").map_or(base, |(_, rest)| rest);
    let host = authority.split(['/', ':']).next().unwrap_or_default();
    let is_vertex_host = host == API_DOMAIN || host.ends_with(&format!("-{API_DOMAIN}"));
    let expected = origin_of(region);
    if is_vertex_host && base != expected {
        return Err(internal(format!(
            "the base URL {base} is not the Vertex AI endpoint of location `{region}`, which is \
             {expected}"
        )));
    }
    Ok(base.to_owned())
}

/// A `declared` value the host must pass.
fn declared<'c>(config: &'c ProviderConfig, key: &str) -> ComponentResultV1<&'c str> {
    config.declared.get(key).ok_or_else(|| {
        internal(format!("the provider configuration carries no `{key}` for Vertex AI"))
    })
}

/// The location: the `region` config key.
pub fn region(config: &ProviderConfig) -> ComponentResultV1<&str> {
    declared(config, REGION)
}

/// The project: the `project` config key when the operator set it, else the exported `project_id`.
pub fn project(config: &ProviderConfig) -> ComponentResultV1<&str> {
    config.declared.get(PROJECT_OVERRIDE).map_or_else(|| declared(config, PROJECT_ID), Ok)
}

/// `projects/{project}/locations/{region}/publishers/google/models/{model}:{method}`, each of
/// `project`, `region` and `model` percent-encoded as one path segment, with the base URL it goes
/// under. The region is read first, then the project, then the base URL checked, so the error a
/// host sees for a row missing several values is the same in both references.
pub fn publisher_model_path(
    config: &ProviderConfig,
    model: &str,
    method: &str,
) -> ComponentResultV1<(String, String)> {
    let region = region(config)?;
    let project = project(config)?;
    let base = base_of(config, region)?;
    let path = format!(
        "v1/projects/{}/locations/{}/publishers/google/models/{}:{method}",
        crate::url_segment::encode(project),
        crate::url_segment::encode(region),
        crate::url_segment::encode(model)
    );
    Ok((base, path))
}
