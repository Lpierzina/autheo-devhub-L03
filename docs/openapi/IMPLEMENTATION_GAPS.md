# Implementation gaps and security findings

This is an evidence report, not a request to weaken any boundary.

1. Hive Admin's JWT middleware intentionally permits all reads, including
   `/healthz` and `/v1/mesh`; reads rely on loopback/private-network isolation.
   Do not present those endpoints as application-authenticated.
2. Hive Admin mutation authentication is conditional: when
   `HIVE_JWT_SECRET` is unset, `auth::require_auth` is pass-through. A
   production/private deployment must set it and must retain network isolation.
3. The `api.<platform-domain>` host dispatch in `main.rs` can make the Admin
   router publicly addressable when ingress is configured. The code refuses
   that configuration without JWT enforcement, but operator authorization and
   route-specific read authorization still vary. Treat that as an unsafe
   deployment shape unless separately reviewed.
4. `HIVE_AUTH_BYPASS=1` is a UI-only development escape hatch. `ui/proxy.ts`
   limits it to `NODE_ENV != production`; it must never be configured in
   production. `NEXT_PUBLIC_HIVE_DEV_MINT=1` is likewise local-development
   only because it can mint a development tenant token.
5. Marketplace HMAC nonce records are durable in the marketplace security
   snapshot and expire after ten minutes. They are single-use by
   `key-id:nonce`; clock skew is five minutes. HMAC is enforced by each
   Marketplace handler, not by a global middleware.
6. Marketplace has no allocation status/read endpoint, usage-record endpoint,
   or Marketplace callback handler in the implemented router. Consumers must
   not depend on the older `nodes`, allocation route/fulfilment, or
   `x-marketplace-key` contract described in `docs/marketplace-l0-routing.md`;
   that document is stale relative to `marketplace.rs`.
7. `GET /v1/admin/marketplace` is operator-only, but it is a Hive Admin route,
   not an alternate Marketplace service endpoint.
8. Many Admin handlers return handler-specific JSON rather than a stable
   shared response envelope; the internal spec intentionally leaves those
   schemas broad where code does not declare a stable DTO. Expanding it safely
   requires route-by-route contract work, not guessed schemas.
