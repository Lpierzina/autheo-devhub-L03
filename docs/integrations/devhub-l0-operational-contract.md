# DevHub L0 operational contract

**Status: required external contract; not implemented by the current private
Marketplace API.** Marketplace must treat this document as the integration
gate for capacity-backed orders. It must not infer a reservation, a running
workload, or billable usage from the four legacy settlement/allocation routes.
Those routes are documented in
[the current private API](../openapi/devhub-marketplace-private.yaml), and do
not yet provide the guarantees below.

This contract is deliberately between Marketplace and DevHub, not Marketplace
and an L0 node. DevHub remains the only party that selects nodes, owns
execution evidence, and exposes private infrastructure details.

## Terms and immutable binding

| Term | Authority | Meaning |
| --- | --- | --- |
| `tenant_id` | Marketplace, verified by DevHub | Buyer tenant for an order. |
| `order_id` | Marketplace | Immutable Marketplace order identity. |
| `capacity_hold_id` | DevHub | A temporary, quantified reservation. |
| `allocation_id` | DevHub | A committed entitlement made from one hold. |
| `release_id` | DevHub | An immutable, project-owned release authority. |
| `workload_id` | DevHub | The binding of one allocation to one release. |
| `execution_id` | DevHub | One observed execution generation of a workload. |

Every hold, allocation, workload, event, execution observation, and usage
record is bound to the exact tuple:

```text
{tenant_id, order_id, capacity_hold_id?, allocation_id?, project_id?,
 release_id?, workload_id?}
```

An identifier from one tuple must never be accepted for another tenant, order,
project, or release. Marketplace IDs are opaque strings; DevHub IDs are
opaque, platform-issued strings. Neither is a node ID, host name, credential,
or scheduling directive.

## Transport, request authentication, and idempotency

All Marketplace-to-DevHub operational routes are private service-to-service
HTTPS routes. They use the existing five-header HMAC scheme described in
[the capacity/allocation contract](devhub-l0-capacity-allocation-contract.md):

```text
METHOD\nPATH\nTIMESTAMP\nNONCE\nSHA256_HEX(BODY)
```

The timestamp window is five minutes; the `key-id:nonce` is durable and
single-use for ten minutes. `Idempotency-Key` is mandatory for every
state-changing request. It is scoped to the authenticated Marketplace client,
operation, and primary binding (`order_id` for holds, `capacity_hold_id` for
release, `allocation_id` for cancellation and workload actions).

DevHub stores the normalized request digest and complete response for the
idempotency window. A repeated key with the same digest returns the original
status and body. A repeated key with a different digest returns
`409 idempotency_key_reused`. Marketplace must reuse the same key when
recovering an unknown result, rather than retrying with a new key.

`404` means the caller is not authorized to learn whether the referenced
tenant/order/allocation exists. `409` is a known conflicting state; `410` is
an expired or terminal object; `422` is a syntactically valid request that
cannot be admitted; `503` is retryable only when the response explicitly says
`retryable: true`. Neither side retries a non-idempotent operation by changing
its key.

## 1. Capacity reservation and reconciliation

### Acquire a hold

```http
POST /v1/marketplace/l0/capacity-holds
Idempotency-Key: <opaque key>
```

```json
{
  "order_id": "order_01...",
  "tenant_id": "tenant_01...",
  "listing_id": "dep_01...",
  "quantity": {
    "instances": 2,
    "vcpu_per_instance": 4,
    "memory_mib_per_instance": 16384,
    "storage_gib_per_instance": 100,
    "gpu_per_instance": 0,
    "runtime": "container"
  },
  "requested_expires_at": "2026-09-27T17:19:00Z"
}
```

`listing_id` is a short-lived DevHub advertisement identity, not an authority
to run on a named node. DevHub validates current capacity atomically and
returns the accepted quantity; it never silently reduces it.

```json
{
  "capacity_hold_id": "hold_01...",
  "order_id": "order_01...",
  "tenant_id": "tenant_01...",
  "quantity": {"instances": 2, "vcpu_per_instance": 4,
    "memory_mib_per_instance": 16384, "storage_gib_per_instance": 100,
    "gpu_per_instance": 0, "runtime": "container"},
  "status": "active",
  "created_at": "2026-09-27T17:04:00Z",
  "expires_at": "2026-09-27T17:19:00Z",
  "reconciliation_version": 7
}
```

`requested_expires_at` is bounded by DevHub's advertised maximum; DevHub
returns the actual expiry. A hold is usable only while `status=active` and the
DevHub clock is before `expires_at`. Expiry is automatic and irreversible:
late acquire, commit, release, and renewal requests must not resurrect it.
The first implementation has no implicit renewal; a renewal is a separate
idempotent `POST .../renew` operation with the same binding and an explicit
new expiry.

### Release, cancel, and reconcile

```http
POST /v1/marketplace/l0/capacity-holds/{capacity_hold_id}/release
Idempotency-Key: <opaque key>
```

The body carries `order_id`, `tenant_id`, and a machine-readable
`reason` (`order_cancelled`, `payment_failed`, `checkout_abandoned`, or
`operator_cancelled`). Releasing an active hold is terminal
`released`; repeating that release is successful. Releasing an expired hold
returns its terminal `expired` representation. A hold already consumed by an
allocation cannot be released; cancellation must use the allocation route.

Marketplace reconciles after every timeout, restart, or callback gap:

```http
GET /v1/marketplace/l0/reconciliation?cursor=<opaque>&updated_since=<rfc3339>
```

DevHub returns only the caller's records, in stable `(updated_at, id)` order,
with an opaque cursor and a `snapshot_at`. Each item includes its binding,
state, `reconciliation_version`, and terminal reason, but no topology. A
cursor expires rather than returning a partial history; Marketplace must then
restart from a full `snapshot_at` scan. Marketplace releases locally
abandoned active holds immediately; DevHub's expiry reaper is the independent
backstop.

## 2. Allocation and workload lifecycle

### Commit and cancel

```http
POST /v1/marketplace/l0/allocations
Idempotency-Key: <opaque key>
```

The request references `capacity_hold_id`, `order_id`, and `tenant_id`. DevHub
atomically verifies the binding and unexpired active hold, consumes it once,
and creates an `allocation_id`. Payment/settlement verification remains a
separate prerequisite; a settlement reference cannot substitute for a hold.

```http
POST /v1/marketplace/l0/allocations/{allocation_id}/cancel
Idempotency-Key: <opaque key>
```

Cancellation contains the same tenant/order binding and a reason. It is
idempotent. It terminates pending work, revokes the workload handoff, and
prevents new execution. It does not rewrite historical execution or usage
evidence.

`GET /v1/marketplace/l0/allocations/{allocation_id}` is the authoritative
recovery/status API. It requires the same service authentication and returns
the immutable binding, hold/allocation/workload state, latest event sequence,
terminal outcome, and links to execution and usage evidence. Marketplace must
poll it after an unknown write outcome and periodically while an allocation is
nonterminal.

The lifecycle is monotonic:

```text
hold:       active -> consumed | released | expired
allocation: accepted -> handoff_pending -> provisioning -> active
                              |                 |             |
                              +-> cancelled     +-> failed     +-> completed
                                                               +-> cancelled
```

`failed`, `cancelled`, and `completed` are terminal. DevHub may add
informational substates but must not reuse a terminal allocation ID for a new
attempt. A replacement allocation is a new allocation with an explicit
`replaces_allocation_id` link.

### Events and callbacks

The status API is authoritative. Callbacks are at-least-once delivery hints,
not a replacement for it. Marketplace registers a private HTTPS callback URL
and a callback key reference during service provisioning, not in an order
request. Redirects, DNS rebinding targets, and browser-provided URLs are
invalid.

DevHub signs each callback's exact body using the same canonical HMAC shape
with `POST`, the registered callback path, timestamp, nonce, and body digest.
The callback also carries:

```json
{
  "event_id": "evt_01...",
  "event_type": "allocation.active",
  "occurred_at": "2026-09-27T17:09:01Z",
  "sequence": 12,
  "allocation_id": "alloc_01...",
  "order_id": "order_01...",
  "tenant_id": "tenant_01...",
  "workload_id": "wrk_01...",
  "state": "active"
}
```

`event_id` is globally unique and retained for the reconciliation period.
`sequence` is strictly increasing per `allocation_id`; it is not a global
ordering claim. Marketplace deduplicates `event_id`, records the highest
contiguous sequence per allocation, and fetches allocation status when it sees
a gap, an older sequence, an invalid signature, or a callback retry. It must
acknowledge only after durable deduplication. DevHub retries non-2xx callbacks
with the same `event_id`; it never changes event contents during retry.

## 3. Server-authorized project, release, and workload handoff

Marketplace owns the commercial order. DevHub owns the project, release,
workload definition, deployment credentials, and runtime authorization. A
browser is never allowed to hand Marketplace a repository URL, image tag,
project ID, release ID, callback secret, node choice, or workload credential
as proof of authority.

The handoff is created by an authenticated DevHub server after it has checked
project ownership. The server sends:

```http
POST /v1/marketplace/l0/allocations/{allocation_id}/workload-handoffs
Idempotency-Key: <opaque key>
```

```json
{
  "order_id": "order_01...",
  "tenant_id": "tenant_01...",
  "project_id": "prj_01...",
  "release_id": "rel_01...",
  "release_revision": "sha256:...",
  "workload_capabilities": ["workload_client_certificate:files-v1"]
}
```

The request is service-authenticated by DevHub and accepted only when its
allocation binding matches exactly and the release is published, unrevoked,
and owned by that tenant/project. Marketplace records the opaque
`workload_handoff_id`; it does not interpret the release or receive source,
build, database, certificate, or routing material.

The response returns `workload_handoff_id`, `workload_id`, `allocation_id`,
the immutable binding, and `handoff_status`. The same idempotency key and
payload return the same handoff. A different release/project/buyer under the
same allocation is a conflict. `GET
/v1/marketplace/l0/workload-handoffs/{workload_handoff_id}` and allocation
status provide recovery after a lost response. Cancellation or release
revocation moves the handoff to terminal `revoked`; a revoked handoff cannot
be reactivated.

DevHub, not Marketplace, delivers any workload credential through its
platform-owned runtime mechanism. Marketplace must hold only its own service
credentials and callback verification keys.

## 4. Authoritative execution and usage evidence

Marketplace must not mark an order active because allocation was accepted, a
callback arrived, or a workload record exists. DevHub is authoritative for
execution and measurement and exposes:

```http
GET /v1/marketplace/l0/allocations/{allocation_id}/executions
GET /v1/marketplace/l0/allocations/{allocation_id}/usage?cursor=<opaque>
```

Each execution observation includes a DevHub-issued `execution_id`,
`workload_id`, generation, observed state, observed-at time, start/end time
where applicable, and an immutable `evidence_id`. It is bound to the
allocation/order/tenant tuple. It may identify an approved runtime class, but
never a node address, peer identity, or credential. `active` is authoritative
only when a nonterminal observation says so; stale observations must include a
staleness reason rather than being silently treated as current.

Usage is append-only, interval based, and measured by DevHub:

```json
{
  "usage_record_id": "use_01...",
  "allocation_id": "alloc_01...",
  "execution_id": "exe_01...",
  "order_id": "order_01...",
  "tenant_id": "tenant_01...",
  "interval_start": "2026-09-27T17:10:00Z",
  "interval_end": "2026-09-27T17:15:00Z",
  "dimensions": {"instance_ms": "600000", "vcpu_ms": "2400000",
    "memory_mib_ms": "983040000", "gpu_ms": "0", "storage_gib_ms": "30000000"},
  "measurement_version": "l0-usage-v1",
  "evidence_id": "evd_01...",
  "final": false
}
```

All quantities are decimal integer strings; intervals for the same
`execution_id` may be corrected only by a new record that names
`supersedes_usage_record_id`. Marketplace never overwrites a record in place
or submits a client-measured value as DevHub usage. A final terminal
observation and final usage watermark are required before Marketplace closes
an order as complete.

## Implementation acceptance gate

Marketplace may consume this contract only after DevHub publishes the routes
and schemas above, retains idempotency/event/usage evidence for the agreed
reconciliation period, and demonstrates:

1. duplicate acquire, commit, release, cancellation, callback, and handoff
   requests return their original result without double-reserving or
   double-executing;
2. an expired/released hold cannot create an allocation;
3. tenant/order/project/release swaps are rejected on every lookup and write;
4. a callback gap or lost response is recoverable solely through the
   authenticated status/reconciliation APIs; and
5. an allocation cannot become Marketplace-active without DevHub execution
   evidence, and no usage total is accepted without its immutable evidence
   binding.

Until then, Marketplace should retain the current order as
`integration_unavailable`, release no presumed capacity, and create no
Marketplace-side workload from an allocation response.
