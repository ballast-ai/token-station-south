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
            Self::EndpointConfinement => "endpoint_confinement",
            Self::AuthErrorsAreNotRetriable => "auth_errors_are_not_retriable",
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
