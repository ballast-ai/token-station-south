# Client policy headers: forwarding a header whose meaning does not depend on the body

Status: **accepted** 2026-10-31 (lv: the client decides, and the header is forwarded only when the client sends it). The
upstream facts in §7 are **unverified**.

Date: 2026-10-31

Baseline: south `origin/main` = `3bbcb8ae` (release 0.82.0). Host references carry a `server:` prefix and refer to
token-station-server `dev-v2` = `a6cebbbe6`.

## 1. Problem

A client of a Chat Completions family sent an upstream's request header that selects how the upstream inspects content.
The request was rebuilt from the IR, so the host did not forward the header, and the upstream handled the request under
its default policy.

`forwarded_client_headers` cannot express this. It is admitted only for a family that declares
`north_request_passthrough` (Messages pass-through record §4.3, Q3), and the host applies it only on pass-through and
counting requests. A family that always rebuilds the body cannot forward anything.

Q3 is right for what it decided. Its reason is that a beta flag goes with the client's body, and a flag for a body the
upstream never sees can only mislead. That reason does not reach a header whose meaning does not depend on the body.

Widening `forwarded_client_headers` to rebuilt requests is the wrong fix: it would send `anthropic-beta` with requests
the `anthropic` family rebuilds, which is the case Q3 refused. The two kinds of header need two declarations.

## 2. Decision

Add one provider-world manifest field. A package uses it to say that a header's meaning is independent of the body, so
the host may copy it on every request of that family.

```json
"forwarded_client_policy_headers": { "bailian-openai-compatible": ["x-dashscope-datainspection"] }
```

`forwarded_client_headers`, its rules and its meaning do not change.

## 3. Gate ①

The manifest is refused when, for some family:

- the family is not in `providers`, or the package is not a provider-world package;
- a name is not a lowercase RFC 9110 `tchar` token of at most 64 bytes, repeats, or there are more than
  `MAX_FORWARDED_CLIENT_HEADERS` names;
- a name is in `FORWARDING_REFUSED_HEADERS`, in the package's `secret_headers`, or in the family's
  `immutable_request_headers`;
- a name is also in the family's `forwarded_client_headers`, so one header has one meaning.

Unlike `forwarded_client_headers`, the field does not require `north_request_passthrough`. That is its purpose.

## 4. Gate ②

`forwarded_headers_absent` covers the new field too: the fixture's descriptor and the probes for both stream flags must
carry none of the family's policy headers. A host therefore never merges a client value into a component's. The check
keeps its identifier, so a report reader changes nothing.

## 5. What the host does

For a family that declares the field, on every request it sends to that family, on every northbound surface, the host
copies each declared header the client sent to the upstream request, unchanged. This includes rebuilt requests,
pass-through requests and counting requests.

- A header the client did not send is not added.
- Repeated field lines are combined with `", "`, as for `forwarded_client_headers`.
- A value that breaks the descriptor's header rules is refused with `invalid_request`, never truncated.
- The host forwards no other client header and writes no vendor name.
- A request that moves to another target is decided again from that target's family.

## 6. What this does not decide

The field says a header **may** travel. It does not decide who may use it: every caller that reaches a declaring family
can send the header. A package has no view of tenants, so any such control belongs to the host. An operator that serves
callers who should not choose the upstream's policy must not bind those models to a declaring family until the host has
that control.

## 7. `bailian-openai-compatible`

`provider-openai-compatible` declares `x-dashscope-datainspection` for `bailian-openai-compatible` only. The name is
stored lowercase, and HTTP header names are case-insensitive, so a client's `X-DashScope-DataInspection` matches.

It stays off `openai-compatible`: that family serves any upstream with an OpenAI-compatible API, and the header would
reach all of them.

The following is **unverified**. Public material describes the value `{"input":"cip","output":"cip"}` as enabling an
optional guardrail that needs authorisation on the platform. A community report describes
`{"input":"disable","output":"disable"}` and says it needs an allow-list from the vendor. No test captured real traffic.
The field makes forwarding possible; it does not promise that an upstream honours a value.

## 8. Alternatives considered

- **Widen `forwarded_client_headers`.** Refused in §1.
- **A per-name scope inside `forwarded_client_headers`.** Changes the type of a published field and refuses every
  released manifest reader for one extra boolean.
- **A static request header on the model.** Works with no contract change, and sends the value on every request of the
  model whether or not the client asked. The requirement is that the client decides.

## 9. Changes

| Crate / package | Change |
| --- | --- |
| `south-provider-api` | field `forwarded_client_policy_headers` with gate ① rules; manifest error variant |
| `south-component-conformance` | `forwarded_headers_absent` reads the new field |
| `components/provider-openai-compatible` | the declaration of §7; `south_runtime` raised to the release that carries the field |
| every package | Q47 version bump for the `south-provider-api` change |

## 10. Questions

- **Q1.** Should a host be required to offer per-caller control before using the field? Recommended: no; south cannot
  see tenants, so §6 states the consequence and leaves the control to the host.
