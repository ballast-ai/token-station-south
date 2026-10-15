//! The native reference implementation of the `speech-groq` component.
//!
//! Groq Orpheus text to speech (speech world batch V3-1, design record
//! `docs/design/2026-10-14-speech-v3-1-tts.md` §4; speech record §12 row 3). Transcribed from
//! token-station-server's native arm `TtsUpstream::GroqOrpheus` (`handler/audio/tts.rs:621-637`)
//! and its persona tables `resolve_orpheus_voice` / `map_openai_voice_to_orpheus`
//! (`tts_providers.rs:334-368`):
//!
//! - **Request**: `POST v1/audio/speech` under the row endpoint, `bearer` on the configured slot,
//!   the client's JSON with `model` the upstream id and `voice` the persona; every other member is
//!   forwarded as sent (no `stream_format` handling: native has none for this arm).
//! - **Persona**: Arabic when the upstream id lower-cased contains `arabic`, else English (native
//!   reads the model id). `voice` absent, not a string or blank after trimming is `noura` (Arabic)
//!   or `autumn` (English); otherwise the trimmed value lower-cased through the native table, and
//!   anything else is sent as trimmed, case kept.
//! - **Immutable paths** `input`, `model`, `voice` (native `body_rules`, `tts.rs:76`). Characters
//!   from the request.
//! - **Response**: `binary`, typed by the upstream's `content-type`; an empty body is `unknown`. A
//!   non-2xx carries the `OpenAI` error shape's `message` as the provider message.

use serde_json::{Value, json};
use south_contracts::media::ResponseBodyFormV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechMeteringFormV1, SpeechModelCapabilitiesV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::ProviderConfig;

use crate::openai_speech::forwarded_body;
use crate::speech_shared::{
    FALLBACK_MEDIA_TYPE, bearer, binary_outcome, capabilities, descriptor, facts, input, is_blob,
    json_body, non_2xx, openai_error_message, prepared, required_forms, scalar_count, serves,
    view_status, with_refs,
};
use crate::{ComponentResultV1, SpeechComponentV1, SpeechOutcomeV1};

/// The package name this reference is published as.
pub const NAME: &str = "speech-groq";
/// The package version.
pub const VERSION: &str = "1.0.1";
/// The world this reference exports.
pub const WORLD: &str = "speech-adapter-v1";
/// The family, as a host's speech bindings name it.
pub const GROQ_SPEECH: &str = "groq-speech";

const FORMS: [SpeechMeteringFormV1; 1] = [SpeechMeteringFormV1::Characters];
const IMMUTABLE_BODY_PATHS: [&str; 3] = ["input", "model", "voice"];

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct GroqSpeechReferenceV1;

/// `map_openai_voice_to_orpheus`: a persona or an `OpenAI` name, in any case, to a lower-case persona
/// of the language; anything else as given.
fn persona(voice: &str, arabic: bool) -> &str {
    match (arabic, voice.to_lowercase().as_str()) {
        (true, "lulwa" | "coral" | "sage") => "lulwa",
        (true, "noura" | "alloy" | "nova") => "noura",
        (true, "aisha" | "shimmer") => "aisha",
        (true, "abdullah" | "fable" | "ash") => "abdullah",
        (true, "fahad" | "echo" | "ballad") => "fahad",
        (true, "sultan" | "onyx" | "verse") => "sultan",
        (false, "autumn" | "alloy" | "nova") => "autumn",
        (false, "diana" | "coral" | "sage") => "diana",
        (false, "hannah" | "shimmer") => "hannah",
        (false, "austin" | "fable" | "ash") => "austin",
        (false, "daniel" | "echo" | "ballad") => "daniel",
        (false, "troy" | "onyx" | "verse") => "troy",
        _ => voice,
    }
}

/// `resolve_orpheus_voice`.
fn resolved_voice(voice: Option<&Value>, upstream_model: &str) -> Value {
    let arabic = upstream_model.to_lowercase().contains("arabic");
    let default = if arabic { "noura" } else { "autumn" };
    match voice {
        Some(Value::String(voice)) => {
            let trimmed = voice.trim();
            if trimmed.is_empty() { json!(default) } else { json!(persona(trimmed, arabic)) }
        }
        // An elided voice is no persona: forwarded verbatim, as native forwards an unknown one.
        Some(value) if is_blob(value) => with_refs(value),
        _ => json!(default),
    }
}

impl SpeechComponentV1 for GroqSpeechReferenceV1 {
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
        capabilities(config, GROQ_SPEECH, &FORMS)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        serves(config, GROQ_SPEECH)?;
        let forms = required_forms(context, &FORMS)?;
        let body = json_body(request)?;
        let count = scalar_count(input(body)?);
        let mut template = forwarded_body(body, &context.upstream_model);
        template
            .insert("voice".to_owned(), resolved_voice(body.get("voice"), &context.upstream_model));
        prepared(
            facts(count, &forms, FALLBACK_MEDIA_TYPE),
            descriptor("v1/audio/speech", None, bearer(config), &Value::Object(template), &[])?,
            ResponseBodyFormV1::Binary,
            &[],
            &IMMUTABLE_BODY_PATHS,
            &json!({ "form": "binary", "characters": count }),
        )
    }

    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        let status = view_status(response);
        if (200..300).contains(&status) {
            binary_outcome(state, response)
        } else {
            Ok(non_2xx(status, response, openai_error_message))
        }
    }
}
