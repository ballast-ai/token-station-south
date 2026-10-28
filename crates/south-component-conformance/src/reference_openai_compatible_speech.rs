//! The native reference implementation of the `speech-openai-compatible` component.
//!
//! The `OpenAI` speech protocol, `POST …/audio/speech` (speech world batch V3-1, design record
//! `docs/design/2026-10-14-speech-v3-1-tts.md` §2; speech record §12 row 1). Transcribed from
//! token-station-server's native catch-all arm `TtsUpstream::OpenaiCompat` (`handler/audio/
//! tts.rs:664-672`, decoded at `:1116-1143` and `tts_providers.rs:7-81`), which P21 DI4 moves into a
//! component:
//!
//! - **Family and auth**: `openai-compatible`, `bearer` on the configured slot (SV-Q1: no
//!   `azure-openai-v1` family in 1.0.0; the Azure families below arrive in 1.1.0).
//! - **Path**: the embeddings package's API-root rule — `audio/speech` under the binding's
//!   `base_url` when it has a path, `v1/audio/speech` when it is an origin (native
//!   `{endpoint}/v1/audio/speech` for an origin-only endpoint).
//! - **Body**: the client's JSON with `model` the upstream id; every other member forwarded as
//!   sent, an elided one as an `as_is` reference. When the host requires `tokens`, `stream_format`
//!   is `"sse"` (overriding the client's); otherwise a client `stream_format` goes as sent (SV-Q3).
//! - **Immutable paths** `input`, `model`, `stream_format`, `response_format` (native
//!   `body_rules`, `tts.rs:78`). No refusals beyond the common ones: native validates neither
//!   `voice` nor `response_format`.
//! - **Predicted media type**: `openai_tts_content_type` over the client's `response_format`
//!   lower-cased (absent = `mp3`).
//! - **Character-priced call**: `binary`, typed by the upstream's `content-type`; characters from
//!   the request; an empty body is `unknown`.
//! - **Token-priced call**: `sse` with `/*/data/audio` elided; a `segments` artifact over every
//!   `speech.audio.delta` event typed by the client's format, and `tokens` from the last valid
//!   `speech.audio.done` usage (the crate-private `openai_speech` reader).
//! - **Errors**: the `OpenAI` shape's `message`, else the body text, as the provider message.
//!
//! **Recognition** (speech world batch V3-3, design record `docs/design/2026-10-14-speech-v3-3-asr.md`
//! §2; speech record §12 ASR row 1), transcribed from the native fallback arm
//! `AsrUpstream::OpenaiCompat` (`handler/audio.rs:91-101`, `:226-235`, `:284-287`, `:559-564`):
//!
//! - **Operations**: `transcribe` for every configured model, `translate` on a row carrying
//!   `speech.translate`; `seconds` exactly. **Path**: the same API-root rule, `audio/transcriptions`
//!   or `audio/translations`.
//! - **Parts**: every client part forwarded in order; every text part named `model` set to the
//!   upstream id (appended when absent). Refused before dispatch: a body that is not multipart, an
//!   elided text part, no file part `file`, `stream=true` / `1`, a format outside the `OpenAI` set.
//! - **Verbose rows** (`speech.verbose_json`): `response_format` set to `verbose_json`, `json`
//!   response; every client format rendered from that answer, the reported `duration` billed and
//!   its absence `unknown`.
//! - **Other rows**: the client's format forwarded; a `json` answer passed through, billed by
//!   `duration`, else `usage.seconds` when `usage.type` is `"duration"`, else `seconds: null`; a
//!   text answer passed through with `seconds: null`.
//! - **Immutable form fields** `model`, `file`, `response_format`, `stream` (native `audio.rs:76`).
//!
//! **Azure `OpenAI`** (design record `docs/design/2026-10-15-speech-azure-openai.md`; SV-Q1 (a)'s
//! "later minor"), transcribed from the same two native arms reached on an `azure_openai` or
//! `azure_foundry` row, whose URL native's `build_upstream_url` rewrites
//! (`engine/upstream.rs:537-581`) and whose key it presents in `api-key` (`upstream.rs:190-196`).
//! Two families, the provider world's names, so one host row serves text and speech:
//!
//! - **`azure-openai-v1`**: the GA v1 surface. The `base_url` is `{endpoint}/openai/v1` (the
//!   embeddings package's family), so the API-root rule above gives `audio/speech`,
//!   `audio/transcriptions` and `audio/translations` below it; the deployment rides in the body's
//!   `model` (or the `model` part), as for `openai-compatible`.
//! - **`azure-openai-dated`**: the deployment-addressed surface,
//!   `openai/deployments/{deployment}/audio/{speech,transcriptions,translations}` below the row
//!   endpoint with the query `api-version={version}`; the deployment is the upstream model, one
//!   escaped segment; the version is the config value `api_version` (`api_version_date`, default
//!   `2024-10-21`, the provider world's key and default). An undated version is a 500 configuration
//!   error and an empty upstream model a 400, before any request refusal.
//! - **Both**: `header_secret` `api-key` on the configured slot; body, parts, metering, readers and
//!   errors exactly as `openai-compatible`, because native runs the same arms.

use serde_json::{Value, json};
use south_contracts::media::ResponseBodyFormV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechMeteringFormV1, SpeechModelCapabilitiesV1,
    SpeechOperationV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::ProviderConfig;

use crate::asr_shared::{OpenAiAsr, openai_outcome, prepare_openai};
use crate::openai_speech::{
    AUDIO_ELISION_PATH, client_format, forwarded_body, media_type_of, sse_outcome,
};
use crate::reference::azure_openai_dated;
use crate::speech_shared::{
    bearer, binary_outcome, capability, descriptor, facts, input, internal, json_body, non_2xx,
    openai_error_message, prepared, required_forms, scalar_count, view_status,
};
use crate::{ComponentResultV1, SpeechComponentV1, SpeechOutcomeV1};

/// The package name this reference is published as.
pub const NAME: &str = "speech-openai-compatible";
/// The package version.
pub const VERSION: &str = "1.1.12";
/// The world this reference exports.
pub const WORLD: &str = "speech-adapter-v1";
/// The family, the provider and embeddings worlds' name.
pub const OPENAI_COMPATIBLE: &str = "openai-compatible";
/// Azure `OpenAI`'s GA v1 surface, the provider and embeddings worlds' name.
pub const AZURE_OPENAI_V1: &str = "azure-openai-v1";
/// Azure `OpenAI`'s dated, deployment-addressed surface, the provider world's name.
pub const AZURE_OPENAI_DATED: &str = azure_openai_dated::FAMILY;
/// The header Azure `OpenAI` and Azure Foundry read the resource key from (native
/// `apply_provider_auth`, `upstream.rs:190-196`).
pub const AZURE_SECRET_HEADER: &str = "api-key";

const FORMS: [SpeechMeteringFormV1; 2] =
    [SpeechMeteringFormV1::Characters, SpeechMeteringFormV1::Tokens];
const IMMUTABLE_BODY_PATHS: [&str; 4] = ["input", "model", "stream_format", "response_format"];

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAiCompatibleSpeechReferenceV1;

/// The path of `endpoint` below the binding's `base_url`: its API root is its own path, or `v1`
/// when it is an origin (`reference_openai_compatible_embeddings`'s rule).
fn api_path(config: &ProviderConfig, endpoint: &str) -> String {
    let base = config.base_url.as_str();
    let rest = base.split_once("://").map_or(base.as_str(), |(_, rest)| rest);
    if rest.contains('/') { endpoint.to_owned() } else { format!("v1/{endpoint}") }
}

/// The family a configuration names, when this package serves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    OpenAiCompatible,
    AzureV1,
    AzureDated,
}

impl Family {
    fn of(config: &ProviderConfig) -> ComponentResultV1<Self> {
        match config.provider.as_str() {
            OPENAI_COMPATIBLE => Ok(Self::OpenAiCompatible),
            AZURE_OPENAI_V1 => Ok(Self::AzureV1),
            AZURE_OPENAI_DATED => Ok(Self::AzureDated),
            _ => Err(capability(format!(
                "this package serves the {OPENAI_COMPATIBLE}, {AZURE_OPENAI_V1} and \
                 {AZURE_OPENAI_DATED} families"
            ))),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => OPENAI_COMPATIBLE,
            Self::AzureV1 => AZURE_OPENAI_V1,
            Self::AzureDated => AZURE_OPENAI_DATED,
        }
    }
}

/// Where a call goes and how it is authenticated: path below the binding, query, auth arm.
struct Route {
    path: String,
    query: Option<Value>,
    auth: Option<Value>,
}

/// `{"arm": "header_secret", "header": "api-key", "slot": …}` on the configured slot.
fn azure_key(config: &ProviderConfig) -> Option<Value> {
    config.auth.as_ref().map(|slot| {
        json!({ "arm": "header_secret", "header": AZURE_SECRET_HEADER, "slot": slot.as_str() })
    })
}

/// The dated version: the config value, else native's default; anything not dated is a
/// configuration error (the provider world's `azure-openai-dated` rule, T9 record §4 D3).
fn dated_version(config: &ProviderConfig) -> ComponentResultV1<&str> {
    let version = config
        .declared
        .get(azure_openai_dated::API_VERSION_KEY)
        .unwrap_or(azure_openai_dated::DEFAULT_API_VERSION);
    if crate::reference::is_dated_api_version(version) {
        Ok(version)
    } else {
        Err(internal(format!(
            "the `{}` configuration value is not a dated API version",
            azure_openai_dated::API_VERSION_KEY
        )))
    }
}

/// The route of `endpoint` (`audio/speech`, `audio/transcriptions` or `audio/translations`) for
/// the family. The dated family's configuration and deployment are checked here, so they are
/// refused before any request refusal.
fn route(
    config: &ProviderConfig,
    family: Family,
    endpoint: &str,
    upstream_model: &str,
) -> ComponentResultV1<Route> {
    Ok(match family {
        Family::OpenAiCompatible => {
            Route { path: api_path(config, endpoint), query: None, auth: bearer(config) }
        }
        Family::AzureV1 => {
            Route { path: api_path(config, endpoint), query: None, auth: azure_key(config) }
        }
        Family::AzureDated => {
            let version = dated_version(config)?;
            if upstream_model.is_empty() {
                return Err(capability(
                    "Azure OpenAI addresses the deployment in the URL path, so a request without \
                     a model has no deployment to send to",
                ));
            }
            Route {
                path: format!(
                    "openai/deployments/{}/{endpoint}",
                    crate::url_segment::encode(upstream_model)
                ),
                query: Some(json!([{ "name": "api-version", "value": version }])),
                auth: azure_key(config),
            }
        }
    })
}

/// The upstream endpoint of an operation.
const fn endpoint_of(operation: SpeechOperationV1) -> &'static str {
    match operation {
        SpeechOperationV1::Synthesize => "audio/speech",
        SpeechOperationV1::Transcribe => "audio/transcriptions",
        SpeechOperationV1::Translate => "audio/translations",
    }
}

impl SpeechComponentV1 for OpenAiCompatibleSpeechReferenceV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: NAME.to_owned(),
            version: VERSION.to_owned(),
            api_version: WORLD.to_owned(),
        }
    }

    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<SpeechModelCapabilitiesV1>> {
        crate::asr_shared::capabilities(config, Family::of(config)?.name(), &FORMS, true)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        let family = Family::of(config)?;
        let route = route(config, family, endpoint_of(context.operation), &context.upstream_model)?;
        if context.operation.is_recognition() {
            let protocol = OpenAiAsr {
                path: route.path,
                query: route.query,
                drops_model: false,
                translates: true,
                auth: route.auth,
                upstream_model: &context.upstream_model,
            };
            return prepare_openai(config, request, context, &protocol);
        }
        let forms = required_forms(context, &FORMS)?;
        let body = json_body(request)?;
        let count = scalar_count(input(body)?);
        let format = client_format(body);
        let media_type = media_type_of(format.as_deref());
        let tokens = forms.contains(&SpeechMeteringFormV1::Tokens);
        let mut template = forwarded_body(body, &context.upstream_model);
        let mut state = json!({ "form": "binary", "format": format });
        let (form, elision): (_, &[&str]) = if tokens {
            template.insert("stream_format".to_owned(), json!("sse"));
            state["form"] = json!("sse");
            (ResponseBodyFormV1::Sse, &[AUDIO_ELISION_PATH])
        } else {
            (ResponseBodyFormV1::Binary, &[])
        };
        if forms.contains(&SpeechMeteringFormV1::Characters) {
            state["characters"] = json!(count);
        }
        prepared(
            facts(count, &forms, media_type),
            descriptor(&route.path, route.query, route.auth, &Value::Object(template), &[])?,
            form,
            elision,
            &IMMUTABLE_BODY_PATHS,
            &state,
        )
    }

    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        let status = view_status(response);
        if !(200..300).contains(&status) {
            return Ok(non_2xx(status, response, openai_error_message));
        }
        if state.get("op").is_some() {
            Ok(openai_outcome(state, response, true))
        } else if state["form"] == "sse" {
            sse_outcome(state, response)
        } else {
            binary_outcome(state, response)
        }
    }
}
