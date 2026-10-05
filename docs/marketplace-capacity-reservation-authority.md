# Marketplace capacity reservation authority (Phase 4G)

## Decision

DevHub does not currently have a proven distributed primitive that can atomically
read remaining provider/node capacity, consume it, create an idempotent
reservation, and durably fence stale writers. Phase 4G therefore selects
Option D: fail closed.

`POST /v1/marketplace/workload-intents/v2` remains disabled. After contract
validation it returns HTTP 503
`capacity_reservation_authority_unavailable`; it does not reserve, schedule,
attach, launch, or emit an accepted lifecycle event. This is intentionally a
different outcome from commercial-authorization unavailability: authorization
credentials cannot make capacity accounting safe.

`crates/hive-cloud/src/marketplace_reservations.rs` defines the durable
reservation record and the only acceptable authority interface. It has no
implementation yet. In particular, it does not provide an in-process mutex
that could be mistaken for fleet-wide correctness.

## Consistency inspection

| Primitive | Observed implementation | Classification | Suitable for capacity authority |
| --- | --- | --- | --- |
| `persist.rs` | Per-node JSON snapshot, temp-file fsync and rename; its coalescing writer serializes one process only | Durable local write; no distributed CAS or transaction | No |
| `MarketplaceSecurityStore` and `AllocationStore` | `parking_lot::RwLock` process-local maps; snapshot copied by `store_sync` | Process-local consistency only | No |
| `store_sync.rs` | Leader snapshot pull/adopt; comments explicitly describe wholesale replication and ambiguity during leader changes | Eventual replication; no conditional mutation | No |
| GuardianDB KV (`guardian.rs`) | `KeyValueStore::get/put/delete`, local index, asynchronous Iroh Docs sync | Local-first, eventually consistent CRDT; no CAS | No |
| GuardianDB relational (`relational.rs`) | `BEGIN; ...; COMMIT;` provides atomic overlay application in a local SQL session | Local serializable/atomic transaction only; cross-node replication is CRDT convergence | No |
| Control-plane owner chain (`cluster.rs`) | Observer-local health-based owner choice with a gossiped max-merged epoch | Single-writer routing aid; not quorum-confirmed and no durable fencing at storage | No |
| Container leases (`lease.rs`) | Process-local map, wall-clock expiry, gossip highest-epoch merge | Consensus-free eventual convergence; no durable cross-node CAS | No |
| Marketplace lifecycle outbox | Durable local snapshot and replicated release snapshot | Durable retry queue, not task ownership or reservation state machine | No |
| `openraft` dependency | Optional Cargo dependency only; no use outside the cluster status string | Consensus-backed state machine: not implemented/configured | No |

GuardianDB's own `KeyValueStore` documentation states that operations maintain
eventual consistency. Its SQL transaction implementation is useful for
all-or-nothing local rows but does not establish a single global serial order:
each node writes a local replica and iroh-docs CRDT replication converges later.
Neither API exposes a compare-and-set or quorum-confirmed commit. GuardianDB is
therefore not selected merely because it is replicated.

## Required future authority

The schema in `marketplace_reservations.rs` uses the capacity dimensions currently
authoritative in v2 intents: vCPU, memory MiB, storage GiB, and one execution
slot. It intentionally does not invent GPU, ports, or network capacity.

State transitions required at the authority:

```text
pending -> reserved -> committed
pending -> failed
reserved -> released | expired | revoked
```

`released`, `expired`, `revoked`, and `failed` are terminal. A future authority
must atomically persist the reservation record, its idempotency mapping, and the
capacity accounting in one linearizable operation.

The idempotency index must bind the idempotency key to the request digest,
order/allocation, tenant, placement scope, and capacity vector. Same key plus
same digest returns the original receipt; same key plus a different digest
returns `idempotency_conflict`.

TTL must be configured by policy, not invented in this layer. Expiry, commit,
release, and revocation must be conditional transitions at the same authority,
so expiry versus commit has one winner and capacity is returned at most once.
The record includes an authority term specifically to require a durable fencing
token; the current control-plane epoch is observer-local and is not sufficient.

Preferred Network reservations must bind the network and provider-membership
revisions. The authority must revalidate commercial authorization, provider
eligibility, and those network revisions before both reservation and launch.
Unavailability, stale evidence, or revocation must deny or revoke; they must
not silently fall back to standard placement or move a stateful world.

## Required enablement gate

Before v2 placement can be enabled, DevHub needs an existing or separately
introduced authority with all of the following proven properties:

1. Quorum-confirmed durable compare-and-set or serializable transaction for the
   reservation plus capacity aggregate.
2. A durable, monotonically increasing fencing term enforced by every mutation.
3. Idempotency records in the same authority and transaction.
4. Serialized TTL/recovery processing and restart replay.
5. Partition behavior that rejects mutations when quorum/authority is absent.
6. Production Marketplace authorization, provider/network evidence, and reverse
   S2S credentials configured and revalidated at reservation and pre-launch.

Only then should the scheduler enumerate DevHub-owned candidates, attempt the
best candidate atomically, optionally try another eligible candidate after a
capacity conflict, persist the placement, and publish opaque lifecycle events.
Marketplace receipts/callbacks may include allocation ID, reservation ID, status,
expiry, and safe reason codes, but not provider/node topology.

## Verification status

No new reservation concurrency tests were added because no distributed
reservation authority exists to test. A test over a process-local mutex or a
single-node GuardianDB session would demonstrate a guarantee the production
multi-node implementation does not have. The existing repository policy also
prohibits adding synthetic test suites.

The executable check for this phase is that the v2 endpoint still fails closed
with the authority-specific reason. It is not a claim that concurrent final-unit
reservations are safe in production; no reservation can succeed while the
authority is unavailable.
