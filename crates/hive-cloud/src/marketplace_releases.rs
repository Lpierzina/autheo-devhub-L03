//! DevHub-owned Marketplace release authority.
//!
//! A Marketplace release is deliberately not a deployment: one immutable
//! release can be deployed more than once, while Marketplace allocations bind
//! to the release identity captured below.  This store contains no PEM, HMAC,
//! source checkout, image tag, or mesh-routing material.

use std::{collections::BTreeMap, sync::Arc};

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
use uuid::Uuid;

use crate::state::CloudState;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MarketplaceReleaseSnapshot {
    #[serde(default)]
    pub releases: Vec<ProjectRelease>,
    #[serde(default)]
    pub workloads: Vec<MarketplaceWorkload>,
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
    /// Opaque platform-issued mount selector. It is deterministically bound to
    /// the allocation/project/release tuple and contains no certificate or key.
    #[serde(default)]
    pub credential_id: Option<String>,
    pub created_ms: u64,
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
    let workload = cloud
        .marketplace_releases
        .attach(MarketplaceWorkload {
            allocation_id: request.allocation_id.clone(),
            project_id: project,
            release_id: release.release_id,
            revision: release.revision,
            buyer_tenant: tenant,
            client_certificate_delivery_requested: request.client_certificate_delivery_requested,
            credential_id: request.client_certificate_delivery_requested.then(|| {
                format!(
                    "mw-{}",
                    &hex::encode(Sha256::digest(format!(
                        "{}\0{}\0{}",
                        request.allocation_id, project, release.release_id
                    )))[..48]
                )
            }),
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
