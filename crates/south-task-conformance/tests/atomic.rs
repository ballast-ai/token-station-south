//! 错误实现必须被公共套件拒绝；内存参考实现不计为第二宿主。

use south_task_conformance::{
    AtomicHarness, Completion, Execution, Fault, HarnessFuture, Prepared, Probe, run_atomic_suite,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Defect {
    None,
    SplitPrepare,
    SplitApply,
    IgnoreFence,
    WrongTerminalFact,
    WrongResourceEffect,
}

struct Reference {
    probe: Probe,
    dispatched: bool,
    defect: Defect,
    funds: bool,
}

impl Reference {
    fn new(defect: Defect, funds: bool) -> Self {
        Self { probe: Probe::default(), dispatched: false, defect, funds }
    }
}

impl AtomicHarness for Reference {
    fn expected_resource_digest(&self, _: Completion) -> Option<[u8; 32]> {
        self.funds.then_some([2; 32])
    }
    fn reset(&mut self) -> HarnessFuture<'_, ()> {
        Box::pin(async move {
            self.probe = Probe {
                reserve_effects: self.funds.then_some(0),
                close_effects: self.funds.then_some(0),
                resource_digest: self.funds.then_some([0; 32]),
                ..Probe::default()
            };
            self.dispatched = false;
            Ok(())
        })
    }
    fn prepare(&mut self, different: bool, fault: Fault) -> HarnessFuture<'_, Prepared> {
        Box::pin(async move {
            if self.probe.tasks != 0 {
                return Ok(if different { Prepared::Conflict } else { Prepared::Replayed });
            }
            if fault == Fault::BeforePrepareCommit {
                if self.defect == Defect::SplitPrepare {
                    self.probe.tasks = 1;
                }
                return Err("injected prepare failure".into());
            }
            self.probe.tasks = 1;
            self.probe.execution = Execution::Queued;
            self.probe.version = 1;
            self.probe.reserve_effects = self.funds.then_some(1);
            self.probe.resource_digest = self.funds.then_some([1; 32]);
            if fault == Fault::AfterPrepareCommit {
                return Err("lost prepare reply".into());
            }
            Ok(Prepared::Created)
        })
    }
    fn dispatch(&mut self) -> HarnessFuture<'_, bool> {
        Box::pin(async move {
            if self.dispatched || self.probe.terminal {
                return Ok(false);
            }
            self.dispatched = true;
            self.probe.version += 1;
            Ok(true)
        })
    }
    fn guard(&mut self) -> HarnessFuture<'_, u64> {
        Box::pin(async { Ok(self.probe.version) })
    }
    fn apply(
        &mut self,
        guard: u64,
        completion: Completion,
        fault: Fault,
    ) -> HarnessFuture<'_, bool> {
        Box::pin(async move {
            if (guard != self.probe.version && self.defect != Defect::IgnoreFence)
                || (self.probe.terminal && !self.probe.held)
            {
                return Ok(false);
            }
            if fault == Fault::BeforeApplyCommit {
                if self.defect == Defect::SplitApply {
                    self.probe.terminal = true;
                }
                return Err("injected apply failure".into());
            }
            self.probe.terminal_events = 1;
            self.probe.terminal = true;
            self.probe.execution = match completion {
                Completion::Succeeded if self.defect == Defect::WrongTerminalFact => {
                    Execution::Failed
                }
                Completion::Succeeded => Execution::Succeeded,
                Completion::Failed => Execution::Failed,
                Completion::CancelledUnknown | Completion::CancelledNoCharge => {
                    Execution::Cancelled
                }
            };
            self.probe.held = self.funds && completion == Completion::CancelledUnknown;
            self.probe.version += 1;
            if !self.probe.held {
                self.probe.close_effects = self.funds.then_some(1);
                self.probe.resource_digest = self.funds.then_some([2; 32]);
                if self.defect == Defect::WrongResourceEffect {
                    self.probe.resource_digest = Some([9; 32]);
                }
            }
            if fault == Fault::AfterApplyCommit {
                return Err("lost apply reply".into());
            }
            Ok(true)
        })
    }
    fn takeover(&mut self) -> HarnessFuture<'_, ()> {
        Box::pin(async move {
            self.probe.version += 1;
            Ok(())
        })
    }
    fn cancel_prepared(&mut self) -> HarnessFuture<'_, bool> {
        Box::pin(async move {
            if self.dispatched || self.probe.terminal {
                return Ok(false);
            }
            self.probe.terminal = true;
            self.probe.execution = Execution::Cancelled;
            self.probe.terminal_events = 1;
            self.probe.version += 1;
            self.probe.close_effects = self.funds.then_some(1);
            self.probe.resource_digest = self.funds.then_some([2; 32]);
            Ok(true)
        })
    }
    fn snapshot(&mut self) -> HarnessFuture<'_, Probe> {
        Box::pin(async { Ok(self.probe.clone()) })
    }
}

#[tokio::test]
async fn correct_atomic_hosts_with_and_without_funds_pass_every_case() {
    for funds in [false, true] {
        let reports = run_atomic_suite(&mut Reference::new(Defect::None, funds)).await.unwrap();
        assert_eq!(reports.len(), 11);
    }
}

#[tokio::test]
async fn split_prepare_transaction_is_rejected() {
    let error =
        run_atomic_suite(&mut Reference::new(Defect::SplitPrepare, true)).await.unwrap_err();
    assert!(error.contains("prepare_rollback"), "{error}");
}

#[tokio::test]
async fn split_terminal_transaction_is_rejected() {
    let error = run_atomic_suite(&mut Reference::new(Defect::SplitApply, true)).await.unwrap_err();
    assert!(error.contains("apply_rollback"), "{error}");
}

#[tokio::test]
async fn ignoring_fencing_is_rejected() {
    let error = run_atomic_suite(&mut Reference::new(Defect::IgnoreFence, true)).await.unwrap_err();
    assert!(error.contains("stale_owner"), "{error}");
}

#[tokio::test]
async fn changing_success_into_failure_is_rejected() {
    let error =
        run_atomic_suite(&mut Reference::new(Defect::WrongTerminalFact, true)).await.unwrap_err();
    assert!(error.contains("execution fact"), "{error}");
}

#[tokio::test]
async fn wrong_resource_effect_is_rejected() {
    let error =
        run_atomic_suite(&mut Reference::new(Defect::WrongResourceEffect, true)).await.unwrap_err();
    assert!(error.contains("resource result"), "{error}");
}
