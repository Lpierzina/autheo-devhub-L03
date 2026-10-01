//! DevHub-owned Marketplace release authority.
//!
//! A Marketplace release is deliberately not a deployment: one immutable
//! release can be deployed more than once, while Marketplace allocations bind
//! to the release identity captured below.  This store contains no PEM, HMAC,
//! source checkout, image tag, or mesh-routing material.

use std::{
    collections::BTreeMap,
    path::{Path as StdPath, PathBuf},
    sync::Arc,
};

use axum::{
    Router,
    extract::{Path, State},
    http::HeaderMap,
    response::Json,
    routing::post,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::process::Command;
use uuid::Uuid;

use crate::state::CloudState;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MarketplaceReleaseSnapshot {
    #[serde(default)]
    pub releases: Vec<ProjectRelease>,
    #[serde(default)]
    pub workloads: Vec<MarketplaceWorkload>,
    /// DevHub workload identities are separate from Marketplace's commercial
    /// order and from a future compute allocation. They survive reassignment.
    #[serde(default)]
    pub workload_instances: Vec<DevHubWorkloadInstance>,
    /// Successful migration applications are immutable facts, not build logs.
    /// They replicate with the release authority because both are required to
    /// decide whether a Marketplace workload may become ready.
    #[serde(default)]
    pub migration_facts: Vec<MarketplaceMigrationFact>,
    /// Exactly one managed Postgres identity is associated with each
    /// Marketplace project. Connection material intentionally never appears
    /// here; it remains inside the managed database store.
    #[serde(default)]
    pub managed_postgres: BTreeMap<String, String>,
    /// Durable, retryable Marketplace lifecycle outbox. The payload is a
    /// deliberately safe projection and never contains deployment details.
    #[serde(default)]
    pub lifecycle_events: Vec<MarketplaceLifecycleEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectRelease {
    pub release_id: String,
    pub project_id: String,
    pub revision: String,
    pub published: bool,
    pub revoked: bool,
    #[serde(default)]
    pub workload_client_certificate: Option<WorkloadClientCertificateCapability>,
    /// Immutable source/build identity, never an executable deployment request.
    #[serde(default)]
    pub source_identity: String,
    /// Server-owned execution binding. Existing source-only releases have no
    /// binding and are deliberately not executable Marketplace releases.
    #[serde(default)]
    pub execution: Option<ReleaseExecutionBinding>,
    pub created_ms: u64,
}

/// The immutable executable authority for one release revision. The current
/// platform has no DevHub-wide artifact catalog, so this type is intentionally
/// only populated by a future internal artifact-storage integration; callers
/// cannot submit it through the project or Marketplace APIs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReleaseExecutionBinding {
    pub artifact: ExecutableWorkloadArtifact,
    pub runtime_spec: MinecraftRuntimeSpec,
    pub runtime_spec_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableWorkloadArtifact {
    /// SHA-256 of immutable executable bytes, never an image tag or URL.
    pub digest: String,
    /// An opaque DevHub artifact-store key, never a path, URL, or registry
    /// location disclosed to Marketplace.
    pub storage_reference: String,
    pub workload_type: String,
    pub provenance: String,
    pub approval_state: String,
    pub created_ms: u64,
    pub security_validated: bool,
    pub policy_validated: bool,
}

/// Versioned, server-owned Minecraft launch contract. Secrets are represented
/// only by internal selectors; their values never enter a release record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MinecraftRuntimeSpec {
    pub version: String,
    pub minecraft_version: String,
    pub entrypoint: Vec<String>,
    pub launch_arguments: Vec<String>,
    pub public_ports: Vec<u16>,
    pub healthcheck: String,
    pub cpu_limit: u32,
    pub memory_limit_mib: u64,
    pub storage_gib: u64,
    pub persistent_volume_schema: String,
    pub shutdown_behavior: String,
    pub startup_timeout_seconds: u64,
    pub allowed_environment: Vec<String>,
    pub secret_references: Vec<String>,
}

impl MinecraftRuntimeSpec {
    /// The spec contains no caller-controlled values. It is an execution
    /// contract, not a deployment manifest.
    fn v1() -> Self {
        Self {
            version: "minecraft-runtime-v1".into(),
            minecraft_version: "artifact-defined".into(),
            entrypoint: vec!["devhub-minecraft-launcher".into()],
            launch_arguments: Vec::new(),
            public_ports: vec![25565],
            healthcheck: "minecraft-status-tcp-v1".into(),
            cpu_limit: 0,
            memory_limit_mib: 0,
            storage_gib: 0,
            persistent_volume_schema: "devhub-minecraft-world-v1".into(),
            shutdown_behavior: "graceful-save-then-stop-v1".into(),
            startup_timeout_seconds: 0,
            allowed_environment: Vec::new(),
            secret_references: Vec::new(),
        }
    }

    fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"devhub-minecraft-runtime-spec-v1\0");
        hasher.update(serde_json::to_vec(self).expect("runtime spec serializes"));
        hex::encode(hasher.finalize())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DevHubWorkloadInstance {
    pub workload_instance_id: String,
    pub workload_order_id: String,
    pub buyer_tenant_id: String,
    pub marketplace_idempotency_key: String,
    pub project_id: String,
    pub release_id: String,
    pub revision: String,
    pub artifact_digest: String,
    pub runtime_spec_digest: String,
    pub lifecycle_state: String,
    #[serde(default)]
    pub current_primary_allocation: Option<String>,
    #[serde(default)]
    pub persistent_storage_id: Option<String>,
    pub continuity_policy: String,
    pub created_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkloadClientCertificateCapability {
    pub mode: String,
    pub reload: bool,
}

impl WorkloadClientCertificateCapability {
    pub fn files_v1(&self) -> bool {
        self.mode == "files-v1" && self.reload
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketplaceWorkload {
    /// Opaque DevHub handoff receipt. It is intentionally unrelated to a
    /// deployment id, endpoint, provider, or credential selector.
    #[serde(default)]
    pub workload_handoff_id: String,
    pub allocation_id: String,
    pub project_id: String,
    pub release_id: String,
    pub revision: String,
    pub buyer_tenant: String,
    pub client_certificate_delivery_requested: bool,
    /// Opaque platform-issued credential selector.  It is deterministic from
    /// the immutable workload binding, but never names a host path or exposes
    /// certificate material.
    #[serde(default)]
    pub credential_id: Option<String>,
    pub created_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketplaceLifecycleEvent {
    pub event_id: String,
    pub event_version: u8,
    pub workload_order_id: String,
    pub buyer_tenant_id: String,
    pub allocation_id: String,
    pub lifecycle_status: String,
    pub occurred_at: String,
    pub reason_code: String,
    #[serde(default)]
    pub delivered: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceMigrationFact {
    pub project_id: String,
    pub database_id: String,
    pub version: String,
    pub name: String,
    pub content_sha256: String,
    pub applied_ms: u64,
}

#[derive(Default)]
pub struct MarketplaceReleaseStore(RwLock<MarketplaceReleaseSnapshot>);

impl MarketplaceReleaseStore {
    pub fn snapshot(&self) -> MarketplaceReleaseSnapshot {
        self.0.read().clone()
    }

    pub fn load(&self, snapshot: MarketplaceReleaseSnapshot) {
        *self.0.write() = snapshot;
    }

    pub(crate) fn release(&self, release_id: &str) -> Option<ProjectRelease> {
        self.0
            .read()
            .releases
            .iter()
            .find(|release| release.release_id == release_id)
            .cloned()
    }

    pub(crate) fn workload(&self, allocation_id: &str) -> Option<MarketplaceWorkload> {
        self.0
            .read()
            .workloads
            .iter()
            .find(|workload| workload.allocation_id == allocation_id)
            .cloned()
    }

    pub(crate) fn managed_postgres(&self, project: &str) -> Option<String> {
        self.0.read().managed_postgres.get(project).cloned()
    }

    /// Bind the project once. A conflicting database is never substituted:
    /// doing so could run migrations against a different tenant's engine.
    pub(crate) fn bind_managed_postgres(
        &self,
        project: &str,
        database_id: &str,
    ) -> Result<String, &'static str> {
        let mut state = self.0.write();
        match state.managed_postgres.get(project) {
            Some(existing) if existing == database_id => Ok(existing.clone()),
            Some(_) => Err("marketplace_managed_database_conflict"),
            None => {
                state
                    .managed_postgres
                    .insert(project.to_owned(), database_id.to_owned());
                Ok(database_id.to_owned())
            }
        }
    }

    pub(crate) fn migration_facts(
        &self,
        project: &str,
        database_id: &str,
    ) -> Vec<MarketplaceMigrationFact> {
        self.0
            .read()
            .migration_facts
            .iter()
            .filter(|fact| fact.project_id == project && fact.database_id == database_id)
            .cloned()
            .collect()
    }

    /// Persist only an exact idempotent replay. A version/content mismatch is
    /// a closed failure: modified historical migration text is never replayed.
    pub(crate) fn record_migration(
        &self,
        fact: MarketplaceMigrationFact,
    ) -> Result<(), &'static str> {
        let mut state = self.0.write();
        if let Some(existing) = state.migration_facts.iter().find(|existing| {
            existing.project_id == fact.project_id
                && existing.database_id == fact.database_id
                && existing.version == fact.version
        }) {
            return if existing.name == fact.name && existing.content_sha256 == fact.content_sha256 {
                Ok(())
            } else {
                Err("marketplace_migration_digest_mismatch")
            };
        }
        state.migration_facts.push(fact);
        Ok(())
    }

    fn insert_release(&self, release: ProjectRelease) {
        self.0.write().releases.push(release);
    }

    /// Resolve only a fully approved immutable executable binding. A
    /// source/build identity is provenance, not executable authority.
    pub(crate) fn executable_binding(
        &self,
        project_id: &str,
        release_id: &str,
        revision: &str,
    ) -> Option<ReleaseExecutionBinding> {
        let release = self.release(release_id).filter(|release| {
            release.project_id == project_id
                && release.revision == revision
                && release.published
                && !release.revoked
        })?;
        let binding = release.execution?;
        (binding.artifact.workload_type == "minecraft"
            && binding.artifact.approval_state == "approved"
            && binding.artifact.security_validated
            && binding.artifact.policy_validated
            && valid_sha256(&binding.artifact.digest)
            && binding.runtime_spec.version == "minecraft-runtime-v1"
            && binding.runtime_spec_digest == binding.runtime_spec.digest())
        .then_some(binding)
    }

    pub(crate) fn accept_workload_instance(
        &self,
        mut instance: DevHubWorkloadInstance,
    ) -> Result<DevHubWorkloadInstance, &'static str> {
        let mut state = self.0.write();
        if let Some(existing) = state.workload_instances.iter().find(|existing| {
            existing.marketplace_idempotency_key == instance.marketplace_idempotency_key
        }) {
            return if existing.workload_order_id == instance.workload_order_id
                && existing.buyer_tenant_id == instance.buyer_tenant_id
                && existing.project_id == instance.project_id
                && existing.release_id == instance.release_id
                && existing.revision == instance.revision
                && existing.continuity_policy == instance.continuity_policy
            {
                Ok(existing.clone())
            } else {
                Err("marketplace_workload_idempotency_conflict")
            };
        }
        if state
            .workload_instances
            .iter()
            .any(|existing| existing.workload_order_id == instance.workload_order_id)
        {
            return Err("marketplace_workload_order_already_accepted");
        }
        instance.lifecycle_state = "accepted".into();
        state.workload_instances.push(instance.clone());
        Ok(instance)
    }

    fn attach(&self, workload: MarketplaceWorkload) -> Result<MarketplaceWorkload, &'static str> {
        let mut state = self.0.write();
        if let Some(existing) = state
            .workloads
            .iter()
            .find(|existing| existing.allocation_id == workload.allocation_id)
        {
            return if existing.project_id == workload.project_id
                && existing.release_id == workload.release_id
                && existing.revision == workload.revision
                && existing.buyer_tenant == workload.buyer_tenant
                && existing.workload_handoff_id == workload.workload_handoff_id
                && existing.client_certificate_delivery_requested
                    == workload.client_certificate_delivery_requested
                && existing.credential_id == workload.credential_id
            {
                Ok(existing.clone())
            } else {
                Err("marketplace_workload_already_attached")
            };
        }
        state.workloads.push(workload.clone());
        Ok(workload)
    }

    /// Attach a workload only after the Marketplace allocation boundary has
    /// verified its payment, tenant, project, and immutable release binding.
    /// This keeps browser-authenticated project routes out of the commercial
    /// handoff path entirely.
    pub(crate) fn attach_from_marketplace(
        &self,
        workload_handoff_id: String,
        allocation_id: String,
        project_id: String,
        release_id: String,
        revision: String,
        buyer_tenant: String,
    ) -> Result<MarketplaceWorkload, &'static str> {
        self.attach(MarketplaceWorkload {
            workload_handoff_id,
            allocation_id,
            project_id,
            release_id,
            revision,
            buyer_tenant,
            client_certificate_delivery_requested: false,
            credential_id: None,
            created_ms: hive_core::now_ms(),
        })
    }

    pub fn workload_for_project(&self, project: &str) -> Option<MarketplaceWorkload> {
        self.0
            .read()
            .workloads
            .iter()
            .rev()
            .find(|workload| workload.project_id == project)
            .cloned()
    }

    /// Queue an immutable lifecycle event exactly once. Delivery retries retain
    /// the same event id and byte-for-byte payload; Marketplace deduplicates by
    /// that id rather than interpreting transport retries as new state.
    pub(crate) fn queue_lifecycle_event(
        &self,
        event: MarketplaceLifecycleEvent,
    ) -> MarketplaceLifecycleEvent {
        let mut state = self.0.write();
        if let Some(existing) = state
            .lifecycle_events
            .iter()
            .find(|existing| existing.event_id == event.event_id)
        {
            return existing.clone();
        }
        state.lifecycle_events.push(event.clone());
        event
    }

    pub(crate) fn pending_lifecycle_events(&self) -> Vec<MarketplaceLifecycleEvent> {
        self.0
            .read()
            .lifecycle_events
            .iter()
            .filter(|event| !event.delivered)
            .cloned()
            .collect()
    }

    pub(crate) fn mark_lifecycle_delivered(&self, event_id: &str) -> bool {
        let mut state = self.0.write();
        let Some(event) = state
            .lifecycle_events
            .iter_mut()
            .find(|event| event.event_id == event_id)
        else {
            return false;
        };
        event.delivered = true;
        true
    }

    /// Resolves only an exact immutable binding.  Project identity is never a
    /// credential authority: callers must carry the allocation and release
    /// captured when the workload was attached.
    pub fn workload_binding(
        &self,
        allocation_id: &str,
        project_id: &str,
        release_id: &str,
    ) -> Option<MarketplaceWorkload> {
        self.0
            .read()
            .workloads
            .iter()
            .find(|workload| {
                workload.allocation_id == allocation_id
                    && workload.project_id == project_id
                    && workload.release_id == release_id
            })
            .cloned()
    }

    /// Return a credential only when a project has one unambiguous immutable
    /// workload binding. A project with zero credentials receives no mount; a
    /// project with more than one is refused rather than guessing which
    /// allocation/release a deployment should represent.
    pub fn unambiguous_credential_for_project(
        &self,
        project: &str,
    ) -> Result<Option<String>, &'static str> {
        let state = self.0.read();
        let mut credentials = state
            .workloads
            .iter()
            .filter(|workload| workload.project_id == project)
            .filter_map(|workload| workload.credential_id.as_ref())
            .cloned();
        let first = credentials.next();
        if credentials.next().is_some() {
            return Err("marketplace_workload_credential_binding_ambiguous");
        }
        Ok(first)
    }
}

const CREDENTIAL_ROOT: &str = "/var/lib/hive/marketplace-workload-certs";
const CA_CERT_ENV: &str = "HIVE_MARKETPLACE_CA_CERT";
const CA_KEY_ENV: &str = "HIVE_MARKETPLACE_CA_KEY";

fn credential_id(allocation: &str, project: &str, release: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"hive-marketplace-workload-credential-v1\0");
    for value in [allocation, project, release] {
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    format!("mwc-{}", hex::encode(hasher.finalize()))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn configured_path(name: &str) -> Result<PathBuf, &'static str> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("marketplace_workload_certificate_unavailable")
}

fn trusted_directory(path: &StdPath) -> Result<(), &'static str> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "marketplace_workload_certificate_unavailable")?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != 0
        || metadata.gid() != 0
        || metadata.mode() & 0o022 != 0
    {
        return Err("marketplace_workload_certificate_unavailable");
    }
    Ok(())
}

fn trusted_ca_file(path: &StdPath, private: bool) -> Result<(), &'static str> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "marketplace_workload_certificate_unavailable")?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != 0
        || metadata.gid() != 0
        || (private && metadata.mode() & 0o077 != 0)
        || (!private && metadata.mode() & 0o022 != 0)
    {
        return Err("marketplace_workload_certificate_unavailable");
    }
    Ok(())
}

/// Issue an allocation/project/release-bound client credential beneath the
/// fixed root.  All subprocess output is discarded so an OpenSSL failure
/// cannot disclose a key, CA path, or host details through an API response.
async fn issue_credential(
    allocation: &str,
    project: &str,
    release: &str,
) -> Result<String, &'static str> {
    let root = PathBuf::from(CREDENTIAL_ROOT);
    let ca_cert = configured_path(CA_CERT_ENV)?;
    let ca_key = configured_path(CA_KEY_ENV)?;
    trusted_directory(&root)?;
    trusted_ca_file(&ca_cert, false)?;
    trusted_ca_file(&ca_key, true)?;

    let id = credential_id(allocation, project, release);
    let destination = root.join(&id);
    let mut replace_existing = false;
    if destination.exists() {
        trusted_directory(&destination)?;
        for (name, mode) in [("ca.crt", 0o444), ("tls.crt", 0o444), ("tls.key", 0o400)] {
            let metadata = std::fs::symlink_metadata(destination.join(name))
                .map_err(|_| "marketplace_workload_certificate_unavailable")?;
            use std::os::unix::fs::MetadataExt;
            if !metadata.file_type().is_file()
                || metadata.file_type().is_symlink()
                || metadata.uid() != 0
                || metadata.gid() != 0
                || metadata.mode() & 0o777 != mode
            {
                return Err("marketplace_workload_certificate_unavailable");
            }
        }
        let mut lifetime = Command::new("/usr/bin/openssl");
        lifetime
            .args([
                "x509",
                "-checkend",
                "3600",
                "-noout",
                "-in",
                destination
                    .join("tls.crt")
                    .to_str()
                    .ok_or("marketplace_workload_certificate_unavailable")?,
            ])
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut chain = Command::new("/usr/bin/openssl");
        chain
            .args([
                "verify",
                "-CAfile",
                destination
                    .join("ca.crt")
                    .to_str()
                    .ok_or("marketplace_workload_certificate_unavailable")?,
                destination
                    .join("tls.crt")
                    .to_str()
                    .ok_or("marketplace_workload_certificate_unavailable")?,
            ])
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if lifetime
            .status()
            .await
            .map(|status| status.success())
            .unwrap_or(false)
            && chain
                .status()
                .await
                .map(|status| status.success())
                .unwrap_or(false)
        {
            return Ok(id);
        }
        replace_existing = true;
    }

    let temporary = root.join(format!(".{id}.{}", Uuid::new_v4().simple()));
    std::fs::create_dir(&temporary).map_err(|_| "marketplace_workload_certificate_unavailable")?;
    let key = temporary.join("tls.key");
    let csr = temporary.join("request.csr");
    let cert = temporary.join("tls.crt");
    let ca_copy = temporary.join("ca.crt");
    let subject = format!("/CN=marketplace-workload-{id}");
    let common = |command: &mut Command| {
        command
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
    };
    let mut request = Command::new("/usr/bin/openssl");
    request.args([
        "req",
        "-new",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        key.to_str()
            .ok_or("marketplace_workload_certificate_unavailable")?,
        "-out",
        csr.to_str()
            .ok_or("marketplace_workload_certificate_unavailable")?,
        "-subj",
        &subject,
    ]);
    common(&mut request);
    if !request
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(false)
    {
        let _ = std::fs::remove_dir_all(&temporary);
        return Err("marketplace_workload_certificate_unavailable");
    }
    let mut sign = Command::new("/usr/bin/openssl");
    sign.args([
        "x509",
        "-req",
        "-in",
        csr.to_str()
            .ok_or("marketplace_workload_certificate_unavailable")?,
        "-CA",
        ca_cert
            .to_str()
            .ok_or("marketplace_workload_certificate_unavailable")?,
        "-CAkey",
        ca_key
            .to_str()
            .ok_or("marketplace_workload_certificate_unavailable")?,
        "-CAcreateserial",
        "-out",
        cert.to_str()
            .ok_or("marketplace_workload_certificate_unavailable")?,
        "-days",
        "30",
        "-sha256",
    ]);
    common(&mut sign);
    if !sign
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(false)
    {
        let _ = std::fs::remove_dir_all(&temporary);
        return Err("marketplace_workload_certificate_unavailable");
    }
    if std::fs::copy(&ca_cert, &ca_copy).is_err() {
        let _ = std::fs::remove_dir_all(&temporary);
        return Err("marketplace_workload_certificate_unavailable");
    }
    for (path, mode) in [(&key, 0o400), (&cert, 0o444), (&ca_copy, 0o444)] {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).is_err() {
            let _ = std::fs::remove_dir_all(&temporary);
            return Err("marketplace_workload_certificate_unavailable");
        }
    }
    if replace_existing {
        // Keep the mounted directory inode stable. Renaming each file replaces
        // the exact runtime path atomically, so a running workload observes
        // either its old complete credential or the new complete file.
        for name in ["ca.crt", "tls.crt", "tls.key"] {
            if std::fs::rename(temporary.join(name), destination.join(name)).is_err() {
                let _ = std::fs::remove_dir_all(&temporary);
                return Err("marketplace_workload_certificate_unavailable");
            }
        }
        let _ = std::fs::remove_dir_all(&temporary);
    } else if std::fs::rename(&temporary, &destination).is_err() {
        let _ = std::fs::remove_dir_all(&temporary);
        return Err("marketplace_workload_certificate_unavailable");
    }
    Ok(id)
}

#[derive(Deserialize)]
struct CreateReleaseRequest {
    revision: String,
    published: bool,
    #[serde(default)]
    workload_client_certificate: Option<WorkloadClientCertificateCapability>,
    #[serde(default)]
    source_identity: String,
}

#[derive(Deserialize)]
struct AttachWorkloadRequest {
    allocation_id: String,
    release_id: String,
    revision: String,
    client_certificate_delivery_requested: bool,
}

pub fn routes(cloud: Arc<CloudState>) -> Router {
    Router::new()
        .route(
            "/v1/projects/:project/marketplace-releases",
            post(create_release),
        )
        .with_state(cloud)
}

/// Renew bound credentials before their one-hour validity floor elapses. A
/// failed renewal is deliberately observable in the node health/readiness log;
/// it never falls back to a different workload's credential.
pub fn spawn_credential_rotation(cloud: Arc<CloudState>) {
    tokio::spawn(async move {
        loop {
            let workloads = cloud.marketplace_releases.snapshot().workloads;
            for workload in workloads
                .into_iter()
                .filter(|workload| workload.client_certificate_delivery_requested)
            {
                if issue_credential(
                    &workload.allocation_id,
                    &workload.project_id,
                    &workload.release_id,
                )
                .await
                .is_err()
                {
                    tracing::error!("Marketplace workload credential readiness failed");
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        }
    });
}

async fn create_release(
    State(cloud): State<Arc<CloudState>>,
    Path(project): Path<String>,
    headers: HeaderMap,
    claims: Option<axum::Extension<crate::auth::Claims>>,
    Json(request): Json<CreateReleaseRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, String)> {
    crate::admin::require_project(
        &cloud,
        &headers,
        claims.as_ref().map(|claim| &claim.0),
        &project,
    )?;
    if request.revision.trim().is_empty()
        || request.revision.len() > 256
        || request.source_identity.len() > 512
        || request
            .workload_client_certificate
            .as_ref()
            .is_some_and(|capability| !capability.files_v1())
    {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "invalid_marketplace_release".into(),
        ));
    }
    let release = ProjectRelease {
        release_id: format!("rel_{}", Uuid::new_v4().simple()),
        project_id: project,
        revision: request.revision,
        published: request.published,
        revoked: false,
        workload_client_certificate: request.workload_client_certificate,
        source_identity: request.source_identity,
        // Source identity is retained for provenance only. The current
        // release endpoint cannot attach a DevHub-stored immutable artifact,
        // so a newly created release is intentionally source-only.
        execution: None,
        created_ms: hive_core::now_ms(),
    };
    cloud.marketplace_releases.insert_release(release.clone());
    crate::persist::persist(&cloud);
    Ok(Json(
        json!({"release_id": release.release_id, "revision": release.revision}),
    ))
}

async fn attach_workload(
    State(cloud): State<Arc<CloudState>>,
    Path(project): Path<String>,
    headers: HeaderMap,
    claims: Option<axum::Extension<crate::auth::Claims>>,
    Json(request): Json<AttachWorkloadRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, String)> {
    let tenant = crate::admin::require_project(
        &cloud,
        &headers,
        claims.as_ref().map(|claim| &claim.0),
        &project,
    )?;
    let Some(allocation) = cloud.marketplace_allocations.get(&request.allocation_id) else {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            "marketplace_allocation_not_found".into(),
        ));
    };
    let Some(release) = cloud.marketplace_releases.release(&request.release_id) else {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            "marketplace_release_not_found".into(),
        ));
    };
    if !release.published || release.revoked {
        return Err((
            axum::http::StatusCode::CONFLICT,
            "marketplace_release_unavailable".into(),
        ));
    }
    if allocation.tenant_id != tenant || allocation.tenant_id != cloud.projects.team_of(&project) {
        return Err((
            axum::http::StatusCode::FORBIDDEN,
            "marketplace_buyer_mismatch".into(),
        ));
    }
    if release.project_id != project || release.revision != request.revision {
        return Err((
            axum::http::StatusCode::CONFLICT,
            "marketplace_release_mismatch".into(),
        ));
    }
    if request.client_certificate_delivery_requested
        && !release
            .workload_client_certificate
            .as_ref()
            .is_some_and(WorkloadClientCertificateCapability::files_v1)
    {
        return Err((
            axum::http::StatusCode::CONFLICT,
            "marketplace_workload_client_certificate_unsupported".into(),
        ));
    }
    let credential_id = if request.client_certificate_delivery_requested {
        Some(
            issue_credential(&request.allocation_id, &project, &release.release_id)
                .await
                .map_err(|code| (axum::http::StatusCode::SERVICE_UNAVAILABLE, code.into()))?,
        )
    } else {
        None
    };
    let workload = cloud
        .marketplace_releases
        .attach(MarketplaceWorkload {
            workload_handoff_id: format!("wh_{}", Uuid::new_v4().simple()),
            allocation_id: request.allocation_id.clone(),
            project_id: project.clone(),
            release_id: release.release_id,
            revision: release.revision,
            buyer_tenant: tenant.clone(),
            client_certificate_delivery_requested: request.client_certificate_delivery_requested,
            credential_id,
            created_ms: hive_core::now_ms(),
        })
        .map_err(|code| (axum::http::StatusCode::CONFLICT, code.into()))?;
    // The Marketplace project has one engine identity. Reuse the ordinary
    // managed-database record and provisioning path so project ownership,
    // host routing, lifecycle fencing, and reconciliation remain unchanged.
    // No connection value crosses this boundary or enters the release store.
    let database = if let Some(id) = cloud.marketplace_releases.managed_postgres(&project) {
        cloud.databases.get_raw(&id).ok_or((
            axum::http::StatusCode::CONFLICT,
            "marketplace_managed_database_unavailable".into(),
        ))?
    } else if let Some(existing) = cloud
        .databases
        .project_database_raw(&project, crate::databases::DbKind::Postgres)
    {
        existing
    } else {
        crate::databases::provision(
            cloud.databases.clone(),
            cloud.region.clone(),
            crate::databases::ProvisionReq {
                name: "marketplace-postgres".into(),
                project: project.clone(),
                team: tenant.clone(),
                kind: crate::databases::DbKind::Postgres,
                region: None,
                provider: Some("Marketplace managed Postgres".into()),
                replicas: Vec::new(),
            },
            cloud.db_domain.clone(),
            cloud.node_name.clone(),
            cloud.api_base(),
            |_| {},
        )
    };
    cloud
        .marketplace_releases
        .bind_managed_postgres(&project, &database.id)
        .map_err(|code| (axum::http::StatusCode::CONFLICT, code.into()))?;
    crate::persist::persist(&cloud);
    Ok(Json(json!({
        "allocation_id": workload.allocation_id,
        "release_id": workload.release_id,
        "revision": workload.revision,
    })))
}
