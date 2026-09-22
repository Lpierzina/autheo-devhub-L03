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
HIVE_MARKETPLACE_GATEWAY_LISTEN=<this project's RFC1918 Podman bridge IP>:9443
HIVE_MARKETPLACE_GATEWAY_TLS_CERT=/etc/hive/marketplace-gateway.crt
HIVE_MARKETPLACE_GATEWAY_TLS_KEY=/etc/hive/marketplace-gateway.key
HIVE_MARKETPLACE_GATEWAY_CA_CERT=/etc/hive/marketplace-gateway-ca.crt
HIVE_MARKETPLACE_HMAC_KEYS=<key-id>:<secret>[,...]
```

The listener refuses wildcard, public, loopback, link-local, unspecified, and
IPv6 binds. It is separate from port 8786, the public edge, and
`api.<platform-domain>`. It requires a client certificate chained to the
configured private CA; TLS is transport identity only and never replaces the
Marketplace HMAC check on the receiving router.

Marketplace workload credentials are runtime secret files, never environment
values:

```text
/var/run/autheo/workload-client/ca.crt   mode 0444
/var/run/autheo/workload-client/tls.crt  mode 0444
/var/run/autheo/workload-client/tls.key  mode 0400
```

The directory is platform-owned and non-writable by the workload. The only
injected DevHub routing configuration is
`DEVHUB_PRIVATE_BACKEND_URL=https://devhub-marketplace.internal`; no public or
localhost fallback is supported.

The project runtime contract is stored only in the node's secret environment:

```text
HIVE_MARKETPLACE_RUNTIME_NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY=...
HIVE_MARKETPLACE_RUNTIME_CLERK_SECRET_KEY=...
HIVE_MARKETPLACE_RUNTIME_CLERK_JWT_ISSUER=...
HIVE_MARKETPLACE_RUNTIME_DEVHUB_MARKETPLACE_KEY_ID=...
HIVE_MARKETPLACE_RUNTIME_DEVHUB_MARKETPLACE_SIGNING_SECRET=...
```

Only `NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY` is build-visible. All other values,
including the HMAC secret, are runtime-only project secrets. Credentials must
be delivered by the runtime secret-file mechanism above, not through
`NODE_EXTRA_CA_CERTS` or any PEM-bearing environment variable.

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
