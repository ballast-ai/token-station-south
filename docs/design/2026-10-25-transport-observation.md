# Explicit transport observation and optional local write instrumentation

The host needs provider exchange stages without logging payloads or credentials. Transport may observe its own I/O and clock; contracts/core and provider packages gain no environment, database, cache, host or telemetry dependency.

Add opt-in synchronous TransportObserver callbacks for send start/error, request-body delivery, response headers, bounded body first chunk/complete/error, and bounded connection history. The host supplies an opaque x-request-id (1..128 ASCII alphanumeric, underscore or hyphen); it must not encode credentials or personal data. Invalid/missing IDs disable observation, not requests. Callbacks must return promptly and must not panic or block. Default execution stays on official reqwest =0.13.5 with proxies/redirects/retries/decoding, caps, timeouts and allocation ownership unchanged.

Bound connection history to 256 socket tuples and 60 seconds with independently increasing IDs. A history hit means previously observed, not proven physical reuse: the OS can reuse tuples. Expiration/eviction and counter exhaustion do not claim a new connection. No addresses leave the tracker. Lock failure yields unknown rather than failing the request.

Standard transport cannot prove upstream application receipt, queue admission, pure pool/DNS/TLS time or socket flush. Body delivery is not upload completion. Keep these unknown; processing-header differences are not queue/network measurements.

Carry local-write instrumentation as a reproducible optional patch bundle, not a default dependency on a private reqwest API. Pin the upstream 0.13.5 archive checksum, exact HTTP and South wiring patches, source preimage checksums, preparation tooling and external-host smoke. Prepare in a new directory, never mutate the default workspace or use a host-project absolute path. Cargo patches belong to the final host root and do not propagate from South. Checksum/patch failure must leave no ready output.

The optional hook counts only bytes accepted by the inner HTTP/1.1 connection and successful flush after the complete body. It does not prove remote ACK, application receipt or queue time. Unsupported framing, incomplete/failed writes and cancellation never become flush success. Parser property and scheduled fuzz coverage accompany deterministic partial/vectored/Pending/error tests. This change claims no official SDK release, host verification or stable latency SLA.

Validation: public failing-before-implementation test; default workspace README matrix; optional bundle checksum/failure tests, patched transport/HTTP short tests, external-host compile and Clippy. Project-specific API results remain in the host project; credentials and match media are excluded.
