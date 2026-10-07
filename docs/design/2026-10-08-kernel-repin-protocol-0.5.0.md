# Re-pinning the kernel to mirror v0.4.0 (protocol 0.5.0, canonical IR 3)

Status: implemented on branch `p21-kernel-repin`; not released. The release that carries it is a separate step.

Date: 2026-10-08

Predecessors: `2026-09-29-release-0.39.0.md` (the previous re-pin, to mirror `v0.3.0`),
`2026-09-30-host-zero-vendor-boundary.md` (§8.3 range handshake, §10 B7b, §13.6 the declared-runtime discipline).

Host context: token-station-server P21 handoff §4.5, step 1 of 5. Steps 2 to 4 (B7b in south, #138, Q14) and step 5
(the minor release and the host re-pin) are not part of this change.

## 1. What moves

| Item | Before | After |
|---|---|---|
| Kernel mirror tag | `v0.3.0` = `f585bc83e5f8a31fbbc6535dd17c3ac8deec6a54` | `v0.4.0` = `c2581f37ad468201c903e0d596b59e796173e459` |
| Upstream source (mirror `compatibility.json` `mirror.source_commit`) | `kernel-v0.4.0` = `6822aab1dea54ef646cb2206595cd4955ff9764a` | `kernel-v0.5.0` = `8e34f5a089d0b9c7273b49ddb6952dd87e960019` |
| `token-station-protocol` | 0.4.0 | 0.5.0 |
| Kernel contract `canonical_ir` | 2 | 3 |
| Kernel contracts `stream`, `error_catalog` | 2, 1 | unchanged |

South pins the mirror by commit (`rev`), as before. The workspace dependency in the root `Cargo.toml` is the only pin;
the component and fuzz lockfiles follow it, and only the kernel entry and each component's own version change in them.

## 2. The compatibility tuple, and where each value comes from

Every shipped manifest's `compatibility` changes as follows. The values are derived from the definitions in
`crates/south-provider-api/src/manifest.rs` (`CompatibilityDeclarationV1`) and the mirror's own `compatibility.json`
at `c2581f37`, not inferred from the previous manifests:

- `ir_schema_id` = `token-station-protocol@<protocol crate version>/<mirror tag>` = `token-station-protocol@0.5.0/v0.4.0`.
- `kernel_version` = the mirror release (`release.version` in the mirror) = `0.4.0`.
- `kernel_revision` = the mirrored upstream commit (`mirror.source_commit`) = `8e34f5a089d0b9c7273b49ddb6952dd87e960019`,
  as the 0.39.0 re-pin recorded `6822aab1…` (upstream) rather than `f585bc83…` (mirror).
- `kernel_contracts.canonical_ir` = 3 (the mirror's `contracts.canonical_ir`); `stream` 2 and `error_catalog` 1 are
  unchanged.

South's `compatibility.json` `kernel_contracts.canonical_ir` moves to 3. Its `contracts.canonical_ir` stays `null`:
the kernel owns that number.

The range handshake (`compatibility_admits`) compares `kernel_contracts` exactly, so a host whose `HostRangeV1` still
says `canonical_ir` 2 refuses every package after this change, and a host that moves to 3 refuses every earlier one.
`ir_schema_id`, `kernel_version` and `kernel_revision` remain provenance for the range handshake and exact values for
the one-release exact handshake.

## 3. Package identities

The tuple is package content, so every package whose last published identity carries the old tuple takes a patch
bump, with no behavior change:

| Package | Published in 0.45.0 | Now |
|---|---|---|
| `provider-anthropic` | 1.0.11 | 1.0.12 |
| `provider-bedrock-converse` | 1.0.8 | 1.0.9 |
| `provider-bedrock-converse-bearer` | 1.0.0 | 1.0.1 |
| `provider-gemini` | 1.1.7 | 1.1.8 |
| `provider-openai-compatible` | 2.2.1 | 2.3.0 (already moved by #150, unreleased; not bumped again) |
| `task-kling` | 1.0.7 | 1.0.8 |
| `task-kling-v2` | 0.32.5 | 0.32.6 |
| `task-minimax-v2`, `task-bailian-v2` | 0.31.4 | 0.31.5 |
| `task-byteplus-v2` | 0.36.4 | 0.36.5 |
| `task-xai-v2`, `task-veo-v2`, `task-wan-image-v2`, `task-gmi-image-v2` | 0.35.4 | 0.35.5 |

`shipped_packages_v1::the_kernel_repin_retires_every_published_045_package_identity` pins that none of them reuses its
0.45.0 identity and that each declares the new tuple.

## 4. What protocol 0.5.0 changes, and what that means here

Upstream change: token-station #40 (protocol 0.5.0) and #41 (router-core 0.5.0, version only).

- **`Auth::BearerAndHeader`** (a new variant). `admit_descriptor_auth` matched `Auth` exhaustively, so it now refuses
  this arm with the new `DescriptorAuthErrorV1::BearerAndHeaderNotDeclared`: no manifest arm declares it yet. B7b
  (step 2) adds the `bearer_and_header_secret` arm that admits it. Adding the error variant is a breaking change for a
  host that matches the enum exhaustively.
- **`Auth::header` admits any lowercase field name** of at most 64 bytes outside the kernel's never-credential list,
  instead of only the kernel's `CREDENTIAL_HEADERS`. South's admission already decides which names a package may use
  (sanctioned names, or the package's declared `secret_headers`), so no south code changes; the declared-header arm
  that B7a added is now reachable from a descriptor a component serializes, not only from one built in-process. The
  test `until_b7b_no_component_descriptor_can_name_a_declared_header` was written to fail at exactly this point and is
  replaced by `a_deserialized_descriptor_reaches_the_declared_arm`, as its comment asked.
- **`ProviderEndpoint::permits` admits `%2F` inside one path segment below the endpoint** when every decoded piece is
  non-empty and not `.` or `..` (D5). `admit_descriptor_auth` calls `ProviderConfig::authorize`, so a descriptor that
  encodes an ARN model id as one segment now passes the endpoint check. Adding gate ② cases for Converse and Gemini is
  #138 (step 3). The same rule would also admit an upstream task id containing `/` in an observe URL, which every
  task-v2 component that encodes the id as one segment relied on the kernel gate to refuse. See §4.1.
- **`ProviderConfig::declared` and `ChatRequest::host_values`** (`ComponentValues`): new, defaulted, and skipped when
  empty, so no fixture and no wire shape changes. Nothing in south reads or writes them yet; Q14 (step 4) does.
- **`Usage::explicit_cache_read_tokens`**: new, defaulted, and not serialized at zero. No reference reports it, so the
  fixtures and the usage judges are unchanged.

No provider component's translation, request, response or stream output changes, and the fixture packs are untouched.
The one component behavior change is §4.1.

### 4.1 Task ids containing `/` (owner decision, 2026-10-08)

Seven task-v2 components percent-encode the upstream task id as one observe path segment: `task-kling-v2`,
`task-minimax-v2` (H3 route), `task-bailian-v2`, `task-byteplus-v2`, `task-xai-v2`, `task-wan-image-v2` and
`task-gmi-image-v2`. Under protocol 0.4.0 an id containing `/` became `%2F` and the kernel gate refused the request;
under 0.5.0 the gate admits it.

Decision (lv, option B): each of the seven components refuses a task id containing `/` in `build_observe_request`, with
its existing invalid-id error, before any request is built. A task id never legitimately needs `/`; D5 was opened only
for ARN model ids; and many upstreams decode `%2F` as a path separator, which would turn one task id into a different
path. This restores exactly the 0.4.0 outcome (the poll is refused) one layer earlier. The submission path is
unchanged. The three tests that pinned the kernel's refusal now assert the component's, and
`task_id_separator_v2::every_segment_encoding_task_component_refuses_a_slash_in_the_task_id` covers all seven. The
frozen packs of `task-byteplus-v2`, `task-xai-v2` and `task-gmi-image-v2` had a synthesised `task-v2.observe.encoded`
case whose id contained `/`; that case now uses `a b?c#%` (still covering percent-encoding), and a new
`task-v2.observe.slash` case expects each component's existing invalid-id error. An
encoded backslash (`%5C`) is still refused by the kernel gate. The package versions in §3 already moved and are
unreleased, so this needs no second bump.

Checked and not changed:

- `task-veo-v2`: an operation name is a multi-segment path by design (`models/.../operations/...`), validated per
  segment against `[A-Za-z0-9._-]` and interpolated unencoded, so `%` cannot appear and D5 does not affect it.
- `task-kling` (task-adapter-v1): interpolates the id unencoded. A literal `/` in an id was already admitted under
  0.4.0 (it is a real separator, not `%2F`); the only new admission is an id that itself contains the text `%2F`.
  It is a legacy package; whether to tighten it is left to the owner.
- `task-minimax-v2` on its v1 route passes the id as the `task_id` query parameter, not a path segment.

## 5. `south_runtime` (owner decision, 2026-10-08)

Every package keeps `compatibility.south_runtime` `0.44.0`. Under the declared-runtime discipline (CONTRIBUTING,
§13.6) that field is the oldest runtime that admits the package, and a v0.44.0 or v0.45.0 runtime's range takes its
kernel contracts from that release's `compatibility.json` (`canonical_ir` 2), so `scripts/check-declared-runtime.sh`
would refuse these packages under their declared runtime. The field cannot name the release that will carry this pin
before the workspace version reaches it: `shipped_packages_v1` and the test hosts refuse a `south_runtime` newer than
the workspace version.

Decision (lv): this change keeps `0.44.0`. At the release step (handoff §4.5 step 5) every package's `south_runtime`
moves to 0.46.0 together, and the release also updates
`host_feedback_sf12_to_sf17_retires_every_published_044_package_identity`, which asserts `0.44.0` today. The versions
above are unreleased, so that edit needs no second identity bump. The release index is produced at release time by
`scripts/release_index.py` from the archived manifests; this change does not produce one.

## 6. Hosts

Breaking for hosts on re-pin: the kernel pin and `HostRangeV1.kernel_contracts.canonical_ir` must move together with
the packages (a host cannot load packages of one tuple with the other), and `DescriptorAuthErrorV1` gains a variant.
