//! DevHub-owned executor for approved Minecraft workloads.
//!
//! This is intentionally a narrow executor, not a scheduler.  Marketplace
//! supplies only a release id/revision and commercial capacity request; this
//! module resolves the artifact, runtime image, ports, secret selectors, and
//! node-local world volume from DevHub-owned durable records.

use std::{sync::Arc, time::Duration};

use crate::{
    marketplace_releases::{DevHubWorkloadInstance, MarketplaceLifecycleEvent},
    state::CloudState,
};
use sha2::{Digest, Sha256};
use tokio::process::Command;

const STATEFUL_AFFINITY_REQUIRED: &str = "stateful_workload_node_affinity_required";

pub fn spawn(cloud: Arc<CloudState>) {
    tokio::spawn(async move {
        loop {
            for instance in cloud.marketplace_releases.workload_instances() {
                reconcile(&cloud, instance).await;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

async fn reconcile(cloud: &Arc<CloudState>, mut instance: DevHubWorkloadInstance) {
    if matches!(
        instance.lifecycle_state.as_str(),
        "ready" | "stopped" | "failed"
    ) {
        return;
    }
    let Some(allocation) = cloud
        .marketplace_allocations
        .for_workload_order(&instance.workload_order_id)
    else {
        // A workload order may arrive before its capacity allocation. It stays
        // accepted; there is no speculative placement or start.
        return;
    };
    let target = allocation.approved_node_ids.first().cloned();
    if target.as_deref() != Some(cloud.node_name.as_str()) {
        return;
    }
    if let Some(storage_node) = instance.storage_node.as_deref() {
        if storage_node != cloud.node_name {
            fail(cloud, &mut instance, STATEFUL_AFFINITY_REQUIRED).await;
            return;
        }
    }
    if allocation.resources.vcpu < instance.requested_vcpu
        || allocation.resources.memory_mb < instance.requested_memory_mib
        || allocation.resources.disk_gb < instance.requested_storage_gib
    {
        fail(cloud, &mut instance, "insufficient_allocated_capacity").await;
        return;
    }

    transition(
        cloud,
        &mut instance,
        "resolving_artifact",
        "workload.accepted",
        "accepted",
    )
    .await;
    let Some(binding) = cloud.marketplace_releases.executable_binding(
        &instance.project_id,
        &instance.release_id,
        &instance.revision,
    ) else {
        fail(cloud, &mut instance, "artifact_unapproved_or_missing").await;
        return;
    };
    if binding.artifact.digest != instance.artifact_digest
        || binding.runtime_spec_digest != instance.runtime_spec_digest
    {
        fail(cloud, &mut instance, "runtime_spec_mismatch").await;
        return;
    }
    let Some(catalog) = cloud
        .marketplace_releases
        .runtime_artifact(&binding.artifact.digest)
    else {
        fail(cloud, &mut instance, "artifact_missing").await;
        return;
    };
    if catalog.revoked || catalog.approval_state != "approved" {
        fail(cloud, &mut instance, "artifact_unapproved_or_revoked").await;
        return;
    }
    if catalog.storage_node != cloud.node_name {
        // The legacy transfer store's package bytes are node-local.  Never
        // treat a replicated catalog record as evidence that another node has
        // the bytes, and never substitute a mutable remote image/tag.
        fail(
            cloud,
            &mut instance,
            "artifact_unavailable_on_allocated_node",
        )
        .await;
        return;
    }

    transition(
        cloud,
        &mut instance,
        "materializing",
        "artifact.resolved",
        "immutable_artifact_resolved",
    )
    .await;
    // `reopen` validates the semantic marker; `verified_package` recomputes
    // the package SHA-256.  The sealed artifact store is content-addressed,
    // durable under HIVE_DATA, and this path never accepts a mutable URL/tag.
    let store = crate::persist::data_dir().join("runtime-artifacts-v1");
    let sealed = match hive_backend::reopen_sealed_runtime_artifact(&store, &catalog.package) {
        Ok(sealed) => sealed,
        Err(_) => {
            fail(cloud, &mut instance, "artifact_missing").await;
            return;
        }
    };
    if sealed.verified_package().is_err() {
        fail(cloud, &mut instance, "artifact_digest_mismatch").await;
        return;
    }
    let artifact_root = match sealed.host_app_root() {
        Ok(path) => path,
        Err(_) => {
            fail(cloud, &mut instance, "materialization_failure").await;
            return;
        }
    };
    transition(
        cloud,
        &mut instance,
        "starting",
        "artifact.materialized",
        "sealed_package_verified_and_materialized",
    )
    .await;

    let world = instance
        .persistent_storage_id
        .clone()
        .unwrap_or_else(|| world_volume_name(&instance.workload_instance_id));
    instance.persistent_storage_id = Some(world.clone());
    instance.storage_node = Some(cloud.node_name.clone());
    instance.current_primary_allocation = Some(allocation.allocation_id);
    if ensure_volume(&world).await.is_err() {
        fail(cloud, &mut instance, "storage_binding_unavailable").await;
        return;
    }

    let container = minecraft_container_name(&instance.workload_instance_id);
    let secrets = match resolve_secret_references(&binding.runtime_spec.secret_references) {
        Ok(values) => values,
        Err(reason) => {
            fail(cloud, &mut instance, reason).await;
            return;
        }
    };
    if !hive_backend::container_cli::is_running(false, &container, &crate::git::podman_path_env())
        .await
        && launch(
            &container,
            &world,
            &artifact_root,
            &binding.runtime_spec,
            &secrets,
        )
        .await
        .is_err()
    {
        fail(cloud, &mut instance, "runtime_launch_failure").await;
        return;
    }
    instance.runtime_container_id = Some(container.clone());
    instance.runtime_process_started = true;
    instance.runtime_healthy =
        hive_backend::container_cli::is_running(false, &container, &crate::git::podman_path_env())
            .await;
    if !instance.runtime_healthy {
        fail(cloud, &mut instance, "runtime_launch_failure").await;
        return;
    }
    event(
        cloud,
        &instance,
        "deployment.started",
        "runtime_process_started",
    );

    // Readiness is explicitly TCP-listen only in this milestone.  It proves
    // the selected Minecraft port accepts a connection; it does not claim
    // Minecraft protocol status/ping success.
    match published_port(&container).await {
        Some(port) if tcp_ready(port, binding.runtime_spec.startup_timeout_seconds).await => {
            instance.workload_ready = true;
            instance.lifecycle_state = "ready".into();
            instance.failure_reason = None;
            cloud
                .marketplace_releases
                .update_workload_instance(instance.clone());
            crate::persist::persist(cloud);
            event(
                cloud,
                &instance,
                "workload.ready",
                "tcp_port_accepting_connections",
            );
        }
        _ => fail(cloud, &mut instance, "readiness_timeout").await,
    }
}

async fn ensure_volume(volume: &str) -> Result<(), ()> {
    let status = Command::new("podman")
        .args(["volume", "inspect", volume])
        .env("PATH", crate::git::podman_path_env())
        .status()
        .await
        .map_err(|_| ())?;
    if status.success() {
        return Ok(());
    }
    Command::new("podman")
        .args(["volume", "create", volume])
        .env("PATH", crate::git::podman_path_env())
        .status()
        .await
        .map_err(|_| ())
        .and_then(|status| status.success().then_some(()).ok_or(()))
}

async fn launch(
    container: &str,
    world: &str,
    artifact_root: &std::path::Path,
    spec: &crate::marketplace_releases::MinecraftRuntimeSpec,
    secrets: &[(String, String)],
) -> Result<(), ()> {
    let artifact_root = artifact_root.to_str().ok_or(())?;
    let image = spec.runtime_image.strip_prefix("docker://").ok_or(())?;
    let mut args = vec![
        "run".into(),
        "--detach".into(),
        "--replace".into(),
        "--name".into(),
        container.into(),
        "--memory".into(),
        format!("{}m", spec.memory_limit_mib),
        "--cpus".into(),
        spec.cpu_limit.to_string(),
        "--security-opt".into(),
        "no-new-privileges".into(),
        "--mount".into(),
        format!("type=volume,source={world},target=/data"),
        "--mount".into(),
        format!("type=bind,source={artifact_root},target=/srv/artifact,readonly"),
        // Podman assigns a host port, avoiding collisions while exposing only
        // the one runtime-spec-approved Minecraft port.
        "--publish".into(),
        "127.0.0.1::25565/tcp".into(),
    ];
    for (name, value) in secrets {
        args.push("--env".into());
        args.push(format!("{name}={value}"));
    }
    args.push(image.into());
    args.extend(spec.entrypoint.iter().cloned());
    args.extend(spec.launch_arguments.iter().cloned());
    Command::new("podman")
        .args(args)
        .env("PATH", crate::git::podman_path_env())
        .status()
        .await
        .map_err(|_| ())
        .and_then(|status| status.success().then_some(()).ok_or(()))
}

async fn published_port(container: &str) -> Option<u16> {
    let output = Command::new("podman")
        .args(["port", container, "25565/tcp"])
        .env("PATH", crate::git::podman_path_env())
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let line = std::str::from_utf8(&output.stdout).ok()?.trim();
    line.rsplit(':').next()?.parse().ok()
}

async fn tcp_ready(port: u16, timeout_seconds: u64) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_seconds);
    while tokio::time::Instant::now() < deadline {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return true;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    false
}

fn resolve_secret_references(references: &[String]) -> Result<Vec<(String, String)>, &'static str> {
    references
        .iter()
        .map(|reference| {
            let key = reference
                .strip_prefix("devhub-secret://")
                .ok_or("devhub_secret_reference_invalid")?
                .replace(['/', '-'], "_")
                .to_ascii_uppercase();
            let env = format!("HIVE_DEVHUB_SECRET_{key}");
            let value = std::env::var(&env).map_err(|_| "devhub_secret_unavailable")?;
            Ok((key, value))
        })
        .collect()
}

async fn transition(
    cloud: &Arc<CloudState>,
    instance: &mut DevHubWorkloadInstance,
    state: &str,
    callback: &str,
    reason: &str,
) {
    if instance.lifecycle_state != state {
        instance.lifecycle_state = state.into();
        instance.failure_reason = None;
        cloud
            .marketplace_releases
            .update_workload_instance(instance.clone());
        crate::persist::persist(cloud);
        event(cloud, instance, callback, reason);
    }
}

async fn fail(cloud: &Arc<CloudState>, instance: &mut DevHubWorkloadInstance, reason: &str) {
    instance.lifecycle_state = "failed".into();
    instance.failure_reason = Some(reason.into());
    instance.runtime_healthy = false;
    instance.workload_ready = false;
    cloud
        .marketplace_releases
        .update_workload_instance(instance.clone());
    crate::persist::persist(cloud);
    event(cloud, instance, "workload.failed", reason);
}

fn event(cloud: &Arc<CloudState>, instance: &DevHubWorkloadInstance, status: &str, reason: &str) {
    let mut hash = Sha256::new();
    hash.update(b"devhub-workload-event-v1\0");
    hash.update(instance.workload_instance_id.as_bytes());
    hash.update(status.as_bytes());
    hash.update(reason.as_bytes());
    let event_id = format!("evt_{}", hex::encode(hash.finalize()));
    cloud
        .marketplace_releases
        .queue_lifecycle_event(MarketplaceLifecycleEvent {
            event_id,
            event_version: 1,
            workload_order_id: instance.workload_order_id.clone(),
            buyer_tenant_id: instance.buyer_tenant_id.clone(),
            allocation_id: instance
                .current_primary_allocation
                .clone()
                .unwrap_or_else(|| instance.workload_instance_id.clone()),
            lifecycle_status: status.into(),
            occurred_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            reason_code: reason.into(),
            delivered: false,
        });
}

fn world_volume_name(instance: &str) -> String {
    format!(
        "hive-minecraft-world-{}",
        crate::git::sanitize_tag(instance)
    )
}

fn minecraft_container_name(instance: &str) -> String {
    format!("hive-minecraft-{}", crate::git::sanitize_tag(instance))
}
