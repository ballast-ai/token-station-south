# Security Policy

Token Station South is in bootstrap status and has no supported production release yet. Report
suspected vulnerabilities privately through GitHub's security advisory interface for this
repository. Do not include real credentials, customer content, or personal data in an issue.

## Security invariants

- South never reads host databases or secret stores.
- Resolved credentials never enter provider components or serializable provider contracts.
- Provider components have no network, filesystem, environment, or secret capabilities by default.
- Ordinary provider headers have explicit count, name, value, and total-byte limits and cannot set
  versioned host-reserved authentication, framing, or hop-by-hop headers.
- Request and response bodies, credentials, and tokens are excluded from errors and logs.
- No code may introduce `unsafe` without an explicit security review and documented invariants.
