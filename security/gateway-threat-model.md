# Gateway threat model, tenant boundary, and deployment topology (T134)

## Tenant boundary

Per-account session isolation; no cross-tenant data access; tenant-scoped
rate limits and quotas.

## Deployment topology

- client -> gateway over TLS (WebSocket);
- gateway -> target host over SSH;
- the gateway isolates tenants; there is no direct client-to-target path;
- the gateway has no inbound control-plane exposure beyond the authenticated
  proxy.

## Threat controls

| Area | Threat | Controls |
| --- | --- | --- |
| SSRF | tenant reaches internal metadata endpoints | target allowlist; link-local/metadata ranges blocked; private ranges default-deny |
| Internal access | pivot into the internal network | per-tenant allowlist; audit every target connection; default-deny internal ranges |
| Credentials | credentials stored/forwarded insecurely | never stored on the gateway; keys stay client-side; no plaintext forwarding |
| Audit | abuse without a trace | content-free audit; retention policy; no secrets in logs |
| Rate limit | tenant floods the gateway | per-tenant limits; connection quotas; backpressure |
| Abuse | scanning / spam | suspension policy; pattern-based abuse detection; escalation workflow |

`security/gateway-threat-model.json` is the machine-readable model; the
security review gate (all six areas covered) is enforced by
`scripts/lint-gateway-threat-model.mjs` and
`scripts/test-gateway-threat-model.mjs`.