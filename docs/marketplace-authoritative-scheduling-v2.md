# Marketplace authoritative scheduling v2

## Status

This document records the DevHub Phase 4D boundary. It does not declare
end-to-end authoritative placement available. The v2 parser is exposed at:

```text
POST /v1/marketplace/workload-intents/v2
```

It verifies the existing Marketplace request HMAC and validates the strict
v2 request shape. It then returns
`marketplace_authorization_evidence_unavailable` without reserving capacity,
selecting a provider, selecting a node, or starting a workload. This is
intentional until Marketplace provides a trusted commercial-authorization and
network-membership authority, and DevHub has a transactional capacity
reservation substrate.

## Phase 4D authorization foundation

DevHub now has an Ed25519 verifier foundation in
`crates/hive-cloud/src/marketplace_authorization.rs`. Trust is explicitly
operator-configured using `HIVE_MARKETPLACE_AUTH_ISSUER` and
`HIVE_MARKETPLACE_ED25519_KEYS`. Key entries use:

```text
issuer|key-id|base64url-ed25519-public-key|active-rfc3339|expires-rfc3339-or-empty|active-or-revoked
```

Multiple active keys permit deliberate rotation overlap. Unknown issuers and
key IDs, inactive/expired/revoked keys, algorithms other than exactly
`Ed25519`, invalid base64url, duplicate JSON keys, and invalid signatures are
rejected. This trust source is separate from both
`HIVE_MARKETPLACE_HMAC_KEYS` (Marketplace request authentication) and
`HIVE_DEVHUB_EVENT_HMAC_KEYS` (DevHub callback authentication).

The verifier sorts every object recursively, preserves array order, and emits
compact UTF-8 JSON. It is intentionally **not claimed to implement RFC 8785**.
Marketplace has not supplied authoritative cross-language fixtures describing
its string escaping, Unicode handling, number serialization, or exact
signature-envelope field exclusion. The signed-envelope parser rejects
duplicate keys before ordinary serde deserialization, but the verifier remains
a foundation only until those fixtures are available.

DevHub has typed, disabled client operations for the confirmed private
Marketplace paths:

```text
POST /v1/marketplace/internal/commercial-authorizations/check
POST /v1/marketplace/internal/provider-eligibility/check
```

`HIVE_MARKETPLACE_SERVICE_URL` must be an HTTPS URL before their endpoint
addresses can be formed. No request is sent: Marketplace has not provided an
approved reverse-direction S2S authentication mechanism or either endpoint's
request/response schema. DevHub will not reuse the inbound HMAC secret,
invent a bearer/mTLS contract, or send guessed production bodies. A preferred
network check returns an explicit unsupported result; it never falls back to
standard placement.

Consequently no signature, commercial authorization, provider eligibility,
proposed THEO tier, policy, revocation, or network assertion activates
scheduling. Tier thresholds remain inactive. In particular, Tier 0's proposed
staking exemption cannot replace independent provider qualification.

## Compatibility matrix

| Contract area | Verified legacy DevHub behavior | Current DevHub behavior | Desired v2 intent | Required Marketplace agreement |
| --- | --- | --- | --- | --- |
| Request authentication | HMAC request middleware in `marketplace.rs`; method, path, timestamp, nonce, SHA-256 raw body | Multiple inbound keys from `HIVE_MARKETPLACE_HMAC_KEYS`; durable replay facts replicated through the existing control plane | Same canonical request HMAC, 32-byte hex nonce, stable idempotency key | Confirm TypeScript uses raw UTF-8 bytes, Unix milliseconds, and the exact canonical inputs |
| Legacy allocation | `POST /v1/marketplace/l0/allocations` accepts a paid, Marketplace-selected deployment | Re-checks the listed deployment and passes its one-node allowlist to the scheduler | Deprecated for new orders; no provider/node/deployment selection in v2 | Preserve only for outstanding legacy orders; define a sunset and a separate v2 sender |
| Current workload intent | `POST /v1/marketplace/workloads` accepts DevHub project/release references and no placement configuration | Creates an accepted durable workload only when its immutable executable binding exists; does not reserve or schedule | `marketplace-workload-v2` carries workload outcome, policy versions, placement mode, and authorization references | Replace this transitional request with v2 after integration tests |
| Workload identity | Project, release, revision and buyer tenant are checked in DevHub | Release is immutable and executable only with an approved artifact/runtime binding | Same identities, all in the signed body | Marketplace must bind the order and buyer tenant to the exact project/release authorization |
| Commercial policy | No provider-commercial authorization interface exists | DevHub cannot independently validate active policy, provider approval, compliance, tier, limits, stake evidence, or revocation | Versioned commercial authorization reference plus short-lived authorization evidence | Issuer, key rotation, authorization id, policy version, issue/expiry, revocation/freshness lookup and order/tenant binding |
| Preferred network | No verified buyer/provider network source exists in this repository | v2 validates a shape only and never falls back to standard placement | `placement.mode` is `standard` or `preferred_network` | Network id, buyer authorization, active state, provider opt-in and approval, immutable membership revision, issuer, expiry and live revocation check |
| Capacity | `schedule::place` is a candidate picker, not a reservation protocol | Marketplace stores are replicated wholesale snapshots; no cross-node compare-and-reserve transaction exists | Opaque atomic reservation receipt with expiry/release | A linearizable DevHub reservation owner/transaction model, or a single authoritative reservation service |
| Callback signing | Existing outbox is durable and retries delivery | Callback now uses `X-DevHub-*` headers and signs key id in its canonical string | Ordered per-order revisions and fuller opaque lifecycle projection | Marketplace callback parser and durable deduplication must accept the new directional headers |

Fields deprecated from new authorization: `provider_id`, `canonical_node_id`,
`deployment`, provider recipient, runtime/image references, executable URLs,
secrets, storage placement, and backup-provider selection. The v2 parser uses
`deny_unknown_fields`; those fields are rejected rather than ignored.

## Required v2 request shape

```json
{
  "contract_version": "marketplace-workload-v2",
  "workload_order_id": "opaque-order",
  "buyer_tenant_id": "opaque-tenant",
  "idempotency_key": "stable-operation-key",
  "project_id": "opaque-project",
  "release_id": "opaque-release",
  "revision": "immutable-revision",
  "commercial_authorization_ref": "opaque-ref",
  "commercial_term": {"starts_at": "RFC3339", "ends_at": "RFC3339"},
  "workload": {
    "template": "minecraft",
    "workload_class": "minecraft-community-small",
    "capacity_requirements": {"vcpu": 4, "memory_mib": 8192, "storage_gib": 50}
  },
  "policy": {
    "scheduling_policy_version": "opaque",
    "commercial_policy_version": "opaque"
  },
  "placement": {"mode": "standard"},
  "continuity_policy": {
    "mode": "none",
    "recovery_point_objective_seconds": 0,
    "recovery_time_objective_seconds": 0
  },
  "commercial_authorization": {
    "issuer": "marketplace",
    "authorization_id": "opaque",
    "policy_version": "opaque",
    "issued_at": "RFC3339",
    "expires_at": "RFC3339"
  }
}
```

For `preferred_network`, add `preferred_network_authorization` with
`network_id`, `membership_revision`, `issuer`, `authorization_id`, `issued_at`
and `expires_at`. This only proves that the request is shaped for a future
authority; it is not authorization evidence until the agreed issuer signature
and a current revocation/freshness check are implemented.

Tier numbers and proposed THEO thresholds are not request fields and are not
active DevHub policy. In a future verified policy, Tier 0 emits
`stake_not_required_for_tier` only when the active authorization says so;
Tiers 1-3 require authoritative current stake evidence.

## Candidate selection and reservations

When the missing authority and transaction prerequisites exist, DevHub must:

1. Resolve an immutable release and approved artifact before candidate ranking.
2. Filter on workload/runtime/storage capability, current commercial
   authorization, required stake proof, network membership, node ownership,
   health freshness, and remaining capacity.
3. Use only the preferred network for a preferred request, with no fallback.
4. Rank the remaining candidates with an agreed, versioned deterministic
   policy and persist an internal placement decision.
5. Atomically reserve capacity with workload/order/tenant/scope binding,
   expiry, release and revocation handling.

The existing replicated `RwLock` stores are neither a distributed lock nor a
transactional compare-and-reserve mechanism. They must not be used to claim
atomic cross-node reservations.

## Signature vectors

These deterministic vectors are fixtures for Marketplace TypeScript tests,
not evidence of cross-repository interoperability.

### Marketplace request to DevHub

```text
secret: marketplace-test-secret-32-bytes-value
key id: marketplace-2026-01
body (exact UTF-8): {"contract_version":"marketplace-workload-v2","workload_order_id":"order-vector-1"}
body sha256: e1da1ff6b8076d5b247fefc55dbf069662b7356ccd840c6ae4f87c9a0ec40b77
canonical:
POST
/v1/marketplace/workload-intents/v2
1760000000123
00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff
e1da1ff6b8076d5b247fefc55dbf069662b7356ccd840c6ae4f87c9a0ec40b77
signature hex: b2039a723393aff8749a0c93136fe03ff0aab91da92da95b5debc2dbbcf3ac63
```

### DevHub callback to Marketplace

```text
secret: devhub-test-secret-32-bytes-value
key id: devhub-2026-01
body (exact UTF-8): {"event_id":"evt_vector_1","event_version":1}
body sha256: 47b2b1ac27591b43d7ace91a74688738b46fd8c4d53a0898194fdcd752f2af00
canonical:
POST
/v1/marketplace/callbacks/lifecycle
1760000000456
ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100
devhub-2026-01
47b2b1ac27591b43d7ace91a74688738b46fd8c4d53a0898194fdcd752f2af00
signature hex: bce58b13cc3fca9ac96e55a3f0e9784d36308d1c89b880055fb946702ca10bb3
```

The callback headers are `X-DevHub-Key-Id`, `X-DevHub-Timestamp`,
`X-DevHub-Nonce`, `X-DevHub-Content-SHA256`, and `X-DevHub-Signature`.
