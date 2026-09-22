use south_task_core::*;
use std::{
    future::pending,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

type Error = &'static str;
#[derive(Default)]
struct Host {
    events: Vec<&'static str>,
    _not_sync: std::marker::PhantomData<std::cell::Cell<()>>,
    mode: &'static str,
    failure: Option<&'static str>,
    inspections: usize,
}
impl Host {
    fn step(&mut self, step: &'static str) -> Result<(), Error> {
        self.events.push(step);
        if self.failure == Some(step) { Err("host failure") } else { Ok(()) }
    }
}
impl SubmissionEffects for Host {
    type Input = ();
    type Task = u8;
    type Accepted = u8;
    type Rejected = u8;
    type Outcome = u8;
    type Error = Error;
    fn prepare<'a>(&'a mut self, (): &'a ()) -> HostFuture<'a, PrepareResult<u8, u8>, Error> {
        Box::pin(async move {
            self.step("prepare")?;
            Ok(match self.mode {
                "replay" => PrepareResult::Replayed(7),
                "conflict" => PrepareResult::Conflict(8),
                _ => PrepareResult::Created(1),
            })
        })
    }
    fn begin_dispatch<'a>(&'a mut self, _: &'a u8) -> HostFuture<'a, bool, Error> {
        Box::pin(async move {
            self.step("dispatch")?;
            Ok(self.mode != "lost_dispatch")
        })
    }
    fn send<'a>(&'a mut self, _: &'a u8) -> HostFuture<'a, SubmissionFact<u8, u8>, Error> {
        Box::pin(async move {
            self.step("send")?;
            Ok(match self.mode {
                "rejected" => SubmissionFact::Rejected(2),
                "unknown" => SubmissionFact::Unknown,
                _ => SubmissionFact::Accepted(3),
            })
        })
    }
    fn record_submission<'a>(
        &'a mut self,
        _: &'a u8,
        fact: SubmissionFact<u8, u8>,
    ) -> HostFuture<'a, ApplyResult<u8>, Error> {
        Box::pin(async move {
            let step = match fact {
                SubmissionFact::Accepted(_) => "accepted",
                SubmissionFact::Rejected(_) => "rejected",
                SubmissionFact::Unknown => "unknown",
            };
            self.step(step)?;
            Ok(if self.mode == "lost_record" {
                ApplyResult::Conflict
            } else {
                ApplyResult::Applied(4)
            })
        })
    }
    fn reload<'a>(&'a mut self, _: &'a u8) -> HostFuture<'a, u8, Error> {
        Box::pin(async move {
            self.step("reload")?;
            Ok(9)
        })
    }
}
impl ObservationEffects for Host {
    type Key = ();
    type Task = u8;
    type Raw = u8;
    type Observation = u8;
    type Outcome = u8;
    type Error = Error;
    fn load<'a>(&'a mut self, (): &'a ()) -> HostFuture<'a, ObservationPlan<u8, u8>, Error> {
        Box::pin(async move {
            self.step("load")?;
            Ok(if self.mode == "return" {
                ObservationPlan::Return(8)
            } else {
                ObservationPlan::Query(1)
            })
        })
    }
    fn query<'a>(&'a mut self, _: &'a u8) -> HostFuture<'a, u8, Error> {
        Box::pin(async move {
            self.step("query")?;
            Ok(2)
        })
    }
    fn normalize(&mut self, _: &u8, raw: Result<u8, Error>) -> Result<u8, Error> {
        self.step(if raw.is_ok() { "normalize" } else { "normalize_error" })?;
        Ok(3)
    }
    fn apply<'a>(
        &'a mut self,
        _: &'a u8,
        observation: u8,
    ) -> HostFuture<'a, ApplyResult<u8>, Error> {
        Box::pin(async move {
            assert_eq!(observation, 3);
            self.step("apply")?;
            Ok(if self.mode == "lost_apply" {
                ApplyResult::Conflict
            } else {
                ApplyResult::Applied(4)
            })
        })
    }
    fn reload<'a>(&'a mut self, task: &'a u8) -> HostFuture<'a, u8, Error> {
        SubmissionEffects::reload(self, task)
    }
}
impl WaitEffects for Host {
    type Key = ();
    type Outcome = u8;
    type Error = Error;
    fn inspect<'a>(&'a mut self, (): &'a ()) -> HostFuture<'a, WaitState<u8>, Error> {
        Box::pin(async move {
            self.step("inspect")?;
            self.inspections += 1;
            if self.mode == "slow" {
                pending::<()>().await;
            }
            Ok(if self.mode == "held" || (self.mode == "later" && self.inspections < 3) {
                WaitState::Pending
            } else {
                WaitState::Ready(4)
            })
        })
    }
}
impl CancelEffects for Host {
    type Key = ();
    type Task = u8;
    type Outcome = u8;
    type Error = Error;
    fn load_for_cancel<'a>(&'a mut self, (): &'a ()) -> HostFuture<'a, CancelPlan<u8, u8>, Error> {
        Box::pin(async move {
            self.step("cancel_load")?;
            Ok(match self.mode {
                "terminal" => CancelPlan::Return(8),
                "dispatched" => CancelPlan::Dispatched(1),
                _ => CancelPlan::Prepared(1),
            })
        })
    }
    fn cancel_prepared<'a>(&'a mut self, _: &'a u8) -> HostFuture<'a, ApplyResult<u8>, Error> {
        Box::pin(async move {
            self.step("cancel_prepared")?;
            Ok(if self.mode == "lost_cancel" {
                ApplyResult::Conflict
            } else {
                ApplyResult::Applied(4)
            })
        })
    }
    fn record_cancel_intent<'a>(&'a mut self, _: &'a u8) -> HostFuture<'a, u8, Error> {
        Box::pin(async move {
            self.step("intent")?;
            Ok(9)
        })
    }
}
struct Clock {
    start: tokio::time::Instant,
    cancelled: AtomicBool,
}
impl Clock {
    fn new() -> Self {
        Self { start: tokio::time::Instant::now(), cancelled: AtomicBool::new(false) }
    }
}
impl WaitControl for Clock {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }
    fn sleep(&self, duration: Duration) -> ControlFuture<'_> {
        Box::pin(tokio::time::sleep(duration))
    }
    fn cancelled(&self) -> ControlFuture<'_> {
        Box::pin(async {
            if !self.cancelled.load(Ordering::SeqCst) {
                pending::<()>().await;
            }
        })
    }
}
#[tokio::test]
async fn submit_commits_each_fact_once() {
    for (mode, last) in [("accepted", "accepted"), ("rejected", "rejected"), ("unknown", "unknown")]
    {
        let mut h = Host { mode, ..Host::default() };
        let r = submit(&mut h, &()).await.unwrap();
        assert_eq!(r.outcome, 4);
        assert_eq!(r.disposition, Disposition::Applied);
        assert_eq!(h.events, ["prepare", "dispatch", "send", last]);
    }
}
#[tokio::test]
async fn replay_and_key_conflict_never_claim_or_send() {
    for (mode, value, disposition) in
        [("replay", 7, Disposition::Replayed), ("conflict", 8, Disposition::Conflict)]
    {
        let mut h = Host { mode, ..Host::default() };
        let r = submit(&mut h, &()).await.unwrap();
        assert_eq!((r.outcome, r.disposition), (value, disposition));
        assert_eq!(h.events, ["prepare"]);
    }
}
#[tokio::test]
async fn dispatch_loser_only_reloads() {
    let mut h = Host { mode: "lost_dispatch", ..Host::default() };
    let r = submit(&mut h, &()).await.unwrap();
    assert_eq!((r.outcome, r.disposition), (9, Disposition::Replayed));
    assert_eq!(h.events, ["prepare", "dispatch", "reload"]);
}
#[tokio::test]
async fn record_loser_returns_winner_not_fresh_acceptance() {
    let mut h = Host { mode: "lost_record", ..Host::default() };
    let r = submit(&mut h, &()).await.unwrap();
    assert_eq!((r.outcome, r.disposition), (9, Disposition::Replayed));
    assert_eq!(h.events, ["prepare", "dispatch", "send", "accepted", "reload"]);
}
#[tokio::test]
async fn send_failure_is_recorded_unknown_without_resend() {
    let mut h = Host { failure: Some("send"), ..Host::default() };
    assert_eq!(submit(&mut h, &()).await.unwrap().outcome, 4);
    assert_eq!(h.events, ["prepare", "dispatch", "send", "unknown"]);
}
#[tokio::test]
async fn persistence_failures_propagate_without_uncertainty_rewrite() {
    for failure in ["prepare", "dispatch", "accepted", "reload"] {
        let mut h = Host {
            failure: Some(failure),
            mode: if failure == "reload" { "lost_record" } else { "" },
            ..Host::default()
        };
        assert!(matches!(submit(&mut h, &()).await, Err("host failure")));
        assert!(!h.events.contains(&"unknown"));
        assert_eq!(h.events.last(), Some(&failure));
    }
}
#[tokio::test]
async fn observe_runs_separate_query_policy_and_atomic_apply() {
    let mut h = Host::default();
    assert_eq!(observe(&mut h, &()).await.unwrap(), 4);
    assert_eq!(h.events, ["load", "query", "normalize", "apply"]);
}
#[tokio::test]
async fn observation_loser_returns_persisted_winner() {
    let mut h = Host { mode: "lost_apply", ..Host::default() };
    assert_eq!(observe(&mut h, &()).await.unwrap(), 9);
    assert_eq!(h.events, ["load", "query", "normalize", "apply", "reload"]);
}
#[tokio::test]
async fn direct_observation_uses_same_authoritative_reload() {
    let mut h = Host { mode: "lost_apply", ..Host::default() };
    assert_eq!(apply_observation(&mut h, &1, 3).await.unwrap(), 9);
    assert_eq!(h.events, ["apply", "reload"]);
}
#[tokio::test]
async fn observation_return_skips_query() {
    let mut h = Host { mode: "return", ..Host::default() };
    assert_eq!(observe(&mut h, &()).await.unwrap(), 8);
    assert_eq!(h.events, ["load"]);
}
#[tokio::test]
async fn query_failure_is_mapped_by_host_policy() {
    let mut h = Host { failure: Some("query"), ..Host::default() };
    assert_eq!(observe(&mut h, &()).await.unwrap(), 4);
    assert_eq!(h.events, ["load", "query", "normalize_error", "apply"]);
}
#[tokio::test]
async fn observation_policy_or_atomic_failures_stop_processing() {
    for failure in ["load", "normalize", "apply", "reload"] {
        let mut h = Host { failure: Some(failure), mode: "lost_apply", ..Host::default() };
        assert_eq!(observe(&mut h, &()).await, Err("host failure"));
        assert_eq!(h.events.last(), Some(&failure));
    }
}
#[tokio::test(start_paused = true)]
async fn wait_loops_until_host_permits_delivery() {
    let mut h = Host { mode: "later", ..Host::default() };
    let c = Clock::new();
    assert_eq!(
        wait(&mut h, &(), &c, Duration::from_secs(10), Duration::from_secs(2)).await.unwrap(),
        WaitResult::Ready(4)
    );
    assert_eq!(h.inspections, 3);
    assert_eq!(c.now(), Duration::from_secs(4));
}
#[tokio::test(start_paused = true)]
async fn held_success_times_out_without_task_cancellation() {
    let mut h = Host { mode: "held", ..Host::default() };
    let c = Clock::new();
    assert_eq!(
        wait(&mut h, &(), &c, Duration::from_secs(3), Duration::from_secs(2)).await.unwrap(),
        WaitResult::TimedOut
    );
    assert_eq!(c.now(), Duration::from_secs(3));
    assert!(h.events.iter().all(|e| *e == "inspect"));
}
#[tokio::test(start_paused = true)]
async fn deadline_also_bounds_a_never_finishing_inspection() {
    let mut h = Host { mode: "slow", ..Host::default() };
    let c = Clock::new();
    assert_eq!(
        wait(&mut h, &(), &c, Duration::from_secs(3), Duration::from_secs(1)).await.unwrap(),
        WaitResult::TimedOut
    );
    assert_eq!(c.now(), Duration::from_secs(3));
}
#[tokio::test(start_paused = true)]
async fn cancellation_precedes_expired_deadline_and_ready_result() {
    let mut h = Host::default();
    let c = Clock::new();
    c.cancelled.store(true, Ordering::SeqCst);
    assert_eq!(
        wait(&mut h, &(), &c, Duration::ZERO, Duration::from_secs(1)).await.unwrap(),
        WaitResult::Cancelled
    );
    assert!(h.events.is_empty());
}
#[tokio::test(start_paused = true)]
async fn expired_deadline_precedes_ready_result() {
    let mut h = Host::default();
    let c = Clock::new();
    assert_eq!(
        wait(&mut h, &(), &c, Duration::ZERO, Duration::from_secs(1)).await.unwrap(),
        WaitResult::TimedOut
    );
    assert!(h.events.is_empty());
}
#[tokio::test]
async fn zero_interval_is_rejected_without_reading() {
    let mut h = Host::default();
    assert_eq!(
        wait(&mut h, &(), &Clock::new(), Duration::from_secs(5), Duration::ZERO).await,
        Err(WaitError::InvalidInterval)
    );
    assert!(h.events.is_empty());
}
#[tokio::test]
async fn waiter_read_error_is_not_converted_into_timeout() {
    let mut h = Host { failure: Some("inspect"), ..Host::default() };
    assert_eq!(
        wait(&mut h, &(), &Clock::new(), Duration::from_secs(5), Duration::from_secs(1)).await,
        Err(WaitError::Host("host failure"))
    );
}
#[tokio::test]
async fn prepared_cancel_attempt_and_dispatched_intent_are_distinct() {
    for (mode, expected, events) in [
        ("", 4, vec!["cancel_load", "cancel_prepared"]),
        ("dispatched", 9, vec!["cancel_load", "intent"]),
        ("lost_cancel", 9, vec!["cancel_load", "cancel_prepared", "intent"]),
        ("terminal", 8, vec!["cancel_load"]),
    ] {
        let mut h = Host { mode, ..Host::default() };
        assert_eq!(cancel(&mut h, &()).await.unwrap(), expected);
        assert_eq!(h.events, events);
    }
}
#[tokio::test]
async fn cancel_failure_does_not_fall_through_into_another_mutation() {
    for failure in ["cancel_load", "cancel_prepared", "intent"] {
        let mut h = Host {
            failure: Some(failure),
            mode: if failure == "intent" { "dispatched" } else { "" },
            ..Host::default()
        };
        assert_eq!(cancel(&mut h, &()).await, Err("host failure"));
        assert_eq!(h.events.last(), Some(&failure));
    }
}

struct CancellingClock(Clock);
impl WaitControl for CancellingClock {
    fn now(&self) -> Duration {
        self.0.now()
    }
    fn sleep(&self, duration: Duration) -> ControlFuture<'_> {
        self.0.sleep(duration)
    }
    fn cancelled(&self) -> ControlFuture<'_> {
        Box::pin(tokio::time::sleep(Duration::from_secs(1).saturating_sub(self.now())))
    }
}

#[tokio::test(start_paused = true)]
async fn cancellation_interrupts_inflight_inspection_and_idle_wait() {
    for mode in ["slow", "held"] {
        let mut host = Host { mode, ..Host::default() };
        let control = CancellingClock(Clock::new());
        assert_eq!(
            wait(&mut host, &(), &control, Duration::from_secs(9), Duration::from_secs(5))
                .await
                .unwrap(),
            WaitResult::Cancelled
        );
        assert_eq!(control.now(), Duration::from_secs(1));
        assert_eq!(host.events, ["inspect"]);
    }
}

#[test]
fn workflow_futures_are_send_without_requiring_sync_effects() {
    fn assert_send<T: Send>(_: T) {}
    let mut host = Host::default();
    assert_send(submit(&mut host, &()));
    assert_send(observe(&mut host, &()));
    assert_send(apply_observation(&mut host, &1, 3));
    assert_send(cancel(&mut host, &()));
}

#[test]
fn rust_library_version_is_independent_of_component_runtime_identity() {
    assert_eq!(env!("CARGO_PKG_VERSION"), "0.1.0");
    assert!(include_str!("../../../Cargo.toml").contains("version = \"0.32.0\""));
    assert!(
        include_str!("../../south-provider-runtime/Cargo.toml")
            .contains("version.workspace = true")
    );
    assert!(
        include_str!("../../../components/task-bailian-v2/manifest.json")
            .contains("\"south_runtime\": \"0.32.0\"")
    );
}

struct EagerInspection {
    calls: usize,
}
impl WaitEffects for EagerInspection {
    type Key = ();
    type Outcome = u8;
    type Error = Error;
    fn inspect<'a>(&'a mut self, (): &'a ()) -> HostFuture<'a, WaitState<u8>, Error> {
        self.calls += 1;
        Box::pin(async { Ok(WaitState::Ready(4)) })
    }
}

#[tokio::test]
async fn stopped_waiter_does_not_even_invoke_the_host_inspection_method() {
    for cancelled in [true, false] {
        let mut host = EagerInspection { calls: 0 };
        let control = Clock::new();
        control.cancelled.store(cancelled, Ordering::SeqCst);
        let expected = if cancelled { WaitResult::Cancelled } else { WaitResult::TimedOut };
        assert_eq!(
            wait(&mut host, &(), &control, Duration::ZERO, Duration::from_secs(1)).await.unwrap(),
            expected
        );
        assert_eq!(host.calls, 0);
    }
}
