//! The fake token endpoint the suite controls.
//!
//! The host's generic recipe executor, built for a test, takes an injected token transport; that
//! transport hands every exchange to [`FakeTokenEndpointV1::exchange`] and maps the result back
//! onto whatever the production transport would have returned (a response, or a transport
//! failure). Nothing here opens a socket: §3.4 rule 6 is met by the injection existing only in
//! test builds.

use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

use super::{FakeTokenAnswerV1, FakeTokenHoldV1, FakeTokenReplyV1};

const UNSCRIPTED: FakeTokenAnswerV1 =
    FakeTokenAnswerV1::Respond { status: 500, body: r#"{"error":"unscripted_exchange"}"# };

/// One exchange request as the host's executor rendered it.
#[derive(Clone, PartialEq, Eq)]
pub struct FakeTokenRequestV1 {
    method: String,
    url: String,
    body: Vec<u8>,
}

impl FakeTokenRequestV1 {
    /// Captures a rendered exchange: the HTTP method, the full URL and the encoded body.
    #[must_use]
    pub fn new(
        method: impl Into<String>,
        url: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        Self { method: method.into(), url: url.into(), body: body.into() }
    }

    /// The HTTP method.
    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    /// The full URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The encoded body.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

impl fmt::Debug for FakeTokenRequestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FakeTokenRequestV1")
            .field("method", &self.method)
            .field("url_byte_count", &self.url.len())
            .field("body_byte_count", &self.body.len())
            .finish()
    }
}

/// A response of the fake endpoint. Its body is always JSON.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FakeTokenResponseV1 {
    status: u16,
    body: &'static str,
}

impl FakeTokenResponseV1 {
    /// The HTTP status.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// The JSON body.
    #[must_use]
    pub const fn body(&self) -> &'static str {
        self.body
    }

    /// The `content-type` the response carries.
    #[must_use]
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
}

impl fmt::Debug for FakeTokenResponseV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FakeTokenResponseV1")
            .field("status", &self.status)
            .field("body_byte_count", &self.body.len())
            .finish()
    }
}

/// The endpoint could not be reached: the injected transport must report the same failure the
/// production transport reports for a connection that never produced a response.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FakeTokenUnreachableV1;

struct EndpointState {
    replies: &'static [FakeTokenReplyV1],
    requests: Vec<FakeTokenRequestV1>,
    held: usize,
    released: bool,
    held_wakers: Vec<Waker>,
    arrival_waker: Option<Waker>,
}

/// The fake token endpoint of one case: scripted replies in call order, every request recorded.
///
/// Cloning shares the endpoint; the runner keeps one handle and gives the host another.
#[derive(Clone)]
pub struct FakeTokenEndpointV1 {
    state: Arc<Mutex<EndpointState>>,
}

impl FakeTokenEndpointV1 {
    /// An endpoint answering with `replies` in call order and 500 after they run out.
    #[must_use]
    pub fn new(replies: &'static [FakeTokenReplyV1]) -> Self {
        Self {
            state: Arc::new(Mutex::new(EndpointState {
                replies,
                requests: Vec::new(),
                held: 0,
                released: false,
                held_wakers: Vec::new(),
                arrival_waker: None,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, EndpointState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sends one exchange. The call is recorded now; the reply arrives when the returned future
    /// completes, which may be held (see [`FakeTokenHoldV1`]).
    #[must_use]
    pub fn exchange(&self, request: FakeTokenRequestV1) -> FakeTokenExchangeFutureV1 {
        let mut state = self.lock();
        let index = state.requests.len();
        state.requests.push(request);
        let reply = state.replies.get(index).copied();
        let (answer, hold) = reply
            .map_or((UNSCRIPTED, FakeTokenHoldV1::None), |reply| (reply.answer(), reply.hold()));
        let wait = match hold {
            FakeTokenHoldV1::None => Wait::Now,
            FakeTokenHoldV1::DelayMillis(millis) => Wait::Until {
                deadline: Instant::now() + Duration::from_millis(millis),
                waker: None,
            },
            FakeTokenHoldV1::UntilCompetingWrite => {
                state.held += 1;
                Wait::Released
            }
        };
        if let Some(waker) = state.arrival_waker.take() {
            waker.wake();
        }
        drop(state);
        FakeTokenExchangeFutureV1 { endpoint: self.clone(), answer, wait }
    }

    /// How many exchanges the endpoint has received.
    #[must_use]
    pub fn calls(&self) -> usize {
        self.lock().requests.len()
    }

    /// Every exchange received so far, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<FakeTokenRequestV1> {
        self.lock().requests.clone()
    }

    /// How many exchanges have been held for the competing write so far, released or not.
    pub(crate) fn held(&self) -> usize {
        self.lock().held
    }

    /// Wakes `waker` at the next exchange.
    pub(crate) fn notify_arrival(&self, waker: &Waker) {
        self.lock().arrival_waker = Some(waker.clone());
    }

    /// Releases every exchange held for the competing write, and any held later.
    pub(crate) fn release(&self) {
        let mut state = self.lock();
        state.released = true;
        let wakers = std::mem::take(&mut state.held_wakers);
        drop(state);
        for waker in wakers {
            waker.wake();
        }
    }
}

impl fmt::Debug for FakeTokenEndpointV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("FakeTokenEndpointV1").field("calls", &self.calls()).finish()
    }
}

enum Wait {
    Now,
    Until { deadline: Instant, waker: Option<Arc<Mutex<Option<Waker>>>> },
    Released,
}

/// The reply to one exchange. Runtime-agnostic: a delay is a plain thread that wakes the task, so
/// any executor can poll it.
pub struct FakeTokenExchangeFutureV1 {
    endpoint: FakeTokenEndpointV1,
    answer: FakeTokenAnswerV1,
    wait: Wait,
}

impl FakeTokenExchangeFutureV1 {
    const fn deliver(&self) -> Result<FakeTokenResponseV1, FakeTokenUnreachableV1> {
        match self.answer {
            FakeTokenAnswerV1::Respond { status, body } => Ok(FakeTokenResponseV1 { status, body }),
            FakeTokenAnswerV1::Unreachable => Err(FakeTokenUnreachableV1),
        }
    }
}

impl Future for FakeTokenExchangeFutureV1 {
    type Output = Result<FakeTokenResponseV1, FakeTokenUnreachableV1>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match &mut this.wait {
            Wait::Now => Poll::Ready(this.deliver()),
            Wait::Until { deadline, waker } => {
                if Instant::now() >= *deadline {
                    return Poll::Ready(this.deliver());
                }
                if let Some(slot) = waker {
                    *slot.lock().unwrap_or_else(PoisonError::into_inner) =
                        Some(context.waker().clone());
                } else {
                    let slot = Arc::new(Mutex::new(Some(context.waker().clone())));
                    let timer = Arc::clone(&slot);
                    let deadline = *deadline;
                    std::thread::spawn(move || {
                        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                        let waker = timer.lock().unwrap_or_else(PoisonError::into_inner).take();
                        if let Some(waker) = waker {
                            waker.wake();
                        }
                    });
                    *waker = Some(slot);
                }
                Poll::Pending
            }
            Wait::Released => {
                let mut state = this.endpoint.lock();
                if state.released {
                    drop(state);
                    return Poll::Ready(this.deliver());
                }
                state.held_wakers.push(context.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl fmt::Debug for FakeTokenExchangeFutureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FakeTokenExchangeFutureV1")
            .field("answer", &self.answer)
            .finish_non_exhaustive()
    }
}
