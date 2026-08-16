#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Asynchronous provider transport boundary for reqwest hosts.

use std::{fmt, time::Duration};

use http::{HeaderMap, HeaderName, HeaderValue, header};
use south_contracts::{
    BufferedHttpResponseV1, MAX_RESPONSE_BODY_BYTES, MAX_RESPONSE_CONTENT_TYPE_BYTES,
    MAX_RESPONSE_RETRY_AFTER_BYTES, TransportErrorV1,
};
use south_core::{AsyncHttpTransport, PreparedHttpRequestV1, TransportFuture};
use zeroize::Zeroizing;

/// The largest transport-owned timeout accepted by the buffered call contract.
pub const MAX_TRANSPORT_TIMEOUT: Duration = Duration::from_hours(24);

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
            .body(request.body().as_str().to_owned())
            .timeout(remaining_timeout.min(self.total_timeout))
            .send()
            .await
            .map_err(|error| classify_send_error(&error))?;

        if response.status().is_redirection() {
            return Err(TransportErrorV1::RedirectDenied);
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
        let body = read_bounded_body(response).await?;

        BufferedHttpResponseV1::try_from_parts(status, body, content_type, retry_after)
    }
}

fn assemble_headers(request: &PreparedHttpRequestV1<'_>) -> Result<HeaderMap, TransportErrorV1> {
    let mut headers = HeaderMap::with_capacity(request.headers().len() + 1);
    for (name, value) in request.headers().iter() {
        let name =
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| TransportErrorV1::RequestFailed)?;
        let value = HeaderValue::from_str(value).map_err(|_| TransportErrorV1::RequestFailed)?;
        headers.insert(name, value);
    }

    let authorization = authorization_header(request.bearer_secret())?;
    headers.insert(header::AUTHORIZATION, authorization);
    Ok(headers)
}

fn authorization_header(secret: &str) -> Result<HeaderValue, TransportErrorV1> {
    let mut bearer = Zeroizing::new(String::with_capacity("Bearer ".len() + secret.len()));
    bearer.push_str("Bearer ");
    bearer.push_str(secret);
    let mut authorization =
        HeaderValue::from_str(&bearer).map_err(|_| TransportErrorV1::RequestFailed)?;
    authorization.set_sensitive(true);
    Ok(authorization)
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
    use super::authorization_header;

    #[test]
    fn bearer_header_is_sensitive_and_redacted() {
        const SECRET_SENTINEL: &str = "sensitive-header-secret-sentinel";

        let header = authorization_header(SECRET_SENTINEL)
            .expect("valid credential fixture should produce an authorization header");

        assert!(header.is_sensitive());
        assert!(!format!("{header:?}").contains(SECRET_SENTINEL));
    }
}
