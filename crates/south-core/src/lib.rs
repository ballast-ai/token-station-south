#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Host-neutral provider call orchestration boundaries.

use std::{fmt, future::Future, pin::Pin, time::Duration};

use http::Method;
use south_contracts::{
    BufferedHttpResponseV1, CredentialSlotV1, JsonBodyV1, JsonPostRequestV1, PreparationErrorV1,
    ProviderEndpointV1, SafeHeaders, StreamChunkV1, StreamReadErrorV1, StreamRejectedV1,
    StreamingResponseHeadV1, TransportErrorV1,
};
use thiserror::Error;
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;
use url::Url;
use zeroize::Zeroizing;

/// A trusted endpoint and its only authorized credential slot.
pub struct ProviderBindingV1 {
    endpoint: ProviderEndpointV1,
    credential_slot: CredentialSlotV1,
}

impl ProviderBindingV1 {
    /// Binds a validated endpoint to exactly one validated credential slot.
    #[must_use]
    pub const fn new(endpoint: ProviderEndpointV1, credential_slot: CredentialSlotV1) -> Self {
        Self { endpoint, credential_slot }
    }
}

impl fmt::Debug for ProviderBindingV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ProviderBindingV1").finish_non_exhaustive()
    }
}

/// An owned credential value whose South-owned allocation is zeroized when dropped.
///
/// This guarantee covers only the allocation owned by this value. Each downstream transport
/// implementation is responsible for its own credential copies; this type cannot cover plaintext
/// copies made by transport, TLS, operating-system, or provider infrastructure buffers.
pub struct SecretValue {
    value: Zeroizing<Vec<u8>>,
}

impl SecretValue {
    /// Takes ownership of a credential returned by an injected host resolver.
    #[must_use]
    pub fn new(value: String) -> Self {
        Self { value: Zeroizing::new(value.into_bytes()) }
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

/// An intentionally opaque failure from an injected credential resolver.
///
/// Resolver-specific sources must remain on the host side of this boundary. The provider-call
/// executor maps this value to the frozen `CREDENTIAL_RESOLUTION_FAILED` preparation code.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("credential resolution failed")]
pub struct CredentialResolutionErrorV1;

/// A cancellation-safe, host-provided credential resolution future.
pub type CredentialResolutionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<SecretValue, CredentialResolutionErrorV1>> + Send + 'a>>;

/// A host-owned, read-only credential source.
///
/// Implementations must be cancellation-safe: dropping the future returned by [`Self::resolve`]
/// must stop the in-progress operation without leaving detached work behind.
pub trait CredentialResolver: Send + Sync {
    /// Resolves the one host-bound credential slot for this call.
    fn resolve<'a>(&'a self, slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a>;
}

/// A request that has passed the host endpoint and credential binding checks.
///
/// Its fields are private and this crate exposes no public constructor. Transport implementations
/// receive the plaintext credential only through [`Self::bearer_secret`].
pub struct PreparedHttpRequestV1<'request> {
    method: Method,
    url: Url,
    headers: &'request SafeHeaders,
    body: &'request JsonBodyV1,
    bearer_secret: SecretValue,
}

impl PreparedHttpRequestV1<'_> {
    /// Returns the prepared HTTP method.
    #[must_use]
    pub const fn method(&self) -> &Method {
        &self.method
    }

    /// Returns the destination proven to remain inside the trusted provider binding.
    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// Returns the validated ordinary request headers.
    #[must_use]
    pub const fn headers(&self) -> &SafeHeaders {
        self.headers
    }

    /// Returns the exact validated JSON body.
    #[must_use]
    pub const fn body(&self) -> &JsonBodyV1 {
        self.body
    }

    /// Returns the resolved Bearer credential bytes at the transport assembly boundary.
    #[must_use]
    pub fn bearer_secret(&self) -> &[u8] {
        self.bearer_secret.value.as_slice()
    }
}

impl fmt::Debug for PreparedHttpRequestV1<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedHttpRequestV1")
            .field("method", &self.method)
            .field("header_count", &self.headers.len())
            .field("body_byte_count", &self.body.len())
            .finish_non_exhaustive()
    }
}

/// A cancellation-safe asynchronous HTTP transport future.
pub type TransportFuture<'a> =
    Pin<Box<dyn Future<Output = Result<BufferedHttpResponseV1, TransportErrorV1>> + Send + 'a>>;

/// An injected asynchronous transport for an already prepared request.
///
/// Implementations must stop in-progress I/O when the returned future is dropped. The supplied
/// timeout is the remaining caller-owned deadline budget at the point transport begins.
pub trait AsyncHttpTransport: Send + Sync {
    /// Executes exactly one prepared request within the remaining timeout budget.
    fn execute<'a>(
        &'a self,
        request: &'a PreparedHttpRequestV1<'_>,
        remaining_timeout: Duration,
    ) -> TransportFuture<'a>;
}

/// A provider call failure without request, response, endpoint, or credential context.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProviderCallErrorV1 {
    /// Request preparation, cancellation, or caller deadline failed.
    #[error(transparent)]
    Preparation(#[from] PreparationErrorV1),
    /// The injected HTTP transport failed.
    #[error(transparent)]
    Transport(#[from] TransportErrorV1),
    /// The upstream refused the streaming exchange with a non-2xx status.
    ///
    /// This variant is produced only by the streaming entry point. A rejected exchange never
    /// yields a stream object; the host receives the head and a bounded error body in one shot.
    #[error("upstream rejected the streaming exchange")]
    Rejected(StreamRejectedV1),
}

impl ProviderCallErrorV1 {
    /// Returns the stable code owned by the frozen preparation, transport, or rejection error.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Preparation(error) => error.code(),
            Self::Transport(error) => error.code(),
            Self::Rejected(_) => "UPSTREAM_REJECTED",
        }
    }
}

/// Validates, authorizes, resolves, prepares, and executes one buffered JSON POST.
///
/// Cancellation has deterministic precedence. The scoped race uses biased selection, so when the
/// cancellation signal and either call completion or the absolute deadline are observed ready in
/// the same poll, this function returns `CANCELLED`.
pub async fn execute_provider_call_v1<R, T>(
    binding: &ProviderBindingV1,
    request: &JsonPostRequestV1,
    resolver: &R,
    transport: &T,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<BufferedHttpResponseV1, ProviderCallErrorV1>
where
    R: CredentialResolver + ?Sized,
    T: AsyncHttpTransport + ?Sized,
{
    let destination = request.relative_path().resolve_against(&binding.endpoint)?;

    let requested_slot = request.auth().credential_slot();
    if requested_slot != &binding.credential_slot {
        return Err(PreparationErrorV1::CredentialBindingMismatch.into());
    }

    if cancellation.is_cancelled() {
        return Err(PreparationErrorV1::Cancelled.into());
    }
    if Instant::now() >= deadline {
        return Err(PreparationErrorV1::DeadlineExceeded.into());
    }

    let execution = async {
        let secret = resolver
            .resolve(requested_slot)
            .await
            .map_err(|_| PreparationErrorV1::CredentialResolutionFailed)?;
        let prepared = PreparedHttpRequestV1 {
            method: Method::POST,
            url: destination,
            headers: request.headers(),
            body: request.body(),
            bearer_secret: secret,
        };
        let remaining_timeout = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(PreparationErrorV1::DeadlineExceeded)?;
        transport.execute(&prepared, remaining_timeout).await.map_err(ProviderCallErrorV1::from)
    };

    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(PreparationErrorV1::Cancelled.into()),
        result = timeout_at(deadline, execution) => {
            result.unwrap_or_else(|_| Err(PreparationErrorV1::DeadlineExceeded.into()))
        }
    }
}

/// A cancellation-safe future yielding one bounded chunk pull from a streaming transport.
pub type StreamChunkFutureV1<'a> =
    Pin<Box<dyn Future<Output = Option<Result<StreamChunkV1, StreamReadErrorV1>>> + Send + 'a>>;

/// A transport-owned source of bounded response body chunks.
///
/// Implementations must be cancellation-safe: dropping a pending [`Self::next_chunk`] future
/// between chunks must lose no delivered bytes and leave no detached work, so the caller may
/// re-pull later. The transport owns its idle guard, so a stalled upstream must surface as
/// [`StreamReadErrorV1::StreamIdleTimeout`] from within the pull future. Dropping the source
/// itself must abort the underlying exchange.
pub trait StreamByteSourceV1: Send {
    /// Pulls the next bounded chunk; `None` is a clean upstream end of stream.
    fn next_chunk(&mut self) -> StreamChunkFutureV1<'_>;
}

/// A headers-ready streaming exchange handed from a transport to the orchestration layer.
///
/// Construction proves the invariant that an opened stream always carries a 2xx head; every
/// non-2xx exchange must instead be collapsed into [`StreamRejectedV1`] by the transport.
pub struct OpenedByteStreamV1 {
    head: StreamingResponseHeadV1,
    source: Box<dyn StreamByteSourceV1>,
}

impl OpenedByteStreamV1 {
    /// Binds a 2xx head to its transport chunk source.
    ///
    /// A non-2xx head is a transport implementation error and is refused with the context-free
    /// `REQUEST_FAILED` code rather than becoming a live stream.
    pub fn try_new(
        head: StreamingResponseHeadV1,
        source: Box<dyn StreamByteSourceV1>,
    ) -> Result<Self, TransportErrorV1> {
        if !head.status().is_success() {
            return Err(TransportErrorV1::RequestFailed);
        }
        Ok(Self { head, source })
    }
}

impl fmt::Debug for OpenedByteStreamV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenedByteStreamV1")
            .field("head", &self.head)
            .finish_non_exhaustive()
    }
}

/// A failure reported by a streaming transport while opening one exchange.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum StreamOpenErrorV1 {
    /// The upstream answered with a non-2xx status and a bounded error body.
    #[error("upstream rejected the streaming exchange")]
    Rejected(StreamRejectedV1),
    /// The transport failed before any streaming state existed.
    #[error(transparent)]
    Transport(#[from] TransportErrorV1),
}

/// A cancellation-safe future opening one streaming exchange up to headers-ready.
pub type StreamingOpenFutureV1<'a> =
    Pin<Box<dyn Future<Output = Result<OpenedByteStreamV1, StreamOpenErrorV1>> + Send + 'a>>;

/// An injected asynchronous streaming transport for an already prepared request.
///
/// Implementations must stop in-progress I/O when the returned future is dropped and must never
/// yield a body byte before the head: the future resolves at headers-ready, non-2xx exchanges
/// collapse into [`StreamOpenErrorV1::Rejected`] with a bounded error body, and redirects are
/// refused with `REDIRECT_DENIED` without a second request.
pub trait AsyncStreamingTransport: Send + Sync {
    /// Opens exactly one prepared streaming POST up to headers-ready.
    fn open<'a>(&'a self, request: &'a PreparedHttpRequestV1<'_>) -> StreamingOpenFutureV1<'a>;
}

/// A live 2xx streaming exchange with pull-based bounded chunk delivery.
///
/// The deadline and cancellation token supplied at open time stay armed for the whole stream:
/// both are observed on every pull, and firing mid-pull aborts the in-flight read. The
/// [`Self::next_chunk`] future is cancel-safe and `select!`-compatible; dropping it between
/// chunks loses nothing. After any terminal error, later pulls return `None` without touching
/// the transport again. Dropping this value aborts the underlying connection; there is no
/// detached work to leak.
pub struct StreamingCallV1 {
    head: StreamingResponseHeadV1,
    source: Box<dyn StreamByteSourceV1>,
    deadline: Option<Instant>,
    cancellation: CancellationToken,
    finished: bool,
}

impl StreamingCallV1 {
    /// Returns the headers-ready metadata; an opened stream always carries a 2xx status.
    #[must_use]
    pub const fn head(&self) -> &StreamingResponseHeadV1 {
        &self.head
    }

    /// Pulls the next bounded chunk; `None` is a clean upstream end of stream.
    ///
    /// Cancellation has deterministic precedence: when the cancellation signal and any other
    /// completion are observed ready in the same poll, the pull reports `STREAM_CANCELLED`.
    pub async fn next_chunk(&mut self) -> Option<Result<StreamChunkV1, StreamReadErrorV1>> {
        if self.finished {
            return None;
        }

        let deadline = self.deadline;
        let pull = self.source.next_chunk();
        let bounded_pull = async {
            match deadline {
                Some(deadline) => timeout_at(deadline, pull)
                    .await
                    .unwrap_or(Some(Err(StreamReadErrorV1::StreamDeadlineExceeded))),
                None => pull.await,
            }
        };
        let outcome = tokio::select! {
            biased;
            () = self.cancellation.cancelled() => Some(Err(StreamReadErrorV1::StreamCancelled)),
            outcome = bounded_pull => outcome,
        };

        if !matches!(outcome, Some(Ok(_))) {
            self.finished = true;
        }
        outcome
    }
}

impl fmt::Debug for StreamingCallV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamingCallV1")
            .field("head", &self.head)
            .field("has_deadline", &self.deadline.is_some())
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

/// Validates, authorizes, resolves, prepares, and opens one streaming JSON POST.
///
/// The validation phases are identical to [`execute_provider_call_v1`]: URL containment, then
/// credential slot match, then pre-flight cancellation and deadline checks, then credential
/// resolution inside the same cancellation and deadline scope. The transport resolves at
/// headers-ready; a non-2xx upstream returns [`ProviderCallErrorV1::Rejected`] and no stream
/// object ever exists for a rejected exchange.
///
/// A `None` deadline is legal only because the transport idle guard is not optional; every
/// silent upstream still dies within the transport's configured idle bound.
pub async fn open_streaming_provider_call_v1<R, T>(
    binding: &ProviderBindingV1,
    request: &JsonPostRequestV1,
    resolver: &R,
    transport: &T,
    deadline: Option<Instant>,
    cancellation: &CancellationToken,
) -> Result<StreamingCallV1, ProviderCallErrorV1>
where
    R: CredentialResolver + ?Sized,
    T: AsyncStreamingTransport + ?Sized,
{
    let destination = request.relative_path().resolve_against(&binding.endpoint)?;

    let requested_slot = request.auth().credential_slot();
    if requested_slot != &binding.credential_slot {
        return Err(PreparationErrorV1::CredentialBindingMismatch.into());
    }

    if cancellation.is_cancelled() {
        return Err(PreparationErrorV1::Cancelled.into());
    }
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(PreparationErrorV1::DeadlineExceeded.into());
    }

    let open = async {
        let secret = resolver
            .resolve(requested_slot)
            .await
            .map_err(|_| PreparationErrorV1::CredentialResolutionFailed)?;
        let prepared = PreparedHttpRequestV1 {
            method: Method::POST,
            url: destination,
            headers: request.headers(),
            body: request.body(),
            bearer_secret: secret,
        };
        transport.open(&prepared).await.map_err(|error| match error {
            StreamOpenErrorV1::Rejected(rejected) => ProviderCallErrorV1::Rejected(rejected),
            StreamOpenErrorV1::Transport(error) => error.into(),
        })
    };
    let bounded_open = async {
        match deadline {
            Some(deadline) => timeout_at(deadline, open)
                .await
                .unwrap_or_else(|_| Err(PreparationErrorV1::DeadlineExceeded.into())),
            None => open.await,
        }
    };
    let opened = tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(ProviderCallErrorV1::from(PreparationErrorV1::Cancelled)),
        result = bounded_open => result,
    }?;

    Ok(StreamingCallV1 {
        head: opened.head,
        source: opened.source,
        deadline,
        cancellation: cancellation.clone(),
        finished: false,
    })
}
