# Engineering Lessons

## Paused Tokio time and real sockets

- Do not start a Tokio runtime with time paused while a test still depends on operating-system
  socket readiness. A server-side `write_all` signal proves only that bytes reached the kernel; it
  does not prove that the client runtime has observed readiness. Tokio may therefore auto-advance
  a transport timer before the socket event is dispatched.
- For real loopback timeout tests, complete the connection, response headers, and any required
  first body chunk under the normal clock. Pause time only immediately before the specific pending
  read whose timer is under test.
- A flaky test fix is not accepted from isolated runs alone. Re-run the complete test binary under
  its normal parallel schedule enough times to exercise cross-test runtime and I/O scheduling.

## A conformance case only closes the mutations it can observe

- Adding a case to a frozen table closes some surviving mutations and not others. Which ones is an
  empirical question, and re-measuring is cheap. When `CredentialSlotMismatch` was added to
  `south.provider-quota-metadata.v1`, the adoption record predicted it would kill both mutations
  the two-case table had let through. It killed one. Literal `(1, 1)` evidence now fails on two
  count categories, because the new case expects `(Zero, Zero)`. But an executor that bypasses the
  host assembly layer and calls `south-core` directly still passes, because the binding check that
  rejects the mismatched slot lives in `south-core` — both paths reach it and produce the same
  failure and the same zero counts.
- The general shape: a case can only distinguish two implementations if they behave differently on
  it. A negative case pins the layer that performs the rejection, not every layer the call would
  otherwise traverse. To pin a wrapper you need a case where the wrapper itself changes the
  outcome.
- So do not carry a prediction about mutation coverage into a status. Re-run the mutations against
  the new table and write down what actually happened, including the ones that still survive. An
  adoption note claiming a closed gap that is still open is worse than one that names the gap,
  because the next reader stops looking.
