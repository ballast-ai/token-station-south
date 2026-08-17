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
