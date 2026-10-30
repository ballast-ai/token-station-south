# BytePlus (Seedance video)

Upstream facts for [`task-byteplus-v2`](../../components/task-byteplus-v2/manifest.json). Moved from the
token-station-server host documentation on 2026-10-30; the task-byteplus-v2 fixtures are authoritative for
package behaviour.

- **Billing evidence** is `usage.completion_tokens` in the final task-poll response.
- **Tokens are priced per output-resolution tier**: the official card (`docs.byteplus.com`) lists a different
  per-million-token price at 480–720p, 1080p and 4K; Seedance 2.0's 4K rate is the lowest of the three.
