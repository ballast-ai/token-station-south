# Which upstreams a text family covers: the `openai-responses` endpoint and one family per Responses upstream

Status: **proposed** 2026-10-29. Requested by lv for the server requirement R5 below. No code changes yet.

Date: 2026-10-29

Baseline: south `origin/main` = `ec9728ec` (release 0.81.0 plus docs). Host references carry a `server:` prefix and
refer to token-station-server `dev-v2` = `b28bff82f`. Statements marked **unverified** come from reading code, not
from running anything or capturing traffic.

## 1. Problem

The server requirement `server:docs/plans/2026-10-29-文本上游协议适配与South正式支持闭环需求.md` comes from a
deployment where one text model failed only when the request carried function tools. Its row was bound to a Chat
Completions family; the upstream wanted the Responses wire for that combination. Rebinding the model to a row of the
`openai-responses` family made the requests succeed. The requirement's §8 (R1, the coverage check against 0.81.0) found
two gaps; R5 asks whether package declarations can let the host refuse such misconfigurations, and allows the answer
"not from today's declarations, unknown stays unknown".

There are two different questions behind the incident.

1. **Is the row's upstream one the family's evidence covers?** The Responses record rules that `openai-responses`
   vouches for OpenAI's public API only, and that another upstream is routed to it only after the package carries a
   fixture pack captured from that upstream's traffic (Responses record §3.3, R-Q16, lv 2026-10-10). Nothing a host
   can read says which upstreams those are:
   - `provider-openai-responses` declares no `endpoint` template for `openai-responses` (it has one only for
     `bedrock-mantle`), so the host sends to whatever API root the row names;
   - §3.3 says the release index lists the upstreams that have a pack; the 0.81.0 index entry carries name, version,
     families, capabilities and compatibility, and no such list.

   So a row of `openai-responses` pointed at another upstream is accepted when saved and when sealed. If the deployment
   in the incident points at a third-party upstream, that is exactly this case.
2. **Does this model need a different wire from the one its family speaks?** That is a fact about one upstream model
   ("refuses Chat Completions when tools are present", "Responses only"). Generic families such as
   `openai-compatible` serve operator-chosen upstreams and models; the package does not know the model, and the fact
   changes without a package release.

## 2. What already exists

`ComponentManifestV1::endpoint` (crates/south-provider-api/src/manifest.rs) maps a provider-world family to an `https`
endpoint template whose parameters are the family's `config_schema` keys. `endpoint_admits(family, base_url)`
(crates/south-provider-api/src/config.rs) answers whether a row's API root is one the template produces; a family
without a template admits any root.

The host already enforces it in two places (**unverified**, from reading `server:` code):

- **When a provider row is saved** (rule R4, `server:gateway/src/modules/catalog/service.rs`
  `validate_component_provider`): with the package loaded, a row without configuration values must have an endpoint
  the template admits, otherwise 400.
- **When a request is sealed** (`server:gateway/src/modules/inference/engine/text_admission/component_facts.rs`
  `component_base`): the same check; an endpoint outside the template is refused before admission and the upstream is
  not called. A row saved before a template existed is caught here.

Families that already pin their upstream this way: `anthropic`, `claude-code`, `claude-code-setup-token`, `gemini`,
`gemini-openai-compatible`, `github-copilot` (parameter `plan`), `bedrock`, `anthropic-bedrock-invoke` and
`bedrock-mantle` (parameter `region`). Host tests reach a mock upstream for these through a test-build override of the
send origin, applied after the check (`server:…/credential_recipe/test_hooks.rs` `override_component_origin`).

Provider families without a template: `openai-responses`, `openai-codex`, `openai-compatible`, `azure-openai-v1`,
`azure-openai-dated`, `glm-coding`, `kimi-code`, `bailian-openai-compatible`, `vertex-ai`, `kiro`.

## 3. Proposal

### 3.1 `openai-responses` declares its endpoint

```json
"endpoint": {
  "bedrock-mantle": "https://bedrock-mantle.{region}.api.aws",
  "openai-responses": "https://api.openai.com/v1"
}
```

The value is the API root the family's fixture pack already uses (all 33 `base_url` values in
`fixtures-openai-responses/` are `https://api.openai.com/v1`) and the one the host's runbook tells operators to write.
After a host re-pins:

- saving an `openai-responses` row with another API root is refused (R4, 400 naming the family and the template);
- an existing row with another API root is refused before admission on every request, upstream not called;
- nothing changes for a row that already names the official root.

No contract crate changes and no new host code: the field, its grammar, `endpoint_admits` and both host checks exist.

### 3.2 A third-party Responses upstream gets a family of its own

One template per family means `openai-responses` cannot also admit a second upstream's root. A third-party Responses
upstream is therefore served by **its own family** in `provider-openai-responses`, the way `bedrock-mantle` is:

- the same reference implementation, parameterised by family where the upstream differs;
- its own `endpoint` template (the upstream's API root);
- its own fixture pack `fixtures-<family>/`, captured from the upstream's real traffic (request, non-streaming body,
  stream, usage with cache and reasoning where offered), judged by the same suite;
- a README row naming the capture date, conditions and package version (server requirement A7).

This amends R-Q16. It ruled "admitted to `openai-responses`, each with its own captured-traffic fixture pack, while its
wire and usage match OpenAI's; an upstream that departs gets its own family". The amended rule: **every** third-party
upstream gets its own family with its own pack, whether or not its wire matches OpenAI's. The evidence requirement is
unchanged; what changes is where the evidence is attached. In exchange, the families listed for the package in the
release index are the list of covered upstreams that §3.3 promised, and the host can enforce coverage with the check it
already has.

### 3.3 Considered and not proposed: a new `served_upstreams` declaration

A per-family declaration in the shape of `served_models` (`{"hosts": [...]}`, `{"host_suffixes": [...]}` or
`{"any": true}`) would let one family admit several upstreams. It is not proposed:

- it is a manifest field, so every package re-identifies and declares a higher `south_runtime`, and the host needs a
  new check at row save, at seal and in its deployment checks;
- it buys a shorter manifest only: each upstream still needs its own captured pack (§3.2), and a family per upstream
  costs one `providers` entry and one template;
- it would sit beside `endpoint` and could disagree with it.

It can be revisited if a family ever has to admit a set of hosts that a template cannot express.

### 3.4 What stays unknown

Question 2 of §1 has no package fact, and none is proposed:

- a family that serves operator-chosen upstreams (`openai-compatible`) cannot declare which of their models need the
  Responses wire, and the fact changes without a package release;
- the host must not infer it from upstream error text or keep a model list (server requirement R5), and must not retry
  on the other wire after an upstream error (the first call may already have been charged).

So the misbinding in the incident stays undetectable when the upstream is reached through a generic Chat family.
The operator's check is the post-deployment tool round trip on the three surfaces (server E2E journey J25) after a
model is bound. Once the upstream is identified, §3.1 and §3.2 decide whether the corrected binding is covered.

## 4. Applying it to the incident

The requirement does not name the upstream; the server asks for it in a south issue. The outcome depends on it:

- **OpenAI's public API**: the corrected binding is covered; §3.1 changes nothing for it. Still missing: captured
  traffic for that family (the pack is documentation-derived) and a streaming row with more than one tool call.
- **A third-party upstream**: the corrected binding is outside the family's evidence today. Under §3.2 it needs a new
  family with a captured pack. If §3.1 ships first, the deployment's row is refused once the host re-pins (§5).

## 5. Compatibility and release

- **Package**: `provider-openai-responses` takes a minor (a behaviour change for rows with another API root); the
  manifest field exists, so `south_runtime` is unchanged. No contract crate changes; Q47 applies to the package only.
- **Hosts**: no code change. A deployment whose `openai-responses` rows name another root loses those rows on re-pin
  (refused before admission). Release notes must say so, and the server's deployment documentation must tell operators
  to check their rows before re-pinning.
- **Host tests** that bind an `openai-responses` row to a mock upstream must write the official root and reach the mock
  through the send-origin override, like the Anthropic and Mantle tests. Twelve server test files mention the family
  and do not use the override today; how many of them bind a mock upstream is **unverified**.
- **Tests (south)**: a manifest pin of the template; `endpoint_admits` refuses another root and admits the official
  one with and without a trailing slash; the fixture pack is unchanged.

## 6. Host checklist (after re-pinning)

1. No code change for the check itself (R4 and the seal check).
2. Move host tests that use `openai-responses` rows to the official root plus the send-origin override.
3. Runbook: the `openai-responses` row writes `https://api.openai.com/v1`; another Responses upstream is a different
   family, available only when the package ships it.
4. Recommended, not required: a deployment check for text rows (like the media ones) that lists rows whose endpoint a
   loaded family's template does not admit, so operators see them before a re-pin instead of at request time.

## 7. Questions for review

- **UQ-1** Pin coverage with the existing `endpoint` template (§3.1) rather than a new `served_upstreams` declaration
  (§3.3)? Recommended: **the existing template**.
- **UQ-2** Amend R-Q16 so that every third-party Responses upstream gets its own family with its own captured pack
  (§3.2)? Recommended: **yes**.
- **UQ-3** The template value: only `https://api.openai.com/v1`? Recommended: yes. Any other official API root is added
  when an operator needs it, as a template parameter with its evidence.
- **UQ-4** Release timing. §3.1 refuses any existing row that points `openai-responses` at another upstream, and the
  incident's upstream is not yet identified. Recommended: **hold §3.1 until the upstream is identified**. If it is
  OpenAI's public API, ship §3.1 in the next release. If it is a third-party upstream, ship §3.1 in the same release as
  that upstream's family (§3.2), so the deployment can move its row in one re-pin.
- **UQ-5** The other families without a template (`openai-codex`, `azure-openai-v1`, `azure-openai-dated`,
  `glm-coding`, `kimi-code`, `bailian-openai-compatible`, `vertex-ai`, `kiro`)? Recommended: **out of scope here**.
  Each gets a template when its own record states the hosts it covers. `openai-compatible` stays open by design.
- **UQ-6** Model-level wire requirements on generic families (§3.4)? Recommended: **no declaration**; operators verify
  each binding with the post-deployment tool round trip.
