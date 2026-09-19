//! The task adapter vocabulary: the semantics of an asynchronous media task,
//! frozen ahead of the world that will speak them.
//!
//! Design record: `docs/design/2026-08-27-task-adapter-vocabulary.md` (ruled
//! 2026-08-27, issue #52). A chat call is one request and one response; a
//! media task is a lifecycle — submit, observe until terminal, render — with
//! a durable row, a lease, a reservation and a settlement between the steps.
//! This module is the vocabulary the two halves speak once that seam becomes
//! an ABI. It lives here and not in the Canonical IR because the IR's
//! authority is the community host, which does not run asynchronous media
//! tasks; the transport shapes stay IR and are reused verbatim.
//!
//! Ruling D1 belongs to the entry points rather than to a type, and is
//! recorded here so the slice that builds them starts from the ruling: the
//! task-side entry points take a **pre-resolved credential** — the
//! `PreparedSecretResolverV1` shape `south-core` already ships — and not a
//! `CredentialResolver`. Polling and artifact fetch must run under the
//! credential the task was *submitted* with; which credential that was is
//! host state on the durable row, so South cannot enforce the rule, only
//! decide where the choice happens. A pre-resolved secret makes it a
//! statement the host writes at a named place, once. The rule is a host
//! obligation, judged by a host suite (the gate ③ family), never by the
//! component suite.

use std::fmt;

use thiserror::Error;

/// The version of the task adapter vocabulary contract.
pub const TASK_CONTRACT_VERSION: u16 = 2;

/// The maximum byte length of a host-minted task identifier.
pub const MAX_TASK_ID_BYTES: usize = 128;

/// The maximum byte length of a host-minted callback URL, matching the
/// trusted endpoint bound.
pub const MAX_CALLBACK_URL_BYTES: usize = 8 * 1024;

/// A rejected [`HostMintedValuesV1`] construction.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum TaskContractErrorV1 {
    /// The host-minted task identifier is empty.
    #[error("a host-minted task id must not be empty")]
    EmptyTaskId,
    /// The host-minted task identifier exceeds [`MAX_TASK_ID_BYTES`].
    #[error("host-minted task id exceeds the boundary limit")]
    TaskIdTooLarge,
    /// The host-minted task identifier carries a byte outside printable
    /// ASCII. It travels inside provider dialect bodies and provider logs;
    /// the character class, not the length, does the safety work.
    #[error("host-minted task id must be printable ASCII")]
    TaskIdNotPrintableAscii,
    /// An artifact reference is empty — an empty URL list, an empty URL, or
    /// an empty file id. "Succeeded with nothing to point at" is a shape
    /// error, not a success the host should try to render.
    #[error("an artifact reference must not be empty")]
    EmptyArtifactRef,
    /// An artifact reference exceeds [`MAX_ARTIFACT_URLS`] entries or
    /// [`MAX_ARTIFACT_REF_BYTES`] per entry.
    #[error("artifact reference exceeds the boundary limit")]
    ArtifactRefTooLarge,
    /// The callback URL is empty. A dialect with no callback concept omits
    /// the `Option`; an empty string is a declaration that says nothing.
    #[error("a host-minted callback URL must not be empty")]
    EmptyCallbackUrl,
    /// The callback URL exceeds [`MAX_CALLBACK_URL_BYTES`].
    #[error("host-minted callback URL exceeds the boundary limit")]
    CallbackUrlTooLarge,
    /// The callback URL carries an ASCII control byte or a space, which no
    /// URL may and which would let a minted value break out of the header or
    /// body position a dialect places it in.
    #[error("host-minted callback URL carries a control byte or space")]
    CallbackUrlNotUrlSafe,
}

/// The values only the host can produce and the wire needs, handed to
/// `build-submit-request` beside the request (2026-08-27 task-adapter
/// record, ruling D2).
///
/// The component knows *where* they go in its dialect's body; the host knows
/// *what* they are. Three rules are the contract:
///
/// 1. **The component places them; it never invents them.** A component that
///    generates its own task id destroys the host's idempotency anchor — the
///    upstream would accept a resubmission as new work, and the reservation
///    the host is holding would pay for both.
/// 2. **They are opaque strings.** The component must not parse the callback
///    URL, derive anything from the id, or reorder their bytes. A dialect
///    that has no callback concept ignores the `Option`; South does not
///    require it to be used.
/// 3. **The nonce plaintext exists only inside the callback URL.** It is
///    never stored and never logged — only its hash is persisted, and it is
///    written before the upstream call because acceptance can trigger an
///    immediate callback. This type's `Debug` therefore prints byte counts
///    and not values, the same discipline [`CredentialSlotV1`] and
///    [`JsonBodyV1`] follow; a component that echoes the URL into an error
///    message leaks it, which is why the error channel carries an
///    `ErrorEnvelope` and not free text.
///
/// [`CredentialSlotV1`]: crate::CredentialSlotV1
/// [`JsonBodyV1`]: crate::JsonBodyV1
#[derive(Clone, PartialEq, Eq)]
pub struct HostMintedValuesV1 {
    task_id: String,
    callback_url: Option<String>,
}

impl HostMintedValuesV1 {
    /// Validates the host-minted values without interpreting them.
    ///
    /// Both values stay byte-exact: no normalization, no URL parsing —
    /// rule 2 binds South the same way it binds the component.
    ///
    /// # Errors
    ///
    /// Returns the [`TaskContractErrorV1`] naming the value refused.
    pub fn new(task_id: &str, callback_url: Option<&str>) -> Result<Self, TaskContractErrorV1> {
        if task_id.is_empty() {
            return Err(TaskContractErrorV1::EmptyTaskId);
        }
        if task_id.len() > MAX_TASK_ID_BYTES {
            return Err(TaskContractErrorV1::TaskIdTooLarge);
        }
        if !task_id.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(TaskContractErrorV1::TaskIdNotPrintableAscii);
        }
        if let Some(url) = callback_url {
            if url.is_empty() {
                return Err(TaskContractErrorV1::EmptyCallbackUrl);
            }
            if url.len() > MAX_CALLBACK_URL_BYTES {
                return Err(TaskContractErrorV1::CallbackUrlTooLarge);
            }
            if !url.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err(TaskContractErrorV1::CallbackUrlNotUrlSafe);
            }
        }
        Ok(Self { task_id: task_id.to_owned(), callback_url: callback_url.map(str::to_owned) })
    }

    /// Returns the host-minted task identifier, byte-exact.
    #[must_use]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    /// Returns the host-minted callback URL, byte-exact, when the host
    /// declared one.
    #[must_use]
    pub fn callback_url(&self) -> Option<&str> {
        self.callback_url.as_deref()
    }
}

impl fmt::Debug for HostMintedValuesV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostMintedValuesV1")
            .field("task_id_byte_count", &self.task_id.len())
            .field("callback_url_byte_count", &self.callback_url.as_deref().map(str::len))
            .finish_non_exhaustive()
    }
}

/// The maximum number of artifact URLs one terminal observation may carry.
///
/// One family returns a small set (a video plus its cover); none returns a
/// stream of them. A bound exists because this crosses a sandbox boundary,
/// and an unbounded list is an unbounded allocation in the host.
pub const MAX_ARTIFACT_URLS: usize = 16;

/// The maximum byte length of one artifact URL or file id.
pub const MAX_ARTIFACT_REF_BYTES: usize = 8 * 1024;

/// Where a finished task's result is, as the dialect expresses it.
///
/// Two shapes because upstreams genuinely differ, and the difference is what
/// `build-artifact-request` exists to bridge: most families hand back a URL
/// the host can serve or transfer, while one hands back an id that must be
/// exchanged for a download URL in a second call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskArtifactRefV1 {
    /// Direct references the host can fetch or transfer.
    Urls(Vec<String>),
    /// An upstream-scoped id. The host obtains the real location by running
    /// the descriptor `build-artifact-request` returns for it.
    FileId(String),
    /// The upstream reported success with no artifact reference of its own —
    /// the result travels in the terminal body the renderer already has.
    None,
}

impl TaskArtifactRefV1 {
    /// Builds a URL reference.
    ///
    /// # Errors
    ///
    /// Returns [`TaskContractErrorV1`] when the list is empty, longer than
    /// [`MAX_ARTIFACT_URLS`], or carries an entry that is empty or exceeds
    /// [`MAX_ARTIFACT_REF_BYTES`].
    pub fn urls(urls: Vec<String>) -> Result<Self, TaskContractErrorV1> {
        if urls.is_empty() {
            return Err(TaskContractErrorV1::EmptyArtifactRef);
        }
        if urls.len() > MAX_ARTIFACT_URLS {
            return Err(TaskContractErrorV1::ArtifactRefTooLarge);
        }
        for url in &urls {
            if url.is_empty() {
                return Err(TaskContractErrorV1::EmptyArtifactRef);
            }
            if url.len() > MAX_ARTIFACT_REF_BYTES {
                return Err(TaskContractErrorV1::ArtifactRefTooLarge);
            }
        }
        Ok(Self::Urls(urls))
    }

    /// Builds a file-id reference.
    ///
    /// # Errors
    ///
    /// Returns [`TaskContractErrorV1`] when the id is empty or exceeds
    /// [`MAX_ARTIFACT_REF_BYTES`].
    pub fn file_id(id: &str) -> Result<Self, TaskContractErrorV1> {
        if id.is_empty() {
            return Err(TaskContractErrorV1::EmptyArtifactRef);
        }
        if id.len() > MAX_ARTIFACT_REF_BYTES {
            return Err(TaskContractErrorV1::ArtifactRefTooLarge);
        }
        Ok(Self::FileId(id.to_owned()))
    }
}

/// What the upstream reported this task consumed, in the unit it reported.
///
/// **A closed set, deliberately not a map.** South may carry what an upstream
/// *reported*; what a unit costs is the host's. A free map is how the first
/// becomes the second one key at a time — the first `"price_per_second"` a
/// component writes would be a pricing vocabulary living in South.
///
/// Absence is not zero: most families bill from the request, so most terminal
/// observations carry no meter at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TaskMeterV1 {
    /// Seconds of produced media, as the upstream reported them.
    Seconds(f64),
    /// Tokens the upstream billed for this task.
    Tokens(i64),
    /// Provider-defined milliunits — a thousandth of the provider's own
    /// billing unit, for upstreams that report an exact deduction rather
    /// than a physical quantity.
    Milliunits(i64),
}

/// Why a terminal observation is a failure (2026-08-27 task-adapter record,
/// ruling D3).
///
/// There is deliberately no `Expired` **observation** variant: expiry is a
/// failure kind, and the client-facing `Expired` status word is a separate
/// host mapping — keeping them apart is what stops a public vocabulary
/// change from becoming a contract change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskFailureKindV1 {
    /// The upstream said the task failed.
    Failed,
    /// The upstream said the task was cancelled.
    Cancelled,
    /// The upstream **explicitly said** the task expired. Nothing may be
    /// inferred into this kind: measured across six provider families, only
    /// `BytePlus` and `xAI` say it on the wire, and everything else that
    /// looks like expiry is a plain failure or `unknown`.
    ProviderExpired,
}

impl TaskFailureKindV1 {
    /// Every failure kind, in declaration order.
    pub const ALL: [Self; 3] = [Self::Failed, Self::Cancelled, Self::ProviderExpired];

    /// Returns the frozen vocabulary word for this kind.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::ProviderExpired => "provider-expired",
        }
    }
}

/// One observation of a task's state, as the component read it off the wire
/// (2026-08-27 task-adapter record, ruling D3).
///
/// Reading "the upstream said expired" off the wire is dialect knowledge, so
/// it is the component's; deciding whether to try another upstream is funds
/// and routing policy, so it is the host's. The rules that keep each half on
/// its side:
///
/// 1. An unrecognised status word is [`Unknown`], never a synthesised
///    failure — inventing a terminal renders "we cannot tell" as "it is
///    over".
/// 2. A non-2xx query is [`Unknown`]. A failed *query* is not a failed
///    *task*: 429, 5xx, 404 and 401 all mean the observation did not happen.
///    That holds even for an upstream that expresses task expiry only as a
///    404 — a 404 is also what a wrong credential or a transient gateway
///    returns, so reconciliation policy, not the observation, settles such a
///    task. Every family's fixture pack carries a 404-query row pinning
///    this.
/// 3. Neither side may synthesise a terminal from a timeout. A component has
///    no clock, so it can only report expiry when the wire says so; a host
///    that maps its own deadline to a terminal failure settles a task that
///    may still be running. A local deadline yields [`Unknown`], which is a
///    reconciliation input, not an outcome.
/// 4. The host reads retry policy off the failure kind, and the component
///    never expresses it — there is no "retriable" flag here.
///
/// These rules are judgeable in the component suite: per-dialect fixtures
/// feed a terminal body and assert the kind, plus adversarial rows for the
/// words that must fall through to [`Unknown`].
///
/// [`Unknown`]: TaskObservationV1::Unknown
#[derive(Clone, Debug, PartialEq)]
pub enum TaskObservationV1 {
    /// The upstream reports the task is still in progress, in its own word.
    ///
    /// The word is kept verbatim for the host's snapshot and diagnosis. It is
    /// **not** a vocabulary: the host branches on the variant, never on this
    /// string, so a dialect inventing a new progress word changes nothing.
    Running {
        /// The upstream's own status word, as it appeared on the wire.
        status_word: String,
    },
    /// The upstream reports the task finished and its artifact is ready.
    ///
    /// Carries what the upstream *reported* — the artifact reference, and the
    /// meter when the upstream stated one (2026-09-19 vocabulary fit survey).
    /// Without these the host would have to parse the dialect's terminal body
    /// itself, which is the knowledge the component exists to hold.
    Succeeded {
        /// Where the result is, as this dialect expresses it.
        artifact: TaskArtifactRefV1,
        /// What the upstream said this cost, in the unit it said it in.
        /// `None` when the upstream reports no meter, which is the common
        /// case: most families bill from the request, not the result.
        meter: Option<TaskMeterV1>,
    },
    /// The upstream stated a terminal failure of the carried kind, with its
    /// own code and message where it gave them.
    ///
    /// The host surfaces those words and `map-terminal-failure` maps them onto
    /// the closed `ErrorCode` catalog; neither is possible if the observation
    /// drops them. The **kind** remains the only thing the host branches on.
    Failed {
        kind: TaskFailureKindV1,
        /// The upstream's error code, verbatim, when it gave one.
        code: Option<String>,
        /// The upstream's message, verbatim, when it gave one.
        message: Option<String>,
    },
    /// The observation did not establish the task's state. Not a terminal:
    /// a reconciliation input.
    Unknown {
        /// Why the state could not be established — an HTTP status, an
        /// unrecognised word, an unexpected shape. For diagnosis only.
        reason: String,
    },
}

impl TaskObservationV1 {
    /// Returns whether this observation settles the task.
    ///
    /// [`Unknown`] is never terminal — treating "we cannot tell" as settled
    /// is the exact failure rule 3 exists to prevent.
    ///
    /// [`Unknown`]: TaskObservationV1::Unknown
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Succeeded { .. } | Self::Failed { .. })
    }

    /// Returns the frozen vocabulary word for this observation's state.
    #[must_use]
    pub const fn state_word(&self) -> &'static str {
        match self {
            Self::Running { .. } => "running",
            Self::Succeeded { .. } => "succeeded",
            Self::Failed { .. } => "failed",
            Self::Unknown { .. } => "unknown",
        }
    }
}
