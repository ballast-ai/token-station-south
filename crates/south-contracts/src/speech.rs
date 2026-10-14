//! `contracts.speech` v1: the facts, metering, artifacts and transcript deliveries of the
//! `speech-adapter-v1` world (speech record §4–§10).
//!
//! Normative record: `docs/design/2026-09-30-speech-world.md`. The speech world reuses the media
//! vocabulary unchanged ([`crate::media`]: views, descriptors, closed transforms, limits) and adds
//! only what the speech surface needs. Everything here is a fact the upstream or the request
//! carries, never a price: the host keeps the price list, the pricing-form decision and every bound
//! it checks (§8, §9). Outcomes carry an `ErrorEnvelope`, so, as in the image world, they live in
//! the conformance crate with the component trait; this module defines everything they carry. The
//! host-to-component input ([`SpeechCallContextV1`]) tolerates unknown fields; everything a
//! component returns is decoded strictly.

use crate::JsonPointerV1;
use crate::image::member_source_text;
use crate::media::json::{self, Member, Node};
use crate::media::{
    BlobIdV1, BlobSetV1, MAX_MEDIA_TYPE_BYTES, MAX_WAV_CHANNELS, MAX_WAV_SAMPLE_RATE, MediaBodyV1,
    MediaDescriptorErrorV1, MediaLimitsV1, MediaRequestDescriptorV1, MediaTransformV1,
    PathPatternV1, RESERVED_KEY_PREFIX, ResponseBodyFormV1, expand_references, is_media_type,
    is_tchar, walk_references,
};
use crate::{MAX_IMMUTABLE_BODY_PATHS, validate_immutable_body_paths};
use serde::{Deserialize, Deserializer, Serialize};
use std::{collections::BTreeSet, fmt};
use thiserror::Error;

/// `contracts.speech` v1.
pub const SPEECH_CONTRACT_VERSION: u16 = 1;
/// The longest upstream model id a capability declaration names.
pub const MAX_SPEECH_MODEL_ID_BYTES: usize = 256;
/// The most models one `model-capabilities` answer declares.
pub const MAX_SPEECH_MODELS: usize = 256;
/// The most fragments one `segments` artifact concatenates.
pub const MAX_SPEECH_SEGMENTS: usize = 65_536;
/// The most form fields one prepared call declares immutable (the body-path bound of task
/// contract 6).
pub const MAX_IMMUTABLE_FORM_FIELDS: usize = MAX_IMMUTABLE_BODY_PATHS;
/// The longest immutable form-field name, in bytes.
pub const MAX_IMMUTABLE_FORM_FIELD_BYTES: usize = 256;
/// The two speech dialect words of §4, closed (Q14).
pub const SPEECH_DIALECT_WORDS: [&str; 2] = ["speech.translate", "speech.verbose_json"];
/// The prefix every speech dialect word carries; a word under it outside
/// [`SPEECH_DIALECT_WORDS`] is refused.
pub const SPEECH_DIALECT_PREFIX: &str = "speech.";

/// Why a speech contract value is refused. Diagnostics never echo the value.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SpeechContractErrorV1 {
    /// A value does not have the shape of its type.
    #[error("speech contract value is malformed: {0}")]
    Malformed(String),
    /// A count, bound or duration outside its range.
    #[error("speech fact out of range")]
    OutOfRange,
    /// A list that must not be empty is, or one repeats an entry, or exceeds its bound.
    #[error("speech fact list is empty, repeats an entry or is too long")]
    List,
    /// A fact, metering form or bound that the call's operation does not have (§8).
    #[error("speech fact does not belong to the operation")]
    Operation,
    /// A media type outside the grammar.
    #[error("invalid media type")]
    MediaType,
    /// A word under `speech.` outside the two dialect words (§4, Q14).
    #[error("unknown speech dialect word")]
    DialectWord,
    /// The descriptor is refused.
    #[error("speech descriptor refused: {0}")]
    Descriptor(MediaDescriptorErrorV1),
    /// The descriptor's body is `empty`: every speech call carries a JSON, text or multipart body
    /// (§5).
    #[error("speech descriptor body must be json, text or multipart")]
    Body,
    /// More response elision paths than the media limit, or `state` above its bound.
    #[error("prepared speech call exceeds its bounds")]
    PreparedBounds,
    /// The immutability declaration does not match the body kind, or its list is malformed (§7).
    #[error("invalid immutability declaration")]
    Immutability,
    /// A transcript template breaks reference integrity: a reserved key other than an `as_is`
    /// reference, or a string above the fallback threshold.
    #[error("transcript delivery breaks reference integrity")]
    ReferenceIntegrity,
    /// A transcript reference could not be expanded: an unknown blob, or bytes that are not text.
    #[error("transcript delivery could not be filled: {0}")]
    Fill(MediaDescriptorErrorV1),
    /// A metering form the call declared has no value where a value is required (§9).
    #[error("required speech metering is missing: {0:?}")]
    MissingMeter(SpeechMeteringFormV1),
}

fn malformed(error: impl fmt::Display) -> SpeechContractErrorV1 {
    SpeechContractErrorV1::Malformed(error.to_string())
}

fn distinct<T: Ord>(items: &[T]) -> bool {
    items.iter().collect::<BTreeSet<_>>().len() == items.len()
}

/// Whether `value` is a media type for speech output.
///
/// It is `type/subtype` as [`is_media_type`] has it, optionally followed
/// by parameters `;name=value` (one space may follow each `;`), every name and value a run of RFC
/// 7230 `tchar`s, at most [`MAX_MEDIA_TYPE_BYTES`] in all. Case is kept as written.
///
/// Parameters are admitted here, unlike in the image world, because raw PCM is delivered as
/// `audio/L16; rate=24000` (§6). Quoted parameter values are not admitted.
#[must_use]
pub fn is_speech_media_type(value: &str) -> bool {
    if value.len() > MAX_MEDIA_TYPE_BYTES {
        return false;
    }
    let mut pieces = value.split(';');
    let essence = pieces.next().unwrap_or_default();
    let token = |part: &str| !part.is_empty() && part.bytes().all(is_tchar);
    is_media_type(essence)
        && pieces.all(|parameter| {
            let parameter = parameter.strip_prefix(' ').unwrap_or(parameter);
            parameter.split_once('=').is_some_and(|(name, value)| token(name) && token(value))
        })
}

fn media_type(value: String) -> Result<String, SpeechContractErrorV1> {
    if is_speech_media_type(&value) { Ok(value) } else { Err(SpeechContractErrorV1::MediaType) }
}

/// `synthesize`, `transcribe` or `translate` (§4, §8 `operation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechOperationV1 {
    /// Text to speech.
    Synthesize,
    /// Speech to text in the spoken language.
    Transcribe,
    /// Speech to English text.
    Translate,
}

impl SpeechOperationV1 {
    /// Whether the operation reads audio and answers with a transcript.
    #[must_use]
    pub const fn is_recognition(self) -> bool {
        matches!(self, Self::Transcribe | Self::Translate)
    }

    /// Whether `form` is a metering form this operation can report (§9): characters for synthesis,
    /// seconds for recognition, tokens for both.
    #[must_use]
    pub const fn admits(self, form: SpeechMeteringFormV1) -> bool {
        match form {
            SpeechMeteringFormV1::Characters => !self.is_recognition(),
            SpeechMeteringFormV1::Seconds => self.is_recognition(),
            SpeechMeteringFormV1::Tokens => true,
        }
    }
}

/// One of the two speech dialect words a model row sets in `supported_parameters` (§4, Q14).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpeechDialectWordV1 {
    /// `speech.translate`: the model supports translation to English.
    Translate,
    /// `speech.verbose_json`: the model can return a transcription format that carries the audio
    /// duration (§9).
    VerboseJson,
}

impl SpeechDialectWordV1 {
    /// Every word, in [`SPEECH_DIALECT_WORDS`] order.
    pub const ALL: [Self; 2] = [Self::Translate, Self::VerboseJson];

    /// The word as a model row spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Translate => SPEECH_DIALECT_WORDS[0],
            Self::VerboseJson => SPEECH_DIALECT_WORDS[1],
        }
    }

    /// Parses one word; anything outside the two words is refused.
    pub fn parse(word: &str) -> Result<Self, SpeechContractErrorV1> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == word)
            .ok_or(SpeechContractErrorV1::DialectWord)
    }
}

/// The speech dialect words among a model row's `supported_parameters` (§4).
///
/// Entries outside the `speech.` prefix belong to other vocabularies and are skipped; an entry
/// under the prefix that is not one of the two words is refused, so a misspelt word is reported
/// instead of silently turning a capability off.
pub fn speech_dialect_words_v1<'a>(
    supported_parameters: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeSet<SpeechDialectWordV1>, SpeechContractErrorV1> {
    supported_parameters
        .into_iter()
        .filter(|entry| entry.starts_with(SPEECH_DIALECT_PREFIX))
        .map(SpeechDialectWordV1::parse)
        .collect()
}

/// The closed vocabulary of metering forms (§8 `metering_forms`, §9): the unit a call can be
/// billed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechMeteringFormV1 {
    /// Billed characters (synthesis).
    Characters,
    /// Audio seconds (recognition).
    Seconds,
    /// Upstream tokens.
    Tokens,
}

/// The `characters` fact (§8): `count` is the number of Unicode scalar values in the northbound
/// `input` (Q5); `upper` is a tighter bound under the upstream's counting rule, when it has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechCharactersV1 {
    /// Scalar values in `input`.
    pub count: u64,
    /// The billed characters can be at most this many (a weighted count, or `count`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upper: Option<u64>,
}

/// Bounds a component supplies only to tighten the host's reservation (§8 `reservation`).
///
/// Each is at least 1; the host reserves the minimum of its own bound and this value, and checks against
/// its own bound alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechReservationV1 {
    /// Output tokens (token-priced synthesis).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    /// Whole audio seconds (recognition).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_seconds: Option<u64>,
}

/// The pre-dispatch facts of one call (§8). No prices.
///
/// Synthesis carries `characters` and `output_media_type` and may tighten `max_output_tokens`;
/// recognition carries neither and may tighten `max_seconds`. `metering_forms` is non-empty, has no
/// repeats, and names only forms the operation can report ([`SpeechOperationV1::admits`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SpeechFactsWire", into = "SpeechFactsWire")]
pub struct SpeechFactsV1 {
    /// The operation.
    pub operation: SpeechOperationV1,
    /// Characters in the northbound `input` (synthesis only).
    pub characters: Option<SpeechCharactersV1>,
    /// The forms that must be reported on success.
    pub metering_forms: Vec<SpeechMeteringFormV1>,
    /// Tightening bounds.
    pub reservation: Option<SpeechReservationV1>,
    /// The delivered media type (synthesis only), per [`is_speech_media_type`].
    pub output_media_type: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpeechFactsWire {
    operation: SpeechOperationV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    characters: Option<SpeechCharactersV1>,
    metering_forms: Vec<SpeechMeteringFormV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reservation: Option<SpeechReservationV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_media_type: Option<String>,
}

impl TryFrom<SpeechFactsWire> for SpeechFactsV1 {
    type Error = SpeechContractErrorV1;

    fn try_from(wire: SpeechFactsWire) -> Result<Self, Self::Error> {
        if wire.metering_forms.is_empty() || !distinct(&wire.metering_forms) {
            return Err(SpeechContractErrorV1::List);
        }
        let reservation = wire.reservation.unwrap_or_default();
        if [reservation.max_output_tokens, reservation.max_seconds].contains(&Some(0)) {
            return Err(SpeechContractErrorV1::OutOfRange);
        }
        let recognition = wire.operation.is_recognition();
        let shaped = if recognition {
            wire.characters.is_none()
                && wire.output_media_type.is_none()
                && reservation.max_output_tokens.is_none()
        } else {
            wire.characters.is_some()
                && wire.output_media_type.is_some()
                && reservation.max_seconds.is_none()
        };
        if !shaped || !wire.metering_forms.iter().all(|form| wire.operation.admits(*form)) {
            return Err(SpeechContractErrorV1::Operation);
        }
        Ok(Self {
            operation: wire.operation,
            characters: wire.characters,
            metering_forms: wire.metering_forms,
            reservation: wire.reservation,
            output_media_type: wire.output_media_type.map(media_type).transpose()?,
        })
    }
}

impl From<SpeechFactsV1> for SpeechFactsWire {
    fn from(facts: SpeechFactsV1) -> Self {
        Self {
            operation: facts.operation,
            characters: facts.characters,
            metering_forms: facts.metering_forms,
            reservation: facts.reservation,
            output_media_type: facts.output_media_type,
        }
    }
}

/// Where a `characters` metering value comes from (§9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechCharacterSourceV1 {
    /// The billed characters the upstream reported.
    Upstream,
    /// The upstream bills by input characters and reported no number: the request's scalar count.
    Request,
}

/// Billed characters (§9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechCharacterMeterV1 {
    /// The count.
    pub value: u64,
    /// Where it comes from.
    pub source: SpeechCharacterSourceV1,
}

/// How a `seconds` metering value was obtained (§9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechSecondsBasisV1 {
    /// The upstream stated the duration.
    Reported,
    /// The end time of the last timestamped unit: a lower bound, which the ledger labels estimated
    /// (Q12).
    LastTimestamp,
}

/// Audio seconds (§9): a finite, non-negative number.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SecondsWire", into = "SecondsWire")]
pub struct SpeechSecondsMeterV1 {
    /// The duration in seconds.
    pub value: f64,
    /// How it was obtained.
    pub basis: SpeechSecondsBasisV1,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecondsWire {
    value: f64,
    basis: SpeechSecondsBasisV1,
}

impl TryFrom<SecondsWire> for SpeechSecondsMeterV1 {
    type Error = SpeechContractErrorV1;

    fn try_from(wire: SecondsWire) -> Result<Self, Self::Error> {
        if !wire.value.is_finite() || wire.value < 0.0 {
            return Err(SpeechContractErrorV1::OutOfRange);
        }
        Ok(Self { value: wire.value, basis: wire.basis })
    }
}

impl From<SpeechSecondsMeterV1> for SecondsWire {
    fn from(value: SpeechSecondsMeterV1) -> Self {
        Self { value: value.value, basis: value.basis }
    }
}

/// Upstream token counts (§9). Every count is nullable: `null` and `0` are different facts. A count
/// absent from the JSON reads `null`; serialization writes every count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechTokensV1 {
    /// Input tokens.
    #[serde(default)]
    pub input: Option<u64>,
    /// Output tokens.
    #[serde(default)]
    pub output: Option<u64>,
    /// Text input tokens, when the upstream splits input by modality.
    #[serde(default)]
    pub input_text: Option<u64>,
    /// Audio input tokens, when the upstream splits input by modality.
    #[serde(default)]
    pub input_audio: Option<u64>,
}

/// The metering facts of one call (§9). Every fact is nullable; `null` and `0` are different
/// facts. A fact absent from the JSON reads `null`; serialization writes every fact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechMeteringV1 {
    /// Billed characters.
    #[serde(default)]
    pub characters: Option<SpeechCharacterMeterV1>,
    /// Audio seconds.
    #[serde(default)]
    pub seconds: Option<SpeechSecondsMeterV1>,
    /// Upstream token counts.
    #[serde(default)]
    pub tokens: Option<SpeechTokensV1>,
}

/// Checks that a `succeeded` call's metering carries what each declared form requires (§9).
///
/// `characters` must be present (an upstream that reports no number is `source = request`).
/// `tokens` must carry both `input` and `output`; a missing count makes the call `unknown`, never
/// 0. `seconds` may be `null`: the host then applies its documented fallback, marked estimated.
pub fn check_required_metering_v1(
    forms: &[SpeechMeteringFormV1],
    metering: &SpeechMeteringV1,
) -> Result<(), SpeechContractErrorV1> {
    for form in forms {
        let present = match form {
            SpeechMeteringFormV1::Characters => metering.characters.is_some(),
            SpeechMeteringFormV1::Seconds => true,
            SpeechMeteringFormV1::Tokens => metering
                .tokens
                .is_some_and(|tokens| tokens.input.is_some() && tokens.output.is_some()),
        };
        if !present {
            return Err(SpeechContractErrorV1::MissingMeter(*form));
        }
    }
    Ok(())
}

/// How an `inline` string or a `segments` fragment is encoded (§6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechEncodingV1 {
    /// Standard base64.
    Base64,
    /// Hexadecimal.
    Hex,
}

impl SpeechEncodingV1 {
    /// The closed transform that decodes it.
    #[must_use]
    pub const fn transform(self) -> MediaTransformV1 {
        match self {
            Self::Base64 => MediaTransformV1::FromBase64,
            Self::Hex => MediaTransformV1::FromHex,
        }
    }
}

/// A container the host wraps around the decoded bytes (§6):
/// `{"wav_pcm_s16le": {"sample_rate", "channels"}}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ContainerWire", into = "ContainerWire")]
pub enum SpeechContainerV1 {
    /// A 44-byte RIFF/WAVE header around signed 16-bit little-endian PCM.
    WavPcmS16le {
        /// Samples per second, from 1 to [`MAX_WAV_SAMPLE_RATE`].
        sample_rate: u32,
        /// Interleaved channels, from 1 to [`MAX_WAV_CHANNELS`].
        channels: u16,
    },
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum ContainerWire {
    WavPcmS16le(WavWire),
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WavWire {
    sample_rate: u32,
    channels: u16,
}

impl TryFrom<ContainerWire> for SpeechContainerV1 {
    type Error = SpeechContractErrorV1;

    fn try_from(wire: ContainerWire) -> Result<Self, Self::Error> {
        let ContainerWire::WavPcmS16le(WavWire { sample_rate, channels }) = wire;
        if !(1..=MAX_WAV_SAMPLE_RATE).contains(&sample_rate)
            || !(1..=MAX_WAV_CHANNELS).contains(&channels)
        {
            return Err(SpeechContractErrorV1::OutOfRange);
        }
        Ok(Self::WavPcmS16le { sample_rate, channels })
    }
}

impl From<SpeechContainerV1> for ContainerWire {
    fn from(value: SpeechContainerV1) -> Self {
        let SpeechContainerV1::WavPcmS16le { sample_rate, channels } = value;
        Self::WavPcmS16le(WavWire { sample_rate, channels })
    }
}

impl SpeechContainerV1 {
    /// The closed transform that wraps the bytes.
    #[must_use]
    pub const fn transform(self) -> MediaTransformV1 {
        let Self::WavPcmS16le { sample_rate, channels } = self;
        MediaTransformV1::WavPcmS16le { sample_rate, channels }
    }
}

/// One fragment of a `segments` artifact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechSegmentV1 {
    /// Where the fragment's string is.
    pub pointer: JsonPointerV1,
    /// How it is encoded.
    pub encoding: SpeechEncodingV1,
}

/// Where the audio is (§6 `SpeechArtifactV1`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpeechArtifactFormV1 {
    /// The whole response body (`binary` body form).
    Body,
    /// A string in the response view, usually elided.
    Inline {
        /// Where the string is.
        pointer: JsonPointerV1,
        /// How it is encoded.
        encoding: SpeechEncodingV1,
    },
    /// Several fragments, each decoded on its own, then concatenated in order.
    Segments {
        /// The fragments, non-empty, at most [`MAX_SPEECH_SEGMENTS`], no pointer twice.
        items: Vec<SpeechSegmentV1>,
    },
    /// An absolute https URL string in the unelided upstream response, fetched by the host's safe
    /// fetch executor (§10); the component chooses which of the upstream's URLs, never what it is.
    Url {
        /// Where the URL is.
        pointer: JsonPointerV1,
    },
}

/// The audio a synthesis call delivers (§6): a form, the delivered media type, and an optional
/// container the host adds after decoding.
///
/// Wire: `{"form": "body" | "inline" | "segments" | "url", …, "media_type", "container"?}` with
/// `pointer` and `encoding` for `inline`, `items` for `segments` and `pointer` for `url`; a member
/// another form has is refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ArtifactWire", into = "ArtifactWire")]
pub struct SpeechArtifactV1 {
    /// Where the audio is.
    pub form: SpeechArtifactFormV1,
    /// The delivered media type, per [`is_speech_media_type`].
    pub media_type: String,
    /// The container the host adds, when the audio is raw PCM.
    pub container: Option<SpeechContainerV1>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FormWord {
    Body,
    Inline,
    Segments,
    Url,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactWire {
    form: FormWord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pointer: Option<JsonPointerV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    encoding: Option<SpeechEncodingV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    items: Option<Vec<SpeechSegmentV1>>,
    media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    container: Option<SpeechContainerV1>,
}

impl TryFrom<ArtifactWire> for SpeechArtifactV1 {
    type Error = SpeechContractErrorV1;

    fn try_from(wire: ArtifactWire) -> Result<Self, Self::Error> {
        let shape =
            || SpeechContractErrorV1::Malformed("artifact members do not fit its form".into());
        let form = match (wire.form, wire.pointer, wire.encoding, wire.items) {
            (FormWord::Body, None, None, None) => SpeechArtifactFormV1::Body,
            (FormWord::Inline, Some(pointer), Some(encoding), None) => {
                SpeechArtifactFormV1::Inline { pointer, encoding }
            }
            (FormWord::Segments, None, None, Some(items)) => {
                let pointers: Vec<&str> = items.iter().map(|item| item.pointer.as_str()).collect();
                if items.is_empty() || items.len() > MAX_SPEECH_SEGMENTS || !distinct(&pointers) {
                    return Err(SpeechContractErrorV1::List);
                }
                SpeechArtifactFormV1::Segments { items }
            }
            (FormWord::Url, Some(pointer), None, None) => SpeechArtifactFormV1::Url { pointer },
            _ => return Err(shape()),
        };
        Ok(Self { form, media_type: media_type(wire.media_type)?, container: wire.container })
    }
}

impl From<SpeechArtifactV1> for ArtifactWire {
    fn from(artifact: SpeechArtifactV1) -> Self {
        let (form, pointer, encoding, items) = match artifact.form {
            SpeechArtifactFormV1::Body => (FormWord::Body, None, None, None),
            SpeechArtifactFormV1::Inline { pointer, encoding } => {
                (FormWord::Inline, Some(pointer), Some(encoding), None)
            }
            SpeechArtifactFormV1::Segments { items } => {
                (FormWord::Segments, None, None, Some(items))
            }
            SpeechArtifactFormV1::Url { pointer } => (FormWord::Url, Some(pointer), None, None),
        };
        Self {
            form,
            pointer,
            encoding,
            items,
            media_type: artifact.media_type,
            container: artifact.container,
        }
    }
}

impl SpeechArtifactV1 {
    /// Whether the artifact can be read from a response of the declared body form: `body` needs
    /// `binary`; `inline`, `segments` and `url` point into a JSON tree, so they need `json` or
    /// `sse`. A `text` response carries no audio.
    #[must_use]
    pub const fn fits(&self, response_body_form: ResponseBodyFormV1) -> bool {
        match self.form {
            SpeechArtifactFormV1::Body => matches!(response_body_form, ResponseBodyFormV1::Binary),
            SpeechArtifactFormV1::Inline { .. }
            | SpeechArtifactFormV1::Segments { .. }
            | SpeechArtifactFormV1::Url { .. } => {
                matches!(response_body_form, ResponseBodyFormV1::Json | ResponseBodyFormV1::Sse)
            }
        }
    }
}

/// What a recognition call delivers to the client (§6 ASR delivery): a document the component
/// rendered, not bytes.
///
/// Wire, one of: `{"json": <template>}`; `{"text": "…", "media_type": "…"}`;
/// `{"passthrough": {"media_type": "…"}}`. A `json` template may contain
/// `{"$south.ref": {"blob": "<id>", "transform": "as_is"}}` nodes for strings the host elided from
/// the response view; no other `$south.` key, no other transform, and no string above the
/// fallback threshold, so a component cannot smuggle bytes out of the sandbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranscriptDeliveryV1 {
    /// A JSON document, kept as compact source text (members in source order).
    Json {
        /// The template.
        template: String,
    },
    /// A text document (`text`, `srt`, `vtt`) rendered by the component.
    Text {
        /// The document.
        text: String,
        /// Its media type, per [`is_speech_media_type`].
        media_type: String,
    },
    /// The upstream body, unchanged.
    Passthrough {
        /// The media type the client receives, per [`is_speech_media_type`].
        media_type: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeliveryWire {
    #[allow(dead_code)]
    #[serde(default)]
    json: Option<serde::de::IgnoredAny>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    media_type: Option<String>,
    #[serde(default)]
    passthrough: Option<PassthroughWire>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PassthroughWire {
    media_type: String,
}

/// Decodes and checks a transcript delivery (see [`TranscriptDeliveryV1`]).
pub fn parse_transcript_delivery_v1(
    json_text: &str,
    limits: &MediaLimitsV1,
) -> Result<TranscriptDeliveryV1, SpeechContractErrorV1> {
    let wire: DeliveryWire = serde_json::from_str(json_text).map_err(malformed)?;
    let shape = || SpeechContractErrorV1::Malformed("delivery is not exactly one form".into());
    match (wire.json.is_some(), wire.text, wire.media_type, wire.passthrough) {
        (true, None, None, None) => {
            let template = member_source_text(json_text, "json").ok_or_else(shape)?;
            check_template(&template, limits)?;
            Ok(TranscriptDeliveryV1::Json { template })
        }
        (false, Some(text), Some(media_type_text), None) => {
            if text.len() > limits.inline_string_bytes {
                return Err(SpeechContractErrorV1::ReferenceIntegrity);
            }
            Ok(TranscriptDeliveryV1::Text { text, media_type: media_type(media_type_text)? })
        }
        (false, None, None, Some(passthrough)) => Ok(TranscriptDeliveryV1::Passthrough {
            media_type: media_type(passthrough.media_type)?,
        }),
        _ => Err(shape()),
    }
}

fn check_template(template: &str, limits: &MediaLimitsV1) -> Result<(), SpeechContractErrorV1> {
    let tree =
        json::parse(template.as_bytes()).map_err(|_| SpeechContractErrorV1::ReferenceIntegrity)?;
    walk_references(&tree, &mut |reference| {
        if reference.transform == MediaTransformV1::AsIs && reference.media_type.is_none() {
            Ok(())
        } else {
            Err(MediaDescriptorErrorV1::Reference)
        }
    })
    .map_err(|_| SpeechContractErrorV1::ReferenceIntegrity)?;
    if within_threshold(&tree, limits) {
        Ok(())
    } else {
        Err(SpeechContractErrorV1::ReferenceIntegrity)
    }
}

/// Whether every string and key outside a reference node is at most the fallback threshold.
fn within_threshold(node: &Node, limits: &MediaLimitsV1) -> bool {
    match node {
        Node::Scalar(_) => true,
        Node::String(string) => string.decoded.len() <= limits.inline_string_bytes,
        Node::Array(items) => items.iter().all(|item| within_threshold(item, limits)),
        Node::Object(members) => members.iter().all(|Member { key, value }| {
            key.decoded.starts_with(RESERVED_KEY_PREFIX)
                || (key.decoded.len() <= limits.inline_string_bytes
                    && within_threshold(value, limits))
        }),
    }
}

impl TranscriptDeliveryV1 {
    /// Serializes the delivery compactly; [`parse_transcript_delivery_v1`] of the result gives
    /// back an equal delivery. A template is written as its source text.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        match self {
            Self::Json { template } => {
                out.push_str("{\"json\":");
                out.push_str(template);
                out.push('}');
            }
            Self::Text { text, media_type } => {
                out.push_str("{\"text\":");
                json::write_string(text, &mut out);
                out.push_str(",\"media_type\":");
                json::write_string(media_type, &mut out);
                out.push('}');
            }
            Self::Passthrough { media_type } => {
                out.push_str("{\"passthrough\":{\"media_type\":");
                json::write_string(media_type, &mut out);
                out.push_str("}}");
            }
        }
        out
    }

    /// Every blob a `json` template references, in order of appearance.
    #[must_use]
    pub fn referenced_blobs(&self) -> Vec<BlobIdV1> {
        let mut ids = Vec::new();
        if let Self::Json { template } = self
            && let Ok(tree) = json::parse(template.as_bytes())
        {
            let _ = walk_references(&tree, &mut |reference| {
                ids.push(reference.blob.clone());
                Ok(())
            });
        }
        ids
    }
}

/// Fills a checked `json` transcript template: each `$south.ref` node becomes the JSON string of
/// its blob's text (`as_is`), everything else is copied as written.
///
/// `blobs` are the response view's; a reference to a blob the set does not hold, or to bytes that
/// are not UTF-8, is refused.
pub fn fill_transcript_template_v1(
    template: &str,
    blobs: &BlobSetV1,
) -> Result<String, SpeechContractErrorV1> {
    let mut tree =
        json::parse(template.as_bytes()).map_err(|_| SpeechContractErrorV1::ReferenceIntegrity)?;
    expand_references(&mut tree, blobs).map_err(SpeechContractErrorV1::Fill)?;
    let mut out = String::with_capacity(template.len());
    json::write_compact(&tree, &mut out);
    Ok(out)
}

/// Validates the immutable form fields of a multipart call (§7).
///
/// At most [`MAX_IMMUTABLE_FORM_FIELDS`] names, each `[A-Za-z0-9_-]{1,256}`, none twice. A name also covers
/// its array form (`include` covers `include[]`), as a multipart immutable path does in the image
/// world.
pub fn validate_immutable_form_fields_v1(fields: &[String]) -> Result<(), SpeechContractErrorV1> {
    let name_ok = |name: &String| {
        (1..=MAX_IMMUTABLE_FORM_FIELD_BYTES).contains(&name.len())
            && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    };
    if fields.len() <= MAX_IMMUTABLE_FORM_FIELDS && fields.iter().all(name_ok) && distinct(fields) {
        Ok(())
    } else {
        Err(SpeechContractErrorV1::Immutability)
    }
}

/// Whether a multipart field named `name` is covered by a declared immutable form field: the
/// field itself, or its array form `<field>[]`.
#[must_use]
pub fn form_field_is_immutable(fields: &[String], name: &str) -> bool {
    let base = name.strip_suffix("[]").unwrap_or(name);
    fields.iter().any(|field| field == base)
}

/// One model's speech declaration (§7 `model-capabilities`).
///
/// It names the operations the component will perform for it (what the dialect can do, intersected with the dialect words on its row, §4)
/// and the metering forms it can report. Voices and formats are not carried (see the record's
/// S-S-1 implementation section).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CapabilitiesWire", into = "CapabilitiesWire")]
pub struct SpeechModelCapabilitiesV1 {
    /// The upstream model id.
    pub model: String,
    /// Supported operations; non-empty, no repeats.
    pub operations: Vec<SpeechOperationV1>,
    /// Metering forms it can report; non-empty, no repeats, each admitted by some operation.
    pub metering_forms: Vec<SpeechMeteringFormV1>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapabilitiesWire {
    model: String,
    operations: Vec<SpeechOperationV1>,
    metering_forms: Vec<SpeechMeteringFormV1>,
}

impl TryFrom<CapabilitiesWire> for SpeechModelCapabilitiesV1 {
    type Error = SpeechContractErrorV1;

    fn try_from(wire: CapabilitiesWire) -> Result<Self, Self::Error> {
        if wire.model.is_empty() || wire.model.len() > MAX_SPEECH_MODEL_ID_BYTES {
            return Err(SpeechContractErrorV1::OutOfRange);
        }
        if wire.operations.is_empty()
            || !distinct(&wire.operations)
            || wire.metering_forms.is_empty()
            || !distinct(&wire.metering_forms)
        {
            return Err(SpeechContractErrorV1::List);
        }
        if !wire
            .metering_forms
            .iter()
            .all(|form| wire.operations.iter().any(|operation| operation.admits(*form)))
        {
            return Err(SpeechContractErrorV1::Operation);
        }
        Ok(Self {
            model: wire.model,
            operations: wire.operations,
            metering_forms: wire.metering_forms,
        })
    }
}

impl From<SpeechModelCapabilitiesV1> for CapabilitiesWire {
    fn from(value: SpeechModelCapabilitiesV1) -> Self {
        Self {
            model: value.model,
            operations: value.operations,
            metering_forms: value.metering_forms,
        }
    }
}

/// Decodes a `model-capabilities` answer: at most [`MAX_SPEECH_MODELS`] models, each named once.
pub fn parse_speech_model_capabilities_v1(
    json_text: &str,
) -> Result<Vec<SpeechModelCapabilitiesV1>, SpeechContractErrorV1> {
    let models: Vec<SpeechModelCapabilitiesV1> =
        serde_json::from_str(json_text).map_err(malformed)?;
    let names: Vec<&str> = models.iter().map(|model| model.model.as_str()).collect();
    if models.len() > MAX_SPEECH_MODELS || !distinct(&names) {
        return Err(SpeechContractErrorV1::List);
    }
    Ok(models)
}

/// What the host tells `prepare` beside the request view (§7, §8). Tolerates unknown fields, so a
/// newer host can add context an older component ignores.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeechCallContextV1 {
    /// The routed upstream model id.
    pub upstream_model: String,
    /// The operation the northbound route asks for.
    pub operation: SpeechOperationV1,
    /// The metering forms the host's pricing decision requires (§8 "pricing form").
    pub metering_required: Vec<SpeechMeteringFormV1>,
}

/// What `prepare` returns (§7, §8).
///
/// The immutability declaration follows the body kind (§7): a JSON body declares
/// `immutable_body_paths` (task contract 6 §4: `null` = the host injects nothing, `[]` = nothing is
/// reserved, otherwise the reserved dotted paths); a text (SSML) body declares
/// `immutable_body_paths: null`; a multipart body declares `immutable_form_fields` with the same
/// three meanings over form-field names. The key the body kind uses must appear (it may be
/// `null`); the other must not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedSpeechCallV1 {
    /// The pre-dispatch facts.
    pub facts: SpeechFactsV1,
    /// The upstream request; its body is JSON, text or multipart.
    pub descriptor: MediaRequestDescriptorV1,
    /// The declared response body form.
    pub response_body_form: ResponseBodyFormV1,
    /// Response-side elision paths.
    pub response_elision_paths: Vec<PathPatternV1>,
    /// JSON body paths the host must not rewrite; `None` on a multipart body and always `None` on
    /// a text body.
    pub immutable_body_paths: Option<Vec<String>>,
    /// Form fields the host must not add, remove or rewrite; `None` unless the body is multipart.
    pub immutable_form_fields: Option<Vec<String>>,
    /// The bounded, secret-free state `parse-response` receives, as compact JSON.
    pub state: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedWire {
    facts: SpeechFactsV1,
    #[allow(dead_code)]
    descriptor: serde::de::IgnoredAny,
    response_body_form: ResponseBodyFormV1,
    #[serde(default)]
    response_elision_paths: Vec<PathPatternV1>,
    #[serde(default, deserialize_with = "present")]
    immutable_body_paths: Option<Declared>,
    #[serde(default, deserialize_with = "present")]
    immutable_form_fields: Option<Declared>,
    #[allow(dead_code)]
    #[serde(default)]
    state: serde::de::IgnoredAny,
}

/// An immutability key that was present, its value `null` or a list.
#[derive(Deserialize)]
#[serde(transparent)]
struct Declared(Option<Vec<String>>);

/// Distinguishes a key that is present, even with `null`, from an absent one (`None`).
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Decodes and validates a `prepare` result.
pub fn parse_prepared_speech_call_v1(
    json_text: &str,
    limits: &MediaLimitsV1,
) -> Result<PreparedSpeechCallV1, SpeechContractErrorV1> {
    let wire: PreparedWire = serde_json::from_str(json_text).map_err(malformed)?;
    if wire.response_elision_paths.len() > limits.elision_paths {
        return Err(SpeechContractErrorV1::PreparedBounds);
    }
    let descriptor_text = member_source_text(json_text, "descriptor")
        .ok_or_else(|| malformed("descriptor missing"))?;
    let descriptor = MediaRequestDescriptorV1::parse(&descriptor_text, limits)
        .map_err(SpeechContractErrorV1::Descriptor)?;
    let (immutable_body_paths, immutable_form_fields) =
        match (descriptor.body(), wire.immutable_body_paths, wire.immutable_form_fields) {
            (MediaBodyV1::Empty, ..) => return Err(SpeechContractErrorV1::Body),
            (MediaBodyV1::Json(_), Some(Declared(paths)), None) => {
                if let Some(paths) = &paths {
                    validate_immutable_body_paths(paths)
                        .map_err(|_| SpeechContractErrorV1::Immutability)?;
                }
                (paths, None)
            }
            (MediaBodyV1::Text(_), Some(Declared(None)), None) => (None, None),
            (MediaBodyV1::Multipart(_), None, Some(Declared(fields))) => {
                if let Some(fields) = &fields {
                    validate_immutable_form_fields_v1(fields)?;
                }
                (None, fields)
            }
            _ => return Err(SpeechContractErrorV1::Immutability),
        };
    let state = member_source_text(json_text, "state").unwrap_or_else(|| "null".to_owned());
    if state.len() > limits.state_bytes {
        return Err(SpeechContractErrorV1::PreparedBounds);
    }
    Ok(PreparedSpeechCallV1 {
        facts: wire.facts,
        descriptor,
        response_body_form: wire.response_body_form,
        response_elision_paths: wire.response_elision_paths,
        immutable_body_paths,
        immutable_form_fields,
        state,
    })
}

impl PreparedSpeechCallV1 {
    /// Serializes the call compactly; [`parse_prepared_speech_call_v1`] of the result gives back
    /// an equal call. The descriptor and the state are written as their source text, and the
    /// immutability key the body kind uses is always written.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\"facts\":");
        out.push_str(&serde_json::to_string(&self.facts).unwrap_or_default());
        out.push_str(",\"descriptor\":");
        out.push_str(&self.descriptor.to_json());
        out.push_str(",\"response_body_form\":");
        out.push_str(&serde_json::to_string(&self.response_body_form).unwrap_or_default());
        out.push_str(",\"response_elision_paths\":");
        out.push_str(&serde_json::to_string(&self.response_elision_paths).unwrap_or_default());
        if matches!(self.descriptor.body(), MediaBodyV1::Multipart(_)) {
            out.push_str(",\"immutable_form_fields\":");
            out.push_str(&serde_json::to_string(&self.immutable_form_fields).unwrap_or_default());
        } else {
            out.push_str(",\"immutable_body_paths\":");
            out.push_str(&serde_json::to_string(&self.immutable_body_paths).unwrap_or_default());
        }
        out.push_str(",\"state\":");
        out.push_str(&self.state);
        out.push('}');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::BlobV1;

    const TTS_FACTS: &str = r#"{"operation":"synthesize","characters":{"count":5,"upper":10},
        "metering_forms":["characters"],"output_media_type":"audio/mpeg"}"#;

    #[test]
    fn speech_media_types_admit_parameters() {
        for good in ["audio/mpeg", "audio/L16; rate=24000", "audio/L16;rate=24000;channels=1"] {
            assert!(is_speech_media_type(good), "{good}");
        }
        for bad in [
            "",
            "audio",
            "audio/L16; rate",
            "audio/L16; rate=",
            "audio/L16;  rate=1",
            "audio/L16; rate=\"1\"",
            "audio/L16;",
            &format!("audio/{}", "x".repeat(MAX_MEDIA_TYPE_BYTES)),
        ] {
            assert!(!is_speech_media_type(bad), "{bad}");
        }
    }

    #[test]
    fn dialect_words_are_closed() {
        let words =
            speech_dialect_words_v1(["tools", "speech.translate", "speech.verbose_json", "seed"])
                .expect("valid");
        assert_eq!(words.into_iter().collect::<Vec<_>>(), SpeechDialectWordV1::ALL);
        assert!(speech_dialect_words_v1(["tools"]).expect("valid").is_empty());
        for bad in ["speech.verbose-json", "speech.", "speech.Translate"] {
            assert_eq!(speech_dialect_words_v1([bad]), Err(SpeechContractErrorV1::DialectWord));
        }
        for word in SpeechDialectWordV1::ALL {
            assert_eq!(SpeechDialectWordV1::parse(word.as_str()), Ok(word));
        }
    }

    #[test]
    fn facts_round_trip_and_follow_the_operation() {
        let facts: SpeechFactsV1 = serde_json::from_str(TTS_FACTS).expect("valid");
        assert_eq!(facts.characters, Some(SpeechCharactersV1 { count: 5, upper: Some(10) }));
        let round_trip: SpeechFactsV1 =
            serde_json::from_str(&serde_json::to_string(&facts).expect("serializable"))
                .expect("valid");
        assert_eq!(round_trip, facts);
        let asr: SpeechFactsV1 = serde_json::from_str(
            r#"{"operation":"transcribe","metering_forms":["seconds","tokens"],"reservation":{"max_seconds":600}}"#,
        )
        .expect("valid");
        assert_eq!(asr.reservation.and_then(|bound| bound.max_seconds), Some(600));
        for (bad, expected) in [
            (
                r#"{"operation":"synthesize","characters":{"count":1},"metering_forms":[],"output_media_type":"audio/mpeg"}"#,
                SpeechContractErrorV1::List,
            ),
            (
                r#"{"operation":"synthesize","characters":{"count":1},"metering_forms":["tokens","tokens"],"output_media_type":"audio/mpeg"}"#,
                SpeechContractErrorV1::List,
            ),
            (
                r#"{"operation":"synthesize","metering_forms":["characters"],"output_media_type":"audio/mpeg"}"#,
                SpeechContractErrorV1::Operation,
            ),
            (
                r#"{"operation":"synthesize","characters":{"count":1},"metering_forms":["characters"]}"#,
                SpeechContractErrorV1::Operation,
            ),
            (
                r#"{"operation":"synthesize","characters":{"count":1},"metering_forms":["seconds"],"output_media_type":"audio/mpeg"}"#,
                SpeechContractErrorV1::Operation,
            ),
            (
                r#"{"operation":"synthesize","characters":{"count":1},"metering_forms":["characters"],"output_media_type":"audio/mpeg","reservation":{"max_seconds":1}}"#,
                SpeechContractErrorV1::Operation,
            ),
            (
                r#"{"operation":"synthesize","characters":{"count":1},"metering_forms":["tokens"],"output_media_type":"audio/mpeg","reservation":{"max_output_tokens":0}}"#,
                SpeechContractErrorV1::OutOfRange,
            ),
            (
                r#"{"operation":"synthesize","characters":{"count":1},"metering_forms":["characters"],"output_media_type":"audio mpeg"}"#,
                SpeechContractErrorV1::MediaType,
            ),
            (
                r#"{"operation":"translate","characters":{"count":1},"metering_forms":["seconds"]}"#,
                SpeechContractErrorV1::Operation,
            ),
            (
                r#"{"operation":"translate","metering_forms":["characters"]}"#,
                SpeechContractErrorV1::Operation,
            ),
            (
                r#"{"operation":"transcribe","metering_forms":["seconds"],"output_media_type":"text/plain"}"#,
                SpeechContractErrorV1::Operation,
            ),
            (
                r#"{"operation":"transcribe","metering_forms":["tokens"],"reservation":{"max_output_tokens":5}}"#,
                SpeechContractErrorV1::Operation,
            ),
        ] {
            let error = serde_json::from_str::<SpeechFactsV1>(bad).expect_err(bad);
            assert!(error.to_string().contains(&expected.to_string()), "{bad}: {error}");
        }
        for bad in [
            r#"{"operation":"dub","metering_forms":["seconds"]}"#,
            r#"{"operation":"transcribe","metering_forms":["minutes"]}"#,
            r#"{"operation":"transcribe","metering_forms":["seconds"],"price":1}"#,
            r#"{"operation":"synthesize","characters":{"count":1,"weighted":2},"metering_forms":["characters"],"output_media_type":"audio/mpeg"}"#,
        ] {
            assert!(serde_json::from_str::<SpeechFactsV1>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn metering_keeps_null_apart_from_zero() {
        let metering: SpeechMeteringV1 = serde_json::from_str(
            r#"{"characters":{"value":0,"source":"upstream"},"tokens":{"input":12}}"#,
        )
        .expect("valid");
        assert_eq!(
            metering.characters,
            Some(SpeechCharacterMeterV1 { value: 0, source: SpeechCharacterSourceV1::Upstream })
        );
        assert_eq!(metering.seconds, None);
        let json = serde_json::to_value(metering).expect("serializable");
        assert!(json["seconds"].is_null());
        assert!(json["tokens"]["output"].is_null());
        assert_eq!(json["tokens"]["input"], 12);
        let seconds: SpeechMeteringV1 =
            serde_json::from_str(r#"{"seconds":{"value":12.48,"basis":"last_timestamp"}}"#)
                .expect("valid");
        assert_eq!(
            seconds.seconds,
            Some(SpeechSecondsMeterV1 { value: 12.48, basis: SpeechSecondsBasisV1::LastTimestamp })
        );
        assert_eq!(
            serde_json::from_str::<SpeechMeteringV1>(
                &serde_json::to_string(&seconds).expect("serializable")
            )
            .expect("valid"),
            seconds
        );
        for bad in [
            r#"{"seconds":{"value":-0.5,"basis":"reported"}}"#,
            r#"{"seconds":{"value":1,"basis":"guessed"}}"#,
            r#"{"seconds":{"value":"1","basis":"reported"}}"#,
            r#"{"characters":{"value":-1,"source":"request"}}"#,
            r#"{"characters":{"value":1,"source":"catalog"}}"#,
            r#"{"characters":{"value":1}}"#,
            r#"{"tokens":{"input":1,"cached":1}}"#,
            r#"{"minutes":1}"#,
        ] {
            assert!(serde_json::from_str::<SpeechMeteringV1>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn required_metering_is_never_read_as_zero() {
        use SpeechMeteringFormV1::{Characters, Seconds, Tokens};
        let both = SpeechTokensV1 { input: Some(3), output: Some(0), ..SpeechTokensV1::default() };
        let full = SpeechMeteringV1 { tokens: Some(both), ..SpeechMeteringV1::default() };
        assert_eq!(check_required_metering_v1(&[Tokens, Seconds], &full), Ok(()));
        for missing in [
            SpeechTokensV1 { input: Some(3), ..SpeechTokensV1::default() },
            SpeechTokensV1 { output: Some(3), ..SpeechTokensV1::default() },
        ] {
            let metering =
                SpeechMeteringV1 { tokens: Some(missing), ..SpeechMeteringV1::default() };
            assert_eq!(
                check_required_metering_v1(&[Tokens], &metering),
                Err(SpeechContractErrorV1::MissingMeter(Tokens))
            );
        }
        assert_eq!(
            check_required_metering_v1(&[Tokens], &SpeechMeteringV1::default()),
            Err(SpeechContractErrorV1::MissingMeter(Tokens))
        );
        assert_eq!(
            check_required_metering_v1(&[Characters], &full),
            Err(SpeechContractErrorV1::MissingMeter(Characters))
        );
        // A missing duration is the host's estimate, not a refusal (§9).
        assert_eq!(check_required_metering_v1(&[Seconds], &SpeechMeteringV1::default()), Ok(()));
    }

    #[test]
    fn artifact_forms_round_trip() {
        for text in [
            r#"{"form":"body","media_type":"audio/mpeg"}"#,
            r#"{"form":"inline","pointer":"/data/audio","encoding":"hex","media_type":"audio/mpeg"}"#,
            r#"{"form":"inline","pointer":"/candidates/0/content/parts/0/inlineData/data","encoding":"base64","media_type":"audio/wav","container":{"wav_pcm_s16le":{"sample_rate":24000,"channels":1}}}"#,
            r#"{"form":"segments","items":[{"pointer":"/0/data/audio","encoding":"base64"},{"pointer":"/1/data/audio","encoding":"base64"}],"media_type":"audio/mpeg"}"#,
            r#"{"form":"url","pointer":"/output/audio/url","media_type":"audio/wav"}"#,
            r#"{"form":"inline","pointer":"/a","encoding":"base64","media_type":"audio/L16; rate=24000"}"#,
        ] {
            let artifact: SpeechArtifactV1 = serde_json::from_str(text).expect(text);
            assert_eq!(serde_json::to_string(&artifact).expect("serializable"), text);
        }
        let wav: SpeechArtifactV1 = serde_json::from_str(
            r#"{"form":"body","media_type":"audio/wav","container":{"wav_pcm_s16le":{"sample_rate":8000,"channels":2}}}"#,
        )
        .expect("valid");
        assert_eq!(
            wav.container.map(SpeechContainerV1::transform),
            Some(MediaTransformV1::WavPcmS16le { sample_rate: 8000, channels: 2 })
        );
        assert_eq!(SpeechEncodingV1::Hex.transform(), MediaTransformV1::FromHex);
        assert_eq!(SpeechEncodingV1::Base64.transform(), MediaTransformV1::FromBase64);
    }

    #[test]
    fn artifacts_refuse_members_of_another_form_and_bad_values() {
        for bad in [
            r#"{"form":"body","pointer":"/a","media_type":"audio/mpeg"}"#,
            r#"{"form":"inline","pointer":"/a","media_type":"audio/mpeg"}"#,
            r#"{"form":"inline","pointer":"/a","encoding":"data_url","media_type":"audio/mpeg"}"#,
            r#"{"form":"url","pointer":"/a","encoding":"base64","media_type":"audio/mpeg"}"#,
            r#"{"form":"url","media_type":"audio/mpeg"}"#,
            r#"{"form":"url","pointer":"/a"}"#,
            r#"{"form":"segments","items":[],"media_type":"audio/mpeg"}"#,
            r#"{"form":"segments","items":[{"pointer":"/0/d","encoding":"base64"},{"pointer":"/0/d","encoding":"base64"}],"media_type":"audio/mpeg"}"#,
            r#"{"form":"segments","pointer":"/a","items":[{"pointer":"/0/d","encoding":"base64"}],"media_type":"audio/mpeg"}"#,
            r#"{"form":"stream","media_type":"audio/mpeg"}"#,
            r#"{"form":"body","media_type":"audio mpeg"}"#,
            r#"{"form":"body","media_type":"audio/mpeg","extra":1}"#,
            r#"{"form":"url","pointer":"a","media_type":"audio/mpeg"}"#,
            r#"{"form":"body","media_type":"audio/wav","container":{"wav_pcm_s16le":{"sample_rate":0,"channels":1}}}"#,
            r#"{"form":"body","media_type":"audio/wav","container":{"wav_pcm_s16le":{"sample_rate":24000,"channels":9}}}"#,
            r#"{"form":"body","media_type":"audio/wav","container":{"wav_pcm_s16le":{"sample_rate":24000}}}"#,
            r#"{"form":"body","media_type":"audio/wav","container":{"wav_pcm_s16le":{"sample_rate":24000,"channels":1,"bits":16}}}"#,
            r#"{"form":"body","media_type":"audio/wav","container":{"flac":{}}}"#,
            r#"{"form":"body","media_type":"audio/wav","container":"wav_pcm_s16le"}"#,
        ] {
            assert!(serde_json::from_str::<SpeechArtifactV1>(bad).is_err(), "{bad}");
        }
        let item = r#"{"pointer":"/0/d","encoding":"base64"}"#;
        let too_many = format!(
            r#"{{"form":"segments","items":[{}],"media_type":"audio/mpeg"}}"#,
            (0..=MAX_SPEECH_SEGMENTS)
                .map(|index| item.replace("/0/", &format!("/{index}/")))
                .collect::<Vec<_>>()
                .join(",")
        );
        assert!(serde_json::from_str::<SpeechArtifactV1>(&too_many).is_err());
    }

    #[test]
    fn artifacts_fit_their_response_body_form() {
        let artifact = |text: &str| serde_json::from_str::<SpeechArtifactV1>(text).expect(text);
        let body = artifact(r#"{"form":"body","media_type":"audio/mpeg"}"#);
        let segments = artifact(
            r#"{"form":"segments","items":[{"pointer":"/0/data/audio","encoding":"base64"}],"media_type":"audio/mpeg"}"#,
        );
        assert!(body.fits(ResponseBodyFormV1::Binary));
        assert!(!body.fits(ResponseBodyFormV1::Json));
        assert!(segments.fits(ResponseBodyFormV1::Sse));
        assert!(segments.fits(ResponseBodyFormV1::Json));
        assert!(!segments.fits(ResponseBodyFormV1::Binary));
        assert!(!segments.fits(ResponseBodyFormV1::Text));
    }

    #[test]
    fn transcript_deliveries_round_trip() {
        for text in [
            r#"{"json":{"text":"hello","segments":[{"start":0.0,"end":1.5}]}}"#,
            r#"{"json":{"text":{"$south.ref":{"blob":"b3","transform":"as_is"}}}}"#,
            r#"{"text":"1\n00:00:00,000 --> 00:00:01,500\nhello\n","media_type":"application/x-subrip"}"#,
            r#"{"text":"hello","media_type":"text/plain; charset=utf-8"}"#,
            r#"{"passthrough":{"media_type":"application/json"}}"#,
        ] {
            let delivery = parse_transcript_delivery_v1(text, &MediaLimitsV1::V1).expect(text);
            assert_eq!(delivery.to_json(), text);
        }
        let delivery = parse_transcript_delivery_v1(
            r#"{"json": {"text": {"$south.ref": {"blob": "b3", "transform": "as_is"}}, "n": 1.50}}"#,
            &MediaLimitsV1::V1,
        )
        .expect("valid");
        assert_eq!(
            delivery,
            TranscriptDeliveryV1::Json {
                template: r#"{"text":{"$south.ref":{"blob":"b3","transform":"as_is"}},"n":1.50}"#
                    .into()
            }
        );
        assert_eq!(delivery.referenced_blobs(), vec![BlobIdV1::from_index(3)]);
    }

    #[test]
    fn transcript_deliveries_refuse_what_reference_integrity_refuses() {
        let limits = MediaLimitsV1 { inline_string_bytes: 8, ..MediaLimitsV1::V1 };
        for (bad, expected) in [
            (
                r#"{"json":{"a":{"$south.ref":{"blob":"b0","transform":"base64"}}}}"#,
                SpeechContractErrorV1::ReferenceIntegrity,
            ),
            (
                r#"{"json":{"a":{"$south.ref":{"blob":"b0","transform":"as_is","media_type":"text/plain"}}}}"#,
                SpeechContractErrorV1::ReferenceIntegrity,
            ),
            (
                r#"{"json":{"a":{"$south.blob":{"id":"b0","bytes":1,"head":""}}}}"#,
                SpeechContractErrorV1::ReferenceIntegrity,
            ),
            (
                r#"{"json":{"a":{"$south.artifact":{"index":0,"as":"b64_json"}}}}"#,
                SpeechContractErrorV1::ReferenceIntegrity,
            ),
            (
                r#"{"json":{"a":{"$south.ref":{"blob":"b0","transform":"as_is"},"b":1}}}"#,
                SpeechContractErrorV1::ReferenceIntegrity,
            ),
            (r#"{"json":["123456789"]}"#, SpeechContractErrorV1::ReferenceIntegrity),
            (r#"{"json":{"123456789":1}}"#, SpeechContractErrorV1::ReferenceIntegrity),
            (
                r#"{"text":"123456789","media_type":"text/plain"}"#,
                SpeechContractErrorV1::ReferenceIntegrity,
            ),
            (r#"{"text":"x","media_type":"text plain"}"#, SpeechContractErrorV1::MediaType),
            (r#"{"passthrough":{"media_type":"json"}}"#, SpeechContractErrorV1::MediaType),
        ] {
            assert_eq!(parse_transcript_delivery_v1(bad, &limits), Err(expected), "{bad}");
        }
        for bad in [
            "{}",
            r#"{"text":"x"}"#,
            r#"{"media_type":"text/plain"}"#,
            r#"{"json":{},"media_type":"application/json"}"#,
            r#"{"json":{},"passthrough":{"media_type":"application/json"}}"#,
            r#"{"text":"x","media_type":"text/plain","passthrough":{"media_type":"text/plain"}}"#,
            r#"{"passthrough":{"media_type":"text/plain","status":200}}"#,
            r#"{"srt":"x"}"#,
        ] {
            assert!(
                matches!(
                    parse_transcript_delivery_v1(bad, &limits),
                    Err(SpeechContractErrorV1::Malformed(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn transcript_templates_fill_from_the_response_blobs() {
        let blobs = BlobSetV1::new([
            BlobV1::text(BlobIdV1::from_index(0), "a \"long\"\ntranscript".into()),
            BlobV1::file(BlobIdV1::from_index(1), vec![0xff]),
        ])
        .expect("ids");
        let filled = fill_transcript_template_v1(
            r#"{"text":{"$south.ref":{"blob":"b0","transform":"as_is"}},"language":"en"}"#,
            &blobs,
        )
        .expect("fills");
        assert_eq!(filled, r#"{"text":"a \"long\"\ntranscript","language":"en"}"#);
        assert!(matches!(
            fill_transcript_template_v1(
                r#"{"text":{"$south.ref":{"blob":"b1","transform":"as_is"}}}"#,
                &blobs
            ),
            Err(SpeechContractErrorV1::Fill(_))
        ));
        assert_eq!(
            fill_transcript_template_v1(
                r#"{"text":{"$south.ref":{"blob":"b7","transform":"as_is"}}}"#,
                &blobs
            ),
            Err(SpeechContractErrorV1::Fill(MediaDescriptorErrorV1::UnknownBlob))
        );
    }

    #[test]
    fn immutable_form_fields() {
        let fields: Vec<String> = vec!["model".into(), "timestamp_granularities".into()];
        assert_eq!(validate_immutable_form_fields_v1(&fields), Ok(()));
        assert_eq!(validate_immutable_form_fields_v1(&[]), Ok(()));
        assert!(form_field_is_immutable(&fields, "model"));
        assert!(form_field_is_immutable(&fields, "timestamp_granularities[]"));
        assert!(!form_field_is_immutable(&fields, "language"));
        assert!(!form_field_is_immutable(&fields, "model[][]"));
        for bad in [
            vec![String::new()],
            vec!["a b".to_owned()],
            vec!["model[]".to_owned()],
            vec!["a.b".to_owned()],
            vec!["model".to_owned(), "model".to_owned()],
            vec!["x".repeat(MAX_IMMUTABLE_FORM_FIELD_BYTES + 1)],
            (0..=MAX_IMMUTABLE_FORM_FIELDS).map(|index| format!("f{index}")).collect(),
        ] {
            assert_eq!(
                validate_immutable_form_fields_v1(&bad),
                Err(SpeechContractErrorV1::Immutability),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn capabilities() {
        let models = parse_speech_model_capabilities_v1(
            r#"[{"model":"whisper-1","operations":["transcribe","translate"],"metering_forms":["seconds"]},
                {"model":"gpt-4o-mini-tts","operations":["synthesize"],"metering_forms":["characters","tokens"]}]"#,
        )
        .expect("valid");
        assert_eq!(
            models[0].operations,
            [SpeechOperationV1::Transcribe, SpeechOperationV1::Translate]
        );
        for (bad, expected) in [
            (
                r#"[{"model":"","operations":["synthesize"],"metering_forms":["characters"]}]"#,
                SpeechContractErrorV1::OutOfRange,
            ),
            (
                r#"[{"model":"m","operations":[],"metering_forms":["characters"]}]"#,
                SpeechContractErrorV1::List,
            ),
            (
                r#"[{"model":"m","operations":["synthesize"],"metering_forms":[]}]"#,
                SpeechContractErrorV1::List,
            ),
            (
                r#"[{"model":"m","operations":["synthesize","synthesize"],"metering_forms":["tokens"]}]"#,
                SpeechContractErrorV1::List,
            ),
            (
                r#"[{"model":"m","operations":["synthesize"],"metering_forms":["seconds"]}]"#,
                SpeechContractErrorV1::Operation,
            ),
            (
                r#"[{"model":"m","operations":["translate"],"metering_forms":["characters"]}]"#,
                SpeechContractErrorV1::Operation,
            ),
        ] {
            let error = parse_speech_model_capabilities_v1(bad).expect_err(bad);
            assert!(error.to_string().contains(&expected.to_string()), "{bad}: {error}");
        }
        assert_eq!(
            parse_speech_model_capabilities_v1(
                r#"[{"model":"m","operations":["synthesize"],"metering_forms":["tokens"]},
                    {"model":"m","operations":["transcribe"],"metering_forms":["seconds"]}]"#
            ),
            Err(SpeechContractErrorV1::List)
        );
        assert!(
            parse_speech_model_capabilities_v1(
                r#"[{"model":"m","operations":["synthesize"],"metering_forms":["tokens"],"voices":["alloy"]}]"#
            )
            .is_err()
        );
    }

    #[test]
    fn call_contexts_tolerate_unknown_fields() {
        let context: SpeechCallContextV1 = serde_json::from_str(
            r#"{"upstream_model":"tts-1","operation":"synthesize","metering_required":["tokens"],"newer":true}"#,
        )
        .expect("tolerant");
        assert_eq!(context.metering_required, [SpeechMeteringFormV1::Tokens]);
    }

    fn tts_call(body: &str, immutability: &str) -> String {
        format!(
            r#"{{"facts":{TTS_FACTS},"descriptor":{{"method":"POST","path":"v1/audio/speech",
                "auth":{{"arm":"bearer","slot":"default"}},"body":{body}}},
                "response_body_form":"binary"{immutability},"state":{{"format":"mp3","n":[1]}}}}"#
        )
    }

    const JSON_BODY: &str =
        r#"{"json":{"template":{"model":"tts-1","input":"hello","voice":"alloy"}}}"#;
    const SSML_BODY: &str =
        r#"{"text":{"media_type":"application/ssml+xml","text":"<speak version='1.0'/>"}}"#;
    const MULTIPART_BODY: &str = r#"{"multipart":{"parts":[{"name":"model","value":"whisper-1"},{"name":"file","blob":"b0","transform":"as_is","filename":"a.mp3"}]}}"#;

    #[test]
    fn prepared_calls_round_trip_and_keep_source_text() {
        let text = tts_call(JSON_BODY, r#","immutable_body_paths":["model","voice"]"#);
        let prepared = parse_prepared_speech_call_v1(&text, &MediaLimitsV1::V1).expect("valid");
        assert_eq!(prepared.state, r#"{"format":"mp3","n":[1]}"#);
        assert_eq!(prepared.immutable_body_paths, Some(vec!["model".into(), "voice".into()]));
        assert_eq!(prepared.immutable_form_fields, None);
        assert_eq!(prepared.descriptor.path().as_str(), "v1/audio/speech");
        let again =
            parse_prepared_speech_call_v1(&prepared.to_json(), &MediaLimitsV1::V1).expect("valid");
        assert_eq!(again, prepared);

        let nothing = tts_call(JSON_BODY, r#","immutable_body_paths":null"#);
        let prepared = parse_prepared_speech_call_v1(&nothing, &MediaLimitsV1::V1).expect("valid");
        assert_eq!(prepared.immutable_body_paths, None);
        assert!(prepared.to_json().contains(r#""immutable_body_paths":null"#));

        let ssml = tts_call(SSML_BODY, r#","immutable_body_paths":null"#);
        let prepared = parse_prepared_speech_call_v1(&ssml, &MediaLimitsV1::V1).expect("valid");
        assert_eq!(
            parse_prepared_speech_call_v1(&prepared.to_json(), &MediaLimitsV1::V1),
            Ok(prepared)
        );

        let asr = format!(
            r#"{{"facts":{{"operation":"transcribe","metering_forms":["seconds"]}},
                "descriptor":{{"method":"POST","path":"v1/audio/transcriptions","body":{MULTIPART_BODY}}},
                "response_body_form":"json","response_elision_paths":["/text"],
                "immutable_form_fields":["model","response_format"]}}"#
        );
        let prepared = parse_prepared_speech_call_v1(&asr, &MediaLimitsV1::V1).expect("valid");
        assert_eq!(prepared.state, "null");
        assert_eq!(
            prepared.immutable_form_fields,
            Some(vec!["model".into(), "response_format".into()])
        );
        let again =
            parse_prepared_speech_call_v1(&prepared.to_json(), &MediaLimitsV1::V1).expect("valid");
        assert_eq!(again, prepared);
    }

    #[test]
    fn prepared_calls_refuse_a_declaration_that_does_not_fit_the_body() {
        for (body, immutability) in [
            (JSON_BODY, ""),
            (JSON_BODY, r#","immutable_form_fields":null"#),
            (JSON_BODY, r#","immutable_body_paths":[],"immutable_form_fields":[]"#),
            (JSON_BODY, r#","immutable_body_paths":["a..b"]"#),
            (JSON_BODY, r#","immutable_body_paths":["model","model"]"#),
            (SSML_BODY, ""),
            (SSML_BODY, r#","immutable_body_paths":[]"#),
            (SSML_BODY, r#","immutable_body_paths":["speak"]"#),
            (SSML_BODY, r#","immutable_form_fields":null"#),
            (MULTIPART_BODY, ""),
            (MULTIPART_BODY, r#","immutable_body_paths":null"#),
            (MULTIPART_BODY, r#","immutable_form_fields":["model[]"]"#),
            (MULTIPART_BODY, r#","immutable_body_paths":null,"immutable_form_fields":null"#),
        ] {
            assert_eq!(
                parse_prepared_speech_call_v1(&tts_call(body, immutability), &MediaLimitsV1::V1),
                Err(SpeechContractErrorV1::Immutability),
                "{body} {immutability}"
            );
        }
        assert_eq!(
            parse_prepared_speech_call_v1(
                &tts_call(r#""empty""#, r#","immutable_body_paths":null"#),
                &MediaLimitsV1::V1
            ),
            Err(SpeechContractErrorV1::Body)
        );
    }

    #[test]
    fn prepared_calls_refuse_what_exceeds_their_bounds() {
        let text = tts_call(JSON_BODY, r#","immutable_body_paths":null"#);
        let big_state =
            text.replace(r#"{"format":"mp3","n":[1]}"#, &format!(r#""{}""#, "x".repeat(9000)));
        assert_eq!(
            parse_prepared_speech_call_v1(&big_state, &MediaLimitsV1::V1),
            Err(SpeechContractErrorV1::PreparedBounds)
        );
        let paths = (0..=MediaLimitsV1::V1.elision_paths)
            .map(|index| format!(r#""/{index}/data""#))
            .collect::<Vec<_>>()
            .join(",");
        let many_paths = text.replace(
            r#""response_body_form":"binary""#,
            &format!(r#""response_body_form":"binary","response_elision_paths":[{paths}]"#),
        );
        assert_eq!(
            parse_prepared_speech_call_v1(&many_paths, &MediaLimitsV1::V1),
            Err(SpeechContractErrorV1::PreparedBounds)
        );
        let get = text.replace(r#""method":"POST""#, r#""method":"GET""#);
        assert_eq!(
            parse_prepared_speech_call_v1(&get, &MediaLimitsV1::V1),
            Err(SpeechContractErrorV1::Descriptor(MediaDescriptorErrorV1::Method))
        );
        for bad in [
            text.replace(r#""response_body_form":"binary""#, r#""response_body_form":"xml""#),
            text.replace(
                r#""response_body_form":"binary""#,
                r#""response_body_form":"binary","repeat":2"#,
            ),
            text.replace(r#""operation":"synthesize""#, r#""operation":"transcribe""#),
        ] {
            assert!(
                matches!(
                    parse_prepared_speech_call_v1(&bad, &MediaLimitsV1::V1),
                    Err(SpeechContractErrorV1::Malformed(_))
                ),
                "{bad}"
            );
        }
    }
}
