# Architecture

Token Station South is a host-neutral execution boundary between Token Station hosts and model
providers. It contains reusable contracts, pure translations, hardened transports, sandboxed
components, and conformance evidence. It deliberately excludes host business policy.

```text
client
  |
  v
host: routing, admission, identity, secrets, pricing, persistence, settlement
  |
  | explicit contracts and capabilities
  v
South: translation, bounded descriptors, transport, sandbox, conformance
  |
  v
provider APIs
```

The dependency direction is one way: hosts consume South. South never depends on
`token-station`, `token-station-server`, host persistence, or host configuration systems.

## Current versions

The current repository release is `0.79.0`.

| Version line | Packages |
|---|---|
| `0.79.0` | Workspace-versioned runtime and host-facing crates |
| `0.56.0` | `south-contracts` |
| `0.63.0` | `south-provider-api` |
| `0.77.0` | `south-component-conformance` |
| `0.1.0` | Candidate libraries `south-task-core` and `south-task-conformance` |

The three guest-linked crates use independent versions so a repository release that leaves them
unchanged does not re-identify every component package. Component manifests carry their own
package versions and minimum compatible `south_runtime`.

## Crate ownership

| Crate | Current responsibility |
|---|---|
| `south-contracts` | Bounded HTTP, authentication, response, error, streaming, quota, media, and speech contracts |
| `south-core` | Endpoint binding, explicit credential resolution, deadlines, cancellation, and raw-call wrappers |
| `south-host-grammars` | Pure bounded host-only SSE and event-stream grammars; no component may link it |
| `south-transport-reqwest` | Hardened buffered and streaming execution with explicit headers, caps, and redaction |
| `south-provider-conformance` | Immutable public provider and host-integration fixture suites |
| `south-testkit` | Reusable runners and reference executors for host conformance |
| `south-provider-api` | Component WIT packages and the manifest schema |
| `south-component-conformance` | Package admission, behavior suites, native references, and frozen fixtures |
| `south-provider-runtime` | JSON-faced Wasmtime execution, resource limits, gated imports, and model catalogs |
| `south-north-codec` | Pure OpenAI Chat, Responses, and Anthropic Messages mappings to canonical IR |
| `south-task-core` | Candidate host-neutral task workflow sequencing with explicit effects |
| `south-task-conformance` | Candidate atomic task-effects conformance vocabulary |

## Dependency graph

This graph shows direct production dependencies plus optional-feature edges. It intentionally
excludes development-only dependencies.

```text
south-core -------------------------------> south-contracts
south-transport-reqwest ------------------> south-core
south-transport-reqwest ------------------> south-contracts
south-provider-conformance ---------------> south-contracts
south-provider-conformance ---------------> south-provider-api
south-provider-conformance ---------------> south-host-grammars
south-testkit ----------------------------> south-contracts
south-testkit ----------------------------> south-core
south-testkit ----------------------------> south-host-grammars
south-testkit ----------------------------> south-provider-api
south-testkit ----------------------------> south-provider-conformance
south-component-conformance --------------> south-contracts
south-component-conformance --------------> south-provider-api
south-component-conformance --------------> token-station-protocol (pinned kernel tag)
south-component-conformance --sandbox-----> south-provider-runtime (optional)
south-provider-runtime -------------------> south-provider-api
south-north-codec ------------------------> token-station-protocol (pinned kernel tag)
```

`south-host-grammars` has no production dependencies. Only `south-transport-reqwest` owns a network
client, and only `south-provider-runtime` owns the Wasmtime engine. The conformance and north-codec
kernel edges are explicit sanctioned exceptions: both pin the distribution kernel revision.

No crate owns a database, cache, migration directory, host repository dependency, credential
source, or implicit network path.

## Six-layer runtime model

### 1. Host policy

The host authenticates callers, selects a package and provider binding, enforces tenant and quota
policy, reserves funds, supplies credentials and configuration, persists state, settles usage, and
authorizes delivery. These decisions never move into South.

### 2. Canonical request and package selection

The host maps its northbound request to canonical IR, selects one package digest, and supplies the
package's declared values. The package does not route between upstreams and cannot discover host
state.

### 3. Component translation

The selected component performs pure translation: metadata, request construction, response and
stream parsing, provider-error mapping, and task observation or media rendering. Native references
and Wasm guests share the same implementation.

### 4. Runtime mediation

`south-provider-runtime` loads one isolated package instance, checks its manifest and reported
identity, links only admitted imports, applies payload and memory ceilings, and enforces epoch
deadlines. The runtime surface remains JSON-only and does not consume canonical IR.

### 5. Host effects and transport

The host resolves secrets and declared credential recipes, finalizes signatures, and supplies
explicit time and cancellation. `south-core` and `south-transport-reqwest` execute only prepared,
bounded operations. Redirects, retries, cookies, compression, referer propagation, implicit system
proxies, and ambient credentials remain disabled.

### 6. Evidence and settlement

South parses objective response facts and usage. The host applies prices, records durable outcomes,
settles funds, and releases output. Conformance evidence proves a bounded interface; it does not
replace host integration review or live upstream validation.

## Component worlds

All official packages and evidence links are in the [component catalog](docs/components.md).

### Provider adapter v2

Provider components translate canonical chat IR into bounded calls and translate buffered or
streaming responses back into canonical facts. Declared capabilities cover chat, tools, structured
output, stream framing, request facts, immutable fields, client-protocol pass-through, credentials,
signing, and quota metadata.

Kiro currently accepts inline PNG, JPEG, GIF, and WebP images in supported user turns and maps them
to its `userInputMessage.images` form. Remote, malformed, unsupported, and misplaced image inputs
remain refused or dropped according to the package contract. See the
[Kiro image design](docs/design/2026-10-19-kiro-image-input.md).

The Bailian OpenAI-compatible family declares a reasoning budget location and overhead. The host
can seal an authorized output cap while the component divides it between thinking and answer
budgets. See the [Bailian reasoning-cap design](docs/design/2026-10-21-bailian-thinking-cap.md).

### Task adapter v1 and v2

Task components translate submit, observe, and render operations for asynchronous provider work.
The host owns polling schedules, durable locators, credentials, recovery, price selection,
settlement, and delivery authorization. Task v2 carries bounded locator, usage, estimate, artifact,
and rendering facts without acquiring those host responsibilities.

The current Bailian task package supports Wan media inputs for the declared Wan 3.0, image-to-video,
reference-to-video, and video-edit families, including bounded image, video, audio, frame, and clip
rules. See the [Wan media design](docs/design/2026-10-21-bailian-wan-media.md).

### Embeddings adapter v1

Embeddings components translate text and token-id inputs; packages declaring embeddings contract 2
may also accept bounded inline media. The world imports no host functions. Hosts retain HTTP,
credential execution, vector extraction, pricing, and settlement. See the
[embeddings design](docs/design/2026-09-30-embeddings-contract.md).

### Image adapter v1

Image components translate generation and editing requests, bounded media transformations, and
inline or URL artifacts. Hosts execute safe artifact fetches and enforce delivery policy. See the
[image-world design](docs/design/2026-09-30-image-world.md).

### Speech adapter v1

Speech components translate synthesis, transcription, translation, and declared artifact fetches.
Qwen currently supports ASR for `qwen3-asr-flash`, with bounded data-URL audio, optional prompt and
language controls, JSON or text output, and seconds-based usage. See the
[Qwen ASR design](docs/design/2026-10-21-speech-qwen-asr.md) and
[speech-world design](docs/design/2026-09-30-speech-world.md).

## Four conformance gates

1. **Package admission.** Validate schema, world, contract tuple, declared runtime, endpoint and
   authentication declarations, reported identity, and forbidden imports before loading code.
2. **Component behavior.** Run frozen fixture suites against native references and Wasm packages;
   judge determinism, translation, refusal, endpoint confinement, stream incrementality, usage,
   and error mapping.
3. **Host integration.** Run public suites against real host executors for transport, credentials,
   signing, framing, pass-through delivery, safe fetch, and other host-owned effects.
4. **Runtime enforcement.** `south-provider-runtime` enforces the sandbox on every call: admitted
   imports only, memory and payload ceilings, epoch deadlines, and stream-instance limits. This
   gate has no fixture suite; the runtime's construction is the evidence.

Passing an earlier gate does not imply a later gate passed. Recorded host status and suite versions
come only from [compatibility.json](compatibility.json).

Upstream validation is not a conformance gate. Where a design record requires captured traffic or
staged evidence, behavior is verified against the real provider, and that evidence sits beside the
four gates rather than replacing any of them.

## Compatibility handshake

Admission evaluates each package independently against the host range:

- `runtime_abi` must match exactly.
- Kernel contract identifiers, including canonical IR, must match exactly.
- Each host-decoded South contract version must fall inside the host-supported range.
- The package's `south_runtime` is the oldest runtime it needs, and admission requires
  `host.south_runtime_min <= package.south_runtime <= host.south_runtime`.
- Per-package failure is isolated; one rejected package does not invalidate unrelated packages.
- Rejection never selects a native fallback. A host must fail closed or choose another explicitly
  admitted package through normal routing policy.
- If multiple admitted packages declare the same family, the binding pins a specific package
  digest. Family names alone never resolve an ambiguous implementation.

The manifest tuple is necessary but not sufficient. [compatibility.json](compatibility.json) is the
authority for repository contract versions, host-supported ranges, conformance suite versions,
package adoption, and verification status. The [release index](docs/releases.md) records what was
prepared; it does not override adoption evidence.

## Security and ownership boundaries

Components have no network, filesystem, environment, clock, randomness, or secret access by
default. WASI has no preopens, environment, or sockets. Imports are closed and admitted per world;
memory, payload, call-time, and stream-instance bounds are explicit.

Hosts own secret material and generic credential execution. Components may declare credential
fields, recipes, request signing, and allowed destinations as data, but they never read a keychain
or tenant store. Service-account private keys, encryption keys, refresh concurrency, and database
correctness remain host-side. A request finalizer marks where a host signature belongs without
moving signing material into South.

South carries objective facts such as tokens, seconds, images, characters, provider costs, and
settlement outcomes. Hosts own price tables, markups, wallet selection, quota policy, attribution,
funding transactions, and tenant-facing decisions.

The reqwest transport accepts only the prepared method, URL, body, headers, auth material, and
timeouts supplied through contracts. Reserved-header checks, declared secret redaction, bounded
quota capture, disabled redirects, and explicit proxy behavior prevent ambient policy from leaking
into an execution.

See [Security](SECURITY.md), the
[provider-runtime design](docs/design/2026-08-21-provider-runtime.md), and the
[host-zero-vendor boundary](docs/design/2026-09-30-host-zero-vendor-boundary.md) for detailed
threat and ownership decisions.

## Host integration rules

Hosts that consume `south-transport-reqwest` keep its reqwest node aligned with the host's primary
stack at the repository-pinned version. The unified feature set includes `rustls-tls` and `stream`
and excludes `default-tls`, `native-tls`, `cookies`, and `system-proxy`. Unrelated pre-existing
reqwest versions gain no new dependents.

During migration a kernel crate may re-export South types under legacy Rust paths, but it must not
define duplicate nominal types. South never depends back on that compatibility layer. Production
double-send is not a migration strategy; hosts use fixture parity, bounded dual-run evidence, and
explicit cutover decisions.

## Reference map

- [Component catalog](docs/components.md)
- [Release index](docs/releases.md)
- [Compatibility manifest](compatibility.json)
- [Contributing rules](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Provider API promotion](docs/design/2026-08-21-provider-api-promotion.md)
- [Component conformance](docs/design/2026-08-21-component-conformance.md)
- [Canonical IR inventory](docs/design/2026-08-21-canonical-ir-inventory.md)
- [Task v2 candidate](docs/design/2026-09-20-task-adapter-v2-candidate.md)
- [Task request estimate](docs/design/2026-09-20-task-request-estimate.md)
- [Shared task core](docs/design/2026-09-20-shared-task-core.md)


## Explicit transport observation

The reqwest transport accepts a host callback for buffered transport stages and bounded recent connection history. It owns only its I/O clock; the host owns correlation, collection, persistence and any global tracing. IDs and payloads are bounded, and credentials, bodies and socket addresses are never emitted. Tuple history is not proof of physical reuse. Upstream application receipt and queue timing remain unknown without a verified server contract.

The optional local-write experiment is carried as a checksum-pinned [preparation bundle](patches/reqwest-0.13.5-local-write/README.md), not a default dependency on a private reqwest API. Preparation builds a separate, consistent South path snapshot and final-host patch roots. Default builds remain compatible with registry reqwest. Local flush is a client boundary, not upstream receipt; neither a mock nor a processing-header difference supplies the missing server clock.
