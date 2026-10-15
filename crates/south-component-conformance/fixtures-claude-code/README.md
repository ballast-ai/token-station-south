# Frozen samples: the Claude Code subscription families (`claude-code`, `claude-code-setup-token`)

Gate ② samples for the two Claude Code families of `provider-anthropic` 1.2.0, judged by
`south.provider-component.v1` (`docs/design/2026-10-15-claude-code-family.md` §8). No row is a capture of real traffic:
lv ruled on 2026-10-15 that this family is migrated by the host's native code, and every value below is transcribed from
the host's native `claude_code` arm (server `gateway/src/modules/inference/engine/claude_code.rs`, `upstream.rs`,
`token_refresh.rs`, `text_admission/sender.rs`; record §2 cites each line).

**This directory is judged together with `fixtures-anthropic/`**, the directory the manifest's `conformance.fixtures`
names. The response-side functions take no configuration, so the family-neutral response, stream, error and
capabilities rows are not repeated here; and the manifest's `credentials` section owes the `credential.*` rows below,
which only this directory carries. Neither directory passes alone. Every gate ② run of the package (the native test,
the sandbox parity test and the release's gate ② report) loads both (`tests/support/anthropic_packs.rs`). Every row is
named `claude-code-*`, so no name repeats across the two.

Each row's source:

- **Login-token form** (`claude-code`; the native arm's `login_style`, a credential with a refresh token):
  `claude-code-login-chat` (the minted token in `x-api-key`, `anthropic-beta` the CLI's set, `x-app: cli`,
  `anthropic-dangerous-direct-browser-access: true`, `anthropic-version: 2023-06-01`, `accept: text/event-stream` for a
  streaming request, and a string system prompt turned into the identity block followed by the client's text, as
  `inject_identity_prepends_to_string_system`); `claude-code-login-without-system` (the host's
  `claude_code_login_identity_is_injected_and_resealed_before_durable_admission`: `hello`, a cap of 321, the identity
  as the only system block, `accept: application/json` for a buffered request; `inject_identity_adds_when_absent`);
  `claude-code-login-identity-already-first` (`inject_identity_idempotent_for_prefixed_string`);
  `claude-code-login-marked-system-blocks` (a system prompt with a prompt-cache marker is a block array, which gains
  the identity as its first block, the array arm of `inject_identity`); `claude-code-login-marked-identity-already-first`
  (`inject_identity_idempotent_for_prefixed_array_block_0`); `claude-code-login-end-user-hash` (the host's
  `end_user_hash` becomes `metadata.user_id` as for the `anthropic` family, record §5 D4).
- **Setup-token form** (`claude-code-setup-token`; no refresh token): `claude-code-setup-token-chat` (the host's
  `claude_code_setup_token_prepares_an_exact_messages_request_before_dispatch`: `Authorization: Bearer`,
  `anthropic-beta: oauth-2025-04-20`, no system block); `claude-code-setup-token-system-untouched` (a system prompt
  stays the string the `anthropic` family sends; no identity, `x-app` or browser-access header).
- **Credential rows** for the recipe `claude_code` (record §4), run by gate ②'s `CredentialRecipeMatch` with south's
  reference interpreter: `rotation.claude-code-refresh` (the JSON refresh-token grant with the CLI's `client_id`,
  rotation and write-back, `expires_in` as the expiry), `rotation.claude-code-rotation-without-new-token` (nothing
  written back: the stored refresh token stays), `clock.claude-code-expiry-missing-uses-the-default` (no `expires_in`:
  the 60-second `default_seconds`, record §6 D6), `clock.claude-code-expiry-clamped-to-a-day` (the host's 24-hour
  clamp), `clock.claude-code-no-refresh-token` (a configuration error naming `refresh_token` before any recipe runs),
  and `on-status` 400 and 401 (`reauth_required`, the native arm's terminal statuses), 403, 429 (`transient`, the
  `4xx` class) and 500 (`transient`, the default). Every token is fake.

The user-agent `claude-cli/2.1.114 (external, cli)` is not in any row: `user-agent` is reserved, and the host fills the
controlled slot from the manifest's `user_agent`, which `claude_code_conformance_v1` pins to the native value.

Expectations were written by hand from the native code, then checked by running the reference and the reference
interpreter.
