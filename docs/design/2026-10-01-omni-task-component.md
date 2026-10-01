# The Gemini Omni video task component and task contract 8 (inline artifacts, token buckets)

Status: proposed — drafted for review by the host team (token-station-server P21 / P26), not accepted

Date: 2026-10-01

Predecessors: `2026-09-18-task-adapter-world.md` and `2026-09-20-task-adapter-v2-candidate.md` (the task world, its
seven operations, the locator and `SubmitOutcomeV2`), `2026-09-27-task-contract-v6-facts.md` (request estimates,
`fetch_with_credential`, `immutable_body_paths`), `2026-09-28-task-contract-v7-artifact-role.md` (artifact roles; the
host counts and delivers primary artifacts only), `2026-09-30-host-zero-vendor-boundary.md` (this record depends on its
§3 credential recipes and trust model, §5.2 `decode_sse_v1`, §6.3 host-computed bounds, §6.4 `rejected`, §7.3 endpoint
templates, §7.5 catalog data and §8 compatibility ranges), `2026-09-30-image-world.md` (its §6 elision vocabulary —
`elide_v1`, blob placeholders, `$south.artifact` nodes — and its §9.1 token buckets and declared-bucket rule, both
reused here rather than redefined).

Origin: token-station-server plan P26 (`docs/product-review-v2/plans/2026-09-30-P26-*.md`, "host plan P26" below),
step O2, with the owner's rulings of 2026-09-30: DO2 = ① (a task-world component), DO3 = unfreeze (`Prefer:
respond-async` takes effect), DO4 = A (the blocking response returns gateway artifact paths, not base64), DO5 = A
(keep bucketed token pricing through a task-contract increment and a provider-agnostic host pricing function), DO6 = B
(the native `/v1beta/interactions` surface goes offline once this component accepts image input); DO7 was left to
the O0-b sample and is answered in §4. The umbrella plan is P21 in the same directory (DP0, DP1).

Baseline: south = the revised umbrella branch (`2ff1214`, on v0.42.0 `3135e36`); host token-station-server
`61f4c712`. Host line numbers carry a `server:` prefix and refer to `gateway/src/` unless a path says otherwise;
kernel line numbers carry `kernel:` and refer to `crates/protocol/src/` at `f585bc83`. Upstream facts marked "O0-b
sample, 2026-10-01" were measured by the host team on Vertex AI (host plan P26, section "O0-b"); the Gemini API line
(`generativelanguage.googleapis.com`) was **not** sampled, because the measuring project had it disabled.

## 1. Problem

### 1.1 Today's Omni bridge

The host serves Omni through one execution arm on `/v1/video/generations`, chosen by provider type `gemini` and the
substring `omni` in the upstream model id (server:modules/inference/handler/video.rs:174-190;
server:…/handler/video/gemini.rs:486-488). The arm is synchronous: one POST to `{endpoint}/v1beta/interactions`
(gemini.rs:753-756) with `response_format.delivery = "uri"` hard-coded (:503-506), then the artifact is either taken
inline or fetched from a Files API URI with `x-goog-api-key` (:613-652) and returned as `b64_json` (:889-900). It runs
on the sealed stream-settlement path, not the task framework: no `async_tasks` row, so `Prefer: respond-async` is
ignored (video.rs:137-140) and a process that dies mid-wait leaves the call to manual review (host plan P26 §1.2,
row 1). Usage is parsed by the host from wire field names (server:…/engine/token_counter/media.rs:358-408) and priced
per modality with two pieces of Omni knowledge inside an otherwise generic function — an image-model input-price
fallback (:422-429) and "audio bills at the text rate because Omni bundles audio into the clip" (:449-452); an
unknown modality is billed at the text-output rate with a warning (:455-459). The reservation bound gives every
output modality `default_max_output_tokens` (gemini.rs:719-738; 4,096 by default).

Host plan P26 §2 classifies 321 production lines of this arm and 28 lines of its callers as provider logic; none of
it is visible to the J1 vendor-identifier gate (P26 §2.5).

### 1.2 What the O0-b samples established

All measured on Vertex AI, `POST https://aiplatform.googleapis.com/v1beta1/projects/{project}/locations/global/
interactions`, OAuth bearer from a service account (O0-b sample, 2026-10-01):

| Fact | Measured |
|---|---|
| Model | short name `gemini-omni-flash-preview`; the catalog also lists `gemini-omni-1.1-flash-preview`; both PUBLIC_PREVIEW. `:generateContent` refuses Omni ("only supported in the Interactions API") |
| Working request | `{"model", "input": "<prompt>", "response_format": {"type": "video", "delivery": "inline"}}` |
| `delivery: "uri"` | requires `gcs_uri` (a customer GCS bucket); 400 otherwise, not billed |
| Duration | no parameter exists: `response_format.duration_seconds` and `generation_config.video_config.duration_seconds` are refused as unknown. Every clip is 10 s, 1280×720, 24 fps, H.264 + AAC, ~3.7 MB mp4 |
| Response | HTTP 200 JSON after ~40 s: `id`, `status: "completed"`, `usage`, `role`, `created`, `updated`, `event_id`, `service_tier`, `object: "interaction"`, `model`, `steps[]` — one or more `{type: "thought", signature (~7.7 KB), summary[]}`, then `{type: "model_output", content: [{type: "video", mime_type: "video/mp4", data: <base64>}]}`; the `data` string is standard-alphabet base64, ~4.9 MB |
| Usage | `total_tokens`, `total_input_tokens`, `input_tokens_by_modality[]`, `total_output_tokens`, `output_tokens_by_modality[]` (one `video` entry), `total_thought_tokens`; `total_tokens = input + output + thought` in both samples (17 + 57,920 + 234 = 58,171; 1 + 57,920 + 739 = 58,660). No `total_cached_tokens` appeared |
| Video tokens | **57,920 per clip in both samples**, independent of the prompt |
| Stored interactions | with `stream: true` (and `background: true`) the answer is SSE (`interaction.created` with `status: in_progress`, `interaction.status_update`, …); every interaction is stored and `GET …/interactions/{id}` later returns the complete result, video and usage included. The GET body has no `role` / `created` / `updated` / `event_id` and starts `steps[]` with a `{type: "user_input", content: [{type: "text", text}]}` step |
| Errors | not uniform: an unknown-parameter 400 is JSON `{"error": {"message", "code": "invalid_request"}}`; the missing-`gcs_uri` 400 arrived as an SSE frame (`event: error` / `data: {"error": {…}, "event_type": "error"}`) under `Content-Type: application/json`, although the request did not ask to stream |

Consequences: P26-F1 is confirmed — every real clip (57,920 video tokens) exceeds the host's per-modality bound of
4,096, so every settlement on today's bridge would go to manual review. And the upstream offers what DO7 asked about:
a stored interaction retrievable by id.

### 1.3 What the task contract cannot express today

Option ① of host plan P26 §4.2 fits the task world except for two gaps, both visible in the contract types:

- **Artifacts**: `TaskArtifactRefV2` is `Urls`, `FileId` or `None` (crates/south-contracts/src/task_v2.rs:296-303).
  The only delivery Omni offers without a customer bucket is inline base64 in the JSON body.
- **Usage**: `TaskUsageFactsV2` carries `seconds`, `milliunits`, `tokens` and `outputs` (task_v2.rs:100-105) — one
  token count, no modality and no reasoning bucket. The host's component path prices tokens at one rate
  (server:…/handler/video/durable.rs:1293-1331). DO5 = A keeps today's per-bucket amounts, so the contract needs
  buckets.

There is a third, smaller gap: a 4.9 MB observation body handed to `parse-observation` as a JSON string
(crates/south-provider-api/wit/task-adapter-v2.wit:43-44) fits the runtime's 16 MiB payload limit
(crates/south-provider-runtime/src/runtime.rs:30-32) but contradicts the image world's rule that bytes stay out of
the sandbox (image record §6), and it would put the clip through the sandbox on every poll.

## 2. Decisions

- **D1 One new task package, Vertex first.** `task-gemini-omni-v2`, world `task-adapter-v2`, one family in v1:
  `vertex-interactions-video` (§3). The Gemini API line is deferred (not sampled, and no production row needs it:
  host plan P26 "O0-a" found no `gemini`-type provider rows).
- **D2 Submit, then poll by id (DO7).** The component submits with `background: true` and polls `GET
  interactions/{id}`; the upstream id is persisted before the first poll, so a host restart resumes polling instead
  of parking the call. Terminal-on-submit is never used for a success (§4).
- **D3 Task contract 8, additive.** Two increments, both following the umbrella §8.6 additive rule: an
  `inline` artifact form whose bytes the host holds (§6), and declared token buckets in usage (§7). The WIT does not
  change.
- **D4 Bytes never enter the sandbox.** The package declares response blob paths; the host elides those strings
  with the image world's `elide_v1` before every response reaches the component, and the component names the
  artifact by a pointer into the response (§6). It is the image world's `inline {pointer, encoding, media_type}`
  form, reused.
- **D5 Delivery through gateway paths (DO4).** The host decodes and stores the clip before settlement and serves it
  at `/v1/video/tasks/{id}/artifacts/art_N`; no second upstream hop exists, so P26-F4 disappears (§6.4, §9.3).
- **D6 Bucketed pricing is generic host policy (DO5).** The host gains one provider-agnostic bucketed token pricing
  function; neither of today's Omni fallbacks survives (§7.3).
- **D7 The reservation bound is the host's, and tight.** Per-bucket maxima per output come from catalog data
  (umbrella §7.5); for Omni: 57,920 video tokens per clip. The component may only tighten. P26-F1 is fixed by
  construction (§7.4).
- **D8 Error classification by the component, both wire shapes.** JSON error bodies and SSE `event: error` frames
  are both parsed, without trusting the content type; a 4xx is `Rejected`, a 5xx or an unreadable body is
  `Unknown` (§8).

## 3. Package identity, families and manifest

### 3.1 Identity

| Property | Value |
|---|---|
| Package | `task-gemini-omni-v2`, version 0.1.0 at first release |
| World / WIT | `task-adapter-v2` / `token-station:task-adapter@2.0.0` (unchanged) |
| Task contract | 8 (§13); the package does not decode under contract 7 |
| Suite | `south.task-component.v2`, with the rows of §10 |
| Families | v1: `vertex-interactions-video`. Deferred: `gemini-interactions-video` (§3.3) |
| Reference implementation | `reference_gemini_omni_task_v2.rs` in `south-component-conformance`, transcribed from the O0-b samples, not from the host bridge (the bridge speaks `delivery: uri` to a different upstream) |

The family names describe the wire (Interactions video on Vertex / on the Gemini API), not a model, so a later Omni
model on the same wire is a catalog row, not a package change.

### 3.2 Why Vertex is v1

1. It is the only line with samples. Host plan P26 §5 makes samples a hard gate; a Gemini API family written from
   documentation would be guesswork in exactly the places the samples corrected (delivery, duration, error framing).
2. No production row needs the Gemini API line (P26 "O0-a": no `gemini`-type provider rows, no `omni` model rows).
3. Its cost is a dependency, not a design gap: the Vertex service-account recipe of umbrella §3.3 (phase B4) and
   an endpoint template (umbrella §7.3, phase B2) — §9 gives the stopgap that works before both land and says which
   J1 items it leaves.

### 3.3 The Gemini API family (deferred)

Adding it needs a sample (the request and response may differ; the Files API `uri` delivery certainly does) and
an auth arm the Vertex family does not use (`header_secret` `x-goog-api-key`). Umbrella R6 says "a family that needs
a different auth arm from its siblings also belongs to its own package", while the OpenAI-compatible reference
already emits bearer or `api-key` per family inside one package (umbrella §4.1). Until that reading is settled
(Q-S1), this record does not pre-commit: the deferred family is either a second family here or a sibling package.

### 3.4 Manifest sketch

```json
{
  "name": "task-gemini-omni-v2",
  "version": "0.1.0",
  "api_version": "task-adapter-v2",
  "providers": ["vertex-interactions-video"],
  "capabilities": ["submit", "observe", "render"],
  "auth_arms": ["bearer"],
  "permissions": { "network": false, "filesystem": false, "secrets": ["provider_api_key"] },
  "response_blob_paths": ["/steps/*/content/*/data"],
  "token_buckets": ["text_input", "image_input", "total_input", "video_output", "total_output", "reasoning_output"],
  "required_token_buckets": ["total_input", "video_output", "total_output", "reasoning_output"],
  "credentials": { "schema": "south.credential-recipe.v1", "…": "umbrella §3.3 Vertex sketch, §9.1" },
  "endpoint": { "vertex-interactions-video":
                "https://aiplatform.googleapis.com/v1beta1/projects/{project}/locations/global" },
  "conformance": { "required_suite": "south.task-component.v2", "fixtures": "fixtures-gemini-omni-task-v2/" },
  "compatibility": { "wit_package": "token-station:task-adapter@2.0.0", "contracts": { "task": 8, "media": 1 },
                     "…": "per umbrella §8.3" }
}
```

`response_blob_paths`, `token_buckets` and `required_token_buckets` are new task-world manifest fields (§6.2, §7.1);
`credentials`, `endpoint` and the `compatibility` range are the umbrella's (§3.3, §7.3, §8.3). The fields are
package-level because the response-side functions receive no configuration (umbrella R6;
task-adapter-v2.wit:43-44, `parse-observation` takes only response parts). Absent `response_blob_paths` means no
declared elision (the fallback threshold still applies, §6.2), absent bucket lists mean the package reports no
buckets — both equal today's behavior (umbrella R5).

## 4. Lifecycle: submit, then poll by id (DO7)

### 4.1 Why not synchronous

A synchronous submit would hold the connection ~40 s (O0-b) and, on the task path, would have to report the clip as
`AcceptedTerminal` from the submit response. The host seals a terminal-on-submit observation into an encrypted
column (server:modules/tasks/component.rs:426-440) and delays the first poll so the poller does not park a task that
has "no upstream id and no sealed observation" (durable.rs:586-599). A process that dies during the 40 s has neither
and goes to manual review. With a stored interaction the upstream id is enough to recover, so the component uses the
recoverable form the task world was built for (2026-09-20-task-adapter-v2-candidate.md, "TaskLocatorV2").

### 4.2 Submit

`build-submit-request` emits `POST {base_url}/interactions` with the body of §5, which always carries
`"background": true` and never `"stream": true`. The locator route is the fixed relative path `interactions` (the
Veo package uses the same pattern with `v1beta`, reference_veo_task_v2.rs:44, :87-95).

`parse-submit-response` maps:

| Upstream answer | `SubmitOutcomeV2` |
|---|---|
| 2xx, JSON interaction with a valid `id` (§4.4), any `status` | `Accepted(id)` |
| 2xx, SSE body whose first `interaction.created` event carries a valid `id` (defensive; §4.5) | `Accepted(id)` |
| 2xx without a valid `id`, or unparsable | `Unknown` (the host keeps the reservation) |
| 4xx, either error shape (§8) | `Rejected(envelope)` |
| 5xx, anything else | `Unknown` |

**Terminal-on-submit is never used for a success.** If a 2xx submit answer is already `completed`, the component
still returns `Accepted(id)` and the first poll fetches the result: the clip then arrives on the observe path, where
§6's elision applies, and never lands in the host's sealed-observation column (which holds an observation, not
bytes). `AcceptedTerminal` is used only for a submit answer that is already `failed` and carries no `id` — a shape
not yet observed, kept so such an answer is not misread as `Unknown` (inferred; Q-L2 asks for a sample).

### 4.3 Poll

`build-observe-request` emits `GET {base_url}/interactions/{id}` (the id percent-encoded as one segment, as every
task package does). `parse-observation` maps the measured status words, and only those:

| Upstream `status` | Observation |
|---|---|
| `in_progress` | `Progress { running: true, status_word: "in_progress" }` |
| `completed`, with exactly one video part and the required buckets (§7.1) | `Succeeded { artifacts: Inline([…]), usage }` |
| `completed` without a video part, or missing a required bucket | `Unknown` — the upstream may have charged; nothing is delivered and nothing is settled |
| a failure status word with an `error` object | `Failed { kind: Failed, code, message }` |
| any other word | `Unknown` (the task world never fabricates a failure from an unrecognized word) |
| non-2xx on the GET | `Unknown` (polling continues; Veo does the same, reference_veo_task_v2.rs:326-328) |

Only `in_progress` and `completed` were observed (O0-b). The failure words (`failed`, `cancelled`, …) and whether a
content-policy refusal is a failed interaction, a 4xx at submit, or a completed interaction without video are not
known; §10 requires a fixture for each before the corresponding row is filled, and until then they are `Unknown`
(Q-L2). `TaskFailureKindV1::Cancelled` / `ProviderExpired` are not used until the wire is seen to say so
(crates/south-contracts/src/task.rs:273-283).

### 4.4 The upstream id

Measured ids are 40-character strings of ASCII letters and digits (O0-b). The component accepts a non-empty id of
at most `MAX_TASK_ID_BYTES` (128, task.rs:41) bytes drawn from `[A-Za-z0-9_-]` and refuses anything else as
`Unknown` at submit; base64url characters are admitted because the measured ids look like base64url (inferred). The
id is persisted and rendered verbatim (2026-09-20-task-adapter-v2-candidate.md, "Kling input").

### 4.5 Open measurement: `background: true` alone

O0-b measured `stream: true` (SSE) and `stream: true` with `background: true` (accepted; a paid generation), and
GET by id. It did **not** measure `background: true` without `stream`. This record assumes it answers promptly with
a JSON interaction carrying `id` and `status: in_progress` (inferred from the SSE `interaction.created` event, which
carries the same fields). The submit table tolerates an SSE answer (decoded with `decode_sse_v1`, umbrella §5.2),
but a submit that streams until completion would make the host's buffered submit wait ~40 s and read the clip
through the submit path. So one sample (O0-c, §12) is a gate before implementation; if `background` alone is
refused, the fallback is `background: true, stream: true` with the component taking the id from the first event —
which requires the host to stop reading after the first event, a new generic transport behavior that this record
does not propose (Q-L1).

## 5. Request mapping

The input is the host's unified video request (the same JSON every task package receives; durable.rs:2118-2160
writes `model` = the upstream model before calling the component).

| Unified request | Upstream body | Rule |
|---|---|---|
| `model` (host-written) | `model` | verbatim; must be a plain identifier |
| `prompt` | `input` (text) or the text part of `input` (with images) | required; 400 when missing, as today (gemini.rs:686-688) |
| `aspect_ratio` | `response_format.aspect_ratio` | verbatim, as today's bridge (gemini.rs:507-508); not exercised by O0-b (inferred to be accepted on Vertex) |
| — | `response_format.type = "video"`, `response_format.delivery = "inline"` | always |
| — | `background = true` | always (§4) |
| `duration` | — | absent or exactly 10 is accepted; any other value is a 400 capability error (the upstream has no duration parameter and always produces 10 s, O0-b). Today's bridge ignores `duration` silently |
| `n` | — | absent or 1; anything else is a 400 (one interaction yields one clip) |
| `image` / `image_url` / `first_frame` | one image part in `input` (§5.1) | after the host's input-image prefetch, as a `data:` URI (the Veo precedent, reference_veo_task_v2.rs:23-25, :103-114) |
| `reference_images` | further image parts in `input` | bounded count (Q-L3) |
| `last_frame` / `last_frame_url` / video inputs (edit) | — | 400 capability error in v1 |
| any other field | — | ignored, as today's bridge ignores it |

### 5.1 Image input (the DO6 precondition)

DO6 takes the native surface offline only after this component accepts image input. The Interactions `input` field
takes either a string (measured) or a list of content parts; the GET-by-id body echoes the prompt as a `user_input`
step with `content: [{type: "text", text}]` (O0-b), and the output video part is `{type: "video", mime_type, data}`.
By analogy the image part is `{type: "image", mime_type, data}` with standard base64 — **inferred, not measured**,
so image input ships only after an O0-c sample of an image-to-video request (§12). Images come from the host's
existing prefetch as `data:` URIs inside the task request, so their bytes do cross the sandbox, exactly as for Veo
today; moving task-world request media to blob references (image record §6.1) is a separate change (Q-S4). The
runtime's 16 MiB payload limit bounds the total.

### 5.2 Immutable body paths

`immutable_body_paths = ["model", "input", "response_format", "background", "stream"]`. `response_format` as a whole
is reserved because operator extras that set `delivery: "uri"` or `gcs_uri` would change the artifact form the
component parses, and `stream` / `background` would change the submit answer's shape. Today the bridge reserves only
`model`, `input`, `response_format.type` and `response_format.delivery` (durable.rs:142-147), so an operator extra
that writes `response_format.aspect_ratio` is accepted today and refused after migration (an intentional difference,
§12.3). Nothing in the body steers the bill — the clip length is fixed and usage is reported — so no metering field
needs reserving beyond these.

### 5.3 Refusals

All refusals happen in `build-submit-request`, before admission, with zero upstream calls: a missing prompt, a
`duration` other than 10, `n` other than 1, a frame or video input, an image that is not a `data:` URI, more
reference images than the bound. Messages name the unsupported field and never echo the input.

## 6. Artifacts: inline bytes held by the host (contract 8, part 1)

### 6.1 Why inline, and why not `gcs_uri`

On Vertex, `delivery: "uri"` requires a `gcs_uri` in a customer bucket (O0-b). Supporting it would need: an operator
bucket per provider row (non-secret configuration), the service account's write access to it, a fetch of `gs://…`
or its HTTPS form with the same bearer, confinement of that fetch to the configured bucket (not the provider
endpoint, so the host's rule "a credential-gated URL must lie under the bound endpoint",
server:modules/tasks/component.rs:529-533, would refuse it), and object lifecycle. That is a storage integration,
not a translation, and a clip of ~3.7 MB does not need it. Inline is v1; `gcs_uri` is Q-L4.

### 6.2 Elision before the component sees a response

The package declares `response_blob_paths` (manifest, §3.4) with the image world's path-pattern grammar (image record
§6.2: RFC 6901 pointers with `*`, at most 32, each ≤ 256 bytes). For every response the host hands this package —
submit, observe, error — it runs south's `elide_v1` with those paths and the fallback threshold, keeps the blobs in
process memory for the duration of that one call, and gives the component `HttpResponseParts` whose `body` is the
compact elided JSON. The placeholder is the image world's `{"$south.blob": {"id", "bytes", "head"}}`. A body that is
not JSON (an SSE error, §8) is passed through unchanged if it is within the fallback threshold, and otherwise is
`Unknown` without calling the component. A `$south.` key in the upstream body makes the round `Unknown`, as in the
image world (§6.5 there). For the measured success the view shrinks from ~4.9 MB to ~10 KB (the thought signatures
stay; they are not declared).

`elide_v1` is the image world's function (`contracts.media` v1); this record adds nothing to its grammar. Task
packages that declare no paths still get the fallback threshold, which by itself would elide any string above
1 MiB — a behavior change for existing task packages unless the host applies elision only to packages declaring
contract 8 (recommended; §13).

### 6.3 The inline artifact form

`TaskArtifactRefV2` gains a variant:

```rust
pub enum TaskArtifactRefV2 {
    Urls(Vec<TaskArtifactV2>),
    FileId(String),
    None,
    /// Contract 8: artifacts whose bytes are strings in the upstream response, held by the host.
    Inline(Vec<TaskInlineArtifactV2>),
}

pub struct TaskInlineArtifactV2 {
    pointer: JsonPointerV1,        // concrete RFC 6901 pointer, no `*`; must land on a `$south.blob` node of the view
    encoding: InlineEncodingV1,    // closed: `base64` (RFC 4648 §4, padded) | `data_url`
    media_type: String,            // `type/subtype`, ≤ 255 bytes
    role: TaskArtifactRoleV2,      // contract 7; v1 of this package reports only `Primary`
}
```

Wire form: `{"inline": [{"pointer": "/steps/1/content/0/data", "encoding": "base64", "media_type": "video/mp4",
"role": null}]}`. Bounds and rules: 1 to `MAX_ARTIFACT_URLS` (16) items, at least one primary (the contract 7 rule,
task_v2.rs:320-336); pointers distinct. The same shape as the image world's `inline {pointer, encoding, media_type}`
(image record §10.1), so a pointer names which of the upstream's strings is the artifact and never carries bytes;
the component cannot invent content. Omni reports exactly one item, `media_type` taken from the part's `mime_type`
(`video/mp4` in both samples), refused as `Unknown` if it is not `video/*`.

### 6.4 Host handling and delivery (DO4)

1. **Resolve and verify before settlement**: for each inline item, look up the blob the pointer lands on (a pointer
   that does not land on a blob of this call's view → `Unknown`), decode per `encoding` (strict alphabet; failure or
   an empty result → `Unknown`), and check the decoded size against the host's artifact limit. The measured clip
   decodes to 3,695,253 bytes (O0-b; the sample's `data` length 4,927,004 is a multiple of 4 and uses `+` and `/`).
2. **Store, then settle**: write the bytes to the host's artifact store under the task, then record the terminal
   state and settle — the image world's verify-then-settle order (image record §10.2). The observation persisted
   with the task carries pointers only, never bytes.
3. **Failure to decode or store is retried by observing again**, not parked at once: the upstream keeps the
   interaction and returns it again by id (O0-b), so the host's next poll yields the same pointer and fresh bytes.
   After the host's existing poll budget the task is `Unknown` → manual review. How long the upstream retains an
   interaction is not known (Q-L5); a long outage can turn a recoverable task into a manual one.
4. **Rendering**: the observation handed to `render-success` is the persisted one (pointers); the component writes a
   reference node `{"$south.artifact": {"index": i}}` at `data[i].url`, using the image world's `$south.artifact`
   node, and the host replaces it with `/v1/video/tasks/{task_id}/artifacts/art_{i}` — the path the credential-gated
   URL rewrite produces today (server:modules/tasks/component.rs:478-509). A `$south.` node left anywhere after
   replacement refuses delivery, as a leftover gated URL does today (same function, :502-506).
5. **Serving** `art_i` returns the stored bytes with the declared `media_type`. Counting, envelopes and storage
   follow contract 7: primary artifacts only.

Blocking callers receive `{"created", "data": [{"url": "/v1/video/tasks/{id}/artifacts/art_0"}]}` instead of today's
`{"created", "data": [{"b64_json"}]}` (gemini.rs:889-900) — the DO4 shape change.

The host's artifact store is assumed to exist for task artifacts (contract 7 speaks of hosts "storing" primary
artifacts); whether today's store accepts bytes that did not come from a URL fetch was not read (inferred). A host
without a store must refuse to load an `inline` package for blocking delivery rather than inline the bytes into the
JSON response.

## 7. Usage and pricing (contract 8, part 2; DO5)

### 7.1 Token buckets in the usage facts

`TaskUsageFactsV2` gains one optional member, `token_buckets`, a map from a **closed** bucket vocabulary to a
non-negative integer. The vocabulary is the image world's (image record §9.1: `text_input`, `image_input`,
`cached_text_input`, `cached_image_input`, `cached_input`, `text_output`, `image_output`, `total_input`,
`total_output`) plus the modalities and the reasoning bucket video needs:

| Added bucket | Meaning |
|---|---|
| `audio_input`, `video_input` | input tokens of that modality |
| `audio_output`, `video_output` | output tokens of that modality |
| `reasoning_output` | reasoning ("thought") tokens. **Outside** `total_output` for this vocabulary, matching the measured relation `total = input + output + thought` (O0-b) and the Gemini text measurement (umbrella §16 Q13) |

One vocabulary for both worlds is the point: a bucket means the same thing wherever it is reported, and the host has
one pricing function for both (§7.3). The image record's open question on declared buckets (Q16 there) and this
record should be decided together.

Rules (contract text):

- Null and absent mean "not reported"; 0 means "reported as zero" (the task contract's existing distinction,
  task_v2.rs:108-121).
- **Declared buckets.** The manifest lists `token_buckets` (every bucket the package may report) and
  `required_token_buckets` (those it must report on every success). On `Succeeded`, every required bucket is non-null,
  and no bucket outside `token_buckets` appears; otherwise the component returns `Unknown`, never a partial success
  (the image world's §9.1 rule; declaring per package rather than per model because the task world has no
  `model-capabilities` call).
- `token_buckets` present on a success is what tells the host the bill is bucketed; `tokens` is then null.

Omni's mapping (from the measured fields):

| Wire | Bucket |
|---|---|
| `usage.total_input_tokens` | `total_input` (required) |
| `usage.input_tokens_by_modality[]` with `modality` `text` / `image` | `text_input` / `image_input` (each optional; only `text` was observed) |
| `usage.total_output_tokens` | `total_output` (required) |
| `usage.output_tokens_by_modality[]` with `modality` `video` | `video_output` (required) |
| `usage.total_thought_tokens` | `reasoning_output` (required; 234 and 739 observed — a zero must be reported as 0, not omitted, if the wire ever omits it on a non-thinking answer: Q-L2) |
| any other modality string | none: the observation is `Unknown` (today the host bills an unknown modality at the text rate, media.rs:455-459) |
| `usage.total_cached_tokens` | `cached_input`, if a sample ever shows it (not declared in v1) |

The component also checks the dialect's own relations and answers `Unknown` when they fail: Σ input modalities =
`total_input` when any modality entry is present; Σ output modalities = `total_output`; `total_tokens` = `total_input`
+ `total_output` + `total_thought_tokens` when `total_tokens` is present (both samples satisfy all three). This is the
strictness umbrella §6.2 item 1 asks of every reference implementation.

### 7.2 Request estimate

The component reports `requested_seconds = 10` (the fixed clip length is dialect knowledge, O0-b) and
`requested_outputs = 1`, using existing contract 6 fields; this also satisfies the host's rule that a component
priced through the component path must report seconds or have a binding default (durable.rs:2201-2216). Contract 8
adds one optional estimate member, `bucket_ceilings` (bucket → integer), which may only **tighten** the reservation
(§7.4). Omni reports `{"video_output": 57920}`.

### 7.3 The host's bucketed pricing function

One provider-agnostic function replaces `calculate_gemini_interactions_cost` (media.rs:418-466):

```text
price_token_buckets(row, buckets) -> micro-dollars
  input part:   if any input-modality bucket is present:
                    Σ over modality m of (m_input − cached share) × rate(row, m_input)
                    + cached_input × rate(row, cached_input)   // cached deducted from text_input first (image §9.1)
                else total_input × rate(row, total_input)
  output part:  if any output-modality bucket is present: Σ m_output × rate(row, m_output)
                else total_output × rate(row, text_output)
  reasoning:    reasoning_output × rate(row, reasoning_output)
  each term:    saturating multiply, divide by 1e6, clamp ≥ 0 (today's arithmetic, media.rs:439)
```

`rate(row, bucket)` reads one catalog column per bucket from a single host table that is the same for every provider:
`text_input` / `total_input` → `input_price_per_million`; `image_input`, `audio_input`, `video_input` → their own input
column when the row has one, otherwise `input_price_per_million` (one input price unless the row says otherwise — a
catalog rule, not a provider fact); `cached_input` → `cached_input_price_per_million`, otherwise the input rate;
`text_output` and `reasoning_output` → `output_price_per_million`; `image_output` → `image_output_price_per_million`;
`video_output` → `video_price_per_million_tokens`; `audio_output` → an audio output column. **An output bucket a package
declares whose column the row does not price is a configuration error at binding load** (503 at request time), not a
fallback to the text rate.

What does not survive from today's function: the image-model input fallback through
`image_input_text_price_per_million` (media.rs:422-429), which serves Nano Banana-class rows that never reach this
path once the native surface is gone (DO6); "audio bills at the text rate because Omni bundles audio into the clip"
(:449-452) — Omni reports no audio bucket, and a package that does gets the audio column or a load error; and the
warn-and-bill-text branch for unknown modalities (:455-459), replaced by the closed vocabulary. For the measured
bucket set (`total_input`, `text_input`, `video_output`, `total_output`, `reasoning_output`) the new function gives the
same amount as today's on the same numbers, which §12.2 uses as the reconciliation item.

The ledger row is unchanged in shape: token-type usage with `estimated: false`, input column = `total_input`, output
column = `total_output + reasoning_output` (today's rule, media.rs:470-476). The host's component meter gains a
fourth value, token buckets, chosen when the package declares `token_buckets` and the row prices them; it takes
precedence over the single-rate token meter (durable.rs:1293-1331), which stays for packages that report `tokens`.

### 7.4 The reservation bound (umbrella §6.3 instance; fixes P26-F1)

**The host bound, per bucket**, computed without provider knowledge:

- Input: the northbound request's text bytes (the prompt; a token occupies at least one byte) plus the host's
  configured per-media-part allowance for each image input (media bytes are not counted — the embeddings record's
  instance, §7.3 there, for the same reason).
- Each declared output bucket and `reasoning_output`: a per-output maximum from catalog data
  (`max_tokens_per_output`, umbrella §7.5; an operator row field until the south catalog exists) × `requested_outputs`
  (1). A declared output bucket with no maximum on the row is a configuration error (503), because the host would have
  no bound of its own — Omni cannot send a cap (no duration and no output-cap parameter, O0-b), which is exactly the
  umbrella's "families that cannot send the cap" case.
- **Reservation** = `price_token_buckets(row, min(host bound, bucket_ceilings))`, bucket by bucket. **Every check**
  compares the reported buckets with the host bound only.

**Concrete numbers for Omni.** Catalog data: `video_output` 57,920 per output (measured, identical in both samples,
independent of the prompt, and the clip length is fixed at 10 s); `reasoning_output` 4,096 per output (the host's
`default_max_output_tokens`, about 5.5× the larger observed value, 739). At the repository price card cited by host
plan P26-F1 ($1.5 input, $9 text output, $17.5 video output per million tokens):

| | Today's bound (gemini.rs:719-738) | Proposed bound, 2,000-byte prompt | Sample s2 settles | Sample s3 settles |
|---|---|---|---|---|
| Input | body bytes × $1.5/M (~$0.0003) | 2,000 × $1.5/M = $0.0030 | 17 → $0.000025 | 1 → $0.000001 |
| Video output | 4,096 × $17.5/M = $0.0717 | 57,920 × $17.5/M = $1.0136 | $1.0136 | $1.0136 |
| Text + image output | 2 × 4,096 × $9/M = $0.0737 | not declared: 0 | 0 | 0 |
| Reasoning | 4,096 × $9/M = $0.0369 | 4,096 × $9/M = $0.0369 | 234 → $0.0021 | 739 → $0.0067 |
| **Total** | **~$0.18** | **~$1.05** | **$1.0157** | **$1.0203** |

Today every clip settles above its reservation and goes to manual review (settlements above the reservation are
parked, server:modules/billing/repo/settlements.rs:1493-1501, cited by host plan P26-F1). With the proposed bound both
samples settle normally and the reservation exceeds the settlement by about 3–4 %. If the upstream ever produces more
video tokens per clip, the bucket exceeds the host bound, the call goes to manual review with the reservation held
(umbrella §6.3, "funds outcome of a hit"), and the fix is a catalog datum, not code. A reasoning count above 4,096 is
the same case; the host should watch the flag rate before cutover.

### 7.5 What the host can and cannot detect

Out-of-bound buckets, broken sums (§7.1) and a settlement above the reservation are caught. Under-reporting —
e.g. a component that reports 0 reasoning tokens — falls in the undetectable zone of umbrella §6.3; the trust comes
from the pinned digest, the gate ② usage rows of §10 built from the O0-b samples, and the dual run of §12.

## 8. Errors and outcomes

### 8.1 Reading an error body

Two measured shapes, and the content type does not tell them apart (the SSE-framed 400 came with `Content-Type:
application/json`, O0-b). The component therefore tries, in order:

1. the body as JSON with an `error` object: `message` (string) and `code` (string, e.g. `invalid_request`);
2. the body as SSE, decoded with south's `decode_sse_v1` (umbrella §5.2, in `south-contracts` and callable inside
   the component): the first frame whose `event` is `error`, whose `data` is JSON with the same `error` object;
3. otherwise no detail: the envelope carries a fixed message by status.

Messages are bounded (`MAX_ARTIFACT_REF_BYTES`) and passed on; the upstream's documentation link in the measured
message is kept verbatim (it names the upstream's own documentation, not a secret).

### 8.2 Classification

| Where | Upstream answer | Component outcome | Public code (kernel `ErrorCode`, kernel:error.rs:15-48) |
|---|---|---|---|
| submit | 400 (either shape) | `Rejected` | `invalid_request` (400) |
| submit | 401 / 403 | `Rejected` | `auth` |
| submit | 429 | `Rejected` | `rate_limit` |
| submit | other 4xx | `Rejected` | `invalid_request` with the upstream status |
| submit | 5xx, transport failure, unreadable 2xx | `Unknown` | — (the host keeps the reservation) |
| observe | `failed` with an `error` object (not yet sampled) | `Failed { kind: Failed }` → `map-terminal-failure` | `provider_protocol_error` (502), as Veo maps a failed operation (reference_veo_task_v2.rs:361-378); a content-policy code, once sampled, maps to `content_policy` |
| observe | non-2xx, unknown status word, `completed` without video or required buckets | `Unknown` | — (polling continues; after the host's budget, manual review) |

A `Rejected` submit **releases the reservation today**: the host's task submission path records an upstream refusal and
releases the reservation in the same transaction (durable.rs:404-406, :578-583, :747). That is already the umbrella §6.4
outcome for this world, so nothing waits on the dual run here. The O0-b evidence supports it for 400: the
missing-`gcs_uri` 400 was not billed. Whether every 4xx on this upstream is unbilled is inferred, and a 429 is
`Rejected` because a rate-limited request produced nothing.

### 8.3 Retry and cooldown

The component gives only the classification and the code; credential cooldown, retry on another credential and
backoff are host policy keyed on `ErrorCode` (`auth`, `rate_limit` and `capacity` are documented as the retriable or
credential-health classes, kernel:error.rs:18-27). No provider-specific retry signal is defined; Vertex `Retry-After`
handling, if any, is the host's generic header rule (inferred; the samples carried no such header).

### 8.4 What changes against today's bridge

Today every upstream 4xx after dispatch is `delivery_unknown` → manual review with the reservation held
(gemini.rs:802-814). After migration a 4xx at submit is `Rejected` and released at once, and a 4xx can no longer
occur after the clip exists (the GET either returns the interaction or is `Unknown`). This is an intended difference,
not a dual-run failure (§12.3).

## 9. Credentials and endpoint

### 9.1 Vertex: the service-account recipe

The family's slot `provider_api_key` is `minted` by the umbrella's §3.3 Vertex sketch unchanged: `jwt_sign` (RS256,
key from the service-account JSON's `/private_key`, `iss` from `/client_email`, scope `cloud-platform`, `aud` = the
token step's endpoint), then `oauth2_token` with the JWT-bearer grant; `present` = `token.access_token`; does not
rotate; `project_id` is a non-secret field imported from the same JSON and exported as an attribute. The descriptor
asks for `Auth::bearer` on the slot; the trust rules of umbrella §3.4 apply (the token endpoint is shown to the
operator and confirmed per package digest; a first-party release only, until signing exists). The component never
sees the key or the token (umbrella R3).

### 9.2 Endpoint

The upstream path is `/v1beta1/projects/{project}/locations/global/interactions` (O0-b). The family declares the
umbrella §7.3 template `https://aiplatform.googleapis.com/v1beta1/projects/{project}/locations/global`, whose
`{project}` comes from the recipe's exported attribute; the component appends `/interactions` and
`/interactions/{id}`, which `ProviderEndpoint::permits` admits as a path prefix (kernel:provider.rs:65-84). The
location is fixed to `global` because that is the only location sampled; a regional template is a later family
change, not a host one.

### 9.3 Before phases B2 and B4 land (stopgap)

Today the host already mints Vertex tokens for task components through the binding recipe `VertexSa`
(server:core/task_execution.rs:36-38; minted in server:modules/tasks/component.rs:402-413), so the bearer half works
now. The endpoint half does not: for a `vertex_ai` provider row the host builds the component's base URL itself as
`…/v1/projects/{project}/locations/{region}` (server:modules/inference/engine/vertex.rs:264-267, called from
durable.rs:2043-2055), and because confinement is a path prefix the component cannot reach `/v1beta1/…`. Two
stopgaps, both J1 items to clear before DP0 is claimed:

- a `task_component` provider row whose operator-entered `endpoint` is the full `v1beta1` base with the project in it
  (durable.rs:2053 uses `provider.endpoint` for non-Vertex rows), with the binding recipe `VertexSa` —
  inferred to work; whether `VertexSa` minting depends on the row's provider type was not read;
- or a host change that lets a Vertex row's task endpoint carry an API version, which is host code and is not
  recommended.

### 9.4 P26-F4 disappears

P26-F4 is the bridge sending `x-goog-api-key` to whatever `uri` the upstream response names, through a client that
follows redirects (host plan P26 §8). Inline delivery has no second hop: the bytes arrive in the authenticated GET
of the interaction itself, whose URL the component built under the confined endpoint. There is no artifact URL, so
`fetch_with_credential` is never set and `fetch_bound_media` (server:modules/tasks/component.rs:515-560) is never
reached for this package. If `gcs_uri` delivery is ever added (Q-L4), the bucket fetch needs its own confinement rule
(§6.1).

## 10. Conformance (gate ②)

Fixtures live in `crates/south-component-conformance/fixtures-gemini-omni-task-v2/`, transcribed from the O0-b
samples with the video `data` replaced by a short base64 placeholder of the same alphabet (and, for the elision rows,
a placeholder longer than 1 MiB generated by the test, not stored), the thought `signature` truncated, and the project
id replaced. The README states their source, as the Veo fixtures README does for its own.

**Rows required by name** (missing any one fails `Coverage`):

| Row | Assertion |
|---|---|
| `task-v2.submit.text` | body `{model, input, response_format: {type: "video", delivery: "inline"}, background: true}`, URL, bearer slot, estimate (10 s, 1 output, `bucket_ceilings`), immutable paths |
| `task-v2.submit.aspect-ratio` | `response_format.aspect_ratio` placed |
| `task-v2.submit.image` (after O0-c) | image part shape inside `input` |
| `task-v2.submit.refused-duration` / `refused-n` / `refused-frame` | capability errors, zero descriptors |
| `task-v2.created.accepted` | JSON `in_progress` answer → `Accepted(id)` (built from O0-c; until then from the SSE `interaction.created` fields) |
| `task-v2.created.accepted-completed` | a `completed` submit answer → `Accepted(id)`, not `AcceptedTerminal` |
| `task-v2.created.http-400-json` | the unknown-parameter 400 → `Rejected`, message kept |
| `task-v2.created.http-400-sse` | the measured SSE-framed 400 under `application/json` → `Rejected`, message kept |
| `task-v2.created.http-500` | `Unknown` |
| `task-v2.observation.progress` | `in_progress` → `Progress` |
| `task-v2.observation.succeeded` | the s2 body (elided view) → one inline artifact, pointer `/steps/1/content/0/data`, `video/mp4`; buckets `total_input` 17, `text_input` 17, `total_output` 57,920, `video_output` 57,920, `reasoning_output` 234 |
| `task-v2.observation.succeeded-get` | the s3 GET body, with its leading `user_input` step and three thoughts → pointer index shifts accordingly; `reasoning_output` 739 |
| `task-v2.observation.missing-usage` / `missing-video-bucket` / `missing-thoughts` | `Unknown` |
| `task-v2.observation.sum-mismatch` | modality sums or `total_tokens` disagree → `Unknown` |
| `task-v2.observation.unknown-modality` | an output modality outside the vocabulary → `Unknown` |
| `task-v2.observation.no-video` | `completed`, usage present, no video part → `Unknown` |
| `task-v2.render.direct` | `data[0].url` is `{"$south.artifact": {"index": 0}}`; no other `$south.` node |

**Checks**: the existing task suite checks, plus three new ones that bind every contract 8 package:

- `UsageNeverDefaulted` (umbrella §6.2 item 3) on the task side: success fixtures carry `usage_pointer`; the suite
  deletes the usage object and each required bucket in turn and requires `Unknown`.
- `ReferenceIntegrity` (image record §12.1) for the task outputs: every inline pointer lands on a `$south.blob` of the
  view; no output string exceeds the fallback threshold; `render-success` emits only `$south.artifact` nodes whose
  index names an existing primary artifact.
- `DeclaredBucketsOnly`: no success reports a bucket outside the manifest's `token_buckets`.

**Usage judge** (umbrella §6.2 item 5): Omni has documentation but the field relations above were checked against
captured traffic; the judge's expectations come from the O0-b captures archived with the fixtures, not from the
reference implementation.

## 11. Host obligations (gate ③)

A host suite, frozen in south and run by both hosts before `host_capabilities` marks contract 8 `verified`
(umbrella R4):

1. **Elision**: for a contract 8 package, every response is elided with `elide_v1` and the package's
   `response_blob_paths` before it reaches the component; golden vectors include the s2 shape and an SSE error body.
2. **Inline artifacts**: pointer resolution against the call's blobs; strict decoding; size limit; store before the
   terminal state and before settlement; a decode or store failure re-observes rather than settling; bytes never
   enter the persisted observation.
3. **Rendering**: `$south.artifact` nodes replaced with `/v1/video/tasks/{id}/artifacts/art_{i}`; a leftover
   `$south.` node refuses delivery; `art_i` serves the stored bytes with the declared media type.
4. **Bucketed meter**: the suite checks only what is not a price — every reported bucket is consumed exactly once,
   `cached_input` is deducted from `text_input`, a declared output bucket the row cannot price or that has no
   per-output maximum refuses at load, reservation = the §7.4 minimum, checks use the host bound only, and a bound
   hit parks with the reservation held. The pricing function and its column table stay host code and host tests:
   prices do not enter South (ARCHITECTURE.md:108-112, as cited by umbrella §7.5).
5. **Routing**: a model row routed to this package with `Prefer: respond-async` gets 202 and a task envelope (DO3);
   the exclusion at server:modules/inference/handler/video.rs:137-140 is gone.

## 12. Migration, dual run and what retires

### 12.1 Steps (host plan P26 O0–O4, refined)

| Step | Side | Content | Gate |
|---|---|---|---|
| O0-c | Host (live, paid) | Three Vertex samples: `background: true` without `stream` (submit answer shape); an image-to-video request (image part shape, `image_input` bucket); one refused or failed generation (failure status words, whether charged). Roughly one to three paid generations | Answers recorded in host plan P26; fixtures of §10 rows marked "after O0-c" |
| O2a | South | Contract 8 (§6.3, §7.1, §7.2), manifest fields, the elision hook for task packages, `price_token_buckets` golden vectors, suite checks of §10 | Contract tests red first (contract 7 shapes still decode; new members validate) |
| O2b | South | `task-gemini-omni-v2` package and reference implementation; fixtures from O0-b / O0-c | Package passes the suite; wasm ≡ reference |
| O2c | Host | §11 obligations; the T09 canary guest gains an "inline artifact + declared buckets" mode, proving a provider the host has never seen is served with zero host changes | Canary J2 green in both billing forms |
| O3 | Both | South release; host pin; an Omni component leg (process-level, both billing forms, outside CI); the reconciliation of §12.2 | §12.2 items equal; §12.3 differences asserted on their own |
| O4 | Host + ops | Route Omni rows to the package; drain; delete §12.4; DO6 B for the native surface once `task-v2.submit.image` is green | Structural ratchet of host plan P26 O4; J3 (removing the package leaves the host starting, Omni rows refused) |

### 12.2 What the dual run can compare

Today's bridge only serves provider type `gemini`, i.e. the Gemini API line (video.rs:174-190), and v1 of this
package serves only Vertex. **The same request cannot reach the same upstream through both arms**, so the
upstream-level reconciliation host plan P26 O3 describes ("upstream submit body and auth header equal item by item")
cannot hold: the body differs by design (`delivery: inline`, `background: true`), the URL differs (`v1beta1` Vertex
path) and the auth differs (minted bearer versus `x-goog-api-key`). What is compared, with both arms driven by mock
upstreams that answer with the O0-b s2 / s3 bodies, under each billing form:

1. **Settled amount**: today's `calculate_gemini_interactions_cost` and `price_token_buckets` on the same usage give
   the same micro-dollars (s2: 1,015,731; s3: 1,020,252 at the P26-F1 price card).
2. **Ledger row**: input and output columns, `estimated: false`, endpoint `/v1/video/generations`.
3. **Delivered bytes**: the bridge's `b64_json` decoded equals the bytes served at `art_0` (DO4: the shape differs,
   the content does not).
4. **Counter-proof**: removing the usage object, or the `video` bucket, parks the task; it never settles at $0.

### 12.3 Intended differences (asserted separately, not reconciliation failures)

- Reservation: §7.4 (~$1.05) against today's ~$0.18; the new one is asserted to be ≥ both samples' settlements.
- Response shape: gateway artifact path instead of `b64_json` (DO4); `Prefer: respond-async` returns 202 (DO3).
- A 4xx at submit releases the reservation instead of parking it (§8.4).
- `duration` other than 10 and `n` other than 1 are refused instead of ignored (§5).
- An operator extra under `response_format` is refused at save time (§5.2).
- An unknown output modality is `Unknown` instead of billed at the text rate (§7.1).
- `POST /v1/video/estimates` answers with the §7.4 reservation instead of a 400 (video.rs:424-432 and P26-F7 go
  away; the estimate path already calls the same component preparation, durable.rs:1220-1238).

### 12.4 What retires in the host

From host plan P26 §2: the whole Omni part of `video/gemini.rs` (lines 483-901, 321 production lines, including the
`x-goog-api-key` artifact fetch behind P26-F4); the five caller branches (28 lines: video.rs:137-140, :179-190,
:424-432; durable.rs:142-147 `GEMINI_OMNI_VIDEO_OWNED`, :174-175); `extract_gemini_interactions_usage`
(media.rs:358-408) once the native surface is offline too (DO6); `calculate_gemini_interactions_cost` becomes the
generic `price_token_buckets` without its two Omni fallbacks; `GeminiInteractionsUsage` and
`gemini_interactions_token_usage` become the generic bucket type and ledger fold. The bridge's source ratchet and the
"Omni is not in the task framework" ratchet note (durable.rs:3460) are rewritten. P26-F1, F4, F6 and F7 close with
the code they describe.

What the host gains, once and for every provider: the contract 8 adoption (elision for task packages, inline
artifacts, the bucket meter and its pricing function). Per the umbrella's Q1 ruling that is a host change, made once
for a new metering unit and artifact form (P21 §1.4); afterwards a task family with inline artifacts or token buckets
is a package-layer change.

## 13. Versioning

- **Task contract 7 → 8, additive** (umbrella §8.6): `TaskArtifactRefV2::Inline` is a new variant no existing package
  emits; `token_buckets` and `bucket_ceilings` are optional and absent means null. A contract 7 JSON decodes under 8.
  This departs from the contract 6 and 7 records' rule "new keys must appear, old JSON is refused"
  (2026-09-27-task-contract-v6-facts.md, "contract shape"), which the umbrella §8.6 already retires.
- **WIT**: unchanged (`token-station:task-adapter@2.0.0`).
- **Manifest**: `response_blob_paths`, `token_buckets`, `required_token_buckets` — optional task-world fields, south
  minor; absent equals today (umbrella R5).
- **Dependencies**: `contracts.media` v1 (`elide_v1`, `$south.blob`, `$south.artifact`) and `decode_sse_v1` ship with
  the image world's first minor, so contract 8 ships in or after it; the package declares
  `contracts: {"task": 8, "media": 1}`. With the exact tuple of today (manifest.rs:657-694) every task package is
  re-stamped by that release anyway; under the umbrella §8 ranges, none is.
- **Package identity**: new; no existing package bumps for contract 8 (no existing package changes behavior).

## 14. Rejected alternatives

- **Synchronous terminal-on-submit** (DO7's other option): the clip would come through the submit path and the
  sealed-observation column, and a process lost during the ~40 s wait leaves only manual review; the stored
  interaction makes the recoverable form available (§4).
- **Bytes inside the observation** (a `Bytes` artifact or a `data:` URL written by the component): ~5 MB through the
  sandbox on every poll, into the persisted observation and the encrypted terminal column, and close to the runtime
  payload limit for longer clips (§1.3).
- **A component-written artifact URL for the bytes**: the image record's §10.1 reason — the component would choose
  what the host fetches; a pointer only chooses which upstream string is the artifact.
- **`gcs_uri` delivery in v1**: a storage integration with its own confinement rule, for a clip that fits inline
  (§6.1).
- **One token count at the video rate** (DO5 B): ruled out by lv; it under-bills input and thoughts or over-bills
  input.
- **The reservation from the component's rate** (`tokens_per_second` × seconds, today's single-rate component path,
  durable.rs:1293-1331): the component's number would be both the reservation and the thing checked against it
  (umbrella §6.3).
- **Keeping the per-modality `default_max_output_tokens` bound**: P26-F1; every clip parks.
- **Billing an unknown modality at the text rate**: a silent price for a bucket nobody priced.
- **A new synchronous video world** (host plan P26 option ③): everything it needs exists in the task world.
- **The Gemini API family first**: not sampled, and nothing in production needs it (§3.2).
- **Submitting with `stream: true` and reading to the first event**: needs a new host transport behavior and keeps a
  connection open for the whole generation; only the fallback if O0-c refuses `background` alone (§4.5).

## 15. Open questions

Tags: S = south maintainers, L = lv, K = kernel. No kernel change is proposed; no question is tagged K.

- **Q-S1** Umbrella R6 says a family needing a different auth arm belongs to its own package, while the
  OpenAI-compatible reference already presents per family. Read R6 as "only `host_signed` stands alone"
  (recommended), so the Gemini API line can later be a second family here, or keep it literal and make that line a
  sibling package?
- **Q-S2** The bucket vocabulary is shared with the image world and is **wire-faithful**: `total_output` excludes
  `reasoning_output`, as Omni reports it. The umbrella freezes the opposite convention for the IR
  (`reasoning_tokens` ⊂ `output_tokens`, §6.2 item 6). Recommended: keep buckets wire-faithful and state in the
  contract that bucket `total_output` is not the IR's `output_tokens` (the ledger fold of §7.3 adds them); the
  alternative is a bucket named for the IR convention, which would make the component do arithmetic the host can do.
- **Q-S3** Apply `elide_v1` only to packages declaring contract 8 (recommended), or to every task package (the
  fallback threshold would then elide strings above 1 MiB in existing packages' responses)?
- **Q-S4** Task-world request media still crosses the sandbox as `data:` URIs (§5.1). Move it to the media
  vocabulary's blob references in a later contract, or accept the 16 MiB bound for task requests?
- **Q-S5** Declared buckets per package (this record; the task world has no `model-capabilities`) versus per model
  (the image world): acceptable to differ?
- **Q-S6** ARCHITECTURE.md:114-115 admits a metering vocabulary only with "a second consumer in sight". Token
  buckets in the task contract have one host consumer today (token-station-server); the image world shares the
  vocabulary but not a second host. Does the community host intend to adopt contract 8, or do the maintainers grant
  an explicit exemption (the embeddings record's E-Q5 asks the same)?
- **Q-L1** Run O0-c (one to three paid generations: `background` alone, image-to-video, a refusal) before O2b?
  Recommended: yes — submit shape and image input are guesses without it.
- **Q-L2** Until a failure or content-policy sample exists, every non-`completed` terminal word and every
  `completed` interaction without a video is `Unknown` → manual review. Accept that conservative default for v1?
  Recommended: yes.
- **Q-L3** Reference-image count: Omni's limit is unknown; take it from O0-c or documentation, and refuse above it.
- **Q-L4** `gcs_uri` delivery: not in v1 (recommended). Revisit only if a clip size or a customer requirement makes
  inline unusable.
- **Q-L5** Upstream retention of stored interactions is unknown; it bounds how long §6.4's re-observe recovery
  works. Measure or find documentation before cutover.
- **Q-L6** DO4 changes the blocking response shape. O0-a found no callers (no `gemini` rows), so no deprecation
  window seems needed; confirm.
- **Q-L7** Ship v1 on the §9.3 stopgap (`task_component` row + `VertexSa`, J1 items) before umbrella phases B2 / B4,
  or wait for the endpoint template and recipes? Recommended: stopgap for the component leg and dual run only;
  cutover after B2 / B4 so no new J1 item reaches production.
- **Q-L8** Catalog data for the bound: `video_output` 57,920 and `reasoning_output` 4,096 per output. Accept these
  values, with a bound hit parking for review?
