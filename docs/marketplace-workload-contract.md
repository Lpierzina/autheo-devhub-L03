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

When the capability is available, the allocation-scoped, platform-owned
runtime credential mount contains only:

| path | mode |
| --- | --- |
| `/var/run/autheo/workload-client/ca.crt` | `0444` |
| `/var/run/autheo/workload-client/tls.crt` | `0444` |
| `/var/run/autheo/workload-client/tls.key` | `0400` |

The containing directory is not writable by the workload. Certificates,
private keys, database URLs, and source configuration never belong in
deployment records, build environments, build logs, Marketplace API responses,
or ordinary environment variables.

## Certificate lifecycle and readiness

The release attachment is the only issuance authority. Its credential selector
is derived from, and remains bound to, the original tuple
`{allocation_id, project_id, release_id}`. A project with more than one
certificate-bearing workload is refused rather than allowing a deployment to
guess or reuse another allocation's credential.

The platform keeps the authoritative files beneath the fixed root
`/var/lib/hive/marketplace-workload-certs`; the runtime can mount only the
platform-issued selector into the three fixed paths above. It cannot request a
host path, a mount destination, a socket, a device, a symlink, or a generic
bind mount. The root and each credential directory must be root-owned and
non-writable, and files must be root-owned regular files at the documented
modes.

The node renewer checks both trust-chain verification and remaining lifetime.
When a credential is close to expiry, it replaces `ca.crt`, `tls.crt`, and
`tls.key` atomically at their existing paths without changing the mounted
directory identity. If issue, renewal, ownership, mode, or chain validation
fails, the workload credential path refuses with a typed, secret-free
`marketplace_workload_certificate_unavailable` failure; it never downgrades to
non-mTLS transport.

Operators must provide the shared private Marketplace CA and gateway
certificate/key through Ansible vault only. The managed host paths are:

| server-only path | mode |
| --- | --- |
| `/etc/hive/marketplace-ca/ca.key` | `0400` |
| `/etc/hive/marketplace-ca/ca.crt` | `0444` |
| `/etc/hive/marketplace-ca/gateway.key` | `0400` |
| `/etc/hive/marketplace-ca/gateway.crt` | `0444` |

`HIVE_MARKETPLACE_HMAC_KEYS` is also an Ansible-vault/systemd-secret value.
Neither it nor any certificate/key value belongs in inventory defaults, source
control, browser configuration, release records, build environments, logs, or
API responses.

`DEVHUB_PRIVATE_BACKEND_URL=https://devhub-marketplace.internal` is the only
Marketplace routing value injected into a workload. There is no localhost,
public-hostname, raw-IP, ticket, relay, or mesh fallback.

The private gateway is mTLS transport only. The final receiving DevHub
Marketplace router still verifies the request's HMAC, timestamp, body digest,
durable nonce, and idempotency semantics. Iroh trust authorizes the internal
mesh leg only and is never Marketplace authorization.

`DATABASE_URL`, private keys, CA material, HMAC secrets, Iroh identities,
tickets, relays, peer addresses, node identity, and routing decisions are not
valid workload metadata and must never appear in release/workload records or
browser-visible responses.

## Managed Postgres migrations

Marketplace schema files run only through the BuildExecutor migration surface.
The migration target is read exclusively from the ready, live,
platform-managed Postgres record; it must be a canonical usable IPv4 address
and is passed to the host verifier only as `<ipv4>:5432`. It is never derived
from tenant input, DNS, URLs, repository configuration, release metadata, or
environment variables.

Ordinary BuildExecutor work remains `--network=none`. A host publishes the
separate migration capability only after Ansible has installed and verified a
root-owned verifier, lifecycle lock, dedicated DNS-disabled Podman bridge, and
matching live nftables policy. The policy allows solely new TCP/5432 traffic
from that bridge to the exact declared managed address and established/related
return traffic. IPv6, DNS, host/fleet/public/private broad egress, lateral
containers, DNAT, wildcard targets/ports, and default forwarding are denied.

Migration readiness requires the valid host capability, trusted verifier,
matching live network/firewall state, exact managed target, successful
attachment and migration execution, and terminal cleanup. A missing, malformed,
stale, or unverifiable capability fails closed with a topology-free migration
error; no alternate network mode exists. Containers, temporary volumes, and
attachments are managed by the existing cancellation-safe BuildExecutor
lifecycle and are removed after success, error, timeout, cancellation, or
partial startup. Operators must set the migration-only BuildExecutor inventory
variables and rerun the serialized role after changing a managed target; failed
provisioning revokes the nested capability while leaving ordinary offline builds
unchanged.

The target authorization is strictly job-scoped. Startup invokes the
root-owned verifier to insert only the exact target into its reviewed
`migration_target_ipv4` set. Every terminal path — successful migration,
SQL/session failure, timeout, request cancellation, dropped future, partial
startup, explicit destruction, and an unexpected `BuildSession` or
`CleanupGuard` drop — uses the same cleanup contract: stop/remove migration
containers, remove temporary migration volumes, prove no migration attachment
remains, clear the target, prove the set empty, recheck for surviving
attachments, then release the trusted lifecycle lock. The clear operation is
parameterless: it validates the trusted capability, verifier bytes, policy
identity, live network, and reviewed nft declaration before it may affect the
one declared migration set. It accepts no address, table, set, policy, or
tenant input.

If any removal, attachment proof, target clear, or empty-set proof fails, the
session is not reported as cleanly complete. The failure remains typed and
topology-free to callers; privileged host diagnostics retain the detail needed
for remediation. Operators must stop admitting Marketplace migrations on that
host, preserve the failed lifecycle state, inspect the root-owned verifier and
its declared capability/policy under the lifecycle lock, remove only the
identified managed migration containers and temporary volumes, run the
parameterless verifier cleanup operation, and require its empty-set and
attachment proofs to pass before restoring the capability. Do not manually
flush unrelated nftables state or broaden BuildExecutor networking. Ordinary
BuildExecutor jobs remain `--network=none` throughout.

## Immutable executable release binding

An execution-ready Marketplace release has one immutable binding:

```text
{ project_id, release_id, revision, artifact_id, artifact_digest, runtime_spec_digest }
```

`artifact_digest` is a lowercase `sha256:` content digest. Its only accepted
reference form is `devhub://runtime-artifacts/<digest-without-prefix>`.
`devhub://` is an authority reference, not a network URL: it names a sealed
runtime-artifact package verified by DevHub's runtime-artifact transfer
service. Mutable OCI tags, arbitrary image URLs, and Marketplace-provided
execution references are not artifact identity.

The release record retains the platform import/build transaction and semantic
tree digest as provenance plus the policy approval fact. Approval is checked
again when a workload is created. A project revision is inserted once: a
subsequent attempt to bind that same revision to a different artifact or
runtime-spec digest fails with `marketplace_release_revision_already_bound`.

The current runtime-artifact transfer service verifies and materializes a
sealed package for a specific deployment transaction. It is not yet a
fleet-wide reusable artifact catalog, so a valid binding does not itself claim
that every node can currently materialize that artifact. A future catalog must
preserve the same digest/reference contract and reject revoked or unapproved
artifacts before materialization.

## Minecraft runtime and storage capability boundary

The server-owned `minecraft` runtime spec is versioned and digest-bound to the
release. It carries the server implementation/version, artifact reference,
entrypoint and arguments, TCP ports, CPU/memory/storage requirements, a
required `world` persistent mount, bounded startup/shutdown settings, a
TCP-listen health contract, allowed environment names, and logical internal
secret references. Secret values are resolved only by DevHub.

The TCP health check proves that the configured game port is listening. It is
stronger than node health or process spawn but is not a Minecraft protocol
ping; no protocol-level probe exists in the current runtime.

Workload instances and volume bindings are durable and placement-independent,
but the only currently advertised storage backend is `node_local`:

| Capability | Current value |
| --- | --- |
| persistent on the attached node | supported |
| node-local snapshot | supported where the runtime provides it |
| portable restore | unsupported |
| replication | unsupported |
| multi-node attachment | unsupported |

Only `continuity_policy.mode = none` is accepted on this backend.
`warm_standby` fails closed with `portable_storage_unavailable`; automatic
failover fails with `automatic_failover_unsupported`. No request is silently
downgraded.

Before warm standby can be enabled, a DevHub-owned portable backend must
provide tenant-isolated, authenticated and idempotent create-volume,
attach/detach, immutable snapshot (digest/reference and explicit consistency
semantics), restore-on-another-eligible-node, delete/release, encryption, and
durability guarantees. Cross-node restore, shared volumes, replication,
fencing, automatic failover, traffic cutover, and zero-downtime migration are
therefore explicitly unsupported in this phase.

Settlement stays `settlement_unavailable` until the selected
`autheo-testnet-v1` profile verifies all of:

- `HIVE_MARKETPLACE_TESTNET_THEO_TOKEN`
- `HIVE_MARKETPLACE_TESTNET_ATOMIC_SPLIT_CONTRACT`
- `HIVE_MARKETPLACE_TESTNET_FEE_RECIPIENT`
- `HIVE_MARKETPLACE_TESTNET_ATOMIC_SPLIT_AUDITED=1`
- `HIVE_MARKETPLACE_TESTNET_CONFIGURATION_REFERENCE`

No contract deployment or audit-completion claim is made by this integration.

## Migration networking status

The existing BuildExecutor default is still `--network=none`. Database
migrations must not be enabled until a separately audited, root-owned
`migration_network` capability, exact-target verifier, and nftables policy are
installed together. The policy must permit only the operator-declared managed
Postgres IPv4 target on TCP 5432 and reject DNS, public internet, host,
fleet/private-subnet, wildcard, and alternate-port traffic. Missing capability,
verifier, lock, nftables state, or target proof must fail closed. This is an
operator blocker, not evidence of settlement or migration readiness.
