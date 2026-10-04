# DevHub L0 capacity-allocation contract

Marketplace calls DevHub through the four currently implemented
`/v1/marketplace/*` endpoints using
an HMAC key configured by the operator. DevHub treats Marketplace as a
server-to-server client, never as a mesh peer or a source of provider
recipients, node bindings, settlement configuration, or buyer transactions.

## Required configuration

```text
HIVE_MARKETPLACE_HMAC_KEYS=<key-id>:<secret>[,<key-id>:<secret>...]
HIVE_MARKETPLACE_PROVIDER_RECIPIENTS=<provider-id>=<0x-address>[,...]
HIVE_MARKETPLACE_SETTLEMENT_PROFILE=autheo-testnet-v1
HIVE_MARKETPLACE_TESTNET_THEO_TOKEN=<0x-address>
HIVE_MARKETPLACE_TESTNET_ATOMIC_SPLIT_CONTRACT=<approved MarketplaceAtomicFeeSplit V2 address>
HIVE_MARKETPLACE_TESTNET_FEE_RECIPIENT=<0x-address>
HIVE_MARKETPLACE_TESTNET_ATOMIC_SPLIT_AUDITED=1
HIVE_MARKETPLACE_TESTNET_CONFIGURATION_REFERENCE=<approved V2 configuration reference>
HIVE_MARKETPLACE_EVENT_URL=<operator-provisioned https callback URL>
HIVE_MARKETPLACE_EVENT_KEY_ID=<DevHub callback key id>
HIVE_DEVHUB_EVENT_HMAC_KEYS=<key-id>:<secret>[,<key-id>:<secret>...]
```

`HIVE_MARKETPLACE_TESTNET_ATOMIC_SPLIT_AUDITED` is an explicit deployment
approval gate. DevHub fails closed unless it is exactly `1`, the V2
configuration reference is non-empty, the fixed RPC reports chain 785, THEO
has the expected token symbol and code, and the configured V2 deployment has
code. A contract address alone is never an approval. The former AtomicSplit
deployment is historical-only and cannot issue a new intent.

Base Mainnet and the separately reviewed Autheo Testnet configuration are
documented in [Marketplace settlement profiles](../marketplace-settlement-profiles.md).
The testnet profile is selected only by its explicit profile name; its
environment values cannot repoint or otherwise alter the Base Mainnet profile.

Each request signs the canonical UTF-8 string:

```text
METHOD\nPATH\nTIMESTAMP\nNONCE\nSHA256_HEX(BODY)
```

with `HMAC-SHA256`. It sends the key ID, timestamp, nonce, body digest, and
signature in `X-Marketplace-Key-Id`, `X-Marketplace-Timestamp`,
`X-Marketplace-Nonce`, `X-Marketplace-Content-SHA256`, and
`X-Marketplace-Signature`. Writes additionally require `Idempotency-Key`.
Nonces are durable and single-use; secrets, headers, and bodies must not be
logged.

For allocation-bound workload handoffs, `HIVE_MARKETPLACE_EVENT_URL`,
`HIVE_MARKETPLACE_EVENT_KEY_ID`, and `HIVE_DEVHUB_EVENT_HMAC_KEYS` are also
required. DevHub rejects the handoff when that signed delivery path is not
configured, rather than accepting work that Marketplace would need to advance
by polling. The callback URL is operator configuration, never an order or
browser field. Callback signing is directional: DevHub sends
`X-DevHub-Key-Id`, `X-DevHub-Timestamp`, `X-DevHub-Nonce`,
`X-DevHub-Content-SHA256`, and `X-DevHub-Signature` over:

```text
POST\nPATH\nTIMESTAMP\nNONCE\nKEY_ID\nSHA256_HEX(BODY)
```

## Settlement and allocation

Discovery records are short-lived (60 seconds), contain only the public safe
schema, and derive a provider recipient solely from the operator-owned
provider registry. Payment intents snapshot the selected deployment binding,
buyer address, complete selected settlement configuration, V2 settlement key,
and exact split:
`fee = floor(gross / 20)` and `provider = gross - fee`. Amounts remain decimal
atomic integers through the full EVM `uint256` range.

DevHub does not sign, construct, relay, or broadcast a buyer transaction.
Verification independently reads the snapshot's chain receipt and accepts only
a successful receipt with sufficient confirmations and exactly one matching
`Settlement` log from the snapshot contract. The V2 event's settlement key is
independently derived with Solidity-compatible `abi.encode` and every
immutable intent field plus fee arithmetic is matched before allocation can be
submitted. The settlement key, transaction hash, and log index are retained as
the verification identity; order reference alone is never a replay key.

Immediately before scheduling, DevHub derives the listing anew from its live
registry and rejects expired, revoked/unhealthy, capacity-exhausted, or
provider/node-mismatched placements. The scheduler receives a one-node
allowlist; it never substitutes another node.

## Hardware inventory projection

`GET /v1/marketplace/l0/deployments` may include a compatibility-additive
`hardware` object alongside the legacy `capabilities` object. `capabilities`
remains the compatibility contract (`vcpu`, `ram_mib`, and `storage_gib`);
Marketplace must not change its interpretation or units. `hardware` is the
forward-looking structured inventory representation.

Every advertisement body binds its `canonical_node_id` (the DevHub registry
identity) and `provider_id` (from the operator-owned provider registry).
Marketplace must verify both values against the listing it is processing.
DevHub only emits advertisements for the provider/node relationship it derives
itself. A URL or requested identifier is never evidence of that relationship.

The response is trustworthy through the private mTLS gateway plus the
five-header HMAC authorization described above. DevHub does not add an
independent response signature. Inventory comes from the DevHub-controlled
node process and authenticated Hive registry, not Marketplace/provider input.
The listing's `issued_at` and `expires_at` are UTC RFC3339 timestamps; its
60-second expiry is a hard sellability bound. Marketplace must treat an
expired record as unavailable and must use `health` and `availability`
separately from hardware facts.

The currently producible fields are:

* `hardware.cpu.model`, `physical_cores`, `threads`, and `architecture` when
  the local trusted probe can observe them. Physical cores are omitted rather
  than inferred from threads/vCPU.
* `hardware.memory.total_mib`, when within DevHub's accepted bound.
* `hardware.gpus[]` only when the local NVIDIA device probe has an unambiguous
  model and per-device VRAM. Devices are grouped only by identical model and
  per-device VRAM; aggregate legacy GPU fields are never expanded into
  fabricated devices.

The following desired fields are not currently authoritative and are omitted:
memory ECC; typed storage devices, capacity, and usable capacity; minimum
egress bandwidth; and non-NVIDIA GPU device inventory. `storage_gib` remains
the legacy largest-volume capacity field, not a typed storage inventory.

DevHub validates models as bounded printable strings, CPU thread/core counts,
memory, GPU counts, and VRAM against finite bounds before exposing them.
The projection has no IPs, hostnames, ports, routes, peer identities, device
serials, filesystem paths, internal labels, service URLs, agent payloads,
diagnostics, credentials, certificates, or keys.
