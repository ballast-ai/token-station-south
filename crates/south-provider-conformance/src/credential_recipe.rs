//! Canonical cases for the credential-recipe host suite (gate ③ of B4,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §3.5–§3.7).
//!
//! Gate ① checks that a recipe is well formed and gate ② that a reference interpreter renders and
//! extracts what a recipe says. Neither touches the host, and the invariants that keep a credential
//! alive are the host's: one exchange per refresh however many requests race for it, re-reading
//! after the lock, compare-and-swap write-back with the loser re-reading the winner, the 60 s to
//! 24 h TTL clamp, never wiping refresh material with an empty value, never retrying a credential
//! the token endpoint called unusable, never letting a health probe rotate a refresh chain, and
//! exporting a credential's attributes from its current fields, with `persist` keeping the last
//! value of an absent one (§3.3, §13.8).
//! This suite drives a host's own generic recipe executor and credential store against a fake
//! token endpoint the suite controls, and checks those invariants from the outside.
//!
//! The suite is host-implemented: a host provides [`CredentialRecipeHarnessV1`] around its
//! executor and store, and runs [`run_credential_recipe_conformance_v1`]. The fake endpoint is
//! in-process — the host's executor accepts an injected token transport in test builds only and
//! routes every exchange to [`FakeTokenEndpointV1::exchange`]. That satisfies §3.4 rule 6: the
//! production executor has no configuration or credential field that replaces an endpoint, and
//! the suite needs no socket.

use std::{collections::BTreeMap, fmt};

use south_provider_api::{
    AttributeV1, CREDENTIAL_RECIPE_SCHEMA, ConstantV1, CredentialFieldV1, CredentialsV1,
    EncodingV1, ExtractV1, PredicateV1, PresentV1, RecipeV1, SelectRuleV1, SlotV1, StepKindV1,
    StepV1, ValueSourceV1, ValueSyntaxV1,
};

mod endpoint;
mod runner;

pub use endpoint::{
    FakeTokenEndpointV1, FakeTokenExchangeFutureV1, FakeTokenRequestV1, FakeTokenResponseV1,
    FakeTokenUnreachableV1,
};
pub use runner::{
    CredentialRecipeConformanceFailureV1, CredentialRecipeConformanceReportV1,
    CredentialRecipeFutureV1, CredentialRecipeHarnessV1, CredentialRecipeMismatchCategoryV1,
    CredentialRecipeMismatchV1, CredentialRecipeOpenRefusedV1, CredentialRecipeSessionV1,
    CredentialResolveObservationV1, StoredCredentialV1, run_credential_recipe_conformance_v1,
};

/// The credential-recipe conformance suite version.
pub const CREDENTIAL_RECIPE_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The stable identifier for credential-recipe conformance version one. It is the recipe schema
/// tag on purpose: the suite is the host half of that schema.
pub const CREDENTIAL_RECIPE_CONFORMANCE_SUITE_ID: &str = CREDENTIAL_RECIPE_SCHEMA;

/// The only endpoint the suite's recipes reach. Under `.invalid`, so no real resolver can ever
/// send a test exchange anywhere; the host's injected test transport answers it in process.
pub const CREDENTIAL_RECIPE_FAKE_TOKEN_ENDPOINT_V1: &str =
    "https://token-endpoint.credential-recipe.invalid/oauth/token";

/// The minted secret slot every case resolves.
pub const CREDENTIAL_RECIPE_SLOT_V1: &str = "provider_api_key";

/// How far a stored expiry may sit from the expected one, in seconds.
///
/// A host takes "now" a little before or after the exchange; this absorbs that without admitting
/// an unclamped expiry (the smallest distance any case separates is 50 seconds).
pub const CREDENTIAL_RECIPE_EXPIRY_TOLERANCE_SECONDS_V1: i64 = 5;

/// The refresh margin both recipes declare, so "fresh" means the same on every host.
pub const CREDENTIAL_RECIPE_REFRESH_MARGIN_SECONDS_V1: u32 = 30;

/// The narrower clamp of [`CredentialRecipeKindV1::NonRotating`].
pub const CREDENTIAL_RECIPE_NARROW_MIN_TTL_SECONDS_V1: u32 = 120;
/// See [`CREDENTIAL_RECIPE_NARROW_MIN_TTL_SECONDS_V1`].
pub const CREDENTIAL_RECIPE_NARROW_MAX_TTL_SECONDS_V1: u32 = 3600;

/// The recipes the suite runs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CredentialRecipeKindV1 {
    /// Codex-like: one `oauth2_token` refresh step, JSON encoding, the refresh token rotates and is
    /// written back; no recipe clamp, so only the host's applies.
    Rotating,
    /// Client-credentials: one `oauth2_token` step, form encoding, nothing rotates; the recipe
    /// narrows the clamp to 120 s – 3600 s.
    NonRotating,
    /// Exported attributes (§3.3, §13.8): a selector over two client-credentials recipes, chosen
    /// by whether `workspace` is present, that exports the recipe it chose
    /// ([`CREDENTIAL_RECIPE_SELECTED_ATTRIBUTE_V1`]); both recipes export `account_id` with
    /// `persist` and `label` without.
    Attributed,
}

fixed_debug!(CredentialRecipeKindV1 {
    Rotating => "Rotating",
    NonRotating => "NonRotating",
    Attributed => "Attributed",
});

/// The attribute [`CredentialRecipeKindV1::Attributed`]'s selector exports: the name of the recipe
/// it chose, `client_credentials` or `workspace_credentials`.
pub const CREDENTIAL_RECIPE_SELECTED_ATTRIBUTE_V1: &str = "credential_kind";

impl CredentialRecipeKindV1 {
    /// The recipe's `credentials` section, exactly as a manifest would carry it.
    #[must_use]
    pub fn credentials(self) -> CredentialsV1 {
        match self {
            Self::Rotating => rotating_credentials(),
            Self::NonRotating => non_rotating_credentials(),
            Self::Attributed => attributed_credentials(),
        }
    }

    /// The exchange parameter whose value identifies which stored generation a request used.
    #[must_use]
    pub const fn identifying_param(self) -> &'static str {
        match self {
            Self::Rotating => "refresh_token",
            Self::NonRotating | Self::Attributed => "client_secret",
        }
    }

    /// How the recipe encodes its exchange body.
    #[must_use]
    pub const fn encoding(self) -> EncodingV1 {
        match self {
            Self::Rotating => EncodingV1::Json,
            Self::NonRotating | Self::Attributed => EncodingV1::Form,
        }
    }
}

const fn field(secret: bool, required: bool) -> CredentialFieldV1 {
    CredentialFieldV1 {
        secret,
        required,
        syntax: None,
        media: None,
        default: None,
        description: None,
    }
}

fn constant(value: &str) -> ValueSourceV1 {
    ValueSourceV1 { constant: Some(ConstantV1::Text(value.to_owned())), ..ValueSourceV1::default() }
}

fn from_field(name: &str) -> ValueSourceV1 {
    ValueSourceV1 { field: Some(name.to_owned()), ..ValueSourceV1::default() }
}

fn pointer(pointer: &str, secret: bool, optional: bool) -> ExtractV1 {
    ExtractV1 { pointer: Some(pointer.to_owned()), secret, optional, ..ExtractV1::default() }
}

fn token_step(
    encoding: EncodingV1,
    params: BTreeMap<String, ValueSourceV1>,
    extract: BTreeMap<String, ExtractV1>,
    requires: Vec<String>,
) -> StepV1 {
    StepV1 {
        id: "token".to_owned(),
        kind: StepKindV1::Oauth2Token,
        method: None,
        endpoint: Some(CREDENTIAL_RECIPE_FAKE_TOKEN_ENDPOINT_V1.to_owned()),
        endpoint_params: BTreeMap::new(),
        encoding: Some(encoding),
        params,
        headers: BTreeMap::new(),
        auth: None,
        requires,
        on_status: BTreeMap::new(),
        extract,
        alg: None,
        key: None,
        claims: BTreeMap::new(),
    }
}

fn expiry() -> ExtractV1 {
    ExtractV1 { relative_seconds: Some("/expires_in".to_owned()), ..ExtractV1::default() }
}

fn rotating_credentials() -> CredentialsV1 {
    let params = BTreeMap::from([
        ("grant_type".to_owned(), constant("refresh_token")),
        ("refresh_token".to_owned(), from_field("refresh_token")),
        ("client_id".to_owned(), constant("south-test-only-client")),
    ]);
    let extract = BTreeMap::from([
        ("access_token".to_owned(), pointer("/access_token", true, false)),
        ("refresh_token".to_owned(), pointer("/refresh_token", true, true)),
        ("expires_at".to_owned(), expiry()),
    ]);
    let recipe = RecipeV1 {
        steps: vec![token_step(
            EncodingV1::Json,
            params,
            extract,
            vec!["refresh_token".to_owned()],
        )],
        present: Some(PresentV1::output("token.access_token")),
        rotates_refresh_material: Some(true),
        write_back: BTreeMap::from([(
            "refresh_token".to_owned(),
            "token.refresh_token".to_owned(),
        )]),
        refresh_margin_seconds: Some(CREDENTIAL_RECIPE_REFRESH_MARGIN_SECONDS_V1),
        ..RecipeV1::default()
    };
    CredentialsV1 {
        schema: CREDENTIAL_RECIPE_SCHEMA.to_owned(),
        families: None,
        fields: BTreeMap::from([("refresh_token".to_owned(), field(true, true))]),
        require_one_of: Vec::new(),
        import: BTreeMap::new(),
        seed: None,
        slots: BTreeMap::from([(
            CREDENTIAL_RECIPE_SLOT_V1.to_owned(),
            SlotV1::Minted("rotating_refresh".to_owned()),
        )]),
        recipes: BTreeMap::from([("rotating_refresh".to_owned(), recipe)]),
    }
}

fn non_rotating_credentials() -> CredentialsV1 {
    let params = BTreeMap::from([
        ("grant_type".to_owned(), constant("client_credentials")),
        ("client_id".to_owned(), from_field("client_id")),
        ("client_secret".to_owned(), from_field("client_secret")),
    ]);
    let extract = BTreeMap::from([
        ("access_token".to_owned(), pointer("/access_token", true, false)),
        ("expires_at".to_owned(), expiry()),
    ]);
    let recipe = RecipeV1 {
        steps: vec![token_step(EncodingV1::Form, params, extract, Vec::new())],
        present: Some(PresentV1::output("token.access_token")),
        rotates_refresh_material: Some(false),
        refresh_margin_seconds: Some(CREDENTIAL_RECIPE_REFRESH_MARGIN_SECONDS_V1),
        min_ttl_seconds: Some(CREDENTIAL_RECIPE_NARROW_MIN_TTL_SECONDS_V1),
        max_ttl_seconds: Some(CREDENTIAL_RECIPE_NARROW_MAX_TTL_SECONDS_V1),
        ..RecipeV1::default()
    };
    CredentialsV1 {
        schema: CREDENTIAL_RECIPE_SCHEMA.to_owned(),
        families: None,
        fields: BTreeMap::from([
            ("client_id".to_owned(), field(false, true)),
            ("client_secret".to_owned(), field(true, true)),
        ]),
        require_one_of: Vec::new(),
        import: BTreeMap::new(),
        seed: None,
        slots: BTreeMap::from([(
            CREDENTIAL_RECIPE_SLOT_V1.to_owned(),
            SlotV1::Minted("client_credentials".to_owned()),
        )]),
        recipes: BTreeMap::from([("client_credentials".to_owned(), recipe)]),
    }
}

fn attributed_credentials() -> CredentialsV1 {
    let token = |extra: Option<&str>| {
        let mut params = BTreeMap::from([
            ("grant_type".to_owned(), constant("client_credentials")),
            ("client_id".to_owned(), from_field("client_id")),
            ("client_secret".to_owned(), from_field("client_secret")),
        ]);
        if let Some(field) = extra {
            params.insert(field.to_owned(), from_field(field));
        }
        let extract = BTreeMap::from([
            ("access_token".to_owned(), pointer("/access_token", true, false)),
            ("expires_at".to_owned(), expiry()),
        ]);
        token_step(EncodingV1::Form, params, extract, Vec::new())
    };
    let exported = |field: &str, persist: bool| AttributeV1 {
        field: Some(field.to_owned()),
        selected_recipe: false,
        export: true,
        persist,
    };
    let recipe = |step: StepV1, attributes: &[(&str, AttributeV1)]| RecipeV1 {
        steps: vec![step],
        present: Some(PresentV1::output("token.access_token")),
        rotates_refresh_material: Some(false),
        refresh_margin_seconds: Some(CREDENTIAL_RECIPE_REFRESH_MARGIN_SECONDS_V1),
        attributes: attributes
            .iter()
            .map(|(name, attribute)| ((*name).to_owned(), attribute.clone()))
            .collect(),
        ..RecipeV1::default()
    };
    let selector = RecipeV1 {
        select: vec![
            SelectRuleV1 {
                when: Some(PredicateV1::FieldPresent("workspace".to_owned())),
                recipe: "workspace_credentials".to_owned(),
            },
            SelectRuleV1 { when: None, recipe: "client_credentials".to_owned() },
        ],
        attributes: BTreeMap::from([(
            CREDENTIAL_RECIPE_SELECTED_ATTRIBUTE_V1.to_owned(),
            AttributeV1 { field: None, selected_recipe: true, export: true, persist: false },
        )]),
        ..RecipeV1::default()
    };
    let exports =
        [("account_id", exported("account_id", true)), ("label", exported("label", false))];
    let attribute_field = |required: bool| CredentialFieldV1 {
        syntax: Some(ValueSyntaxV1::Token),
        ..field(false, required)
    };
    CredentialsV1 {
        schema: CREDENTIAL_RECIPE_SCHEMA.to_owned(),
        families: None,
        fields: BTreeMap::from([
            ("client_id".to_owned(), field(false, true)),
            ("client_secret".to_owned(), field(true, true)),
            ("account_id".to_owned(), attribute_field(false)),
            ("label".to_owned(), attribute_field(false)),
            ("workspace".to_owned(), attribute_field(false)),
        ]),
        require_one_of: Vec::new(),
        import: BTreeMap::new(),
        seed: None,
        slots: BTreeMap::from([(
            CREDENTIAL_RECIPE_SLOT_V1.to_owned(),
            SlotV1::Minted("pick".to_owned()),
        )]),
        recipes: BTreeMap::from([
            ("pick".to_owned(), selector),
            ("client_credentials".to_owned(), recipe(token(None), &exports)),
            ("workspace_credentials".to_owned(), recipe(token(Some("workspace")), &exports)),
        ]),
    }
}

/// A minted slot value as stored, with its expiry relative to the moment it is written.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CredentialMintedFixtureV1 {
    value: &'static str,
    remaining_seconds: i64,
}

impl CredentialMintedFixtureV1 {
    /// The minted value.
    #[must_use]
    pub const fn value(&self) -> &'static str {
        self.value
    }

    /// Seconds from now until it expires; negative means it expired that long ago.
    #[must_use]
    pub const fn remaining_seconds(&self) -> i64 {
        self.remaining_seconds
    }
}

impl fmt::Debug for CredentialMintedFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialMintedFixtureV1")
            .field("value_byte_count", &self.value.len())
            .field("remaining_seconds", &self.remaining_seconds)
            .finish()
    }
}

/// One stored generation of a credential: its field values and, optionally, a minted slot value.
///
/// A host stores it as a new generation — bumping whatever version its compare-and-swap compares —
/// either as the seed of a case or as an out-of-band write
/// ([`CredentialRecipeSessionV1::write_generation`]), even when its values equal the stored ones.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CredentialGenerationFixtureV1 {
    fields: &'static [(&'static str, &'static str)],
    minted: Option<CredentialMintedFixtureV1>,
}

impl CredentialGenerationFixtureV1 {
    /// The credential's field values, by field name.
    #[must_use]
    pub const fn fields(&self) -> &'static [(&'static str, &'static str)] {
        self.fields
    }

    /// The stored minted value of [`CREDENTIAL_RECIPE_SLOT_V1`], if any.
    #[must_use]
    pub const fn minted(&self) -> Option<&CredentialMintedFixtureV1> {
        self.minted.as_ref()
    }
}

impl fmt::Debug for CredentialGenerationFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialGenerationFixtureV1")
            .field("field_count", &self.fields.len())
            .field("minted", &self.minted)
            .finish()
    }
}

/// When the fake endpoint delivers a scripted reply.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FakeTokenHoldV1 {
    /// As soon as the exchange is polled.
    None,
    /// After this many milliseconds of wall time, so that a second refresh racing the first has
    /// time to reach the endpoint if the executor lets it.
    DelayMillis(u64),
    /// Only after the runner has committed a competing generation; the exchange is pending until
    /// then.
    UntilCompetingWrite,
}

impl fmt::Debug for FakeTokenHoldV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("None"),
            Self::DelayMillis(millis) => {
                formatter.debug_tuple("DelayMillis").field(millis).finish()
            }
            Self::UntilCompetingWrite => formatter.write_str("UntilCompetingWrite"),
        }
    }
}

/// What the fake endpoint answers to one exchange.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FakeTokenAnswerV1 {
    /// An HTTP response with this status and JSON body.
    Respond {
        /// HTTP status.
        status: u16,
        /// JSON body.
        body: &'static str,
    },
    /// The transport fails before any response, as an unreachable endpoint would.
    Unreachable,
}

impl fmt::Debug for FakeTokenAnswerV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Respond { status, body } => formatter
                .debug_struct("Respond")
                .field("status", status)
                .field("body_byte_count", &body.len())
                .finish(),
            Self::Unreachable => formatter.write_str("Unreachable"),
        }
    }
}

/// One scripted reply of the fake endpoint, consumed in call order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FakeTokenReplyV1 {
    answer: FakeTokenAnswerV1,
    hold: FakeTokenHoldV1,
}

impl FakeTokenReplyV1 {
    /// What the endpoint answers.
    #[must_use]
    pub const fn answer(&self) -> FakeTokenAnswerV1 {
        self.answer
    }

    /// When it answers.
    #[must_use]
    pub const fn hold(&self) -> FakeTokenHoldV1 {
        self.hold
    }
}

impl fmt::Debug for FakeTokenReplyV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FakeTokenReplyV1")
            .field("answer", &self.answer)
            .field("hold", &self.hold)
            .finish()
    }
}

/// The expected result of resolving the slot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CredentialResolveExpectedV1 {
    /// The slot resolves to exactly this value.
    Minted(&'static str),
    /// The credential is unusable until it changes (`reauth_required`, §3.3 `on_status`).
    ReauthRequired,
    /// A failure that may succeed later (`transient`).
    Transient,
}

impl fmt::Debug for CredentialResolveExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Minted(value) => {
                formatter.debug_struct("Minted").field("value_byte_count", &value.len()).finish()
            }
            Self::ReauthRequired => formatter.write_str("ReauthRequired"),
            Self::Transient => formatter.write_str("Transient"),
        }
    }
}

/// The expected stored state after a step.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CredentialStoredExpectedV1 {
    fields: &'static [(&'static str, &'static str)],
    minted: CredentialMintedFixtureV1,
    previous_fields: Option<&'static [(&'static str, &'static str)]>,
}

impl CredentialStoredExpectedV1 {
    /// The exact stored field values: no field missing, none extra, none emptied.
    #[must_use]
    pub const fn fields(&self) -> &'static [(&'static str, &'static str)] {
        self.fields
    }

    /// The stored minted value and its remaining lifetime, within
    /// [`CREDENTIAL_RECIPE_EXPIRY_TOLERANCE_SECONDS_V1`].
    #[must_use]
    pub const fn minted(&self) -> &CredentialMintedFixtureV1 {
        &self.minted
    }

    /// When present, the exact fields of the kept previous generation — the values a rotation
    /// replaced (§3.5). `None` leaves it unconstrained.
    #[must_use]
    pub const fn previous_fields(&self) -> Option<&'static [(&'static str, &'static str)]> {
        self.previous_fields
    }
}

impl fmt::Debug for CredentialStoredExpectedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialStoredExpectedV1")
            .field("field_count", &self.fields.len())
            .field("minted", &self.minted)
            .field("previous_constrained", &self.previous_fields.is_some())
            .finish()
    }
}

/// One step of a case script. The runner executes them in order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CredentialRecipeStepV1 {
    /// Resolve the slot once, as an inference request would.
    Resolve(CredentialResolveExpectedV1),
    /// Resolve the slot twice concurrently; both must give the expected result.
    ResolveConcurrently(CredentialResolveExpectedV1),
    /// Resolve the slot; while its exchange is held at the endpoint, commit `winner` out of band
    /// (another replica's winning write-back), then release the exchange.
    ResolveAgainstCompetingWrite {
        /// The generation that wins the compare-and-swap.
        winner: CredentialGenerationFixtureV1,
        /// The expected result of the losing refresh.
        expected: CredentialResolveExpectedV1,
    },
    /// Commit a new generation out of band, as an operator edit would.
    WriteGeneration(CredentialGenerationFixtureV1),
    /// Run the host's health probe on the credential.
    Probe,
    /// The endpoint has received exactly this many exchanges so far.
    ExpectEndpointCalls(usize),
    /// The store holds exactly this state.
    ExpectStored(CredentialStoredExpectedV1),
    /// The credential exports exactly these attributes into `ProviderConfig.declared` now
    /// ([`CredentialRecipeSessionV1::exported_attributes`]).
    ExpectAttributes(&'static [(&'static str, &'static str)]),
}

impl fmt::Debug for CredentialRecipeStepV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolve(expected) => formatter.debug_tuple("Resolve").field(expected).finish(),
            Self::ResolveConcurrently(expected) => {
                formatter.debug_tuple("ResolveConcurrently").field(expected).finish()
            }
            Self::ResolveAgainstCompetingWrite { winner, expected } => formatter
                .debug_struct("ResolveAgainstCompetingWrite")
                .field("winner", winner)
                .field("expected", expected)
                .finish(),
            Self::WriteGeneration(generation) => {
                formatter.debug_tuple("WriteGeneration").field(generation).finish()
            }
            Self::Probe => formatter.write_str("Probe"),
            Self::ExpectEndpointCalls(count) => {
                formatter.debug_tuple("ExpectEndpointCalls").field(count).finish()
            }
            Self::ExpectStored(expected) => {
                formatter.debug_tuple("ExpectStored").field(expected).finish()
            }
            Self::ExpectAttributes(attributes) => {
                let names: Vec<&str> = attributes.iter().map(|(name, _)| *name).collect();
                formatter.debug_tuple("ExpectAttributes").field(&names).finish()
            }
        }
    }
}

/// The closed set of canonical credential-recipe cases, one per §3.7 item.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CredentialRecipeCaseIdV1 {
    /// A 503 and then an unreachable endpoint each yield `transient`, and neither writes anything.
    ExchangeFailureWritesNothing,
    /// 401 and 400 yield `reauth_required`; a further resolve does not reach the endpoint until the
    /// credential changes, and a changed credential is tried again.
    ReauthRequiredIsNotRetried,
    /// Two concurrent resolves reach the endpoint once and both get the one minted value.
    ConcurrentRefreshIsSingleFlight,
    /// An expiry of 10 s is stored as 60 s and one of 7 days as 24 h.
    ExpiryIsClampedToHostRange,
    /// Under a recipe clamp of 120 s – 3600 s, 10 s is stored as 120 s and 7 days as 3600 s.
    ExpiryIsClampedToRecipeRange,
    /// A rotation writes the new refresh token back and keeps the replaced one.
    RotationWritesBackAndKeepsPreviousGeneration,
    /// A rotation whose response omits, empties or nulls the refresh token keeps the stored one.
    EmptyRotationDoesNotWipe,
    /// A refresh that loses the compare-and-swap returns the winner's value and writes nothing.
    CasLoserRereadsTheWinner,
    /// A probe on a rotating recipe with an expired token never reaches the endpoint.
    ProbeDoesNotRotate,
    /// Two transient failures on the same generation, with no write in between, and the next resolve
    /// exchanges again and mints: a transient failure holds nothing (§13.5 D5, host feedback SF2).
    TransientFailureIsRetried,
    /// The exported attributes follow the stored fields across an operator edit: the selector's
    /// choice changes with them, an attribute without `persist` disappears with its field, and none
    /// is cached from an earlier generation (§3.3, §13.8).
    ExportedAttributesFollowTheCredential,
    /// A `persist` attribute whose field becomes absent keeps its last value; a new value replaces
    /// it (§3.3).
    PersistedAttributeOutlivesItsField,
}

fixed_debug!(CredentialRecipeCaseIdV1 {
    ExchangeFailureWritesNothing => "ExchangeFailureWritesNothing",
    ReauthRequiredIsNotRetried => "ReauthRequiredIsNotRetried",
    ConcurrentRefreshIsSingleFlight => "ConcurrentRefreshIsSingleFlight",
    ExpiryIsClampedToHostRange => "ExpiryIsClampedToHostRange",
    ExpiryIsClampedToRecipeRange => "ExpiryIsClampedToRecipeRange",
    RotationWritesBackAndKeepsPreviousGeneration => "RotationWritesBackAndKeepsPreviousGeneration",
    EmptyRotationDoesNotWipe => "EmptyRotationDoesNotWipe",
    CasLoserRereadsTheWinner => "CasLoserRereadsTheWinner",
    ProbeDoesNotRotate => "ProbeDoesNotRotate",
    TransientFailureIsRetried => "TransientFailureIsRetried",
    ExportedAttributesFollowTheCredential => "ExportedAttributesFollowTheCredential",
    PersistedAttributeOutlivesItsField => "PersistedAttributeOutlivesItsField",
});

/// One immutable canonical credential-recipe case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CredentialRecipeFixtureV1 {
    case_id: CredentialRecipeCaseIdV1,
    recipe: CredentialRecipeKindV1,
    seed: CredentialGenerationFixtureV1,
    replies: &'static [FakeTokenReplyV1],
    steps: &'static [CredentialRecipeStepV1],
    exchanges: &'static [&'static str],
}

impl CredentialRecipeFixtureV1 {
    /// The stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> CredentialRecipeCaseIdV1 {
        self.case_id
    }

    /// The recipe the credential is minted with.
    #[must_use]
    pub const fn recipe(&self) -> CredentialRecipeKindV1 {
        self.recipe
    }

    /// The generation the store holds when the case opens.
    #[must_use]
    pub const fn seed(&self) -> &CredentialGenerationFixtureV1 {
        &self.seed
    }

    /// The fake endpoint's replies, in call order. A call beyond them is answered 500 and counted.
    #[must_use]
    pub const fn replies(&self) -> &'static [FakeTokenReplyV1] {
        self.replies
    }

    /// The script.
    #[must_use]
    pub const fn steps(&self) -> &'static [CredentialRecipeStepV1] {
        self.steps
    }

    /// For each exchange the endpoint must receive, in order, the value of the recipe's
    /// [`CredentialRecipeKindV1::identifying_param`]: which stored generation the executor used.
    /// Every exchange also goes to [`CREDENTIAL_RECIPE_FAKE_TOKEN_ENDPOINT_V1`] by `POST`.
    #[must_use]
    pub const fn exchanges(&self) -> &'static [&'static str] {
        self.exchanges
    }
}

impl fmt::Debug for CredentialRecipeFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialRecipeFixtureV1")
            .field("case_id", &self.case_id)
            .field("recipe", &self.recipe)
            .field("seed", &self.seed)
            .field("replies", &self.replies)
            .field("steps", &self.steps)
            .field("exchange_count", &self.exchanges.len())
            .finish()
    }
}

const RT_SEED: &str = "south-test-only-refresh-seed";
const RT_2: &str = "south-test-only-refresh-2";
const RT_3: &str = "south-test-only-refresh-3";
const RT_OPERATOR_1: &str = "south-test-only-refresh-operator-1";
const RT_OPERATOR_2: &str = "south-test-only-refresh-operator-2";
const RT_WINNER: &str = "south-test-only-refresh-winner";
const AT_SEED: &str = "south-test-only-access-seed";
const AT_1: &str = "south-test-only-access-1";
const AT_2: &str = "south-test-only-access-2";
const AT_3: &str = "south-test-only-access-3";
const AT_WINNER: &str = "south-test-only-access-winner";
const CLIENT_ID: &str = "south-test-only-client-id";
const CLIENT_SECRET: &str = "south-test-only-client-secret";

const SEED_FIELDS: &[(&str, &str)] = &[("refresh_token", RT_SEED)];
const RT_2_FIELDS: &[(&str, &str)] = &[("refresh_token", RT_2)];
const RT_3_FIELDS: &[(&str, &str)] = &[("refresh_token", RT_3)];
const CLIENT_FIELDS: &[(&str, &str)] =
    &[("client_id", CLIENT_ID), ("client_secret", CLIENT_SECRET)];
const ACCOUNT_1: &str = "south-test-only-account-1";
const ACCOUNT_2: &str = "south-test-only-account-2";
const LABEL: &str = "south-test-only-label";
const ACCOUNT_1_IN_WORKSPACE: &[(&str, &str)] = &[
    ("client_id", CLIENT_ID),
    ("client_secret", CLIENT_SECRET),
    ("account_id", ACCOUNT_1),
    ("label", LABEL),
    ("workspace", "south-test-only-workspace"),
];
const ACCOUNT_1_FIELDS: &[(&str, &str)] =
    &[("client_id", CLIENT_ID), ("client_secret", CLIENT_SECRET), ("account_id", ACCOUNT_1)];
const ACCOUNT_2_FIELDS: &[(&str, &str)] =
    &[("client_id", CLIENT_ID), ("client_secret", CLIENT_SECRET), ("account_id", ACCOUNT_2)];

const EXPIRED: i64 = -60;
const HOUR: i64 = 3600;

const fn minted(value: &'static str, remaining_seconds: i64) -> CredentialMintedFixtureV1 {
    CredentialMintedFixtureV1 { value, remaining_seconds }
}

const fn generation(
    fields: &'static [(&'static str, &'static str)],
    minted: Option<CredentialMintedFixtureV1>,
) -> CredentialGenerationFixtureV1 {
    CredentialGenerationFixtureV1 { fields, minted }
}

const fn stored(
    fields: &'static [(&'static str, &'static str)],
    value: &'static str,
    remaining_seconds: i64,
) -> CredentialRecipeStepV1 {
    CredentialRecipeStepV1::ExpectStored(CredentialStoredExpectedV1 {
        fields,
        minted: minted(value, remaining_seconds),
        previous_fields: None,
    })
}

const fn reply(status: u16, body: &'static str) -> FakeTokenReplyV1 {
    FakeTokenReplyV1 {
        answer: FakeTokenAnswerV1::Respond { status, body },
        hold: FakeTokenHoldV1::None,
    }
}

const fn held(status: u16, body: &'static str, hold: FakeTokenHoldV1) -> FakeTokenReplyV1 {
    FakeTokenReplyV1 { answer: FakeTokenAnswerV1::Respond { status, body }, hold }
}

/// The rotating seed: a refresh token and an access token that expired a minute ago.
const ROTATING_SEED: CredentialGenerationFixtureV1 =
    generation(SEED_FIELDS, Some(minted(AT_SEED, EXPIRED)));

const fn resolve(value: &'static str) -> CredentialRecipeStepV1 {
    CredentialRecipeStepV1::Resolve(CredentialResolveExpectedV1::Minted(value))
}

const REAUTH: CredentialRecipeStepV1 =
    CredentialRecipeStepV1::Resolve(CredentialResolveExpectedV1::ReauthRequired);
const TRANSIENT: CredentialRecipeStepV1 =
    CredentialRecipeStepV1::Resolve(CredentialResolveExpectedV1::Transient);

const fn calls(count: usize) -> CredentialRecipeStepV1 {
    CredentialRecipeStepV1::ExpectEndpointCalls(count)
}

const fn write(generation: CredentialGenerationFixtureV1) -> CredentialRecipeStepV1 {
    CredentialRecipeStepV1::WriteGeneration(generation)
}

const ROTATED_1_HOUR: &str = r#"{"access_token":"south-test-only-access-1","refresh_token":"south-test-only-refresh-2","expires_in":3600}"#;
const ROTATED_2_HOUR: &str = r#"{"access_token":"south-test-only-access-2","refresh_token":"south-test-only-refresh-3","expires_in":3600}"#;

const FIXTURES: &[CredentialRecipeFixtureV1] = &[
    CredentialRecipeFixtureV1 {
        case_id: CredentialRecipeCaseIdV1::ExchangeFailureWritesNothing,
        recipe: CredentialRecipeKindV1::Rotating,
        seed: ROTATING_SEED,
        replies: &[
            reply(503, r#"{"error":"temporarily_unavailable"}"#),
            FakeTokenReplyV1 {
                answer: FakeTokenAnswerV1::Unreachable,
                hold: FakeTokenHoldV1::None,
            },
        ],
        steps: &[
            TRANSIENT,
            calls(1),
            stored(SEED_FIELDS, AT_SEED, EXPIRED),
            // A new generation, so that a host that cools a credential down after a transient
            // failure still tries the second answer.
            write(ROTATING_SEED),
            TRANSIENT,
            calls(2),
            stored(SEED_FIELDS, AT_SEED, EXPIRED),
        ],
        exchanges: &[RT_SEED, RT_SEED],
    },
    CredentialRecipeFixtureV1 {
        case_id: CredentialRecipeCaseIdV1::ReauthRequiredIsNotRetried,
        recipe: CredentialRecipeKindV1::Rotating,
        seed: ROTATING_SEED,
        replies: &[
            reply(401, r#"{"error":"invalid_grant"}"#),
            reply(400, r#"{"error":"invalid_grant"}"#),
            reply(200, ROTATED_1_HOUR),
        ],
        steps: &[
            REAUTH,
            REAUTH,
            calls(1),
            stored(SEED_FIELDS, AT_SEED, EXPIRED),
            write(generation(&[("refresh_token", RT_OPERATOR_1)], None)),
            REAUTH,
            REAUTH,
            calls(2),
            write(generation(&[("refresh_token", RT_OPERATOR_2)], None)),
            resolve(AT_1),
            calls(3),
            stored(RT_2_FIELDS, AT_1, HOUR),
        ],
        exchanges: &[RT_SEED, RT_OPERATOR_1, RT_OPERATOR_2],
    },
    CredentialRecipeFixtureV1 {
        case_id: CredentialRecipeCaseIdV1::ConcurrentRefreshIsSingleFlight,
        recipe: CredentialRecipeKindV1::Rotating,
        seed: ROTATING_SEED,
        replies: &[
            held(200, ROTATED_1_HOUR, FakeTokenHoldV1::DelayMillis(200)),
            reply(200, ROTATED_2_HOUR),
        ],
        steps: &[
            CredentialRecipeStepV1::ResolveConcurrently(CredentialResolveExpectedV1::Minted(AT_1)),
            calls(1),
            stored(RT_2_FIELDS, AT_1, HOUR),
        ],
        exchanges: &[RT_SEED],
    },
    CredentialRecipeFixtureV1 {
        case_id: CredentialRecipeCaseIdV1::ExpiryIsClampedToHostRange,
        recipe: CredentialRecipeKindV1::Rotating,
        seed: ROTATING_SEED,
        replies: &[
            reply(
                200,
                r#"{"access_token":"south-test-only-access-1","refresh_token":"south-test-only-refresh-2","expires_in":10}"#,
            ),
            reply(
                200,
                r#"{"access_token":"south-test-only-access-2","refresh_token":"south-test-only-refresh-3","expires_in":604800}"#,
            ),
        ],
        steps: &[
            resolve(AT_1),
            stored(RT_2_FIELDS, AT_1, 60),
            write(generation(RT_2_FIELDS, Some(minted(AT_1, EXPIRED)))),
            resolve(AT_2),
            stored(RT_3_FIELDS, AT_2, 86_400),
            calls(2),
        ],
        exchanges: &[RT_SEED, RT_2],
    },
    CredentialRecipeFixtureV1 {
        case_id: CredentialRecipeCaseIdV1::ExpiryIsClampedToRecipeRange,
        recipe: CredentialRecipeKindV1::NonRotating,
        seed: generation(CLIENT_FIELDS, None),
        replies: &[
            reply(200, r#"{"access_token":"south-test-only-access-1","expires_in":10}"#),
            reply(200, r#"{"access_token":"south-test-only-access-2","expires_in":604800}"#),
        ],
        steps: &[
            resolve(AT_1),
            stored(CLIENT_FIELDS, AT_1, 120),
            write(generation(CLIENT_FIELDS, Some(minted(AT_1, EXPIRED)))),
            resolve(AT_2),
            stored(CLIENT_FIELDS, AT_2, 3600),
            calls(2),
        ],
        exchanges: &[CLIENT_SECRET, CLIENT_SECRET],
    },
    CredentialRecipeFixtureV1 {
        case_id: CredentialRecipeCaseIdV1::RotationWritesBackAndKeepsPreviousGeneration,
        recipe: CredentialRecipeKindV1::Rotating,
        seed: ROTATING_SEED,
        replies: &[reply(200, ROTATED_1_HOUR)],
        steps: &[
            resolve(AT_1),
            calls(1),
            CredentialRecipeStepV1::ExpectStored(CredentialStoredExpectedV1 {
                fields: RT_2_FIELDS,
                minted: minted(AT_1, HOUR),
                previous_fields: Some(SEED_FIELDS),
            }),
        ],
        exchanges: &[RT_SEED],
    },
    CredentialRecipeFixtureV1 {
        case_id: CredentialRecipeCaseIdV1::EmptyRotationDoesNotWipe,
        recipe: CredentialRecipeKindV1::Rotating,
        seed: ROTATING_SEED,
        replies: &[
            reply(200, r#"{"access_token":"south-test-only-access-1","expires_in":3600}"#),
            reply(
                200,
                r#"{"access_token":"south-test-only-access-2","refresh_token":"","expires_in":3600}"#,
            ),
            reply(
                200,
                r#"{"access_token":"south-test-only-access-3","refresh_token":null,"expires_in":3600}"#,
            ),
        ],
        steps: &[
            resolve(AT_1),
            stored(SEED_FIELDS, AT_1, HOUR),
            write(generation(SEED_FIELDS, Some(minted(AT_1, EXPIRED)))),
            resolve(AT_2),
            stored(SEED_FIELDS, AT_2, HOUR),
            write(generation(SEED_FIELDS, Some(minted(AT_2, EXPIRED)))),
            resolve(AT_3),
            stored(SEED_FIELDS, AT_3, HOUR),
            calls(3),
        ],
        exchanges: &[RT_SEED, RT_SEED, RT_SEED],
    },
    CredentialRecipeFixtureV1 {
        case_id: CredentialRecipeCaseIdV1::CasLoserRereadsTheWinner,
        recipe: CredentialRecipeKindV1::Rotating,
        seed: ROTATING_SEED,
        replies: &[held(
            200,
            r#"{"access_token":"south-test-only-access-loser","refresh_token":"south-test-only-refresh-loser","expires_in":3600}"#,
            FakeTokenHoldV1::UntilCompetingWrite,
        )],
        steps: &[
            CredentialRecipeStepV1::ResolveAgainstCompetingWrite {
                winner: generation(&[("refresh_token", RT_WINNER)], Some(minted(AT_WINNER, HOUR))),
                expected: CredentialResolveExpectedV1::Minted(AT_WINNER),
            },
            calls(1),
            stored(&[("refresh_token", RT_WINNER)], AT_WINNER, HOUR),
        ],
        exchanges: &[RT_SEED],
    },
    CredentialRecipeFixtureV1 {
        case_id: CredentialRecipeCaseIdV1::ProbeDoesNotRotate,
        recipe: CredentialRecipeKindV1::Rotating,
        seed: ROTATING_SEED,
        replies: &[reply(200, ROTATED_1_HOUR)],
        steps: &[CredentialRecipeStepV1::Probe, calls(0), stored(SEED_FIELDS, AT_SEED, EXPIRED)],
        exchanges: &[],
    },
    // Case 1 writes a new generation between its two failures, so a host that latched a transient
    // failure to the generation passed it. Here nothing is written: each resolve must reach the
    // endpoint, and the third mints.
    CredentialRecipeFixtureV1 {
        case_id: CredentialRecipeCaseIdV1::TransientFailureIsRetried,
        recipe: CredentialRecipeKindV1::Rotating,
        seed: ROTATING_SEED,
        replies: &[
            reply(503, r#"{"error":"temporarily_unavailable"}"#),
            FakeTokenReplyV1 {
                answer: FakeTokenAnswerV1::Unreachable,
                hold: FakeTokenHoldV1::None,
            },
            reply(200, ROTATED_1_HOUR),
        ],
        steps: &[
            TRANSIENT,
            calls(1),
            TRANSIENT,
            calls(2),
            resolve(AT_1),
            calls(3),
            stored(RT_2_FIELDS, AT_1, HOUR),
        ],
        exchanges: &[RT_SEED, RT_SEED, RT_SEED],
    },
    // Attributes come from the stored fields, so neither case needs an exchange.
    CredentialRecipeFixtureV1 {
        case_id: CredentialRecipeCaseIdV1::ExportedAttributesFollowTheCredential,
        recipe: CredentialRecipeKindV1::Attributed,
        seed: generation(ACCOUNT_1_IN_WORKSPACE, None),
        replies: &[],
        steps: &[
            CredentialRecipeStepV1::ExpectAttributes(&[
                ("account_id", ACCOUNT_1),
                ("credential_kind", "workspace_credentials"),
                ("label", LABEL),
            ]),
            write(generation(ACCOUNT_2_FIELDS, None)),
            CredentialRecipeStepV1::ExpectAttributes(&[
                ("account_id", ACCOUNT_2),
                ("credential_kind", "client_credentials"),
            ]),
            calls(0),
        ],
        exchanges: &[],
    },
    CredentialRecipeFixtureV1 {
        case_id: CredentialRecipeCaseIdV1::PersistedAttributeOutlivesItsField,
        recipe: CredentialRecipeKindV1::Attributed,
        seed: generation(ACCOUNT_1_FIELDS, None),
        replies: &[],
        steps: &[
            CredentialRecipeStepV1::ExpectAttributes(&[
                ("account_id", ACCOUNT_1),
                ("credential_kind", "client_credentials"),
            ]),
            write(generation(CLIENT_FIELDS, None)),
            CredentialRecipeStepV1::ExpectAttributes(&[
                ("account_id", ACCOUNT_1),
                ("credential_kind", "client_credentials"),
            ]),
            write(generation(ACCOUNT_2_FIELDS, None)),
            CredentialRecipeStepV1::ExpectAttributes(&[
                ("account_id", ACCOUNT_2),
                ("credential_kind", "client_credentials"),
            ]),
            write(generation(CLIENT_FIELDS, None)),
            CredentialRecipeStepV1::ExpectAttributes(&[
                ("account_id", ACCOUNT_2),
                ("credential_kind", "client_credentials"),
            ]),
            calls(0),
        ],
        exchanges: &[],
    },
];

/// Returns the immutable canonical credential-recipe case table.
#[must_use]
pub const fn credential_recipe_fixtures_v1() -> &'static [CredentialRecipeFixtureV1] {
    FIXTURES
}
