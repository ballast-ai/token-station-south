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
use crate::speech_shared::{
    bearer, binary_outcome, descriptor, facts, input, json_body, non_2xx, openai_error_message,
    prepared, required_forms, scalar_count, serves, view_status,
};
use crate::{ComponentResultV1, SpeechComponentV1, SpeechOutcomeV1};

/// The package name this reference is published as.
pub const NAME: &str = "speech-openai-compatible";
/// The package version.
pub const VERSION: &str = "1.0.1";
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

/// The path of `endpoint` below the binding's `base_url`: its API root is its own path, or `v1`
/// when it is an origin (`reference_openai_compatible_embeddings`'s rule).
fn api_path(config: &ProviderConfig, endpoint: &str) -> String {
    let base = config.base_url.as_str();
    let rest = base.split_once("://").map_or(base.as_str(), |(_, rest)| rest);
    if rest.contains('/') { endpoint.to_owned() } else { format!("v1/{endpoint}") }
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
        crate::asr_shared::capabilities(config, OPENAI_COMPATIBLE, &FORMS, true)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        serves(config, OPENAI_COMPATIBLE)?;
        if context.operation.is_recognition() {
            let endpoint = if context.operation == SpeechOperationV1::Translate {
                "audio/translations"
            } else {
                "audio/transcriptions"
            };
            let protocol = OpenAiAsr {
                path: api_path(config, endpoint),
                drops_model: false,
                translates: true,
                auth: bearer(config),
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
            descriptor(
                &api_path(config, "audio/speech"),
                None,
                bearer(config),
                &Value::Object(template),
                &[],
            )?,
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
