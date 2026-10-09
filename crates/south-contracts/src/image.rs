//! `contracts.image` v1: the facts, metering, artifacts and render templates of the
//! `image-adapter-v1` world (image record §7–§10).
//!
//! Normative record: `docs/design/2026-09-30-image-world.md`. Everything here is a fact the
//! upstream or the request carries, never a price: the host keeps the price list, the
//! pricing-form decision and every bound it checks (§9.3, §9.4). Outcomes carry an
//! `ErrorEnvelope`, so they live in the conformance crate with the other component traits (§15).
//! The host-to-component inputs ([`ImageCallContextV1`], [`ImageRenderContextV1`]) tolerate
//! unknown fields; everything a component returns is decoded strictly.

use crate::media::json::{self, Member, Node};
use crate::media::{
    MediaDescriptorErrorV1, MediaLimitsV1, MediaRequestDescriptorV1, PathPatternV1,
    RESERVED_KEY_PREFIX, ResponseBodyFormV1, is_media_type,
};
use crate::{JsonPointerV1, validate_immutable_body_paths};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt};
use thiserror::Error;

/// `contracts.image` v1.
pub const IMAGE_CONTRACT_VERSION: u16 = 1;
/// The most images one call may ask for, across every round.
pub const MAX_IMAGE_REQUESTED_OUTPUTS: u32 = 100;
/// The most inputs of one role a request may carry.
pub const MAX_IMAGE_INPUTS_PER_ROLE: u32 = 64;
/// The largest edge of a size fact, in pixels.
pub const MAX_IMAGE_EDGE: u32 = 65_536;
/// The most tier candidates one call may declare.
pub const MAX_TIER_CANDIDATES: usize = 16;
/// The longest upstream model id a capability declaration names.
pub const MAX_IMAGE_MODEL_ID_BYTES: usize = 256;
/// The most models one `model-capabilities` answer declares.
pub const MAX_IMAGE_MODELS: usize = 256;
/// The most request keys a model maps to input roles.
pub const MAX_IMAGE_ROLE_KEYS: usize = 32;
/// The key of an artifact position in a render template.
pub const ARTIFACT_KEY: &str = "$south.artifact";

/// Why an image contract value is refused. Diagnostics never echo the value.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ImageContractErrorV1 {
    /// A value does not have the shape of its type.
    #[error("image contract value is malformed: {0}")]
    Malformed(String),
    /// A tier word outside `[A-Za-z0-9_.-]{1,32}`.
    #[error("invalid tier word")]
    TierWord,
    /// Tier candidates are empty, too many, repeated, or do not contain their default.
    #[error("invalid tier candidates")]
    TierCandidates,
    /// A count, size or bound outside its range.
    #[error("image fact out of range")]
    OutOfRange,
    /// A list that must not be empty is, or one repeats an entry.
    #[error("image fact list is empty or repeats an entry")]
    List,
    /// A decimal string outside its grammar.
    #[error("invalid decimal amount")]
    Decimal,
    /// A media type outside the grammar.
    #[error("invalid media type")]
    MediaType,
    /// The descriptor is refused.
    #[error("image descriptor refused: {0}")]
    Descriptor(MediaDescriptorErrorV1),
    /// `repeat` outside `1..=`the media limit, or `state` above its bound.
    #[error("prepared image call exceeds its bounds")]
    PreparedBounds,
    /// Immutable body paths are malformed.
    #[error("invalid immutable body paths")]
    ImmutablePaths,
    /// A render template breaks reference integrity (§12.1).
    #[error("render template breaks reference integrity")]
    ReferenceIntegrity,
    /// A render template delivers an artifact in a way §7 does not allow.
    #[error("render template uses a disallowed artifact delivery")]
    Delivery,
}

fn malformed(error: impl fmt::Display) -> ImageContractErrorV1 {
    ImageContractErrorV1::Malformed(error.to_string())
}

/// `generate` or `edit` (§7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageOperationV1 {
    /// Text to image.
    Generate,
    /// Image to image.
    Edit,
}

/// The closed vocabulary of input roles (§8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageInputRoleV1 {
    /// An image to edit.
    InputImage,
    /// An image to take style or content from.
    ReferenceImage,
    /// An edit mask.
    Mask,
}

/// Input counts per role (§8, `inputs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageInputsV1 {
    /// Images to edit.
    #[serde(default)]
    pub input_image: u32,
    /// Reference images.
    #[serde(default)]
    pub reference_image: u32,
    /// Masks.
    #[serde(default)]
    pub mask: u32,
}

/// Pixel dimensions (§8, `size`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageSizeV1 {
    /// Width, from 1 to [`MAX_IMAGE_EDGE`].
    pub width: u32,
    /// Height, from 1 to [`MAX_IMAGE_EDGE`].
    pub height: u32,
}

/// One tier word: `[A-Za-z0-9_.-]{1,32}`, case-sensitive, matched exactly (R-2, ruled
/// 2026-10-09). The component reports the upstream's spelling (`1K`, `720P`, xAI's `1k`).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TierWordV1(String);

impl TierWordV1 {
    /// Validates one word.
    pub fn parse(word: &str) -> Result<Self, ImageContractErrorV1> {
        let valid = (1..=32).contains(&word.len())
            && word
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'));
        if valid { Ok(Self(word.to_owned())) } else { Err(ImageContractErrorV1::TierWord) }
    }

    /// Returns the word as reported.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TierWordV1 {
    type Error = ImageContractErrorV1;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<TierWordV1> for String {
    fn from(value: TierWordV1) -> Self {
        value.0
    }
}

impl fmt::Debug for TierWordV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The tier dimensions a model may report (§8). Closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TierDimensionV1 {
    /// Output resolution (`1K`, `2K`, `4K`, …).
    Resolution,
    /// Output quality (`low`, `medium`, `high`, …).
    Quality,
}

/// Tier words by dimension (§8, `tier`).
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageTierV1 {
    /// The resolution word, when the request determines one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<TierWordV1>,
    /// The quality word, when the request determines one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<TierWordV1>,
}

/// The candidate tiers when the upstream decides the tier (§8, `tier_candidates`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TierCandidatesV1 {
    /// The candidates, in the order the host tries them after the default (§9.4 item 4).
    pub candidates: Vec<ImageTierV1>,
    /// The tier billed when the upstream's quote does not identify one.
    pub default: ImageTierV1,
}

impl TierCandidatesV1 {
    fn validate(&self) -> Result<(), ImageContractErrorV1> {
        let distinct =
            self.candidates.iter().collect::<BTreeSet<_>>().len() == self.candidates.len();
        if self.candidates.is_empty()
            || self.candidates.len() > MAX_TIER_CANDIDATES
            || !distinct
            || !self.candidates.contains(&self.default)
        {
            return Err(ImageContractErrorV1::TierCandidates);
        }
        Ok(())
    }
}

/// The closed vocabulary of metering forms (§9.1): the unit a row can be billed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeteringFormV1 {
    /// Token buckets.
    Tokens,
    /// Delivered images.
    Images,
    /// Upstream credits.
    Credits,
    /// Succeeded rounds.
    Requests,
}

/// The closed vocabulary of token buckets (§9.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenBucketV1 {
    /// Text input tokens.
    TextInput,
    /// Image input tokens.
    ImageInput,
    /// Cached text input tokens.
    CachedTextInput,
    /// Cached image input tokens.
    CachedImageInput,
    /// Cached input tokens the upstream does not split into text and image.
    CachedInput,
    /// Text output tokens.
    TextOutput,
    /// Image output tokens.
    ImageOutput,
    /// Total input tokens.
    TotalInput,
    /// Total output tokens.
    TotalOutput,
}

/// Request-derived token bounds a component may tighten (§8, `bounds.max_tokens`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenBoundsV1 {
    /// Text input tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_input: Option<u64>,
    /// Image input tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_input: Option<u64>,
    /// Output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
}

/// A decimal amount: `0` or a positive integer without leading zeros, optionally followed by a
/// fraction of at most `PLACES` digits. Never negative, never an exponent.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DecimalV1<const PLACES: usize>(String);

/// Credits: at most six decimal places (§9.1).
pub type CreditsV1 = DecimalV1<6>;
/// A dollar amount: at most ten decimal places, so one xAI tick is representable (§9.1).
pub type UsdAmountV1 = DecimalV1<10>;

impl<const PLACES: usize> DecimalV1<PLACES> {
    /// Validates one amount.
    pub fn parse(text: &str) -> Result<Self, ImageContractErrorV1> {
        let (whole, fraction) =
            text.split_once('.').map_or((text, None), |(whole, fraction)| (whole, Some(fraction)));
        let whole_ok = !whole.is_empty()
            && whole.len() <= 18
            && whole.bytes().all(|byte| byte.is_ascii_digit())
            && (whole == "0" || !whole.starts_with('0'));
        let fraction_ok = fraction.is_none_or(|fraction| {
            (1..=PLACES).contains(&fraction.len())
                && fraction.bytes().all(|byte| byte.is_ascii_digit())
        });
        if whole_ok && fraction_ok {
            Ok(Self(text.to_owned()))
        } else {
            Err(ImageContractErrorV1::Decimal)
        }
    }

    /// Returns the amount as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<const PLACES: usize> TryFrom<String> for DecimalV1<PLACES> {
    type Error = ImageContractErrorV1;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl<const PLACES: usize> From<DecimalV1<PLACES>> for String {
    fn from(value: DecimalV1<PLACES>) -> Self {
        value.0
    }
}

impl<const PLACES: usize> fmt::Debug for DecimalV1<PLACES> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Bounds a component supplies only to tighten the host's (§8, `bounds`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageBoundsV1 {
    /// Images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_images: Option<u32>,
    /// Token buckets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<TokenBoundsV1>,
    /// Credits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_credits: Option<CreditsV1>,
}

/// The pre-dispatch facts of one call (§8). No prices.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ImageFactsWire", into = "ImageFactsWire")]
pub struct ImageFactsV1 {
    /// `generate` or `edit`.
    pub operation: ImageOperationV1,
    /// Input counts per role.
    pub inputs: ImageInputsV1,
    /// Images sent upstream × `repeat`, from 1 to [`MAX_IMAGE_REQUESTED_OUTPUTS`].
    pub requested_outputs: u32,
    /// Pixel size, when the request determines it.
    pub size: Option<ImageSizeV1>,
    /// Tier words by dimension.
    pub tier: ImageTierV1,
    /// Candidate tiers, when the upstream decides.
    pub tier_candidates: Option<TierCandidatesV1>,
    /// The metering forms this call reports on success; non-empty, no repeats.
    pub metering_forms: Vec<MeteringFormV1>,
    /// Tightening bounds.
    pub bounds: Option<ImageBoundsV1>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageFactsWire {
    operation: ImageOperationV1,
    #[serde(default)]
    inputs: ImageInputsV1,
    requested_outputs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<ImageSizeV1>,
    #[serde(default)]
    tier: ImageTierV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tier_candidates: Option<TierCandidatesV1>,
    metering_forms: Vec<MeteringFormV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bounds: Option<ImageBoundsV1>,
}

fn distinct<T: Ord>(items: &[T]) -> bool {
    items.iter().collect::<BTreeSet<_>>().len() == items.len()
}

impl TryFrom<ImageFactsWire> for ImageFactsV1 {
    type Error = ImageContractErrorV1;

    fn try_from(wire: ImageFactsWire) -> Result<Self, Self::Error> {
        let inputs_ok = [wire.inputs.input_image, wire.inputs.reference_image, wire.inputs.mask]
            .iter()
            .all(|count| *count <= MAX_IMAGE_INPUTS_PER_ROLE);
        let size_ok = wire.size.is_none_or(|size| {
            (1..=MAX_IMAGE_EDGE).contains(&size.width)
                && (1..=MAX_IMAGE_EDGE).contains(&size.height)
        });
        let bounds_ok = wire
            .bounds
            .as_ref()
            .is_none_or(|bounds| bounds.max_images.is_none_or(|images| images >= 1));
        if !inputs_ok
            || !size_ok
            || !bounds_ok
            || !(1..=MAX_IMAGE_REQUESTED_OUTPUTS).contains(&wire.requested_outputs)
        {
            return Err(ImageContractErrorV1::OutOfRange);
        }
        if wire.metering_forms.is_empty() || !distinct(&wire.metering_forms) {
            return Err(ImageContractErrorV1::List);
        }
        if let Some(candidates) = &wire.tier_candidates {
            candidates.validate()?;
        }
        Ok(Self {
            operation: wire.operation,
            inputs: wire.inputs,
            requested_outputs: wire.requested_outputs,
            size: wire.size,
            tier: wire.tier,
            tier_candidates: wire.tier_candidates,
            metering_forms: wire.metering_forms,
            bounds: wire.bounds,
        })
    }
}

impl From<ImageFactsV1> for ImageFactsWire {
    fn from(facts: ImageFactsV1) -> Self {
        Self {
            operation: facts.operation,
            inputs: facts.inputs,
            requested_outputs: facts.requested_outputs,
            size: facts.size,
            tier: facts.tier,
            tier_candidates: facts.tier_candidates,
            metering_forms: facts.metering_forms,
            bounds: facts.bounds,
        }
    }
}

/// Token counts by bucket (§9.1). Every bucket is nullable: `null` and `0` are different facts.
/// A bucket absent from the JSON reads `null`; serialization writes every bucket.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageTokenUsageV1 {
    /// Text input tokens.
    #[serde(default)]
    pub text_input: Option<u64>,
    /// Image input tokens.
    #[serde(default)]
    pub image_input: Option<u64>,
    /// Cached text input tokens.
    #[serde(default)]
    pub cached_text_input: Option<u64>,
    /// Cached image input tokens.
    #[serde(default)]
    pub cached_image_input: Option<u64>,
    /// Cached input tokens not split by kind.
    #[serde(default)]
    pub cached_input: Option<u64>,
    /// Text output tokens.
    #[serde(default)]
    pub text_output: Option<u64>,
    /// Image output tokens.
    #[serde(default)]
    pub image_output: Option<u64>,
    /// Total input tokens.
    #[serde(default)]
    pub total_input: Option<u64>,
    /// Total output tokens.
    #[serde(default)]
    pub total_output: Option<u64>,
}

impl ImageTokenUsageV1 {
    /// The count of one bucket.
    #[must_use]
    pub const fn get(&self, bucket: TokenBucketV1) -> Option<u64> {
        match bucket {
            TokenBucketV1::TextInput => self.text_input,
            TokenBucketV1::ImageInput => self.image_input,
            TokenBucketV1::CachedTextInput => self.cached_text_input,
            TokenBucketV1::CachedImageInput => self.cached_image_input,
            TokenBucketV1::CachedInput => self.cached_input,
            TokenBucketV1::TextOutput => self.text_output,
            TokenBucketV1::ImageOutput => self.image_output,
            TokenBucketV1::TotalInput => self.total_input,
            TokenBucketV1::TotalOutput => self.total_output,
        }
    }
}

/// The upstream's own quote, an evidence fact used only to recognise the served tier (§9.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamCostV1 {
    /// Always `USD`.
    pub currency: UpstreamCurrencyV1,
    /// The quoted amount.
    pub amount: UsdAmountV1,
}

/// The currencies an upstream quote may be in. Closed: `USD` only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpstreamCurrencyV1 {
    /// US dollars.
    #[serde(rename = "USD")]
    Usd,
}

/// The metering facts of one round (§9.1). Every fact is nullable; a missing required fact is
/// an `unknown` round, a missing evidence fact is `null` and the host applies its fallback.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageMeteringV1 {
    /// Token buckets (required for the `tokens` form: every declared bucket non-null).
    #[serde(default)]
    pub tokens: Option<ImageTokenUsageV1>,
    /// The image count the upstream reported (evidence on `succeeded`, required on
    /// `charged_failure`).
    #[serde(default)]
    pub images_reported: Option<u32>,
    /// Credits (required for the `credits` form).
    #[serde(default)]
    pub credits: Option<CreditsV1>,
    /// The upstream's quote (evidence).
    #[serde(default)]
    pub upstream_cost: Option<UpstreamCostV1>,
}

/// How an `inline` artifact's string is encoded (§10.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactEncodingV1 {
    /// Standard base64.
    Base64,
    /// A base64 `data:` URL.
    DataUrl,
}

/// Where one artifact is (§10.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImageArtifactV1 {
    /// A string in the response view, usually elided.
    Inline {
        /// Where the string is.
        pointer: JsonPointerV1,
        /// How it is encoded.
        encoding: ArtifactEncodingV1,
        /// The image's media type.
        media_type: String,
    },
    /// The whole response body (`binary` form).
    Body {
        /// The image's media type.
        media_type: String,
    },
    /// A URL string in the unelided upstream response; the component chooses which of the
    /// upstream's URLs, never what the URL is.
    Url {
        /// Where the URL is.
        pointer: JsonPointerV1,
        /// The image's media type, when the component knows it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
    },
}

impl ImageArtifactV1 {
    /// Checks the media types.
    pub fn validate(&self) -> Result<(), ImageContractErrorV1> {
        let media_type = match self {
            Self::Inline { media_type, .. } | Self::Body { media_type } => {
                Some(media_type.as_str())
            }
            Self::Url { media_type, .. } => media_type.as_deref(),
        };
        if media_type.is_some_and(|value| !is_media_type(value)) {
            return Err(ImageContractErrorV1::MediaType);
        }
        Ok(())
    }

    /// Whether a render template may deliver this artifact as `delivery` (§7's table):
    /// `b64_json` always; `url` only for a `url` artifact.
    #[must_use]
    pub const fn allows(&self, delivery: ArtifactDeliveryV1) -> bool {
        match delivery {
            ArtifactDeliveryV1::B64Json => true,
            ArtifactDeliveryV1::Url => matches!(self, Self::Url { .. }),
        }
    }
}

/// How a render template delivers one artifact (§7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactDeliveryV1 {
    /// As a plain base64 string.
    B64Json,
    /// As the upstream's URL, passed through.
    Url,
}

/// The response formats a model can render (`response_format`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageResponseFormatV1 {
    /// `b64_json`.
    B64Json,
    /// `url`.
    Url,
}

/// One request key and the role it maps to (§8; the first key that appears wins).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputRoleKeyV1 {
    /// The northbound request key (a JSON member or multipart field name).
    pub key: String,
    /// The role.
    pub role: ImageInputRoleV1,
}

/// What one model declares that is bound to the dialect (§7, revised per R-1): never limits,
/// ranges or default words, which are catalog data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ImageModelCapabilitiesWire", into = "ImageModelCapabilitiesWire")]
pub struct ImageModelCapabilitiesV1 {
    /// The upstream model id.
    pub model: String,
    /// Supported operations; non-empty.
    pub operations: Vec<ImageOperationV1>,
    /// Request keys and their roles, in priority order.
    pub input_roles: Vec<InputRoleKeyV1>,
    /// Renderable response formats; non-empty.
    pub response_formats: Vec<ImageResponseFormatV1>,
    /// Metering forms it can report; non-empty.
    pub metering_forms: Vec<MeteringFormV1>,
    /// Token buckets the upstream reports; non-empty exactly when `tokens` is a form.
    pub token_buckets: Vec<TokenBucketV1>,
    /// Names of the tier dimensions it may report.
    pub tier_dimensions: Vec<TierDimensionV1>,
    /// Whether `prepare` may repeat one descriptor (one image per upstream call).
    pub repeat: bool,
    /// Request-side elision paths.
    pub request_elision_paths: Vec<PathPatternV1>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageModelCapabilitiesWire {
    model: String,
    operations: Vec<ImageOperationV1>,
    #[serde(default)]
    input_roles: Vec<InputRoleKeyV1>,
    response_formats: Vec<ImageResponseFormatV1>,
    metering_forms: Vec<MeteringFormV1>,
    #[serde(default)]
    token_buckets: Vec<TokenBucketV1>,
    #[serde(default)]
    tier_dimensions: Vec<TierDimensionV1>,
    #[serde(default)]
    repeat: bool,
    #[serde(default)]
    request_elision_paths: Vec<PathPatternV1>,
}

impl TryFrom<ImageModelCapabilitiesWire> for ImageModelCapabilitiesV1 {
    type Error = ImageContractErrorV1;

    fn try_from(wire: ImageModelCapabilitiesWire) -> Result<Self, Self::Error> {
        if wire.model.is_empty() || wire.model.len() > MAX_IMAGE_MODEL_ID_BYTES {
            return Err(ImageContractErrorV1::OutOfRange);
        }
        let keys: Vec<&str> = wire.input_roles.iter().map(|entry| entry.key.as_str()).collect();
        let lists_ok = !wire.operations.is_empty()
            && distinct(&wire.operations)
            && !wire.response_formats.is_empty()
            && distinct(&wire.response_formats)
            && !wire.metering_forms.is_empty()
            && distinct(&wire.metering_forms)
            && distinct(&wire.token_buckets)
            && distinct(&wire.tier_dimensions)
            && distinct(&keys)
            && keys.iter().all(|key| !key.is_empty())
            && wire.input_roles.len() <= MAX_IMAGE_ROLE_KEYS
            && wire.request_elision_paths.len() <= MediaLimitsV1::V1.elision_paths
            && wire.metering_forms.contains(&MeteringFormV1::Tokens)
                != wire.token_buckets.is_empty();
        if !lists_ok {
            return Err(ImageContractErrorV1::List);
        }
        Ok(Self {
            model: wire.model,
            operations: wire.operations,
            input_roles: wire.input_roles,
            response_formats: wire.response_formats,
            metering_forms: wire.metering_forms,
            token_buckets: wire.token_buckets,
            tier_dimensions: wire.tier_dimensions,
            repeat: wire.repeat,
            request_elision_paths: wire.request_elision_paths,
        })
    }
}

impl From<ImageModelCapabilitiesV1> for ImageModelCapabilitiesWire {
    fn from(value: ImageModelCapabilitiesV1) -> Self {
        Self {
            model: value.model,
            operations: value.operations,
            input_roles: value.input_roles,
            response_formats: value.response_formats,
            metering_forms: value.metering_forms,
            token_buckets: value.token_buckets,
            tier_dimensions: value.tier_dimensions,
            repeat: value.repeat,
            request_elision_paths: value.request_elision_paths,
        }
    }
}

/// Decodes a `model-capabilities` answer: at most [`MAX_IMAGE_MODELS`] models, each named once.
pub fn parse_image_model_capabilities_v1(
    json_text: &str,
) -> Result<Vec<ImageModelCapabilitiesV1>, ImageContractErrorV1> {
    let models: Vec<ImageModelCapabilitiesV1> =
        serde_json::from_str(json_text).map_err(malformed)?;
    let names: Vec<&str> = models.iter().map(|model| model.model.as_str()).collect();
    if models.len() > MAX_IMAGE_MODELS || !distinct(&names) {
        return Err(ImageContractErrorV1::List);
    }
    Ok(models)
}

/// What the host tells `prepare` beside the request view (§7). Tolerates unknown fields, so a
/// newer host can add context an older component ignores.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageCallContextV1 {
    /// The routed upstream model id.
    pub upstream_model: String,
    /// The metering forms the host's pricing decision requires (§9.3).
    pub metering_required: Vec<MeteringFormV1>,
}

/// What the host tells `render` (§7): the component has no clock. Tolerates unknown fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRenderContextV1 {
    /// Seconds since the Unix epoch for the client body's `created`.
    pub created: i64,
}

/// What `prepare` returns (§7, §8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedImageCallV1 {
    /// The pre-dispatch facts.
    pub facts: ImageFactsV1,
    /// The upstream request.
    pub descriptor: MediaRequestDescriptorV1,
    /// How many times the host sends the descriptor, one after another.
    pub repeat: u8,
    /// The declared response body form.
    pub response_body_form: ResponseBodyFormV1,
    /// Response-side elision paths.
    pub response_elision_paths: Vec<PathPatternV1>,
    /// Request-body paths the host must not rewrite (task contract 6 §4).
    pub immutable_body_paths: Vec<String>,
    /// The bounded, secret-free state `parse-response` and `render` receive, as compact JSON.
    pub state: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedWire {
    facts: ImageFactsV1,
    #[allow(dead_code)]
    descriptor: serde::de::IgnoredAny,
    #[serde(default = "one")]
    repeat: u8,
    response_body_form: ResponseBodyFormV1,
    #[serde(default)]
    response_elision_paths: Vec<PathPatternV1>,
    #[serde(default)]
    immutable_body_paths: Vec<String>,
    #[allow(dead_code)]
    #[serde(default)]
    state: serde::de::IgnoredAny,
}

const fn one() -> u8 {
    1
}

/// Decodes and validates a `prepare` result.
pub fn parse_prepared_image_call_v1(
    json_text: &str,
    limits: &MediaLimitsV1,
) -> Result<PreparedImageCallV1, ImageContractErrorV1> {
    let wire: PreparedWire = serde_json::from_str(json_text).map_err(malformed)?;
    if !(1..=limits.repeat).contains(&wire.repeat)
        || wire.response_elision_paths.len() > limits.elision_paths
    {
        return Err(ImageContractErrorV1::PreparedBounds);
    }
    validate_immutable_body_paths(&wire.immutable_body_paths)
        .map_err(|_| ImageContractErrorV1::ImmutablePaths)?;
    let tree =
        json::parse(json_text.as_bytes()).map_err(|_| malformed("prepared call is not JSON"))?;
    let descriptor_text =
        member_text(&tree, "descriptor").ok_or_else(|| malformed("descriptor missing"))?;
    let descriptor = MediaRequestDescriptorV1::parse(&descriptor_text, limits)
        .map_err(ImageContractErrorV1::Descriptor)?;
    let state = member_text(&tree, "state").unwrap_or_else(|| "null".to_owned());
    if state.len() > limits.state_bytes {
        return Err(ImageContractErrorV1::PreparedBounds);
    }
    Ok(PreparedImageCallV1 {
        facts: wire.facts,
        descriptor,
        repeat: wire.repeat,
        response_body_form: wire.response_body_form,
        response_elision_paths: wire.response_elision_paths,
        immutable_body_paths: wire.immutable_body_paths,
        state,
    })
}

impl PreparedImageCallV1 {
    /// Serializes the call compactly; [`parse_prepared_image_call_v1`] of the result gives back an
    /// equal call. The descriptor and the state are written as their source text.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\"facts\":");
        out.push_str(&serde_json::to_string(&self.facts).unwrap_or_default());
        out.push_str(",\"descriptor\":");
        out.push_str(&self.descriptor.to_json());
        out.push_str(",\"repeat\":");
        out.push_str(&self.repeat.to_string());
        out.push_str(",\"response_body_form\":");
        out.push_str(&serde_json::to_string(&self.response_body_form).unwrap_or_default());
        out.push_str(",\"response_elision_paths\":");
        out.push_str(&serde_json::to_string(&self.response_elision_paths).unwrap_or_default());
        out.push_str(",\"immutable_body_paths\":");
        out.push_str(&serde_json::to_string(&self.immutable_body_paths).unwrap_or_default());
        out.push_str(",\"state\":");
        out.push_str(&self.state);
        out.push('}');
        out
    }
}

/// The compact source text of one top-level member of a JSON object.
///
/// Members keep source order and numbers are written as they were; `None` when `document` is not
/// an object with that key. Hosts and codecs use it to keep a component's JSON (a template, a
/// state) byte for byte.
#[must_use]
pub fn member_source_text(document: &str, key: &str) -> Option<String> {
    member_text(&json::parse(document.as_bytes()).ok()?, key)
}

fn member_text(node: &Node, key: &str) -> Option<String> {
    let Node::Object(members) = node else { return None };
    let member = members.iter().find(|member| member.key.decoded == key)?;
    let mut text = String::new();
    json::write_compact(&member.value, &mut text);
    Some(text)
}

/// One artifact position in a render template: `{"$south.artifact": {"index", "as"}}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactMarkerV1 {
    /// The artifact's position in the succeeded rounds' artifacts, in round order.
    pub index: usize,
    /// How to deliver it.
    #[serde(rename = "as")]
    pub delivery: ArtifactDeliveryV1,
}

/// Checks a render template's reference integrity (§12.1) against the call's artifacts.
///
/// Every `$south.artifact` points to an existing artifact and uses a delivery its form allows;
/// no other `$south.` key appears (a `$south.blob` or `$south.ref` in output is refused); no
/// string exceeds the fallback threshold, so a component cannot smuggle bytes out of the sandbox.
/// Returns the markers in template order.
pub fn check_render_template_v1(
    template: &str,
    artifacts: &[ImageArtifactV1],
    limits: &MediaLimitsV1,
) -> Result<Vec<ArtifactMarkerV1>, ImageContractErrorV1> {
    let tree =
        json::parse(template.as_bytes()).map_err(|_| ImageContractErrorV1::ReferenceIntegrity)?;
    let mut markers = Vec::new();
    check_node(&tree, artifacts, limits, &mut markers)?;
    Ok(markers)
}

fn marker_of(members: &[Member]) -> Result<Option<ArtifactMarkerV1>, ImageContractErrorV1> {
    match members {
        [only] if only.key.decoded == ARTIFACT_KEY => {
            let mut text = String::new();
            json::write_compact(&only.value, &mut text);
            serde_json::from_str(&text)
                .map(Some)
                .map_err(|_| ImageContractErrorV1::ReferenceIntegrity)
        }
        _ if members.iter().any(|member| member.key.decoded == ARTIFACT_KEY) => {
            Err(ImageContractErrorV1::ReferenceIntegrity)
        }
        _ => Ok(None),
    }
}

fn check_node(
    node: &Node,
    artifacts: &[ImageArtifactV1],
    limits: &MediaLimitsV1,
    markers: &mut Vec<ArtifactMarkerV1>,
) -> Result<(), ImageContractErrorV1> {
    match node {
        Node::Scalar(_) => Ok(()),
        Node::String(string) => {
            if string.decoded.len() > limits.inline_string_bytes {
                Err(ImageContractErrorV1::ReferenceIntegrity)
            } else {
                Ok(())
            }
        }
        Node::Array(items) => {
            items.iter().try_for_each(|item| check_node(item, artifacts, limits, markers))
        }
        Node::Object(members) => {
            if let Some(marker) = marker_of(members)? {
                let artifact =
                    artifacts.get(marker.index).ok_or(ImageContractErrorV1::ReferenceIntegrity)?;
                if !artifact.allows(marker.delivery) {
                    return Err(ImageContractErrorV1::Delivery);
                }
                markers.push(marker);
                return Ok(());
            }
            for Member { key, value } in members {
                if key.decoded.starts_with(RESERVED_KEY_PREFIX)
                    || key.decoded.len() > limits.inline_string_bytes
                {
                    return Err(ImageContractErrorV1::ReferenceIntegrity);
                }
                check_node(value, artifacts, limits, markers)?;
            }
            Ok(())
        }
    }
}

/// The value the host delivers for one artifact: plain base64 bytes, or the upstream's URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveredArtifactV1 {
    /// Verified image bytes, written as plain base64 where the template says `b64_json`.
    Bytes(Vec<u8>),
    /// The upstream URL, written as is where the template says `url`.
    Url(String),
}

/// Fills a checked render template (§10.2 step 3).
///
/// Each marker becomes a JSON string — the artifact's base64 for `b64_json`, its URL for `url` —
/// and everything else is copied as written. `delivered[i]` is artifact `i`; a marker whose delivery does not match what was
/// delivered is refused.
pub fn fill_render_template_v1(
    template: &str,
    delivered: &[DeliveredArtifactV1],
) -> Result<String, ImageContractErrorV1> {
    let mut tree =
        json::parse(template.as_bytes()).map_err(|_| ImageContractErrorV1::ReferenceIntegrity)?;
    fill_node(&mut tree, delivered)?;
    let mut out = String::new();
    json::write_compact(&tree, &mut out);
    Ok(out)
}

fn fill_node(
    node: &mut Node,
    delivered: &[DeliveredArtifactV1],
) -> Result<(), ImageContractErrorV1> {
    match node {
        Node::Scalar(_) | Node::String(_) => Ok(()),
        Node::Array(items) => items.iter_mut().try_for_each(|item| fill_node(item, delivered)),
        Node::Object(members) => {
            if let Some(marker) = marker_of(members)? {
                let value = match (marker.delivery, delivered.get(marker.index)) {
                    (ArtifactDeliveryV1::B64Json, Some(DeliveredArtifactV1::Bytes(bytes))) => {
                        crate::media::encode_base64(bytes)
                    }
                    (ArtifactDeliveryV1::Url, Some(DeliveredArtifactV1::Url(url))) => url.clone(),
                    _ => return Err(ImageContractErrorV1::Delivery),
                };
                let mut literal = String::with_capacity(value.len() + 2);
                json::write_string(&value, &mut literal);
                *node = Node::Scalar(literal);
                return Ok(());
            }
            members.iter_mut().try_for_each(|member| fill_node(&mut member.value, delivered))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_words_are_exact_and_bounded() {
        for good in ["1K", "1k", "720P", "high", "a.b_c-d", &"x".repeat(32)] {
            assert!(TierWordV1::parse(good).is_ok(), "{good}");
        }
        for bad in ["", "1 K", "k/2", "é", &"x".repeat(33)] {
            assert!(TierWordV1::parse(bad).is_err(), "{bad}");
        }
        assert_ne!(
            TierWordV1::parse("1K").expect("valid"),
            TierWordV1::parse("1k").expect("valid")
        );
    }

    #[test]
    fn decimals() {
        for good in ["0", "12", "0.5", "1.123456"] {
            assert!(CreditsV1::parse(good).is_ok(), "{good}");
        }
        for bad in ["", "-1", "01", "1.", ".5", "1.1234567", "1e3", "1,5"] {
            assert!(CreditsV1::parse(bad).is_err(), "{bad}");
        }
        assert!(UsdAmountV1::parse("0.0000001234").is_ok());
        assert!(UsdAmountV1::parse("0.00000012345").is_err());
    }

    #[test]
    fn facts_parse_and_validate() {
        let facts: ImageFactsV1 = serde_json::from_str(
            r#"{"operation":"generate","inputs":{"reference_image":1},"requested_outputs":2,
                "size":{"width":1024,"height":1024},"tier":{"resolution":"2K"},
                "tier_candidates":{"candidates":[{"resolution":"1k"},{"resolution":"2k"}],"default":{"resolution":"1k"}},
                "metering_forms":["tokens"],"bounds":{"max_tokens":{"output":5000}}}"#,
        )
        .expect("valid");
        assert_eq!(facts.inputs.reference_image, 1);
        assert_eq!(facts.tier.resolution.as_ref().map(TierWordV1::as_str), Some("2K"));
        let round_trip: ImageFactsV1 =
            serde_json::from_str(&serde_json::to_string(&facts).expect("serializable"))
                .expect("valid");
        assert_eq!(round_trip, facts);
        for bad in [
            r#"{"operation":"generate","requested_outputs":0,"metering_forms":["images"]}"#,
            r#"{"operation":"generate","requested_outputs":101,"metering_forms":["images"]}"#,
            r#"{"operation":"generate","requested_outputs":1,"metering_forms":[]}"#,
            r#"{"operation":"generate","requested_outputs":1,"metering_forms":["images","images"]}"#,
            r#"{"operation":"generate","requested_outputs":1,"metering_forms":["images"],"size":{"width":0,"height":1}}"#,
            r#"{"operation":"generate","requested_outputs":1,"metering_forms":["images"],"tier_candidates":{"candidates":[{"resolution":"1k"}],"default":{"resolution":"2k"}}}"#,
            r#"{"operation":"generate","requested_outputs":1,"metering_forms":["images"],"price":1}"#,
            r#"{"operation":"upscale","requested_outputs":1,"metering_forms":["images"]}"#,
        ] {
            assert!(serde_json::from_str::<ImageFactsV1>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn metering_keeps_null_apart_from_zero() {
        let metering: ImageMeteringV1 = serde_json::from_str(
            r#"{"tokens":{"text_input":10,"image_output":0},"upstream_cost":{"currency":"USD","amount":"0.0400000000"}}"#,
        )
        .expect("valid");
        let tokens = metering.tokens.expect("tokens");
        assert_eq!(tokens.get(TokenBucketV1::ImageOutput), Some(0));
        assert_eq!(tokens.get(TokenBucketV1::TextOutput), None);
        assert_eq!(metering.images_reported, None);
        let json = serde_json::to_value(&metering).expect("serializable");
        assert!(json["tokens"]["text_output"].is_null());
        assert!(json["images_reported"].is_null());
        assert!(serde_json::from_str::<ImageMeteringV1>(r#"{"credits":"-1"}"#).is_err());
        assert!(
            serde_json::from_str::<ImageMeteringV1>(
                r#"{"upstream_cost":{"currency":"EUR","amount":"1"}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn capabilities_tie_buckets_to_the_tokens_form() {
        let good = r#"[{"model":"gpt-image-1","operations":["generate","edit"],
            "input_roles":[{"key":"image","role":"input_image"},{"key":"mask","role":"mask"}],
            "response_formats":["b64_json"],"metering_forms":["tokens"],
            "token_buckets":["text_input","image_input","cached_input","total_output"],
            "request_elision_paths":["/image"]}]"#;
        let models = parse_image_model_capabilities_v1(good).expect("valid");
        assert!(!models[0].repeat);
        for bad in [
            r#"[{"model":"m","operations":["generate"],"response_formats":["b64_json"],"metering_forms":["tokens"]}]"#,
            r#"[{"model":"m","operations":["generate"],"response_formats":["b64_json"],"metering_forms":["images"],"token_buckets":["text_input"]}]"#,
            r#"[{"model":"m","operations":[],"response_formats":["b64_json"],"metering_forms":["images"]}]"#,
            r#"[{"model":"m","operations":["generate"],"response_formats":["b64_json"],"metering_forms":["images"],"input_roles":[{"key":"a","role":"mask"},{"key":"a","role":"mask"}]}]"#,
            r#"[{"model":"m","operations":["generate"],"response_formats":["b64_json"],"metering_forms":["images"]},{"model":"m","operations":["edit"],"response_formats":["url"],"metering_forms":["images"]}]"#,
        ] {
            assert!(parse_image_model_capabilities_v1(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn prepared_calls_keep_the_descriptor_text_and_bound_state() {
        let text = r#"{"facts":{"operation":"edit","requested_outputs":1,"metering_forms":["images"]},
            "descriptor":{"method":"POST","path":"v1/images/edits","auth":{"arm":"bearer","slot":"default"},
              "body":{"multipart":{"parts":[{"name":"prompt","value":"x"},{"name":"image","blob":"b0","transform":"as_is"}]}}},
            "response_body_form":"json","response_elision_paths":["/data/*/b64_json"],
            "immutable_body_paths":["prompt"],"state":{"n":1,"b":[true]}}"#;
        let prepared = parse_prepared_image_call_v1(text, &MediaLimitsV1::V1).expect("valid");
        assert_eq!(prepared.repeat, 1);
        assert_eq!(prepared.state, r#"{"n":1,"b":[true]}"#);
        assert_eq!(prepared.descriptor.path().as_str(), "v1/images/edits");
        let too_many =
            text.replace(r#""response_body_form""#, r#""repeat":11,"response_body_form""#);
        assert_eq!(
            parse_prepared_image_call_v1(&too_many, &MediaLimitsV1::V1),
            Err(ImageContractErrorV1::PreparedBounds)
        );
        let big_state =
            text.replace(r#"{"n":1,"b":[true]}"#, &format!(r#""{}""#, "x".repeat(9000)));
        assert_eq!(
            parse_prepared_image_call_v1(&big_state, &MediaLimitsV1::V1),
            Err(ImageContractErrorV1::PreparedBounds)
        );
    }

    fn artifacts() -> Vec<ImageArtifactV1> {
        serde_json::from_str(
            r#"[{"form":"inline","pointer":"/data/0/b64_json","encoding":"base64","media_type":"image/png"},
                {"form":"url","pointer":"/data/1/url"}]"#,
        )
        .expect("valid")
    }

    #[test]
    fn render_templates_are_checked_and_filled() {
        let template = r#"{"created":1,"data":[{"b64_json":{"$south.artifact":{"index":0,"as":"b64_json"}}},{"url":{"$south.artifact":{"index":1,"as":"url"}}}],"usage":{"total_tokens":3}}"#;
        let markers =
            check_render_template_v1(template, &artifacts(), &MediaLimitsV1::V1).expect("valid");
        assert_eq!(markers.len(), 2);
        let filled = fill_render_template_v1(
            template,
            &[
                DeliveredArtifactV1::Bytes(b"hi".to_vec()),
                DeliveredArtifactV1::Url("https://cdn.example/a.png".into()),
            ],
        )
        .expect("fills");
        assert_eq!(
            filled,
            r#"{"created":1,"data":[{"b64_json":"aGk="},{"url":"https://cdn.example/a.png"}],"usage":{"total_tokens":3}}"#
        );
    }

    #[test]
    fn render_templates_refuse_what_section_12_refuses() {
        let limits = MediaLimitsV1 { inline_string_bytes: 8, ..MediaLimitsV1::V1 };
        for (template, expected) in [
            (r#"[{"$south.artifact":{"index":0,"as":"url"}}]"#, ImageContractErrorV1::Delivery),
            (
                r#"[{"$south.artifact":{"index":2,"as":"b64_json"}}]"#,
                ImageContractErrorV1::ReferenceIntegrity,
            ),
            (r#"[{"$south.blob":{"id":"b0"}}]"#, ImageContractErrorV1::ReferenceIntegrity),
            (
                r#"[{"$south.ref":{"blob":"b0","transform":"as_is"}}]"#,
                ImageContractErrorV1::ReferenceIntegrity,
            ),
            (
                r#"[{"$south.artifact":{"index":0,"as":"b64_json"},"x":1}]"#,
                ImageContractErrorV1::ReferenceIntegrity,
            ),
            (r#"["123456789"]"#, ImageContractErrorV1::ReferenceIntegrity),
        ] {
            assert_eq!(
                check_render_template_v1(template, &artifacts(), &limits),
                Err(expected),
                "{template}"
            );
        }
        assert_eq!(
            fill_render_template_v1(
                r#"[{"$south.artifact":{"index":0,"as":"b64_json"}}]"#,
                &[DeliveredArtifactV1::Url("u".into())]
            ),
            Err(ImageContractErrorV1::Delivery)
        );
    }

    #[test]
    fn artifact_forms() {
        let artifacts = artifacts();
        assert!(artifacts[0].allows(ArtifactDeliveryV1::B64Json));
        assert!(!artifacts[0].allows(ArtifactDeliveryV1::Url));
        assert!(artifacts[1].allows(ArtifactDeliveryV1::Url));
        assert!(
            serde_json::from_str::<ImageArtifactV1>(r#"{"form":"body","media_type":"image png"}"#)
                .expect("shape")
                .validate()
                .is_err()
        );
        assert!(
            serde_json::from_str::<ImageArtifactV1>(r#"{"form":"url","pointer":"x"}"#).is_err()
        );
    }
}
