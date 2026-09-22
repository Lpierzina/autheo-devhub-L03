//! DevHub-owned Marketplace release authority.
//!
//! A Marketplace release is deliberately not a deployment: one immutable
//! release can be deployed more than once, while Marketplace allocations bind
//! to the release identity captured below.  This store contains no PEM, HMAC,
//! source checkout, image tag, or mesh-routing material.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::Json,
    routing::post,
    Router,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

#[derive(Default)]
pub struct MarketplaceReleaseStore(RwLock<MarketplaceReleaseSnapshot>);

impl MarketplaceReleaseStore {
    pub fn snapshot(&self) -> MarketplaceReleaseSnapshot {
        self.0.read().clone()
    }

    pub fn load(&self, snapshot: MarketplaceReleaseSnapshot) {
        *self.0.write() = snapshot;
    }

    fn release(&self, release_id: &str) -> Option<ProjectRelease> {
        self.0
            .read()
            .releases
            .iter()
            .find(|release| release.release_id == release_id)
            .cloned()
    }

    fn insert_release(&self, release: ProjectRelease) {
        self.0.write().releases.push(release);
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

    pub fn workload_for_project(&self, project: &str) -> Option<MarketplaceWorkload> {
        self.0
            .read()
            .workloads
            .iter()
            .rev()
            .find(|workload| workload.project_id == project)
            .cloned()
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

fn configured_path(name: &str) -> Result<PathBuf, &'static str> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("marketplace_workload_certificate_unavailable")
}

fn trusted_directory(path: &Path) -> Result<(), &'static str> {
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

fn trusted_ca_file(path: &Path, private: bool) -> Result<(), &'static str> {
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
        return Ok(id);
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
    if std::fs::rename(&temporary, &destination).is_err() {
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
        .route(
            "/v1/projects/:project/marketplace-workloads",
            post(attach_workload),
        )
        .with_state(cloud)
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
            allocation_id: request.allocation_id,
            project_id: project,
            release_id: release.release_id,
            revision: release.revision,
            buyer_tenant: tenant,
            client_certificate_delivery_requested: request.client_certificate_delivery_requested,
            credential_id,
            created_ms: hive_core::now_ms(),
        })
        .map_err(|code| (axum::http::StatusCode::CONFLICT, code.into()))?;
    crate::persist::persist(&cloud);
    Ok(Json(json!({
        "allocation_id": workload.allocation_id,
        "release_id": workload.release_id,
        "revision": workload.revision,
    })))
}
