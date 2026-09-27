# OpenAPI documentation: private boundaries

These files describe code currently implemented in `crates/hive-cloud`; they
do not create listeners, reverse-proxy entries, CORS permissions, Swagger UI,
or any public access.

| File | Surface | Intended exposure |
| --- | --- | --- |
| `devhub-marketplace-private.yaml` | Marketplace → DevHub | Private service-to-service only, HMAC authenticated |
| `hive-admin-internal.yaml` | Hive Admin/control plane | Loopback or private management network only |

Marketplace calls DevHub through exactly four private service-to-service routes:

* `GET /v1/marketplace/l0/deployments`
* `POST /v1/marketplace/payment-intents`
* `POST /v1/marketplace/payments/verify`
* `POST /v1/marketplace/l0/allocations`

DevHub calls Marketplace only for
`GET /v1/marketplace/orders/{marketplace_order_id}/placement-policy`, from its
server-side route with a Clerk JWT minted from the
`autheo-marketplace-v1` template. That Marketplace endpoint is not a Hive
Admin route and is intentionally absent from this private DevHub OpenAPI
document.

The four routes in this OpenAPI file do not include capacity holds, allocation
status/read APIs, callbacks, project/release handoff, execution evidence, or
DevHub/Hive usage ingestion. `POST /usage-records` is buyer-authenticated and
is not a DevHub/Hive ingestion surface. The required, separately reviewed
reference for those capabilities is
[DevHub L0 operational contract](../integrations/devhub-l0-operational-contract.md).
That contract is not yet implemented by this OpenAPI surface.

Do not add Swagger UI for Hive Admin. Static files are intentionally the only
documentation delivery mechanism in this change.

## Deployment topology

For the intended HP DL380 deployment, publish only HTTPS `443` at the
Internet perimeter. Keep Marketplace (`3000`), DevHub (`3001`), Hive UI
(`3002`), and Hive Admin (`8786`) loopback/private as appropriate to the
local service topology. In particular, never open `8786` publicly.

Mesh relay/gateway ports, where intentionally public, are protocol/data-plane
ports and are not an authorization reason to expose the Admin HTTP port.
