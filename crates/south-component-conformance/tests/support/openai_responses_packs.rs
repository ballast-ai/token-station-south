//! The gate ② pack of `provider-openai-responses`: three directories judged as one.
//!
//! `fixtures-openai-responses/` (the manifest's `conformance.fixtures`) holds the family-neutral
//! rows and the `openai-responses` request rows; `fixtures-openai-codex/` (R3 record §3.3) holds
//! the `openai-codex` request and stream rows and the `credential.*` rows of the Codex recipe;
//! `fixtures-bedrock-mantle/` (record `2026-10-15-bedrock-mantle.md` §6) holds the `bedrock-mantle`
//! request rows and the response and stream rows transcribed from the host's native Mantle arm.
//! The response-side functions take no configuration, so the Codex pack needs no response, error
//! or capabilities rows of its own; and a manifest whose `credentials` section applies to
//! `openai-codex` owes the credential rows, which only the Codex pack carries. So no directory
//! passes gate ② alone, and every gate ② run of this package — native, sandboxed, and the release
//! report — loads all three through [`package_pack`]. Every Codex row is named `codex-*` and every
//! Mantle row `mantle-*`, so no case name repeats across the three.

#![allow(dead_code, reason = "each test binary mounting this module uses a different subset")]

use std::path::{Path, PathBuf};

use south_component_conformance::{CredentialFixturePackV1, FixturePackV1};

/// The directory the manifest's `conformance.fixtures` names.
pub const RESPONSES_PACK: &str = "fixtures-openai-responses";

/// The Codex family's directory (R3 record §3.3).
pub const CODEX_PACK: &str = "fixtures-openai-codex";

/// The Bedrock Mantle family's directory (record `2026-10-15-bedrock-mantle.md` §6).
pub const MANTLE_PACK: &str = "fixtures-bedrock-mantle";

/// The directory of one pack.
pub fn pack_dir(pack: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(pack)
}

/// One directory, alone.
pub fn load(pack: &str) -> FixturePackV1 {
    FixturePackV1::load(&pack_dir(pack)).unwrap_or_else(|error| panic!("{pack} loads: {error}"))
}

/// The three directories as the one pack gate ② judges the package by.
pub fn package_pack() -> FixturePackV1 {
    let responses = load(RESPONSES_PACK);
    let codex = load(CODEX_PACK);
    let mantle = load(MANTLE_PACK);
    let cases =
        responses.cases().iter().chain(codex.cases()).chain(mantle.cases()).cloned().collect();
    let credentials = responses
        .credentials()
        .cases()
        .iter()
        .chain(codex.credentials().cases())
        .chain(mantle.credentials().cases())
        .cloned()
        .collect();
    FixturePackV1::from_cases(cases)
        .with_credentials(CredentialFixturePackV1::from_cases(credentials))
}
