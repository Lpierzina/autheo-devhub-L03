# Marketplace workload execution boundary

Marketplace submits only a signed commercial workload intent to
`POST /v1/marketplace/workloads`. The request is HMAC-authenticated by the
existing private Marketplace gateway and must contain exactly:

```json
{
  "workload_order_id": "order_...",
  "buyer_tenant_id": "tenant_...",
  "idempotency_key": "order-create-...",
  "project_id": "project_...",
  "release_id": "rel_...",
  "revision": "immutable-revision",
  "term": {"starts_at": "2026-10-01T00:00:00Z", "ends_at": "2026-11-01T00:00:00Z"},
  "capacity_requirements": {"vcpu": 4, "memory_mib": 8192, "storage_gib": 50},
  "continuity_policy": {
    "mode": "none",
    "recovery_point_objective_seconds": 300,
    "recovery_time_objective_seconds": 900
  }
}
```

Unknown request fields are rejected. In particular, Marketplace cannot provide
a provider, node, image/tag/URL, manifest, secret, secret-bearing environment
value, storage implementation, network placement, or backup placement.

DevHub resolves an executable binding only when an approved release has an
internally stored immutable artifact and a server-owned Minecraft runtime spec.
The binding is:

```text
{project_id, release_id, revision, artifact_digest, runtime_spec_digest}
```

The artifact record carries its SHA-256 digest, opaque DevHub storage
reference, workload type, provenance, approval state, creation time, and
security/policy validation results. Runtime specs are versioned and
server-owned; their model includes entrypoint, arguments, public port,
health-check, CPU/memory/storage limits, persistent-world schema, graceful
shutdown, startup timeout, allowed environment names, and internal secret
selectors. Secret values are never stored in the release or received from
Marketplace.

## Current safe behavior

The platform currently has no DevHub-wide immutable artifact catalog that can
resolve a Marketplace release to stored executable bytes. Existing
`source_identity` is source/build provenance only, not executable authority.
Consequently source-only releases return
`release_executable_artifact_unavailable`; DevHub does not create a workload
instance, schedule a node, or report a deployment as started.

When executable resolution becomes available, a successful request creates a
durable `workload_instance_id` independent of compute allocation. It records
the commercial order, tenant, exact release binding, artifact/runtime digests,
lifecycle state, primary allocation, persistent storage identity, and
continuity policy. The opaque receipt and retry-safe signed callback expose no
provider or node topology.

## Continuity and failover status

Only `continuity_policy.mode: "none"` is currently accepted. `warm_standby`
and `automatic_failover` return `workload_continuity_unsupported`.

DevHub has a node-local deployment-disk snapshot primitive, but no
DevHub-owned persistent Minecraft world volume, durable snapshot catalog,
checksum/sequence record, restore operation, cross-node replication, standby
reservation/readiness, fencing, or routing cutover. It therefore makes no
state-continuity, warm-standby, or failover claim.

Automatic promotion must remain unavailable until it enforces:

```text
failure detection -> confirmation -> primary fencing -> fencing confirmation
-> backup-state validation -> promotion -> traffic transition
-> new-primary confirmation
```
