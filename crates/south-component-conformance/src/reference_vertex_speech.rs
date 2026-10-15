//! The native reference implementation of the `speech-vertex` component.
//!
//! Gemini TTS through Vertex AI's `:generateContent` (speech world batch V4, design record
//! `docs/design/2026-10-15-speech-v4-tts.md` §3; speech record §12 row 8). Transcribed from
//! token-station-server's native arm `TtsUpstream::VertexGemini` (`handler/audio/tts.rs:411-438`,
//! `:696-748`, `:1032-1050`), its tables `vertex_tts_audio_format`, `resolve_vertex_voice` and
//! `decode_vertex_tts_audio` (`tts_providers.rs:851-950`) and `build_vertex_media_url`
//! (`engine/vertex.rs:207-237`):
//!
//! - **Location** (the crate-private module `vertex_location`, which `embeddings-vertex` and
//!   `image-vertex` also use): `region` from the `region` config key, the project from the `project`
//!   config key, else the `project_id` the credential exports; a Vertex AI base URL must be the
//!   region's origin. A missing value or a mismatched base is a 500 naming it, before any request
//!   refusal. **Path** `v1/projects/{project}/locations/{region}/publishers/google/models/
//!   {model}:generateContent`, each segment percent-encoded. **Auth** `bearer` on the configured
//!   slot, which the package's `vertex_sa` recipe mints from the service account.
//! - **Body** `{"contents": [{"role": "user", "parts": [{"text": input}]}], "generationConfig":
//!   {"responseModalities": ["AUDIO"], "speechConfig": {"voiceConfig": {"prebuiltVoiceConfig":
//!   {"voiceName": voice}}}}}`; nothing else from the client.
//! - **Voice**: the client's string (else `""`), trimmed and lower-cased, is matched: `""` and the
//!   eight names `alloy`, `echo`, `fable`, `onyx`, `nova`, `shimmer`, `ash`, `sage` fold to `Kore`;
//!   anything else is sent as the client wrote it (native folds only eight, record §8 item 2).
//! - **Format** (Q7, 1.1.0): lower-cased; absent or `wav` is WAV, `pcm` raw PCM; `mp3`, `opus`,
//!   `aac` and `flac`, which native silently serves as WAV (and 1.0.x did for the dual run), are
//!   refused before dispatch (record §3.4; lv moved 1.1.0 into 0.66.0 on 2026-10-15); any other
//!   string is refused with native's text without its parenthesis. The upstream always returns
//!   24 kHz mono 16-bit PCM, so nothing is sent for the format.
//! - **Immutable paths** `contents`, `generationConfig.responseModalities` (native `body_rules`).
//! - **Response**: `json` with every `inlineData.data` elided. On a 2xx: a body that is not a JSON
//!   object is `unknown`; the first part of `candidates[0].content.parts` that has an
//!   `inlineData.data` member carries the audio: a non-empty string is an `inline` base64 artifact
//!   (`audio/wav` inside the `wav_pcm_s16le` 24000 Hz mono container, or `audio/L16; rate=24000`
//!   for `pcm`), billed by the request's characters; an empty one is `unknown`. With no audio, a
//!   `promptFeedback.blockReason` is `rejected` and a candidate `finishReason` in the I3-2 policy
//!   table is `unknown`, both with a `content_policy` envelope; anything else is `unknown`. A
//!   non-2xx reads Google's `{"error": {"message"}}` (`gemini_image`), else the body text.

use serde_json::{Map, Value, json};
use south_contracts::media::ResponseBodyFormV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechContainerV1, SpeechEncodingV1,
    SpeechMeteringFormV1, SpeechModelCapabilitiesV1,
};
use south_provider_api::ComponentMetadataV1;
use token_station_protocol::ProviderConfig;

use crate::gemini_image::{POLICY_FINISH_REASONS, error_envelope, policy_envelope};
use crate::speech_shared::{
    capabilities, descriptor, facts, holds_audio, inline_outcome, input, internal, invalid,
    is_blob, json_body, non_2xx_with, prepared, required_forms, scalar_count, serves,
    state_characters, unknown, view_status, with_refs,
};
use crate::{ComponentResultV1, SpeechComponentV1, SpeechDeliveryV1, SpeechOutcomeV1};

/// The package name this reference is published as.
pub const NAME: &str = "speech-vertex";
/// The package version.
pub const VERSION: &str = "1.1.1";
/// The world this reference exports.
pub const WORLD: &str = "speech-adapter-v1";
/// The family, as a host's speech bindings name it.
pub const VERTEX_AI_SPEECH: &str = "vertex-ai-speech";

const FORMS: [SpeechMeteringFormV1; 1] = [SpeechMeteringFormV1::Characters];
/// The native arm's `body_rules` (`tts.rs:63`).
const IMMUTABLE_BODY_PATHS: [&str; 2] = ["contents", "generationConfig.responseModalities"];
/// Every inline payload the upstream returns, so the audio never enters the sandbox.
const RESPONSE_ELISION_PATH: &str = "/candidates/*/content/parts/*/inlineData/data";
/// The voice the folded names and an absent voice resolve to.
const DEFAULT_VOICE: &str = "Kore";
/// The upstream's fixed output (`tts_providers.rs:851-857`, `:875`).
const SAMPLE_RATE: u32 = 24_000;
/// `VertexTtsFormat::content_type` (`tts_providers.rs:863-872`).
const WAV: &str = "audio/wav";
const PCM: &str = "audio/L16; rate=24000";

/// The reference component. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct VertexSpeechReferenceV1;

/// `vertex_tts_audio_format` over the client's `response_format`: `"wav"` or `"pcm"`.
fn format_word(body: &Map<String, Value>) -> ComponentResultV1<&'static str> {
    match body.get("response_format") {
        Some(value) if is_blob(value) => {
            Err(invalid("'response_format' is longer than the gateway accepts"))
        }
        Some(Value::String(format)) => match format.to_lowercase().as_str() {
            "wav" => Ok("wav"),
            "pcm" => Ok("pcm"),
            // Q7 (lv 2026-09-30): refused in 1.1.0, where native serves them as WAV (record §3.4).
            transcoded @ ("mp3" | "opus" | "aac" | "flac") => Err(invalid(format!(
                "Gemini TTS on Vertex cannot emit response_format='{transcoded}'. Supported: wav, pcm."
            ))),
            other => Err(invalid(format!(
                "Unknown response_format='{other}'. Gemini TTS on Vertex emits 24kHz mono PCM; \
                 supported: wav, pcm."
            ))),
        },
        _ => Ok("wav"),
    }
}

/// `resolve_vertex_voice`: an elided voice is forwarded as an `as_is` reference.
fn voice(raw: Option<&Value>) -> Value {
    let given = match raw {
        Some(value) if is_blob(value) => return with_refs(value),
        Some(Value::String(voice)) => voice.as_str(),
        _ => "",
    };
    match given.trim().to_lowercase().as_str() {
        "" | "alloy" | "echo" | "fable" | "onyx" | "nova" | "shimmer" | "ash" | "sage" => {
            json!(DEFAULT_VOICE)
        }
        _ => json!(given),
    }
}

/// The delivered media type of a format word.
fn media_type(word: &str) -> &'static str {
    if word == "pcm" { PCM } else { WAV }
}

/// Where the audio is, as native's `find_map` finds it: the first part of `candidates[0]` with an
/// `inlineData.data` member, and that member.
fn audio_part(body: &Value) -> Option<(usize, &Value)> {
    body.pointer("/candidates/0/content/parts")?
        .as_array()?
        .iter()
        .enumerate()
        .find_map(|(index, part)| part.get("inlineData")?.get("data").map(|data| (index, data)))
}

/// A 2xx without audio (record §3.3 step 3, I3-2 GI-Q5).
fn without_audio(body: &Value) -> SpeechOutcomeV1 {
    if let Some(reason) = body
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)
        .filter(|r| !r.is_empty())
    {
        return SpeechOutcomeV1::Rejected { error: policy_envelope(Some(reason)) };
    }
    let candidates = body["candidates"].as_array().map(Vec::as_slice).unwrap_or_default();
    if let Some(reason) = candidates
        .iter()
        .filter_map(|candidate| candidate["finishReason"].as_str())
        .find(|reason| POLICY_FINISH_REASONS.contains(reason))
    {
        return SpeechOutcomeV1::Unknown {
            reason: format!("a candidate finished for {reason} without audio"),
            error: Some(policy_envelope(Some(reason))),
        };
    }
    unknown("a 2xx without audio")
}

impl SpeechComponentV1 for VertexSpeechReferenceV1 {
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
        capabilities(config, VERTEX_AI_SPEECH, &FORMS)
    }

    fn prepare(
        &self,
        config: &ProviderConfig,
        request: &Value,
        context: &SpeechCallContextV1,
    ) -> ComponentResultV1<PreparedSpeechCallV1> {
        serves(config, VERTEX_AI_SPEECH)?;
        // The location is read first, so a misconfigured row is a 500 before any request refusal
        // (I3-3 §4; the base URL is checked against the region, the path is relative to it).
        let (_base, path) = crate::vertex_location::publisher_model_path(
            config,
            &context.upstream_model,
            "generateContent",
        )?;
        let forms = required_forms(context, &FORMS)?;
        let body = json_body(request)?;
        let text = input(body)?;
        let format = format_word(body)?;
        let template = json!({
            "contents": [{ "role": "user", "parts": [{ "text": text }] }],
            "generationConfig": {
                "responseModalities": ["AUDIO"],
                "speechConfig": {
                    "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": voice(body.get("voice")) } }
                }
            }
        });
        let auth =
            config.auth.as_ref().map(|slot| json!({ "arm": "bearer", "slot": slot.as_str() }));
        let count = scalar_count(text);
        prepared(
            facts(count, &forms, media_type(format)),
            descriptor(&path, None, auth, &template, &[])?,
            ResponseBodyFormV1::Json,
            &[RESPONSE_ELISION_PATH],
            &IMMUTABLE_BODY_PATHS,
            &json!({ "format": format, "characters": count }),
        )
    }

    fn parse_response(
        &self,
        state: &Value,
        response: &Value,
    ) -> ComponentResultV1<SpeechOutcomeV1> {
        let status = view_status(response);
        let json = response.pointer("/body/json").unwrap_or(&Value::Null);
        if !(200..300).contains(&status) {
            return Ok(non_2xx_with(status, response, error_envelope(status, json)));
        }
        if !json.is_object() {
            return Ok(unknown("a 2xx without a JSON object body"));
        }
        let audio = match audio_part(json) {
            // A string (or its placeholder) carries audio when it is not empty.
            Some((index, data)) if data.is_string() || is_blob(data) => {
                if !holds_audio(Some(data)) {
                    return Ok(unknown("a 2xx without audio"));
                }
                index
            }
            _ => return Ok(without_audio(json)),
        };
        let format =
            state["format"].as_str().ok_or_else(|| internal("the state has no audio format"))?;
        let mut outcome = inline_outcome(
            &format!("/candidates/0/content/parts/{audio}/inlineData/data"),
            SpeechEncodingV1::Base64,
            media_type(format),
            state_characters(state)?,
        )?;
        if format == "wav"
            && let SpeechOutcomeV1::Succeeded {
                delivery: SpeechDeliveryV1::Audio(artifact), ..
            } = &mut outcome
        {
            artifact.container =
                Some(SpeechContainerV1::WavPcmS16le { sample_rate: SAMPLE_RATE, channels: 1 });
        }
        Ok(outcome)
    }
}
