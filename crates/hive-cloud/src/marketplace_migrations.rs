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
