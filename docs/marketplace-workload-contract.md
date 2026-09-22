# Marketplace workload contract

Marketplace settlement, DevHub project ownership, and workload deployment are
separate authorities.

1. Marketplace calls only the four private HMAC routes through
   `https://devhub-marketplace.internal`. Allocation accepts settlement and
   resource facts only; it never accepts repository URLs, image references,
   Dockerfile/Compose settings, or any browser-supplied deployment request.
2. An authenticated DevHub caller creates a durable project release at
   `POST /v1/projects/{project}/marketplace-releases`. A release is immutable
   authority distinct from a deployment record and has a platform-issued
   `release_id`, `project_id`, revision, publication/revocation state,
   source/build identity, and workload capabilities.
3. An authenticated DevHub caller attaches an allocation through
   `POST /v1/projects/{project}/marketplace-workloads`. DevHub verifies exact
   project ownership and buyer tenant, then requires the release to exist, be
   published, not be revoked, and have the exact requested revision. The
   resulting workload snapshot persists the allocation, project, release,
   revision, buyer tenant, and client-certificate delivery request.

Client certificate delivery requires the release capability exactly:

```yaml
workload_client_certificate:
  mode: files-v1
  reload: true
```

Project identity is not a capability. In particular, a rollback to a release
without this declaration cannot inherit `files-v1` from a later release and
must fail the credential/readiness path.

The private gateway is mTLS transport only. The final receiving DevHub
Marketplace router still verifies the request's HMAC, timestamp, body digest,
durable nonce, and idempotency semantics. Iroh trust authorizes the internal
mesh leg only and is never Marketplace authorization.

`DATABASE_URL`, private keys, CA material, HMAC secrets, Iroh identities,
tickets, relays, peer addresses, node identity, and routing decisions are not
valid workload metadata and must never appear in release/workload records or
browser-visible responses.

Settlement stays `settlement_unavailable` until the selected
`autheo-testnet-v1` profile verifies all of:

- `HIVE_MARKETPLACE_TESTNET_THEO_TOKEN`
- `HIVE_MARKETPLACE_TESTNET_ATOMIC_SPLIT_CONTRACT`
- `HIVE_MARKETPLACE_TESTNET_FEE_RECIPIENT`
- `HIVE_MARKETPLACE_TESTNET_ATOMIC_SPLIT_AUDITED=1`
- `HIVE_MARKETPLACE_TESTNET_CONFIGURATION_REFERENCE`

No contract deployment or audit-completion claim is made by this integration.
