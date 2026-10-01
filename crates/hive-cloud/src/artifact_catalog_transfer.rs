//! Immutable catalog-artifact materialization.
//!
//! This is deliberately separate from deployment fanout transactions: a
//! catalog digest names reusable immutable bytes, while a deployment transfer
//! also carries a manifest, hidden candidate, and commit authority.

use std::{
    io::{Read, Write},
    os::unix::fs::FileExt,
    sync::Arc,
};

use anyhow::{bail, Context};
use sha2::{Digest, Sha256};

use crate::{
    marketplace_releases::{ArtifactMaterializationEvidence, DevHubRuntimeArtifact},
    state::CloudState,
};

const CHUNK_BYTES: usize = 512 * 1024;
const TOKEN_TTL_SECS: i64 = 180;

pub async fn materialize(
    cloud: &Arc<CloudState>,
    artifact: &DevHubRuntimeArtifact,
) -> anyhow::Result<()> {
    let store = crate::persist::data_dir().join("runtime-artifacts-v1");
    if locally_verified(&store, artifact) {
        record(cloud, artifact, &cloud.node_name, "already_verified", Some(&artifact.sha256));
        return Ok(());
    }
    if artifact.storage_node == cloud.node_name {
        bail!("catalog artifact is absent or invalid on its recorded source node");
    }
    let source = cloud
        .registry
        .nodes()
        .into_iter()
        .find(|node| node.name == artifact.storage_node)
        .context("catalog artifact source node is not in the live registry")?;
    let (peer_id, address) = source
        .peer_id
        .zip(source.iroh_addr)
        .context("catalog artifact source lacks a verified mesh address")?;
    let token = crate::auth::issue(
        &format!("mesh-node:{}", cloud.node_name),
        "system",
        "service",
        false,
        TOKEN_TTL_SECS,
    )
    .context("mint catalog artifact transfer token")?;
    let temp = store.join(format!(".catalog-{}-{}.partial", artifact.sha256, uuid::Uuid::new_v4()));
    let outcome = async {
        std::fs::create_dir_all(&store).context("create runtime artifact store")?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .context("create catalog artifact partial")?;
        let mut hash = Sha256::new();
        let mut offset = 0u64;
        while offset < artifact.package.package_bytes {
            let length = (artifact.package.package_bytes - offset).min(CHUNK_BYTES as u64) as usize;
            let path = format!(
                "/v1/devhub/runtime-artifacts/v1/{}/package?offset={offset}&length={length}&tok={token}",
                artifact.sha256
            );
            let bytes = crate::gossip::request_to_with_response_cap(
                cloud,
                &peer_id,
                &address,
                hive_p2p::GOSSIP_GET,
                &path,
                &[],
                60,
                CHUNK_BYTES,
            )
            .await
            .context("catalog artifact source did not answer")?;
            if bytes.len() != length {
                bail!("catalog artifact source returned an incomplete chunk");
            }
            output.write_all(&bytes).context("write catalog artifact partial")?;
            hash.update(&bytes);
            offset += bytes.len() as u64;
        }
        output.sync_all().context("sync catalog artifact partial")?;
        let digest = format!("{:x}", hash.finalize());
        anyhow::ensure!(
            digest == artifact.sha256 && digest == artifact.package.package_sha256,
            "catalog artifact package digest mismatch"
        );
        let package = std::fs::File::open(&temp).context("open verified catalog package")?;
        let sealed = hive_backend::materialize_runtime_artifact_package(
            package,
            artifact.package.clone(),
            &store,
        )
        .await
        .context("materialize verified catalog package")?;
        anyhow::ensure!(
            sealed.content_sha256() == artifact.package.semantic_tree_sha256,
            "catalog artifact semantic identity changed during materialization"
        );
        Ok(())
    }
    .await;
    let _ = std::fs::remove_file(&temp);
    match outcome {
        Ok(()) => {
            record(
                cloud,
                artifact,
                &artifact.storage_node,
                "materialized",
                Some(&artifact.sha256),
            );
            Ok(())
        }
        Err(error) => {
            record(cloud, artifact, &artifact.storage_node, "failed", None);
            Err(error)
        }
    }
}

fn locally_verified(store: &std::path::Path, artifact: &DevHubRuntimeArtifact) -> bool {
    hive_backend::reopen_sealed_runtime_artifact(store, &artifact.package)
        .and_then(|sealed| sealed.verified_package().map(|_| ()))
        .is_ok()
}

fn record(
    cloud: &CloudState,
    artifact: &DevHubRuntimeArtifact,
    source_node: &str,
    result: &str,
    verified_digest: Option<&str>,
) {
    cloud
        .marketplace_releases
        .record_artifact_materialization(ArtifactMaterializationEvidence {
            requested_digest: artifact.sha256.clone(),
            source_node: source_node.into(),
            target_node: cloud.node_name.clone(),
            result: result.into(),
            verified_digest: verified_digest.map(str::to_owned),
            timestamp_ms: hive_core::now_ms(),
        });
    crate::persist::persist(cloud);
}

/// Serve one bounded raw package range only to a verified, trusted mesh node.
/// The caller cannot choose a path or storage URL: digest and descriptor stay
/// catalog-authoritative and every response is capped before allocation.
pub async fn mesh_dispatch(
    cloud: &Arc<CloudState>,
    digest: &str,
    signer: Option<&str>,
    token: Option<&str>,
    offset: u64,
    length: usize,
) -> Vec<u8> {
    let Some(signer) = signer else {
        return Vec::new();
    };
    let Some(claims) = token.and_then(|token| crate::auth::verify(token).ok()) else {
        return Vec::new();
    };
    let caller = cloud.registry.nodes().into_iter().find(|node| {
        node.peer_id.as_deref() == Some(signer) && claims.sub == format!("mesh-node:{}", node.name)
    });
    if claims.role != "service"
        || claims.tenant != "system"
        || caller.is_none()
        || !cloud
            .trusted_peer_ids
            .read()
            .map(|ids| ids.contains(signer))
            .unwrap_or(false)
        || length == 0
        || length > CHUNK_BYTES
    {
        return Vec::new();
    }
    let Some(artifact) = cloud.marketplace_releases.runtime_artifact(digest) else {
        return Vec::new();
    };
    if artifact.storage_node != cloud.node_name
        || artifact.revoked
        || artifact.approval_state != "approved"
        || !artifact.security_validated
        || !artifact.policy_validated
        || offset.checked_add(length as u64).is_none()
        || offset + length as u64 > artifact.package.package_bytes
    {
        return Vec::new();
    }
    let store = crate::persist::data_dir().join("runtime-artifacts-v1");
    let Ok(sealed) = hive_backend::reopen_sealed_runtime_artifact(&store, &artifact.package) else {
        return Vec::new();
    };
    let Ok(package) = sealed.verified_package() else {
        return Vec::new();
    };
    let (file, descriptor) = package.into_parts();
    if descriptor.package_sha256 != artifact.sha256 {
        return Vec::new();
    }
    let mut output = vec![0u8; length];
    match file.read_at(&mut output, offset) {
        Ok(read) if read == length => output,
        _ => Vec::new(),
    }
}
