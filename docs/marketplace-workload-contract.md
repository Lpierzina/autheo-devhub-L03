# Marketplace workload handoff

Marketplace selects capacity. It never supplies a repository URL, build
configuration, image reference, runtime secret, mesh address, certificate, or
client connectivity data to DevHub.

## DevHub release authority

`devhub_project_release` is a DevHub-owned immutable record:

```json
{
  "release_id": "rel_...",
  "project_id": "prj_...",
  "revision": "sha256:...",
  "published": true,
  "revoked": false,
  "marketplace_capabilities": {
    "workload_client_certificate": { "mode": "files-v1", "reload": true }
  }
}
```

The authenticated DevHub attachment endpoint validates that the project belongs
to the authenticated tenant and that the selected published release belongs to
that exact project and revision. Marketplace receives neither this record nor
its resolved deployment configuration.

## Credential delivery

Client credentials are issued only when the persisted allocation attachment
requests `{"enabled":true,"mode":"files-v1"}` and the immutable release
declares the matching `files-v1` capability with `reload:true`. The runtime
mount is allocation-scoped and contains only:

| path | mode |
| --- | --- |
| `/var/run/autheo/workload-client/ca.crt` | `0444` |
| `/var/run/autheo/workload-client/tls.crt` | `0444` |
| `/var/run/autheo/workload-client/tls.key` | `0400` |

The containing directory is read-only to the workload. Certificates, private
keys, database URLs, and source configuration are never written to deployment
records, build environments, build logs, Marketplace API responses, or
ordinary environment variables.

## Operator configuration

```text
# Explicitly identifies the DevHub Marketplace workload project.
HIVE_MARKETPLACE_PROJECT_ID=prj_...

# Enables the reviewed v2 Buildah-in-runsc capability only after its deployment
# declaration and probe have succeeded. Absence is a typed failure for
# Dockerfile/Compose source deployments.
HIVE_BUILD_EXECUTOR_V2=1

# Private-only Marketplace gateway. This must be the Marketplace project's
# Podman bridge address; wildcard, loopback, public, and link-local addresses
# are rejected.
HIVE_MARKETPLACE_GATEWAY_BIND=10.x.y.z:8443
HIVE_MARKETPLACE_GATEWAY_SERVER_NAME=devhub-marketplace.internal

# Root-owned state directory for the private CA and rotated workload material.
# It must not be a project checkout, deployment root, or publicly served path.
HIVE_MARKETPLACE_IDENTITY_DIR=/var/lib/hive/marketplace-identity
```

Settlement remains unavailable unless the existing audited Testnet configuration
is complete. Enabling builder-v2, a workload release, or mTLS does not change
that independent fail-closed condition.
