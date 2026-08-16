# Repository Instructions

- Use English for all code, comments, documentation, diagnostics, logs, tests, and commits.
- Treat `CONTRIBUTING.md`, `ARCHITECTURE.md`, and `compatibility.json` as binding repository rules.
- Follow the pinned Glimpse Rust standards recorded in `CONTRIBUTING.md`.
- Write or update an English design record before changing a public contract or runtime behavior.
- Use test-driven development: verify a public test fails before production implementation.
- Keep changes surgical. Do not add speculative APIs to placeholder crates.
- Never add a dependency on `token-station`, `token-station-server`, a database, or a cache client.
- Never add hidden environment, filesystem, clock, randomness, network, or secret-store access to
  contracts or core code.
- Run every local verification command in `README.md` before declaring work complete.
