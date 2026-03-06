//! Database operations for the build cache

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

use crate::error::{Error, Result};
use crate::models::*;
use crate::schema::{CREATE_SCHEMA, CREATE_VIEWS, MIGRATE_V1_TO_V2, MIGRATE_V2_TO_V3, SCHEMA_VERSION};

/// SQLite cache database
pub struct CacheDatabase {
    conn: Connection,
}

impl CacheDatabase {
    /// Open or create a cache database
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.initialize()?;
        Ok(db)
    }

    /// Create an in-memory database (for testing)
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.initialize()?;
        Ok(db)
    }

    /// Initialize the database schema, running migrations if needed
    fn initialize(&self) -> Result<()> {
        let has_schema: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_info'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)?;

        if !has_schema {
            // Fresh database: create schema at current version
            self.conn.execute_batch(CREATE_SCHEMA)?;
            self.conn.execute_batch(CREATE_VIEWS)?;
            self.conn.execute(
                "INSERT INTO schema_info (version, description) VALUES (?1, ?2)",
                params![SCHEMA_VERSION, "Initial schema"],
            )?;
            return Ok(());
        }

        // Existing database: check version and migrate if needed
        let current_version: i32 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(version), 1) FROM schema_info",
                [],
                |row| row.get(0),
            )
            .unwrap_or(1);

        if current_version < 2 {
            // Migrate v1 -> v2: add revision tracking columns
            self.conn.execute_batch(MIGRATE_V1_TO_V2)?;
            self.conn.execute(
                "INSERT INTO schema_info (version, description) VALUES (?1, ?2)",
                params![2, "Add revision tracking columns"],
            )?;
        }

        if current_version < 3 {
            // Migrate v2 -> v3: add remote_version column
            self.conn.execute_batch(MIGRATE_V2_TO_V3)?;
            self.conn.execute(
                "INSERT INTO schema_info (version, description) VALUES (?1, ?2)",
                params![3, "Add remote_version column"],
            )?;
        }

        Ok(())
    }

    /// Get or create a package record
    pub fn get_or_create_package(
        &self,
        pkg_id: &str,
        pkg_name: &str,
        host_triplet: &str,
    ) -> Result<PackageRecord> {
        // Try to find existing
        if let Some(record) = self.get_package(pkg_id, host_triplet)? {
            return Ok(record);
        }

        // Create new
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO packages (pkg_id, pkg_name, host_triplet, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![pkg_id, pkg_name, host_triplet, now, now],
        )?;

        self.get_package(pkg_id, host_triplet)?
            .ok_or_else(|| Error::Other("Failed to create package".to_string()))
    }

    /// Get a package by ID and host
    pub fn get_package(&self, pkg_id: &str, host_triplet: &str) -> Result<Option<PackageRecord>> {
        let result = self
            .conn
            .query_row(
                "SELECT id, pkg_id, pkg_name, pkg_family, build_script, ghcr_pkg, host_triplet,
                        current_version, upstream_version, is_outdated, recipe_hash,
                        base_version, remote_version, revision,
                        last_build_date, last_build_id, last_build_status, ghcr_tag,
                        created_at, updated_at
                 FROM packages WHERE pkg_id = ?1 AND host_triplet = ?2",
                params![pkg_id, host_triplet],
                Self::row_to_package_record,
            )
            .optional()?;

        Ok(result)
    }

    /// Find packages by name (or pkg_id suffix) and host.
    /// Returns all matches since multiple packages can share the same name
    /// (e.g., coreutils from gnu, uutils, vlang).
    pub fn find_packages_by_name(
        &self,
        name: &str,
        host_triplet: &str,
    ) -> Result<Vec<PackageRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, pkg_id, pkg_name, pkg_family, build_script, ghcr_pkg, host_triplet,
                    current_version, upstream_version, is_outdated, recipe_hash,
                    base_version, remote_version, revision,
                    last_build_date, last_build_id, last_build_status, ghcr_tag,
                    created_at, updated_at
             FROM packages
             WHERE (pkg_name = ?1 OR pkg_id LIKE '%.' || ?1)
               AND host_triplet = ?2
             ORDER BY pkg_id",
        )?;

        let rows = stmt.query_map(params![name, host_triplet], Self::row_to_package_record)?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Sqlite)
    }

    /// Update package after a build
    pub fn update_build_result(
        &self,
        pkg_id: &str,
        host_triplet: &str,
        version: &str,
        status: BuildStatus,
        build_id: Option<&str>,
        ghcr_tag: Option<&str>,
        recipe_hash: Option<&str>,
        base_version: Option<&str>,
        remote_version: Option<&str>,
        revision: i32,
    ) -> Result<()> {
        // Input validation
        if version.is_empty() || version == "unknown" {
            return Err(Error::Other(format!(
                "Invalid version '{}': must not be empty or 'unknown'",
                version
            )));
        }

        let now = Utc::now().to_rfc3339();
        let status_str = status.as_str();

        self.conn.execute(
            "UPDATE packages SET
                current_version = ?1,
                last_build_date = ?2,
                last_build_status = ?3,
                last_build_id = ?4,
                ghcr_tag = ?5,
                recipe_hash = ?6,
                base_version = ?7,
                remote_version = ?8,
                revision = ?9,
                is_outdated = 0,
                updated_at = ?10
             WHERE pkg_id = ?11 AND host_triplet = ?12",
            params![
                version,
                now,
                status_str,
                build_id,
                ghcr_tag,
                recipe_hash,
                base_version,
                remote_version,
                revision,
                now,
                pkg_id,
                host_triplet
            ],
        )?;

        // Add to build history
        if let Some(record) = self.get_package(pkg_id, host_triplet)? {
            if let Some(id) = record.id {
                self.conn.execute(
                    "INSERT INTO build_history (package_id, build_id, version, build_date, build_status, ghcr_tag)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, build_id, version, now, status_str, ghcr_tag],
                )?;
            }
        }

        Ok(())
    }

    /// Get the next revision number for a package version
    pub fn get_revision(
        &self,
        pkg_id: &str,
        host_triplet: &str,
        base_version: &str,
        remote_version: Option<&str>,
        recipe_hash: Option<&str>,
    ) -> Result<i32> {
        let result = self
            .conn
            .query_row(
                "SELECT base_version, revision, recipe_hash, remote_version FROM packages
                 WHERE pkg_id = ?1 AND host_triplet = ?2",
                params![pkg_id, host_triplet],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, i32>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;

        match result {
            Some((Some(stored_base), stored_revision, stored_hash, stored_remote))
                if stored_base == base_version =>
            {
                // Same base version but different remote version → new upstream, reset
                if let (Some(new_remote), Some(old_remote)) =
                    (remote_version, stored_remote.as_deref())
                {
                    if new_remote != old_remote {
                        return Ok(0);
                    }
                }

                match (recipe_hash, stored_hash.as_deref()) {
                    // Both hashes known and differ: bump revision
                    (Some(new), Some(old)) if new != old => Ok(stored_revision + 1),
                    // Same hash or unknown: reuse current revision
                    _ => Ok(stored_revision),
                }
            }
            _ => {
                // Different base version or no record: start at 0
                Ok(0)
            }
        }
    }

    /// Update recipe hash for a package
    pub fn update_recipe_hash(&self, pkg_id: &str, host_triplet: &str, hash: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE packages SET recipe_hash = ?1, updated_at = ?2 WHERE pkg_id = ?3 AND host_triplet = ?4",
            params![hash, now, pkg_id, host_triplet],
        )?;
        Ok(())
    }

    /// Mark package as outdated
    pub fn mark_outdated(
        &self,
        pkg_id: &str,
        host_triplet: &str,
        upstream_version: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE packages SET is_outdated = 1, upstream_version = ?1, updated_at = ?2
             WHERE pkg_id = ?3 AND host_triplet = ?4",
            params![upstream_version, now, pkg_id, host_triplet],
        )?;
        Ok(())
    }

    /// Get packages needing rebuild for a host
    pub fn get_packages_needing_rebuild(&self, host_triplet: &str) -> Result<Vec<PackageRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, pkg_id, pkg_name, pkg_family, build_script, ghcr_pkg, host_triplet,
                    current_version, upstream_version, is_outdated, recipe_hash,
                    base_version, remote_version, revision,
                    last_build_date, last_build_id, last_build_status, ghcr_tag,
                    created_at, updated_at
             FROM packages
             WHERE host_triplet = ?1
               AND (is_outdated = 1 OR last_build_status IS NULL OR last_build_status = 'pending')
             ORDER BY pkg_name",
        )?;

        let rows = stmt.query_map(params![host_triplet], Self::row_to_package_record)?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Sqlite)
    }

    /// Get build statistics for a host
    pub fn get_stats(&self, host_triplet: &str) -> Result<BuildStats> {
        self.conn
            .query_row(
                "SELECT
                    COUNT(*) as total,
                    COALESCE(SUM(CASE WHEN last_build_status = 'success' THEN 1 ELSE 0 END), 0) as successful,
                    COALESCE(SUM(CASE WHEN last_build_status = 'failed' THEN 1 ELSE 0 END), 0) as failed,
                    COALESCE(SUM(CASE WHEN last_build_status = 'pending' OR last_build_status IS NULL THEN 1 ELSE 0 END), 0) as pending,
                    COALESCE(SUM(CASE WHEN is_outdated = 1 THEN 1 ELSE 0 END), 0) as outdated
                 FROM packages WHERE host_triplet = ?1",
                params![host_triplet],
                |row| {
                    Ok(BuildStats {
                        total_packages: row.get(0)?,
                        successful: row.get(1)?,
                        failed: row.get(2)?,
                        pending: row.get(3)?,
                        outdated: row.get(4)?,
                    })
                },
            )
            .map_err(Error::Sqlite)
    }

    /// Record a failed build with retry backoff
    pub fn record_failure(
        &self,
        pkg_id: &str,
        host_triplet: &str,
        error_message: &str,
    ) -> Result<()> {
        let record = self
            .get_package(pkg_id, host_triplet)?
            .ok_or_else(|| Error::PackageNotFound(pkg_id.to_string()))?;

        let package_id = record.id.unwrap();
        let now = Utc::now();

        // Get current failure count
        let failure_count: i32 = self
            .conn
            .query_row(
                "SELECT failure_count FROM failed_packages WHERE package_id = ?1",
                params![package_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let new_count = failure_count + 1;

        // Exponential backoff: 1h, 2h, 4h, 8h, max 24h
        let backoff_hours = std::cmp::min(1 << failure_count, 24);
        let next_retry = now + Duration::hours(backoff_hours as i64);

        self.conn.execute(
            "INSERT INTO failed_packages (package_id, failure_count, last_failure_date, last_error_message, next_retry_date)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(package_id) DO UPDATE SET
                failure_count = ?2,
                last_failure_date = ?3,
                last_error_message = ?4,
                next_retry_date = ?5",
            params![
                package_id,
                new_count,
                now.to_rfc3339(),
                error_message,
                next_retry.to_rfc3339()
            ],
        )?;

        Ok(())
    }

    /// Clear failure record after successful build
    pub fn clear_failure(&self, pkg_id: &str, host_triplet: &str) -> Result<()> {
        if let Some(record) = self.get_package(pkg_id, host_triplet)? {
            if let Some(id) = record.id {
                self.conn.execute(
                    "DELETE FROM failed_packages WHERE package_id = ?1",
                    params![id],
                )?;
            }
        }
        Ok(())
    }

    /// List all packages with optional status filter
    pub fn list_packages(
        &self,
        host_triplet: &str,
        status_filter: Option<BuildStatus>,
        include_outdated: bool,
    ) -> Result<Vec<PackageRecord>> {
        let base_query =
            "SELECT id, pkg_id, pkg_name, pkg_family, build_script, ghcr_pkg, host_triplet,
                    current_version, upstream_version, is_outdated, recipe_hash,
                    base_version, remote_version, revision,
                    last_build_date, last_build_id, last_build_status, ghcr_tag,
                    created_at, updated_at
             FROM packages
             WHERE host_triplet = ?1";

        let query = match (&status_filter, include_outdated) {
            (Some(_), true) => format!(
                "{} AND (last_build_status = ?2 OR is_outdated = 1) ORDER BY pkg_name",
                base_query
            ),
            (Some(_), false) => format!(
                "{} AND last_build_status = ?2 ORDER BY pkg_name",
                base_query
            ),
            (None, true) => format!("{} AND is_outdated = 1 ORDER BY pkg_name", base_query),
            (None, false) => format!("{} ORDER BY pkg_name", base_query),
        };

        let mut stmt = self.conn.prepare(&query)?;

        let mut results = Vec::new();

        if let Some(status) = status_filter {
            let mut rows = stmt.query(params![host_triplet, status.as_str()])?;
            while let Some(row) = rows.next()? {
                results.push(Self::row_to_package_record(row)?);
            }
        } else {
            let mut rows = stmt.query(params![host_triplet])?;
            while let Some(row) = rows.next()? {
                results.push(Self::row_to_package_record(row)?);
            }
        }

        Ok(results)
    }

    /// Get recent build history
    pub fn get_recent_builds(
        &self,
        host_triplet: &str,
        limit: i64,
    ) -> Result<Vec<(PackageRecord, BuildHistoryEntry)>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.pkg_id, p.pkg_name, p.pkg_family, p.build_script, p.ghcr_pkg, p.host_triplet,
                    p.current_version, p.upstream_version, p.is_outdated, p.recipe_hash,
                    p.base_version, p.remote_version, p.revision,
                    p.last_build_date, p.last_build_id, p.last_build_status, p.ghcr_tag,
                    p.created_at, p.updated_at,
                    bh.id, bh.build_id, bh.version, bh.build_date, bh.build_status,
                    bh.duration_seconds, bh.ghcr_tag, bh.error_message
             FROM packages p
             JOIN build_history bh ON p.id = bh.package_id
             WHERE p.host_triplet = ?1
             ORDER BY bh.build_date DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![host_triplet, limit], |row| {
            let pkg = Self::row_to_package_record(row)?;
            let history = BuildHistoryEntry {
                id: Some(row.get(20)?),
                package_id: pkg.id.unwrap_or(0),
                build_id: row.get::<_, Option<String>>(21)?.unwrap_or_default(),
                version: row.get(22)?,
                build_date: row
                    .get::<_, String>(23)
                    .ok()
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now),
                build_status: row
                    .get::<_, String>(24)
                    .ok()
                    .and_then(|s| BuildStatus::from_str(&s))
                    .unwrap_or(BuildStatus::Pending),
                duration_seconds: row.get(25).ok(),
                artifact_size_bytes: None,
                ghcr_tag: row.get(26).ok(),
                ghcr_digest: None,
                build_log_url: None,
                error_message: row.get(27).ok(),
            };
            Ok((pkg, history))
        })?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Sqlite)
    }

    /// Helper to convert row to PackageRecord
    ///
    /// Expected column order:
    ///   0: id, 1: pkg_id, 2: pkg_name, 3: pkg_family, 4: build_script,
    ///   5: ghcr_pkg, 6: host_triplet, 7: current_version, 8: upstream_version,
    ///   9: is_outdated, 10: recipe_hash, 11: base_version, 12: remote_version,
    ///   13: revision, 14: last_build_date, 15: last_build_id,
    ///   16: last_build_status, 17: ghcr_tag, 18: created_at, 19: updated_at
    fn row_to_package_record(row: &rusqlite::Row) -> rusqlite::Result<PackageRecord> {
        Ok(PackageRecord {
            id: Some(row.get(0)?),
            pkg_id: row.get(1)?,
            pkg_name: row.get(2)?,
            pkg_family: row.get(3)?,
            build_script: row.get(4)?,
            ghcr_pkg: row.get(5)?,
            host_triplet: row.get(6)?,
            current_version: row.get(7)?,
            upstream_version: row.get(8)?,
            is_outdated: row.get::<_, i32>(9)? != 0,
            recipe_hash: row.get(10)?,
            base_version: row.get(11)?,
            remote_version: row.get(12)?,
            revision: row.get::<_, Option<i32>>(13)?.unwrap_or(0),
            last_build_date: row
                .get::<_, Option<String>>(14)?
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            last_build_id: row.get(15)?,
            last_build_status: row
                .get::<_, Option<String>>(16)?
                .and_then(|s| BuildStatus::from_str(&s)),
            ghcr_tag: row.get(17)?,
            created_at: row
                .get::<_, String>(18)
                .ok()
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(Utc::now),
            updated_at: row
                .get::<_, String>(19)
                .ok()
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(Utc::now),
        })
    }

    /// Prune old build history entries
    pub fn prune_history(&self, keep_last: i64) -> Result<i64> {
        let result = self.conn.execute(
            "DELETE FROM build_history WHERE id NOT IN (
                SELECT id FROM build_history bh2
                WHERE bh2.package_id = build_history.package_id
                ORDER BY build_date DESC
                LIMIT ?1
             )",
            params![keep_last],
        )?;
        Ok(result as i64)
    }

    /// List all packages (no filters) - used for export
    pub fn list_all_packages(&self) -> Result<Vec<PackageRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, pkg_id, pkg_name, pkg_family, build_script, ghcr_pkg, host_triplet,
                    current_version, upstream_version, is_outdated, recipe_hash,
                    base_version, remote_version, revision,
                    last_build_date, last_build_id, last_build_status, ghcr_tag,
                    created_at, updated_at
             FROM packages ORDER BY pkg_id, host_triplet",
        )?;

        let rows = stmt.query_map([], Self::row_to_package_record)?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Sqlite)
    }

    /// Import a package record (used by export from MongoDB)
    pub fn import_package(&self, record: &PackageRecord) -> Result<()> {
        let created = record.created_at.to_rfc3339();
        let updated = record.updated_at.to_rfc3339();
        let build_date = record.last_build_date.map(|d| d.to_rfc3339());
        let status_str = record.last_build_status.map(|s| s.as_str().to_string());

        self.conn.execute(
            "INSERT OR REPLACE INTO packages (
                pkg_id, pkg_name, pkg_family, build_script, ghcr_pkg, host_triplet,
                current_version, upstream_version, is_outdated, recipe_hash,
                base_version, remote_version, revision,
                last_build_date, last_build_id, last_build_status, ghcr_tag,
                created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                record.pkg_id,
                record.pkg_name,
                record.pkg_family,
                record.build_script,
                record.ghcr_pkg,
                record.host_triplet,
                record.current_version,
                record.upstream_version,
                record.is_outdated as i32,
                record.recipe_hash,
                record.base_version,
                record.remote_version,
                record.revision,
                build_date,
                record.last_build_id,
                status_str,
                record.ghcr_tag,
                created,
                updated,
            ],
        )?;
        Ok(())
    }

    /// Import a build history entry (used by export from MongoDB)
    pub fn import_build_history(
        &self,
        pkg_id: &str,
        host_triplet: &str,
        entry: &BuildHistoryEntry,
    ) -> Result<()> {
        let record = self.get_package(pkg_id, host_triplet)?;
        let package_id = match record {
            Some(r) => {
                r.id.ok_or_else(|| Error::PackageNotFound(pkg_id.to_string()))?
            }
            None => return Err(Error::PackageNotFound(pkg_id.to_string())),
        };

        self.conn.execute(
            "INSERT INTO build_history (package_id, build_id, version, build_date, build_status, duration_seconds, ghcr_tag, error_message, build_log_url)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                package_id,
                entry.build_id,
                entry.version,
                entry.build_date.to_rfc3339(),
                entry.build_status.as_str(),
                entry.duration_seconds,
                entry.ghcr_tag,
                entry.error_message,
                entry.build_log_url,
            ],
        )?;
        Ok(())
    }

    /// Check if retry is allowed for a package
    pub fn is_retry_allowed(&self, pkg_id: &str, host_triplet: &str) -> Result<bool> {
        let record = self.get_package(pkg_id, host_triplet)?;

        if record.is_none() {
            return Ok(true); // New package, allow
        }

        let package_id = record.unwrap().id.unwrap();
        let now = Utc::now().to_rfc3339();

        let allowed: bool = self
            .conn
            .query_row(
                "SELECT next_retry_date IS NULL OR next_retry_date <= ?1
                 FROM failed_packages WHERE package_id = ?2",
                params![now, package_id],
                |row| row.get(0),
            )
            .unwrap_or(true);

        Ok(allowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_database() {
        let db = CacheDatabase::in_memory().unwrap();
        let stats = db.get_stats("x86_64-linux").unwrap();
        assert_eq!(stats.total_packages, 0);
    }

    #[test]
    fn test_package_crud() {
        let db = CacheDatabase::in_memory().unwrap();

        // Create
        let record = db
            .get_or_create_package("github.com.test.pkg", "testpkg", "x86_64-linux")
            .unwrap();
        assert_eq!(record.pkg_name, "testpkg");

        // Read
        let found = db
            .get_package("github.com.test.pkg", "x86_64-linux")
            .unwrap();
        assert!(found.is_some());

        // Update
        db.update_build_result(
            "github.com.test.pkg",
            "x86_64-linux",
            "1.0.0",
            BuildStatus::Success,
            Some("build-123"),
            Some("v1.0.0-x86_64-linux"),
            Some("abc123"),
            Some("1.0.0"),
            None,
            0,
        )
        .unwrap();

        let updated = db
            .get_package("github.com.test.pkg", "x86_64-linux")
            .unwrap()
            .unwrap();
        assert_eq!(updated.current_version, Some("1.0.0".to_string()));
        assert_eq!(updated.last_build_status, Some(BuildStatus::Success));
    }

    #[test]
    fn test_stats() {
        let db = CacheDatabase::in_memory().unwrap();

        db.get_or_create_package("pkg1", "pkg1", "x86_64-linux")
            .unwrap();
        db.get_or_create_package("pkg2", "pkg2", "x86_64-linux")
            .unwrap();

        db.update_build_result(
            "pkg1",
            "x86_64-linux",
            "1.0",
            BuildStatus::Success,
            Some("b1"),
            None,
            None,
            Some("1.0"),
            None,
            0,
        )
        .unwrap();

        let stats = db.get_stats("x86_64-linux").unwrap();
        assert_eq!(stats.total_packages, 2);
        assert_eq!(stats.successful, 1);
    }
}
