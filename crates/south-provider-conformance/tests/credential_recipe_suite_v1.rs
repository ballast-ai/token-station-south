//! Self-test of `south.credential-recipe.v1` (gate ③ of B4, design record §3.7).
//!
//! A suite nobody has run proves nothing, and a suite that every executor passes proves nothing
//! either. So this file carries a minimal in-memory host — a generic recipe executor over a store
//! with versions, a refresh lock, re-read after the lock, compare-and-swap and loser re-read, the
//! clamp, the no-wipe rule, the reauth latch and a read-only probe — and shows that it passes
//! every case. It then breaks that host one invariant at a time and shows that each break fails
//! exactly the case that guards the invariant, and no other.

use std::{
    collections::BTreeMap,
    fmt::Display,
    path::Path,
    sync::{Mutex, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use south_provider_api::{
    ComponentManifestV1, ConstantV1, CredentialsV1, EncodingV1, HOST_MAX_TTL_SECONDS,
    HOST_MIN_TTL_SECONDS, PresentCandidateV1, PresentV1, RecipeV1, SlotV1, StatusActionV1, StepV1,
};
use south_provider_conformance::{
    CREDENTIAL_RECIPE_CONFORMANCE_SUITE_ID, CREDENTIAL_RECIPE_CONFORMANCE_SUITE_VERSION,
    CREDENTIAL_RECIPE_FAKE_TOKEN_ENDPOINT_V1, CredentialGenerationFixtureV1,
    CredentialRecipeCaseIdV1, CredentialRecipeConformanceFailureV1, CredentialRecipeFixtureV1,
    CredentialRecipeFutureV1, CredentialRecipeHarnessV1, CredentialRecipeKindV1,
    CredentialRecipeOpenRefusedV1, CredentialRecipeSessionV1, CredentialResolveObservationV1,
    FakeTokenEndpointV1, FakeTokenRequestV1, StoredCredentialV1, credential_recipe_fixtures_v1,
    run_credential_recipe_conformance_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(CredentialRecipeFixtureV1: Display);
assert_not_impl_any!(StoredCredentialV1: Display);
assert_not_impl_any!(CredentialResolveObservationV1: Display);

use CredentialRecipeCaseIdV1 as Case;

#[test]
fn suite_identity_and_canonical_case_order_are_frozen() {
    assert_eq!(CREDENTIAL_RECIPE_CONFORMANCE_SUITE_VERSION, 1);
    assert_eq!(CREDENTIAL_RECIPE_CONFORMANCE_SUITE_ID, "south.credential-recipe.v1");
    let case_ids: Vec<_> =
        credential_recipe_fixtures_v1().iter().map(CredentialRecipeFixtureV1::case_id).collect();
    assert_eq!(
        case_ids,
        [
            Case::ExchangeFailureWritesNothing,
            Case::ReauthRequiredIsNotRetried,
            Case::ConcurrentRefreshIsSingleFlight,
            Case::ExpiryIsClampedToHostRange,
            Case::ExpiryIsClampedToRecipeRange,
            Case::RotationWritesBackAndKeepsPreviousGeneration,
            Case::EmptyRotationDoesNotWipe,
            Case::CasLoserRereadsTheWinner,
            Case::ProbeDoesNotRotate,
            Case::TransientFailureIsRetried,
        ]
    );
}

/// The suite's recipes must be ones gate ① admits, or a host could rightly refuse to open a case.
#[test]
fn both_recipes_pass_gate_one_inside_a_shipped_manifest() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/provider-openai-compatible/manifest.json");
    let shipped: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    for kind in [CredentialRecipeKindV1::Rotating, CredentialRecipeKindV1::NonRotating] {
        let mut manifest = shipped.clone();
        manifest["credentials"] = serde_json::to_value(kind.credentials()).unwrap();
        let manifest: ComponentManifestV1 = serde_json::from_value(manifest).unwrap();
        manifest.validate().unwrap_or_else(|error| panic!("{kind:?} fails gate 1: {error}"));
        assert_eq!(
            manifest.credentials.unwrap().endpoints(),
            [CREDENTIAL_RECIPE_FAKE_TOKEN_ENDPOINT_V1]
        );
    }
}

/// Debug output never carries a fixture value, so a host may log a failing case.
#[test]
fn debug_output_carries_no_secret_material() {
    let rendered = format!("{:?}", credential_recipe_fixtures_v1());
    assert!(!rendered.contains("south-test-only"), "{rendered}");
}

// ---------------------------------------------------------------------------------------------
// The reference host.
// ---------------------------------------------------------------------------------------------

/// One invariant broken on purpose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    None,
    /// Drops the stale minted value when an exchange fails transiently.
    WritesOnTransientFailure,
    /// Keeps no `reauth_required` latch: every resolve tries the endpoint again.
    RetriesAfterReauth,
    /// Takes no refresh lock and does not re-read: concurrent resolves both exchange.
    NoSingleFlight,
    /// Applies only the recipe's clamp.
    IgnoresHostClamp,
    /// Applies only the host's clamp.
    IgnoresRecipeClamp,
    /// Rotates without keeping the replaced generation.
    KeepsNoPreviousGeneration,
    /// Writes back whatever the rotation returned, including nothing.
    WipesOnEmptyRotation,
    /// Writes back without comparing versions.
    CasOverwrites,
    /// Probes by resolving, which rotates.
    ProbeRefreshes,
    /// Treats a transient failure like `reauth_required`: the generation is latched until it
    /// changes (host feedback SF2).
    LatchesTransientFailure,
}

struct ReferenceHost {
    fault: Fault,
}

impl CredentialRecipeHarnessV1 for ReferenceHost {
    fn open<'a>(
        &'a self,
        credentials: &'a CredentialsV1,
        seed: &'a CredentialGenerationFixtureV1,
        endpoint: FakeTokenEndpointV1,
    ) -> CredentialRecipeFutureV1<
        'a,
        Result<Box<dyn CredentialRecipeSessionV1 + 'a>, CredentialRecipeOpenRefusedV1>,
    > {
        let fault = self.fault;
        Box::pin(async move {
            let mut row = Row::default();
            row.replace(seed);
            Ok(Box::new(ReferenceSession {
                credentials: credentials.clone(),
                endpoint,
                fault,
                row: Mutex::new(row),
                refresh_lock: tokio::sync::Mutex::new(()),
            }) as Box<dyn CredentialRecipeSessionV1>)
        })
    }
}

fn now() -> i64 {
    i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()).unwrap()
}

#[derive(Clone, Default)]
struct Row {
    fields: BTreeMap<String, String>,
    previous: BTreeMap<String, String>,
    minted: Option<(String, i64)>,
    version: u64,
    reauth_at: Option<u64>,
}

impl Row {
    fn replace(&mut self, generation: &CredentialGenerationFixtureV1) {
        self.fields = generation
            .fields()
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        self.minted = generation
            .minted()
            .map(|minted| (minted.value().to_owned(), now() + minted.remaining_seconds()));
        self.version += 1;
    }

    fn fresh(&self, margin: i64) -> Option<String> {
        self.minted
            .as_ref()
            .filter(|(_, expires_at)| *expires_at > now() + margin)
            .map(|(value, _)| value.clone())
    }

    fn latched(&self) -> bool {
        self.reauth_at == Some(self.version)
    }
}

struct ReferenceSession {
    credentials: CredentialsV1,
    endpoint: FakeTokenEndpointV1,
    fault: Fault,
    row: Mutex<Row>,
    refresh_lock: tokio::sync::Mutex<()>,
}

enum Exchanged {
    Minted { value: String, ttl: i64, outputs: BTreeMap<String, Value> },
    Failed(CredentialResolveObservationV1),
}

impl ReferenceSession {
    fn read(&self) -> Row {
        self.row.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn recipe(&self, slot: &str) -> &RecipeV1 {
        let Some(SlotV1::Minted(name)) = self.credentials.slots.get(slot) else {
            panic!("slot {slot} is not minted");
        };
        &self.credentials.recipes[name]
    }

    fn margin(recipe: &RecipeV1) -> i64 {
        i64::from(recipe.refresh_margin_seconds.unwrap_or(0))
    }

    fn clamp(&self, recipe: &RecipeV1, ttl: i64) -> i64 {
        let host = (i64::from(HOST_MIN_TTL_SECONDS), i64::from(HOST_MAX_TTL_SECONDS));
        let declared = (
            recipe.min_ttl_seconds.map_or(0, i64::from),
            recipe.max_ttl_seconds.map_or(i64::MAX, i64::from),
        );
        let (low, high) = match self.fault {
            Fault::IgnoresHostClamp => declared,
            Fault::IgnoresRecipeClamp => host,
            _ => (host.0.max(declared.0), host.1.min(declared.1)),
        };
        ttl.clamp(low, high)
    }

    async fn resolve_slot(&self, slot: &str) -> CredentialResolveObservationV1 {
        let recipe = self.recipe(slot);
        let margin = Self::margin(recipe);
        let single_flight = self.fault != Fault::NoSingleFlight;
        let retries = self.fault == Fault::RetriesAfterReauth;

        // ① The authoritative row; ② fresh is returned as is.
        let mut row = self.read();
        if let Some(value) = row.fresh(margin) {
            return CredentialResolveObservationV1::Minted(value);
        }
        if row.latched() && !retries {
            return CredentialResolveObservationV1::ReauthRequired;
        }
        // ④ The refresh lock; ⑤ re-read under it.
        let _guard = if single_flight { Some(self.refresh_lock.lock().await) } else { None };
        if single_flight {
            row = self.read();
            if let Some(value) = row.fresh(margin) {
                return CredentialResolveObservationV1::Minted(value);
            }
            if row.latched() && !retries {
                return CredentialResolveObservationV1::ReauthRequired;
            }
        }
        // ⑥ The exchange.
        let (value, ttl, outputs) = match self.exchange(recipe, &row).await {
            Exchanged::Minted { value, ttl, outputs } => (value, ttl, outputs),
            Exchanged::Failed(failure) => {
                let mut current = self.row.lock().unwrap_or_else(PoisonError::into_inner);
                if current.version == row.version {
                    match failure {
                        CredentialResolveObservationV1::ReauthRequired => {
                            current.reauth_at = Some(row.version);
                        }
                        CredentialResolveObservationV1::Transient
                            if self.fault == Fault::LatchesTransientFailure =>
                        {
                            current.reauth_at = Some(row.version);
                        }
                        CredentialResolveObservationV1::Transient
                            if self.fault == Fault::WritesOnTransientFailure =>
                        {
                            current.minted = None;
                        }
                        _ => {}
                    }
                }
                drop(current);
                return failure;
            }
        };
        let expires_at = now() + self.clamp(recipe, ttl);
        // ⑥ Compare-and-swap write-back; ⑦ the loser re-reads the winner.
        let mut current = self.row.lock().unwrap_or_else(PoisonError::into_inner);
        if current.version != row.version && self.fault != Fault::CasOverwrites {
            return current.fresh(margin).map_or(
                CredentialResolveObservationV1::Transient,
                CredentialResolveObservationV1::Minted,
            );
        }
        for (field, output) in &recipe.write_back {
            let rotated = outputs.get(output.split_once('.').unwrap().1);
            let rotated = match (rotated, self.fault) {
                (Some(Value::String(value)), _) if !value.is_empty() => value.clone(),
                (_, Fault::WipesOnEmptyRotation) => String::new(),
                _ => continue,
            };
            if let Some(replaced) = current.fields.insert(field.clone(), rotated)
                && self.fault != Fault::KeepsNoPreviousGeneration
            {
                current.previous.insert(field.clone(), replaced);
            }
        }
        current.minted = Some((value.clone(), expires_at));
        current.version += 1;
        drop(current);
        CredentialResolveObservationV1::Minted(value)
    }

    async fn exchange(&self, recipe: &RecipeV1, row: &Row) -> Exchanged {
        let [step] = recipe.steps.as_slice() else {
            return Exchanged::Failed(CredentialResolveObservationV1::OtherFailure);
        };
        let request = render(step, &row.fields);
        let Ok(response) = self.endpoint.exchange(request).await else {
            return Exchanged::Failed(CredentialResolveObservationV1::Transient);
        };
        if !(200..300).contains(&response.status()) {
            return Exchanged::Failed(classify(step, response.status()));
        }
        let Ok(body) = serde_json::from_str::<Value>(response.body()) else {
            return Exchanged::Failed(CredentialResolveObservationV1::Transient);
        };
        let mut outputs = BTreeMap::new();
        let mut ttl = recipe.default_seconds.map(i64::from);
        for (name, extract) in &step.extract {
            if let Some(pointer) = &extract.pointer {
                match body.pointer(pointer) {
                    Some(value) => {
                        outputs.insert(name.clone(), value.clone());
                    }
                    None if extract.optional => {}
                    None => return Exchanged::Failed(CredentialResolveObservationV1::Transient),
                }
            } else if let Some(pointer) = &extract.relative_seconds {
                ttl = body.pointer(pointer).and_then(Value::as_i64).or(ttl);
            } else {
                return Exchanged::Failed(CredentialResolveObservationV1::OtherFailure);
            }
        }
        let Some([PresentCandidateV1::Output(present)]) =
            recipe.present.as_ref().map(PresentV1::as_slice)
        else {
            panic!("the suite's recipes present one step output");
        };
        let present = present.split_once('.').unwrap().1;
        match (outputs.get(present).and_then(Value::as_str), ttl) {
            (Some(value), Some(ttl)) => Exchanged::Minted { value: value.to_owned(), ttl, outputs },
            _ => Exchanged::Failed(CredentialResolveObservationV1::Transient),
        }
    }
}

fn render(step: &StepV1, fields: &BTreeMap<String, String>) -> FakeTokenRequestV1 {
    let params: BTreeMap<&str, String> = step
        .params
        .iter()
        .map(|(name, source)| {
            let value = match (&source.constant, &source.field) {
                (Some(ConstantV1::Text(text)), _) => text.clone(),
                (_, Some(field)) => fields.get(field).cloned().unwrap_or_default(),
                _ => panic!("the reference host renders only constants and fields"),
            };
            (name.as_str(), value)
        })
        .collect();
    let body = match step.encoding {
        Some(EncodingV1::Form) => params
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("&")
            .into_bytes(),
        _ => serde_json::to_vec(&params).unwrap(),
    };
    FakeTokenRequestV1::new("POST", step.endpoint.clone().unwrap(), body)
}

/// `on_status`: an exact code, then its class, then 4xx → reauth, 5xx → transient.
fn classify(step: &StepV1, status: u16) -> CredentialResolveObservationV1 {
    let class = format!("{}xx", status / 100);
    match step.on_status.get(&status.to_string()).or_else(|| step.on_status.get(&class)) {
        Some(StatusActionV1::Goto(_)) => CredentialResolveObservationV1::OtherFailure,
        Some(StatusActionV1::ReauthRequired) => CredentialResolveObservationV1::ReauthRequired,
        None if (400..500).contains(&status) => CredentialResolveObservationV1::ReauthRequired,
        Some(StatusActionV1::Transient) | None => CredentialResolveObservationV1::Transient,
    }
}

impl CredentialRecipeSessionV1 for ReferenceSession {
    fn resolve<'a>(
        &'a self,
        slot: &'a str,
    ) -> CredentialRecipeFutureV1<'a, CredentialResolveObservationV1> {
        Box::pin(self.resolve_slot(slot))
    }

    fn probe(&self) -> CredentialRecipeFutureV1<'_, ()> {
        Box::pin(async move {
            let slot = south_provider_conformance::CREDENTIAL_RECIPE_SLOT_V1;
            let rotates = self.recipe(slot).rotates_refresh_material == Some(true);
            // A probe of a rotating recipe only reads; one of a non-rotating recipe may mint.
            if !rotates || self.fault == Fault::ProbeRefreshes {
                self.resolve_slot(slot).await;
            }
        })
    }

    fn write_generation<'a>(
        &'a self,
        generation: &'a CredentialGenerationFixtureV1,
    ) -> CredentialRecipeFutureV1<'a, ()> {
        Box::pin(async move {
            self.row.lock().unwrap_or_else(PoisonError::into_inner).replace(generation);
        })
    }

    fn stored(&self) -> CredentialRecipeFutureV1<'_, StoredCredentialV1> {
        Box::pin(async move {
            let row = self.read();
            StoredCredentialV1::new(row.fields, row.previous, row.minted)
        })
    }
}

async fn run(fault: Fault) -> Result<Vec<Case>, CredentialRecipeConformanceFailureV1> {
    let host = ReferenceHost { fault };
    tokio::time::timeout(Duration::from_secs(10), run_credential_recipe_conformance_v1(&host))
        .await
        .expect("the suite must not hang")
        .map(|report| report.passed_case_ids().to_vec())
}

#[tokio::test]
async fn the_reference_host_passes_every_case() {
    let passed = run(Fault::None).await.unwrap_or_else(|failure| panic!("{failure:?}"));
    assert_eq!(passed.len(), credential_recipe_fixtures_v1().len());
}

#[tokio::test]
async fn each_broken_invariant_fails_exactly_the_case_that_guards_it() {
    let expectations = [
        (Fault::WritesOnTransientFailure, Case::ExchangeFailureWritesNothing),
        (Fault::RetriesAfterReauth, Case::ReauthRequiredIsNotRetried),
        (Fault::NoSingleFlight, Case::ConcurrentRefreshIsSingleFlight),
        (Fault::IgnoresHostClamp, Case::ExpiryIsClampedToHostRange),
        (Fault::IgnoresRecipeClamp, Case::ExpiryIsClampedToRecipeRange),
        (Fault::KeepsNoPreviousGeneration, Case::RotationWritesBackAndKeepsPreviousGeneration),
        (Fault::WipesOnEmptyRotation, Case::EmptyRotationDoesNotWipe),
        (Fault::CasOverwrites, Case::CasLoserRereadsTheWinner),
        (Fault::ProbeRefreshes, Case::ProbeDoesNotRotate),
        (Fault::LatchesTransientFailure, Case::TransientFailureIsRetried),
    ];
    // Every case is guarded by exactly one fault here, so the table is fully discriminating.
    let guarded: Vec<_> = expectations.iter().map(|(_, case)| *case).collect();
    let all: Vec<_> =
        credential_recipe_fixtures_v1().iter().map(CredentialRecipeFixtureV1::case_id).collect();
    assert_eq!(guarded, all);

    for (fault, case) in expectations {
        let failure = run(fault).await.expect_err(&format!("{fault:?} must fail the suite"));
        assert_eq!(failure.failed_case_ids(), [case], "{fault:?}: {failure:?}");
    }
}
