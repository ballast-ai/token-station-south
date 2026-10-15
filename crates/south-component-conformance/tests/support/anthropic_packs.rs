//! The gate ② pack of `provider-anthropic`: two directories judged as one.
//!
//! `fixtures-anthropic/` (the manifest's `conformance.fixtures`) holds the family-neutral rows and
//! the `anthropic` request rows; `fixtures-claude-code/` (Claude Code record §8) holds the
//! `claude-code` and `claude-code-setup-token` request rows and the `credential.*` rows of the
//! `claude_code` recipe. The response-side functions take no configuration, so the Claude Code
//! pack needs no response, stream, error or capabilities rows of its own; and a manifest whose
//! `credentials` section applies to `claude-code` owes the credential rows, which only the Claude
//! Code pack carries. So neither directory passes gate ② alone, and every gate ② run of this
//! package (native, sandboxed, and the release report) loads both through [`package_pack`].
//! Every Claude Code row is named `claude-code-*`, so no case name repeats across the two.

#![allow(dead_code, reason = "each test binary mounting this module uses a different subset")]

use std::path::{Path, PathBuf};

use south_component_conformance::{CredentialFixturePackV1, FixturePackV1};

/// The directory the manifest's `conformance.fixtures` names.
pub const ANTHROPIC_PACK: &str = "fixtures-anthropic";

/// The Claude Code families' directory (Claude Code record §8).
pub const CLAUDE_CODE_PACK: &str = "fixtures-claude-code";

/// The directory of one pack.
pub fn pack_dir(pack: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(pack)
}

/// One directory, alone.
pub fn load(pack: &str) -> FixturePackV1 {
    FixturePackV1::load(&pack_dir(pack)).unwrap_or_else(|error| panic!("{pack} loads: {error}"))
}

/// Both directories as the one pack gate ② judges the package by.
pub fn package_pack() -> FixturePackV1 {
    let anthropic = load(ANTHROPIC_PACK);
    let claude_code = load(CLAUDE_CODE_PACK);
    let cases = anthropic.cases().iter().chain(claude_code.cases()).cloned().collect();
    let credentials = anthropic
        .credentials()
        .cases()
        .iter()
        .chain(claude_code.credentials().cases())
        .cloned()
        .collect();
    FixturePackV1::from_cases(cases)
        .with_credentials(CredentialFixturePackV1::from_cases(credentials))
}
