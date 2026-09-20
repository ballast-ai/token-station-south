//! Task workflow ordering over explicit host effects, without implicit I/O or business policy.
#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

use std::{future::Future, pin::Pin, time::Duration};

/// A scoped host operation. Dropping it must not detach work.
pub type HostFuture<'a, T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'a>>;
/// An explicitly supplied clock or cancellation operation.
pub type ControlFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// The atomic preparation decision.
pub enum PrepareResult<T, O> {
    /// A new durable task can attempt dispatch.
    Created(T),
    /// The same request already exists.
    Replayed(O),
    /// The idempotency identity belongs to another request.
    Conflict(O),
}
/// A provider submission fact, not a funds decision.
pub enum SubmissionFact<A, R> {
    /// Acceptance details belong to the host.
    Accepted(A),
    /// Definite rejection details belong to the host.
    Rejected(R),
    /// Acceptance cannot be established safely.
    Unknown,
}
/// The result of a host-owned composite atomic effect.
pub enum ApplyResult<O> {
    /// The transaction won and returns its authoritative outcome.
    Applied(O),
    /// The transaction lost; the caller must reload the winner.
    Conflict,
}
/// Why submission returned the authoritative result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// This operation committed its submission fact.
    Applied,
    /// An existing or concurrently committed task was returned.
    Replayed,
    /// Preparation rejected reuse of an idempotency identity.
    Conflict,
}
/// A submission result with its replay disposition.
pub struct SharedResult<O> {
    /// The host's authoritative outcome.
    pub outcome: O,
    /// The path that produced this outcome.
    pub disposition: Disposition,
}
/// Submission effects. Each persistence method owns its complete atomic boundary.
pub trait SubmissionEffects: Send {
    /// Host admission input.
    type Input: Sync + ?Sized;
    /// Persisted execution identity and dispatch authority.
    type Task: Send + Sync;
    /// Acceptance facts, including optional upstream identity.
    type Accepted: Send;
    /// Definite rejection facts.
    type Rejected: Send;
    /// Authoritative public or internal task projection.
    type Outcome: Send;
    /// Host error, never interpreted outside the send step.
    type Error: Send;
    /// Prepares or replays atomically; resources and events stay in that transaction.
    fn prepare<'a>(
        &'a mut self,
        input: &'a Self::Input,
    ) -> HostFuture<'a, PrepareResult<Self::Task, Self::Outcome>, Self::Error>;
    /// Persists the dispatch claim before any upstream send.
    fn begin_dispatch<'a>(&'a mut self, task: &'a Self::Task) -> HostFuture<'a, bool, Self::Error>;
    /// Sends at most once. Any error means acceptance is unknown.
    fn send<'a>(
        &'a mut self,
        task: &'a Self::Task,
    ) -> HostFuture<'a, SubmissionFact<Self::Accepted, Self::Rejected>, Self::Error>;
    /// Commits the fact and all policy effects atomically under the original claim.
    fn record_submission<'a>(
        &'a mut self,
        task: &'a Self::Task,
        fact: SubmissionFact<Self::Accepted, Self::Rejected>,
    ) -> HostFuture<'a, ApplyResult<Self::Outcome>, Self::Error>;
    /// Reads the authoritative persisted result after losing a compare-and-swap.
    fn reload<'a>(&'a mut self, task: &'a Self::Task)
    -> HostFuture<'a, Self::Outcome, Self::Error>;
}
/// Prepares and submits once, preserving uncertainty and atomic winners.
pub async fn submit<H: SubmissionEffects>(
    host: &mut H,
    input: &H::Input,
) -> Result<SharedResult<H::Outcome>, H::Error> {
    let task = match host.prepare(input).await? {
        PrepareResult::Created(task) => task,
        PrepareResult::Replayed(outcome) => {
            return Ok(SharedResult { outcome, disposition: Disposition::Replayed });
        }
        PrepareResult::Conflict(outcome) => {
            return Ok(SharedResult { outcome, disposition: Disposition::Conflict });
        }
    };
    if !host.begin_dispatch(&task).await? {
        return Ok(SharedResult {
            outcome: host.reload(&task).await?,
            disposition: Disposition::Replayed,
        });
    }
    let fact = host.send(&task).await.unwrap_or(SubmissionFact::Unknown);
    match host.record_submission(&task, fact).await? {
        ApplyResult::Applied(outcome) => {
            Ok(SharedResult { outcome, disposition: Disposition::Applied })
        }
        ApplyResult::Conflict => Ok(SharedResult {
            outcome: host.reload(&task).await?,
            disposition: Disposition::Replayed,
        }),
    }
}

/// Whether the host's persisted state requires an upstream observation.
pub enum ObservationPlan<T, O> {
    /// Query using this exact restored execution handle.
    Query(T),
    /// Return without querying, including when no claim was acquired.
    Return(O),
}
/// Observation effects. Policy mapping stays distinct from I/O and atomic application.
pub trait ObservationEffects: Send {
    /// Host lookup identity.
    type Key: Sync + ?Sized;
    /// Exact restored binding and claim.
    type Task: Send + Sync;
    /// Uninterpreted response facts.
    type Raw: Send;
    /// Host-normalized observation, including private policy tokens.
    type Observation: Send;
    /// Authoritative result projection.
    type Outcome: Send;
    /// Host error.
    type Error: Send;
    /// Restores the original binding and obtains any necessary claim.
    fn load<'a>(
        &'a mut self,
        key: &'a Self::Key,
    ) -> HostFuture<'a, ObservationPlan<Self::Task, Self::Outcome>, Self::Error>;
    /// Queries only the restored endpoint and execution identity.
    fn query<'a>(&'a mut self, task: &'a Self::Task) -> HostFuture<'a, Self::Raw, Self::Error>;
    /// Applies host policy to a response or query failure, without committing effects.
    fn normalize(
        &mut self,
        task: &Self::Task,
        raw: Result<Self::Raw, Self::Error>,
    ) -> Result<Self::Observation, Self::Error>;
    /// Atomically applies execution, resource and event effects under the claim.
    fn apply<'a>(
        &'a mut self,
        task: &'a Self::Task,
        observation: Self::Observation,
    ) -> HostFuture<'a, ApplyResult<Self::Outcome>, Self::Error>;
    /// Reloads the durable winner after a failed compare-and-swap.
    fn reload<'a>(&'a mut self, task: &'a Self::Task)
    -> HostFuture<'a, Self::Outcome, Self::Error>;
}
/// Performs one explicit observation; never starts a worker or retries implicitly.
pub async fn observe<H: ObservationEffects>(
    host: &mut H,
    key: &H::Key,
) -> Result<H::Outcome, H::Error> {
    let task = match host.load(key).await? {
        ObservationPlan::Query(task) => task,
        ObservationPlan::Return(outcome) => return Ok(outcome),
    };
    let raw = host.query(&task).await;
    let observation = host.normalize(&task, raw)?;
    apply_observation(host, &task, observation).await
}
/// Applies an already normalized observation, including synchronous submission results.
pub async fn apply_observation<H: ObservationEffects>(
    host: &mut H,
    task: &H::Task,
    observation: H::Observation,
) -> Result<H::Outcome, H::Error> {
    match host.apply(task, observation).await? {
        ApplyResult::Applied(outcome) => Ok(outcome),
        ApplyResult::Conflict => host.reload(task).await,
    }
}

/// The host's delivery-aware waiting projection.
pub enum WaitState<O> {
    /// No deliverable result yet, including execution success held for settlement.
    Pending,
    /// An outcome may be returned to this waiter.
    Ready(O),
}
/// One explicit waiting step: inspect persisted state or invoke shared observation once.
///
/// Only observed facts may trigger host policy effects. Timeout or cancellation itself must not
/// cancel the upstream task, manufacture failure, or release resources.
pub trait WaitEffects: Send {
    /// Lookup identity.
    type Key: Sync + ?Sized;
    /// Deliverable outcome.
    type Outcome: Send;
    /// Read error.
    type Error: Send;
    /// Inspects or advances once, then projects the result with host delivery authorization.
    ///
    /// Dropping this future must not detach work. The host must preserve recoverability across
    /// partially completed I/O; dropping a committed transaction does not roll it back.
    fn inspect<'a>(
        &'a mut self,
        key: &'a Self::Key,
    ) -> HostFuture<'a, WaitState<Self::Outcome>, Self::Error>;
}
/// Explicit monotonic time, bounded sleep and cancellation. No operation detaches work.
pub trait WaitControl: Sync {
    /// Monotonic elapsed time, using the same origin as the caller's deadline.
    fn now(&self) -> Duration;
    /// Sleeps for the supplied duration; dropping the future cancels this wait.
    fn sleep(&self, duration: Duration) -> ControlFuture<'_>;
    /// Resolves when this waiter should stop, without cancelling the upstream task.
    fn cancelled(&self) -> ControlFuture<'_>;
}
/// Why a waiter stopped.
#[derive(Debug, PartialEq, Eq)]
pub enum WaitResult<O> {
    /// The host permitted delivery.
    Ready(O),
    /// Only this waiter's time budget expired.
    TimedOut,
    /// Only this waiter was cancelled.
    Cancelled,
}
/// Invalid waiting parameters or a host read failure.
#[derive(Debug, PartialEq, Eq)]
pub enum WaitError<E> {
    /// A zero interval would permit an unbounded busy loop.
    InvalidInterval,
    /// The host could not inspect the task.
    Host(E),
}
impl<E> std::fmt::Display for WaitError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidInterval => "task wait interval must be positive",
            Self::Host(_) => "task wait host operation failed",
        })
    }
}
impl<E: std::fmt::Debug> std::error::Error for WaitError<E> {}
/// Waits within an explicit deadline, with cancellation before timeout before read readiness.
pub async fn wait<H: WaitEffects, C: WaitControl>(
    host: &mut H,
    key: &H::Key,
    control: &C,
    deadline: Duration,
    interval: Duration,
) -> Result<WaitResult<H::Outcome>, WaitError<H::Error>> {
    if interval.is_zero() {
        return Err(WaitError::InvalidInterval);
    }
    loop {
        match controlled(control, deadline, async { host.inspect(key).await }).await {
            Controlled::Ready(result) => match result.map_err(WaitError::Host)? {
                WaitState::Ready(outcome) => return Ok(WaitResult::Ready(outcome)),
                WaitState::Pending => {}
            },
            Controlled::TimedOut => return Ok(WaitResult::TimedOut),
            Controlled::Cancelled => return Ok(WaitResult::Cancelled),
        }
        let remaining = deadline.saturating_sub(control.now());
        match controlled(control, deadline, control.sleep(interval.min(remaining))).await {
            Controlled::Ready(()) => {}
            Controlled::TimedOut => return Ok(WaitResult::TimedOut),
            Controlled::Cancelled => return Ok(WaitResult::Cancelled),
        }
    }
}

/// The authorized host plan for a cancellation request.
pub enum CancelPlan<T, O> {
    /// Dispatch has not yet been proven to have happened.
    Prepared(T),
    /// Dispatch already happened, so only cancellation intent may be recorded.
    Dispatched(T),
    /// Return without mutation, including an already terminal task.
    Return(O),
}
/// Host-owned cancellation authority and atomic effects; no monetary values cross this trait.
pub trait CancelEffects: Send {
    /// Authorized lookup input.
    type Key: Sync + ?Sized;
    /// Persisted task identity and atomic guard.
    type Task: Send + Sync;
    /// Authoritative cancellation result.
    type Outcome: Send;
    /// Host error.
    type Error: Send;
    /// Authorizes the request and obtains a cancellation plan.
    fn load_for_cancel<'a>(
        &'a mut self,
        key: &'a Self::Key,
    ) -> HostFuture<'a, CancelPlan<Self::Task, Self::Outcome>, Self::Error>;
    /// Cancels only if atomic proof of non-dispatch still holds, including all policy effects.
    fn cancel_prepared<'a>(
        &'a mut self,
        task: &'a Self::Task,
    ) -> HostFuture<'a, ApplyResult<Self::Outcome>, Self::Error>;
    /// Records intent atomically, rechecking terminal state without changing terminal outcomes.
    fn record_cancel_intent<'a>(
        &'a mut self,
        task: &'a Self::Task,
    ) -> HostFuture<'a, Self::Outcome, Self::Error>;
}
/// Attempts an authorized cancellation without treating a dispatched task as free.
pub async fn cancel<H: CancelEffects>(host: &mut H, key: &H::Key) -> Result<H::Outcome, H::Error> {
    match host.load_for_cancel(key).await? {
        CancelPlan::Return(outcome) => Ok(outcome),
        CancelPlan::Dispatched(task) => host.record_cancel_intent(&task).await,
        CancelPlan::Prepared(task) => match host.cancel_prepared(&task).await? {
            ApplyResult::Applied(outcome) => Ok(outcome),
            ApplyResult::Conflict => host.record_cancel_intent(&task).await,
        },
    }
}

enum Controlled<T> {
    Ready(T),
    TimedOut,
    Cancelled,
}

async fn controlled<C: WaitControl, F: Future>(
    control: &C,
    deadline: Duration,
    operation: F,
) -> Controlled<F::Output> {
    let mut cancellation = control.cancelled();
    let mut timer = control.sleep(deadline.saturating_sub(control.now()));
    let mut operation = std::pin::pin!(operation);
    std::future::poll_fn(|cx| {
        if cancellation.as_mut().poll(cx).is_ready() {
            return std::task::Poll::Ready(Controlled::Cancelled);
        }
        if control.now() >= deadline || timer.as_mut().poll(cx).is_ready() {
            return std::task::Poll::Ready(Controlled::TimedOut);
        }
        operation.as_mut().poll(cx).map(Controlled::Ready)
    })
    .await
}
