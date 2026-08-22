//! A deterministic host request finalizer for the host-signed auth arm.
//!
//! South ships no signing algorithm, so the thing under test is never "is this signature valid" —
//! it is "did South hand the signer the finished request, and does South hold the signer to its
//! declaration". Both need a signer that is *reproducible*, not one that is *strong*: the digest
//! below is a keyed FNV-1a over a canonical rendering of the whole view. It is deliberately not
//! cryptographic, and deliberately not `SigV4`. A test that depended on real AWS canonicalisation
//! would be testing AWS, and it would drag an AWS implementation into a crate whose entire point
//! is that South has none.
//!
//! What the digest *does* guarantee is coverage: every field of the view feeds it, so a South
//! change that stops showing the signer the body, the query, or the user agent changes the
//! signature and fails the fixtures.

use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

use south_contracts::{ControlledUserAgentV1, SignedHeaderV1};
use south_core::{
    FinalizeFutureV1, FinalizeViewV1, FinalizedHeadersV1, RequestFinalizationErrorV1,
    RequestFinalizerV1,
};

/// The test-only signing key. Never a real credential; never leaves this crate's fixtures.
pub const FAKE_SIGNING_KEY_V1: &str = "south-test-only-fake-signing-key-v1";

/// What a [`DeterministicRequestFinalizerV1`] does when South asks it to sign.
///
/// The five failure behaviours are the five ways a real signer breaks its contract. Each maps to
/// exactly one conformance case, and each must be rejected *before* the transport is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinalizerBehaviorV1 {
    /// Emit exactly the declared headers with reproducible values.
    Correct,
    /// Emit the declared headers plus one the declaration does not name.
    EmitsUndeclared,
    /// Omit one declared header.
    OmitsDeclared,
    /// Emit a declared header with an empty value.
    EmitsEmptyValue,
    /// Emit one declared header twice.
    EmitsDuplicate,
    /// Fail.
    Fails,
}

/// What the finalizer saw, recorded so a fixture can assert the view really was final.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedFinalizeViewV1 {
    /// The request method.
    pub method: String,
    /// The absolute URL, binding-resolved and carrying the sanctioned query.
    pub url: String,
    /// The ordinary headers, name and value, in declaration order.
    pub headers: Vec<(String, String)>,
    /// The exact body bytes.
    pub body: Vec<u8>,
    /// The sanctioned user agent, when the request declared one.
    pub user_agent: Option<String>,
    /// The credential slot naming the signing identity.
    pub slot: String,
    /// The headers the finalizer was required to emit.
    pub emits: Vec<SignedHeaderV1>,
}

/// A reproducible finalizer that records its view and can misbehave on demand.
pub struct DeterministicRequestFinalizerV1 {
    behavior: FinalizerBehaviorV1,
    calls: AtomicUsize,
    observed: Mutex<Option<ObservedFinalizeViewV1>>,
}

impl DeterministicRequestFinalizerV1 {
    /// Creates a finalizer with the given behaviour.
    #[must_use]
    pub const fn new(behavior: FinalizerBehaviorV1) -> Self {
        Self { behavior, calls: AtomicUsize::new(0), observed: Mutex::new(None) }
    }

    /// Creates a well-behaved finalizer.
    #[must_use]
    pub const fn correct() -> Self {
        Self::new(FinalizerBehaviorV1::Correct)
    }

    /// Returns how many times South asked this finalizer to sign.
    #[must_use]
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// Returns the view of the most recent signing request.
    #[must_use]
    pub fn observed(&self) -> Option<ObservedFinalizeViewV1> {
        self.observed.lock().map_or(None, |observed| observed.clone())
    }

    /// Returns the value this finalizer produces for one header of one view.
    ///
    /// Public so a fixture can state the expected wire bytes independently of the code that
    /// produced them: an assertion that recomputes the value the same way the subject did would
    /// pass for any subject.
    #[must_use]
    pub fn expected_value(canonical: &str, header: SignedHeaderV1) -> String {
        format!("{}={:016x}", header.header_name(), keyed_digest(canonical, header))
    }

    /// Renders a view the way this finalizer signs it.
    #[must_use]
    pub fn canonical_rendering(view: &ObservedFinalizeViewV1) -> String {
        let mut canonical = String::new();
        canonical.push_str(&view.method);
        canonical.push('\n');
        canonical.push_str(&view.url);
        canonical.push('\n');
        for (name, value) in &view.headers {
            canonical.push_str(name);
            canonical.push(':');
            canonical.push_str(value);
            canonical.push('\n');
        }
        canonical.push_str(view.user_agent.as_deref().unwrap_or("-"));
        canonical.push('\n');
        canonical.push_str(&view.slot);
        canonical.push('\n');
        canonical.push_str(&String::from_utf8_lossy(&view.body));
        canonical
    }
}

fn observe(view: &FinalizeViewV1<'_>) -> ObservedFinalizeViewV1 {
    ObservedFinalizeViewV1 {
        method: view.method().to_string(),
        url: view.url().to_string(),
        headers: view
            .headers()
            .iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect(),
        body: view.body().to_vec(),
        user_agent: view.user_agent().map(|agent: ControlledUserAgentV1| agent.as_str().to_owned()),
        slot: view.slot().as_str().to_owned(),
        emits: view.emits().headers().to_vec(),
    }
}

/// A keyed FNV-1a over the canonical rendering and the header name.
///
/// Per-header keying means two headers of one request never share a value, so a fixture that
/// asserts byte-equality catches a South change that binds the right value to the wrong name.
fn keyed_digest(canonical: &str, header: SignedHeaderV1) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in FAKE_SIGNING_KEY_V1
        .as_bytes()
        .iter()
        .chain(header.header_name().as_bytes())
        .chain(canonical.as_bytes())
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

impl RequestFinalizerV1 for DeterministicRequestFinalizerV1 {
    fn finalize<'a>(&'a self, view: FinalizeViewV1<'a>) -> FinalizeFutureV1<'a> {
        let observed = observe(&view);
        let declared = observed.emits.clone();
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut slot) = self.observed.lock() {
            *slot = Some(observed.clone());
        }
        let behavior = self.behavior;
        Box::pin(async move {
            if behavior == FinalizerBehaviorV1::Fails {
                return Err(RequestFinalizationErrorV1);
            }
            let canonical = Self::canonical_rendering(&observed);
            let mut headers = FinalizedHeadersV1::new();
            let emitted: &[SignedHeaderV1] = match behavior {
                FinalizerBehaviorV1::OmitsDeclared => &declared[..declared.len() - 1],
                _ => &declared,
            };
            for header in emitted {
                let value = if behavior == FinalizerBehaviorV1::EmitsEmptyValue {
                    String::new()
                } else {
                    Self::expected_value(&canonical, *header)
                };
                headers.insert(*header, value.into_bytes());
            }
            match behavior {
                FinalizerBehaviorV1::EmitsUndeclared => {
                    // Whichever permitted header the declaration did not name; the fixtures always
                    // leave at least one out so this cannot silently become a no-op.
                    if let Some(extra) =
                        SignedHeaderV1::ALL.into_iter().find(|header| !declared.contains(header))
                    {
                        headers.insert(extra, b"undeclared".to_vec());
                    }
                }
                FinalizerBehaviorV1::EmitsDuplicate => {
                    if let Some(first) = declared.first() {
                        headers.insert(*first, b"duplicate".to_vec());
                    }
                }
                _ => {}
            }
            Ok(headers)
        })
    }
}

/// A finalizer that never returns, for cancellation and deadline fixtures.
pub struct HangingRequestFinalizerV1 {
    calls: AtomicUsize,
}

impl HangingRequestFinalizerV1 {
    /// Creates a finalizer whose future is never ready.
    #[must_use]
    pub const fn new() -> Self {
        Self { calls: AtomicUsize::new(0) }
    }

    /// Returns how many times South asked this finalizer to sign.
    #[must_use]
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Default for HangingRequestFinalizerV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestFinalizerV1 for HangingRequestFinalizerV1 {
    fn finalize<'a>(&'a self, _view: FinalizeViewV1<'a>) -> FinalizeFutureV1<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(std::future::pending())
    }
}

/// Recomputes the value the reference finalizer must have emitted for one header.
///
/// Fixtures call this with the view they independently expected, so the assertion is "the wire
/// carries the signature of *the request I expected South to build*", not "the wire carries
/// whatever the signer produced".
#[must_use]
pub fn expected_signature_v1(view: &ObservedFinalizeViewV1, header: SignedHeaderV1) -> String {
    DeterministicRequestFinalizerV1::expected_value(
        &DeterministicRequestFinalizerV1::canonical_rendering(view),
        header,
    )
}
