//! Versioned task facts and the non-secret recovery locator; no wire derives.

use crate::image::{ARTIFACT_KEY, ArtifactEncodingV1};
use crate::media::{BlobIdV1, BlobSetV1, RESERVED_KEY_PREFIX, decode_base64, parse_data_url_v1};
use crate::{
    HostMintedValuesV1, JsonPointerV1, MAX_ARTIFACT_REF_BYTES, MAX_ARTIFACT_URLS, RelativePathV1,
    TaskFailureKindV1,
};
use serde_json::Value;
use std::fmt;
use thiserror::Error;

/// The supported locator schema, independent of the component world version.
pub const TASK_LOCATOR_SCHEMA_VERSION: u16 = 1;

/// A refused task-v2 value. Diagnostics never echo the rejected input.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum TaskContractErrorV2 {
    /// The locator schema is not supported.
    #[error("unsupported task locator schema")]
    UnsupportedLocatorSchema,
    /// The route is not a bounded relative path.
    #[error("invalid task locator route")]
    InvalidLocator,
    /// Metering facts must be finite and nonnegative.
    #[error("invalid task usage facts")]
    InvalidUsage,
    /// A scalar exceeds its bound or carries a non-finite number.
    #[error("invalid task artifact scalar")]
    InvalidScalar,
    /// An artifact reference violates its boundary.
    #[error("invalid task artifact reference")]
    InvalidArtifact,
    /// The host render context is incomplete or exceeds its bounds.
    #[error("invalid task render context")]
    InvalidRenderContext,
    /// A request estimate is invalid or cannot fit the unit range.
    #[error("invalid task request estimate")]
    InvalidRequestEstimate,
    /// An observation exceeds its text or artifact bounds.
    #[error("invalid task observation")]
    InvalidObservation,
    /// Immutable request-body paths are malformed, duplicated or exceed their bounds.
    #[error("invalid immutable body paths")]
    InvalidImmutableBodyPaths,
    /// An artifact role word is not in the closed vocabulary.
    #[error("invalid task artifact role")]
    InvalidArtifactRole,
    /// An inline artifact's pointer does not land on a blob of the call's view, or the blob does
    /// not decode to a non-empty artifact within the host's limit (contract 8). The host observes
    /// again rather than settling: the upstream answers the same pointer with fresh bytes.
    #[error("unresolved inline task artifact")]
    InlineArtifactUnresolved,
    /// A rendered body names an artifact that does not exist, uses a malformed artifact marker, or
    /// carries another `$south.` key (contract 8). The host refuses delivery.
    #[error("task render breaks reference integrity")]
    ReferenceIntegrity,
}

/// At most this many immutable request-body paths per prepared task.
pub const MAX_IMMUTABLE_BODY_PATHS: usize = 64;
/// Each immutable path is at most this many bytes.
pub const MAX_IMMUTABLE_BODY_PATH_BYTES: usize = 256;

/// Validates the request-body paths a host must not rewrite (contract 6, D6).
///
/// Dotted object paths only: every segment is `[A-Za-z0-9_-]+`, no array indices. A host must
/// leave each path, its ancestors and its descendants untouched when it adds its own fields.
pub fn validate_immutable_body_paths(paths: &[String]) -> Result<(), TaskContractErrorV2> {
    let segment_ok = |segment: &str| {
        !segment.is_empty()
            && segment.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    let valid = paths.len() <= MAX_IMMUTABLE_BODY_PATHS
        && paths.iter().enumerate().all(|(index, path)| {
            path.len() <= MAX_IMMUTABLE_BODY_PATH_BYTES
                && path.split('.').all(segment_ok)
                && !paths[..index].contains(path)
        });
    if valid { Ok(()) } else { Err(TaskContractErrorV2::InvalidImmutableBodyPaths) }
}

/// A component-interpreted route; never a request body, secret or upstream id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskLocatorV2 {
    route: RelativePathV1,
}
impl TaskLocatorV2 {
    /// Constructs a versioned, validated recovery route.
    pub fn new(schema_version: u16, route: &str) -> Result<Self, TaskContractErrorV2> {
        if schema_version != TASK_LOCATOR_SCHEMA_VERSION {
            return Err(TaskContractErrorV2::UnsupportedLocatorSchema);
        }
        let route =
            RelativePathV1::parse(route).map_err(|_| TaskContractErrorV2::InvalidLocator)?;
        Ok(Self { route })
    }
    /// Returns the fixed supported schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        TASK_LOCATOR_SCHEMA_VERSION
    }
    /// Returns the original relative route.
    #[must_use]
    pub fn route(&self) -> &str {
        self.route.as_str()
    }
}

/// The closed vocabulary of token buckets a task component may report (contract 8).
///
/// The image world's nine words with the same spelling and meaning (image record §9.1), plus the
/// modality and reasoning buckets video needs. `reasoning_output` is **outside** `total_output`,
/// as the upstreams that report thoughts separately say it (`total = input + output + thought`);
/// it is not the IR's `reasoning_tokens ⊂ output_tokens` convention, and a host folding buckets
/// into a ledger adds the two. A new word is a contract bump, not a default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaskTokenBucketV2 {
    /// Text input tokens.
    TextInput,
    /// Image input tokens.
    ImageInput,
    /// Audio input tokens.
    AudioInput,
    /// Video input tokens.
    VideoInput,
    /// Cached text input tokens.
    CachedTextInput,
    /// Cached image input tokens.
    CachedImageInput,
    /// Cached input tokens the upstream does not split by modality.
    CachedInput,
    /// Text output tokens.
    TextOutput,
    /// Image output tokens.
    ImageOutput,
    /// Audio output tokens.
    AudioOutput,
    /// Video output tokens.
    VideoOutput,
    /// Reasoning ("thought") tokens, outside `total_output`.
    ReasoningOutput,
    /// Total input tokens.
    TotalInput,
    /// Total output tokens, reasoning excluded.
    TotalOutput,
}
impl TaskTokenBucketV2 {
    /// Every bucket, in wire order.
    pub const ALL: [Self; 14] = [
        Self::TextInput,
        Self::ImageInput,
        Self::AudioInput,
        Self::VideoInput,
        Self::CachedTextInput,
        Self::CachedImageInput,
        Self::CachedInput,
        Self::TextOutput,
        Self::ImageOutput,
        Self::AudioOutput,
        Self::VideoOutput,
        Self::ReasoningOutput,
        Self::TotalInput,
        Self::TotalOutput,
    ];
    /// The wire spelling.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::TextInput => "text_input",
            Self::ImageInput => "image_input",
            Self::AudioInput => "audio_input",
            Self::VideoInput => "video_input",
            Self::CachedTextInput => "cached_text_input",
            Self::CachedImageInput => "cached_image_input",
            Self::CachedInput => "cached_input",
            Self::TextOutput => "text_output",
            Self::ImageOutput => "image_output",
            Self::AudioOutput => "audio_output",
            Self::VideoOutput => "video_output",
            Self::ReasoningOutput => "reasoning_output",
            Self::TotalInput => "total_input",
            Self::TotalOutput => "total_output",
        }
    }
    /// Parses the wire spelling; a word outside the vocabulary is `None`, never a default.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|bucket| bucket.word() == word)
    }
    const fn slot(self) -> usize {
        self as usize
    }
}

/// Token counts by bucket (contract 8): at least one bucket reported, each at most `i64::MAX`.
/// A bucket that is not reported reads `None`, which is different from a reported zero.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TaskTokenBucketsV2 {
    counts: [Option<u64>; 14],
}
impl TaskTokenBucketsV2 {
    /// Collects reported buckets; an empty set, a bucket reported twice or a count above
    /// `i64::MAX` is refused.
    pub fn new(
        entries: impl IntoIterator<Item = (TaskTokenBucketV2, u64)>,
    ) -> Result<Self, TaskContractErrorV2> {
        let mut counts = [None; 14];
        for (bucket, count) in entries {
            let slot = &mut counts[bucket.slot()];
            if slot.is_some() || i64::try_from(count).is_err() {
                return Err(TaskContractErrorV2::InvalidUsage);
            }
            *slot = Some(count);
        }
        if counts.iter().all(Option::is_none) {
            return Err(TaskContractErrorV2::InvalidUsage);
        }
        Ok(Self { counts })
    }
    /// The count of one bucket, `None` when it was not reported.
    #[must_use]
    pub const fn get(&self, bucket: TaskTokenBucketV2) -> Option<u64> {
        self.counts[bucket.slot()]
    }
    /// The reported buckets, in wire order.
    pub fn iter(&self) -> impl Iterator<Item = (TaskTokenBucketV2, u64)> + '_ {
        TaskTokenBucketV2::ALL
            .into_iter()
            .filter_map(|bucket| self.get(bucket).map(|count| (bucket, count)))
    }
}
impl fmt::Debug for TaskTokenBucketsV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_map()
            .entries(self.iter().map(|(bucket, count)| (bucket.word(), count)))
            .finish()
    }
}

/// Independent upstream metering facts; no price or host settlement decision.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TaskUsageFactsV2 {
    seconds: Option<f64>,
    milliunits: Option<i64>,
    tokens: Option<i64>,
    outputs: Option<u32>,
    token_buckets: Option<TaskTokenBucketsV2>,
}
impl TaskUsageFactsV2 {
    /// Validates all present facts without conflating zero with missing.
    pub fn new(
        seconds: Option<f64>,
        milliunits: Option<i64>,
        tokens: Option<i64>,
    ) -> Result<Self, TaskContractErrorV2> {
        if seconds.is_some_and(|value| !value.is_finite() || value < 0.0)
            || milliunits.is_some_and(|value| value < 0)
            || tokens.is_some_and(|value| value < 0)
        {
            return Err(TaskContractErrorV2::InvalidUsage);
        }
        Ok(Self { seconds, milliunits, tokens, outputs: None, token_buckets: None })
    }
    /// Attaches token buckets (contract 8). Their presence is what tells the host the bill is
    /// bucketed, so a single `tokens` count beside them is refused.
    pub const fn with_token_buckets(
        mut self,
        token_buckets: Option<TaskTokenBucketsV2>,
    ) -> Result<Self, TaskContractErrorV2> {
        if token_buckets.is_some() && self.tokens.is_some() {
            return Err(TaskContractErrorV2::InvalidUsage);
        }
        self.token_buckets = token_buckets;
        Ok(self)
    }
    /// Returns the reported token buckets, if the upstream's bill is bucketed.
    #[must_use]
    pub const fn token_buckets(&self) -> Option<TaskTokenBucketsV2> {
        self.token_buckets
    }
    /// Attaches the delivered output count (contract 6); absent is distinct from zero.
    pub fn with_outputs(mut self, outputs: Option<i64>) -> Result<Self, TaskContractErrorV2> {
        self.outputs = outputs
            .map(|count| u32::try_from(count).map_err(|_| TaskContractErrorV2::InvalidUsage))
            .transpose()?;
        Ok(self)
    }
    /// Returns the delivered clip or image count the upstream reported.
    #[must_use]
    pub const fn outputs(&self) -> Option<u32> {
        self.outputs
    }
    /// Returns reported elapsed seconds.
    #[must_use]
    pub const fn seconds(&self) -> Option<f64> {
        self.seconds
    }
    /// Returns reported thousandths of provider billing units.
    #[must_use]
    pub const fn milliunits(&self) -> Option<i64> {
        self.milliunits
    }
    /// Returns reported tokens.
    #[must_use]
    pub const fn tokens(&self) -> Option<i64> {
        self.tokens
    }
}

/// Closed JSON scalar categories, without publishing serde's wire types.
#[derive(Clone, Debug, PartialEq)]
pub enum TaskScalarV2 {
    /// An explicit null or an omitted upstream scalar rendered as null.
    Null,
    /// A bounded byte-exact string.
    String(String),
    /// A signed JSON integer.
    Signed(i64),
    /// An unsigned JSON integer, including values above `i64::MAX`.
    Unsigned(u64),
    /// A finite JSON floating-point number.
    Float(f64),
}
impl TaskScalarV2 {
    /// Checks values constructed through the public enum variants.
    pub const fn validate(&self) -> Result<(), TaskContractErrorV2> {
        match self {
            Self::String(value) if value.len() > MAX_ARTIFACT_REF_BYTES => {
                Err(TaskContractErrorV2::InvalidScalar)
            }
            Self::Float(value) if !value.is_finite() => Err(TaskContractErrorV2::InvalidScalar),
            _ => Ok(()),
        }
    }
}

/// The part a direct artifact plays in a task's result (task contract 7).
///
/// Hosts count, deliver and store only [`Self::Primary`] artifacts; every other role is a
/// companion the component reports beside a primary and places itself when rendering. The
/// vocabulary is closed: a new role is a contract bump, not a new word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskArtifactRoleV2 {
    /// A deliverable output of the task. Spelled `null` on the wire.
    Primary,
    /// The last frame of a primary video, reported after it. Spelled `"last_frame"` on the wire.
    LastFrame,
}
impl TaskArtifactRoleV2 {
    /// Every role, in wire order.
    pub const ALL: [Self; 2] = [Self::Primary, Self::LastFrame];
    /// The wire spelling: `None` for the primary role, a word for every companion role.
    #[must_use]
    pub const fn word(self) -> Option<&'static str> {
        match self {
            Self::Primary => None,
            Self::LastFrame => Some("last_frame"),
        }
    }
    /// Parses the wire spelling; an unknown word is refused rather than defaulted.
    pub fn from_word(word: Option<&str>) -> Result<Self, TaskContractErrorV2> {
        Self::ALL
            .into_iter()
            .find(|role| role.word() == word)
            .ok_or(TaskContractErrorV2::InvalidArtifactRole)
    }
    /// Whether the host counts and delivers an artifact of this role.
    #[must_use]
    pub const fn is_primary(self) -> bool {
        matches!(self, Self::Primary)
    }
}

/// A direct artifact URL with the upstream's scalar id and duration facts.
#[derive(Clone, PartialEq)]
pub struct TaskArtifactV2 {
    url: String,
    id: TaskScalarV2,
    duration: TaskScalarV2,
    fetch_with_credential: bool,
    role: TaskArtifactRoleV2,
}
impl TaskArtifactV2 {
    /// Validates a bounded URL and scalar facts without interpreting them. The artifact is a
    /// primary output until [`Self::with_role`] says otherwise.
    pub fn new(
        url: &str,
        id: TaskScalarV2,
        duration: TaskScalarV2,
    ) -> Result<Self, TaskContractErrorV2> {
        if url.is_empty() || url.len() > MAX_ARTIFACT_REF_BYTES {
            return Err(TaskContractErrorV2::InvalidArtifact);
        }
        id.validate()?;
        duration.validate()?;
        Ok(Self {
            url: url.to_owned(),
            id,
            duration,
            fetch_with_credential: false,
            role: TaskArtifactRoleV2::Primary,
        })
    }
    /// Assigns the part this artifact plays (contract 7): a companion role keeps it out of the
    /// host's output count, delivery envelope and storage.
    #[must_use]
    pub const fn with_role(mut self, role: TaskArtifactRoleV2) -> Self {
        self.role = role;
        self
    }
    /// The part this artifact plays in the task's result.
    #[must_use]
    pub const fn role(&self) -> TaskArtifactRoleV2 {
        self.role
    }
    /// Marks the URL as fetchable only with the task's bound credential (contract 6, D5): the host
    /// fetches it with the same authentication as the submission and never hands it to a client.
    #[must_use]
    pub const fn with_bound_credential(mut self) -> Self {
        self.fetch_with_credential = true;
        self
    }
    /// Whether the host must fetch this artifact with the bound credential.
    #[must_use]
    pub const fn fetch_with_credential(&self) -> bool {
        self.fetch_with_credential
    }
    /// Returns the original artifact URL; callers must not log it.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
    /// Returns the upstream artifact identifier scalar.
    #[must_use]
    pub const fn id(&self) -> &TaskScalarV2 {
        &self.id
    }
    /// Returns the upstream duration scalar.
    #[must_use]
    pub const fn duration(&self) -> &TaskScalarV2 {
        &self.duration
    }
}
impl fmt::Debug for TaskArtifactV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TaskArtifactV2")
            .field("url_byte_count", &self.url.len())
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

/// An artifact whose bytes are a string in the upstream response, held by the host (contract 8).
///
/// The pointer names which of the upstream's strings is the artifact; it never carries bytes, so
/// the component cannot invent content. It must land on a `$south.blob` node of the view the
/// component was given (the image world's `inline` form, image record §10.1).
#[derive(Clone, PartialEq, Eq)]
pub struct TaskInlineArtifactV2 {
    pointer: JsonPointerV1,
    encoding: ArtifactEncodingV1,
    media_type: String,
    role: TaskArtifactRoleV2,
}
impl TaskInlineArtifactV2 {
    /// Validates a concrete, non-root RFC 6901 pointer and a `type/subtype` media type. The
    /// artifact is a primary output until [`Self::with_role`] says otherwise.
    pub fn new(
        pointer: &str,
        encoding: ArtifactEncodingV1,
        media_type: &str,
    ) -> Result<Self, TaskContractErrorV2> {
        let pointer = JsonPointerV1::parse(pointer)
            .ok()
            .filter(|pointer| !pointer.as_str().is_empty())
            .ok_or(TaskContractErrorV2::InvalidArtifact)?;
        if !crate::media::is_media_type(media_type) {
            return Err(TaskContractErrorV2::InvalidArtifact);
        }
        Ok(Self {
            pointer,
            encoding,
            media_type: media_type.to_owned(),
            role: TaskArtifactRoleV2::Primary,
        })
    }
    /// Assigns the part this artifact plays (contract 7).
    #[must_use]
    pub const fn with_role(mut self, role: TaskArtifactRoleV2) -> Self {
        self.role = role;
        self
    }
    /// The pointer into the response view.
    #[must_use]
    pub fn pointer(&self) -> &str {
        self.pointer.as_str()
    }
    /// How the string is encoded.
    #[must_use]
    pub const fn encoding(&self) -> ArtifactEncodingV1 {
        self.encoding
    }
    /// The artifact's media type.
    #[must_use]
    pub fn media_type(&self) -> &str {
        &self.media_type
    }
    /// The part this artifact plays in the task's result.
    #[must_use]
    pub const fn role(&self) -> TaskArtifactRoleV2 {
        self.role
    }
}
impl fmt::Debug for TaskInlineArtifactV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TaskInlineArtifactV2")
            .field("pointer", &self.pointer.as_str())
            .field("encoding", &self.encoding)
            .field("media_type", &self.media_type)
            .field("role", &self.role)
            .finish()
    }
}

/// Direct artifacts, an id requiring another fetch, or an inline result.
#[derive(Clone, PartialEq)]
pub enum TaskArtifactRefV2 {
    /// A bounded nonempty set of direct artifacts.
    Urls(Vec<TaskArtifactV2>),
    /// A bounded provider file identifier.
    FileId(String),
    /// A terminal result that has no separate artifact reference.
    None,
    /// Contract 8: artifacts whose bytes are strings in the upstream response, held by the host.
    /// The same bounds as [`Self::Urls`], and the pointers are distinct.
    Inline(Vec<TaskInlineArtifactV2>),
}
impl TaskArtifactRefV2 {
    /// Validates direct artifact references.
    pub fn urls(urls: Vec<TaskArtifactV2>) -> Result<Self, TaskContractErrorV2> {
        let value = Self::Urls(urls);
        value.validate()?;
        Ok(value)
    }
    /// Validates inline artifact references (contract 8).
    pub fn inline(items: Vec<TaskInlineArtifactV2>) -> Result<Self, TaskContractErrorV2> {
        let value = Self::Inline(items);
        value.validate()?;
        Ok(value)
    }
    /// The number of primary artifacts: what a host counts, stores and names `art_0` …
    /// (contract 7). A file id is one artifact; an empty result none.
    #[must_use]
    pub fn primary_count(&self) -> usize {
        match self {
            Self::Urls(items) => items.iter().filter(|item| item.role().is_primary()).count(),
            Self::Inline(items) => items.iter().filter(|item| item.role().is_primary()).count(),
            Self::FileId(_) => 1,
            Self::None => 0,
        }
    }
    /// Validates a file reference.
    pub fn file_id(id: &str) -> Result<Self, TaskContractErrorV2> {
        let value = Self::FileId(id.to_owned());
        value.validate()?;
        Ok(value)
    }
    /// Rechecks public enum construction at a component boundary. A direct set must stay within
    /// [`MAX_ARTIFACT_URLS`] (companion roles included) and carry at least one primary artifact:
    /// a result made only of companions has nothing for the host to deliver.
    pub fn validate(&self) -> Result<(), TaskContractErrorV2> {
        match self {
            Self::Urls(items)
                if items.is_empty()
                    || items.len() > MAX_ARTIFACT_URLS
                    || !items.iter().any(|item| item.role().is_primary()) =>
            {
                Err(TaskContractErrorV2::InvalidArtifact)
            }
            Self::FileId(id) if id.is_empty() || id.len() > MAX_ARTIFACT_REF_BYTES => {
                Err(TaskContractErrorV2::InvalidArtifact)
            }
            Self::Inline(items)
                if items.is_empty()
                    || items.len() > MAX_ARTIFACT_URLS
                    || !items.iter().any(|item| item.role().is_primary())
                    || items.iter().enumerate().any(|(index, item)| {
                        items[..index].iter().any(|earlier| earlier.pointer == item.pointer)
                    }) =>
            {
                Err(TaskContractErrorV2::InvalidArtifact)
            }
            _ => Ok(()),
        }
    }
}
impl fmt::Debug for TaskArtifactRefV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Urls(items) => formatter.debug_tuple("Urls").field(&items.len()).finish(),
            Self::FileId(id) => {
                formatter.debug_struct("FileId").field("byte_count", &id.len()).finish()
            }
            Self::None => formatter.write_str("None"),
            Self::Inline(items) => formatter.debug_tuple("Inline").field(items).finish(),
        }
    }
}

/// Why a task failed, as its upstream said it (task contract 9, the served models record,
/// `docs/design/2026-10-28-served-models-prefetch-and-failure-category.md` §4).
///
/// Set only from an explicit upstream signal, the discipline of
/// [`TaskFailureKindV1::ProviderExpired`]: nothing is inferred from a message text. The words are
/// the host's `error.reason` words they map to. Whether the host could fetch a succeeded task's
/// artifacts is the host's own fact, so `artifact_unavailable` is not a category (FC-Q2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskFailureCategoryV1 {
    /// The upstream refused or filtered the output for its content.
    ContentPolicy,
    /// The upstream says the task timed out.
    UpstreamTimeout,
}

impl TaskFailureCategoryV1 {
    /// Every category, in declaration order.
    pub const ALL: [Self; 2] = [Self::ContentPolicy, Self::UpstreamTimeout];

    /// Returns the frozen vocabulary word for this category.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::ContentPolicy => "content_policy",
            Self::UpstreamTimeout => "upstream_timeout",
        }
    }
}

/// The upstream's execution facts, independent of host billing and delivery.
#[derive(Clone, Debug, PartialEq)]
#[expect(
    clippy::large_enum_variant,
    reason = "contract 8's token buckets keep usage `Copy`; boxing the variant would change the \
              public pattern every host matches, for a value moved a few times per poll"
)]
pub enum TaskObservationV2 {
    /// Explicit queued/running distinction with the original status word.
    Progress { running: bool, status_word: String },
    /// Execution succeeded, carrying every available metering dimension.
    Succeeded { artifacts: TaskArtifactRefV2, usage: TaskUsageFactsV2 },
    /// Explicit upstream terminal failure; no timeout synthesized by a clock. `category` (task
    /// contract 9) says why, only from an explicit upstream signal; absent keeps the host's own
    /// mapping.
    Failed {
        kind: TaskFailureKindV1,
        code: Option<String>,
        message: Option<String>,
        category: Option<TaskFailureCategoryV1>,
    },
    /// The observation was unavailable or unrecognized.
    Unknown { reason: String },
}
impl TaskObservationV2 {
    /// Validates public enum values before encoding or consuming them.
    pub fn validate(&self) -> Result<(), TaskContractErrorV2> {
        let valid = match self {
            Self::Progress { status_word, .. } => status_word.len() <= MAX_ARTIFACT_REF_BYTES,
            Self::Succeeded { artifacts, .. } => return artifacts.validate(),
            Self::Failed { code, message, .. } => {
                code.as_ref().is_none_or(|s| s.len() <= MAX_ARTIFACT_REF_BYTES)
                    && message.as_ref().is_none_or(|s| s.len() <= MAX_ARTIFACT_REF_BYTES)
            }
            Self::Unknown { reason } => reason.len() <= MAX_ARTIFACT_REF_BYTES,
        };
        if valid { Ok(()) } else { Err(TaskContractErrorV2::InvalidObservation) }
    }
}

/// Host-provided public response metadata, with no callback or credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRenderContextV2 {
    task_id: String,
    created: i64,
    model: String,
    provider: String,
    upstream_task_id: Option<String>,
}
impl TaskRenderContextV2 {
    /// Validates explicit host metadata; a synchronous result may have no upstream id.
    pub fn new(
        task_id: &str,
        created: i64,
        model: &str,
        provider: &str,
        upstream_task_id: Option<&str>,
    ) -> Result<Self, TaskContractErrorV2> {
        HostMintedValuesV1::new(task_id, None)
            .map_err(|_| TaskContractErrorV2::InvalidRenderContext)?;
        let valid = |s: &str| {
            !s.is_empty() && s.len() <= MAX_ARTIFACT_REF_BYTES && !s.chars().any(char::is_control)
        };
        if !valid(model) || !valid(provider) || upstream_task_id.is_some_and(|id| !valid(id)) {
            return Err(TaskContractErrorV2::InvalidRenderContext);
        }
        Ok(Self {
            task_id: task_id.to_owned(),
            created,
            model: model.to_owned(),
            provider: provider.to_owned(),
            upstream_task_id: upstream_task_id.map(str::to_owned),
        })
    }
    /// Returns the gateway task id.
    #[must_use]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    /// Returns the host-selected Unix creation time.
    #[must_use]
    pub const fn created(&self) -> i64 {
        self.created
    }
    /// Returns the public model name.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }
    /// Returns the public provider name.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }
    /// Returns the original upstream task id when one exists.
    #[must_use]
    pub fn upstream_task_id(&self) -> Option<&str> {
        self.upstream_task_id.as_deref()
    }
}

/// Request-only estimation basis, independent of reported upstream usage.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskRequestEstimateV2 {
    requested_seconds: Option<f64>,
    milliunits_per_second: Option<i64>,
    resolution: Option<String>,
    input_image_count: Option<u32>,
    tokens_per_second: Option<i64>,
    requested_outputs: Option<u32>,
    bucket_ceilings: Option<TaskTokenBucketsV2>,
}
impl TaskRequestEstimateV2 {
    /// Validates protocol request duration and its optional unit rate.
    pub fn new(
        requested_seconds: Option<f64>,
        milliunits_per_second: Option<i64>,
    ) -> Result<Self, TaskContractErrorV2> {
        if requested_seconds.is_some_and(|seconds| !seconds.is_finite() || seconds < 0.0)
            || milliunits_per_second.is_some_and(|rate| rate < 0)
        {
            return Err(TaskContractErrorV2::InvalidRequestEstimate);
        }
        Ok(Self {
            requested_seconds,
            milliunits_per_second,
            resolution: None,
            input_image_count: None,
            tokens_per_second: None,
            requested_outputs: None,
            bucket_ceilings: None,
        })
    }
    /// Attaches per-bucket ceilings that may only tighten the host's own reservation bound
    /// (contract 8). The host compares reported buckets with its own bound, never with these.
    #[must_use]
    pub const fn with_bucket_ceilings(mut self, ceilings: Option<TaskTokenBucketsV2>) -> Self {
        self.bucket_ceilings = ceilings;
        self
    }
    /// Returns the component's per-bucket ceilings, if it states any.
    #[must_use]
    pub const fn bucket_ceilings(&self) -> Option<TaskTokenBucketsV2> {
        self.bucket_ceilings
    }
    /// Attaches the protocol token rate and the requested output count (contract 6).
    ///
    /// The token rate is a protocol unit rate like `milliunits_per_second`: the component derives
    /// it from its own formula (e.g. resolution), the host multiplies time and price. The output
    /// count is at least one; absent means the component makes no statement.
    pub fn with_output_facts(
        mut self,
        tokens_per_second: Option<i64>,
        requested_outputs: Option<i64>,
    ) -> Result<Self, TaskContractErrorV2> {
        if tokens_per_second.is_some_and(|rate| rate < 0) {
            return Err(TaskContractErrorV2::InvalidRequestEstimate);
        }
        self.requested_outputs = requested_outputs
            .map(|count| {
                u32::try_from(count)
                    .ok()
                    .filter(|count| *count >= 1)
                    .ok_or(TaskContractErrorV2::InvalidRequestEstimate)
            })
            .transpose()?;
        self.tokens_per_second = tokens_per_second;
        Ok(self)
    }
    /// Returns the protocol token rate, without any monetary price.
    #[must_use]
    pub const fn tokens_per_second(&self) -> Option<i64> {
        self.tokens_per_second
    }
    /// Returns the requested clip or image count; missing is distinct from one.
    #[must_use]
    pub const fn requested_outputs(&self) -> Option<u32> {
        self.requested_outputs
    }
    /// Estimates tokens using explicit host time, with the same rounding and bound as
    /// [`Self::estimate_milliunits`]; never reports actual usage.
    pub fn estimate_tokens(&self, host_seconds: f64) -> Result<Option<i64>, TaskContractErrorV2> {
        estimate_at_rate(self.tokens_per_second, host_seconds)
    }
    /// Attaches normalized request facts without inferring a price or missing values.
    pub fn with_input_facts(
        mut self,
        resolution: Option<&str>,
        input_image_count: Option<u32>,
    ) -> Result<Self, TaskContractErrorV2> {
        if resolution.is_some_and(|value| {
            value.is_empty()
                || value.len() > 32
                || !value.bytes().all(|b| b.is_ascii_alphanumeric())
        }) {
            return Err(TaskContractErrorV2::InvalidRequestEstimate);
        }
        self.resolution = resolution.map(str::to_owned);
        self.input_image_count = input_image_count;
        Ok(self)
    }
    /// Returns the component-normalized resolution, if reported.
    #[must_use]
    pub fn resolution(&self) -> Option<&str> {
        self.resolution.as_deref()
    }
    /// Returns the actual number of input images; missing is distinct from zero.
    #[must_use]
    pub const fn input_image_count(&self) -> Option<u32> {
        self.input_image_count
    }
    /// Returns the duration actually present in the prepared request.
    #[must_use]
    pub const fn requested_seconds(&self) -> Option<f64> {
        self.requested_seconds
    }
    /// Returns the protocol unit rate, without any monetary price.
    #[must_use]
    pub const fn milliunits_per_second(&self) -> Option<i64> {
        self.milliunits_per_second
    }
    /// Estimates units using explicit host time; never reports actual usage.
    pub fn estimate_milliunits(
        &self,
        host_seconds: f64,
    ) -> Result<Option<i64>, TaskContractErrorV2> {
        estimate_at_rate(self.milliunits_per_second, host_seconds)
    }
}

/// `ceil(rate × host_seconds)`, refusing non-finite / negative time and results at or above the
/// exclusive `i64` bound. Shared by the milliunit and token estimates so both round alike.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "estimation uses floating seconds; the rounded result is checked below the exclusive i64 bound before conversion"
)]
fn estimate_at_rate(
    rate: Option<i64>,
    host_seconds: f64,
) -> Result<Option<i64>, TaskContractErrorV2> {
    if !host_seconds.is_finite() || host_seconds < 0.0 {
        return Err(TaskContractErrorV2::InvalidRequestEstimate);
    }
    let Some(rate) = rate else {
        return Ok(None);
    };
    let estimate = (rate as f64 * host_seconds).ceil();
    // i64::MAX rounds up to 2^63 in f64: equality must also be rejected.
    if !estimate.is_finite() || estimate >= 9_223_372_036_854_775_808.0 {
        return Err(TaskContractErrorV2::InvalidRequestEstimate);
    }
    Ok(Some(estimate as i64))
}

/// Resolves one inline artifact against the view and blobs of the call that produced it
/// (contract 8, host obligation).
///
/// The pointer must land on a `{"$south.blob": {"id", …}}` node of `view`, the blob must be one of
/// `blobs`, and its text must decode under the artifact's encoding (strict standard base64, or a
/// `data:` URL) to between 1 and `max_bytes` bytes.
///
/// Every failure is [`TaskContractErrorV2::InlineArtifactUnresolved`]; a host observes again
/// rather than settling, because the upstream returns the same interaction by id.
pub fn resolve_task_inline_artifact_v1(
    view: &str,
    blobs: &BlobSetV1,
    artifact: &TaskInlineArtifactV2,
    max_bytes: usize,
) -> Result<Vec<u8>, TaskContractErrorV2> {
    let unresolved = || TaskContractErrorV2::InlineArtifactUnresolved;
    let view: Value = serde_json::from_str(view).map_err(|_| unresolved())?;
    let node =
        view.pointer(artifact.pointer()).and_then(Value::as_object).ok_or_else(unresolved)?;
    let blob = match node.iter().next() {
        Some((key, placeholder)) if node.len() == 1 && key == crate::media::BLOB_KEY => placeholder,
        _ => return Err(unresolved()),
    };
    let id = blob
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| BlobIdV1::parse(id).ok())
        .ok_or_else(unresolved)?;
    let blob = blobs.get(&id).filter(|blob| blob.is_text()).ok_or_else(unresolved)?;
    let text = std::str::from_utf8(blob.bytes()).map_err(|_| unresolved())?;
    let bytes = match artifact.encoding() {
        ArtifactEncodingV1::Base64 => decode_base64(text).map_err(|_| unresolved())?,
        ArtifactEncodingV1::DataUrl => parse_data_url_v1(text).map_err(|_| unresolved())?.1,
    };
    if bytes.is_empty() || bytes.len() > max_bytes {
        return Err(unresolved());
    }
    Ok(bytes)
}

/// Replaces every artifact marker `{"$south.artifact": {"index": i}}` in a rendered body with
/// `path_of(i)` (contract 8, host obligation), returning how many it replaced.
///
/// `i` counts the primary artifacts of `artifacts` (contract 7: `art_0` is the first primary). A
/// marker that is not exactly that shape, names no primary artifact, or any other `$south.` key
/// left in the body refuses delivery with [`TaskContractErrorV2::ReferenceIntegrity`]; the body is
/// then left partly replaced and must be discarded.
pub fn fill_task_artifact_markers_v1(
    render: &mut Value,
    artifacts: &TaskArtifactRefV2,
    path_of: impl Fn(usize) -> String,
) -> Result<usize, TaskContractErrorV2> {
    fn marker_index(node: &serde_json::Map<String, Value>, primaries: usize) -> Option<usize> {
        let marker = node.get(ARTIFACT_KEY)?.as_object()?;
        let index = marker.get("index")?.as_u64().and_then(|index| usize::try_from(index).ok())?;
        (node.len() == 1 && marker.len() == 1 && index < primaries).then_some(index)
    }
    fn visit(
        node: &mut Value,
        primaries: usize,
        path_of: &dyn Fn(usize) -> String,
        filled: &mut usize,
    ) -> Result<(), TaskContractErrorV2> {
        match node {
            Value::Array(items) => {
                items.iter_mut().try_for_each(|item| visit(item, primaries, path_of, filled))
            }
            Value::Object(members) if members.contains_key(ARTIFACT_KEY) => {
                let index = marker_index(members, primaries)
                    .ok_or(TaskContractErrorV2::ReferenceIntegrity)?;
                *node = Value::String(path_of(index));
                *filled += 1;
                Ok(())
            }
            Value::Object(members) => {
                if members.keys().any(|key| key.starts_with(RESERVED_KEY_PREFIX)) {
                    return Err(TaskContractErrorV2::ReferenceIntegrity);
                }
                members.values_mut().try_for_each(|value| visit(value, primaries, path_of, filled))
            }
            _ => Ok(()),
        }
    }
    artifacts.validate()?;
    let mut filled = 0;
    visit(render, artifacts.primary_count(), &path_of, &mut filled)?;
    Ok(filled)
}
