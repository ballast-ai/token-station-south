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
//! - **Gemini 3.8** (1.3.0, south #357 round; `gemini-3.8-flash-tts`, `gemini-3.8-flash-lite-tts`,
//!   from Google's "Generate speech with Gemini TTS" page): the voice goes in
//!   `speechConfig.voiceConfig.voice` (the 3.8 shape; the page does not mention
//!   `prebuiltVoiceConfig`), and the request asks for raw PCM with `generationConfig.responseFormat:
//!   [{"audio": {"mimeType": "AUDIO_L16"}}]`, because a 3.8 unary answer is a whole WAV file by
//!   default; the delivery is then the 2.5 one. An answer whose `inlineData.mimeType` still says
//!   WAV is delivered as that WAV for `wav` and is `unknown` for `pcm`, never wrapped twice. The
//!   locations are `global`, `us` and `eu` (the multi-region hosts are in `vertex_location`). Billed
//!   by characters or by tokens, as the host prices the model: tokens from
//!   `usageMetadata.promptTokenCount` / `candidatesTokenCount`, each a positive integer, else
//!   `unknown`. 2.5 keeps characters only. Nothing about 3.8 has been called against the upstream
//!   (unverified), and its usage fields are documented by the Gemini API, not by the 3.8 page.

use serde_json::{Map, Value, json};
use south_contracts::media::ResponseBodyFormV1;
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechCallContextV1, SpeechContainerV1, SpeechEncodingV1,
    SpeechMeteringFormV1, SpeechModelCapabilitiesV1, SpeechTokensV1,
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
pub const VERSION: &str = "1.3.0";
/// The world this reference exports.
pub const WORLD: &str = "speech-adapter-v1";
/// The family, as a host's speech bindings name it.
pub const VERTEX_AI_SPEECH: &str = "vertex-ai-speech";

const FORMS: [SpeechMeteringFormV1; 1] = [SpeechMeteringFormV1::Characters];
/// The forms a Gemini 3.8 model can report.
const FORMS_38: [SpeechMeteringFormV1; 2] =
    [SpeechMeteringFormV1::Characters, SpeechMeteringFormV1::Tokens];
/// The id prefix of the 3.8 generation, whose request shape differs from 2.5.
const GEMINI_38: &str = "gemini-3.8-";

/// The metering forms `model` can report.
fn forms_for(model: &str) -> &'static [SpeechMeteringFormV1] {
    if model.starts_with(GEMINI_38) { &FORMS_38 } else { &FORMS }
}
/// The native arm's `body_rules` (`tts.rs:63`).
const IMMUTABLE_BODY_PATHS: [&str; 2] = ["contents", "generationConfig.responseModalities"];
/// A 3.8 request adds `generationConfig.responseFormat` (1.3.0): the raw PCM the delivery assumes.
const IMMUTABLE_BODY_PATHS_38: [&str; 3] =
    ["contents", "generationConfig.responseFormat", "generationConfig.responseModalities"];
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

/// `usageMetadata.promptTokenCount` / `candidatesTokenCount` of a token-priced call: both positive
/// integers, else the reason the answer is `unknown`.
fn usage_tokens(body: &Value) -> Result<SpeechTokensV1, &'static str> {
    let usage = body.get("usageMetadata").ok_or("a token-priced answer without usageMetadata")?;
    let count = |field: &str| usage.get(field).and_then(Value::as_u64).filter(|count| *count > 0);
    match (count("promptTokenCount"), count("candidatesTokenCount")) {
        (Some(input), Some(output)) => Ok(SpeechTokensV1 {
            input: Some(input),
            output: Some(output),
            input_text: None,
            input_audio: None,
        }),
        _ => {
            Err("a token-priced answer without positive promptTokenCount and candidatesTokenCount")
        }
    }
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
        let mut models = capabilities(config, VERTEX_AI_SPEECH, &FORMS)?;
        for model in &mut models {
            model.metering_forms = forms_for(&model.model).to_vec();
        }
        Ok(models)
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
        let forms = required_forms(context, forms_for(&context.upstream_model))?;
        let tokens = forms.contains(&SpeechMeteringFormV1::Tokens);
        let body = json_body(request)?;
        let text = input(body)?;
        let format = format_word(body)?;
        let template = if context.upstream_model.starts_with(GEMINI_38) {
            json!({
                "contents": [{ "role": "user", "parts": [{ "text": text }] }],
                "generationConfig": {
                    "responseModalities": ["AUDIO"],
                    "speechConfig": { "voiceConfig": { "voice": voice(body.get("voice")) } },
                    "responseFormat": [{ "audio": { "mimeType": "AUDIO_L16" } }]
                }
            })
        } else {
            json!({
                "contents": [{ "role": "user", "parts": [{ "text": text }] }],
                "generationConfig": {
                    "responseModalities": ["AUDIO"],
                    "speechConfig": {
                        "voiceConfig": {
                            "prebuiltVoiceConfig": { "voiceName": voice(body.get("voice")) }
                        }
                    }
                }
            })
        };
        let auth =
            config.auth.as_ref().map(|slot| json!({ "arm": "bearer", "slot": slot.as_str() }));
        let count = scalar_count(text);
        let mut state = json!({ "format": format, "characters": count });
        if tokens {
            state["tokens"] = json!(true);
        }
        prepared(
            facts(count, &forms, media_type(format)),
            descriptor(&path, None, auth, &template, &[])?,
            ResponseBodyFormV1::Json,
            &[RESPONSE_ELISION_PATH],
            if context.upstream_model.starts_with(GEMINI_38) {
                &IMMUTABLE_BODY_PATHS_38
            } else {
                &IMMUTABLE_BODY_PATHS
            },
            &state,
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
        // A 3.8 answer that ignored `AUDIO_L16` is already a WAV file: deliver it as such for `wav`,
        // never wrap it twice; for `pcm` it is not what was asked.
        let already_wav = json
            .pointer(&format!("/candidates/0/content/parts/{audio}/inlineData/mimeType"))
            .and_then(Value::as_str)
            .is_some_and(|mime| mime.to_ascii_lowercase().contains("wav"));
        if already_wav && format == "pcm" {
            return Ok(unknown("a WAV answer where raw PCM was asked"));
        }
        let tokens = if state["tokens"] == true {
            match usage_tokens(json) {
                Ok(tokens) => Some(tokens),
                Err(reason) => return Ok(unknown(reason)),
            }
        } else {
            None
        };
        let mut outcome = inline_outcome(
            &format!("/candidates/0/content/parts/{audio}/inlineData/data"),
            SpeechEncodingV1::Base64,
            media_type(format),
            state_characters(state)?,
        )?;
        if let SpeechOutcomeV1::Succeeded {
            delivery: SpeechDeliveryV1::Audio(artifact),
            metering,
        } = &mut outcome
        {
            if format == "wav" && !already_wav {
                artifact.container =
                    Some(SpeechContainerV1::WavPcmS16le { sample_rate: SAMPLE_RATE, channels: 1 });
            }
            metering.tokens = tokens;
        }
        Ok(outcome)
    }
}
