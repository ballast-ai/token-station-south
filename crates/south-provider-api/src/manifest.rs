use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The WIT package this crate owns (compatibility tuple field 3).
pub const WIT_PACKAGE: &str = "token-station:adapter@2.0.0";

/// The provider world name, doubling as the manifest `api_version`
/// (compatibility tuple field 4).
pub const PROVIDER_WORLD: &str = "provider-adapter-v2";

/// The component-behavior conformance suite name (compatibility tuple 6).
///
/// Gate ② judges every provider component under this identifier. Frozen here
/// so the manifest validates exactly; S2 builds the suite under this name.
pub const COMPONENT_BEHAVIOR_SUITE: &str = "south.provider-component.v1";

/// The task world's WIT package (compatibility tuple field 3).
///
/// A package of its own, not a second world inside [`WIT_PACKAGE`]: a package
/// version is shared by every world in it, so housing both would make a
/// chat-side change force a task-side version signal, and tuple field 3 is how
/// a component declares which bytes it was built against (2026-09-18
/// task-adapter-world record, D1).
pub const TASK_WIT_PACKAGE: &str = "token-station:task-adapter@1.0.0";

/// The task world name, doubling as the manifest `api_version`
/// (compatibility tuple field 4).
pub const TASK_WORLD: &str = "task-adapter-v1";

/// The task component-behavior conformance suite name (compatibility tuple 6).
///
/// The *name* is frozen here because it is what a manifest declares and a
/// manifest cannot be validated without it. Its *content* — the per-dialect
/// fixtures, including the one 404-query row each family owes under the
/// vocabulary record's D3 — is authored per family, as fixtures always are.
pub const TASK_BEHAVIOR_SUITE: &str = "south.task-component.v1";

/// The versioned task recovery and rendering ABI package.
pub const TASK_WIT_PACKAGE_V2: &str = "token-station:task-adapter@2.0.0";
/// The task-v2 world; task-v1 remains independently loadable.
pub const TASK_WORLD_V2: &str = "task-adapter-v2";
/// The task-v2 component behavior suite.
pub const TASK_BEHAVIOR_SUITE_V2: &str = "south.task-component.v2";

/// A component world this South knows, and the properties gate ① validates a
/// manifest against once the manifest has declared which world it is for
/// (2026-08-27 manifest-schema record, D1).
///
/// The suite name, the capability vocabulary, and the auth arms are
/// properties of the declared world, not constants of the schema. Each
/// vocabulary is closed on purpose: a word a world does not know is a version
/// mismatch, not a request to honour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldSchemaV1 {
    /// The world name a manifest declares in `api_version` (tuple 4).
    pub world: &'static str,
    /// The WIT package that world's functions live in (tuple 3).
    pub wit_package: &'static str,
    /// The suite that world is judged by (tuple 6).
    pub behavior_suite: &'static str,
    /// That world's capability vocabulary.
    pub capabilities: &'static [&'static str],
    /// That world's auth arm vocabulary.
    pub auth_arms: &'static [&'static str],
}

/// The provider world's capability vocabulary.
///
/// `chat` and `stream` name world functions; `tool_call` and `json_schema`
/// name `ChatRequest` fields the component promises to translate. Closed by
/// construction — v2 defines no function or field beyond these.
pub const PROVIDER_CAPABILITIES: &[&str] = &["chat", "stream", "tool_call", "json_schema"];

/// The provider world's auth arm vocabulary.
///
/// - `bearer`: `Authorization: Bearer <resolved secret>`, including
///   host-minted (OAuth-shaped) credentials whose product is a bearer token.
/// - `header_secret`: the resolved secret travels verbatim in one sanctioned
///   provider header, or in one the manifest declares in `secret_headers`.
/// - `oauth`: the host exchanges the named grant for a token before the funds
///   marker and presents it as a bearer token; the component never sees the
///   exchange.
/// - `host_signed`: the host's request finalizer signs every request after
///   the component returns its descriptor; the descriptor itself carries no
///   auth, and the manifest's `emits` set is the contract the finalizer's
///   output is diffed against (2026-08-27 manifest-schema record, D2–D3).
pub const PROVIDER_AUTH_ARMS: &[&str] = &["bearer", "header_secret", "oauth", "host_signed"];

/// The signed-header vocabulary a `host_signed` manifest may name in `emits`.
///
/// The wire names of the host half's frozen `SignedHeaderV1` enum
/// (`south-contracts`), repeated here because this crate deliberately depends
/// on no other south crate; a conformance-crate test pins the two lists
/// together.
pub const SIGNED_HEADER_NAMES: &[&str] =
    &["authorization", "x-amz-date", "x-amz-content-sha256", "x-amz-security-token"];

// -- Declared secret headers (B7a, host-zero-vendor-boundary §10) -------------

/// The maximum byte length of a declared secret header name.
///
/// Mirrors `south_contracts::MAX_SECRET_HEADER_NAME_BYTES`, repeated because
/// this crate depends on no other south crate; a conformance-crate test pins
/// the two together, with the list and the name rules below.
pub const MAX_SECRET_HEADER_NAME_BYTES: usize = 64;

/// The maximum number of names `secret_headers` may list. Mirrors
/// `south_contracts::MAX_DECLARED_SECRET_HEADERS`.
pub const MAX_SECRET_HEADERS: usize = 8;

/// The names `secret_headers` may never list, sorted.
///
/// Every name south already reserves for another purpose: framing and
/// hop-by-hop headers, `host`, `authorization`, cookies, `user-agent`, the
/// signed headers, the five sanctioned secret headers, `accept`, and the
/// response metadata the contracts read. Mirrors
/// `south_contracts::UNDECLARABLE_SECRET_HEADER_NAMES`.
pub const UNDECLARABLE_SECRET_HEADER_NAMES: &[&str] = &[
    "accept",
    "anthropic-ratelimit-tokens-limit",
    "anthropic-ratelimit-tokens-remaining",
    "anthropic-ratelimit-tokens-reset",
    "anthropic-ratelimit-unified-limit",
    "anthropic-ratelimit-unified-remaining",
    "anthropic-ratelimit-unified-reset",
    "anthropic-request-id",
    "api-key",
    "authorization",
    "cf-ray",
    "connection",
    "content-encoding",
    "content-length",
    "content-type",
    "cookie",
    "expect",
    "host",
    "keep-alive",
    "ocp-apim-subscription-key",
    "openai-organization",
    "openai-processing-ms",
    "openai-version",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "request-id",
    "retry-after",
    "server",
    "set-cookie",
    "set-cookie2",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "user-agent",
    "x-amz-content-sha256",
    "x-amz-date",
    "x-amz-security-token",
    "x-api-key",
    "x-goog-api-key",
    "x-ratelimit-limit-tokens",
    "x-ratelimit-remaining-tokens",
    "x-ratelimit-reset-tokens",
    "x-request-id",
    "xi-api-key",
];

/// Validates one `secret_headers` name: 1 to [`MAX_SECRET_HEADER_NAME_BYTES`]
/// bytes of lowercase RFC 9110 `tchar`, and not on
/// [`UNDECLARABLE_SECRET_HEADER_NAMES`].
///
/// # Errors
///
/// Returns [`ManifestErrorV1::InvalidSecretHeaderName`] for the syntax and
/// [`ManifestErrorV1::SecretHeaderIsReserved`] for a reserved name.
pub fn validate_secret_header_name(name: &str) -> Result<(), ManifestErrorV1> {
    let syntax = !name.is_empty()
        && name.len() <= MAX_SECRET_HEADER_NAME_BYTES
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"!#$%&'*+-.^_`|~".contains(&byte)
        });
    if !syntax {
        return Err(ManifestErrorV1::InvalidSecretHeaderName(name.to_owned()));
    }
    if UNDECLARABLE_SECRET_HEADER_NAMES.contains(&name) {
        return Err(ManifestErrorV1::SecretHeaderIsReserved(name.to_owned()));
    }
    Ok(())
}

/// The provider world, as gate ① validates it.
pub const PROVIDER_WORLD_SCHEMA: WorldSchemaV1 = WorldSchemaV1 {
    world: PROVIDER_WORLD,
    wit_package: WIT_PACKAGE,
    behavior_suite: COMPONENT_BEHAVIOR_SUITE,
    capabilities: PROVIDER_CAPABILITIES,
    auth_arms: PROVIDER_AUTH_ARMS,
};

/// The task world's capability vocabulary.
///
/// Every word names a lifecycle stage; unlike the provider world's set, none
/// names a request field, because a task component's request body is its
/// dialect's own and it promises nothing about IR fields.
///
/// `submit`, `observe` and `render` are **mandatory** — a component missing
/// one cannot complete a task. `artifact_fetch` is the single optional word:
/// it declares that `build-artifact-request` may return `Some`, so a host
/// knows before the first call whether to wire that execution path rather than
/// having to infer the component's shape by calling it (2026-09-18
/// task-adapter-world record, D3).
pub const TASK_CAPABILITIES: &[&str] = &["submit", "observe", "render", "artifact_fetch"];

/// The lifecycle stages every task component must declare.
pub const TASK_REQUIRED_CAPABILITIES: &[&str] = &["submit", "observe", "render"];

/// The task world, as gate ① validates it.
///
/// Its auth arms are [`PROVIDER_AUTH_ARMS`] unchanged: a task component
/// authenticates exactly as a chat one does — it names a credential and never
/// holds one. `host_signed` matters more here than in chat, since Kling's
/// HS256 JWT and Bedrock's `SigV4` are both task-side families (2026-09-18
/// task-adapter-world record, D4).
pub const TASK_WORLD_SCHEMA: WorldSchemaV1 = WorldSchemaV1 {
    world: TASK_WORLD,
    wit_package: TASK_WIT_PACKAGE,
    behavior_suite: TASK_BEHAVIOR_SUITE,
    capabilities: TASK_CAPABILITIES,
    auth_arms: PROVIDER_AUTH_ARMS,
};

/// Task-v2 currently admits the two validated descriptor credential arms.
pub const TASK_WORLD_SCHEMA_V2: WorldSchemaV1 = WorldSchemaV1 {
    world: TASK_WORLD_V2,
    wit_package: TASK_WIT_PACKAGE_V2,
    behavior_suite: TASK_BEHAVIOR_SUITE_V2,
    capabilities: TASK_CAPABILITIES,
    auth_arms: &["bearer", "header_secret"],
};

/// Every world this South can admit.
pub const KNOWN_WORLDS: &[WorldSchemaV1] =
    &[PROVIDER_WORLD_SCHEMA, TASK_WORLD_SCHEMA, TASK_WORLD_SCHEMA_V2];

/// Resolves a manifest's declared `api_version` to a world this South knows.
#[must_use]
pub fn known_world(api_version: &str) -> Option<&'static WorldSchemaV1> {
    KNOWN_WORLDS.iter().find(|schema| schema.world == api_version)
}

/// What the sandbox must grant.
///
/// `network` and `filesystem` exist so a manifest can *ask*, and be refused
/// with a named reason; silently ignoring the request would let an author
/// believe it was granted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentPermissionsV1 {
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub filesystem: bool,
    /// Names of credentials, never credentials. Each must look like a
    /// reference name, which is what stops a key being pasted here.
    #[serde(default)]
    pub secrets: Vec<String>,
}

/// Where the component's conformance fixtures live, and which suite gates it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceSpecV1 {
    pub required_suite: String,
    /// Directory inside the component package, e.g. `fixtures/`.
    pub fixtures: String,
}

/// The versions this component was built and verified against — the manifest
/// half of the compatibility tuple that is not already a top-level field.
///
/// Together with `version` (tuple 7), `api_version` (tuple 4) and
/// `conformance.required_suite` (tuple 6), this completes the seven-field
/// tuple of the S0 contract freeze. The runtime refuses any mismatch at load
/// time — refusal, never silent degradation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityDeclarationV1 {
    /// Tuple 1 — the IR revision, `token-station-protocol@<crate>/<kernel-tag>`,
    /// e.g. `token-station-protocol@0.4.0/v0.3.0`.
    pub ir_schema_id: String,
    /// Tuple 2a — the kernel distribution release, e.g. `0.2.0`.
    pub kernel_version: String,
    /// Tuple 2b — the kernel's mirrored upstream commit (40 lowercase hex).
    pub kernel_revision: String,
    /// Tuple 3 — must equal [`WIT_PACKAGE`].
    pub wit_package: String,
    /// Tuple 5 — the south runtime version the component was verified with.
    pub south_runtime: String,
    /// The runtime ABI epoch the component was built for; must equal the
    /// host's (B3, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §8.3).
    /// Absent on packages built before the range handshake, which only the
    /// exact handshake ([`compatibility_matches`]) can admit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_abi: Option<u32>,
    /// The kernel contract numbers the component was built against
    /// (`canonical_ir`, `stream`, `error_catalog`); each must equal the host's.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub kernel_contracts: BTreeMap<String, u32>,
    /// The south contract versions the component speaks (e.g. `{"task": 7}`);
    /// each must be one the host accepts.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contracts: BTreeMap<String, u32>,
}

/// A provider component package's `manifest.json`.
///
/// Untrusted third-party input, so it parses permissively and
/// [`ComponentManifestV1::validate`] rejects with an enumerable reason — a
/// registry has to record *why* it turned a package away. Unknown fields are
/// refused (`deny_unknown_fields`): a misspelt key must fail loudly, not
/// deserialize into a defaulted shape the record-keeping then lies about, and
/// a new manifest field must arrive through a schema bump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentManifestV1 {
    pub name: String,
    /// Tuple 7 — the component's own version, a `major.minor.patch` triple.
    pub version: String,
    /// Tuple 4 — the world this manifest is for; must name a world in
    /// [`KNOWN_WORLDS`].
    pub api_version: String,
    /// Provider dialect families this component translates, e.g.
    /// `openai-compatible`. Also names the usage cache convention the host's
    /// pricing folds (S0 ruling D1).
    pub providers: Vec<String>,
    /// Words from the declared world's capability vocabulary; a word the
    /// world does not know is refused with its name.
    pub capabilities: BTreeSet<String>,
    /// Auth arms the component's descriptors may use, from the declared
    /// world's vocabulary; empty means the component never attaches a
    /// credential (unauthenticated upstreams).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub auth_arms: BTreeSet<String>,
    /// The headers a `host_signed` component promises its host's finalizer
    /// will emit — the allow-list South diffs the finalizer's output against,
    /// in both directions. Required non-empty with the `host_signed` arm,
    /// refused without it. A `Vec`, not a set, so a duplicate is refused by
    /// name rather than silently collapsed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emits: Vec<String>,
    /// Secret-bearing header names this package's descriptors may present under
    /// the `header_secret` arm, beyond the five sanctioned ones (B7a,
    /// host-zero-vendor-boundary §10). For this package's requests the host
    /// reserves every listed name on the ordinary header channel and drops it
    /// from response transcripts. Requires the `header_secret` arm. A `Vec`,
    /// not a set, so a duplicate is refused by name. Absent means none, which is
    /// today's behavior.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_headers: Vec<String>,
    /// Whether this package's upstreams report token usage. `reported` (the
    /// default, omitted when serialized) or `absent`. Provider world only.
    #[serde(default, skip_serializing_if = "UsageEvidenceV1::is_reported")]
    pub usage_evidence: UsageEvidenceV1,
    /// What the host feeds `parse-stream-chunk`: upstream bytes unchanged
    /// (`bytes`, the default, omitted when serialized) or the canonical
    /// re-encoding of AWS eventstream messages (`aws-eventstream`). Package
    /// level, because the stream parser receives no configuration. Provider
    /// world only.
    #[serde(default, skip_serializing_if = "StreamFramingV1::is_bytes")]
    pub stream_framing: StreamFramingV1,
    /// How the package's minted secret slots are produced: fields, import,
    /// slots and recipes (B4, §3.3). Absent means every slot is `static`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<crate::CredentialsV1>,
    /// How the host signs a `host_signed` package's requests. Absent means the
    /// host infers nothing from the declaration (today's behavior).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing: Option<SigningV1>,
    /// Where each provider family's request carries the output cap, the model
    /// and the stream flag, keyed by family. A family without an entry uses
    /// [`RequestFactsV1::top_level`]. Provider world only.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub request_facts: BTreeMap<String, RequestFactsV1>,
    /// Each family's `https` endpoint template, whose parameters are its
    /// `config_schema` keys (§7.3). Provider world only.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub endpoint: BTreeMap<String, String>,
    /// Each family's non-secret configuration keys (§7.3). Provider world only.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config_schema: BTreeMap<String, BTreeMap<String, crate::ConfigKeyV1>>,
    // B7a (query, quota, user-agent): provider instances, §10. Provider world only.
    /// Query parameters the package's requests may carry beyond the fixed sanctioned ones, each a
    /// name and a value syntax. A `Vec`, so a repeated name is refused rather than collapsed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_parameters: Vec<crate::QueryParameterDeclarationV1>,
    /// The response headers the host's transport captures as quota metadata, each feeding one
    /// closed field. Package level, because responses are parsed without configuration (R6).
    /// Absent (or empty) means each field is read from its own canonical header, as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quota_headers: Vec<crate::QuotaHeaderDeclarationV1>,
    /// Each family's `user-agent` value, under the controlled user-agent value grammar (§16 Q15).
    /// A family without an entry sends whatever the host sends today.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub user_agent: BTreeMap<String, String>,
    pub permissions: ComponentPermissionsV1,
    pub conformance: ConformanceSpecV1,
    pub compatibility: CompatibilityDeclarationV1,
}

/// How the host signs a `host_signed` provider package's requests (B2,
/// `docs/design/2026-09-30-host-zero-vendor-boundary.md` §5.3).
///
/// The host picks its finalizer by `scheme` — a public-standard executor kept
/// in the host and selected by declaration — instead of inferring it from the
/// provider type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningV1 {
    pub scheme: SigningSchemeV1,
    /// The scheme's service name, e.g. `bedrock`.
    pub service: String,
    /// The endpoint-template parameter that names the region, so the region
    /// the host signs for is the region in the origin it sends to.
    pub region: TemplateParamV1,
    /// Each input the scheme needs, mapped to a credential field name.
    /// `aws-sigv4` requires `access_key_id` and `secret_access_key` and admits
    /// `session_token`. Checking the names against declared credential fields
    /// arrives with credential recipes (§3.3, phase B4).
    pub credentials: BTreeMap<String, String>,
}

/// A signing scheme the host implements. A closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SigningSchemeV1 {
    /// AWS Signature Version 4.
    AwsSigv4,
}

/// A reference to an endpoint-template parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateParamV1 {
    pub template_param: String,
}

/// The headers an `aws-sigv4` signature always emits.
const SIGV4_REQUIRED_EMITS: [&str; 3] = ["authorization", "x-amz-date", "x-amz-content-sha256"];

/// How a provider package's upstream frames its stream, which decides what the
/// host feeds `parse-stream-chunk` (B2, `docs/design/2026-09-30-host-zero-vendor-boundary.md`
/// §5.2).
///
/// There is no `sse`, `ndjson` or `json` value: components already split those
/// themselves, and a value the host would only branch on without using would
/// let it pick a decoder by declaration it never runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StreamFramingV1 {
    /// The host feeds the upstream bytes unchanged.
    #[default]
    Bytes,
    /// The host deframes AWS eventstream with `south_contracts::AwsEventStreamDeframerV1` and
    /// feeds each message's `south_contracts::reencode_eventstream_v1`. With a family declaring
    /// `request_facts.stream: "none"`, a non-streaming caller takes the buffered path: the whole
    /// body is deframed and its re-encoding handed to `parse-response`.
    AwsEventstream,
}

impl StreamFramingV1 {
    /// Whether this is the default, `bytes`.
    #[must_use]
    pub const fn is_bytes(&self) -> bool {
        matches!(self, Self::Bytes)
    }
}

/// Whether a provider package's upstreams report token usage (B1,
/// `docs/design/2026-09-30-host-zero-vendor-boundary.md` §6.2 item 4).
///
/// Package-level: a package whose families differ on this is two packages.
/// For an `absent` package, `parse-response` returns all-zero usage (the IR
/// field is not optional), the component never emits a usage event, and the
/// host never reads either — it meters with its own provider-agnostic
/// estimator and labels the result as an estimate. Gate ② holds an `absent`
/// package to `AbsentFamilyEmitsNoUsage`; a `reported` package to the usage
/// rows and `UsageNeverDefaulted`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageEvidenceV1 {
    /// The upstream reports exact usage, which is funds evidence.
    #[default]
    Reported,
    /// The upstream never reports tokens.
    Absent,
}

/// Where one provider family's request carries the facts the host seals (B2,
/// `docs/design/2026-09-30-host-zero-vendor-boundary.md` §7.2).
///
/// These are the component's own declarations: the host's seal proves a
/// descriptor is consistent with them, not that the upstream reads the cap
/// where the component wrote it (§6.3's undetectable zone).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestFactsV1 {
    /// JSON Pointers into the descriptor body where the component may write
    /// the output cap, at most [`MAX_OUTPUT_CAP_LOCATIONS`]; empty for a wire
    /// that has no cap field. With a cap set, exactly one location holds it.
    pub output_cap: Vec<String>,
    pub model: ModelLocationV1,
    pub stream: StreamLocationV1,
}

/// The most output-cap locations one family may declare.
pub const MAX_OUTPUT_CAP_LOCATIONS: usize = 4;

/// Where a request names its model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelLocationV1 {
    /// A JSON Pointer into the body whose value is the model.
    Body(String),
    /// A path template with exactly one `{model}` placeholder; the model,
    /// encoded as one path segment, sits where the placeholder is.
    Url(String),
}

/// Where a request carries its stream flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamLocationV1 {
    /// A JSON Pointer into the body whose value is `true` for a streaming
    /// request.
    Body(String),
    /// The URL differs; the host checks only that the response's content type
    /// matches the request.
    Url,
    /// The upstream always streams and has no switch.
    None,
}

impl RequestFactsV1 {
    /// The locations a family without an entry uses: today's three top-level
    /// fields (`max_tokens` or `max_completion_tokens`, `model`, `stream`).
    #[must_use]
    pub fn top_level() -> Self {
        Self {
            output_cap: vec!["/max_tokens".to_owned(), "/max_completion_tokens".to_owned()],
            model: ModelLocationV1::Body("/model".to_owned()),
            stream: StreamLocationV1::Body("/stream".to_owned()),
        }
    }
}

/// A JSON Pointer naming a member somewhere below the document root: every
/// reference token non-empty and every `~` escaped.
fn is_member_pointer(pointer: &str) -> bool {
    let Some(tokens) = pointer.strip_prefix('/') else {
        return false;
    };
    pointer.len() <= 256
        && tokens.split('/').all(|token| {
            !token.is_empty()
                && token
                    .split('~')
                    .skip(1)
                    .all(|rest| rest.starts_with('0') || rest.starts_with('1'))
        })
}

/// A path template: starts with `/`, printable ASCII without `?`, `#` or
/// spaces, and exactly one `{model}` with no other brace.
fn is_model_url_template(template: &str) -> bool {
    template.starts_with('/')
        && template.len() <= 256
        && template.bytes().all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'?' | b'#'))
        && template.matches("{model}").count() == 1
        && template.replacen("{model}", "", 1).bytes().all(|byte| !matches!(byte, b'{' | b'}'))
}

impl UsageEvidenceV1 {
    /// Whether this is the default, `reported`.
    #[must_use]
    pub const fn is_reported(&self) -> bool {
        matches!(self, Self::Reported)
    }
}

/// The identity a loaded component must report back from `metadata()`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentMetadataV1 {
    pub name: String,
    pub version: String,
    pub api_version: String,
}

/// A borrowed view of the complete seven-field compatibility tuple, in the
/// S0 contract order, for the runtime handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatibilityTupleV1<'m> {
    pub ir_schema_id: &'m str,
    pub kernel_version: &'m str,
    pub kernel_revision: &'m str,
    pub wit_package: &'m str,
    pub wit_world: &'m str,
    pub south_runtime: &'m str,
    pub conformance_suite: &'m str,
    pub component_version: &'m str,
}

impl ComponentManifestV1 {
    /// The identity a loaded component must report back from `metadata()`.
    #[must_use]
    pub fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: self.name.clone(),
            version: self.version.clone(),
            api_version: self.api_version.clone(),
        }
    }

    /// The complete compatibility tuple this manifest declares.
    #[must_use]
    pub fn compatibility_tuple(&self) -> CompatibilityTupleV1<'_> {
        CompatibilityTupleV1 {
            ir_schema_id: &self.compatibility.ir_schema_id,
            kernel_version: &self.compatibility.kernel_version,
            kernel_revision: &self.compatibility.kernel_revision,
            wit_package: &self.compatibility.wit_package,
            wit_world: &self.api_version,
            south_runtime: &self.compatibility.south_runtime,
            conformance_suite: &self.conformance.required_suite,
            component_version: &self.version,
        }
    }

    /// Checks everything gate ① requires before a package may be admitted.
    ///
    /// Order is deliberate: identity first (which resolves the declared
    /// world), then the sandbox, then the world's vocabulary, then role
    /// coherence, then conformance, then the compatibility declaration — so a
    /// package missing a name is not reported as a tuple violation.
    ///
    /// # Errors
    ///
    /// Returns the first [`ManifestErrorV1`] found.
    pub fn validate(&self) -> Result<(), ManifestErrorV1> {
        let world = self.validate_identity()?;
        self.validate_sandbox()?;
        if let Some(credentials) = &self.credentials {
            credentials.validate(&self.permissions.secrets, &self.providers)?;
        }
        self.validate_vocabulary(world)?;
        self.validate_signing()?;
        self.validate_secret_headers()?;
        self.validate_role(world)?;
        self.validate_instances(world)?;
        self.validate_conformance(world)?;
        self.validate_compatibility(world)
    }

    fn validate_identity(&self) -> Result<&'static WorldSchemaV1, ManifestErrorV1> {
        if self.name.is_empty() {
            return Err(ManifestErrorV1::MissingName);
        }
        validate_component_name(&self.name)?;
        if !is_semver_triple(&self.version) {
            return Err(ManifestErrorV1::InvalidVersion(self.version.clone()));
        }
        known_world(&self.api_version)
            .ok_or_else(|| ManifestErrorV1::ApiVersionIsNotAKnownWorld(self.api_version.clone()))
    }

    fn validate_sandbox(&self) -> Result<(), ManifestErrorV1> {
        if self.permissions.network {
            return Err(ManifestErrorV1::NetworkPermissionDenied);
        }
        if self.permissions.filesystem {
            return Err(ManifestErrorV1::FilesystemPermissionDenied);
        }
        for secret in &self.permissions.secrets {
            if !is_secret_ref_name(secret) {
                return Err(ManifestErrorV1::SecretIsNotAReferenceName(secret.clone()));
            }
        }
        Ok(())
    }

    fn validate_vocabulary(&self, world: &WorldSchemaV1) -> Result<(), ManifestErrorV1> {
        for capability in &self.capabilities {
            if !world.capabilities.contains(&capability.as_str()) {
                return Err(ManifestErrorV1::CapabilityIsNotInTheWorldVocabulary {
                    capability: capability.clone(),
                    world: world.world.to_owned(),
                });
            }
        }
        for auth_arm in &self.auth_arms {
            if !world.auth_arms.contains(&auth_arm.as_str()) {
                return Err(ManifestErrorV1::AuthArmIsNotInTheWorldVocabulary {
                    auth_arm: auth_arm.clone(),
                    world: world.world.to_owned(),
                });
            }
        }
        Ok(())
    }

    /// The `host_signed` coherence rules (2026-08-27 manifest-schema record,
    /// D2–D3).
    ///
    /// A signed request's descriptor carries no auth, so the manifest
    /// declaration is the only thing telling the host these requests are
    /// finalized. Making that indistinguishability unreachable means the arm
    /// admits no mixture: `host_signed` stands alone, its `emits` allow-list
    /// is non-empty, duplicate-free, and drawn from the frozen signed-header
    /// vocabulary — the same shapes, refused with the same words, as the host
    /// half's `SignedHeaderSetV1`.
    fn validate_signing(&self) -> Result<(), ManifestErrorV1> {
        if !self.auth_arms.contains("host_signed") {
            if let Some(header) = self.emits.first() {
                return Err(ManifestErrorV1::EmitsRequireTheHostSignedArm(header.clone()));
            }
            return Ok(());
        }
        if self.auth_arms.len() > 1 {
            return Err(ManifestErrorV1::HostSignedAdmitsNoOtherArm);
        }
        if self.emits.is_empty() {
            return Err(ManifestErrorV1::HostSignedNamesNoHeader);
        }
        let mut seen = BTreeSet::new();
        for header in &self.emits {
            if !SIGNED_HEADER_NAMES.contains(&header.as_str()) {
                return Err(ManifestErrorV1::EmitIsNotASignedHeader(header.clone()));
            }
            if !seen.insert(header.as_str()) {
                return Err(ManifestErrorV1::HostSignedNamesAHeaderTwice(header.clone()));
            }
        }
        Ok(())
    }

    /// The `secret_headers` rules (B7a, host-zero-vendor-boundary §10): the list
    /// needs the `header_secret` arm, is bounded and duplicate-free, and every
    /// name passes [`validate_secret_header_name`]. Every world whose vocabulary
    /// has `header_secret` may declare them, since task descriptors present
    /// credentials exactly as chat ones do.
    fn validate_secret_headers(&self) -> Result<(), ManifestErrorV1> {
        let Some(first) = self.secret_headers.first() else {
            return Ok(());
        };
        if !self.auth_arms.contains("header_secret") {
            return Err(ManifestErrorV1::SecretHeadersRequireTheHeaderSecretArm(first.clone()));
        }
        if self.secret_headers.len() > MAX_SECRET_HEADERS {
            return Err(ManifestErrorV1::TooManySecretHeaders(self.secret_headers.len()));
        }
        let mut seen = BTreeSet::new();
        for name in &self.secret_headers {
            validate_secret_header_name(name)?;
            if !seen.insert(name.as_str()) {
                return Err(ManifestErrorV1::SecretHeaderDeclaredTwice(name.clone()));
            }
        }
        Ok(())
    }

    fn validate_role(&self, world: &WorldSchemaV1) -> Result<(), ManifestErrorV1> {
        if world.world == PROVIDER_WORLD {
            if !self.capabilities.contains("chat") {
                return Err(ManifestErrorV1::ChatCapabilityRequired);
            }
            if self.providers.is_empty() {
                return Err(ManifestErrorV1::ProviderFamilyRequired);
            }
            self.validate_request_facts()?;
            self.validate_endpoints()?;
            self.validate_signing_declaration()?;
        } else if self.signing.is_some() {
            return Err(ManifestErrorV1::InvalidSigning(
                "signing is a provider-world declaration".to_owned(),
            ));
        } else if !self.stream_framing.is_bytes() {
            return Err(ManifestErrorV1::StreamFramingIsAProviderWorldDeclaration);
        } else if !self.endpoint.is_empty() || !self.config_schema.is_empty() {
            return Err(ManifestErrorV1::EndpointIsAProviderWorldDeclaration);
        } else if !self.usage_evidence.is_reported() {
            return Err(ManifestErrorV1::UsageEvidenceIsAProviderWorldDeclaration);
        } else if !self.request_facts.is_empty() {
            return Err(ManifestErrorV1::RequestFactsIsAProviderWorldDeclaration);
        }
        if matches!(world.world, TASK_WORLD | TASK_WORLD_V2) {
            // Three stages, all required: a component missing one cannot carry
            // a task to a terminal state. `artifact_fetch` is deliberately not
            // here — it is the one optional word.
            for stage in TASK_REQUIRED_CAPABILITIES {
                if !self.capabilities.contains(*stage) {
                    return Err(ManifestErrorV1::TaskLifecycleCapabilityRequired {
                        missing: (*stage).to_owned(),
                    });
                }
            }
            if self.providers.is_empty() {
                return Err(ManifestErrorV1::ProviderFamilyRequired);
            }
        }
        for provider in &self.providers {
            validate_component_name(provider)
                .map_err(|_| ManifestErrorV1::InvalidProviderFamily(provider.clone()))?;
        }
        Ok(())
    }

    fn validate_signing_declaration(&self) -> Result<(), ManifestErrorV1> {
        let Some(signing) = &self.signing else {
            return Ok(());
        };
        let invalid = |detail: &str| ManifestErrorV1::InvalidSigning(detail.to_owned());
        if !self.auth_arms.contains("host_signed") {
            return Err(invalid("signing is declared only by a host_signed package"));
        }
        if signing.service.is_empty()
            || signing.service.len() > 64
            || !signing
                .service
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(invalid("the service must be lowercase letters, digits and hyphens"));
        }
        match signing.scheme {
            SigningSchemeV1::AwsSigv4 => {
                if let Some(missing) = SIGV4_REQUIRED_EMITS
                    .iter()
                    .find(|header| !self.emits.iter().any(|emit| emit == *header))
                {
                    return Err(ManifestErrorV1::InvalidSigning(format!(
                        "aws-sigv4 always emits `{missing}`, which emits does not list"
                    )));
                }
                let inputs: Vec<&str> = signing.credentials.keys().map(String::as_str).collect();
                if !inputs.contains(&"access_key_id")
                    || !inputs.contains(&"secret_access_key")
                    || inputs.iter().any(|input| {
                        !matches!(*input, "access_key_id" | "secret_access_key" | "session_token")
                    })
                {
                    return Err(invalid(
                        "aws-sigv4 credentials are access_key_id, secret_access_key and optionally session_token",
                    ));
                }
            }
        }
        if signing.credentials.values().any(|field| {
            field.is_empty()
                || field.len() > 64
                || !field
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        }) {
            return Err(invalid("a credential field name is not lowercase snake_case"));
        }
        let placeholder = format!("{{{}}}", signing.region.template_param);
        if self.providers.iter().any(|family| {
            self.endpoint.get(family).is_none_or(|template| !template.contains(&placeholder))
        }) {
            return Err(invalid(
                "the region parameter must appear in every family's endpoint template",
            ));
        }
        Ok(())
    }

    fn validate_request_facts(&self) -> Result<(), ManifestErrorV1> {
        for (family, facts) in &self.request_facts {
            let invalid = |detail: &str| ManifestErrorV1::InvalidRequestFacts {
                family: family.clone(),
                detail: detail.to_owned(),
            };
            if !self.providers.contains(family) {
                return Err(invalid("names a family the manifest does not declare"));
            }
            if facts.output_cap.len() > MAX_OUTPUT_CAP_LOCATIONS {
                return Err(invalid("declares more than four output-cap locations"));
            }
            let mut seen = BTreeSet::new();
            for pointer in &facts.output_cap {
                if !is_member_pointer(pointer) || !seen.insert(pointer) {
                    return Err(invalid("an output-cap location is not a distinct member pointer"));
                }
            }
            match &facts.model {
                ModelLocationV1::Body(pointer) if !is_member_pointer(pointer) => {
                    return Err(invalid("the model location is not a member pointer"));
                }
                ModelLocationV1::Url(template) if !is_model_url_template(template) => {
                    return Err(invalid(
                        "the model URL template must be a path with exactly one `{model}`",
                    ));
                }
                ModelLocationV1::Body(_) | ModelLocationV1::Url(_) => {}
            }
            if let StreamLocationV1::Body(pointer) = &facts.stream
                && !is_member_pointer(pointer)
            {
                return Err(invalid("the stream location is not a member pointer"));
            }
        }
        Ok(())
    }

    /// The `credentials` section that applies to `family`: the package's section when it is
    /// unscoped or lists the family, otherwise none (§13.5 D2). A family without a section uses its
    /// static slots as the operator entered them, as before credential recipes.
    #[must_use]
    pub fn credentials_for(&self, family: &str) -> Option<&crate::CredentialsV1> {
        self.credentials.as_ref().filter(|credentials| {
            self.providers.iter().any(|provider| provider == family)
                && credentials.applies_to(family)
        })
    }

    /// Where `family`'s request carries its sealed facts: its declared entry,
    /// or [`RequestFactsV1::top_level`].
    #[must_use]
    pub fn request_facts_for(&self, family: &str) -> RequestFactsV1 {
        self.request_facts.get(family).cloned().unwrap_or_else(RequestFactsV1::top_level)
    }

    fn validate_conformance(&self, world: &WorldSchemaV1) -> Result<(), ManifestErrorV1> {
        if self.conformance.required_suite != world.behavior_suite {
            return Err(ManifestErrorV1::ConformanceSuiteIsNotTheWorldSuite {
                declared: self.conformance.required_suite.clone(),
                world: world.world.to_owned(),
                expected: world.behavior_suite.to_owned(),
            });
        }
        if self.conformance.fixtures.is_empty() {
            return Err(ManifestErrorV1::MissingFixtures);
        }
        validate_package_relative_path(&self.conformance.fixtures)?;
        Ok(())
    }

    fn validate_compatibility(&self, world: &WorldSchemaV1) -> Result<(), ManifestErrorV1> {
        if self.compatibility.wit_package != world.wit_package {
            return Err(ManifestErrorV1::WitPackageIsNotTheWorldPackage {
                declared: self.compatibility.wit_package.clone(),
                world: world.world.to_owned(),
                expected: world.wit_package.to_owned(),
            });
        }
        if !is_ir_schema_id(&self.compatibility.ir_schema_id) {
            return Err(ManifestErrorV1::InvalidIrSchemaId(
                self.compatibility.ir_schema_id.clone(),
            ));
        }
        if !is_semver_triple(&self.compatibility.kernel_version) {
            return Err(ManifestErrorV1::InvalidKernelVersion(
                self.compatibility.kernel_version.clone(),
            ));
        }
        if !is_commit_hash(&self.compatibility.kernel_revision) {
            return Err(ManifestErrorV1::InvalidKernelRevision(
                self.compatibility.kernel_revision.clone(),
            ));
        }
        if !is_semver_triple(&self.compatibility.south_runtime) {
            return Err(ManifestErrorV1::InvalidSouthRuntimeVersion(
                self.compatibility.south_runtime.clone(),
            ));
        }
        Ok(())
    }
}

/// Validates the one-component package identity: lowercase ASCII kebab-case,
/// alphanumeric at both ends, at most 64 bytes.
///
/// # Errors
///
/// Returns [`ManifestErrorV1::InvalidName`] otherwise.
pub fn validate_component_name(value: &str) -> Result<(), ManifestErrorV1> {
    let bytes = value.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= 64
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');
    if valid { Ok(()) } else { Err(ManifestErrorV1::InvalidName(value.to_owned())) }
}

/// Validates a normalized package-relative path without consulting the file
/// system. Runtime walkers still reject symlinks and special files.
///
/// # Errors
///
/// Returns [`ManifestErrorV1::InvalidFixturesPath`] for absolute,
/// parent-relative, platform-ambiguous, empty-component, control-character,
/// or over-deep paths.
pub fn validate_package_relative_path(value: &str) -> Result<(), ManifestErrorV1> {
    let normalized = value.strip_suffix('/').unwrap_or(value);
    let components: Vec<_> = normalized.split('/').collect();
    let invalid = value.is_empty()
        || value.len() > 256
        || value.starts_with('/')
        || value.ends_with("//")
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || components.len() > 16
        || components
            .iter()
            .any(|component| component.is_empty() || matches!(*component, "." | ".."));
    if invalid { Err(ManifestErrorV1::InvalidFixturesPath(value.to_owned())) } else { Ok(()) }
}

/// A reference name is lowercase alphanumeric with underscores, starting with
/// a letter. Deliberately narrow: a real credential — `sk-live-abc`, a base64
/// blob, a JWT — cannot satisfy it, so pasting one fails at admission rather
/// than leaking into a registry.
fn is_secret_ref_name(value: &str) -> bool {
    let mut chars = value.chars();
    let starts_with_letter = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    starts_with_letter && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn is_semver_triple(value: &str) -> bool {
    let mut parts = value.split('.');
    let triple = [parts.next(), parts.next(), parts.next()];
    parts.next().is_none()
        && triple.iter().all(|part| {
            part.is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// `token-station-protocol@<major.minor.patch>/v<major.minor.patch>` — crate
/// revision, then the kernel distribution tag it rode in on.
fn is_ir_schema_id(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("token-station-protocol@") else {
        return false;
    };
    let Some((crate_version, tag)) = rest.split_once('/') else {
        return false;
    };
    let Some(tag_version) = tag.strip_prefix('v') else {
        return false;
    };
    is_semver_triple(crate_version) && is_semver_triple(tag_version)
}

fn is_commit_hash(value: &str) -> bool {
    value.len() == 40
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Why gate ① refused a manifest.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ManifestErrorV1 {
    #[error("manifest declares no name")]
    MissingName,
    #[error("component name `{0}` must be one lowercase kebab-case component of at most 64 bytes")]
    InvalidName(String),
    #[error("version `{0}` is not a `major.minor.patch` triple")]
    InvalidVersion(String),
    #[error("api_version `{0}` is not a world this South knows")]
    ApiVersionIsNotAKnownWorld(String),
    #[error("capability `{capability}` is not in the `{world}` world's vocabulary")]
    CapabilityIsNotInTheWorldVocabulary { capability: String, world: String },
    #[error("auth arm `{auth_arm}` is not in the `{world}` world's vocabulary")]
    AuthArmIsNotInTheWorldVocabulary { auth_arm: String, world: String },
    #[error("emits names `{0}` but the manifest does not declare the `host_signed` arm")]
    EmitsRequireTheHostSignedArm(String),
    #[error(
        "a host-signed component's requests are all finalized; `host_signed` admits no other arm"
    )]
    HostSignedAdmitsNoOtherArm,
    #[error("a host-signed declaration must name at least one header")]
    HostSignedNamesNoHeader,
    #[error("`{0}` is not a signed header the finalizer vocabulary permits")]
    EmitIsNotASignedHeader(String),
    #[error("a host-signed declaration must not name the same header twice; `{0}` repeats")]
    HostSignedNamesAHeaderTwice(String),
    #[error("secret_headers names `{0}` but the manifest does not declare the `header_secret` arm")]
    SecretHeadersRequireTheHeaderSecretArm(String),
    #[error("secret_headers lists {0} names; at most 8 are allowed")]
    TooManySecretHeaders(usize),
    #[error(
        "secret header `{0}` must be 1 to 64 bytes of lowercase letters, digits and RFC 9110 \
         token symbols"
    )]
    InvalidSecretHeaderName(String),
    #[error("`{0}` is reserved for another purpose and cannot be declared as a secret header")]
    SecretHeaderIsReserved(String),
    #[error("secret_headers must not name the same header twice; `{0}` repeats")]
    SecretHeaderDeclaredTwice(String),
    #[error("components have no network; the host makes every request")]
    NetworkPermissionDenied,
    #[error("components have no file system")]
    FilesystemPermissionDenied,
    #[error("`{0}` is not a credential reference name; declare a name, never a credential")]
    SecretIsNotAReferenceName(String),
    #[error("every provider component must support `chat`")]
    ChatCapabilityRequired,
    #[error(
        "every task component must support `{missing}`; the three lifecycle \
         stages are mandatory and only `artifact_fetch` is optional"
    )]
    TaskLifecycleCapabilityRequired { missing: String },
    #[error(
        "usage_evidence is a provider-world declaration; other worlds meter through their own \
         contracts"
    )]
    UsageEvidenceIsAProviderWorldDeclaration,
    #[error("request_facts is a provider-world declaration")]
    RequestFactsIsAProviderWorldDeclaration,
    #[error("request_facts for family `{family}`: {detail}")]
    InvalidRequestFacts { family: String, detail: String },
    #[error("credentials: {0}")]
    InvalidCredentials(String),
    #[error("signing: {0}")]
    InvalidSigning(String),
    #[error("stream_framing is a provider-world declaration")]
    StreamFramingIsAProviderWorldDeclaration,
    #[error("endpoint and config_schema are provider-world declarations")]
    EndpointIsAProviderWorldDeclaration,
    #[error("endpoint or config_schema for family `{family}`: {detail}")]
    InvalidEndpoint { family: String, detail: String },
    #[error("a provider component must declare at least one provider family")]
    ProviderFamilyRequired,
    #[error("provider family `{0}` must be one lowercase kebab-case component")]
    InvalidProviderFamily(String),
    #[error(
        "conformance.required_suite `{declared}` is not the suite the `{world}` world is judged \
         by (`{expected}`)"
    )]
    ConformanceSuiteIsNotTheWorldSuite { declared: String, world: String, expected: String },
    #[error("conformance.fixtures is empty")]
    MissingFixtures,
    #[error("conformance.fixtures `{0}` must be a normalized relative package path")]
    InvalidFixturesPath(String),
    #[error(
        "compatibility.wit_package `{declared}` is not the `{world}` world's package \
         (`{expected}`)"
    )]
    WitPackageIsNotTheWorldPackage { declared: String, world: String, expected: String },
    #[error("compatibility.ir_schema_id `{0}` is not `token-station-protocol@<x.y.z>/v<x.y.z>`")]
    InvalidIrSchemaId(String),
    #[error("compatibility.kernel_version `{0}` is not a `major.minor.patch` triple")]
    InvalidKernelVersion(String),
    #[error("compatibility.kernel_revision `{0}` is not a 40-hex commit")]
    InvalidKernelRevision(String),
    #[error("compatibility.south_runtime `{0}` is not a `major.minor.patch` triple")]
    InvalidSouthRuntimeVersion(String),
    // B7a (query, quota, user-agent).
    #[error("{0} is a provider-world declaration")]
    InstanceIsAProviderWorldDeclaration(String),
    #[error("query parameter `{name}`: {detail}")]
    InvalidQueryParameter { name: String, detail: String },
    #[error("quota header `{header}`: {detail}")]
    InvalidQuotaHeader { header: String, detail: String },
    #[error("user_agent for family `{family}`: {detail}")]
    InvalidUserAgent { family: String, detail: String },
}

// -- compatibility admission -------------------------------------------------

/// What the admitting host was built against, for the tuple handshake.
///
/// The manifest-side constants (`wit_package`, world name, suite name) are
/// already exact-validated by `accepts_manifest` (in the conformance crate,
/// which this one deliberately does not depend on); these four are the values
/// only a live host knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostExpectationsV1 {
    pub ir_schema_id: String,
    pub kernel_version: String,
    pub kernel_revision: String,
    pub south_runtime: String,
}

/// Why the compatibility handshake refused a component.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CompatibilityMismatchV1 {
    #[error("component was built against IR `{declared}`; this host distributes `{expected}`")]
    IrSchema { declared: String, expected: String },
    #[error("component was built against kernel `{declared}`; this host distributes `{expected}`")]
    KernelVersion { declared: String, expected: String },
    #[error("component pins kernel revision `{declared}`; this host distributes `{expected}`")]
    KernelRevision { declared: String, expected: String },
    #[error("component was verified with south runtime `{declared}`; this host runs `{expected}`")]
    SouthRuntime { declared: String, expected: String },
}

/// The runtime ABI epoch this south release speaks (§8.3). Incremented only on
/// an incompatible change to the loader, the sandbox or WIT semantics, which
/// needs a design record.
pub const RUNTIME_ABI: u32 = 1;

/// What an admitting host accepts, as ranges and sets rather than one exact
/// tuple (B3, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §8.3, §8.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRangeV1 {
    /// Must equal the component's `runtime_abi`.
    pub runtime_abi: u32,
    /// The oldest south runtime a component may declare. A host raises it to
    /// enforce a release that tightened gate ① or ② (§8.6).
    pub south_runtime_min: String,
    /// The south runtime this host links; a component may not declare a newer
    /// one, which could rely on fields or semantics this runtime lacks.
    pub south_runtime: String,
    /// The kernel contract numbers this host distributes; a component must
    /// declare exactly these.
    pub kernel_contracts: BTreeMap<String, u32>,
    /// For each south contract, the versions this host's codecs decode.
    pub contracts: BTreeMap<String, BTreeSet<u32>>,
}

/// Why the range handshake refused a component.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CompatibilityMismatchV2 {
    #[error(
        "component declares no runtime_abi; it was built before the range handshake and only an \
         exact-tuple host can load it"
    )]
    MissingRuntimeAbi,
    #[error("component was built for runtime ABI {declared}; this host speaks {expected}")]
    RuntimeAbi { declared: u32, expected: u32 },
    #[error("compatibility.south_runtime `{0}` is not a `major.minor.patch` triple")]
    InvalidSouthRuntime(String),
    #[error(
        "component was verified with south runtime {declared}, older than this host's minimum \
         {minimum}"
    )]
    SouthRuntimeBelowMinimum { declared: String, minimum: String },
    #[error("component was verified with south runtime {declared}, newer than this host's {host}")]
    SouthRuntimeAboveHost { declared: String, host: String },
    #[error(
        "component declares kernel contract `{name}` as {declared:?}; this host distributes \
         {expected:?}"
    )]
    KernelContract { name: String, declared: Option<u32>, expected: Option<u32> },
    #[error("component speaks `{name}` contract {declared}; this host decodes {accepted:?}")]
    Contract { name: String, declared: u32, accepted: Vec<u32> },
}

fn version_triple(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let triple =
        (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    parts.next().is_none().then_some(triple)
}

/// Gate ①, range half: the component's declaration must fall inside what the host accepts (§8.3).
///
/// `world`, WIT package and suite stay exact
/// ([`ComponentManifestV1::validate`]); `ir_schema_id`, `kernel_version` and
/// `kernel_revision` are provenance only.
///
/// # Errors
///
/// Returns the first [`CompatibilityMismatchV2`] found.
pub fn compatibility_admits(
    manifest: &ComponentManifestV1,
    host: &HostRangeV1,
) -> Result<(), CompatibilityMismatchV2> {
    let declared = &manifest.compatibility;
    let abi = declared.runtime_abi.ok_or(CompatibilityMismatchV2::MissingRuntimeAbi)?;
    if abi != host.runtime_abi {
        return Err(CompatibilityMismatchV2::RuntimeAbi {
            declared: abi,
            expected: host.runtime_abi,
        });
    }
    let parse = |version: &str| {
        version_triple(version)
            .ok_or_else(|| CompatibilityMismatchV2::InvalidSouthRuntime(version.to_owned()))
    };
    let component = parse(&declared.south_runtime)?;
    if component < parse(&host.south_runtime_min)? {
        return Err(CompatibilityMismatchV2::SouthRuntimeBelowMinimum {
            declared: declared.south_runtime.clone(),
            minimum: host.south_runtime_min.clone(),
        });
    }
    if component > parse(&host.south_runtime)? {
        return Err(CompatibilityMismatchV2::SouthRuntimeAboveHost {
            declared: declared.south_runtime.clone(),
            host: host.south_runtime.clone(),
        });
    }
    for name in declared.kernel_contracts.keys().chain(host.kernel_contracts.keys()) {
        let (component, expected) =
            (declared.kernel_contracts.get(name), host.kernel_contracts.get(name));
        if component != expected {
            return Err(CompatibilityMismatchV2::KernelContract {
                name: name.clone(),
                declared: component.copied(),
                expected: expected.copied(),
            });
        }
    }
    for (name, version) in &declared.contracts {
        let accepted = host.contracts.get(name);
        if !accepted.is_some_and(|accepted| accepted.contains(version)) {
            return Err(CompatibilityMismatchV2::Contract {
                name: name.clone(),
                declared: *version,
                accepted: accepted.map(|set| set.iter().copied().collect()).unwrap_or_default(),
            });
        }
    }
    Ok(())
}

/// Gate ①, tuple half: the manifest's compatibility declaration must equal
/// what the admitting host was built against.
///
/// Superseded by [`compatibility_admits`] (§8.4); kept for one release so a
/// host can move at its own pace. Refusal, never silent
/// degradation, and never a partial acceptance.
///
/// # Errors
///
/// Returns the first [`CompatibilityMismatchV1`] found, in tuple order.
pub fn compatibility_matches(
    manifest: &ComponentManifestV1,
    expectations: &HostExpectationsV1,
) -> Result<(), CompatibilityMismatchV1> {
    let declared = manifest.compatibility_tuple();
    if declared.ir_schema_id != expectations.ir_schema_id {
        return Err(CompatibilityMismatchV1::IrSchema {
            declared: declared.ir_schema_id.to_owned(),
            expected: expectations.ir_schema_id.clone(),
        });
    }
    if declared.kernel_version != expectations.kernel_version {
        return Err(CompatibilityMismatchV1::KernelVersion {
            declared: declared.kernel_version.to_owned(),
            expected: expectations.kernel_version.clone(),
        });
    }
    if declared.kernel_revision != expectations.kernel_revision {
        return Err(CompatibilityMismatchV1::KernelRevision {
            declared: declared.kernel_revision.to_owned(),
            expected: expectations.kernel_revision.clone(),
        });
    }
    if declared.south_runtime != expectations.south_runtime {
        return Err(CompatibilityMismatchV1::SouthRuntime {
            declared: declared.south_runtime.to_owned(),
            expected: expectations.south_runtime.clone(),
        });
    }
    Ok(())
}
