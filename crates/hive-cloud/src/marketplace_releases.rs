//! DevHub-owned Marketplace release authority.
//!
//! A Marketplace release is deliberately not a deployment: one immutable
//! release can be deployed more than once, while Marketplace allocations bind
//! to the release identity captured below.  This store contains no PEM, HMAC,
//! source checkout, image tag, or mesh-routing material.

use std::{collections::BTreeMap, sync::Arc};

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
            allocation_id: request.allocation_id,
            project_id: project,
            release_id: release.release_id,
            revision: release.revision,
            buyer_tenant: tenant,
            client_certificate_delivery_requested: request.client_certificate_delivery_requested,
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
