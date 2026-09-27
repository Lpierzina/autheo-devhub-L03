# Remaining implementation gaps and security findings

Resolved findings are intentionally omitted. This is an evidence report, not a
request to weaken any boundary.

1. `GET /v1/admin/marketplace` is operator-only, but it is a Hive Admin route,
   not an alternate Marketplace service endpoint.
2. Many Admin handlers return handler-specific JSON rather than a stable
   shared response envelope; the internal spec intentionally leaves those
   schemas broad where code does not declare a stable DTO. Expanding it safely
   requires route-by-route contract work, not guessed schemas.

## Required Marketplace operational work

The current private API lacks capacity reservation/hold, allocation
status/read, callbacks, server-authorized project/release/workload handoff,
active-execution evidence, and DevHub-authoritative usage measurement. `POST
/usage-records` remains buyer-authenticated and is not a DevHub/Hive ingestion
surface.

[DevHub L0 operational contract](../integrations/devhub-l0-operational-contract.md)
is the required external specification before these routes are implemented.
It defines authentication, tenant/order/release binding, idempotency and
replay handling, reconciliation, error semantics, and operational ownership.
The endpoints described there remain an implementation gap; the document does
not make them available through the existing router.

## Resolved boundaries

- With `HIVE_JWT_SECRET`, every Admin read except minimal `/healthz` requires a
  verified JWT or tenant API key. Route handlers retain tenant gates and
  platform-wide operations still require the independently-derived
  `platform_admin` claim; tenant `role: owner` is not platform authority.
- The Admin listener accepts loopback by default. A private RFC1918/IPv6-ULA
  management bind requires both `HIVE_ADMIN_PRIVATE_NETWORK=1` and JWT
  enforcement. Public, wildcard, and link-local binds fail startup.
- Public host dispatch now rejects `api.`, `admin.`, `webhook.`, and
  `api-<region>.` rather than forwarding them to Admin. There is no supported
  public Admin, Swagger, Marketplace, or webhook publication topology.
- Marketplace routes are protected as one HMAC-gated router. The middleware
  verifies the exact raw body, five current headers, canonical signature,
  constant-time MAC, timestamp skew, and durable nonce before invoking a
  handler. A nonce is consumed only after the other checks pass.
- `HIVE_AUTH_BYPASS=1` is inert in a production dashboard. Development minting
  also requires non-production mode, the bypass flag, and a loopback
  `HIVE_ADMIN`; `NEXT_PUBLIC_HIVE_DEV_MINT=1` alone cannot mint a token.
