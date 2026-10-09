//! `wasi:io` host functions that never enter a tokio runtime.
//!
//! `wasmtime_wasi::p2::add_to_linker_sync` implements every blocking
//! `wasi:io` function (`blocking-flush`, `blocking-write-and-flush`,
//! `blocking-read`, `poll`, resource drops, …) by running the async
//! implementation under `tokio::runtime::Handle::block_on` whenever the
//! calling thread is inside a tokio runtime. A host calls a component from a
//! request task, so that call always panicked with "Cannot start a runtime
//! from within a runtime" the first time a guest reached one of them. A guest
//! reaches them exactly when it traps: Rust's panic and allocation-failure
//! messages are written to stderr and flushed with `blocking-flush` just
//! before the abort (host gap #101, 2026-10-09).
//!
//! The replacements below keep the semantics of the async implementations in
//! `wasmtime-wasi-io` and change only how their futures are driven:
//! [`finish`] polls them on the calling thread and never parks it. That is
//! enough because a component here has no real I/O to wait for: stdin is
//! closed, stdout and stderr discard what they are given (`Ctx::new` builds the
//! default `WasiCtx`), and the sockets and HTTP imports are refused at load. A
//! stdio future is therefore ready the first time it is polled. A future that
//! is not (a guest asking to sleep on a clock) is answered with a trap instead
//! of parking the host thread past the call deadline, which the epoch ticker
//! cannot interrupt inside a host function.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use wasmtime::component::{HasData, Linker, Resource, ResourceTable};
use wasmtime_wasi::WasiView as _;
use wasmtime_wasi::p2::bindings::sync::io::poll::{self as sync_poll, Pollable};
use wasmtime_wasi::p2::bindings::sync::io::streams::{
    self as sync_streams, InputStream, OutputStream,
};
use wasmtime_wasi::p2::{DynPollable, StreamError, StreamResult};
use wasmtime_wasi_io::bindings::wasi::io::poll::{
    Host as AsyncPoll, HostPollable as AsyncHostPollable,
};
use wasmtime_wasi_io::bindings::wasi::io::streams::{
    Host as AsyncStreams, HostInputStream as AsyncHostInputStream,
    HostOutputStream as AsyncHostOutputStream,
};

use crate::loader::Ctx;

/// Replaces the blocking `wasi:io` functions `add_to_linker_sync` registered.
///
/// Must run after `add_to_linker_sync`: it overwrites those definitions.
pub fn add_to_linker(linker: &mut Linker<Ctx>) -> wasmtime::Result<()> {
    linker.allow_shadowing(true);
    let registered =
        sync_poll::add_to_linker::<Ctx, NonBlocking>(linker, |ctx| Io(ctx.ctx().table)).and_then(
            |()| sync_streams::add_to_linker::<Ctx, NonBlocking>(linker, |ctx| Io(ctx.ctx().table)),
        );
    linker.allow_shadowing(false);
    registered
}

struct NonBlocking;

impl HasData for NonBlocking {
    type Data<'a> = Io<'a>;
}

struct Io<'a>(&'a mut ResourceTable);

const WOULD_BLOCK: &str = "the guest waited on host I/O that cannot complete: this sandbox has \
                           no blocking waits";

/// How many times a future that wakes itself is polled again before it is called stalled.
const MAX_REPOLLS: usize = 64;

/// A waker that only remembers it was woken.
struct Woken(AtomicBool);

impl Wake for Woken {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::Release);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::Release);
    }
}

/// The future did not finish without waiting for something outside this process.
struct Stalled;

/// Drives `future` to completion on the calling thread without ever parking it.
///
/// A future that returns `Pending` without having asked to be polled again is
/// waiting for the outside world and is [`Stalled`], and so is one that
/// panics while it is polled (a clock sleep built outside a tokio runtime
/// does): a host function must not take the calling task down with it.
fn finish<F: Future>(future: F) -> Result<F::Output, Stalled> {
    let woken = Arc::new(Woken(AtomicBool::new(false)));
    let waker = Waker::from(Arc::clone(&woken));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    catch_unwind(AssertUnwindSafe(|| {
        for _ in 0..MAX_REPOLLS {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return Some(output);
            }
            if !woken.0.swap(false, Ordering::AcqRel) {
                return None;
            }
        }
        None
    }))
    .ok()
    .flatten()
    .ok_or(Stalled)
}

fn stalled_trap() -> wasmtime::Error {
    wasmtime::Error::msg(WOULD_BLOCK)
}

fn stalled_stream_error() -> StreamError {
    StreamError::trap(WOULD_BLOCK)
}

impl sync_streams::Host for Io<'_> {
    fn convert_stream_error(
        &mut self,
        err: StreamError,
    ) -> wasmtime::Result<sync_streams::StreamError> {
        Ok(AsyncStreams::convert_stream_error(self.0, err)?.into())
    }
}

impl sync_streams::HostOutputStream for Io<'_> {
    fn drop(&mut self, stream: Resource<OutputStream>) -> wasmtime::Result<()> {
        finish(AsyncHostOutputStream::drop(self.0, stream)).map_err(|Stalled| stalled_trap())?
    }

    fn check_write(&mut self, stream: Resource<OutputStream>) -> StreamResult<u64> {
        AsyncHostOutputStream::check_write(self.0, stream)
    }

    fn write(&mut self, stream: Resource<OutputStream>, bytes: Vec<u8>) -> StreamResult<()> {
        AsyncHostOutputStream::write(self.0, stream, bytes)
    }

    fn blocking_write_and_flush(
        &mut self,
        stream: Resource<OutputStream>,
        bytes: Vec<u8>,
    ) -> StreamResult<()> {
        finish(AsyncHostOutputStream::blocking_write_and_flush(self.0, stream, bytes))
            .map_err(|Stalled| stalled_stream_error())?
    }

    fn blocking_write_zeroes_and_flush(
        &mut self,
        stream: Resource<OutputStream>,
        len: u64,
    ) -> StreamResult<()> {
        finish(AsyncHostOutputStream::blocking_write_zeroes_and_flush(self.0, stream, len))
            .map_err(|Stalled| stalled_stream_error())?
    }

    fn subscribe(
        &mut self,
        stream: Resource<OutputStream>,
    ) -> wasmtime::Result<Resource<Pollable>> {
        AsyncHostOutputStream::subscribe(self.0, stream)
    }

    fn write_zeroes(&mut self, stream: Resource<OutputStream>, len: u64) -> StreamResult<()> {
        AsyncHostOutputStream::write_zeroes(self.0, stream, len)
    }

    fn flush(&mut self, stream: Resource<OutputStream>) -> StreamResult<()> {
        AsyncHostOutputStream::flush(self.0, Resource::new_borrow(stream.rep()))
    }

    fn blocking_flush(&mut self, stream: Resource<OutputStream>) -> StreamResult<()> {
        finish(AsyncHostOutputStream::blocking_flush(self.0, Resource::new_borrow(stream.rep())))
            .map_err(|Stalled| stalled_stream_error())?
    }

    fn splice(
        &mut self,
        dst: Resource<OutputStream>,
        src: Resource<InputStream>,
        len: u64,
    ) -> StreamResult<u64> {
        AsyncHostOutputStream::splice(self.0, dst, src, len)
    }

    fn blocking_splice(
        &mut self,
        dst: Resource<OutputStream>,
        src: Resource<InputStream>,
        len: u64,
    ) -> StreamResult<u64> {
        finish(AsyncHostOutputStream::blocking_splice(self.0, dst, src, len))
            .map_err(|Stalled| stalled_stream_error())?
    }
}

impl sync_streams::HostInputStream for Io<'_> {
    fn drop(&mut self, stream: Resource<InputStream>) -> wasmtime::Result<()> {
        finish(AsyncHostInputStream::drop(self.0, stream)).map_err(|Stalled| stalled_trap())?
    }

    fn read(&mut self, stream: Resource<InputStream>, len: u64) -> StreamResult<Vec<u8>> {
        AsyncHostInputStream::read(self.0, stream, len)
    }

    fn blocking_read(&mut self, stream: Resource<InputStream>, len: u64) -> StreamResult<Vec<u8>> {
        finish(AsyncHostInputStream::blocking_read(self.0, stream, len))
            .map_err(|Stalled| stalled_stream_error())?
    }

    fn skip(&mut self, stream: Resource<InputStream>, len: u64) -> StreamResult<u64> {
        AsyncHostInputStream::skip(self.0, stream, len)
    }

    fn blocking_skip(&mut self, stream: Resource<InputStream>, len: u64) -> StreamResult<u64> {
        finish(AsyncHostInputStream::blocking_skip(self.0, stream, len))
            .map_err(|Stalled| stalled_stream_error())?
    }

    fn subscribe(&mut self, stream: Resource<InputStream>) -> wasmtime::Result<Resource<Pollable>> {
        AsyncHostInputStream::subscribe(self.0, stream)
    }
}

impl sync_poll::Host for Io<'_> {
    fn poll(&mut self, pollables: Vec<Resource<DynPollable>>) -> wasmtime::Result<Vec<u32>> {
        finish(AsyncPoll::poll(self.0, pollables)).map_err(|Stalled| stalled_trap())?
    }
}

impl sync_poll::HostPollable for Io<'_> {
    fn ready(&mut self, pollable: Resource<DynPollable>) -> wasmtime::Result<bool> {
        finish(AsyncHostPollable::ready(self.0, pollable)).map_err(|Stalled| stalled_trap())?
    }

    fn block(&mut self, pollable: Resource<DynPollable>) -> wasmtime::Result<()> {
        finish(AsyncHostPollable::block(self.0, pollable)).map_err(|Stalled| stalled_trap())?
    }

    fn drop(&mut self, pollable: Resource<DynPollable>) -> wasmtime::Result<()> {
        AsyncHostPollable::drop(self.0, pollable)
    }
}
