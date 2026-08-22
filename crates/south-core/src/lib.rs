#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Host-neutral provider call orchestration boundaries.

pub mod raw;

use std::{fmt, future::Future, pin::Pin, time::Duration};

use http::Method;
use south_contracts::{
    BufferedHttpResponseV1, ControlledUserAgentV1, CredentialSlotV1, JsonBodyV1, JsonPostRequestV1,
    PreparationErrorV1, ProviderAuthV1, ProviderEndpointV1, SafeHeaders, SignedHeaderSetV1,
    SignedHeaderV1, StreamChunkV1, StreamReadErrorV1, StreamRejectedV1, StreamingResponseHeadV1,
    TransportErrorV1,
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

/// The finalised request, exactly as the transport will send it.
///
/// A signature is a function of the whole request, so the finalizer must see the request after
/// every South-owned decision is made: the path resolved against the binding, the sanctioned
/// query appended, the ordinary headers validated, the body bytes fixed. Anything South changed
/// afterwards would invalidate the signature it just asked for.
///
/// The view carries no credential. For this arm South never resolves the slot (host-signed D2);
/// it hands over the slot *declaration* so a finalizer serving several identities can pick the
/// right signing material from its own store.
///
/// Fields are private with accessors, unlike the sketch in the design record: it is the shape
/// every other type in this crate uses, and it lets the view grow a field without breaking every
/// finalizer. `user_agent` is one such field the record missed — it called the user agent "already
/// a `SafeHeaders` host obligation and therefore in the view", but `user-agent` is reserved and
/// travels in its own typed slot, so a finalizer that signs it needs this accessor to see it.
pub struct FinalizeViewV1<'a> {
    method: &'a Method,
    url: &'a Url,
    headers: &'a SafeHeaders,
    body: &'a [u8],
    user_agent: Option<ControlledUserAgentV1>,
    slot: &'a CredentialSlotV1,
    emits: &'a SignedHeaderSetV1,
}

impl<'a> FinalizeViewV1<'a> {
    /// Returns the method the transport will use.
    #[must_use]
    pub const fn method(&self) -> &'a Method {
        self.method
    }

    /// Returns the binding-resolved URL, sanctioned query already appended.
    #[must_use]
    pub const fn url(&self) -> &'a Url {
        self.url
    }

    /// Returns the validated ordinary headers, in declaration order.
    #[must_use]
    pub const fn headers(&self) -> &'a SafeHeaders {
        self.headers
    }

    /// Returns the exact body bytes the transport will write.
    #[must_use]
    pub const fn body(&self) -> &'a [u8] {
        self.body
    }

    /// Returns the sanctioned user-agent the transport will apply, when the request declared one.
    #[must_use]
    pub const fn user_agent(&self) -> Option<ControlledUserAgentV1> {
        self.user_agent
    }

    /// Returns the credential-slot declaration naming the signing identity.
    #[must_use]
    pub const fn slot(&self) -> &'a CredentialSlotV1 {
        self.slot
    }

    /// Returns the headers this finalizer is required to emit — no more, no fewer.
    #[must_use]
    pub const fn emits(&self) -> &'a SignedHeaderSetV1 {
        self.emits
    }
}

impl fmt::Debug for FinalizeViewV1<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The URL and body are request content, not credentials, but a view is the one place that
        // holds all of a request at once; keep `Debug` to shapes so a stray log line cannot
        // reproduce a customer payload.
        formatter
            .debug_struct("FinalizeViewV1")
            .field("method", &self.method)
            .field("header_count", &self.headers.len())
            .field("body_byte_count", &self.body.len())
            .field("emits", &self.emits.headers())
            .finish_non_exhaustive()
    }
}

/// The headers a finalizer emitted, on their way through South's allow-list diff.
///
/// Values zeroize on drop. A signature is not a secret in the sense a credential is — it is
/// public once sent — but it is derived from one, and a signing key recovered from a signature is
/// a cryptographic failure, not a logging one. Treating these like credentials costs nothing.
#[derive(Default)]
pub struct FinalizedHeadersV1 {
    values: Vec<(SignedHeaderV1, Zeroizing<Vec<u8>>)>,
}

impl FinalizedHeadersV1 {
    /// Starts an empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one emitted header.
    ///
    /// Deliberately infallible: duplicates and empties are South's to reject, in one place, with
    /// one error code. A finalizer that could fail here would have two ways to report the same
    /// mistake, and the second one would be untested.
    pub fn insert(&mut self, header: SignedHeaderV1, value: Vec<u8>) -> &mut Self {
        self.values.push((header, Zeroizing::new(value)));
        self
    }

    /// Returns the emitted headers in the order the finalizer produced them.
    #[must_use]
    pub fn emitted(&self) -> impl ExactSizeIterator<Item = (SignedHeaderV1, &[u8])> {
        self.values.iter().map(|(header, value)| (*header, value.as_slice()))
    }
}

impl fmt::Debug for FinalizedHeadersV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FinalizedHeadersV1")
            .field("emitted", &self.values.iter().map(|(header, _)| *header).collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

/// An intentionally opaque failure from an injected request finalizer.
///
/// Signer-specific sources stay host-side, exactly as [`CredentialResolutionErrorV1`] keeps
/// resolver-specific sources there: a signing failure often names the key that failed.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("host request finalization failed")]
pub struct RequestFinalizationErrorV1;

/// A cancellation-safe, host-provided request finalization future.
pub type FinalizeFutureV1<'a> = Pin<
    Box<dyn Future<Output = Result<FinalizedHeadersV1, RequestFinalizationErrorV1>> + Send + 'a>,
>;

/// A host-owned signer that turns a finalised request into its declared auth headers.
///
/// South ships no signing algorithm and never sees a signing key. Implementations must be
/// cancellation-safe: dropping the future returned by [`Self::finalize`] must stop the operation
/// without leaving detached work behind.
pub trait RequestFinalizerV1: Send + Sync {
    /// Signs the finalised request and returns exactly the declared headers.
    fn finalize<'a>(&'a self, view: FinalizeViewV1<'a>) -> FinalizeFutureV1<'a>;
}

/// One auth header bound to the wire: its frozen name and its zeroizing value.
type BoundAuthHeader = (&'static str, Zeroizing<Vec<u8>>);

/// Diffs what the finalizer emitted against what it declared.
///
/// Rejection order is the design record's: an undeclared name, a declared name that never
/// arrived, an empty value, a duplicate. All four are the same code — an operator reading
/// `REQUEST_FINALIZATION_REJECTED` needs to know the signer is broken, and the four cases are
/// distinguished in the conformance suite rather than in a code the wire carries.
fn diff_finalized(
    emits: &SignedHeaderSetV1,
    finalized: &FinalizedHeadersV1,
) -> Result<Vec<BoundAuthHeader>, PreparationErrorV1> {
    // Pass one, over what arrived: nothing undeclared, nothing empty, nothing twice.
    let mut seen: Vec<SignedHeaderV1> = Vec::with_capacity(emits.len());
    for (header, value) in finalized.emitted() {
        if !emits.contains(header) || value.is_empty() || seen.contains(&header) {
            return Err(PreparationErrorV1::RequestFinalizationRejected);
        }
        seen.push(header);
    }

    // Pass two, over what was declared: everything declared arrived. This is also where the
    // binding order comes from — the declaration's canonical order, not the order the signer
    // happened to emit in, so the wire is a function of the declaration alone.
    //
    // A `seen.len() != emits.len()` check between the two passes would read like a third judge
    // and be dead code: pass one already proved every arrival is declared and unique, so a count
    // mismatch can only mean a declared header is missing, which is exactly what this loop
    // rejects. Removing it as a mutation left every test green — that is what dead judges look
    // like, and it is why this loop keeps the rejection instead of an `unwrap`.
    let mut bound: Vec<BoundAuthHeader> = Vec::with_capacity(emits.len());
    for declared in emits.headers() {
        let Some((_, value)) = finalized.emitted().find(|(header, _)| header == declared) else {
            return Err(PreparationErrorV1::RequestFinalizationRejected);
        };
        bound.push((declared.header_name(), Zeroizing::new(value.to_vec())));
    }
    Ok(bound)
}

/// A request that has passed the host endpoint and credential binding checks.
///
/// Its fields are private and this crate exposes no public constructor. Transport implementations
/// receive the plaintext credential only through [`Self::auth_headers`].
pub struct PreparedHttpRequestV1<'request> {
    method: Method,
    url: Url,
    headers: &'request SafeHeaders,
    body: &'request JsonBodyV1,
    auth_headers: Vec<BoundAuthHeader>,
    user_agent: Option<ControlledUserAgentV1>,
}

impl<'request> PreparedHttpRequestV1<'request> {
    /// Binds the resolved secret to the one auth header declared by the request.
    ///
    /// The Bearer arm produces `authorization` with a `Bearer `-prefixed value; the header-secret
    /// arm produces the sanctioned header name with the verbatim secret bytes. The value lives in
    /// a South-owned allocation that zeroizes on drop, and the original resolver allocation is
    /// dropped (and therefore zeroized) here when a prefixed copy replaces it.
    ///
    /// `ProviderAuthV1` is `#[non_exhaustive]` since 0.7.0 (host-prelude D2): an arm newer than
    /// this crate fails closed as `UNSUPPORTED_AUTH_SHAPE` (the resolved secret is dropped and
    /// zeroized on that path), never panics.
    fn assemble(
        request: &'request JsonPostRequestV1,
        destination: Url,
        secret: SecretValue,
    ) -> Result<Self, PreparationErrorV1> {
        let (auth_header_name, auth_header_value) = match request.auth() {
            ProviderAuthV1::Bearer(_) => {
                const BEARER_PREFIX: &[u8] = b"Bearer ";
                let mut value =
                    Zeroizing::new(Vec::with_capacity(BEARER_PREFIX.len() + secret.value.len()));
                value.extend_from_slice(BEARER_PREFIX);
                value.extend_from_slice(&secret.value);
                ("authorization", value)
            }
            ProviderAuthV1::HeaderSecret { header, .. } => (header.header_name(), secret.value),
            // The host-signed arm has no secret to bind and no header to assemble here: its
            // headers exist only after the finalizer has seen the finished request. Reaching this
            // point means a caller used the unsigned entry point for a signed request, which is a
            // build/wiring mistake, not a runtime condition — fail closed rather than send an
            // unauthenticated request that the upstream would reject with a confusing 403.
            ProviderAuthV1::HostSigned { .. } | _ => {
                return Err(PreparationErrorV1::UnsupportedAuthShape);
            }
        };

        Ok(Self {
            method: Method::POST,
            url: destination,
            headers: request.headers(),
            body: request.body(),
            auth_headers: vec![(auth_header_name, auth_header_value)],
            user_agent: request.user_agent(),
        })
    }

    /// Assembles the host-signed arm: no auth header yet, and no credential resolved.
    ///
    /// The result is the request the finalizer sees. `bind_finalized` completes it.
    fn assemble_unsigned(
        request: &'request JsonPostRequestV1,
        destination: Url,
    ) -> Result<Self, PreparationErrorV1> {
        match request.auth() {
            ProviderAuthV1::HostSigned { .. } => Ok(Self {
                method: Method::POST,
                url: destination,
                headers: request.headers(),
                body: request.body(),
                auth_headers: Vec::new(),
                user_agent: request.user_agent(),
            }),
            _ => Err(PreparationErrorV1::UnsupportedAuthShape),
        }
    }

    /// Borrows this request as the view a finalizer signs.
    fn finalize_view<'view>(
        &'view self,
        emits: &'view SignedHeaderSetV1,
        slot: &'view CredentialSlotV1,
    ) -> FinalizeViewV1<'view> {
        FinalizeViewV1 {
            method: &self.method,
            url: &self.url,
            headers: self.headers,
            body: self.body.as_str().as_bytes(),
            user_agent: self.user_agent,
            slot,
            emits,
        }
    }

    /// Attaches the diffed finalizer output.
    fn bind_finalized(&mut self, headers: Vec<BoundAuthHeader>) {
        self.auth_headers = headers;
    }
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

    /// Returns every auth header as its name and complete value bytes.
    ///
    /// For the Bearer arm the value carries its `Bearer ` prefix; for the header-secret arm the
    /// value is the verbatim resolved secret. No name is ever a plain-channel header: every
    /// sanctioned name, every signed name, and `authorization` itself stay on the reserved-header
    /// blacklist.
    ///
    /// The two credential arms yield exactly one element, as the single-header accessor this
    /// replaced always did. The host-signed arm yields the finalizer's diffed output in the
    /// declaration's canonical order — one to four elements, never zero: a request that reached a
    /// transport has passed the allow-list diff, and an empty declaration cannot be constructed.
    #[must_use]
    pub fn auth_headers(&self) -> impl ExactSizeIterator<Item = (&'static str, &[u8])> {
        self.auth_headers.iter().map(|(name, value)| (*name, value.as_slice()))
    }

    /// Returns the sanctioned user-agent declaration, when the request attached one.
    ///
    /// Transports must apply it as the request's only `user-agent` header. The ordinary header
    /// channel cannot carry the name (it is reserved) and the auth channel never produces it, so
    /// applying this declaration is the single source of the header on the wire.
    #[must_use]
    pub const fn user_agent(&self) -> Option<ControlledUserAgentV1> {
        self.user_agent
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
    let destination =
        request.relative_path().resolve_against_with_query(&binding.endpoint, request.query())?;

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
        let prepared = PreparedHttpRequestV1::assemble(request, destination, secret)?;
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

/// Executes one host-signed buffered provider call.
///
/// The twin of [`execute_provider_call_v1`] for [`ProviderAuthV1::HostSigned`]: a finalizer takes
/// the resolver's place. A separate entry point rather than an extra parameter, because the two
/// are mutually exclusive — this arm never resolves a credential (host-signed D2), and the other
/// two never sign. One function taking both would have to reject three of the four combinations
/// at runtime; two functions reject them at the call site.
///
/// The finalizer runs inside the same biased `select!` as credential resolution does, so
/// cancellation and the absolute deadline pre-empt a slow signer exactly as they pre-empt a slow
/// resolver. It is called at most once: a retry is a new call, with a fresh timestamp.
///
/// # Errors
///
/// Returns [`ProviderCallErrorV1`]. A non-host-signed request is `UNSUPPORTED_AUTH_SHAPE`; a
/// finalizer error is `REQUEST_FINALIZATION_FAILED`; output that does not match the declaration
/// is `REQUEST_FINALIZATION_REJECTED`. All three are preparation errors — nothing reached the
/// network.
pub async fn execute_signed_provider_call_v1<F, T>(
    binding: &ProviderBindingV1,
    request: &JsonPostRequestV1,
    finalizer: &F,
    transport: &T,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<BufferedHttpResponseV1, ProviderCallErrorV1>
where
    F: RequestFinalizerV1 + ?Sized,
    T: AsyncHttpTransport + ?Sized,
{
    let (destination, emits) = prepare_signed(binding, request)?;

    if cancellation.is_cancelled() {
        return Err(PreparationErrorV1::Cancelled.into());
    }
    if Instant::now() >= deadline {
        return Err(PreparationErrorV1::DeadlineExceeded.into());
    }

    let execution = async {
        let mut prepared = PreparedHttpRequestV1::assemble_unsigned(request, destination)?;
        finalize_into(&mut prepared, finalizer, emits, request.auth().credential_slot()).await?;
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

/// Resolves the destination and proves the request really is host-signed.
fn prepare_signed<'request>(
    binding: &ProviderBindingV1,
    request: &'request JsonPostRequestV1,
) -> Result<(Url, &'request SignedHeaderSetV1), ProviderCallErrorV1> {
    let destination =
        request.relative_path().resolve_against_with_query(&binding.endpoint, request.query())?;

    let ProviderAuthV1::HostSigned { emits, .. } = request.auth() else {
        return Err(PreparationErrorV1::UnsupportedAuthShape.into());
    };

    // The binding check is identical to the other arms: South never resolves this slot, but the
    // binding still decides which identity a request may be signed with.
    if request.auth().credential_slot() != &binding.credential_slot {
        return Err(PreparationErrorV1::CredentialBindingMismatch.into());
    }

    Ok((destination, emits))
}

/// Runs the finalizer over the finished request and binds its diffed output.
async fn finalize_into<F>(
    prepared: &mut PreparedHttpRequestV1<'_>,
    finalizer: &F,
    emits: &SignedHeaderSetV1,
    slot: &CredentialSlotV1,
) -> Result<(), ProviderCallErrorV1>
where
    F: RequestFinalizerV1 + ?Sized,
{
    let emitted = finalizer
        .finalize(prepared.finalize_view(emits, slot))
        .await
        .map_err(|_| PreparationErrorV1::RequestFinalizationFailed)?;
    let bound = diff_finalized(emits, &emitted)?;
    prepared.bind_finalized(bound);
    Ok(())
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
        // Both signals are observed at every pull entry before the source is touched, so a
        // continuously ready source cannot keep delivering past cancellation or the deadline.
        // Cancellation is checked first to keep its documented precedence.
        if self.cancellation.is_cancelled() {
            self.finished = true;
            return Some(Err(StreamReadErrorV1::StreamCancelled));
        }
        if self.deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            self.finished = true;
            return Some(Err(StreamReadErrorV1::StreamDeadlineExceeded));
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
    let destination =
        request.relative_path().resolve_against_with_query(&binding.endpoint, request.query())?;

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
        let prepared = PreparedHttpRequestV1::assemble(request, destination, secret)
            .map_err(ProviderCallErrorV1::from)?;
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

/// Opens one host-signed streaming provider call.
///
/// The streaming twin of [`execute_signed_provider_call_v1`]; same seam, same position, same
/// three preparation errors. Streaming does not change when the signature is computed: the body
/// is complete before the request is opened, so there is exactly one thing to sign here too.
/// (Request-body streaming with chunked signatures is out of scope — see the design record §5.)
///
/// # Errors
///
/// Returns [`ProviderCallErrorV1`], as the buffered twin does.
pub async fn open_streaming_signed_provider_call_v1<F, T>(
    binding: &ProviderBindingV1,
    request: &JsonPostRequestV1,
    finalizer: &F,
    transport: &T,
    deadline: Option<Instant>,
    cancellation: &CancellationToken,
) -> Result<StreamingCallV1, ProviderCallErrorV1>
where
    F: RequestFinalizerV1 + ?Sized,
    T: AsyncStreamingTransport + ?Sized,
{
    let (destination, emits) = prepare_signed(binding, request)?;

    if cancellation.is_cancelled() {
        return Err(PreparationErrorV1::Cancelled.into());
    }
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(PreparationErrorV1::DeadlineExceeded.into());
    }

    let open = async {
        let mut prepared = PreparedHttpRequestV1::assemble_unsigned(request, destination)
            .map_err(ProviderCallErrorV1::from)?;
        finalize_into(&mut prepared, finalizer, emits, request.auth().credential_slot()).await?;
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
