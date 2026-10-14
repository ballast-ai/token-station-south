//! What the suite decided, in a form a registry can store and act on.
//!
//! A report is not a pass/fail bit. A refused package is kept as a draft with
//! a recorded reason, and an upgrade or a canary is gated on the same
//! evidence. So every check that ran is named, every failure carries the case
//! that produced it, and [`CheckV1`] is a closed enumeration rather than a
//! string — an operator filters on it, and a later suite that adds a check
//! has to say so in the type.

use std::fmt;

/// One property the suite asserts of a component.
///
/// The sandbox rows — no network, no file system, bounded memory and time —
/// are deliberately absent: they are properties of the runtime's construction
/// (gate ④), not answers a component can be asked for, and a fixture that
/// claimed to check them would be theatre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckV1 {
    /// The pack carries at least one case for every fixture family.
    ///
    /// Without it a component passes by shipping nothing.
    Coverage,
    /// The component's output equals the fixture's expected output, byte for
    /// byte after canonical serialization.
    FixtureMatch,
    /// The same input, invoked twice, produced the same output.
    ///
    /// Catches map iteration order, clocks and randomness. A non-deterministic
    /// component makes a conformance pass meaningless, because the run that
    /// admitted it is not the run the host will get.
    Determinism,
    /// An input carrying a field this ABI version does not model was
    /// tolerated. A component meeting a newer peer must degrade, not fail.
    UnknownFieldTolerance,
    /// However a provider's streaming body is split into byte chunks, the
    /// component emits the same events.
    ///
    /// A chunk off a socket is not a whole frame, and under the v2 bytes ABI
    /// a split may land inside a UTF-8 sequence. A component that assumes
    /// otherwise passes every fixture and then corrupts or drops events in
    /// production, where the split points depend on the network.
    StreamIncrementality,
    /// A query that did not succeed never produced a terminal observation.
    ///
    /// Task-world only. 429, 5xx, 404 and 401 all mean the observation did
    /// not happen — a failed *query* is not a failed *task*. A component that
    /// reads a 404 as "gone" settles a task that may still be running, and the
    /// host releases a reservation against live work (2026-08-27 vocabulary
    /// record, D3 rule 2).
    TerminalOnlyFromTheWire,
    /// Every request the component built addresses the upstream it was
    /// configured against.
    ///
    /// The component chooses the URL and names the credential the host will
    /// attach to it. This is the check that keeps those two from combining.
    EndpointConfinement,
    /// A `401` or `403` from the upstream did not map onto a retriable code.
    ///
    /// Retriable means "try another upstream". Classifying a rejected
    /// credential as retriable would replay it across every provider the
    /// operator configured.
    AuthErrorsAreNotRetriable,
    /// Each usage row a `reported` package must ship by name shows what its
    /// name says (B1, host-zero-vendor-boundary §6.2 item 2).
    ///
    /// The rows are `response.usage` (non-zero usage, with a `usage_pointer`),
    /// `response.missing-usage` (a protocol error), `response.cached-usage` (a
    /// cache bucket), `stream.usage-terminal` (a usage event before the
    /// terminal) and `stream.no-usage` (no usage event). A missing row is a
    /// [`CheckV1::Coverage`] failure.
    UsageRows,
    /// Deleting the usage object a response fixture points at makes the
    /// component refuse the response rather than report zeros (§6.2 item 3).
    /// A stream fixture may point too, into the `data:` JSON of the chunk
    /// holding its terminal frame: the stream must then end in a provider
    /// protocol error with no usage event (`OpenAI` Responses upstream record
    /// §12.2, for a family that only streams).
    ///
    /// Usage is funds evidence: a zero the upstream never said would settle a
    /// call as free. The pointer is the fixture author's, so for a third-party
    /// package the check proves only what the author pointed at. In the
    /// embeddings world, `not_reported` is the other honest answer (record §10).
    UsageNeverDefaulted,
    /// An `absent` package never reports usage: every response carries
    /// all-zero usage and no stream emits a usage event (§6.2 item 4).
    AbsentFamilyEmitsNoUsage,
    /// Every usage the component reports keeps the IR partition: cache read
    /// and write within input, the cache-write tiers within cache write, and
    /// reasoning within output (§6.2 item 6; kernel `Usage`).
    UsagePartition,
    /// Every request the component built presents its credential through an
    /// arm its manifest declares (B2, host-zero-vendor-boundary §4.4).
    ///
    /// A host presents strictly what the descriptor says, so an arm the
    /// manifest never declared would put the credential somewhere the operator
    /// was not told about.
    DescriptorAuthWithinManifest,
    /// Every request the component built carries the output cap, the model
    /// and the stream flag where its manifest's `request_facts` declare, and
    /// the cap moves nowhere else when it changes (B2, host-zero-vendor-boundary
    /// §7.6).
    ///
    /// The host seals a request against these declarations instead of a table
    /// keyed by provider type; a component that writes the cap elsewhere would
    /// spend more than the host reserved.
    RequestFactsHonoured,
    /// For a family that declares `north_request_passthrough`, the descriptor the component builds
    /// for the probe IR equals the one it builds for every fixture request in method, URL, headers
    /// and authentication; only the body may differ (the `generateContent` request pass-through
    /// record, `docs/design/2026-10-14-generate-content-request-passthrough.md` §5).
    ///
    /// The host keeps the probe's descriptor and sends the client's own body; a component that put
    /// anything from the body into the URL or a header would send a request the host never sealed.
    DescriptorIndependentOfBody,
    /// Each `credential.*` fixture's run of the package's credential recipe, by south's reference
    /// interpreter, equals the fixture's expected run: the rendered exchange requests, the
    /// extracted values, the expiry, the write-back and the exported attributes (B4,
    /// host-zero-vendor-boundary §3.7).
    ///
    /// The host executes recipes, so a package's recipe is data whose meaning only a run shows.
    /// A recipe that sends the refresh token to the wrong parameter, reads the wrong clock, or
    /// loses rotated refresh material passes gate ① and fails here.
    CredentialRecipeMatch,
    /// A request built with keys the manifest does not declare added to `ProviderConfig.declared`
    /// and `ChatRequest.host_values` equals the request built without them (Q14,
    /// host-zero-vendor-boundary §13.8). In the embeddings world, which has no `host_values`, the
    /// key goes into `declared` alone (embeddings record §16).
    ///
    /// A component may read only the keys its package declares; a host passes only those. A
    /// component that acted on another key would depend on a value no conforming host sends, or
    /// on one a different package's declaration put there.
    UndeclaredValuesIgnored,
    /// The vector locator an embeddings request was prepared with, applied to the paired 2xx
    /// response fixture, resolves exactly as many vectors as the request has inputs
    /// (embeddings record §10).
    ///
    /// The host extracts vectors by the component's declaration; a locator that resolves the
    /// wrong count turns every such call into a protocol error after dispatch.
    LocatorResolves,
    /// Each row the embeddings suite requires by name shows what its name says (embeddings
    /// record §10): a single and a batch text request, dimensions placed in the body, a
    /// capability refusal, unmodelled fields carried, a usage sample, missing usage answered by
    /// the dialect's rule, a count mismatch refused, a rejected credential `rejected` and a 5xx
    /// `unknown`. A missing row is a [`CheckV1::Coverage`] failure.
    NamedRowAssertion,
    /// A request case holding a media input is built or refused as the package's `media`
    /// capability says (embeddings record §17.6).
    ///
    /// A package declaring `media` builds it or refuses it with a `capability` error (a model that
    /// takes no media); a package that does not declare `media` refuses it with a `capability`
    /// error. Either answer other than these would put a media input in front of a dialect that
    /// cannot carry it, or turn a per-model refusal into a failure the host cannot word.
    MediaInputsFollowTheDeclaration,
    /// For every metering form a media package declares, at least one response case gives exact
    /// metering facts with a `succeeded` outcome (image record §12.1, `metering_sample`; in the
    /// speech world also each `characters` source and `seconds` basis the package uses, speech
    /// record §11).
    ///
    /// The samples are DP1's usage samples: transcribed from the native arm, they are the only
    /// evidence that what the component reports is what the upstream billed.
    MeteringSample,
    /// For every required metering fact a media package can report — each declared token bucket,
    /// `credits`, `images_reported` on `charged_failure`; in the speech world each token count and
    /// the whole `usage` — a 2xx case missing it ends `unknown` (image record §12.1, speech record
    /// §11, `missing_meter_is_not_zero`).
    ///
    /// A missing fact read as zero would settle a call as free.
    MissingMeterIsNotZero,
    /// For every evidence fact a media package reports, a 2xx case missing it still `succeeds`,
    /// with that fact `null` (image record §12.1, `evidence_absent_is_null`).
    ///
    /// An evidence fact the upstream did not give must stay absent; a component that fills it in
    /// would hand the host a number nobody reported.
    EvidenceAbsentIsNull,
    /// Every refusal a media package makes in `prepare` is an `invalid_request` or `capability`
    /// error and produces no descriptor (image record §12.1, `pre_dispatch_refusal`; speech record
    /// §8).
    ///
    /// A refusal there is answered before anything is reserved or sent; any other code, or a
    /// refusal after a descriptor, would blur the line between "never sent" and "sent and failed".
    PreDispatchRefusal,
    /// Every output of a media package keeps its references honest (image record §12.1,
    /// `reference_integrity`): every `$south.ref` names a blob of the view, no `$south.blob`
    /// appears in any output, every `$south.artifact` names an existing artifact with an allowed
    /// delivery, every `inline` and `url` pointer lands on a string of the response, and no output
    /// string exceeds the fallback threshold. In the speech world every `segments` pointer lands on
    /// an elided string, and an artifact fits the declared response body form (speech record §6).
    ///
    /// The bytes stay out of the sandbox only if nothing the component returns can carry them, or
    /// point the host at bytes the upstream did not send.
    ReferenceIntegrity,
    /// Every request fixture of a family that declares `immutable_body_paths` leaves each declared
    /// path in one state, present in every request the component built or absent from every one
    /// (the `OpenAI` Responses upstream record §12.2).
    ///
    /// The host refuses an operator extra that touches a declared path, which protects what the
    /// family fixes only if the family really fixes it: a path the component writes for some
    /// requests and not for others is not a fixed field.
    ImmutablePathsHonoured,
    /// Every success a task contract 8 package reports carries only token buckets its manifest
    /// declares in `token_buckets`, and every bucket it declares in `required_token_buckets`
    /// (docs/design/2026-10-01-omni-task-component.md §10, `DeclaredBucketsOnly`).
    ///
    /// The host prices only the buckets a row can price and refuses a binding whose declared
    /// buckets it cannot; a bucket outside the declaration would be billed by no rule, and a
    /// missing required one would settle part of the bill as free.
    DeclaredBucketsOnly,
    /// A speech package that declares `seconds` has a 2xx response case missing the duration, and
    /// every such case ends with `seconds: null` or `unknown`, never `0` or any other number
    /// (speech record §11, `absent_duration_is_null_or_unknown`).
    ///
    /// The host estimates a `null` duration and marks it estimated; a number the upstream never
    /// gave would be billed as a measurement.
    AbsentDurationIsNullOrUnknown,
    /// A speech package that handles the dialect word `speech.verbose_json` renders, from one
    /// duration-bearing upstream response, every client format it accepts (`json`, `text`,
    /// `verbose_json`, `srt`, `vtt`) with the same `seconds` (speech record §9, §11,
    /// `client_format_rendering`).
    ///
    /// The client's format may change what it receives, never what it is billed: a component that
    /// asked the upstream for the client's subtitle format would lose the duration.
    ClientFormatRendering,
    /// `prepare` refuses an operation the model does not declare, and produces no descriptor
    /// (speech record §4, §11, `undeclared_operation_refused`); a prepared call names the
    /// operation it was asked for.
    ///
    /// A recognition model without `speech.translate` that transcribed when asked to translate is
    /// the defect this world was opened to fix.
    UndeclaredOperationRefused,
    /// Every SSML request body a speech package builds is well-formed XML that carries the
    /// northbound `input` unchanged as text and the northbound `voice` unchanged as an attribute
    /// value, and the pack has a case with `<`, `&`, `'` and `"` in both (speech record §5, §11,
    /// `ssml_escaping`).
    ///
    /// Both values are the client's; an unescaped one rewrites the upstream request.
    SsmlEscaping,
    /// A speech package that declares `artifact_fetch` has a `url` artifact case, and every `url`
    /// artifact of any package points at a string of the upstream response that passes the
    /// `ArtifactUrlV1` grammar (speech record §10, §11, `second_hop_artifact`).
    ///
    /// The host fetches only through its safe fetch executor; a pointer at anything else is a
    /// delivery that can never be verified.
    SecondHopArtifact,
    /// A 2xx that carries no audio — an empty body, an empty inline string, no `segments`
    /// fragment, no URL — ends `unknown`, and the pack has such a case for every response body form
    /// its successful syntheses use (speech record §10, §11, `empty_audio_is_unknown`).
    ///
    /// The host verifies the bytes before it settles; a `succeeded` with nothing to deliver would
    /// ask it to settle a call it cannot deliver.
    EmptyAudioIsUnknown,
}

impl CheckV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Coverage => "coverage",
            Self::FixtureMatch => "fixture_match",
            Self::Determinism => "determinism",
            Self::UnknownFieldTolerance => "unknown_field_tolerance",
            Self::StreamIncrementality => "stream_incrementality",
            Self::TerminalOnlyFromTheWire => "terminal_only_from_the_wire",
            Self::EndpointConfinement => "endpoint_confinement",
            Self::AuthErrorsAreNotRetriable => "auth_errors_are_not_retriable",
            Self::UsageRows => "usage_rows",
            Self::UsageNeverDefaulted => "usage_never_defaulted",
            Self::AbsentFamilyEmitsNoUsage => "absent_family_emits_no_usage",
            Self::UsagePartition => "usage_partition",
            Self::DescriptorAuthWithinManifest => "descriptor_auth_within_manifest",
            Self::RequestFactsHonoured => "request_facts_honoured",
            Self::DescriptorIndependentOfBody => "descriptor_independent_of_body",
            Self::CredentialRecipeMatch => "credential_recipe_match",
            Self::UndeclaredValuesIgnored => "undeclared_values_ignored",
            Self::LocatorResolves => "locator_resolves",
            Self::NamedRowAssertion => "named_row_assertion",
            Self::MediaInputsFollowTheDeclaration => "media_inputs_follow_the_declaration",
            Self::MeteringSample => "metering_sample",
            Self::MissingMeterIsNotZero => "missing_meter_is_not_zero",
            Self::EvidenceAbsentIsNull => "evidence_absent_is_null",
            Self::PreDispatchRefusal => "pre_dispatch_refusal",
            Self::ReferenceIntegrity => "reference_integrity",
            Self::ImmutablePathsHonoured => "immutable_paths_honoured",
            Self::DeclaredBucketsOnly => "declared_buckets_only",
            Self::AbsentDurationIsNullOrUnknown => "absent_duration_is_null_or_unknown",
            Self::ClientFormatRendering => "client_format_rendering",
            Self::UndeclaredOperationRefused => "undeclared_operation_refused",
            Self::SsmlEscaping => "ssml_escaping",
            Self::SecondHopArtifact => "second_hop_artifact",
            Self::EmptyAudioIsUnknown => "empty_audio_is_unknown",
        }
    }
}

impl fmt::Display for CheckV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerdictV1 {
    Passed,
    /// Why, in terms an operator can act on without reading the fixture.
    Failed(String),
}

/// One check, against one case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeV1 {
    pub check: CheckV1,
    /// The fixture case, e.g. `provider.error.rate-limit`. [`CheckV1::Coverage`]
    /// names the missing family instead.
    pub case: String,
    pub verdict: VerdictV1,
}

impl OutcomeV1 {
    #[must_use]
    pub fn passed(check: CheckV1, case: impl Into<String>) -> Self {
        Self { check, case: case.into(), verdict: VerdictV1::Passed }
    }

    #[must_use]
    pub fn failed(check: CheckV1, case: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { check, case: case.into(), verdict: VerdictV1::Failed(detail.into()) }
    }

    #[must_use]
    pub const fn is_failure(&self) -> bool {
        matches!(self.verdict, VerdictV1::Failed(_))
    }

    /// Why it failed; empty when it passed.
    #[must_use]
    pub fn detail(&self) -> &str {
        match &self.verdict {
            VerdictV1::Passed => "",
            VerdictV1::Failed(detail) => detail,
        }
    }
}

impl fmt::Display for OutcomeV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.verdict {
            VerdictV1::Passed => write!(f, "{}: {} passed", self.case, self.check),
            VerdictV1::Failed(detail) => {
                write!(f, "{}: {} failed: {detail}", self.case, self.check)
            }
        }
    }
}

/// The verdict on one component, against one suite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportV1 {
    suite: &'static str,
    outcomes: Vec<OutcomeV1>,
}

impl ReportV1 {
    #[must_use]
    pub const fn new(suite: &'static str, outcomes: Vec<OutcomeV1>) -> Self {
        Self { suite, outcomes }
    }

    /// The suite that produced this. Matches the manifest's
    /// `conformance.required_suite`.
    #[must_use]
    pub const fn suite(&self) -> &'static str {
        self.suite
    }

    #[must_use]
    pub fn outcomes(&self) -> &[OutcomeV1] {
        &self.outcomes
    }

    pub fn failures(&self) -> impl Iterator<Item = &OutcomeV1> {
        self.outcomes.iter().filter(|outcome| outcome.is_failure())
    }

    /// Whether the package may enter the runtime registry.
    #[must_use]
    pub fn is_passing(&self) -> bool {
        self.failures().next().is_none()
    }
}

impl fmt::Display for ReportV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{}: {} checks, {} failed",
            self.suite,
            self.outcomes.len(),
            self.failures().count()
        )?;
        for failure in self.failures() {
            writeln!(f, "  {failure}")?;
        }
        Ok(())
    }
}
