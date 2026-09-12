# Remaining implementation gaps and security findings

Resolved findings are intentionally omitted. This is an evidence report, not a
request to weaken any boundary.

1. Marketplace has no allocation status/read endpoint, usage-record endpoint,
   or Marketplace callback handler in the implemented router. Consumers must
   not depend on the older `nodes`, allocation route/fulfilment, or
   `x-marketplace-key` contract described in `docs/marketplace-l0-routing.md`;
   that document is stale relative to `marketplace.rs`.
2. `GET /v1/admin/marketplace` is operator-only, but it is a Hive Admin route,
   not an alternate Marketplace service endpoint.
3. Many Admin handlers return handler-specific JSON rather than a stable
   shared response envelope; the internal spec intentionally leaves those
   schemas broad where code does not declare a stable DTO. Expanding it safely
   requires route-by-route contract work, not guessed schemas.

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
