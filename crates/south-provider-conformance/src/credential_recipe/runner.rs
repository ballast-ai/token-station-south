//! The host harness and the runner of the credential-recipe suite.

use std::{
    collections::BTreeMap,
    fmt,
    future::{Future, poll_fn},
    pin::Pin,
    task::Poll,
    time::{SystemTime, UNIX_EPOCH},
};

use south_provider_api::{CredentialsV1, EncodingV1};

use super::{
    CREDENTIAL_RECIPE_CONFORMANCE_SUITE_ID, CREDENTIAL_RECIPE_CONFORMANCE_SUITE_VERSION,
    CREDENTIAL_RECIPE_EXPIRY_TOLERANCE_SECONDS_V1, CREDENTIAL_RECIPE_FAKE_TOKEN_ENDPOINT_V1,
    CREDENTIAL_RECIPE_SLOT_V1, CredentialGenerationFixtureV1, CredentialRecipeCaseIdV1,
    CredentialRecipeFixtureV1, CredentialRecipeKindV1, CredentialRecipeStepV1,
    CredentialResolveExpectedV1, CredentialStoredExpectedV1, FakeTokenEndpointV1,
    credential_recipe_fixtures_v1,
};

/// A boxed future returned by the harness. `Send`, so a host may drive it on any runtime.
pub type CredentialRecipeFutureV1<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The host refused to open a case, e.g. because it does not admit the recipe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CredentialRecipeOpenRefusedV1;

/// What a host implements to run the suite: its own generic recipe executor and credential store,
/// assembled for a test.
pub trait CredentialRecipeHarnessV1: Send + Sync {
    /// Opens one case: a fresh, empty store holding one credential of the kind `credentials`
    /// declares, seeded with `seed` as its first generation, and the host's generic recipe executor
    /// over it with its token egress injected to `endpoint`.
    ///
    /// The host treats every endpoint of `credentials` as operator-confirmed for the case (§3.4
    /// rule 1) and the package as first party (§3.4 rule 5): those gates are not under test here.
    /// The injection must exist only in test builds (§3.4 rule 6).
    fn open<'a>(
        &'a self,
        credentials: &'a CredentialsV1,
        seed: &'a CredentialGenerationFixtureV1,
        endpoint: FakeTokenEndpointV1,
    ) -> CredentialRecipeFutureV1<
        'a,
        Result<Box<dyn CredentialRecipeSessionV1 + 'a>, CredentialRecipeOpenRefusedV1>,
    >;
}

/// One open case. Each method goes through the same code the host runs in production.
pub trait CredentialRecipeSessionV1: Send + Sync {
    /// Resolves `slot` exactly as an inference request would: the stored value when it is fresh,
    /// otherwise the recipe runs under the host's lock, re-read, compare-and-swap and loser re-read.
    /// Called twice concurrently by one case; the two calls must not be serialised by the harness
    /// itself.
    fn resolve<'a>(
        &'a self,
        slot: &'a str,
    ) -> CredentialRecipeFutureV1<'a, CredentialResolveObservationV1>;

    /// Runs the health probe the host runs on a credential of this kind.
    fn probe(&self) -> CredentialRecipeFutureV1<'_, ()>;

    /// Commits `generation` out of band, as an operator edit or another replica's winning
    /// write-back would: it replaces the fields and the minted value (none when absent), bumps the
    /// version the compare-and-swap compares, and does **not** wait for the refresh lock — the
    /// suite calls it while a refresh holds that lock.
    ///
    /// It must produce a **new generation even when the values equal the stored ones**: case
    /// `ExchangeFailureWritesNothing` rewrites its seed unchanged, and the cases read every write as
    /// the operator acting, which clears a `reauth_required` latch. A store that bumps its
    /// generation only when a value changes (a trigger comparing old and new values, for example)
    /// commits this write through a path that forces the bump. The runner cannot observe a
    /// generation, so a harness that skips the bump makes the suite's verdicts unsound without
    /// failing it (§13.5 D5, host feedback SF3).
    fn write_generation<'a>(
        &'a self,
        generation: &'a CredentialGenerationFixtureV1,
    ) -> CredentialRecipeFutureV1<'a, ()>;

    /// Reads the stored state from the authoritative store.
    fn stored(&self) -> CredentialRecipeFutureV1<'_, StoredCredentialV1>;
}

/// The result of resolving a slot, as the host classified it.
#[derive(Clone, PartialEq, Eq)]
pub enum CredentialResolveObservationV1 {
    /// The slot resolved to this value.
    Minted(String),
    /// The credential is unusable until it changes.
    ReauthRequired,
    /// A failure that may succeed later.
    Transient,
    /// Any other failure, e.g. a configuration error.
    OtherFailure,
}

impl fmt::Debug for CredentialResolveObservationV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Minted(value) => {
                formatter.debug_struct("Minted").field("value_byte_count", &value.len()).finish()
            }
            Self::ReauthRequired => formatter.write_str("ReauthRequired"),
            Self::Transient => formatter.write_str("Transient"),
            Self::OtherFailure => formatter.write_str("OtherFailure"),
        }
    }
}

/// The stored state of the case's credential, read from the authoritative store.
#[derive(Clone, PartialEq, Eq)]
pub struct StoredCredentialV1 {
    fields: BTreeMap<String, String>,
    previous_fields: BTreeMap<String, String>,
    minted: Option<(String, i64)>,
}

impl StoredCredentialV1 {
    /// `fields` holds every stored field value (an emptied field is reported as `""`, a removed
    /// one is absent). `previous_fields` holds the values the last rotation replaced, which the
    /// host keeps so that a wrong rotation can be rolled back (§3.5); empty when none was kept.
    /// `minted` is the stored value of the minted slot and its expiry in epoch **seconds**.
    #[must_use]
    pub const fn new(
        fields: BTreeMap<String, String>,
        previous_fields: BTreeMap<String, String>,
        minted: Option<(String, i64)>,
    ) -> Self {
        Self { fields, previous_fields, minted }
    }

    /// The stored field values.
    #[must_use]
    pub const fn fields(&self) -> &BTreeMap<String, String> {
        &self.fields
    }

    /// The previous generation's replaced values.
    #[must_use]
    pub const fn previous_fields(&self) -> &BTreeMap<String, String> {
        &self.previous_fields
    }

    /// The stored minted value and its expiry in epoch seconds.
    #[must_use]
    pub fn minted(&self) -> Option<(&str, i64)> {
        self.minted.as_ref().map(|(value, expires_at)| (value.as_str(), *expires_at))
    }
}

impl fmt::Debug for StoredCredentialV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoredCredentialV1")
            .field("field_names", &self.fields.keys().collect::<Vec<_>>())
            .field("previous_field_names", &self.previous_fields.keys().collect::<Vec<_>>())
            .field("minted_present", &self.minted.is_some())
            .finish()
    }
}

/// The closed reasons why a case can fail.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CredentialRecipeMismatchCategoryV1 {
    /// The host refused to open the case.
    SessionOpen,
    /// A resolve gave another result than expected (kind or minted value).
    Resolve,
    /// The endpoint had received a different number of exchanges at an `ExpectEndpointCalls` step.
    EndpointCalls,
    /// The recorded exchanges differ from the expected ones: count, method, URL, or which stored
    /// generation they carried.
    Exchange,
    /// A competing-write step's refresh finished without its exchange being held at the endpoint.
    CompetingWriteNotReached,
    /// The stored fields differ: one is missing, extra, emptied or holds another value.
    StoredFields,
    /// The stored minted value differs or is absent.
    StoredMinted,
    /// The stored expiry is absent or outside the tolerance of the expected one.
    StoredExpiry,
    /// The kept previous generation differs.
    PreviousGeneration,
}

fixed_debug!(CredentialRecipeMismatchCategoryV1 {
    SessionOpen => "SessionOpen",
    Resolve => "Resolve",
    EndpointCalls => "EndpointCalls",
    Exchange => "Exchange",
    CompetingWriteNotReached => "CompetingWriteNotReached",
    StoredFields => "StoredFields",
    StoredMinted => "StoredMinted",
    StoredExpiry => "StoredExpiry",
    PreviousGeneration => "PreviousGeneration",
});

/// One mismatch, without expected or observed values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CredentialRecipeMismatchV1 {
    case_id: CredentialRecipeCaseIdV1,
    step: Option<usize>,
    category: CredentialRecipeMismatchCategoryV1,
}

impl CredentialRecipeMismatchV1 {
    /// The case that mismatched.
    #[must_use]
    pub const fn case_id(&self) -> CredentialRecipeCaseIdV1 {
        self.case_id
    }

    /// The index of the script step that mismatched; `None` for the case-level exchange check and
    /// for opening the case.
    #[must_use]
    pub const fn step(&self) -> Option<usize> {
        self.step
    }

    /// The closed mismatch category.
    #[must_use]
    pub const fn category(&self) -> CredentialRecipeMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for CredentialRecipeMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialRecipeMismatchV1")
            .field("case_id", &self.case_id)
            .field("step", &self.step)
            .field("category", &self.category)
            .finish()
    }
}

/// A successful report for the complete suite.
pub struct CredentialRecipeConformanceReportV1 {
    passed_case_ids: Vec<CredentialRecipeCaseIdV1>,
}

impl CredentialRecipeConformanceReportV1 {
    /// The stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        CREDENTIAL_RECIPE_CONFORMANCE_SUITE_ID
    }

    /// The suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        CREDENTIAL_RECIPE_CONFORMANCE_SUITE_VERSION
    }

    /// Every passed case, in table order.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[CredentialRecipeCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for CredentialRecipeConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialRecipeConformanceReportV1")
            .field("suite_id", &CREDENTIAL_RECIPE_CONFORMANCE_SUITE_ID)
            .field("suite_version", &CREDENTIAL_RECIPE_CONFORMANCE_SUITE_VERSION)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// Every mismatch of an evaluated suite.
pub struct CredentialRecipeConformanceFailureV1 {
    evaluated_case_count: usize,
    mismatches: Vec<CredentialRecipeMismatchV1>,
}

impl CredentialRecipeConformanceFailureV1 {
    /// The stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        CREDENTIAL_RECIPE_CONFORMANCE_SUITE_ID
    }

    /// The suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        CREDENTIAL_RECIPE_CONFORMANCE_SUITE_VERSION
    }

    /// How many cases were evaluated.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Every mismatch in evaluation order.
    #[must_use]
    pub fn mismatches(&self) -> &[CredentialRecipeMismatchV1] {
        &self.mismatches
    }

    /// The cases with at least one mismatch, in table order, without repeats.
    #[must_use]
    pub fn failed_case_ids(&self) -> Vec<CredentialRecipeCaseIdV1> {
        let mut failed: Vec<_> = self.mismatches.iter().map(|mismatch| mismatch.case_id).collect();
        failed.dedup();
        failed
    }
}

impl fmt::Debug for CredentialRecipeConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialRecipeConformanceFailureV1")
            .field("suite_id", &CREDENTIAL_RECIPE_CONFORMANCE_SUITE_ID)
            .field("suite_version", &CREDENTIAL_RECIPE_CONFORMANCE_SUITE_VERSION)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs every case in table order without failing fast.
///
/// Every caller must wrap the runner in an outer watchdog: it has no internal timeout, so an
/// executor that deadlocks (say, a compare-and-swap loser waiting for a lock it holds) stays
/// pending forever. One case holds an exchange for 200 ms of wall time, so the suite takes at
/// least that long.
pub async fn run_credential_recipe_conformance_v1(
    harness: &dyn CredentialRecipeHarnessV1,
) -> Result<CredentialRecipeConformanceReportV1, CredentialRecipeConformanceFailureV1> {
    let fixtures = credential_recipe_fixtures_v1();
    let mut passed_case_ids = Vec::with_capacity(fixtures.len());
    let mut mismatches = Vec::new();
    for fixture in fixtures {
        let before = mismatches.len();
        run_case(harness, fixture, &mut mismatches).await;
        if mismatches.len() == before {
            passed_case_ids.push(fixture.case_id());
        }
    }
    if mismatches.is_empty() {
        Ok(CredentialRecipeConformanceReportV1 { passed_case_ids })
    } else {
        Err(CredentialRecipeConformanceFailureV1 {
            evaluated_case_count: fixtures.len(),
            mismatches,
        })
    }
}

struct Recorder<'a> {
    case_id: CredentialRecipeCaseIdV1,
    mismatches: &'a mut Vec<CredentialRecipeMismatchV1>,
}

impl Recorder<'_> {
    fn record_if(
        &mut self,
        condition: bool,
        step: Option<usize>,
        category: CredentialRecipeMismatchCategoryV1,
    ) {
        if condition {
            self.mismatches.push(CredentialRecipeMismatchV1 {
                case_id: self.case_id,
                step,
                category,
            });
        }
    }
}

async fn run_case(
    harness: &dyn CredentialRecipeHarnessV1,
    fixture: &CredentialRecipeFixtureV1,
    mismatches: &mut Vec<CredentialRecipeMismatchV1>,
) {
    let mut recorder = Recorder { case_id: fixture.case_id(), mismatches };
    let endpoint = FakeTokenEndpointV1::new(fixture.replies());
    let credentials = fixture.recipe().credentials();
    let Ok(session) = harness.open(&credentials, fixture.seed(), endpoint.clone()).await else {
        recorder.record_if(true, None, CredentialRecipeMismatchCategoryV1::SessionOpen);
        return;
    };
    for (index, step) in fixture.steps().iter().enumerate() {
        let at = Some(index);
        match step {
            CredentialRecipeStepV1::Resolve(expected) => {
                let observed = session.resolve(CREDENTIAL_RECIPE_SLOT_V1).await;
                let differs = !resolves_as(&observed, *expected);
                recorder.record_if(differs, at, CredentialRecipeMismatchCategoryV1::Resolve);
            }
            CredentialRecipeStepV1::ResolveConcurrently(expected) => {
                let (first, second) = join(
                    session.resolve(CREDENTIAL_RECIPE_SLOT_V1),
                    session.resolve(CREDENTIAL_RECIPE_SLOT_V1),
                )
                .await;
                let differs = !resolves_as(&first, *expected) || !resolves_as(&second, *expected);
                recorder.record_if(differs, at, CredentialRecipeMismatchCategoryV1::Resolve);
            }
            CredentialRecipeStepV1::ResolveAgainstCompetingWrite { winner, expected } => {
                match resolve_against_competing_write(session.as_ref(), &endpoint, winner).await {
                    Some(observed) => {
                        let differs = !resolves_as(&observed, *expected);
                        recorder.record_if(
                            differs,
                            at,
                            CredentialRecipeMismatchCategoryV1::Resolve,
                        );
                    }
                    None => recorder.record_if(
                        true,
                        at,
                        CredentialRecipeMismatchCategoryV1::CompetingWriteNotReached,
                    ),
                }
            }
            CredentialRecipeStepV1::WriteGeneration(generation) => {
                session.write_generation(generation).await;
            }
            CredentialRecipeStepV1::Probe => session.probe().await,
            CredentialRecipeStepV1::ExpectEndpointCalls(count) => {
                let differs = endpoint.calls() != *count;
                recorder.record_if(differs, at, CredentialRecipeMismatchCategoryV1::EndpointCalls);
            }
            CredentialRecipeStepV1::ExpectStored(expected) => {
                let stored = session.stored().await;
                compare_stored(&stored, expected, at, &mut recorder);
            }
        }
    }
    drop(session);
    let differs = !exchanges_match(fixture, &endpoint);
    recorder.record_if(differs, None, CredentialRecipeMismatchCategoryV1::Exchange);
}

/// Resolves while the exchange is held, commits `winner`, then releases the exchange. `None` when
/// the refresh finished without its exchange ever being held.
async fn resolve_against_competing_write(
    session: &dyn CredentialRecipeSessionV1,
    endpoint: &FakeTokenEndpointV1,
    winner: &CredentialGenerationFixtureV1,
) -> Option<CredentialResolveObservationV1> {
    let mut refresh = session.resolve(CREDENTIAL_RECIPE_SLOT_V1);
    let held_before = endpoint.held();
    let finished_early = poll_fn(|context| {
        endpoint.notify_arrival(context.waker());
        if let Poll::Ready(observed) = refresh.as_mut().poll(context) {
            return Poll::Ready(Some(observed));
        }
        if endpoint.held() > held_before { Poll::Ready(None) } else { Poll::Pending }
    })
    .await;
    if finished_early.is_some() {
        endpoint.release();
        return None;
    }
    session.write_generation(winner).await;
    endpoint.release();
    Some(refresh.await)
}

/// Polls both futures on the caller's task until both finish.
async fn join<A, B>(mut first: A, mut second: B) -> (A::Output, B::Output)
where
    A: Future + Unpin,
    B: Future + Unpin,
{
    let mut first_output = None;
    let mut second_output = None;
    poll_fn(|context| {
        if first_output.is_none()
            && let Poll::Ready(output) = Pin::new(&mut first).poll(context)
        {
            first_output = Some(output);
        }
        if second_output.is_none()
            && let Poll::Ready(output) = Pin::new(&mut second).poll(context)
        {
            second_output = Some(output);
        }
        match (first_output.take(), second_output.take()) {
            (Some(first), Some(second)) => Poll::Ready((first, second)),
            (first, second) => {
                first_output = first;
                second_output = second;
                Poll::Pending
            }
        }
    })
    .await
}

fn resolves_as(
    observed: &CredentialResolveObservationV1,
    expected: CredentialResolveExpectedV1,
) -> bool {
    match (observed, expected) {
        (
            CredentialResolveObservationV1::Minted(value),
            CredentialResolveExpectedV1::Minted(want),
        ) => value == want,
        (
            CredentialResolveObservationV1::ReauthRequired,
            CredentialResolveExpectedV1::ReauthRequired,
        )
        | (CredentialResolveObservationV1::Transient, CredentialResolveExpectedV1::Transient) => {
            true
        }
        _ => false,
    }
}

fn as_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
}

fn compare_stored(
    stored: &StoredCredentialV1,
    expected: &CredentialStoredExpectedV1,
    at: Option<usize>,
    recorder: &mut Recorder<'_>,
) {
    recorder.record_if(
        stored.fields != as_map(expected.fields()),
        at,
        CredentialRecipeMismatchCategoryV1::StoredFields,
    );
    let minted = stored.minted();
    recorder.record_if(
        minted.map(|(value, _)| value) != Some(expected.minted().value()),
        at,
        CredentialRecipeMismatchCategoryV1::StoredMinted,
    );
    let expiry_differs = minted.is_none_or(|(_, expires_at)| {
        let remaining = expires_at.saturating_sub(now_epoch_seconds());
        remaining.abs_diff(expected.minted().remaining_seconds())
            > CREDENTIAL_RECIPE_EXPIRY_TOLERANCE_SECONDS_V1.unsigned_abs()
    });
    recorder.record_if(expiry_differs, at, CredentialRecipeMismatchCategoryV1::StoredExpiry);
    if let Some(previous) = expected.previous_fields() {
        recorder.record_if(
            stored.previous_fields != as_map(previous),
            at,
            CredentialRecipeMismatchCategoryV1::PreviousGeneration,
        );
    }
}

fn exchanges_match(fixture: &CredentialRecipeFixtureV1, endpoint: &FakeTokenEndpointV1) -> bool {
    let requests = endpoint.requests();
    let recipe = fixture.recipe();
    requests.len() == fixture.exchanges().len()
        && requests.iter().zip(fixture.exchanges()).all(|(request, expected)| {
            request.method() == "POST"
                && request.url() == CREDENTIAL_RECIPE_FAKE_TOKEN_ENDPOINT_V1
                && param(recipe, request.body()).as_deref() == Some(*expected)
        })
}

/// The value of the recipe's identifying parameter in an encoded exchange body.
fn param(recipe: CredentialRecipeKindV1, body: &[u8]) -> Option<String> {
    let name = recipe.identifying_param();
    match recipe.encoding() {
        EncodingV1::Json => serde_json::from_slice::<serde_json::Value>(body)
            .ok()?
            .get(name)?
            .as_str()
            .map(str::to_owned),
        EncodingV1::Form => std::str::from_utf8(body).ok()?.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (form_decode(key)? == name).then(|| form_decode(value)).flatten()
        }),
    }
}

/// Decodes one `application/x-www-form-urlencoded` component.
fn form_decode(component: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(component.len());
    let mut input = component.bytes();
    while let Some(byte) = input.next() {
        match byte {
            b'+' => bytes.push(b' '),
            b'%' => {
                let high = char::from(input.next()?).to_digit(16)?;
                let low = char::from(input.next()?).to_digit(16)?;
                bytes.push(u8::try_from(high * 16 + low).ok()?);
            }
            other => bytes.push(other),
        }
    }
    String::from_utf8(bytes).ok()
}
