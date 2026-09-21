//! 任务复合原子效果的公共履约套件。宿主适配器必须操作真实效果实现。

use std::{future::Future, pin::Pin};

/// 测试适配器返回去敏错误，不返回凭证或数据库连接信息。
pub type HarnessFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// 故障注入位置；提交前必须由真实事务内部失败，不能在调用前短路。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    None,
    BeforePrepareCommit,
    AfterPrepareCommit,
    BeforeApplyCommit,
    AfterApplyCommit,
}

/// 准备事务的幂等裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prepared {
    Created,
    Replayed,
    Conflict,
}

/// 套件使用的客观结果与费用证据；宿主自己确定资源政策。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completion {
    Succeeded,
    Failed,
    CancelledUnknown,
    CancelledNoCharge,
}

/// 持久执行事实，不包含资源政策。成功、失败和取消不能互相替代。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Execution {
    #[default]
    Absent,
    Queued,
    Running,
    Unknown,
    Succeeded,
    Failed,
    Cancelled,
}

/// 不导出金额或账本格式。摘要仅用于证明失败没有改变宿主资源。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Probe {
    pub tasks: u32,
    pub execution: Execution,
    pub terminal: bool,
    pub held: bool,
    pub reserve_effects: Option<u32>,
    pub close_effects: Option<u32>,
    pub terminal_events: u32,
    pub version: u64,
    pub resource_digest: Option<[u8; 32]>,
}

/// 每个场景独立重建 fixture；必须使用生产的原子效果和真实存储。
/// `takeover` 表示新 owner 已持久接管，不把时间流逝冒充 fencing。
pub trait AtomicHarness: Send {
    /// 按本夹具的独立政策计算预期资源，禁止从本次实际关闭结果回读后充当预期。
    fn expected_resource_digest(&self, completion: Completion) -> Option<[u8; 32]>;
    fn reset(&mut self) -> HarnessFuture<'_, ()>;
    fn prepare(&mut self, different_request: bool, fault: Fault) -> HarnessFuture<'_, Prepared>;
    fn dispatch(&mut self) -> HarnessFuture<'_, bool>;
    fn guard(&mut self) -> HarnessFuture<'_, u64>;
    fn apply(
        &mut self,
        guard: u64,
        completion: Completion,
        fault: Fault,
    ) -> HarnessFuture<'_, bool>;
    fn takeover(&mut self) -> HarnessFuture<'_, ()>;
    fn cancel_prepared(&mut self) -> HarnessFuture<'_, bool>;
    fn snapshot(&mut self) -> HarnessFuture<'_, Probe>;
}

/// 一个独立事故场景的已通过记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseReport {
    pub name: &'static str,
}

/// 运行统一故障判据；任一错误即返回其场景名，不能跳过后冒称通过。
pub async fn run_atomic_suite<H: AtomicHarness>(host: &mut H) -> Result<Vec<CaseReport>, String> {
    let mut reports = Vec::new();
    for name in [
        "prepare_rollback",
        "prepare_replay",
        "prepare_reply_loss",
        "apply_rollback",
        "apply_reply_loss",
        "terminal_replay",
        "stale_owner",
        "cancel_before_dispatch",
        "dispatch_before_cancel",
        "cancel_unknown_then_evidence",
        "failure_closes",
    ] {
        host.reset().await.map_err(|error| format!("{name}: reset: {error}"))?;
        let resources = host.expected_resource_digest(Completion::Succeeded).is_some();
        resource_shape(&host.snapshot().await?, resources)?;
        run_case(host, name).await.map_err(|error| format!("{name}: {error}"))?;
        resource_shape(&host.snapshot().await?, resources)
            .map_err(|error| format!("{name}: {error}"))?;
        reports.push(CaseReport { name });
    }
    Ok(reports)
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message.into()) }
}

fn resource_shape(probe: &Probe, resources: bool) -> Result<(), String> {
    require(
        probe.reserve_effects.is_some() == resources
            && probe.close_effects.is_some() == resources
            && probe.resource_digest.is_some() == resources,
        "host resource applicability changed",
    )
}

async fn prepare<H: AtomicHarness>(host: &mut H) -> Result<(), String> {
    require(
        host.prepare(false, Fault::None).await? == Prepared::Created,
        "first prepare must create",
    )?;
    let probe = host.snapshot().await?;
    require(
        probe.tasks == 1 && !probe.terminal && probe.terminal_events == 0,
        "prepare facts mismatch",
    )?;
    require(probe.reserve_effects.is_none_or(|n| n == 1), "prepare resource must happen once")?;
    require(probe.close_effects.is_none_or(|n| n == 0), "prepare must not close resources")
}

async fn close<H: AtomicHarness>(host: &mut H, completion: Completion) -> Result<(), String> {
    let guard = host.guard().await?;
    require(host.apply(guard, completion, Fault::None).await?, "fresh apply must commit")?;
    closed(&host.snapshot().await?, completion, host.expected_resource_digest(completion))
}

fn closed(
    probe: &Probe,
    completion: Completion,
    expected_resources: Option<[u8; 32]>,
) -> Result<(), String> {
    let expected_execution = match completion {
        Completion::Succeeded => Execution::Succeeded,
        Completion::Failed => Execution::Failed,
        Completion::CancelledUnknown | Completion::CancelledNoCharge => Execution::Cancelled,
    };
    require(
        probe.execution == expected_execution,
        "committed execution fact differs from observation",
    )?;
    require(
        probe.resource_digest == expected_resources,
        "committed resource result differs from host policy",
    )?;
    resource_shape(probe, expected_resources.is_some())?;
    require(probe.tasks == 1 && probe.terminal && !probe.held, "terminal facts mismatch")?;
    require(probe.terminal_events == 1, "terminal event must happen once")?;
    require(probe.reserve_effects.is_none_or(|n| n == 1), "reservation count changed")?;
    require(probe.close_effects.is_none_or(|n| n == 1), "resource close must happen once")
}

async fn run_case<H: AtomicHarness>(host: &mut H, name: &str) -> Result<(), String> {
    if name == "prepare_rollback" {
        let before = host.snapshot().await?;
        require(
            host.prepare(false, Fault::BeforePrepareCommit).await.is_err(),
            "injected prepare must fail",
        )?;
        require(host.snapshot().await? == before, "partial prepare effects escaped rollback")?;
        return prepare(host).await;
    }
    if name == "prepare_reply_loss" {
        require(
            host.prepare(false, Fault::AfterPrepareCommit).await.is_err(),
            "lost reply must be observed",
        )?;
        let committed = host.snapshot().await?;
        require(
            committed.tasks == 1 && committed.reserve_effects.is_none_or(|n| n == 1),
            "lost reply did not commit",
        )?;
        require(
            host.prepare(false, Fault::None).await? == Prepared::Replayed,
            "lost reply replay must find original",
        )?;
        return require(host.snapshot().await? == committed, "lost reply repeated effects");
    }
    prepare(host).await?;
    if name == "prepare_replay" {
        let before = host.snapshot().await?;
        require(
            host.prepare(false, Fault::None).await? == Prepared::Replayed,
            "same request must replay",
        )?;
        require(
            host.prepare(true, Fault::None).await? == Prepared::Conflict,
            "different request must conflict",
        )?;
        return require(host.snapshot().await? == before, "replay or conflict changed effects");
    }
    if name == "cancel_before_dispatch" {
        require(host.cancel_prepared().await?, "prepared cancellation must win")?;
        let committed = host.snapshot().await?;
        closed(
            &committed,
            Completion::CancelledNoCharge,
            host.expected_resource_digest(Completion::CancelledNoCharge),
        )?;
        require(!host.dispatch().await?, "canceled task dispatched")?;
        require(!host.cancel_prepared().await?, "duplicate cancellation committed")?;
        return require(
            host.snapshot().await? == committed,
            "duplicate cancellation changed effects",
        );
    }
    require(host.dispatch().await?, "first dispatch must win")?;
    let before = host.snapshot().await?;
    if name == "dispatch_before_cancel" {
        require(!host.cancel_prepared().await?, "dispatch winner released resources")?;
        require(!host.dispatch().await?, "task dispatched twice")?;
        return require(host.snapshot().await? == before, "losing cancellation changed effects");
    }
    run_observation_case(host, name).await
}

async fn run_observation_case<H: AtomicHarness>(host: &mut H, name: &str) -> Result<(), String> {
    let before = host.snapshot().await?;
    let guard = host.guard().await?;
    match name {
        "apply_rollback" => {
            require(
                host.apply(guard, Completion::Succeeded, Fault::BeforeApplyCommit).await.is_err(),
                "injected apply must fail",
            )?;
            require(host.snapshot().await? == before, "partial terminal effects escaped rollback")?;
            close(host, Completion::Succeeded).await
        }
        "apply_reply_loss" => {
            require(
                host.apply(guard, Completion::Succeeded, Fault::AfterApplyCommit).await.is_err(),
                "lost reply must be observed",
            )?;
            let committed = host.snapshot().await?;
            closed(
                &committed,
                Completion::Succeeded,
                host.expected_resource_digest(Completion::Succeeded),
            )?;
            require(
                !host.apply(guard, Completion::Succeeded, Fault::None).await?,
                "lost reply repeated terminal",
            )?;
            require(host.snapshot().await? == committed, "lost reply repeated close or event")
        }
        "terminal_replay" => {
            close(host, Completion::Succeeded).await?;
            let committed = host.snapshot().await?;
            require(
                !host.apply(guard, Completion::Failed, Fault::None).await?,
                "stale failure replaced success",
            )?;
            let current = host.guard().await?;
            require(
                !host.apply(current, Completion::Succeeded, Fault::None).await?,
                "terminal reapplied at current version",
            )?;
            require(host.snapshot().await? == committed, "terminal replay changed effects")
        }
        "stale_owner" => {
            host.takeover().await?;
            let winner = host.snapshot().await?;
            require(winner.version != guard, "takeover did not change guard")?;
            require(
                !host.apply(guard, Completion::Succeeded, Fault::None).await?,
                "stale owner wrote after takeover",
            )?;
            require(host.snapshot().await? == winner, "stale owner changed effects")?;
            close(host, Completion::Succeeded).await
        }
        "cancel_unknown_then_evidence" => {
            require(
                host.apply(guard, Completion::CancelledUnknown, Fault::None).await?,
                "unknown cancellation not recorded",
            )?;
            let unknown = host.snapshot().await?;
            require(
                unknown.terminal && unknown.terminal_events == 1,
                "cancellation lost execution fact",
            )?;
            require(
                unknown.execution == Execution::Cancelled,
                "committed execution fact differs from cancellation",
            )?;
            resource_shape(&unknown, before.reserve_effects.is_some())?;
            if before.reserve_effects.is_some() {
                require(
                    unknown.held && unknown.close_effects == Some(0),
                    "unknown cancellation closed resources",
                )?;
                require(
                    unknown.resource_digest == before.resource_digest,
                    "unknown cancellation changed reserved resources",
                )?;
                close(host, Completion::CancelledNoCharge).await
            } else {
                require(
                    !unknown.held
                        && unknown.close_effects.is_none()
                        && unknown.resource_digest.is_none(),
                    "no-funds host invented financial effects",
                )
            }
        }
        "failure_closes" => close(host, Completion::Failed).await,
        _ => Err("unknown conformance case".into()),
    }
}
