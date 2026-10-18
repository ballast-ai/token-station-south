# Family model parameters and unsigned thinking: provider-world manifest declarations

Status: accepted (every open point ruled by lv on 2026-10-18; §9). Implemented (#272); released in 0.74.0. Requested by lv on 2026-10-18 (host dialect de-hardcoding record, DH-Q2 = A: "south adds the
manifest fields, releases them, the host consumes them, south writes its own English record"). Rulings relayed from lv on 2026-10-18 (by token-station-server-14, confirmed by the user) are marked in §9; open
points remain there.

Date: 2026-10-18

Baseline: south `origin/main` = `7bceecfa` (release 0.73.1). Host references carry a `server:` prefix and refer to
token-station-server `dev-v2` = `bb3dd4f2`, read on 2026-10-18; paths are under
`gateway/src/modules/inference/` unless they say otherwise. The host record is
the server's 2026-10-17 dialect de-hardcoding plan under `server:docs/product-review-v2/plans/` (the DH record; DH3, DH4). Statements
marked **unverified** come from reading code, not from running anything.

## 1. Problem

lv's goal is that a dialect plugs in without a host rebuild. After the host's S7-5 cleanup, three production places
in the host still compare a south family name, and two host tables are compiled in from south:

| # | Host place | What it decides |
|---|---|---|
| H1 | `server:…/engine/text_admission/reasoning_replay.rs:135-143` `ReplayLeg::of`: `dialect == "anthropic"` | Which Responses replay carriers a row accepts. On the `anthropic` family an OpenAI opaque reasoning item (`encrypted_content` without the `tsr.` marker) is refused and the next candidate is tried. On every other family it is accepted when the model declares `reasoning_replay.openai.v1`. A Claude carrier (`tsr.c1.`) is accepted on every family when the model declares `reasoning_replay.claude.v1`. |
| H2 | `server:…/sender/responses.rs:553` `responses_request_ir(body, dialect != "anthropic")` | Whether thinking parts on unmarked assistant turns that carry tool calls stay in the IR. They are kept on every family except `anthropic`, where they are stripped, together with thinking on every unmarked turn without tool calls (which every family strips). |
| H3 | `server:…/engine/south_component.rs:1847-1864` `openai_compatible_model_capability`: `dialect == "openai-compatible"` | Only that family gets a `ModelCapability` for the target model, and with the host's `upstream_requires_reasoning_content` model setting the word `reasoning_content`. `provider-openai-compatible` emits `reasoning_content` on assistant turns only for a model that declares it (`reference.rs:320-331`). |
| H4 | `server:gateway/src/infra/config/capabilities.rs:497-583` | Save-time validation of a model's `supported_parameters`. The host imports `anthropic_dialect::DIALECT_PARAMETERS` and hard-codes the `anthropic.` prefix. Words under that prefix must be one of the six. The cache-point namespace comes from `declared_parameters::CACHE_POINT_PARAMETERS`, and the `reasoning_replay.` words are a host constant. |
| H5 | `server:…/engine/south_component.rs:1810-1838` `claude_model_capability` | Passes the declared words to the component when one is a Claude dialect word or a cache-point word, and pre-refuses a forced tool choice with `anthropic_dialect::refuses_forced_tool`. |
| H6 | `server:…/engine/request_extras.rs:38-46` `HOST_MANAGED_HEADERS` | `accept`, `anthropic-version`, `content-type`, `traceparent`, `tracestate`, `user-agent`, `x-request-id`. `anthropic-version` is the one vendor name. |

The DH record's inventory (§1 #7) also listed `chatgpt-account-id` and `x-dashscope-async` in H6. The host list at
`bb3dd4f2` no longer carries them, so this record declares nothing for them.

For H1–H5 the knowledge belongs to a package, but the host holds it, keyed by a family name or compiled from a
south module. A new dialect with its own words, or a family that wants the Anthropic replay rule, needs a host edit.

## 2. Declaration 1: `model_parameters`

Each family's words that its component reads from `ProviderConfig.models[].supported_parameters`:

```json
"model_parameters": {
  "anthropic": {
    "words": ["anthropic.effort.xhigh", "anthropic.sampling.exclusive", "anthropic.sampling.none",
              "anthropic.thinking.adaptive", "anthropic.thinking.budget", "anthropic.tool_choice.auto_only",
              "reasoning_replay.claude.v1"],
    "exclusive": [["anthropic.thinking.adaptive", "anthropic.thinking.budget"]]
  }
}
```

`ComponentManifestV1::model_parameters: BTreeMap<String, ModelParametersV1>`, `#[serde(default)]`, omitted when
empty. `ModelParametersV1 { words: Vec<String>, exclusive: Vec<Vec<String>> }`, `deny_unknown_fields`; `exclusive`
defaults to empty and is omitted when empty. The object form (rather than a bare word list) carries MP-Q5's ruling:
the host keeps its save-time check that a model declares at most one word of each exclusive group, read from here
instead of from a dialect name.

| Property | Rule | Precedent |
|---|---|---|
| Keyed by | provider family; each key a family the manifest declares | `immutable_body_paths` |
| Worlds | provider world only (`WorldSchemaV1::request_declarations`); elsewhere gate ① refuses it | `immutable_request_headers` |
| Word syntax | 1 to 64 bytes of `[a-z0-9_]` segments joined by `.`; no empty segment | — |
| Order and repeats | sorted, no repeats (so a manifest has one spelling) | — |
| `exclusive` | each group 2 or more distinct words, each in `words`, sorted; groups sorted, disjoint; at most 16 groups | — |
| Count | at most 64 words per family | `MAX_IMMUTABLE_REQUEST_HEADERS` |
| Gate ① error | `ManifestErrorV1::InvalidModelParameters { family, detail }` | `InvalidImmutableRequestHeaders` |
| Public helper | `ModelParametersV1::is_valid`, `MAX_MODEL_PARAMETERS`, `MAX_MODEL_PARAMETER_BYTES`, `MAX_MODEL_PARAMETER_GROUPS` | `are_immutable_request_headers` |

Semantics, for a host:

1. **The words are complete for what they name.** The list holds every `supported_parameters` word the family's
   component reads, whether dotted (`anthropic.thinking.adaptive`, `bedrock.converse.cache_point`,
   `reasoning_replay.claude.v1`) or bare (`reasoning_content`, `reasoning_effort`). A component that reads a word it
   does not declare is a package bug; the package tests pin it (§7). One deliberate exception, for byte identity
   (MP-Q3): `provider-openai-compatible` reads `reasoning_content` on every family, but only `openai-compatible`
   declares it, so a host keeps deriving it for that family alone.
2. **Reserved namespaces come from the declarations.** A dotted word's namespace is its first segment. A host treats
   the namespace of every declared dotted word of every loaded package as reserved: a model word in a reserved
   namespace must be declared by some loaded package, or it is refused at save time. This keeps today's typo
   protection (`anthropic.thinking.adaptve` is refused) without the host naming `anthropic.`. The `speech.` words
   stay a `south-contracts` vocabulary (`SPEECH_DIALECT_WORDS`), because the speech world has no
   `request_declarations` (MP-Q4).
3. **Host-derived words go only to families that declare them.** A host that derives a word from its own model
   settings (today `reasoning_content` from `upstream_requires_reasoning_content`) adds it to the target's capability
   only when the row's family declares that word.
4. **A host builds a model capability where it did before, by declaration instead of by name.** It builds the
   `ModelCapability` for the target model on a family that declares a host-derived word (today `reasoning_content`,
   so `openai-compatible` alone), and as it does today when the model declares words of its own. It does **not**
   build an empty one for every declaring family. A missing entry and an empty one are different inputs:
   `wants_reasoning_content` (`reference.rs:328-333`) keeps the DeepSeek name heuristic for a missing entry and reads
   an empty one as a declared "no", so an empty capability would drop `reasoning_content` from a `deepseek…` model
   on the six other OpenAI-compatible families (found by the host on 2026-10-18). `family_model_parameters_v1` pins
   both: the same request for an ordinary model, a different one for `DeepSeek-R1`. `Dialect::of` returns the
   default dialect for both by construction.
5. **Replay carriers follow the declaration.** A family accepts a carrier only when it declares that carrier's model
   word: an OpenAI opaque item needs `reasoning_replay.openai.v1`, a Claude carrier needs `reasoning_replay.claude.v1`.
   The model must still declare the word as well. This replaces H1's family name. On `anthropic` it is byte-identical,
   because the family does not declare the OpenAI word. Elsewhere it refuses one misconfiguration earlier, as
   incompatible rather than as a component capability error: a model on, say, an `openai-compatible` row that declares
   `reasoning_replay.claude.v1`. **Difference registered as MP-D1.**
6. **Exclusive groups.** A model's `supported_parameters` holding two words of one declared group is refused at save
   time. The component still refuses the combination at request time (`Dialect::of`).
7. Declaring a word changes nothing a component builds. The component reads `supported_parameters` exactly as before.

## 3. Declaration 2: `drops_unsigned_thinking`

The families whose upstream refuses an unsigned thinking block in an earlier assistant turn:

```json
"drops_unsigned_thinking": ["anthropic"]
```

`ComponentManifestV1::drops_unsigned_thinking: BTreeSet<String>`, `#[serde(default)]`, omitted when empty; each entry
a family the manifest declares (`ManifestErrorV1::InvalidDropsUnsignedThinkingFamily` names one that is not); provider
world only. The shape follows the precedent of
`subscription_quota_families`.

Semantics: for a declared family, a host removes thinking and redacted-thinking parts from every assistant turn
that carries no replay marker, before the IR reaches the component. For any other family it keeps them on turns that
carry tool calls. This is H2 with the family name replaced by the declaration. On the host's Responses surface it is
byte-identical. The host applies it on the Responses surface only, as H2 does today (MP-Q2, ruled).

The alternative is that `provider-anthropic` drops unsigned thinking itself. Its `assistant_part_to_block` today
writes a thinking block without a signature when the model does not declare replay (`reference_anthropic.rs:289-303`).
That would need no field, but it changes the package's requests on every host surface, so it needs its own
measurement (MP-Q1).

## 4. `provider-anthropic` protects `anthropic-version` on `anthropic`

The `anthropic` family declares `immutable_request_headers: {"anthropic": ["anthropic-version"]}`. Its requests always
carry the header (`reference_anthropic.rs:1166`). With this declaration a host can drop the vendor name from H6
without opening the header to request extras. This revises PRH-Q3 of the protected request headers record
(`2026-10-15-immutable-request-headers.md` §8: "`anthropic` declares nothing"), whose reason was that the host floor
already covered the header. The credential headers (`x-api-key`, `authorization`) stay covered by south's reserved
headers, which every host applies (`SafeHeaders`).

## 5. The forced-tool pre-refusal: DH-Q4 needs a component change first

DH-Q4 = A deletes the host's pre-refusal (H5) and leaves the refusal to the components. Failover does not depend on
the pre-refusal: a `ToolChoiceUnsupported` from `seal_target` is fatal on Chat (`chat.rs:206-230`), Messages
(`messages.rs:211-224`) and Responses (`sender/responses.rs:90-94`, `:340`), and only a replay mismatch moves to the
next candidate.

The precondition of A does not hold everywhere, though (found by the host on 2026-10-18):

- `provider-anthropic` and `provider-bedrock-converse` refuse exactly what the host pre-refuses
  (`anthropic_dialect.rs:204-213`).
- `provider-kiro` does not. A forced choice has no slot on its wire, so it sends the tools as for `auto`, and its
  comment relies on the host's pre-refusal (`reference_kiro.rs:357-361`).
- `provider-openai-compatible` does not read `anthropic.tool_choice.auto_only` at all. A Claude model behind an
  OpenAI-compatible family (e.g. `github-copilot`) that declares the word is refused today by the host alone.

So deleting H5 first would let a forced choice through on those families. **lv ruled on 2026-10-18 (DH-Q4
revisited):** the host keeps H5 until it re-pins to the release in which every family that may serve a Claude model
refuses a forced choice itself, and that work joins this release. The implementation goes through every provider
family. Each one either refuses a forced choice when the model declares `anthropic.tool_choice.auto_only`, pinned by
a test. lv ruled on 2026-10-18 that no family is exempt by its wire. The first draft exempted `gemini`, `vertex-ai`
and the three `provider-openai-responses` families as unable to serve a Claude model, but that was argued from the
wire, not measured. Done in this release:

- `provider-kiro`: read `anthropic.tool_choice.auto_only` and refuse a forced choice with the capability error
  `anthropic_dialect` uses. Its `model_parameters` gains the word.
- `provider-openai-compatible`: the same, on every family. **Unverified** whether any of its families should honor
  the other five Claude words; this record adds only the refusal (MP-Q7).
- `provider-gemini`, `provider-vertex` and `provider-openai-responses` (all three families, `bedrock-mantle` and
  `openai-codex` included): the same refusal, and the word in their `model_parameters`.
- `provider-anthropic-bedrock-invoke` already refuses: it builds its body through `provider-anthropic`'s
  `checked_body_of`.

The refusal is shared: `anthropic_dialect::refuse_declared_forced_tool`, with the message `Dialect::refuse_forced_tool`
gives. `anthropic_dialect_v1` pins every provider family of every provider package on a declared and an undeclared
model.

The refusal moves from the host to the component unchanged in meaning; one client-visible difference is left to the
host: the error code changes from the host's `tool_choice_unsupported` to the component's capability error, both
HTTP 400. `refuses_forced_tool` and `DIALECT_PARAMETERS` stay exported, as removing them is not needed for this
record.

## 6. Shipped declarations

Proposed below; each list is pinned by a test against the words its reference module reads (§7). A family not listed
declares nothing.

| Package | Family | `model_parameters` | `drops_unsigned_thinking` |
|---|---|---|---|
| `provider-anthropic` | `anthropic`, `claude-code`, `claude-code-setup-token` | the six `anthropic.*` words, `reasoning_replay.claude.v1` | `anthropic` only (MP-Q3) |
| `provider-bedrock-converse` | `bedrock` | the six `anthropic.*` words, the three `bedrock.converse.cache_point*` words, `reasoning_replay.claude.v1` | — |
| `provider-bedrock-converse-bearer` | `bedrock-bearer` | as `bedrock` | — |
| `provider-anthropic-bedrock-invoke` | `anthropic-bedrock-invoke` | as `anthropic` (it builds through `provider-anthropic`'s `checked_body_of`) | — |
| `provider-openai-compatible` | `openai-compatible` | `anthropic.tool_choice.auto_only` (§5), `reasoning_content`, `reasoning_effort` | — |
| `provider-openai-compatible` | the other six families | `anthropic.tool_choice.auto_only` (§5), `reasoning_effort` (MP-Q3) | — |
| `provider-kiro` | `kiro` | `anthropic.tool_choice.auto_only` (§5) | — |
| `provider-openai-responses` | all three families | `anthropic.tool_choice.auto_only` (§5), `reasoning_effort`, `reasoning_replay.openai.v1` | — |
| `provider-gemini`, `provider-vertex` | `gemini`, `vertex-ai` | `anthropic.tool_choice.auto_only` (§5) | — |

Declaring `reasoning_content` only on `openai-compatible` keeps H3 byte-identical. Two deliberate changes follow from
the table, both registered:

- **MP-D2.** The six other `provider-openai-compatible` families and the three `provider-openai-responses` families
  declare `reasoning_effort`. **No request changes, because a host builds no new capability for them (§2 item 4).**
  The first draft let every declaring family get one, and argued it changed nothing because
  `reasoning_effort_allowed` (`reference.rs:570-580`) answers a missing and an empty entry alike. That holds for
  `reasoning_effort` but not for `reasoning_content`. The host found that on 2026-10-18, and §2 item 4 was corrected.
- **MP-D1.** See §2 item 5.

## 7. Compatibility, versioning, tests

- **No contract number changes.** These are manifest fields only. No WIT, IR or `contracts.*` number moves, as for
  `immutable_request_headers` (0.67.0).
- **Runtime.** `ComponentManifestV1` is `deny_unknown_fields`, so a runtime before the release that ships the fields
  refuses a package that declares them. A host on an older south fails closed. Following the 0.60.0 and 0.67.0
  precedent, the feature PR leaves `south_runtime` as it is, and the release commit raises it on the nine declaring
  packages together with `shipped_packages_v1::declared_runtime_now`.
- **Q47.** `south-provider-api` 0.61.0 → 0.62.0 (minor: two public fields, a public struct, two `ManifestErrorV1`
  variants, three constants). `south-component-conformance` 0.72.0 → 0.73.0 (the exported word lists, the shared
  refusal, and the refusals in `kiro`, OpenAI-compatible, Gemini, Vertex and OpenAI Responses). Every package's
  `component.wasm` re-identifies. All nine provider packages declare and take a minor (`provider-anthropic` 1.4.0,
  `provider-anthropic-bedrock-invoke`, `provider-bedrock-converse`, `provider-bedrock-converse-bearer`,
  `provider-kiro` and `provider-vertex` 1.2.0, `provider-gemini` 1.5.0, `provider-openai-compatible` 2.12.0,
  `provider-openai-responses` 1.5.0). The section attributes record makes `task-gmi-image-v2` 0.36.0 in the same
  release, and the other 31 packages take a patch.
- **Release.** 0.74.0.
- **Tests.**
  - `south-provider-api` `tests/model_parameters_v1.rs`: absent is omitted, round-trip, an unknown key inside a
    declaration refused, undeclared families refused by name (both fields), the word grammar and bounds at and past
    the limit, unsorted and repeated words refused, the exclusive-group rules and bound; both fields refused outside
    the provider world (embeddings, task, image and speech).
  - `south-component-conformance`: `declared_parameters::family` holds each family's list, unit-tested against the
    constants of the modules that read the words (`anthropic_dialect`, `CACHE_POINT_PARAMETERS`,
    `responses_vocabulary`). `tests/family_model_parameters_v1.rs` pins every provider manifest to those lists, only
    `anthropic` in `drops_unsigned_thinking`, `anthropic`'s `anthropic-version`, and a missing and an empty model
    entry building the same request (§2 item 4). `tests/anthropic_dialect_v1.rs` pins the forced-tool refusal on
    every Claude-serving family (§5). A test that every word a module reads is declared is not written: reading is
    not observable generically, so completeness rests on the per-module constants and review.

## 8. Host checklist (after re-pinning to 0.74.0)

1. H4: derive reserved namespaces and admitted words from the loaded packages' `model_parameters` (§2 item 2). Drop
   `ANTHROPIC_DIALECT_PREFIX`, the `DIALECT_PARAMETERS` and `CACHE_POINT_PARAMETERS` imports and the
   `reasoning_replay.` constant. Keep the save-time adaptive/budget exclusivity check, judged by the declared words
   instead of a dialect name (MP-Q5, ruled): south exports the exclusive pair beside the words (§7).
2. H3, H5: build the capability per §2 items 3–4, and delete `openai_compatible_model_capability`'s name comparison.
   Delete the forced-tool pre-refusal (DH-Q4) only after re-pinning to the release in which `provider-kiro` and
   `provider-openai-compatible` refuse themselves (§5).
3. H1: `leg_accepts` reads the family's declared replay words (§2 item 5). `ReplayLeg` loses its name-keyed variant.
4. H2: `responses_request_ir` takes `drops_unsigned_thinking.contains(family)`.
5. H6: remove `anthropic-version` from `HOST_MANAGED_HEADERS`. Stored extras on that name for `anthropic` rows are
   then skipped at run time by the existing immutable-header skip.
6. Add the `InvalidModelParameters` arm; struct literals gain the two fields.

## 9. Rulings

- **MP-Q1** Unsigned thinking: (a) the `drops_unsigned_thinking` declaration, which is byte-identical, or (b)
  `provider-anthropic` drops unsigned thinking itself, so no field is needed but the requests change on every
  surface. **Recommended, and ruled by lv on 2026-10-18: (a)**; (b) can follow after measurement.
- **MP-Q2** Should the host apply `drops_unsigned_thinking` on Chat and Messages too? **Ruled (lv, 2026-10-18): no.**
  Responses only, as today; only the family name is replaced by the declaration.
- **MP-Q3** Identity versus intent for `claude-code`, `claude-code-setup-token` and `anthropic-bedrock-invoke`.
  Today they are not in H2's strip, and only `openai-compatible` gets `reasoning_content` in H3, not `glm-coding` or
  `kimi-code`. **Recommended, and ruled by lv on 2026-10-18:** ship the byte-identical table now, and widen each list in its own change with a
  measurement.
- **MP-Q4** `speech.` words stay a `south-contracts` vocabulary. **Recommended, and ruled by lv on 2026-10-18: yes**; they are not dialect words.
- **MP-Q5** The adaptive/budget mutual exclusion is a Claude rule the host checks at save time. **Ruled (lv,
  2026-10-18): keep the save-time check**, judged by declared words rather than a dialect name. South therefore
  exports the exclusive pairs as data the host can read without naming `anthropic` (exact form left to the
  implementation; the simplest is a per-family `exclusive` list in `model_parameters`, which would change §2's shape,
  so it is settled before code).
- **MP-Q7** `provider-kiro` and `provider-openai-compatible` take only the forced-tool refusal (§5), not the other
  Claude words. **Recommended, and ruled by lv on 2026-10-18: yes**; the other words reshape requests and need their own measurement. The
  alternative is DH-Q4 option B, a declared `refuses` rule, but that is a second field for one word.
- **MP-Q6** Field names. **Recommended, and ruled by lv on 2026-10-18:** `model_parameters` and `drops_unsigned_thinking`. The DH record's working
  name `reasoning_replay` is not used, because the replay carriers are covered by `model_parameters` (§2 item 5).

## Revision note

- 2026-10-18: first draft. Not implemented, built or tested.
- 2026-10-18: §5 corrected. `provider-kiro` and `provider-openai-compatible` do not refuse a forced choice (the
  host found it); they now take the refusal before H5 is deleted. MP-Q7 added.
- 2026-10-18: lv's rulings recorded (relayed by token-station-server-14, confirmed by the user). DH-Q4 revisited: the
  host keeps its pre-refusal until it re-pins to the release in which every family that may serve a Claude model
  refuses a forced choice itself; that work joins this release (§5). MP-Q2: Responses only. MP-Q5: keep the save-time
  exclusivity check, judged by declared words.
- 2026-10-18: §2 takes the object form `{words, exclusive}` for MP-Q5 (still one field); lv adopted the shape the same
  day (relayed by token-station-server-14, confirmed by the user). lv ruled MP-Q1, Q3, Q4, Q6 and Q7 as recommended (relayed by token-station-server-14, confirmed by the
  user). Status accepted. Per MP-Q3, the follow-ups each get their own change with a measurement: `claude-code`,
  `claude-code-setup-token` and `anthropic-bedrock-invoke` into `drops_unsigned_thinking`; `reasoning_content` for
  `glm-coding` and `kimi-code`.
- 2026-10-18 (after review by the host): the following changes, relayed by token-station-server-14 and confirmed by
  the user.
  - §2 item 4 and MP-D2 corrected: a missing and an empty capability differ on `deepseek…` models.
  - §5: lv ruled that `gemini`, `vertex-ai` and the three `provider-openai-responses` families refuse a forced choice
    too, in this release; `provider-gemini` and `provider-vertex` therefore take a minor.
  - lv accepted that reserved namespaces and exclusive groups come only from loaded packages, and that the whole
    `bedrock.` namespace is reserved by the cache-point words.
