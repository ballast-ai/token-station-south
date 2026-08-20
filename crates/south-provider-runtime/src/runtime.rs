//! The shared engine, its epoch ticker, and the limits every store carries.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use wasmtime::{Config, Engine};

/// What a guest may consume before it is cut off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeLimitsV1 {
    /// Ceiling on a store's linear memory. A guest allocation that would grow
    /// past it fails inside the guest, which typically traps.
    pub memory_bytes: usize,
    /// Wall-clock deadline for one guest call. A guest still running at the
    /// deadline traps at its next epoch check.
    pub call_timeout: Duration,
    /// Ceiling on any single JSON payload crossing the boundary, in either
    /// direction. A guest may not force an unbounded host allocation, and the
    /// host may not hand a guest one either.
    pub max_payload_bytes: usize,
}

impl Default for RuntimeLimitsV1 {
    fn default() -> Self {
        Self {
            // Generous for protocol translation, far below anything that
            // could pressure the host. A component is a codec, not a
            // database.
            memory_bytes: 64 * 1024 * 1024,
            call_timeout: Duration::from_secs(2),
            max_payload_bytes: 16 * 1024 * 1024,
        }
    }
}

/// How often the ticker thread advances the engine epoch. One tick is the
/// resolution of every call deadline.
pub const EPOCH_TICK: Duration = Duration::from_millis(10);
pub const MAX_STREAM_INSTANCES: usize = 64;

/// A shared engine, its epoch ticker, and the limits applied to every store.
///
/// One per process is the intent; components loaded from it share JIT caches
/// and the single ticker thread.
#[derive(Clone)]
pub struct ComponentRuntimeV1 {
    engine: Engine,
    limits: RuntimeLimitsV1,
    active_streams: Arc<AtomicUsize>,
    // Held so the ticker stops when the last clone drops.
    _ticker: Arc<TickerGuard>,
}

impl ComponentRuntimeV1 {
    /// Builds an engine with epoch interruption on and starts the ticker.
    ///
    /// # Errors
    ///
    /// Returns the engine construction error, which on a supported target
    /// means the configuration itself was rejected.
    pub fn new(limits: RuntimeLimitsV1) -> wasmtime::Result<Self> {
        let mut config = Config::new();
        config.epoch_interruption(true);
        config.wasm_component_model(true);
        // On-disk compilation cache: without it every component recompiles
        // from scratch on each start. Best-effort — if the cache cannot be
        // set up we simply keep compiling.
        if let Ok(cache) = wasmtime::Cache::from_file(None) {
            config.cache(Some(cache));
        }
        let engine = Engine::new(&config)?;

        let ticker = TickerGuard::start(engine.clone());

        Ok(Self {
            engine,
            limits,
            active_streams: Arc::new(AtomicUsize::new(0)),
            _ticker: Arc::new(ticker),
        })
    }

    pub(crate) const fn engine(&self) -> &Engine {
        &self.engine
    }

    pub(crate) const fn limits(&self) -> RuntimeLimitsV1 {
        self.limits
    }

    /// The number of epoch ticks equivalent to the configured call timeout,
    /// rounded up and never zero.
    pub(crate) fn deadline_ticks(&self) -> u64 {
        let ticks = self.limits.call_timeout.as_millis().div_ceil(EPOCH_TICK.as_millis());
        u64::try_from(ticks).unwrap_or(u64::MAX).max(1)
    }

    pub(crate) fn try_acquire_stream(&self) -> Option<StreamPermit> {
        let acquired = self
            .active_streams
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < MAX_STREAM_INSTANCES).then(|| active + 1)
            })
            .is_ok();
        acquired.then(|| StreamPermit { active: Arc::clone(&self.active_streams) })
    }
}

pub struct StreamPermit {
    active: Arc<AtomicUsize>,
}

impl Drop for StreamPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

impl std::fmt::Debug for ComponentRuntimeV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentRuntimeV1").field("limits", &self.limits).finish_non_exhaustive()
    }
}

/// Advances the engine epoch until dropped.
struct TickerGuard {
    stop: Arc<AtomicBool>,
}

impl TickerGuard {
    #[allow(
        clippy::expect_used,
        reason = "spawning a thread only fails when the process is already dying"
    )]
    fn start(engine: Engine) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&stop);

        std::thread::Builder::new()
            .name("south-component-epoch-ticker".to_owned())
            .spawn(move || {
                while !observed.load(Ordering::Relaxed) {
                    std::thread::sleep(EPOCH_TICK);
                    engine.increment_epoch();
                }
            })
            .expect("spawning a thread only fails when the process is already dying");

        Self { stop }
    }
}

impl Drop for TickerGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
