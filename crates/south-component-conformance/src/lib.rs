#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Component-package and component-behavior conformance gates (gates ① and ②
//! of the four-gate layering).
//!
//! - **Gate ① (package admission)**: everything decidable before loading any
//!   code — [`accepts_manifest`] and the identity comparison
//!   [`reported_identity_matches`]. The manifest schema and the
//!   compatibility-tuple handshake live in `south-provider-api`, because the
//!   loader now *requires* the handshake: it is production admission, not a
//!   fixture, and a host cannot be left free to forget it.
//! - **Gate ② (component behavior)**: [`run_provider_component_suite_v1`] —
//!   typed decode, fixture-pinned translation, determinism, byte-level stream
//!   incrementality, endpoint confinement and error-catalog discipline,
//!   judged against the [`ProviderComponentV1`] seam so it exists and bites
//!   before any runtime can instantiate a component, and so a component
//!   author can run it against a native build without a WASM toolchain.
//!   A package whose manifest declares credential recipes is also judged by
//!   [`credential_recipe_checks_v1`]: its `credential.*` fixtures run through
//!   the test-only reference interpreter in [`credential_recipe`].
//! - Gate ③ (host integration) is the three frozen suites in
//!   `south-provider-conformance`, unchanged. Gate ④ (runtime enforcement)
//!   is a property of the S3 runtime's construction; no fixture here
//!   pretends to check it.
//!
//! This crate is the one sanctioned typed consumer of the Canonical IR in
//! this repository (S0 invariant 6): it takes `token-station-protocol` at a
//! **fixed kernel revision** (the distribution channel), so gate ② can judge
//! typed decode and byte-exact serialization. Production crates
//! (`south-provider-api`, `south-provider-runtime`) never gain this
//! dependency.
//!
//! The design record is `docs/design/2026-08-21-component-conformance.md`;
//! the contract it enforces is the S0 canonical IR inventory.

pub mod abi;
mod component;
mod component_v2;
pub use component_v2::{PreparedTaskV2, SubmitOutcomeV2, TaskComponentV2};
pub mod abi_task_v2;
pub mod reference_kling_task_v2;
mod task_fixture_v2;
mod task_suite_v2;
pub mod task_v2_json;
pub use task_fixture_v2::{
    TASK_FIXTURE_KIND_V2, TaskCaseV2, TaskFamilyV2, TaskFixturePackV2, TaskUsageMetaV2,
};
pub use task_suite_v2::{
    TASK_COMPONENT_SUITE_V2, run_task_component_suite_v2, run_task_component_suite_v2_for_manifest,
};
pub mod anthropic_dialect;
mod base64;
pub mod declared_parameters;
mod descriptor_auth;
mod url_segment;
pub use descriptor_auth::{
    AdmittedAuthV1, DescriptorAuthErrorV1, admit_descriptor_auth, admit_media_descriptor_auth,
};
// B7a (query, quota, user-agent): a manifest's declared instances as contract types.
mod declared_instances;
pub use declared_instances::{
    DeclaredInstancesErrorV1, DeclaredInstancesV1, contract_query_syntax,
};
mod credential_fixture;
pub mod credential_recipe;
pub use credential_fixture::{
    CREDENTIAL_FIXTURE_KIND_V1, CredentialCaseV1, CredentialFamilyV1, CredentialFixturePackV1,
    credential_recipe_checks_v1,
};
mod fixture;
mod reasoning_replay;
pub mod reference;
pub mod reference_anthropic;
pub mod reference_anthropic_bedrock_invoke;
pub mod reference_bedrock_converse;
// The `:generateContent` text wire the Gemini API and Vertex AI provider packages share (design
// record docs/design/2026-10-15-provider-vertex.md §3).
mod gemini_text;
pub mod reference_gemini;
pub mod reference_kiro;
pub mod reference_kling_task;
pub mod reference_openai_responses;
pub mod reference_vertex;
mod report;
pub mod responses_vocabulary;
#[cfg(feature = "sandbox")]
pub mod sandbox;
pub mod sse_split;
mod suite;
mod task_fixture;
pub mod task_json;
mod task_suite;
mod typed_request;
mod undeclared_values;

pub use component::{
    ComponentResultV1, ProviderComponentV1, StreamParserV1, SubmitOutcomeV1, TaskComponentV1,
};
pub use fixture::{CaseV1, FIXTURE_KIND_V1, FixtureErrorV1, FixturePackV1, ProviderFamilyV1};
pub use report::{CheckV1, OutcomeV1, ReportV1, VerdictV1};
pub use suite::{
    PROVIDER_COMPONENT_SUITE_V1, run_provider_component_suite_v1,
    run_provider_component_suite_v1_for_manifest,
};
pub use task_fixture::{TASK_FIXTURE_KIND_V1, TaskCaseV1, TaskFamilyV1, TaskFixturePackV1};
pub use task_suite::{TASK_COMPONENT_SUITE_V1, run_task_component_suite_v1};

use south_provider_api::{ComponentManifestV1, ComponentMetadataV1, ManifestErrorV1};

/// Gate ①, first half: everything the host can decide before loading any
/// code.
///
/// # Errors
///
/// Returns the [`ManifestErrorV1`] that disqualified the package.
pub fn accepts_manifest(manifest: &ComponentManifestV1) -> Result<(), ManifestErrorV1> {
    manifest.validate()
}

/// Gate ①, second half: what a loaded component reports must be what it
/// declared.
///
/// A package whose `metadata()` disagrees with its `manifest.json` has either
/// been repackaged around its vetting, or been built from different source
/// than it claims. Either way the registry's record of what is installed
/// would be wrong, and every later decision keyed on that record — canary by
/// version, rollback, capability dispatch — would act on a fiction.
#[must_use]
pub fn reported_identity_matches(
    reported: &ComponentMetadataV1,
    manifest: &ComponentManifestV1,
) -> bool {
    *reported == manifest.metadata()
}

#[cfg(feature = "sandbox")]
pub mod sandbox_task_v2;

pub mod reference_minimax_task_v2;

/// Managed Bailian video dialect.
pub mod reference_bailian_task_v2;
pub mod reference_byteplus_task_v2;
/// Gemini Omni video over the Interactions API (task contract 8).
pub mod reference_gemini_omni_task_v2;
pub mod reference_gmi_image_task_v2;
pub mod reference_veo_task_v2;
pub mod reference_wan_image_task_v2;
// The DashScope pieces the Wan task package and the Qwen-Image package share (image record I3-1 §3),
// and the DashScope error reader `speech-qwen` shares with `image-qwen` (speech batch V4 §2.2).
mod bailian_image;
pub mod reference_xai_task_v2;

// Embeddings contract 1 (docs/design/2026-09-30-embeddings-contract.md, v1 scope §15).
pub mod abi_embeddings;
pub mod abi_image;
mod component_embeddings;
mod component_image;
pub use component_embeddings::{EmbeddingsComponentV1, PreparedEmbeddingsV1};
pub use component_image::{ImageComponentV1, ImageOutcomeV1, ImageRenderedV1};
mod embeddings_fixture;
pub use embeddings_fixture::{
    EMBEDDINGS_FIXTURE_KIND_V1, EmbeddingsCaseV1, EmbeddingsFamilyV1, EmbeddingsFixturePackV1,
};
pub mod embeddings_json;
mod embeddings_suite;
pub mod image_fixture;
pub mod image_json;
pub mod image_suite;
pub use embeddings_suite::{
    EMBEDDINGS_COMPONENT_SUITE_V1, EMBEDDINGS_MEDIA_ROW_V2, EMBEDDINGS_REQUIRED_ROWS_V1,
    run_embeddings_component_suite_v1,
};
pub use image_suite::{IMAGE_COMPONENT_SUITE_V1, run_image_component_suite_v1};
pub mod reference_azure_image;
pub mod reference_ideogram_image;
pub mod reference_minimax_image;
pub mod reference_qwen_image;
pub mod reference_stability_image;
// The request and response pieces the two I3-1 image references share.
mod image_shared;
// The `:generateContent` image pieces the Gemini API package and the Vertex package of I3-3 share
// (image record I3-2 §7).
mod gemini_image;
pub mod reference_gemini_embeddings;
pub mod reference_gemini_image;
pub mod reference_openai_compatible_embeddings;
pub mod reference_vertex_embeddings;
pub mod reference_vertex_image;
#[cfg(feature = "sandbox")]
pub mod sandbox_embeddings;
#[cfg(feature = "sandbox")]
pub mod sandbox_image;

// The speech world (docs/design/2026-09-30-speech-world.md §7, §11; step S-S-3).
pub mod abi_speech;
mod component_speech;
pub use component_speech::{SpeechComponentV1, SpeechDeliveryV1, SpeechOutcomeV1};
pub mod reference_synthetic_speech;
#[cfg(feature = "sandbox")]
pub mod sandbox_speech;
pub mod speech_fixture;
pub mod speech_json;
pub mod speech_suite;
pub use speech_suite::{SPEECH_COMPONENT_SUITE_V1, run_speech_component_suite_v1};
// The four text-to-speech packages of batch V3-1 (design record
// docs/design/2026-10-14-speech-v3-1-tts.md): their references, the pieces all four share and the
// OpenAI speech body and event-stream reader the OpenAI-compatible and Groq packages share.
mod openai_speech;
pub mod reference_elevenlabs_speech;
pub mod reference_groq_speech;
pub mod reference_openai_compatible_speech;
pub mod reference_xai_speech;
mod speech_shared;
// The two text-to-speech packages of batch V3-2 (design record
// docs/design/2026-10-14-speech-v3-2-tts.md) and the `MiniMax` `base_resp` table the speech and
// image `MiniMax` references share.
mod minimax_shared;
pub mod reference_mimo_speech;
pub mod reference_minimax_speech;
// The speech recognition of batch V3-3 (design record docs/design/2026-10-14-speech-v3-3-asr.md):
// the pieces the four recognition references share and the new `speech-azure` reference; the
// OpenAI-compatible, xAI and ElevenLabs references above gain `transcribe` in place.
mod asr_shared;
pub mod reference_azure_speech;
// The text-to-speech packages of batch V4 (design record docs/design/2026-10-15-speech-v4-tts.md):
// Bailian Qwen-TTS and Vertex Gemini TTS; `speech-azure` above gains SSML synthesis in place.
pub mod reference_qwen_speech;
pub mod reference_vertex_speech;
// The XML well-formedness reader the `ssml_escaping` row uses.
mod ssml_check;
// The location rules the four Vertex AI references share (image record I3-3 §4; speech batch V4
// §3.1; provider-vertex record §3).
mod vertex_location;
