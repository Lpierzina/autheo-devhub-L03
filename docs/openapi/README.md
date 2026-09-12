# OpenAPI documentation: private boundaries

These files describe code currently implemented in `crates/hive-cloud`; they
do not create listeners, reverse-proxy entries, CORS permissions, Swagger UI,
or any public access.

| File | Surface | Intended exposure |
| --- | --- | --- |
| `devhub-marketplace-private.yaml` | Marketplace → DevHub | Private service-to-service only, HMAC authenticated |
| `hive-admin-internal.yaml` | Hive Admin/control plane | Loopback or private management network only |

The implementation currently has five Marketplace routes:

* `GET /v1/marketplace/l0/deployments`
* `GET /v1/marketplace/settlement-config`
* `POST /v1/marketplace/payment-intents`
* `POST /v1/marketplace/payments/verify`
* `POST /v1/marketplace/l0/allocations`

There is no Marketplace allocation lookup/status route, settlement-profile
write route, usage-record route, or Marketplace callback route in the current
router. Do not infer one from prior architecture documents.

Do not add Swagger UI for Hive Admin. Static files are intentionally the only
documentation delivery mechanism in this change.

## Deployment topology

For the intended HP DL380 deployment, publish only HTTPS `443` at the
Internet perimeter. Keep Marketplace (`3000`), DevHub (`3001`), Hive UI
(`3002`), and Hive Admin (`8786`) loopback/private as appropriate to the
local service topology. In particular, never open `8786` publicly.

Mesh relay/gateway ports, where intentionally public, are protocol/data-plane
ports and are not an authorization reason to expose the Admin HTTP port.
