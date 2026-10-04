# Marketplace consumer handoff

This is the implementation handoff for the separate Marketplace repository.
It describes what Marketplace can consume from DevHub **now**, and what must
remain unavailable until DevHub implements the operational contract.

Marketplace owns commercial listings, quotes, orders, payment verification,
and commercial history. DevHub owns scheduling and all operational evidence.
Marketplace must never represent its own records as DevHub capacity,
provisioning, execution, completion, usage, or attestation.

## Available now

DevHub currently exposes this private, service-to-service API:

| Method | Route | Marketplace action |
| --- | --- | --- |
| `GET` | `/v1/marketplace/l0/deployments` | Discover short-lived eligible deployment advertisements. |
| `POST` | `/v1/marketplace/payment-intents` | Create an immutable THEO settlement intent for a selected advertisement. |
| `POST` | `/v1/marketplace/payments/verify` | Ask DevHub to verify the on-chain receipt against that immutable intent. |
| `POST` | `/v1/marketplace/l0/allocations` | Submit a **DevHub-verified** settlement for constrained scheduling admission. |
| `POST` | `/v1/marketplace/workload-intents/v2` | Strictly validate the future authoritative intent shape; currently fails closed and grants no placement. |

The machine-readable schema is
[`docs/openapi/devhub-marketplace-private.yaml`](../openapi/devhub-marketplace-private.yaml).
Do not infer support for an operation from a client helper, route-like
documentation, or an undocumented response field.

### Request authentication

Every request carries:

```text
X-Marketplace-Key-Id
X-Marketplace-Timestamp
X-Marketplace-Nonce
X-Marketplace-Content-SHA256
X-Marketplace-Signature
```

The exact signing input is UTF-8:

```text
METHOD\nPATH\nTIMESTAMP\nNONCE\nSHA256_HEX(BODY)
```

Use HMAC-SHA256. `PATH` is the canonical path only, without host or query
string. `SHA256_HEX(BODY)` is lowercase hexadecimal over the exact bytes sent;
for `GET`, it is the digest of an empty body. Timestamps are Unix milliseconds.
DevHub accepts at most five minutes of clock skew and rejects a reused
`key-id:nonce` for ten minutes.

`Idempotency-Key` is required for `POST /payment-intents` and
`POST /l0/allocations`. On an ambiguous network result, retry the same request
to the same private integration path with the same idempotency key; do not
invent a new order, payment intent, or allocation.

Marketplace configuration must map its
`DEVHUB_MARKETPLACE_KEY_ID` and `DEVHUB_MARKETPLACE_SIGNING_SECRET` to an
operator-configured DevHub entry in `HIVE_MARKETPLACE_HMAC_KEYS`. This secret
is service-only: it must never reach a browser, order record, client log, or
analytics payload.

### Commercial flow

1. Fetch an advertisement. It expires after 60 seconds and is not a capacity
   reservation.
2. Create a payment intent from that exact advertisement and preserve the
   returned immutable settlement snapshot.
3. Have Marketplace's existing THEO flow submit the transaction.
4. Call payment verification until DevHub reports `verified`.
5. Submit the allocation with the verified payment intent, Marketplace's
   opaque tenant ID, and the requested resource shape.
6. Store the returned opaque `allocation_id` and status as a correlation
   record only.

Marketplace must not submit repository URLs, image references, build
credentials, workload credentials, node choices, callback URLs, or browser
claims as part of this flow. `tenant_id` is an opaque correlation value, not a
credential.

### Buyer-controlled DevHub flow

Project and immutable-release creation remain DevHub user-authorized
operations:

| Method | Route | Authority |
| --- | --- | --- |
| `POST` | `/v1/projects/{project}/marketplace-releases` | Authenticated DevHub project owner |

Marketplace may direct the buyer to DevHub, but must not use a browser-supplied
project or release ID as proof of ownership. The documented
`/v1/projects/{project}/marketplace-workloads` route is not registered and
must not be called. Current server-to-server workload handling validates
project tenant, release publication, and revision through the Marketplace
boundary only.
Marketplace receives no deployment credential, database credential, private
node address, mesh identity, or runtime routing material.

## Do not treat as operational evidence

The current allocation API returns `status: "submitted"` after receipt
verification and a point-in-time constrained scheduling check. It does **not**
mean any of the following:

| Marketplace must not claim | Why |
| --- | --- |
| Capacity is held | DevHub has no capacity-hold API or durable reservation ledger. |
| Workload handoff exists | The allocation and DevHub workload attachment are separate current flows. |
| Provisioning started or succeeded | No allocation lifecycle producer exists. |
| A workload is active or completed | No allocation-bound execution evidence exists. |
| Usage was measured | No allocation-bound usage record API exists. |
| A provider is owed payment | Commercial settlement remains Marketplace's responsibility. |

Marketplace should display the result as `operational_status: integration_unavailable`
or an equivalent non-operational pending state. Do not advance an order to
active, completed, usage-billable, or payout-eligible from an allocation
response, elapsed time, or Marketplace-local deployment record.

## Blocked until DevHub implements the operational contract

The following documented contract is a target, not a live API:
[`devhub-l0-operational-contract.md`](devhub-l0-operational-contract.md).

Marketplace must not call or emulate these capabilities yet:

| Capability | Required DevHub authority before Marketplace enables it |
| --- | --- |
| Capacity hold, release, or renewal | Transactional, durable capacity ledger with expiry and no oversell. |
| Paid allocation commitment | Hold consumption, tenant/order/resource binding, and digest-bound idempotency. |
| Allocation status and reconciliation | Authoritative allocation reads and cursor-based recovery feed. |
| Service-created workload handoff | Server-side project/release ownership validation and durable handoff record. |
| Lifecycle callbacks | Configured trusted callback destination, separate outbound signing keys, durable outbox, retries, event IDs, and allocation sequences. |
| Execution reads | DevHub-issued execution IDs and observed lifecycle evidence. |
| Usage reads | DevHub-measured, append-only, allocation-bound usage records. |
| Allocation cancellation | A real lifecycle with defined cancellation effects and terminal evidence. |

When these capabilities are delivered, Marketplace must reconcile through
DevHub allocation status after a timeout, callback gap, duplicate callback, or
out-of-order event. A callback is a notification, never authority to advance
an arbitrary Marketplace order.

## Marketplace endpoints DevHub already consumes

These endpoints belong in the Marketplace repository. They are not implemented
by this repository:

| Method | Route | Used for |
| --- | --- | --- |
| `GET` | `/v1/marketplace/orders/{order_id}/placement-policy` | DevHub validates allowed placement for a Marketplace-initiated deployment. |
| `GET` | `/v1/marketplace/projects/{project_id}/resources` | DevHub UI reads Marketplace-owned resource state. |
| `POST` | `/v1/marketplace/orders/{order_id}/project-attachments` | Marketplace records a DevHub project attachment. |

These Marketplace endpoints must authenticate DevHub's service identity and
must not accept a browser tenant assertion as authority. Their exact current
consumer expectations are documented in
[`docs/marketplace-l0-routing.md`](../marketplace-l0-routing.md) and
[`docs/marketplace-project-resources.md`](../marketplace-project-resources.md).

## Callback receiver: future Marketplace work

DevHub's durable outbox sends callbacks only when its operator has configured
`HIVE_MARKETPLACE_EVENT_URL`, `HIVE_MARKETPLACE_EVENT_KEY_ID`, and a matching
directional key in `HIVE_DEVHUB_EVENT_HMAC_KEYS`. Marketplace's receiver must
not treat a callback as operational authority until the missing reservation and
ordered lifecycle contract is implemented. The receiver route is Marketplace
owned:

```text
POST /v1/marketplace/internal/devhub/events
```

The receiver must verify `X-DevHub-Key-Id`, `X-DevHub-Timestamp`,
`X-DevHub-Nonce`, `X-DevHub-Content-SHA256`, and `X-DevHub-Signature` using:

```text
METHOD\nPATH\nTIMESTAMP\nNONCE\nKEY_ID\nSHA256_HEX(BODY)
```

It must use a callback key namespace distinct from inbound Marketplace request
keys; enforce a five-minute timestamp window; persist nonce and event-ID replay
facts; enforce allocation-scoped sequence rules once DevHub emits them; and
acknowledge only after its durable transaction completes.

Until that API exists, no Marketplace callback URL, callback secret, or
callback-processing success state should be stored as proof of an operational
transition.

## Integration acceptance checklist

Before Marketplace enables the current settlement path:

- [ ] HMAC signing uses the exact raw body and canonical path.
- [ ] The Marketplace key pair is provisioned out of band and never exposed to
      browsers.
- [ ] Payment verification is `verified` before allocation submission.
- [ ] Allocation retries preserve their idempotency key.
- [ ] UI language distinguishes submitted allocation from a running workload.
- [ ] No order state, payout, or usage calculation relies on the unavailable
      operational-contract routes.

Before Marketplace enables any future operational lifecycle:

- [ ] DevHub publishes the route and OpenAPI schema.
- [ ] DevHub provides a real capacity, lifecycle, execution, or usage source
      for the claimed state.
- [ ] Marketplace integration tests verify the actual DevHub endpoint rather
      than a local Marketplace record.
- [ ] Callback recovery uses DevHub status reconciliation, not callback
      delivery alone.
