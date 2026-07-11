//! Single-file backup and restore for the libSQL database.
//!
//! Backup uses `VACUUM INTO`, which writes a fully-checkpointed, consistent
//! snapshot to a brand-new file with no `-wal`/`-shm` sidecars, regardless of
//! the live database's journal mode. Restore validates the snapshot, takes a
//! safety backup of the current database, swaps the file into place atomically,
//! and then deletes any stale WAL sidecars — left behind they would be replayed
//! onto the restored file and corrupt it.
//!
//! Hardening: both operations hold an exclusive lock file so a concurrent
//! backup/restore (or a running MCP server doing the same) can't race; snapshots
//! are written `0600` (dir `0700`) since the graph may hold secrets; every
//! snapshot is `PRAGMA integrity_check`-verified before it is trusted; and the
//! default backup directory is pruned to the most recent `keep` snapshots.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use tracing::{info, warn};
use turso::{Builder, Connection, Database};

use crate::constant::{
    ENV_DATABASE_URL, SQL_INTEGRITY_CHECK, SQL_TABLE_EXISTS, SQL_VACUUM_INTO_TEMPLATE,
};
use crate::paths::AsobiPaths;

/// Tables a valid Asobi snapshot must contain.
const REQUIRED_TABLES: [&str; 2] = ["asobi_entities", "topics"];
/// Prefix for snapshots produced by the `backup` command (pruned by retention).
const BACKUP_PREFIX: &str = "asobi";
/// Prefix for the safety snapshot taken before a restore (kept, not pruned).
const PRE_RESTORE_PREFIX: &str = "pre-restore";

/// The live database file the CLI operates on: `ASOBI_DATABASE_URL` if set,
/// otherwise the resolved workspace `db_path()`. Backup and restore both go
/// through this so they always target the same file as [`crate::db::init_db`].
pub fn effective_db_path() -> PathBuf {
    std::env::var(ENV_DATABASE_URL)
        .map(PathBuf::from)
        .unwrap_or_else(|_| AsobiPaths::resolve().db_path())
}

/// Directory holding managed snapshots, co-located with the live DB so backups
/// stay next to the data they snapshot even under `ASOBI_DATABASE_URL`.
fn backups_dir() -> PathBuf {
    effective_db_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("backups")
}

/// `<backups_dir>/<prefix>-YYYYMMDD-HHMMSSmmm.db`. Millisecond precision keeps
/// two snapshots taken in the same second from colliding.
fn default_backup_path(prefix: &str) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%3f");
    backups_dir().join(format!("{prefix}-{stamp}.db"))
}

/// Snapshot the live database to `output` (or a timestamped default in the
/// managed directory) via `VACUUM INTO`, verify its integrity, and — when using
/// the default location — prune to the newest `keep` snapshots. Returns the path
/// written. Refuses to overwrite an existing file.
pub async fn backup(conn: &Connection, output: Option<PathBuf>, keep: usize) -> Result<PathBuf> {
    let _lock = OperationLock::acquire(&effective_db_path())?;
    let is_default = output.is_none();
    let dest = backup_to(conn, output).await?;
    if is_default {
        prune_backups(BACKUP_PREFIX, keep);
    }
    Ok(dest)
}

/// Core snapshot logic without locking or retention, so [`restore`] can reuse it
/// for the pre-restore safety backup while already holding the lock.
async fn backup_to(conn: &Connection, output: Option<PathBuf>) -> Result<PathBuf> {
    let dest = output.unwrap_or_else(|| default_backup_path(BACKUP_PREFIX));
    if dest.exists() {
        bail!("backup target already exists: {}", dest.display());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating backup directory {}", parent.display()))?;
        restrict_permissions(parent, 0o700)?;
    }

    let dest_str = dest.to_str().context("backup path is not valid UTF-8")?;
    let sql = SQL_VACUUM_INTO_TEMPLATE.replace("{}", &dest_str.replace('\'', "''"));
    conn.execute(&sql, ())
        .await
        .with_context(|| format!("vacuuming into {}", dest.display()))?;
    restrict_permissions(&dest, 0o600)?;

    assert_integrity(&dest)
        .await
        .with_context(|| format!("verifying snapshot {}", dest.display()))?;

    Ok(dest)
}

/// Replace the live database with `source`. Consumes the open `db`/`conn` so the
/// connection is fully closed (and the WAL flushed) before the file is swapped.
pub async fn restore(db: Database, conn: Connection, source: &Path, force: bool) -> Result<()> {
    let db_path = effective_db_path();
    let _lock = OperationLock::acquire(&db_path)?;

    if !source.is_file() {
        bail!("restore source not found: {}", source.display());
    }
    validate_snapshot(source)
        .await
        .with_context(|| format!("{} is not a valid Asobi database", source.display()))?;

    if !force && !confirm_restore(&db_path, source)? {
        info!("Restore aborted.");
        return Ok(());
    }

    // Safety snapshot of the current database before we overwrite it.
    let safety = backup_to(&conn, Some(default_backup_path(PRE_RESTORE_PREFIX)))
        .await
        .context("creating pre-restore safety backup")?;
    info!("Saved pre-restore backup to {}", safety.display());

    // Close the live database so SQLite flushes and releases the WAL/SHM files.
    drop(conn);
    drop(db);

    // Stage into a temp file in the same directory, then atomically rename over
    // the live DB so a crash mid-copy can never leave a truncated primary.
    let tmp = db_path.with_extension("restore-tmp");
    std::fs::copy(source, &tmp)
        .with_context(|| format!("staging snapshot at {}", tmp.display()))?;
    restrict_permissions(&tmp, 0o600)?;
    std::fs::rename(&tmp, &db_path)
        .with_context(|| format!("swapping restored DB into {}", db_path.display()))?;
    remove_sidecars(&db_path)?;

    info!("Restored {} from {}", db_path.display(), source.display());
    Ok(())
}

/// Open `source` and confirm it carries the expected Asobi schema and passes
/// an integrity check. Rejects a wrong-file or corrupt-file mistake before any
/// destructive step.
async fn validate_snapshot(source: &Path) -> Result<()> {
    let source = source
        .to_str()
        .context("snapshot path is not valid UTF-8")?;
    let db = Builder::new_local(source).build().await?;
    let conn = db.connect()?;
    for table in REQUIRED_TABLES {
        let mut rows = conn.query(SQL_TABLE_EXISTS, turso::params![table]).await?;
        if rows.next().await?.is_none() {
            bail!("missing expected table `{table}`");
        }
    }
    check_integrity(&conn).await?;
    Ok(())
}

/// Run `PRAGMA integrity_check` against the database at `path`.
async fn assert_integrity(path: &Path) -> Result<()> {
    let path = path.to_str().context("database path is not valid UTF-8")?;
    let db = Builder::new_local(path).build().await?;
    let conn = db.connect()?;
    check_integrity(&conn).await
}

/// `PRAGMA integrity_check` returns a single `ok` row when the database is sound,
/// or one row per problem otherwise.
///
/// libSQL's vector extension keeps an internal `libsql_vector_meta_shadow` table
/// whose physical row order `VACUUM INTO` does not preserve, so `integrity_check`
/// emits a benign `row not in PRIMARY KEY order for libsql_vector_meta_shadow`
/// notice on every snapshot. That shadow table is a rebuildable index, not graph
/// data, so we tolerate notices that reference it and fail only on anything else.
async fn check_integrity(conn: &Connection) -> Result<()> {
    let mut rows = conn.query(SQL_INTEGRITY_CHECK, ()).await?;
    let mut problems = Vec::new();
    while let Some(row) = rows.next().await? {
        let line: String = row.get(0)?;
        if line == "ok" || line.contains("libsql_vector") {
            continue;
        }
        problems.push(line);
    }
    if !problems.is_empty() {
        bail!("integrity check failed: {}", problems.join("; "));
    }
    Ok(())
}

/// Delete all but the newest `keep` snapshots with `prefix` in the managed
/// directory. Best-effort: retention failures are logged, never fatal — a failed
/// prune must not fail the backup that just succeeded.
fn prune_backups(prefix: &str, keep: usize) {
    let dir = backups_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let needle = format!("{prefix}-");
    let mut snaps: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&needle) && n.ends_with(".db"))
        })
        .collect();
    if snaps.len() <= keep {
        return;
    }
    // Timestamp format sorts lexicographically == chronologically.
    snaps.sort();
    for old in &snaps[..snaps.len() - keep] {
        if let Err(e) = std::fs::remove_file(old) {
            warn!("could not prune old backup {}: {e}", old.display());
        }
    }
}

fn confirm_restore(db_path: &Path, source: &Path) -> Result<bool> {
    use std::io::Write;
    print!(
        "Replace the live database at {} with {}? [y/N]: ",
        db_path.display(),
        source.display()
    );
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

/// Delete `<db>-wal` and `<db>-shm` if present.
fn remove_sidecars(db_path: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm"] {
        let mut raw = db_path.as_os_str().to_owned();
        raw.push(suffix);
        let sidecar = PathBuf::from(raw);
        if sidecar.exists() {
            std::fs::remove_file(&sidecar)
                .with_context(|| format!("removing stale sidecar {}", sidecar.display()))?;
        }
    }
    Ok(())
}

/// Tighten file/dir permissions on Unix; a no-op elsewhere.
#[cfg(unix)]
pub fn restrict_permissions(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("setting permissions on {}", path.display()))
}

#[cfg(not(unix))]
pub fn restrict_permissions(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

/// Exclusive advisory lock guarding a backup/restore against concurrent runs.
/// Created with `O_EXCL`; removed on drop. A crash leaves a stale lock that the
/// next run reports with a path so it can be cleared manually.
struct OperationLock(PathBuf);

impl OperationLock {
    fn acquire(db_path: &Path) -> Result<Self> {
        let mut raw = db_path.as_os_str().to_owned();
        raw.push(".lock");
        let lock_path = PathBuf::from(raw);
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(_) => Ok(Self(lock_path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => bail!(
                "another backup/restore is in progress (lock: {}). \
                 If no asobi process is running, delete that file and retry.",
                lock_path.display()
            ),
            Err(e) => Err(e).with_context(|| format!("creating lock file {}", lock_path.display())),
        }
    }
}

impl Drop for OperationLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntityInput;
    use tempfile::tempdir;

    fn set_db(path: &Path) {
        unsafe { std::env::set_var(ENV_DATABASE_URL, path.to_str().unwrap()) };
    }

    async fn seed(conn: &Connection, name: &str) {
        crate::db::create_entities(
            conn,
            vec![EntityInput {
                name: name.to_string(),
                entity_type: "project".to_string(),
                observations: vec!["obs".to_string()],
            }],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn backup_then_restore_roundtrip() {
        let dir = tempdir().unwrap();
        let db_file = dir.path().join("rt.db");
        let snap = dir.path().join("snap.db");
        set_db(&db_file);

        let (db, conn) = crate::db::init_db().await.unwrap();
        seed(&conn, "alpha").await;

        backup(&conn, Some(snap.clone()), 3).await.unwrap();
        assert!(snap.exists());

        // Mutate after the backup so a successful restore is observable.
        crate::db::reset(&conn).await.unwrap();

        restore(db, conn, &snap, true).await.unwrap();

        let (_db, conn) = crate::db::init_db().await.unwrap();
        let graph = crate::db::open_nodes(&conn, vec!["alpha".to_string()])
            .await
            .unwrap();
        assert_eq!(graph.entities.len(), 1, "restore did not bring back alpha");
        assert_eq!(graph.entities[0].name, "alpha");
    }

    #[tokio::test]
    async fn backup_refuses_existing_output() {
        let dir = tempdir().unwrap();
        let db_file = dir.path().join("b.db");
        let snap = dir.path().join("snap.db");
        set_db(&db_file);

        let (_db, conn) = crate::db::init_db().await.unwrap();
        backup(&conn, Some(snap.clone()), 3).await.unwrap();

        let err = backup(&conn, Some(snap), 3).await.unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn restore_rejects_non_asobi_file() {
        let dir = tempdir().unwrap();
        let db_file = dir.path().join("r.db");
        let bogus = dir.path().join("bogus.db");
        std::fs::write(&bogus, b"not a database").unwrap();
        set_db(&db_file);

        let (db, conn) = crate::db::init_db().await.unwrap();
        let err = restore(db, conn, &bogus, true).await.unwrap_err();
        assert!(err.to_string().contains("not a valid Asobi database"));
    }

    #[tokio::test]
    async fn restore_clears_stale_wal_sidecars() {
        let dir = tempdir().unwrap();
        let db_file = dir.path().join("w.db");
        let snap = dir.path().join("snap.db");
        set_db(&db_file);

        let (db, conn) = crate::db::init_db().await.unwrap();
        seed(&conn, "alpha").await;
        backup(&conn, Some(snap.clone()), 3).await.unwrap();

        // Plant stale sidecars that must not survive the restore.
        let wal = PathBuf::from(format!("{}-wal", db_file.display()));
        let shm = PathBuf::from(format!("{}-shm", db_file.display()));
        std::fs::write(&wal, b"stale").unwrap();
        std::fs::write(&shm, b"stale").unwrap();

        restore(db, conn, &snap, true).await.unwrap();

        assert!(!wal.exists(), "stale -wal sidecar survived restore");
        assert!(!shm.exists(), "stale -shm sidecar survived restore");
    }

    #[tokio::test]
    async fn default_backup_prunes_to_keep() {
        let dir = tempdir().unwrap();
        let db_file = dir.path().join("p.db");
        set_db(&db_file);

        // Pre-populate the managed dir with old snapshots, then prune to 2.
        let bdir = backups_dir();
        std::fs::create_dir_all(&bdir).unwrap();
        for name in ["asobi-20200101-000000000.db", "asobi-20200102-000000000.db"] {
            std::fs::write(bdir.join(name), b"old").unwrap();
        }

        let (_db, conn) = crate::db::init_db().await.unwrap();
        // Default-location backup (output None) triggers retention.
        backup(&conn, None, 2).await.unwrap();

        let mut remaining: Vec<String> = std::fs::read_dir(&bdir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.starts_with("asobi-"))
            .collect();
        remaining.sort();
        assert_eq!(remaining.len(), 2, "retention should keep exactly 2");
        // The freshly written snapshot (newest) must be among the survivors.
        assert!(
            remaining
                .iter()
                .any(|n| n.as_str() > "asobi-20200102-000000000.db")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn backup_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let db_file = dir.path().join("perm.db");
        let snap = dir.path().join("snap.db");
        set_db(&db_file);

        let (_db, conn) = crate::db::init_db().await.unwrap();
        backup(&conn, Some(snap.clone()), 3).await.unwrap();

        let mode = std::fs::metadata(&snap).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "snapshot must be owner-only");
    }

    #[test]
    fn lock_is_exclusive_and_released() {
        let dir = tempdir().unwrap();
        let db_file = dir.path().join("lock.db");

        let guard = OperationLock::acquire(&db_file).unwrap();
        assert!(
            OperationLock::acquire(&db_file).is_err(),
            "second lock should be refused"
        );
        drop(guard);
        assert!(
            OperationLock::acquire(&db_file).is_ok(),
            "lock should be re-acquirable after release"
        );
    }
}
