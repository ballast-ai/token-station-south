#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Host-neutral provider call orchestration boundaries.

use std::{fmt, future::Future, pin::Pin, time::Duration};

use http::Method;
use south_contracts::{
    BufferedHttpResponseV1, CredentialSlotV1, JsonBodyV1, JsonPostRequestV1, PreparationErrorV1,
    ProviderEndpointV1, SafeHeaders, TransportErrorV1,
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
/// This guarantee covers this value and a South-owned HTTP authorization owner. It cannot cover
/// plaintext copies made by HTTP, TLS, operating-system, or provider infrastructure buffers.
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
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ProviderCallErrorV1 {
    /// Request preparation, cancellation, or caller deadline failed.
    #[error(transparent)]
    Preparation(#[from] PreparationErrorV1),
    /// The injected HTTP transport failed.
    #[error(transparent)]
    Transport(#[from] TransportErrorV1),
}

impl ProviderCallErrorV1 {
    /// Returns the stable code owned by the frozen preparation or transport error.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Preparation(error) => error.code(),
            Self::Transport(error) => error.code(),
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
