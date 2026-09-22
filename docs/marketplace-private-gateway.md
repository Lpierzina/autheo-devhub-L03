# Marketplace private mesh gateway

Marketplace applications use `https://devhub-marketplace.internal` as a
stable service URL. The hostname is added only to the configured Marketplace
project's Podman network and resolves to that node's project-bridge gateway;
it is never public DNS.

```text
Marketplace container -- private HTTPS --> node-local gateway
    -- signed Iroh QUIC gossip --> selected eligible DevHub node
    -- raw request --> Marketplace HMAC router
```

The node gateway accepts exactly these paths:

- `GET /v1/marketplace/l0/deployments`
- `POST /v1/marketplace/payment-intents`
- `POST /v1/marketplace/payments/verify`
- `POST /v1/marketplace/l0/allocations`

It copies only the contract headers (`X-Marketplace-*`, `Idempotency-Key`, and
`Content-Type`) plus the original raw body into the mesh envelope. The
destination reconstructs a normal request and enters
`marketplace::routes`, where HMAC verification and durable nonce consumption
occur exactly once. The gateway is not an Admin proxy and cannot reach any
other Admin route.

## Operator configuration

Set these node-local, secret-managed values on every Marketplace-capable node:

```text
HIVE_MARKETPLACE_PROJECT=marketplace
HIVE_MARKETPLACE_GATEWAY_HOST=devhub-marketplace.internal
HIVE_MARKETPLACE_GATEWAY_LISTEN=<this project's private Podman bridge IP>:9443
HIVE_MARKETPLACE_GATEWAY_TLS_CERT=/etc/hive/marketplace-gateway.crt
HIVE_MARKETPLACE_GATEWAY_TLS_KEY=/etc/hive/marketplace-gateway.key
HIVE_MARKETPLACE_GATEWAY_CLIENT_CA=/etc/hive/marketplace-workload-ca.crt
HIVE_MARKETPLACE_WORKLOAD_CERT_ROOT=/var/lib/hive/marketplace-workload-certs
HIVE_MARKETPLACE_HMAC_KEYS=<key-id>:<secret>[,...]
```

The listener refuses wildcard and public binds. It is separate from port 8786,
the public edge, and `api.<platform-domain>`. It requires a client certificate
that chains to `HIVE_MARKETPLACE_GATEWAY_CLIENT_CA`; a TLS client identity is
not an application authorization and never bypasses Marketplace HMAC.

Workload credentials are platform-created root-owned runtime files only:

```text
/var/run/autheo/workload-client/ca.crt   0444
/var/run/autheo/workload-client/tls.crt  0444
/var/run/autheo/workload-client/tls.key  0400
```

The workload must explicitly declare immutable release capability
`workload_client_certificate: { mode: "files-v1", reload: true }`. Missing,
malformed, or unsupported declarations fail readiness; they do not downgrade
to unauthenticated TLS. Credential files are never environment variables,
build inputs, build logs, records, or API/browser responses.

The project runtime contract is stored only in the node's secret environment:

```text
HIVE_MARKETPLACE_RUNTIME_NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY=...
HIVE_MARKETPLACE_RUNTIME_CLERK_SECRET_KEY=...
HIVE_MARKETPLACE_RUNTIME_CLERK_JWT_ISSUER=...
HIVE_MARKETPLACE_RUNTIME_DEVHUB_MARKETPLACE_KEY_ID=...
HIVE_MARKETPLACE_RUNTIME_DEVHUB_MARKETPLACE_SIGNING_SECRET=...
```

Only `NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY` is build-visible. All other values,
including the HMAC secret, are runtime-only project secrets. `DATABASE_URL`
is also runtime-only secret material: it is never a build variable, deployment
record, Marketplace record, API response, browser response, or log.

| Value | Classification |
| --- | --- |
| `NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY` | public, automatically injected at build/runtime |
| `CLERK_SECRET_KEY` | runtime-only secret |
| `CLERK_JWT_ISSUER` | server-only runtime configuration |
| `DEVHUB_PRIVATE_BACKEND_URL` | automatically injected safe runtime configuration |
| `DEVHUB_MARKETPLACE_KEY_ID` | server-only runtime configuration |
| `DEVHUB_MARKETPLACE_SIGNING_SECRET` | runtime-only secret |
| `HIVE_MARKETPLACE_SETTLEMENT_PROFILE` | server-only operator configuration |
| `DATABASE_URL` | runtime-only secret, automatically injected after database readiness |
| `HIVE_MARKETPLACE_HMAC_KEYS` | node secret, delivered only by vault/systemd secret handling |

## Routing and failures

The gateway chooses from the same healthy Marketplace-eligible node set used
for advertisements. A single-node Genesis deployment handles the request
locally without dialing itself. Reads can fail over when a mesh request fails
before a response; writes never silently replay to another node because a
response loss cannot prove that the first node did not consume the request.
Marketplace's existing `Idempotency-Key` is the write retry mechanism.

The mesh leg is authenticated by Iroh's Ed25519 endpoint identity and the
configured Hive peer-trust policy. Marketplace HMAC remains the independent
application authorization mechanism. Neither identity nor topology is
disclosed to the Marketplace application.

## Post-quantum posture

This gateway does **not** establish a global post-quantum claim. Current Hive
runtime posture reports mesh key exchange as classical X25519; `post_quantum`
must remain unavailable until negotiated session telemetry proves a live
Iroh connection selected a hybrid ML-KEM group. Even then that evidence is
session-scoped to the Iroh mesh leg only. Public HTTPS, database TLS, relay
TLS, raw streams, tickets, and response payloads outside that session are not
covered by the claim.
