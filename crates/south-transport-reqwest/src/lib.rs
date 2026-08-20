#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Asynchronous provider transport boundary for reqwest hosts.

use std::{fmt, sync::Arc, time::Duration};

use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, header};
use south_contracts::{
    BufferedHttpResponseV1, MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES, MAX_RESPONSE_BODY_BYTES,
    MAX_RESPONSE_CONTENT_TYPE_BYTES, MAX_RESPONSE_RETRY_AFTER_BYTES, MAX_STREAM_CHUNK_BYTES,
    MAX_STREAM_ERROR_BODY_BYTES, ProviderQuotaMetadataFieldV1, ProviderQuotaMetadataV1,
    StreamChunkV1, StreamReadErrorV1, StreamRejectedV1, StreamTransportConfigV1,
    StreamingResponseHeadV1, TransportErrorV1,
};
use south_core::{
    AsyncHttpTransport, AsyncStreamingTransport, OpenedByteStreamV1, PreparedHttpRequestV1,
    StreamByteSourceV1, StreamChunkFutureV1, StreamOpenErrorV1, StreamingOpenFutureV1,
    TransportFuture,
};
use zeroize::Zeroizing;

pub use south_contracts::MAX_TRANSPORT_TIMEOUT;

/// Explicit bounded timeouts for one dedicated reqwest transport.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ReqwestTransportConfigV1 {
    total: Duration,
    connect: Duration,
    read: Duration,
}

impl ReqwestTransportConfigV1 {
    /// Validates explicit total, connect, and per-read timeouts.
    pub fn try_new(
        total_timeout: Duration,
        connect_timeout: Duration,
        read_timeout: Duration,
    ) -> Result<Self, TransportErrorV1> {
        if total_timeout.is_zero()
            || total_timeout > MAX_TRANSPORT_TIMEOUT
            || connect_timeout.is_zero()
            || connect_timeout > total_timeout
            || read_timeout.is_zero()
            || read_timeout > total_timeout
        {
            return Err(TransportErrorV1::ClientBuildFailed);
        }

        Ok(Self { total: total_timeout, connect: connect_timeout, read: read_timeout })
    }

    /// Returns the transport-wide request timeout.
    #[must_use]
    pub const fn total_timeout(self) -> Duration {
        self.total
    }

    /// Returns the TCP connect timeout.
    #[must_use]
    pub const fn connect_timeout(self) -> Duration {
        self.connect
    }

    /// Returns the timeout reset for each response read.
    #[must_use]
    pub const fn read_timeout(self) -> Duration {
        self.read
    }
}

impl fmt::Debug for ReqwestTransportConfigV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReqwestTransportConfigV1")
            .field("total_timeout", &self.total)
            .field("connect_timeout", &self.connect)
            .field("read_timeout", &self.read)
            .finish()
    }
}

/// A dedicated asynchronous client with provider-call policy fixed at construction.
pub struct ReqwestTransportV1 {
    client: reqwest::Client,
    total_timeout: Duration,
}

impl ReqwestTransportV1 {
    /// Builds one transport with proxies, redirects, retries, referers, and decoding disabled.
    pub fn new(config: ReqwestTransportConfigV1) -> Result<Self, TransportErrorV1> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .timeout(config.total)
            .connect_timeout(config.connect)
            .read_timeout(config.read)
            .build()
            .map_err(|_| TransportErrorV1::ClientBuildFailed)?;

        Ok(Self { client, total_timeout: config.total })
    }
}

impl fmt::Debug for ReqwestTransportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ReqwestTransportV1").finish_non_exhaustive()
    }
}

impl AsyncHttpTransport for ReqwestTransportV1 {
    fn execute<'a>(
        &'a self,
        request: &'a PreparedHttpRequestV1<'_>,
        remaining_timeout: Duration,
    ) -> TransportFuture<'a> {
        Box::pin(async move { self.execute_one(request, remaining_timeout).await })
    }
}

impl ReqwestTransportV1 {
    async fn execute_one(
        &self,
        request: &PreparedHttpRequestV1<'_>,
        remaining_timeout: Duration,
    ) -> Result<BufferedHttpResponseV1, TransportErrorV1> {
        let headers = assemble_headers(request)?;
        let response = self
            .client
            .request(request.method().clone(), request.url().clone())
            .headers(headers)
            .body(request_body(request.body().shared_owner()))
            .timeout(effective_timeout(remaining_timeout, self.total_timeout))
            .send()
            .await
            .map_err(|error| classify_send_error(&error))?;

        if response.status().is_redirection() {
            return Err(TransportErrorV1::RedirectDenied);
        }
        if response
            .content_length()
            .is_some_and(|declared| declared > MAX_RESPONSE_BODY_BYTES as u64)
        {
            return Err(TransportErrorV1::ResponseBodyTooLarge);
        }

        let status = response.status();
        let content_type = response_metadata(
            response.headers(),
            header::CONTENT_TYPE,
            MAX_RESPONSE_CONTENT_TYPE_BYTES,
        )?;
        let retry_after = response_metadata(
            response.headers(),
            header::RETRY_AFTER,
            MAX_RESPONSE_RETRY_AFTER_BYTES,
        )?;
        let provider_quota_metadata = provider_quota_metadata(response.headers())?;
        let body = read_bounded_body(response).await?;

        BufferedHttpResponseV1::try_from_parts_with_provider_quota_metadata(
            status,
            body,
            content_type,
            retry_after,
            provider_quota_metadata,
        )
    }
}

/// A dedicated asynchronous streaming client with provider-call policy fixed at construction.
///
/// Hardening matches the buffered transport: no proxy, no redirects, no retries, no referer, and
/// **all decompression disabled** — byte transparency is part of the streaming contract, because
/// host-side frame decoding (for example eventstream CRC checks) would break under transparent
/// decompression. The `accept` header is a host obligation through the normal `SafeHeaders`
/// path; this transport does not guess it.
///
/// Timeout wiring: the connect guard lives on the client builder; the idle guard is reqwest's
/// per-read timeout, which covers every body read await and doubles as the time-to-first-byte
/// bound during the header wait; the optional total is an outer per-request bound. A transport
/// timer that fires before headers-ready keeps the pre-stream `TRANSPORT_TIMEOUT` code. The
/// frozen mid-stream taxonomy owns no transport-timeout code, so any transport timer expiring
/// mid-stream — the idle guard, or the optional total bound — surfaces as `STREAM_IDLE_TIMEOUT`;
/// only the caller's own deadline maps to `STREAM_DEADLINE_EXCEEDED`.
///
/// The auth header value — `Authorization` for the Bearer arm, one sanctioned secret header for
/// the header-secret arm — shares one South-owned allocation that zeroizes its plaintext bytes
/// when the exchange releases its last clone, with the same coverage caveats as the buffered
/// transport.
pub struct ReqwestStreamingTransportV1 {
    client: reqwest::Client,
    total_timeout: Option<Duration>,
}

impl ReqwestStreamingTransportV1 {
    /// Builds one streaming transport from validated stream timeouts.
    pub fn new(config: StreamTransportConfigV1) -> Result<Self, TransportErrorV1> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(config.connect_timeout())
            .read_timeout(config.idle_timeout())
            .build()
            .map_err(|_| TransportErrorV1::ClientBuildFailed)?;

        Ok(Self { client, total_timeout: config.total_timeout() })
    }

    async fn open_one(
        &self,
        request: &PreparedHttpRequestV1<'_>,
    ) -> Result<OpenedByteStreamV1, StreamOpenErrorV1> {
        let headers = assemble_headers(request)?;
        let mut builder = self
            .client
            .request(request.method().clone(), request.url().clone())
            .headers(headers)
            .body(request_body(request.body().shared_owner()));
        if let Some(total) = self.total_timeout {
            builder = builder.timeout(total);
        }
        let response = builder.send().await.map_err(|error| classify_send_error(&error))?;

        if response.status().is_redirection() {
            return Err(TransportErrorV1::RedirectDenied.into());
        }
        let status = response.status();
        let content_type = response_metadata(
            response.headers(),
            header::CONTENT_TYPE,
            MAX_RESPONSE_CONTENT_TYPE_BYTES,
        )?;
        let retry_after = response_metadata(
            response.headers(),
            header::RETRY_AFTER,
            MAX_RESPONSE_RETRY_AFTER_BYTES,
        )?;
        let provider_quota_metadata = provider_quota_metadata(response.headers())?;
        let head = StreamingResponseHeadV1::try_from_parts_with_provider_quota_metadata(
            status,
            content_type,
            retry_after,
            provider_quota_metadata,
        )?;

        if status.is_success() {
            let source = ReqwestStreamSource { response, pending: Bytes::new() };
            Ok(OpenedByteStreamV1::try_new(head, Box::new(source))?)
        } else {
            let body = read_bounded_error_body(response).await;
            Err(StreamOpenErrorV1::Rejected(StreamRejectedV1::new(head, body)))
        }
    }
}

impl fmt::Debug for ReqwestStreamingTransportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ReqwestStreamingTransportV1").finish_non_exhaustive()
    }
}

impl AsyncStreamingTransport for ReqwestStreamingTransportV1 {
    fn open<'a>(&'a self, request: &'a PreparedHttpRequestV1<'_>) -> StreamingOpenFutureV1<'a> {
        Box::pin(async move { self.open_one(request).await })
    }
}

/// A buffered and streaming transport pair built from one timeout configuration.
///
/// Both adopting hosts build both transports from one timeout source; this constructor removes
/// that duplication (host-prelude D3). Process-singleton policy — whether construction failure is
/// memoized as a permanent fallback — stays host-side: the hosts have genuinely different rulings
/// there, which is the signal that `OnceLock` wiring is policy, not scaffolding.
pub struct TransportPairV1 {
    /// The buffered transport, configured verbatim from the supplied timeouts.
    pub buffered: ReqwestTransportV1,
    /// The streaming transport, derived from the same timeouts (see [`Self::try_new`]).
    pub streaming: ReqwestStreamingTransportV1,
}

impl TransportPairV1 {
    /// Builds both hardened transports from one buffered timeout configuration.
    ///
    /// The buffered transport takes the configuration verbatim. The streaming transport is the
    /// production streaming shape derived from it: no total bound (a long generation is
    /// legitimate wall-clock work; legal only because the idle guard is mandatory), the same
    /// connect timeout, and the read timeout as the idle guard. A host that needs a bounded
    /// streaming total constructs [`ReqwestStreamingTransportV1`] directly instead.
    pub fn try_new(config: ReqwestTransportConfigV1) -> Result<Self, TransportErrorV1> {
        let buffered = ReqwestTransportV1::new(config)?;
        let stream_config = StreamTransportConfigV1::try_new(
            None,
            config.connect_timeout(),
            config.read_timeout(),
        )?;
        let streaming = ReqwestStreamingTransportV1::new(stream_config)?;
        Ok(Self { buffered, streaming })
    }
}

impl fmt::Debug for TransportPairV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransportPairV1")
            .field("buffered", &self.buffered)
            .field("streaming", &self.streaming)
            .finish()
    }
}

/// A live reqwest body wrapped as a delivery-bounded pull source.
///
/// Oversized network reads are split at [`MAX_STREAM_CHUNK_BYTES`]; the remainder stays buffered
/// in `pending`, so dropping a pull future between chunks loses nothing. Dropping the source
/// drops the reqwest response and aborts the connection.
struct ReqwestStreamSource {
    response: reqwest::Response,
    pending: Bytes,
}

impl StreamByteSourceV1 for ReqwestStreamSource {
    fn next_chunk(&mut self) -> StreamChunkFutureV1<'_> {
        Box::pin(async move {
            while self.pending.is_empty() {
                match self.response.chunk().await {
                    Ok(Some(bytes)) => self.pending = bytes,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(classify_stream_read_error(&error))),
                }
            }
            let bounded = self.pending.split_to(self.pending.len().min(MAX_STREAM_CHUNK_BYTES));
            Some(StreamChunkV1::try_new(bounded))
        })
    }
}

/// Buffers a rejected exchange's error body up to [`MAX_STREAM_ERROR_BODY_BYTES`].
///
/// The body is best-effort classifier input: the head already carries the load-bearing status,
/// so a read failure or stall while draining the error body returns the bytes collected so far
/// instead of discarding the rejection.
async fn read_bounded_error_body(mut response: reqwest::Response) -> Vec<u8> {
    let mut body = Vec::new();
    while body.len() <= MAX_STREAM_ERROR_BODY_BYTES {
        match response.chunk().await {
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            Ok(None) | Err(_) => break,
        }
    }
    body.truncate(MAX_STREAM_ERROR_BODY_BYTES);
    body
}

fn classify_stream_read_error(error: &reqwest::Error) -> StreamReadErrorV1 {
    if error.is_timeout() {
        StreamReadErrorV1::StreamIdleTimeout
    } else {
        StreamReadErrorV1::StreamReadFailed
    }
}

fn effective_timeout(remaining_timeout: Duration, total_timeout: Duration) -> Duration {
    remaining_timeout.min(total_timeout)
}

struct JsonBodyOwner(Arc<str>);

impl AsRef<[u8]> for JsonBodyOwner {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

fn request_body(owner: Arc<str>) -> Bytes {
    Bytes::from_owner(JsonBodyOwner(owner))
}

fn assemble_headers(request: &PreparedHttpRequestV1<'_>) -> Result<HeaderMap, TransportErrorV1> {
    let mut headers = HeaderMap::with_capacity(request.headers().len() + 2);
    for (name, value) in request.headers().iter() {
        let name =
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| TransportErrorV1::RequestFailed)?;
        let value = HeaderValue::from_str(value).map_err(|_| TransportErrorV1::RequestFailed)?;
        headers.insert(name, value);
    }

    // The sanctioned user-agent declaration is the single source of this header: the ordinary
    // channel cannot carry the name (reserved) and the client sets no default, so inserting it
    // here is what puts it on the wire exactly once. The value grammar admits only printable
    // ASCII, so encoding it cannot fail; the error arm exists because `HeaderValue::from_str` is
    // fallible, not because an accepted value can reach it.
    if let Some(user_agent) = request.user_agent() {
        let value = HeaderValue::from_str(user_agent.as_str())
            .map_err(|_| TransportErrorV1::RequestFailed)?;
        headers.insert(HeaderName::from_static("user-agent"), value);
    }

    // The prepared request carries exactly one auth header: `authorization` with its `Bearer `
    // prefix, or one sanctioned secret header with the verbatim secret. Injecting only that pair
    // keeps `Authorization` off the wire for header-secret exchanges.
    let (auth_name, auth_value) = request.auth_header();
    let auth_name = HeaderName::from_bytes(auth_name.as_bytes())
        .map_err(|_| TransportErrorV1::RequestFailed)?;
    headers.insert(auth_name, auth_header_value(auth_value)?);
    Ok(headers)
}

/// A South-owned auth header allocation that zeroizes its plaintext bytes on drop.
///
/// Clones made by `Bytes` and `HeaderValue` share this owner. The guarantee ends at this
/// allocation and does not cover plaintext copies inside HTTP, TLS, operating-system, or provider
/// infrastructure buffers.
struct AuthHeaderOwner {
    value: Zeroizing<Vec<u8>>,
    #[cfg(test)]
    drop_probe: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl AuthHeaderOwner {
    fn new(value: &[u8]) -> Self {
        Self {
            value: Zeroizing::new(value.to_vec()),
            #[cfg(test)]
            drop_probe: None,
        }
    }

    #[cfg(test)]
    fn new_with_drop_probe(value: &[u8], drop_probe: Arc<std::sync::atomic::AtomicBool>) -> Self {
        let mut owner = Self::new(value);
        owner.drop_probe = Some(drop_probe);
        owner
    }
}

impl AsRef<[u8]> for AuthHeaderOwner {
    fn as_ref(&self) -> &[u8] {
        self.value.as_slice()
    }
}

#[cfg(test)]
impl Drop for AuthHeaderOwner {
    fn drop(&mut self) {
        if let Some(drop_probe) = &self.drop_probe {
            drop_probe.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

fn auth_header_value(value: &[u8]) -> Result<HeaderValue, TransportErrorV1> {
    auth_header_value_from_owner(AuthHeaderOwner::new(value))
}

fn auth_header_value_from_owner(owner: AuthHeaderOwner) -> Result<HeaderValue, TransportErrorV1> {
    let bytes = Bytes::from_owner(owner);
    let mut value =
        HeaderValue::from_maybe_shared(bytes).map_err(|_| TransportErrorV1::RequestFailed)?;
    value.set_sensitive(true);
    Ok(value)
}

fn response_metadata(
    headers: &HeaderMap,
    name: HeaderName,
    maximum_bytes: usize,
) -> Result<Option<String>, TransportErrorV1> {
    let values = headers.get_all(name);
    let mut values = values.iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() || value.as_bytes().len() > maximum_bytes {
        return Err(TransportErrorV1::ResponseMetadataInvalid);
    }
    value
        .to_str()
        .map(str::to_owned)
        .map(Some)
        .map_err(|_| TransportErrorV1::ResponseMetadataInvalid)
}

const PROVIDER_QUOTA_METADATA_FIELDS: [ProviderQuotaMetadataFieldV1; 9] = [
    ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens,
    ProviderQuotaMetadataFieldV1::XRateLimitRemainingTokens,
    ProviderQuotaMetadataFieldV1::XRateLimitResetTokens,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensLimit,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensRemaining,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensReset,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedLimit,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedRemaining,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedReset,
];

fn provider_quota_metadata(
    headers: &HeaderMap,
) -> Result<ProviderQuotaMetadataV1, TransportErrorV1> {
    ProviderQuotaMetadataV1::try_from_iter(
        PROVIDER_QUOTA_METADATA_FIELDS.into_iter().filter_map(|field| {
            optional_quota_metadata(headers, field).map(|value| (field, value))
        }),
    )
}

fn optional_quota_metadata(
    headers: &HeaderMap,
    field: ProviderQuotaMetadataFieldV1,
) -> Option<String> {
    let values = headers.get_all(field.as_header_name());
    let mut values = values.iter();
    let value = values.next()?;
    if values.next().is_some() || value.as_bytes().len() > MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES {
        return None;
    }
    value.to_str().ok().map(str::to_owned)
}

async fn read_bounded_body(mut response: reqwest::Response) -> Result<Vec<u8>, TransportErrorV1> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| classify_read_error(&error))? {
        let next_length =
            body.len().checked_add(chunk.len()).ok_or(TransportErrorV1::ResponseBodyTooLarge)?;
        if next_length > MAX_RESPONSE_BODY_BYTES {
            return Err(TransportErrorV1::ResponseBodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn classify_send_error(error: &reqwest::Error) -> TransportErrorV1 {
    if error.is_timeout() {
        TransportErrorV1::TransportTimeout
    } else if error.is_connect() {
        TransportErrorV1::ConnectFailed
    } else {
        TransportErrorV1::RequestFailed
    }
}

fn classify_read_error(error: &reqwest::Error) -> TransportErrorV1 {
    if error.is_timeout() {
        TransportErrorV1::TransportTimeout
    } else {
        TransportErrorV1::ResponseReadFailed
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use south_contracts::JsonBodyV1;

    use super::{
        AuthHeaderOwner, auth_header_value, auth_header_value_from_owner, effective_timeout,
        request_body,
    };

    #[test]
    fn auth_header_value_is_sensitive_and_redacted() {
        const SECRET_SENTINEL: &str = "sensitive-header-secret-sentinel";

        let header = auth_header_value(SECRET_SENTINEL.as_bytes())
            .expect("valid credential fixture should produce an auth header value");

        assert!(header.is_sensitive());
        assert!(!format!("{header:?}").contains(SECRET_SENTINEL));
    }

    #[test]
    fn auth_header_value_shares_and_drops_its_zeroizing_owner() {
        let dropped = Arc::new(AtomicBool::new(false));
        let owner = AuthHeaderOwner::new_with_drop_probe(
            b"owner-lifetime-secret-sentinel",
            Arc::clone(&dropped),
        );
        let owner_pointer = owner.as_ref().as_ptr();

        let header = auth_header_value_from_owner(owner)
            .expect("valid credential fixture should produce an auth header value");
        let header_clone = header.clone();

        assert_eq!(header.as_bytes().as_ptr(), owner_pointer);
        assert!(!dropped.load(Ordering::SeqCst));
        drop(header);
        assert!(!dropped.load(Ordering::SeqCst));
        drop(header_clone);
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn effective_timeout_uses_the_smaller_explicit_budget() {
        assert_eq!(
            effective_timeout(Duration::from_secs(3), Duration::from_secs(8)),
            Duration::from_secs(3)
        );
        assert_eq!(
            effective_timeout(Duration::from_secs(8), Duration::from_secs(3)),
            Duration::from_secs(3)
        );
        assert_eq!(
            effective_timeout(Duration::from_secs(5), Duration::from_secs(5)),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn request_body_keeps_the_shared_contract_allocation() {
        let contract_body = JsonBodyV1::parse(r#"{"body":"shared-owner-sentinel"}"#)
            .expect("fixture body should be valid");
        let original_pointer = contract_body.as_str().as_ptr();

        let body = request_body(contract_body.shared_owner());

        assert_eq!(body.as_ptr(), original_pointer);
        drop(contract_body);
        assert_eq!(body.as_ref(), br#"{"body":"shared-owner-sentinel"}"#);
    }
}
