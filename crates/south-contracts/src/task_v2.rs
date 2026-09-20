//! Versioned task facts and the non-secret recovery locator; no wire derives.

use crate::{
    HostMintedValuesV1, MAX_ARTIFACT_REF_BYTES, MAX_ARTIFACT_URLS, RelativePathV1,
    TaskFailureKindV1,
};
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

/// Independent upstream metering facts; no price or host settlement decision.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TaskUsageFactsV2 {
    seconds: Option<f64>,
    milliunits: Option<i64>,
    tokens: Option<i64>,
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
        Ok(Self { seconds, milliunits, tokens })
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

/// A direct artifact URL with the upstream's scalar id and duration facts.
#[derive(Clone, PartialEq)]
pub struct TaskArtifactV2 {
    url: String,
    id: TaskScalarV2,
    duration: TaskScalarV2,
}
impl TaskArtifactV2 {
    /// Validates a bounded URL and scalar facts without interpreting them.
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
        Ok(Self { url: url.to_owned(), id, duration })
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
            .finish_non_exhaustive()
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
}
impl TaskArtifactRefV2 {
    /// Validates direct artifact references.
    pub fn urls(urls: Vec<TaskArtifactV2>) -> Result<Self, TaskContractErrorV2> {
        let value = Self::Urls(urls);
        value.validate()?;
        Ok(value)
    }
    /// Validates a file reference.
    pub fn file_id(id: &str) -> Result<Self, TaskContractErrorV2> {
        let value = Self::FileId(id.to_owned());
        value.validate()?;
        Ok(value)
    }
    /// Rechecks public enum construction at a component boundary.
    pub const fn validate(&self) -> Result<(), TaskContractErrorV2> {
        match self {
            Self::Urls(items) if items.is_empty() || items.len() > MAX_ARTIFACT_URLS => {
                Err(TaskContractErrorV2::InvalidArtifact)
            }
            Self::FileId(id) if id.is_empty() || id.len() > MAX_ARTIFACT_REF_BYTES => {
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
        }
    }
}

/// The upstream's execution facts, independent of host billing and delivery.
#[derive(Clone, Debug, PartialEq)]
pub enum TaskObservationV2 {
    /// Explicit queued/running distinction with the original status word.
    Progress { running: bool, status_word: String },
    /// Execution succeeded, carrying every available metering dimension.
    Succeeded { artifacts: TaskArtifactRefV2, usage: TaskUsageFactsV2 },
    /// Explicit upstream terminal failure; no timeout synthesized by a clock.
    Failed { kind: TaskFailureKindV1, code: Option<String>, message: Option<String> },
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
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TaskRequestEstimateV2 {
    requested_seconds: Option<f64>,
    milliunits_per_second: Option<i64>,
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
        Ok(Self { requested_seconds, milliunits_per_second })
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
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "estimation uses floating seconds; the rounded result is checked below the exclusive i64 bound before conversion"
    )]
    pub fn estimate_milliunits(
        &self,
        host_seconds: f64,
    ) -> Result<Option<i64>, TaskContractErrorV2> {
        if !host_seconds.is_finite() || host_seconds < 0.0 {
            return Err(TaskContractErrorV2::InvalidRequestEstimate);
        }
        let Some(rate) = self.milliunits_per_second else {
            return Ok(None);
        };
        let estimate = (rate as f64 * host_seconds).ceil();
        // i64::MAX rounds up to 2^63 in f64: equality must also be rejected.
        if !estimate.is_finite() || estimate >= 9_223_372_036_854_775_808.0 {
            return Err(TaskContractErrorV2::InvalidRequestEstimate);
        }
        Ok(Some(estimate as i64))
    }
}
