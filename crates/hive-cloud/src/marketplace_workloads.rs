//! Server-owned Marketplace workload contracts.
//!
//! This module intentionally models a workload separately from an allocation:
//! allocations are short-lived compute entitlements, while a workload names the
//! purchased executable and its storage contract.  The current storage backend
//! is truthful about being node-local.  No caller may infer portable restore,
//! shared attachment, replication, fencing, or failover from a persistent
//! volume record.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MINECRAFT_RUNTIME_SPEC_VERSION: u16 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutableArtifact {
    pub artifact_id: String,
    /// `sha256:<64 lowercase hex>` over the sealed runtime-artifact package.
    pub artifact_digest: String,
    /// `devhub://runtime-artifacts/<sha256 hex>`.  This is a DevHub authority
    /// reference, never an OCI tag, URL, or Marketplace-controlled locator.
    pub reference: String,
    pub provenance: ArtifactProvenance,
    pub approval: ArtifactApproval,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactProvenance {
    /// Platform-issued build/import transaction identity.
    pub source_transaction_id: String,
    /// Semantic tree digest emitted by the sealed runtime artifact service.
    pub semantic_tree_digest: String,
    pub imported_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactApproval {
    pub approved: bool,
    pub security_policy: String,
    pub approved_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MinecraftRuntimeSpec {
    pub version: u16,
    pub workload_type: String,
    pub server_implementation: String,
    pub server_version: String,
    pub artifact_reference: String,
    pub entrypoint: Vec<String>,
    pub launch_args: Vec<String>,
    pub ports: Vec<RuntimePort>,
    pub cpu_millis: u32,
    pub memory_mib: u64,
    pub storage_mib: u64,
    pub persistent_mount: PersistentMountSchema,
    pub startup_timeout_secs: u64,
    pub shutdown: ShutdownBehavior,
    pub health: MinecraftHealthCheck,
    /// These are names and validation rules only. Values are DevHub-resolved.
    pub allowed_environment: Vec<EnvironmentField>,
    /// Logical DevHub secret selectors only; Marketplace never supplies values.
    pub internal_secret_refs: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimePort {
    pub name: String,
    pub port: u16,
    pub protocol: String,
    pub public: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistentMountSchema {
    pub name: String,
    pub mount_path: String,
    pub required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShutdownBehavior {
    pub signal: String,
    pub grace_period_secs: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MinecraftHealthCheck {
    /// `tcp_connect` is the strongest check currently supportable by the
    /// generic container runtime. It proves a listening game port, not a full
    /// Minecraft protocol response.
    pub kind: String,
    pub port_name: String,
    pub initial_delay_secs: u64,
    pub interval_secs: u64,
    pub failure_threshold: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvironmentField {
    pub name: String,
    pub required: bool,
    pub pattern: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseArtifactBinding {
    pub project_id: String,
    pub release_id: String,
    pub revision: String,
    pub artifact_id: String,
    pub artifact_digest: String,
    pub runtime_spec_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkloadInstance {
    pub workload_instance_id: String,
    pub marketplace_workload_reference: String,
    pub buyer_tenant: String,
    pub project_id: String,
    pub release_id: String,
    pub revision: String,
    pub artifact_id: String,
    pub artifact_digest: String,
    pub runtime_spec_version: u16,
    pub runtime_spec_digest: String,
    pub lifecycle_state: String,
    pub current_primary_allocation: Option<String>,
    pub storage_binding_id: String,
    pub continuity_policy: ContinuityPolicy,
    pub created_ms: u64,
    pub updated_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistentVolumeBinding {
    pub volume_id: String,
    pub workload_instance_id: String,
    pub backend: String,
    pub created_ms: u64,
    pub current_attachment: Option<String>,
    pub durability_state: String,
    pub capabilities: StorageCapabilities,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuityPolicy {
    pub mode: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageCapabilities {
    pub persistent: bool,
    pub snapshot: bool,
    pub portable_restore: bool,
    pub replication: bool,
    pub multi_node_attach: bool,
}

impl StorageCapabilities {
    pub const fn node_local() -> Self {
        Self {
            persistent: true,
            snapshot: true,
            portable_restore: false,
            replication: false,
            multi_node_attach: false,
        }
    }
}

pub fn digest_runtime_spec(spec: &MinecraftRuntimeSpec) -> anyhow::Result<String> {
    let encoded = serde_json::to_vec(spec)?;
    Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
}

pub fn valid_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn devhub_artifact_reference(digest: &str) -> Option<String> {
    valid_sha256_digest(digest)
        .then(|| format!("devhub://runtime-artifacts/{}", &digest["sha256:".len()..]))
}

pub fn validate_artifact(artifact: &ExecutableArtifact) -> Result<(), &'static str> {
    if artifact.artifact_id.is_empty()
        || artifact.provenance.source_transaction_id.is_empty()
        || artifact.provenance.semantic_tree_digest.is_empty()
        || artifact.approval.security_policy.is_empty()
        || !artifact.approval.approved
        || !valid_sha256_digest(&artifact.artifact_digest)
        || devhub_artifact_reference(&artifact.artifact_digest).as_deref()
            != Some(artifact.reference.as_str())
    {
        return Err("marketplace_artifact_invalid");
    }
    Ok(())
}

pub fn validate_minecraft_spec(
    spec: &MinecraftRuntimeSpec,
    artifact: &ExecutableArtifact,
) -> Result<(), &'static str> {
    if spec.version != MINECRAFT_RUNTIME_SPEC_VERSION
        || spec.workload_type != "minecraft"
        || spec.server_implementation.is_empty()
        || spec.server_version.is_empty()
        || spec.artifact_reference != artifact.reference
        || spec.entrypoint.is_empty()
        || spec.ports.is_empty()
        || spec.cpu_millis == 0
        || spec.memory_mib == 0
        || spec.storage_mib == 0
        || spec.persistent_mount.name != "world"
        || !spec.persistent_mount.required
        || !spec.persistent_mount.mount_path.starts_with('/')
        || spec.startup_timeout_secs == 0
        || spec.shutdown.grace_period_secs == 0
        || spec.health.kind != "tcp_connect"
        || spec.health.interval_secs == 0
        || spec.health.failure_threshold == 0
    {
        return Err("marketplace_minecraft_runtime_spec_invalid");
    }
    let port = spec.ports.iter().find(|port| port.name == spec.health.port_name);
    if port.is_none_or(|port| port.protocol != "tcp" || port.port == 0) {
        return Err("marketplace_minecraft_runtime_spec_invalid");
    }
    if spec
        .internal_secret_refs
        .iter()
        .any(|reference| reference.is_empty() || reference.contains('/') || reference.contains(':'))
    {
        return Err("marketplace_minecraft_runtime_spec_invalid");
    }
    Ok(())
}

pub fn validate_continuity(
    policy: &ContinuityPolicy,
    capabilities: &StorageCapabilities,
) -> Result<(), &'static str> {
    match policy.mode.as_str() {
        "none" if capabilities.persistent => Ok(()),
        "warm_standby" if capabilities.portable_restore && capabilities.replication => Ok(()),
        "warm_standby" => Err("portable_storage_unavailable"),
        "automatic_failover" => Err("automatic_failover_unsupported"),
        _ => Err("marketplace_continuity_policy_invalid"),
    }
}
