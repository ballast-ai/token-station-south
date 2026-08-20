//! Shared raw-call scaffolding for adopting hosts (the host prelude).
//!
//! Both adopting hosts independently wrote the same South-consumption skeleton: a string-in
//! contract parse to `(ProviderBindingV1, JsonPostRequestV1)`, parse→execute and
//! parse→open-streaming wrappers, and credential resolver adapters. This module is that skeleton,
//! made host-neutral (design record: `docs/design/2026-08-20-host-prelude.md`, D1–D5 ruled
//! 2026-08-20).
//!
//! The boundary claim is deliberate: everything here is convenience-layer orchestration over the
//! existing contracts. Eligibility and scope decisions, dynamic auth material (minting, OAuth
//! refresh, JWT signing), settlement semantics, and the numeric value of any bound stay
//! host-owned. No new parsing grammar is introduced — every grammar stays in `south-contracts`
//! under its existing fuzz obligations.

use std::fmt;

use south_contracts::{
    BearerAuthV1, BufferedHttpResponseV1, ContractErrorV1, ControlledUserAgentV1, CredentialSlotV1,
    HeaderPolicyError, JsonBodyV1, JsonPostRequestV1, ProviderAuthV1, ProviderEndpointV1,
    QueryStringV1, RelativePathV1, SafeHeaders, SecretHeaderV1,
};
use thiserror::Error;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use crate::{
    AsyncHttpTransport, AsyncStreamingTransport, CredentialResolutionErrorV1,
    CredentialResolutionFuture, CredentialResolver, ProviderBindingV1, ProviderCallErrorV1,
    SecretValue, StreamingCallV1, execute_provider_call_v1, open_streaming_provider_call_v1,
};

/// The authentication arm of a raw provider call.
///
/// Mirrors [`ProviderAuthV1`]'s two frozen arms and adds no expressiveness — the raw type only
/// names the scheme; the credential slot travels separately as [`RawProviderCallV1`]'s
/// `requested_slot`.
///
/// `#[non_exhaustive]` from birth (host-prelude D2): the host-signed slice adds a third arm, and
/// host `match`es must already carry a fail-closed wildcard arm.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawAuthV1 {
    /// The secret travels as `Authorization: Bearer …`.
    Bearer,
    /// The secret travels verbatim in one sanctioned provider-specific header.
    HeaderSecret(SecretHeaderV1),
}

/// A borrowed raw provider call carrying exactly what both hosts already assemble.
///
/// `query` and `user_agent` take the already-parsed contract types: which parameters and values
/// are sanctioned is a contracts question, and how a host obtains the raw strings (config,
/// catalog row, upstream URL) is a host question; neither belongs to this layer. URL splitting
/// likewise stays host-side.
pub struct RawProviderCallV1<'a> {
    /// The trusted base endpoint, unparsed.
    pub endpoint: &'a str,
    /// The provider-selected relative path, unparsed and query-free.
    pub relative_path: &'a str,
    /// The host-binding-side credential slot, unparsed.
    pub bound_slot: &'a str,
    /// The request-declaration-side credential slot, unparsed. Production paths keep the two
    /// slots equal; a mismatch surfaces as `CREDENTIAL_BINDING_MISMATCH` at execution time.
    pub requested_slot: &'a str,
    /// Ordinary request headers, validated against the header policy during parse.
    pub headers: &'a [(String, String)],
    /// The JSON request body, unparsed.
    pub body: &'a str,
    /// The authentication arm selected by the host.
    pub auth: RawAuthV1,
    /// The sanctioned query declaration, when the call carries one.
    pub query: Option<QueryStringV1>,
    /// The sanctioned user-agent declaration, when the call carries one.
    pub user_agent: Option<ControlledUserAgentV1>,
}

impl fmt::Debug for RawProviderCallV1<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawProviderCallV1")
            .field("auth", &self.auth)
            .field("header_count", &self.headers.len())
            .field("body_byte_count", &self.body.len())
            .field("has_query", &self.query.is_some())
            .field("has_user_agent", &self.user_agent.is_some())
            .finish_non_exhaustive()
    }
}

/// A raw-call contract validation failure, naming the field that failed.
///
/// This type aggregates the existing contract and header-policy errors; it introduces no new
/// parsing grammar and no new stable codes — [`Self::code`] is always the wrapped error's code.
///
/// `#[non_exhaustive]` from birth: future auth arms may parse new fields.
#[non_exhaustive]
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RawCallErrorV1 {
    /// The `endpoint` field failed contract validation.
    #[error("endpoint failed contract validation")]
    Endpoint(ContractErrorV1),
    /// The `bound_slot` field failed contract validation.
    #[error("bound credential slot failed contract validation")]
    BoundSlot(ContractErrorV1),
    /// The `requested_slot` field failed contract validation.
    #[error("requested credential slot failed contract validation")]
    RequestedSlot(ContractErrorV1),
    /// The `relative_path` field failed contract validation.
    #[error("relative path failed contract validation")]
    RelativePath(ContractErrorV1),
    /// The `body` field failed contract validation.
    #[error("request body failed contract validation")]
    Body(ContractErrorV1),
    /// The `headers` field violated the header policy.
    #[error("request headers violated the header policy")]
    Headers(HeaderPolicyError),
}

impl RawCallErrorV1 {
    /// Returns the stable machine-readable code of the wrapped contract or policy error.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Endpoint(error)
            | Self::BoundSlot(error)
            | Self::RequestedSlot(error)
            | Self::RelativePath(error)
            | Self::Body(error) => error.code(),
            Self::Headers(error) => error.code(),
        }
    }

    /// Returns the [`RawProviderCallV1`] field name that failed.
    #[must_use]
    pub const fn field(&self) -> &'static str {
        match self {
            Self::Endpoint(_) => "endpoint",
            Self::BoundSlot(_) => "bound_slot",
            Self::RequestedSlot(_) => "requested_slot",
            Self::RelativePath(_) => "relative_path",
            Self::Body(_) => "body",
            Self::Headers(_) => "headers",
        }
    }
}

/// Parses one raw call into the binding and request the orchestration entry points consume.
///
/// Deterministic: the same inputs parse identically at admission time and at execution time, so a
/// host may pre-check with [`raw_call_parses`] and rely on the execution-time replay agreeing.
/// Parsing performs no I/O and has no side effects.
pub fn parse_raw_call(
    raw: &RawProviderCallV1<'_>,
) -> Result<(ProviderBindingV1, JsonPostRequestV1), RawCallErrorV1> {
    let endpoint = ProviderEndpointV1::parse(raw.endpoint).map_err(RawCallErrorV1::Endpoint)?;
    let bound_slot = CredentialSlotV1::parse(raw.bound_slot).map_err(RawCallErrorV1::BoundSlot)?;
    let requested_slot =
        CredentialSlotV1::parse(raw.requested_slot).map_err(RawCallErrorV1::RequestedSlot)?;
    let relative_path =
        RelativePathV1::parse(raw.relative_path).map_err(RawCallErrorV1::RelativePath)?;
    let body = JsonBodyV1::parse(raw.body).map_err(RawCallErrorV1::Body)?;
    let headers =
        SafeHeaders::try_from_iter(raw.headers.iter().map(|(name, value)| (name.as_str(), value)))
            .map_err(RawCallErrorV1::Headers)?;

    let binding = ProviderBindingV1::new(endpoint, bound_slot);
    let slot = BearerAuthV1::new(requested_slot);
    let auth = match raw.auth {
        RawAuthV1::Bearer => ProviderAuthV1::Bearer(slot),
        RawAuthV1::HeaderSecret(header) => ProviderAuthV1::HeaderSecret { header, slot },
    };
    let mut request = JsonPostRequestV1::new(relative_path, headers, body, auth);
    if let Some(query) = raw.query.clone() {
        request = request.with_query(query);
    }
    if let Some(user_agent) = raw.user_agent {
        request = request.with_user_agent(user_agent);
    }
    Ok((binding, request))
}

/// Returns whether one raw call parses, for pre-admission checks.
///
/// Carries the same determinism guarantee as [`parse_raw_call`]: a `true` here means the
/// execution-time replay of the same inputs parses too.
#[must_use]
pub fn raw_call_parses(raw: &RawProviderCallV1<'_>) -> bool {
    parse_raw_call(raw).is_ok()
}

/// A raw one-shot wrapper failure: either the parse phase or the orchestrated call.
///
/// `#[non_exhaustive]` from birth, for the same reason as [`RawCallErrorV1`].
#[non_exhaustive]
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RawProviderCallErrorV1 {
    /// Contract parsing failed; the resolver and transport were never invoked.
    #[error(transparent)]
    Parse(RawCallErrorV1),
    /// The orchestrated provider call failed after a successful parse.
    #[error(transparent)]
    Call(ProviderCallErrorV1),
}

impl RawProviderCallErrorV1 {
    /// Returns the stable code owned by the wrapped parse or call error.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Parse(error) => error.code(),
            Self::Call(error) => error.code(),
        }
    }
}

/// Parses one raw call, then executes it as a buffered JSON POST.
///
/// Invariant: a parse failure returns before the resolver or transport is invoked — zero side
/// effects, so a host may treat it as a clean fallback signal.
pub async fn execute_raw_call_v1<R, T>(
    raw: &RawProviderCallV1<'_>,
    resolver: &R,
    transport: &T,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<BufferedHttpResponseV1, RawProviderCallErrorV1>
where
    R: CredentialResolver + ?Sized,
    T: AsyncHttpTransport + ?Sized,
{
    let (binding, request) = parse_raw_call(raw).map_err(RawProviderCallErrorV1::Parse)?;
    execute_provider_call_v1(&binding, &request, resolver, transport, deadline, cancellation)
        .await
        .map_err(RawProviderCallErrorV1::Call)
}

/// Parses one raw call, then opens it as a streaming JSON POST.
///
/// Carries the same zero-side-effect parse invariant as [`execute_raw_call_v1`].
pub async fn open_streaming_raw_call_v1<R, T>(
    raw: &RawProviderCallV1<'_>,
    resolver: &R,
    transport: &T,
    deadline: Option<Instant>,
    cancellation: &CancellationToken,
) -> Result<StreamingCallV1, RawProviderCallErrorV1>
where
    R: CredentialResolver + ?Sized,
    T: AsyncStreamingTransport + ?Sized,
{
    let (binding, request) = parse_raw_call(raw).map_err(RawProviderCallErrorV1::Parse)?;
    open_streaming_provider_call_v1(&binding, &request, resolver, transport, deadline, cancellation)
        .await
        .map_err(RawProviderCallErrorV1::Call)
}

/// A resolver holding one pre-resolved secret in a South-owned zeroizing allocation.
///
/// This is the fund-invariant pattern made host-neutral: all fallible dynamic-auth work
/// (minting, OAuth refresh, JWT signing) happens before construction, so resolution after a
/// host's commit point never fails. [`CredentialResolver::resolve`] may be called repeatedly and
/// always yields the same secret.
pub struct PreparedSecretResolverV1 {
    secret: Zeroizing<String>,
    expected_slot: Option<CredentialSlotV1>,
}

impl PreparedSecretResolverV1 {
    /// Takes ownership of one already-resolved secret.
    #[must_use]
    pub fn new(secret: String) -> Self {
        Self { secret: Zeroizing::new(secret), expected_slot: None }
    }

    /// Adds an optional slot check: resolution for any other slot fails.
    #[must_use]
    pub fn expecting_slot(mut self, slot: CredentialSlotV1) -> Self {
        self.expected_slot = Some(slot);
        self
    }
}

impl fmt::Debug for PreparedSecretResolverV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedSecretResolverV1")
            .field("secret", &"[REDACTED]")
            .field("has_expected_slot", &self.expected_slot.is_some())
            .finish()
    }
}

impl CredentialResolver for PreparedSecretResolverV1 {
    fn resolve<'a>(&'a self, slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        Box::pin(async move {
            if let Some(expected) = &self.expected_slot
                && expected != slot
            {
                return Err(CredentialResolutionErrorV1);
            }
            Ok(SecretValue::new(self.secret.as_str().to_owned()))
        })
    }
}

/// A resolver adapter rejecting secrets larger than a host-supplied byte cap.
///
/// v1 has no credential-value size contract; hosts must bound it. This adapter turns that
/// footnote into a mechanism — the number stays a host parameter. An oversized secret maps to
/// the same opaque [`CredentialResolutionErrorV1`] as any other resolution failure; the
/// oversized allocation is dropped (and therefore zeroized) here.
pub struct BoundedResolverV1<R> {
    inner: R,
    max_secret_bytes: usize,
}

impl<R> BoundedResolverV1<R> {
    /// Wraps a resolver with a host-chosen secret byte cap.
    #[must_use]
    pub const fn new(inner: R, max_secret_bytes: usize) -> Self {
        Self { inner, max_secret_bytes }
    }
}

impl<R> fmt::Debug for BoundedResolverV1<R> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundedResolverV1")
            .field("max_secret_bytes", &self.max_secret_bytes)
            .finish_non_exhaustive()
    }
}

impl<R: CredentialResolver> CredentialResolver for BoundedResolverV1<R> {
    fn resolve<'a>(&'a self, slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        Box::pin(async move {
            let secret = self.inner.resolve(slot).await?;
            if secret.value.len() > self.max_secret_bytes {
                return Err(CredentialResolutionErrorV1);
            }
            Ok(secret)
        })
    }
}
