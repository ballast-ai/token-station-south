# Frozen samples: the Codex subscription backend (`openai-codex`)

Gate ② samples for the `openai-codex` family of `provider-openai-responses` 1.2.0, judged by
`south.provider-component.v1` (`docs/design/2026-09-30-openai-responses-upstream-component.md` §12.1, step R3;
`docs/design/2026-10-12-responses-r3-codex-and-request-fields.md` §3.3). No row is a capture of real traffic: the
measurement of R3 §3.2 is pending, and the request rows follow the host's pinned Codex behaviour, not a recording of
the backend.

**This directory is judged together with `fixtures-openai-responses/`**, the directory the manifest's
`conformance.fixtures` names. The response-side functions take no configuration (record §3.3), so the family-neutral
response, error and capabilities rows are not repeated here; and the manifest's `credentials` section owes the
`credential.*` rows below, which only this directory carries. Neither directory passes alone. Every gate ② run of the
package — the native test, the sandbox parity test and the release's gate ② report — loads both
(`tests/support/openai_responses_packs.rs`). Every row is named `codex-*`, so no name repeats across the two.

Each row's source:

- **Request rows**, from the host's pinned behaviour (server `gateway/tests/south_adoption.rs`) and record §4.3 and
  §10.3: `codex-stream` (a plain-text input lifted to an item list, the default `instructions`, no `temperature`, no
  `max_output_tokens`, `top_p` kept, `store: false`, `stream: true`, `accept: text/event-stream`, the
  `chatgpt-account-id` header from the declared `account_id` attribute, `{base_url}/codex/responses`);
  `codex-without-account` (no attribute, so no header; the request's own instructions win —
  `codex_stream_routes_south_without_account_id`); `codex-store-never-forwarded` (a client `store: true` and
  `previous_response_id` smuggled into `extensions` change nothing — `codex_never_forwards_a_client_store_true`);
  `codex-non-stream-refused` (R-Q5: a capability error at build time; the host's
  `codex_nonstream_routes_south_with_account_id_and_relays_verbatim` becomes this refusal);
  `codex-request-fields-withheld` (RQ3-Q5: none of `user`, `metadata`, `prompt_cache_key`, `truncation`);
  `codex-tools-and-reasoning` (tools, `reasoning.effort`, and `include` for a model declaring
  `reasoning_replay.openai.v1`, NC-1 §2.5).
- **Stream rows**: `codex-host-pinned` is the host's `upstream_responses_sse_body()` byte for byte (data-only frames,
  a `response.created` whose `response` holds only an id, and a terminal with an empty `output`); `codex-text`,
  `codex-tool-call`, `codex-reasoning-encrypted-content` and `codex-incomplete-max-output` (the backend's own limit:
  Codex is never told the cap) are written in the shape of the `fixtures-openai-responses` stream rows. The three
  failure shapes of record §6.4 in Codex's form: `codex-failed` (`response.failed` whose error has no `code`, after the
  host's `codex_response_failed_keeps_the_upstream_error_message`, with a response id added), `codex-error-event` (an
  `error` event nesting an `error` object, no `sequence_number`), `codex-typeless-error-object` (a frame with no
  `type`, after the host's `responses_error_without_event_type_is_normalized_for_clients`).
  `codex-nonzero-tool-usage`: a terminal frame reporting `image_gen` usage is refused (R-Q3, R3 §5) — the Codex form of
  the Responses pack's `response.nonzero-tool-usage`, as a stream row because Codex only streams.
- **Usage sidecars**: every stream row that reports usage carries `provider.stream.<case>.meta.json`, naming the chunk
  of the terminal frame and `/response/usage` in it. `UsageNeverDefaulted` deletes it and requires a provider protocol
  error with no usage event (record §12.2; moved to R3 by §17.4): for a family that only streams, this is where usage is
  proved never to default to zero.
- **Credential rows** for the production recipe `codex` (R3 §2.2), run by gate ②'s `CredentialRecipeMatch` with
  south's reference interpreter: `rotation.codex-refresh` (the rendered request with `client_id` and `scope`, rotation
  and write-back, the fixed 3000-second window ignoring `expires_in`, `account_id` exported), `rotation.codex-rotation-
  without-new-token` (nothing written back), `rotation.codex-account-mismatch` (an `id_token` naming another account:
  `reauth_required`, nothing written back), `clock.codex-fixed-window`, `clock.codex-use-stored` (no refresh token:
  the stored access token is used), `clock.codex-neither-token` (a configuration error naming `access_token`; the
  host refuses such a credential at save time through `require_one_of`), and `on-status` 400, 401 and 429
  (`reauth_required`, the defaults the record keeps) and 503 (`transient`). The `id_token` values are unsigned
  JWTs whose payload names the account; every token is fake.

Expectations were produced by the reference and the reference interpreter, then each was read against the records
and the host behaviour above.
