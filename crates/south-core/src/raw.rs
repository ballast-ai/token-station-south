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
//!
//! The host-signed arm has its own raw type, [`RawSignedProviderCallV1`], and its own pair of
//! one-shot wrappers (design record: `docs/design/2026-09-08-host-prelude-signed-raw-call.md`).
//! It parses the same fields through the same grammars; only the declaration a host attaches
//! differs — a finalizer's emitted-header set in place of a credential scheme.
//!
//! The body-less GET (HTTP contract version six) has its own raw type too, [`RawGetProviderCallV1`]
//! — the raw call minus `body` — with one parse and one buffered one-shot wrapper (design record:
//! `docs/design/2026-09-08-buffered-get-request.md`). It is what a host's task poller hands over:
//! the same endpoint, slot, headers, and credential arm as the submit leg it follows.

use std::fmt;

use south_contracts::{
    BearerAuthV1, BufferedHttpResponseV1, ContractErrorV1, ControlledUserAgentV1, CredentialSlotV1,
    GetRequestV1, HeaderPolicyError, JsonBodyV1, JsonPostRequestV1, ProviderAuthV1,
    ProviderEndpointV1, QueryStringV1, RelativePathV1, SafeHeaders, SecretHeaderV1,
    SignedHeaderSetV1,
};
use thiserror::Error;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use crate::{
    AsyncHttpTransport, AsyncStreamingTransport, CredentialResolutionErrorV1,
    CredentialResolutionFuture, CredentialResolver, ProviderBindingV1, ProviderCallErrorV1,
    RequestFinalizerV1, SecretValue, StreamingCallV1, execute_get_call_v1,
    execute_provider_call_v1, execute_signed_provider_call_v1, open_streaming_provider_call_v1,
    open_streaming_signed_provider_call_v1,
};

/// The authentication arm of a raw provider call.
///
/// Mirrors [`ProviderAuthV1`]'s two frozen arms and adds no expressiveness — the raw type only
/// names the scheme; the credential slot travels separately as [`RawProviderCallV1`]'s
/// `requested_slot`.
///
/// `#[non_exhaustive]` from birth (host-prelude D2), so host `match`es must already carry a
/// fail-closed wildcard arm. The host-signed slice did not become the third arm D2 anticipated:
/// its declaration is a [`SignedHeaderSetV1`], which this `Copy`, lifetime-free enum cannot carry.
/// It became the sibling type [`RawSignedProviderCallV1`] instead (signed-raw-call record, D1).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawAuthV1 {
    /// The secret travels as `Authorization: Bearer …`.
    Bearer,
    /// The secret travels verbatim in one sanctioned provider-specific header.
    HeaderSecret(SecretHeaderV1),
    /// The secret travels both as `Authorization: Bearer …` and verbatim in the named sanctioned
    /// header (auth contract version four).
    BearerAndHeaderSecret(SecretHeaderV1),
}

impl RawAuthV1 {
    /// Attaches the parsed slot to this arm, producing the contract declaration.
    const fn declare(self, slot: BearerAuthV1) -> ProviderAuthV1 {
        match self {
            Self::Bearer => ProviderAuthV1::Bearer(slot),
            Self::HeaderSecret(header) => ProviderAuthV1::HeaderSecret { header, slot },
            Self::BearerAndHeaderSecret(header) => {
                ProviderAuthV1::BearerAndHeaderSecret { header, slot }
            }
        }
    }
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

/// A borrowed host-signed raw provider call: [`RawProviderCallV1`]'s field set with the
/// finalizer's declaration in place of a credential scheme.
///
/// The host-prelude record (D2) planned the host-signed slice as a third [`RawAuthV1`] arm. That
/// arm cannot carry what the host-signed request must declare: `RawAuthV1` is `Copy` and
/// lifetime-free, and [`SignedHeaderSetV1`] is neither. So the declaration travels where the
/// scheme would have, on a sibling type — the same fields, the same grammars, the same
/// [`RawCallErrorV1`] field names; only `emits` replaces `auth`. South still never resolves this
/// slot (host-signed D2): the host's [`RequestFinalizerV1`] owns the signing material, and the
/// slot only participates in the binding check.
pub struct RawSignedProviderCallV1<'a> {
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
    /// The headers the host's finalizer will emit — no more, no fewer. South diffs the
    /// finalizer's output against this set before anything reaches the transport.
    pub emits: &'a SignedHeaderSetV1,
    /// The sanctioned query declaration, when the call carries one.
    pub query: Option<QueryStringV1>,
    /// The sanctioned user-agent declaration, when the call carries one.
    pub user_agent: Option<ControlledUserAgentV1>,
}

impl fmt::Debug for RawSignedProviderCallV1<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawSignedProviderCallV1")
            .field("declared_header_count", &self.emits.len())
            .field("header_count", &self.headers.len())
            .field("body_byte_count", &self.body.len())
            .field("has_query", &self.query.is_some())
            .field("has_user_agent", &self.user_agent.is_some())
            .finish_non_exhaustive()
    }
}

/// A borrowed raw body-less GET: [`RawProviderCallV1`]'s field set minus `body`.
///
/// This is what a host's task poller already assembles for one poll — the endpoint, slot,
/// headers, and credential arm of the submit leg it follows, plus the provider-selected path
/// (which carries the task id for five of the six polling families) and, for the one family
/// that carries it as a query, a [`QueryStringV1`] declaring `task_id`. There is no body field
/// because there is no body slot: the contract type this parses to, [`GetRequestV1`], cannot
/// carry one, so a host cannot send a payload on a poll by mistake (buffered-GET record, D1).
///
/// Same grammars, same [`RawCallErrorV1`] field names; [`RawCallErrorV1::Body`] is never
/// produced here. The credential arm is the same [`RawAuthV1`] the POST shape uses. There is no
/// streaming twin (D2), and no host-signed twin until a consumer needs one.
pub struct RawGetProviderCallV1<'a> {
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
    /// The authentication arm selected by the host.
    pub auth: RawAuthV1,
    /// The sanctioned query declaration, when the call carries one.
    pub query: Option<QueryStringV1>,
    /// The sanctioned user-agent declaration, when the call carries one.
    pub user_agent: Option<ControlledUserAgentV1>,
}

impl fmt::Debug for RawGetProviderCallV1<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawGetProviderCallV1")
            .field("auth", &self.auth)
            .field("header_count", &self.headers.len())
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
    /// The `body` field failed contract validation. Never produced for a body-less GET, which
    /// has no such field.
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

    /// Returns the raw-call field name that failed. The names are shared by every raw shape
    /// ([`RawProviderCallV1`], [`RawSignedProviderCallV1`], [`RawGetProviderCallV1`]).
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
    let parts = parse_raw_parts(
        raw.endpoint,
        raw.relative_path,
        raw.bound_slot,
        raw.requested_slot,
        raw.headers,
        raw.body,
    )?;
    let auth = raw.auth.declare(BearerAuthV1::new(parts.requested_slot));
    let request = finish_request(
        JsonPostRequestV1::new(parts.relative_path, parts.headers, parts.body, auth),
        raw.query.clone(),
        raw.user_agent,
    );
    Ok((parts.binding, request))
}

/// Returns whether one raw call parses, for pre-admission checks.
///
/// Carries the same determinism guarantee as [`parse_raw_call`]: a `true` here means the
/// execution-time replay of the same inputs parses too.
#[must_use]
pub fn raw_call_parses(raw: &RawProviderCallV1<'_>) -> bool {
    parse_raw_call(raw).is_ok()
}

/// Parses one host-signed raw call into the binding and request the signed entry points consume.
///
/// Same determinism and zero-side-effect guarantees as [`parse_raw_call`], through the same
/// grammars; the request's auth is [`ProviderAuthV1::HostSigned`] carrying a clone of the
/// declaration. A host may pre-check with [`raw_signed_call_parses`].
pub fn parse_raw_signed_call(
    raw: &RawSignedProviderCallV1<'_>,
) -> Result<(ProviderBindingV1, JsonPostRequestV1), RawCallErrorV1> {
    let parts = parse_raw_parts(
        raw.endpoint,
        raw.relative_path,
        raw.bound_slot,
        raw.requested_slot,
        raw.headers,
        raw.body,
    )?;
    let auth = ProviderAuthV1::HostSigned {
        slot: BearerAuthV1::new(parts.requested_slot),
        emits: raw.emits.clone(),
    };
    let request = finish_request(
        JsonPostRequestV1::new(parts.relative_path, parts.headers, parts.body, auth),
        raw.query.clone(),
        raw.user_agent,
    );
    Ok((parts.binding, request))
}

/// Returns whether one host-signed raw call parses, for pre-admission checks.
///
/// Carries the same determinism guarantee as [`parse_raw_signed_call`].
#[must_use]
pub fn raw_signed_call_parses(raw: &RawSignedProviderCallV1<'_>) -> bool {
    parse_raw_signed_call(raw).is_ok()
}

/// Parses one raw body-less GET into the binding and request [`execute_get_call_v1`] consumes.
///
/// Same determinism and zero-side-effect guarantees as [`parse_raw_call`], through the same
/// grammars in the same order minus the body step. A host may pre-check with
/// [`raw_get_call_parses`].
pub fn parse_raw_get_call(
    raw: &RawGetProviderCallV1<'_>,
) -> Result<(ProviderBindingV1, GetRequestV1), RawCallErrorV1> {
    let parts =
        parse_raw_binding(raw.endpoint, raw.relative_path, raw.bound_slot, raw.requested_slot)?;
    let headers = parse_raw_headers(raw.headers)?;
    let auth = raw.auth.declare(BearerAuthV1::new(parts.requested_slot));
    let mut request = GetRequestV1::new(parts.relative_path, headers, auth);
    if let Some(query) = raw.query.clone() {
        request = request.with_query(query);
    }
    if let Some(user_agent) = raw.user_agent {
        request = request.with_user_agent(user_agent);
    }
    Ok((parts.binding, request))
}

/// Returns whether one raw body-less GET parses, for pre-admission checks.
///
/// Carries the same determinism guarantee as [`parse_raw_get_call`].
#[must_use]
pub fn raw_get_call_parses(raw: &RawGetProviderCallV1<'_>) -> bool {
    parse_raw_get_call(raw).is_ok()
}

/// The fields the two JSON POST shapes share, parsed through the contract grammars in one place.
struct ParsedRawParts {
    binding: ProviderBindingV1,
    requested_slot: CredentialSlotV1,
    relative_path: RelativePathV1,
    headers: SafeHeaders,
    body: JsonBodyV1,
}

/// The parse order is part of the prelude's observable behavior — a host that pre-checks sees
/// the first failing field — so the POST shapes keep it exactly: endpoint, bound slot, requested
/// slot, relative path, body, headers. The GET shape runs the same sequence with the body step
/// removed, through the same two helpers.
fn parse_raw_parts(
    endpoint: &str,
    relative_path: &str,
    bound_slot: &str,
    requested_slot: &str,
    headers: &[(String, String)],
    body: &str,
) -> Result<ParsedRawParts, RawCallErrorV1> {
    let parts = parse_raw_binding(endpoint, relative_path, bound_slot, requested_slot)?;
    let body = JsonBodyV1::parse(body).map_err(RawCallErrorV1::Body)?;
    let headers = parse_raw_headers(headers)?;
    Ok(ParsedRawParts {
        binding: parts.binding,
        requested_slot: parts.requested_slot,
        relative_path: parts.relative_path,
        headers,
        body,
    })
}

/// The fields every raw shape shares: where the call goes and which identity it binds.
struct ParsedRawBinding {
    binding: ProviderBindingV1,
    requested_slot: CredentialSlotV1,
    relative_path: RelativePathV1,
}

fn parse_raw_binding(
    endpoint: &str,
    relative_path: &str,
    bound_slot: &str,
    requested_slot: &str,
) -> Result<ParsedRawBinding, RawCallErrorV1> {
    let endpoint = ProviderEndpointV1::parse(endpoint).map_err(RawCallErrorV1::Endpoint)?;
    let bound_slot = CredentialSlotV1::parse(bound_slot).map_err(RawCallErrorV1::BoundSlot)?;
    let requested_slot =
        CredentialSlotV1::parse(requested_slot).map_err(RawCallErrorV1::RequestedSlot)?;
    let relative_path =
        RelativePathV1::parse(relative_path).map_err(RawCallErrorV1::RelativePath)?;
    Ok(ParsedRawBinding {
        binding: ProviderBindingV1::new(endpoint, bound_slot),
        requested_slot,
        relative_path,
    })
}

fn parse_raw_headers(headers: &[(String, String)]) -> Result<SafeHeaders, RawCallErrorV1> {
    SafeHeaders::try_from_iter(headers.iter().map(|(name, value)| (name.as_str(), value)))
        .map_err(RawCallErrorV1::Headers)
}

fn finish_request(
    mut request: JsonPostRequestV1,
    query: Option<QueryStringV1>,
    user_agent: Option<ControlledUserAgentV1>,
) -> JsonPostRequestV1 {
    if let Some(query) = query {
        request = request.with_query(query);
    }
    if let Some(user_agent) = user_agent {
        request = request.with_user_agent(user_agent);
    }
    request
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

/// Parses one raw body-less GET, then executes it as a buffered call.
///
/// The GET twin of [`execute_raw_call_v1`], with the same zero-side-effect parse invariant: a
/// parse failure returns before the resolver or transport is invoked. There is no streaming
/// twin (buffered-GET record, D2) — a poll is a bounded reply by nature.
pub async fn execute_get_raw_call_v1<R, T>(
    raw: &RawGetProviderCallV1<'_>,
    resolver: &R,
    transport: &T,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<BufferedHttpResponseV1, RawProviderCallErrorV1>
where
    R: CredentialResolver + ?Sized,
    T: AsyncHttpTransport + ?Sized,
{
    let (binding, request) = parse_raw_get_call(raw).map_err(RawProviderCallErrorV1::Parse)?;
    execute_get_call_v1(&binding, &request, resolver, transport, deadline, cancellation)
        .await
        .map_err(RawProviderCallErrorV1::Call)
}

/// Parses one host-signed raw call, then executes it as a buffered JSON POST.
///
/// The signed twin of [`execute_raw_call_v1`]: a finalizer takes the resolver's place, exactly as
/// [`execute_signed_provider_call_v1`] is the twin of `execute_provider_call_v1`. The parse
/// invariant is the same — a parse failure returns before the finalizer or the transport is
/// invoked — and so is everything after the parse: the finalizer runs once, inside the deadline
/// and cancellation scope, and its output is diffed against `emits` before any byte is sent.
pub async fn execute_signed_raw_call_v1<F, T>(
    raw: &RawSignedProviderCallV1<'_>,
    finalizer: &F,
    transport: &T,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<BufferedHttpResponseV1, RawProviderCallErrorV1>
where
    F: RequestFinalizerV1 + ?Sized,
    T: AsyncHttpTransport + ?Sized,
{
    let (binding, request) = parse_raw_signed_call(raw).map_err(RawProviderCallErrorV1::Parse)?;
    execute_signed_provider_call_v1(
        &binding,
        &request,
        finalizer,
        transport,
        deadline,
        cancellation,
    )
    .await
    .map_err(RawProviderCallErrorV1::Call)
}

/// Parses one host-signed raw call, then opens it as a streaming JSON POST.
///
/// Carries the same zero-side-effect parse invariant as [`execute_signed_raw_call_v1`].
pub async fn open_streaming_signed_raw_call_v1<F, T>(
    raw: &RawSignedProviderCallV1<'_>,
    finalizer: &F,
    transport: &T,
    deadline: Option<Instant>,
    cancellation: &CancellationToken,
) -> Result<StreamingCallV1, RawProviderCallErrorV1>
where
    F: RequestFinalizerV1 + ?Sized,
    T: AsyncStreamingTransport + ?Sized,
{
    let (binding, request) = parse_raw_signed_call(raw).map_err(RawProviderCallErrorV1::Parse)?;
    open_streaming_signed_provider_call_v1(
        &binding,
        &request,
        finalizer,
        transport,
        deadline,
        cancellation,
    )
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
