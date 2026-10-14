//! The native reference implementation of the `speech-openai-compatible` component.
//!
//! The `OpenAI` speech protocol, `POST …/audio/speech` (speech world batch V3-1, design record
//! `docs/design/2026-10-14-speech-v3-1-tts.md` §2; speech record §12 row 1). Transcribed from
//! token-station-server's native catch-all arm `TtsUpstream::OpenaiCompat` (`handler/audio/
//! tts.rs:664-672`, decoded at `:1116-1143` and `tts_providers.rs:7-81`), which P21 DI4 moves into a
//! component:
//!
//! - **Family and auth**: `openai-compatible`, `bearer` on the configured slot (SV-Q1: no
//!   `azure-openai-v1` family in 1.0.0).
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

use serde_json::{Value, json};
use south_contracts::media::ResponseBodyFormV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechMeteringFormV1, SpeechModelCapabilitiesV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::ProviderConfig;

use crate::openai_speech::{
    AUDIO_ELISION_PATH, client_format, forwarded_body, media_type_of, sse_outcome,
};
use crate::speech_shared::{
    bearer, binary_outcome, capabilities, descriptor, facts, input, json_body, non_2xx,
    openai_error_message, prepared, required_forms, scalar_count, serves, view_status,
};
use crate::{ComponentResultV1, SpeechComponentV1, SpeechOutcomeV1};

/// The package name this reference is published as.
pub const NAME: &str = "speech-openai-compatible";
/// The package version.
pub const VERSION: &str = "1.0.0";
/// The world this reference exports.
pub const WORLD: &str = "speech-adapter-v1";
/// The family, the provider and embeddings worlds' name.
pub const OPENAI_COMPATIBLE: &str = "openai-compatible";

const FORMS: [SpeechMeteringFormV1; 2] =
    [SpeechMeteringFormV1::Characters, SpeechMeteringFormV1::Tokens];
const IMMUTABLE_BODY_PATHS: [&str; 4] = ["input", "model", "stream_format", "response_format"];

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAiCompatibleSpeechReferenceV1;

/// The path below the binding's `base_url`: its API root is its own path, or `v1` when it is an
/// origin (`reference_openai_compatible_embeddings`'s rule).
fn speech_path(config: &ProviderConfig) -> &'static str {
    let base = config.base_url.as_str();
    let rest = base.split_once("://").map_or(base.as_str(), |(_, rest)| rest);
    if rest.contains('/') { "audio/speech" } else { "v1/audio/speech" }
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
        capabilities(config, OPENAI_COMPATIBLE, &FORMS)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        serves(config, OPENAI_COMPATIBLE)?;
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
            descriptor(speech_path(config), None, bearer(config), &Value::Object(template), &[])?,
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
        if state["form"] == "sse" {
            sse_outcome(state, response)
        } else {
            binary_outcome(state, response)
        }
    }
}
