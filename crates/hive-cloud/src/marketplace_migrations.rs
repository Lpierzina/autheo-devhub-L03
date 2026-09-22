//! Marketplace migration facts and readiness checks.
//!
//! SQL is executed only by the BuildExecutor migration surface.  This module
//! deliberately owns no database client and never serializes a connection
//! string: it establishes immutable expected-file facts and the durable,
//! replicated facts that the executor records after a successful isolated run.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use crate::{marketplace_releases::MarketplaceMigrationFact, state::CloudState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExpectedMigration {
    pub version: String,
    pub name: String,
    pub content_sha256: String,
    pub path: PathBuf,
}

/// One runner per immutable project/database identity in this process.  The
/// guard removes itself in Drop, including request cancellation, so a dropped
/// deploy cannot permanently wedge subsequent readiness attempts.
pub(crate) struct MigrationRunGuard {
    key: String,
}

static RUNNERS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();

impl MigrationRunGuard {
    pub(crate) fn acquire(project: &str, database_id: &str) -> Result<Self, &'static str> {
        let key = format!("{project}\u{0}{database_id}");
        let runners = RUNNERS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut runners = runners.lock();
        if runners.contains_key(&key) {
            return Err("marketplace_migration_in_progress");
        }
        runners.insert(key.clone(), 1);
        Ok(Self { key })
    }
}

impl Drop for MigrationRunGuard {
    fn drop(&mut self) {
        if let Some(runners) = RUNNERS.get() {
            runners.lock().remove(&self.key);
        }
    }
}

fn migration_filename(name: &str) -> Option<(String, String)> {
    let stem = name.strip_suffix(".sql")?;
    let (version, migration_name) = stem.split_once('_')?;
    if version.is_empty()
        || migration_name.is_empty()
        || version.len() > 128
        || migration_name.len() > 256
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || !migration_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }
    Some((version.to_owned(), migration_name.to_owned()))
}

/// Read only real, direct migration files, then sort by their complete
/// filename.  Version/name ambiguity and duplicate versions fail closed rather
/// than relying on directory enumeration order.
pub(crate) fn expected(checkout: &Path) -> Result<Vec<ExpectedMigration>, &'static str> {
    let root = checkout.join("db").join("migrations");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("marketplace_migrations_unavailable"),
    };
    let mut output = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| "marketplace_migrations_unavailable")?;
        let metadata = entry
            .metadata()
            .map_err(|_| "marketplace_migrations_unavailable")?;
        if !metadata.is_file() {
            continue;
        }
        let filename = entry.file_name();
        let filename = filename
            .to_str()
            .ok_or("marketplace_migration_invalid_filename")?;
        let Some((version, name)) = migration_filename(filename) else {
            return Err("marketplace_migration_invalid_filename");
        };
        let bytes =
            std::fs::read(entry.path()).map_err(|_| "marketplace_migrations_unavailable")?;
        output.push(ExpectedMigration {
            version,
            name,
            content_sha256: hex::encode(Sha256::digest(bytes)),
            path: entry.path(),
        });
    }
    output.sort_by(|left, right| {
        (left.version.as_str(), left.name.as_str())
            .cmp(&(right.version.as_str(), right.name.as_str()))
    });
    if output
        .windows(2)
        .any(|pair| pair[0].version == pair[1].version)
    {
        return Err("marketplace_migration_duplicate_version");
    }
    Ok(output)
}

/// Compare the exact expected migration set with durable facts. This never
/// accepts a changed historical file and never treats a fact for another
/// project/database as evidence of readiness.
pub(crate) fn readiness(
    cloud: &Arc<CloudState>,
    project: &str,
    database_id: &str,
    expected: &[ExpectedMigration],
) -> Result<(), &'static str> {
    let facts = cloud
        .marketplace_releases
        .migration_facts(project, database_id);
    let by_version: BTreeMap<_, _> = facts
        .iter()
        .map(|fact| (fact.version.as_str(), fact))
        .collect();
    for migration in expected {
        let Some(fact) = by_version.get(migration.version.as_str()) else {
            return Err("marketplace_migration_pending");
        };
        if fact.name != migration.name || fact.content_sha256 != migration.content_sha256 {
            return Err("marketplace_migration_digest_mismatch");
        }
    }
    if facts.len() != expected.len() {
        return Err("marketplace_migration_unexpected_fact");
    }
    Ok(())
}

pub(crate) fn record(
    cloud: &Arc<CloudState>,
    project: &str,
    database_id: &str,
    migration: &ExpectedMigration,
) -> Result<(), &'static str> {
    cloud
        .marketplace_releases
        .record_migration(MarketplaceMigrationFact {
            project_id: project.to_owned(),
            database_id: database_id.to_owned(),
            version: migration.version.clone(),
            name: migration.name.clone(),
            content_sha256: migration.content_sha256.clone(),
            applied_ms: hive_core::now_ms(),
        })?;
    crate::persist::persist(cloud);
    Ok(())
}

/// Run missing migrations in deterministic order inside the dedicated
/// BuildExecutor migration sandbox. No migration command is ever spawned on
/// the controller host or in the public workload process.
pub(crate) async fn run(
    cloud: &Arc<CloudState>,
    project: &str,
    checkout: &Path,
) -> Result<(), &'static str> {
    let workload = cloud
        .marketplace_releases
        .workload_for_project(project)
        .ok_or("marketplace_workload_unattached")?;
    let release = cloud
        .marketplace_releases
        .release(&workload.release_id)
        .ok_or("marketplace_release_unavailable")?;
    if !workload.client_certificate_delivery_requested
        || !release
            .workload_client_certificate
            .as_ref()
            .is_some_and(crate::marketplace_releases::WorkloadClientCertificateCapability::files_v1)
    {
        return Err("marketplace_workload_client_certificate_unsupported");
    }
    let database_id = cloud
        .marketplace_releases
        .managed_postgres(project)
        .ok_or("marketplace_managed_database_unavailable")?;
    let database = cloud
        .databases
        .get_raw(&database_id)
        .filter(|database| {
            database.kind == crate::databases::DbKind::Postgres
                && matches!(database.status, crate::databases::DbStatus::Ready)
                && database.mode == "live"
        })
        .ok_or("marketplace_managed_database_unavailable")?;
    let target = database
        .connection
        .get("net_host")
        .and_then(|value| {
            value.parse::<std::net::Ipv4Addr>().ok().filter(|address| {
                address.to_string() == *value
                    && !address.is_unspecified()
                    && !address.is_loopback()
                    && !address.is_link_local()
                    && !address.is_multicast()
                    && *address != std::net::Ipv4Addr::BROADCAST
            })
        })
        .ok_or("marketplace_migration_network_unavailable")?;
    let database_url = database
        .connection
        .get("DATABASE_URL")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or("marketplace_migration_credential_unavailable")?;
    let expected = expected(checkout)?;
    let _guard = MigrationRunGuard::acquire(project, &database_id)?;
    let facts = cloud
        .marketplace_releases
        .migration_facts(project, &database_id);
    let existing: BTreeMap<_, _> = facts
        .iter()
        .map(|fact| (fact.version.as_str(), fact))
        .collect();
    let executor =
        crate::build_executor::get().map_err(|_| "marketplace_migration_isolation_unavailable")?;
    let mut session = executor
        .begin_migration(
            crate::build_executor::BuildRequest {
                checkout: checkout.to_path_buf(),
                surface: crate::build_executor::BuildSurface::RepositoryCommands,
            },
            crate::build_executor::MigrationNetworkTarget {
                ipv4: target,
                port: 5432,
            },
        )
        .await
        .map_err(|_| "marketplace_migration_isolation_unavailable")?;
    for migration in &expected {
        if let Some(fact) = existing.get(migration.version.as_str()) {
            if fact.name != migration.name || fact.content_sha256 != migration.content_sha256 {
                return Err("marketplace_migration_digest_mismatch");
            }
            continue;
        }
        let filename = migration
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("marketplace_migration_invalid_filename")?;
        let relative = format!("db/migrations/{filename}");
        session
            .run_marketplace_migration(&relative, database_url.clone(), |_| {})
            .await
            .map_err(|_| "marketplace_migration_failed")?;
        record(cloud, project, &database_id, migration)?;
    }
    readiness(cloud, project, &database_id, &expected)
}
