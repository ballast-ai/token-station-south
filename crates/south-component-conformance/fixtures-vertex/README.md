# `fixtures-vertex`

Gate ② pack (`south.provider-component.v1`) of `provider-vertex` (family `vertex-ai`), Gemini models on Vertex AI for
text (`docs/design/2026-10-15-provider-vertex.md` §3 and §4), including the package's `credential.*` cases.

The request body, the response reader, the stream parser and the refusals are the crate-private module `gemini_text`,
which `provider-gemini` shares and `fixtures-gemini/` already covers row by row; the suite test
(`tests/vertex_conformance_v1.rs`, `everything_but_the_route_and_the_auth_is_the_gemini_package`) holds the two references
to the same output over every case of that pack. This pack carries what is Vertex's own (the route, the location rules,
the `bearer` auth on the minted slot) and mirrors `fixtures-gemini/` so the suite's required rows run against this
package's manifest.

Rows, and where their expectations came from (every expectation was written before the reference first ran):

- **Mirrored from `fixtures-gemini/`** (adapted by rule, not re-derived): each request case with the provider row moved to
  `vertex-ai` on `https://us-central1-aiplatform.googleapis.com` with `region` `us-central1` and the exported
  `project_id` `test-project`; the expected URL is the Vertex path
  `v1/projects/test-project/locations/us-central1/publishers/google/models/{model}:{method}` with the Gemini case's own
  model segment and operation, and the auth `bearer` on `provider_api_key`; method, headers and body unchanged. Gemini's
  `provider.request.chat` is here `provider.request.system-and-sampling`. The capabilities case moves its row the same
  way. Every response, stream and error case (and the `usage` sidecar) is copied byte for byte, since the reader is
  shared.
- **Transcribed from the server repository** (read at `origin/dev-v2`): `provider.request.chat` is the request of
  `gateway/tests/gemini_thinking_exhausted.rs` `vertex_chat_nonstream_thinking_exhausted_returns_length_and_settles`
  (region `us-central1`, the credential's project `test-project`, upstream model `gemini-2.5-flash`, one user turn
  `hi`, `max_tokens` 16, the path `/v1/projects/test-project/locations/us-central1/publishers/google/models/
  gemini-2.5-flash:generateContent` and a `Bearer` token). The same file's upstream answer (a candidate without
  `content.parts`, `finishReason` `MAX_TOKENS`, `usageMetadata` without `candidatesTokenCount`: prompt 12 + thoughts 16 =
  total 28) is `provider.response.thinking-exhausted-bills-the-thoughts-as-output` and, as its one-frame `alt=sse` twin,
  `provider.stream.thinking-exhausted-single-terminal-frame`: finish `length`, input 12, output 16 of which 16 reasoning,
  as the host asserts for both.
- **The location rules** (record §3, I3-3 §4, as `fixtures-image-vertex/`): the `global` location on the unprefixed
  origin with `gemini-3.8-flash` (the model the server's `engine/vertex.rs` names as served only there); a regional
  location (`europe-west4`); a proxy base used as given, non-streaming and streaming (the query follows the path);
  the `project` override `override-project` over the exported `proj-a`; the exported `proj-a` alone (both values from
  the server's `handler/images/tests/gemini.rs`); and a Vertex AI base URL of another region refused with a 500 naming
  the expected origin (P2).

Not in this pack: a missing `region`, a missing project and the prefixed `global` host. They are 500s like the region
mismatch, and are pinned by name, with the mismatch, in `tests/vertex_conformance_v1.rs` (`CONFIGURATION_REFUSALS`, run
against a request the package would otherwise refuse, so the configuration error is shown to come first) and through
the guest in `tests/vertex_sandbox_parity_v1.rs`.

Credential cases: the five `credential.*` cases of `fixtures-image-vertex/` (themselves `fixtures-embeddings-vertex/`'s),
copied byte for byte; the packages declare the same `credentials` section, so the same samples judge all of them.

**Unverified**: no row is a capture of Vertex AI traffic. The text arm's error bodies are taken to be the `google.rpc`
envelope the Gemini API answers with, and Vertex's per-chunk `usageMetadata` is the SF27 shape recorded in
`docs/design/2026-09-30-host-zero-vendor-boundary.md` §13.10.
