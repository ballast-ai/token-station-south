# Optional local-write observation for reqwest 0.13.5

This is an opt-in, unreleased preparation bundle. Normal South builds use registry reqwest and do not require this patch. The upstream archive is SHA-256 pinned in manifest.json; the archive's upstream license files are preserved. The patch exposes only fixed stage names, a bounded opaque request ID and a body length to the callback. It never emits headers or bodies. Callbacks must not block or panic.

The local HTTP/1.1 hook proves connection acceptance and a successful local flush after the complete request. It cannot prove remote acknowledgement, upstream application receipt, or queue timing. It supports only POST with one Content-Length and one opaque x-request-id; unsupported framing loses observation without changing I/O. The parser removes exactly the pinned HTTP writer's single encoding SP after the ID header colon, and rejects additional caller whitespace rather than inventing a normalized correlation. Header buffering is bounded and cleared on abort/drop on a best-effort basis, not a cryptographic zeroization guarantee.

## Reproduce in a fresh directory

From a South checkout matching the manifest preimages:

```sh
python3 scripts/prepare_observed_reqwest.py --output /path/to/new-prepared
# For offline preparation, add --crate-archive /path/to/reqwest-0.13.5.crate.
cd /path/to/new-prepared
cargo test -p south-transport-reqwest
cargo clippy -p south-transport-reqwest --all-targets -- -D warnings
cargo run --manifest-path host-check/Cargo.toml
```

Preparation refuses an existing output, validates the archive, patches, source preimages and generated parser hashes, and applies exact hunks without offset/fuzz. Failure removes its temporary output. The prepared manifest records the actual copied input tree, preparer digest, checkout state and generated file hashes. The declared South base is a patch baseline, not proof that a dirty tree equals that commit. Preparation itself does not run tests.

The independent host executable builds the explicit two-argument TransportPairV1::try_new_local_write_observed(config, observer), checks its AsyncHttpTransport identity against the prepared south-core and its error identity against the prepared south-contracts, and sends no request. The normal two-argument try_new_observed remains available after applying the overlay. The HTTP parser is the same checked source included by default transport properties and the scheduled local_write_parser fuzz target.

## Host adoption uses one South source tree

Cargo patch tables do not propagate from a dependency to its host. The generated host-check is a registry-transport/path-core fixture whose final root explicitly patches both reqwest and south-transport-reqwest. Its successful build is not verification of an existing gateway migration.

For a real host, use the complete prepared South snapshot consistently. For example, replace every direct South dependency with its corresponding prepared path (including runtime, conformance and codec crates if the host uses them):

```toml
[dependencies]
south-core = { path = "/path/to/new-prepared/crates/south-core" }
south-contracts = { path = "/path/to/new-prepared/crates/south-contracts" }
south-transport-reqwest = { path = "/path/to/new-prepared/crates/south-transport-reqwest" }

[patch.crates-io]
reqwest = { path = "/path/to/new-prepared/reqwest" }
```

A host keeping a Git South dependency must instead patch that exact Git source, not just crates.io, and cover every South crate participating in its graph. A partial transport-only substitution can leave Git and path copies of south-core/contracts with incompatible Rust types. Use cargo tree -d and public trait/error checks to verify a single source identity. This bundle has not migrated or verified either production host and has not published a South release.

The normal workspace verification matrix remains mandatory for South. The patched fixture additionally runs the existing public tests, the new public local-write executor tests, deterministic server-read/queue gates, parser partial/vectored/Pending/error/cancellation tests and properties. No network API keys or football media are included.
