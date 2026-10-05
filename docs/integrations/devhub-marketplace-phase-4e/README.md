# Marketplace → DevHub Phase 4E interoperability package

Copy this directory unchanged into DevHub's test-data tree. It has no runtime
Marketplace dependency. Package version: `1`. Status: **test contract only**:
issuance, v2 allocation/scheduling, reservations, provider-tier policy,
staking, backups, and execution remain disabled. Marketplace's 500-bps fee is
unchanged.

## Protected Ed25519 envelope

The complete wire envelope has exactly `issuer`, `key_id`, `alg`, `signature`,
and `payload`. Reject duplicate JSON object keys from raw UTF-8 before normal
JSON parsing; reject unknown envelope keys, non-`Ed25519` algorithms, and a
signature that is not unpadded base64url for exactly 64 bytes.

Sign UTF-8 bytes of the recursive TypeScript key-sort / compact
`JSON.stringify` form of:

```json
{"alg":"Ed25519","issuer":"approved-marketplace-issuer","key_id":"approved-key","payload":{}}
```

The signature field is excluded. Arrays retain order, keys use JavaScript
UTF-16 code-unit sort, strings and finite numbers use JavaScript
`JSON.stringify`; this is deliberately not RFC 8785. The issuer/key/algorithm
and payload are all protected. Keep test keys separate from production keys.
Production trust accepts only an active, unrevoked key inside its
activation/expiry interval; rotation overlaps for document lifetime plus retry
window. A valid signature never overrides a newer Marketplace revocation.

## Reverse DevHub → Marketplace HMAC

DevHub identifies as `devhub`. Configure the distinct, inbound credential as
`DEVHUB_COMMERCIAL_AUTHORIZATION_KEY_ID`,
`DEVHUB_COMMERCIAL_AUTHORIZATION_SIGNING_SECRET` (at least 32 characters),
and `DEVHUB_COMMERCIAL_AUTHORIZATION_SCOPES`. It must never reuse
`DEVHUB_MARKETPLACE_SIGNING_SECRET`, which is Marketplace → DevHub.

For both `POST` endpoints, send raw UTF-8 JSON and:

| Header | Exact value |
|---|---|
| `x-devhub-timestamp` | 13-digit Unix epoch milliseconds |
| `x-devhub-nonce` | 16–256 base64url characters, new for every retry |
| `x-devhub-key-id` | configured key ID |
| `x-devhub-content-sha256` | lowercase hex SHA-256 of raw body bytes |
| `x-devhub-signature` | lowercase hex HMAC-SHA256 |

Canonical signing string is exactly
`POST + "\n" + path-without-query + "\n" + timestamp + "\n" + nonce + "\n" + key-id + "\n" + sha256(raw-body)`.
Methods are uppercase. Query strings are unsupported (the exact configured
path has no query); reject them rather than normalize. The tolerance is 90,000
ms. Replay scope is `key-id:path`; durable storage consumes a nonce atomically
until `timestamp + 90,000ms`. Retries use a new nonce but retain the same
semantic request/idempotency at DevHub. HMAC rotation requires accepting the
old and new scoped key IDs during a bounded overlap.

`authorization:check` authorizes only
`/v1/marketplace/internal/commercial-authorizations/check`;
`provider:check` authorizes only
`/v1/marketplace/internal/provider-eligibility/check`. Bad signature, key,
scope, timestamp, nonce, rate-limit, replay, or backing dependency returns
`503 {"code":"unavailable"}`. Callers must treat that outcome and all network
errors as unavailable, never allowed. TLS is required.

## Check APIs

Schemas are in `schemas/`. Requests bind authorization, order, buyer tenant,
workload class, policy ID/version, and expected authorization revision.
Provider checks add a DevHub-selected `candidate_provider_id`; Marketplace
does not choose or return a provider/node. An active result is allowed only
for the commercial endpoint; `eligible` is allowed only for provider checks.
Every other result is denied except `unavailable`, which is operationally
unavailable and must also fail closed. `revoked` is a valid authoritative
decision; `unavailable` means an authoritative record/dependency could not be
read. Current disabled adapters return unavailable and never fabricate allow.

Tier 0 needs no stake only under an approved active policy and all other
qualifications. Proposed thresholds are Tier 0 `0`, Tier 1 `1000`, Tier 2
`100000`, Tier 3 `500000` THEO; no threshold is active here. Tier 1–3 requires
authoritative staking evidence, never legacy bearer-token observations.

## Preferred Provider Network

Each candidate check for reservation, retry/reassignment, and pre-launch must
bind `network_id`, buyer tenant, workload order, policy ID/version, network
authorization revision, and provider membership revision. Marketplace must
confirm active network, buyer ownership or approved membership, candidate
provider approval and opt-in, current terms/compliance/operational approval,
expiry and revocation. Unsupported constrained placement fails closed—there is
no standard-placement fallback.

Migration `035_commercial_authorization_evidence_foundation.sql` adds
`authorization_revision` to networks and buyer/provider memberships and
increments it on relevant updates. It is a per-row counter, not a deployed
durable aggregate membership digest. A production forward-only design still
needs a monotonic network-membership revision/digest record updated
transactionally for insert/delete/revoke as well as updates, plus a repository
that reads it and current revocation state. A signed snapshot is never enough.

## Current persistence blockers

`MARKETPLACE_TEST_DATABASE_URL` is not configured in this environment. Schema
migration 035 exists but has not been verified here. Missing deployed,
implemented components are: issuer/key repository and distribution, signed
authorization repository, atomic replay-nonce store, audit sink, rate limiter,
and network membership revision/revocation repository. No production database
or mock persistence was used.

## Integration instructions

1. Copy this directory; use `fixtures/ed25519-envelope-v1.json` and
   `fixtures/reverse-hmac-v1.json` unchanged.
2. Verify all positive vectors against their canonical JSON/hex and raw public
   keys; verify every negative raw envelope fails before authorization.
3. Implement raw duplicate-key detection before deserialization and HMAC
   request construction exactly as above.
4. Validate request JSON against the schemas, call both checks at candidate
   selection and again before launch, and fail closed on all unavailable,
   stale, revoked, or denied results.
