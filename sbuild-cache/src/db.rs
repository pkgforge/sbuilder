//! Database operations for the build cache

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

use crate::error::{Error, Result};
use crate::models::*;
use crate::schema::{CREATE_SCHEMA, CREATE_VIEWS, SCHEMA_VERSION};

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

    /// Initialize the database schema
    fn initialize(&self) -> Result<()> {
        // Check if we need to create the schema
        let needs_init: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_info'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count == 0)?;

        if needs_init {
            self.conn.execute_batch(CREATE_SCHEMA)?;
            self.conn.execute_batch(CREATE_VIEWS)?;
            self.conn.execute(
                "INSERT INTO schema_info (version, description) VALUES (?1, ?2)",
                params![SCHEMA_VERSION, "Initial schema"],
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
                        last_build_date, last_build_id, last_build_status, ghcr_tag,
                        created_at, updated_at
                 FROM packages WHERE pkg_id = ?1 AND host_triplet = ?2",
                params![pkg_id, host_triplet],
                |row| {
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
                        last_build_date: row
                            .get::<_, Option<String>>(11)?
                            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                            .map(|dt| dt.with_timezone(&Utc)),
                        last_build_id: row.get(12)?,
                        last_build_status: row
                            .get::<_, Option<String>>(13)?
                            .and_then(|s| BuildStatus::from_str(&s)),
                        ghcr_tag: row.get(14)?,
                        created_at: row
                            .get::<_, String>(15)
                            .ok()
                            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(Utc::now),
                        updated_at: row
                            .get::<_, String>(16)
                            .ok()
                            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(Utc::now),
                    })
                },
            )
            .optional()?;

        Ok(result)
    }

    /// Update package after a build
    pub fn update_build_result(
        &self,
        pkg_id: &str,
        host_triplet: &str,
        version: &str,
        status: BuildStatus,
        build_id: &str,
        ghcr_tag: Option<&str>,
        recipe_hash: Option<&str>,
    ) -> Result<()> {
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
                is_outdated = 0,
                updated_at = ?7
             WHERE pkg_id = ?8 AND host_triplet = ?9",
            params![version, now, status_str, build_id, ghcr_tag, recipe_hash, now, pkg_id, host_triplet],
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
                    last_build_date, last_build_id, last_build_status, ghcr_tag,
                    created_at, updated_at
             FROM packages
             WHERE host_triplet = ?1
               AND (is_outdated = 1 OR last_build_status IS NULL OR last_build_status = 'pending')
             ORDER BY pkg_name",
        )?;

        let rows = stmt.query_map(params![host_triplet], |row| {
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
                last_build_date: row
                    .get::<_, Option<String>>(11)?
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
                last_build_id: row.get(12)?,
                last_build_status: row
                    .get::<_, Option<String>>(13)?
                    .and_then(|s| BuildStatus::from_str(&s)),
                ghcr_tag: row.get(14)?,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
        })?;

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
        let record = self.get_package(pkg_id, host_triplet)?
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
        let base_query = "SELECT id, pkg_id, pkg_name, pkg_family, build_script, ghcr_pkg, host_triplet,
                    current_version, upstream_version, is_outdated, recipe_hash,
                    last_build_date, last_build_id, last_build_status, ghcr_tag,
                    created_at, updated_at
             FROM packages
             WHERE host_triplet = ?1";

        let query = match (&status_filter, include_outdated) {
            (Some(_), true) => format!(
                "{} AND (last_build_status = ?2 OR is_outdated = 1) ORDER BY pkg_name",
                base_query
            ),
            (Some(_), false) => format!("{} AND last_build_status = ?2 ORDER BY pkg_name", base_query),
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
    pub fn get_recent_builds(&self, host_triplet: &str, limit: i64) -> Result<Vec<(PackageRecord, BuildHistoryEntry)>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.pkg_id, p.pkg_name, p.pkg_family, p.build_script, p.ghcr_pkg, p.host_triplet,
                    p.current_version, p.upstream_version, p.is_outdated, p.recipe_hash,
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
                id: Some(row.get(17)?),
                package_id: pkg.id.unwrap_or(0),
                build_id: row.get(18)?,
                version: row.get(19)?,
                build_date: row
                    .get::<_, String>(20)
                    .ok()
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now),
                build_status: row
                    .get::<_, String>(21)
                    .ok()
                    .and_then(|s| BuildStatus::from_str(&s))
                    .unwrap_or(BuildStatus::Pending),
                duration_seconds: row.get(22).ok(),
                artifact_size_bytes: None,
                ghcr_tag: row.get(23).ok(),
                ghcr_digest: None,
                build_log_url: None,
                error_message: row.get(24).ok(),
            };
            Ok((pkg, history))
        })?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Sqlite)
    }

    /// Helper to convert row to PackageRecord
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
            last_build_date: row
                .get::<_, Option<String>>(11)?
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            last_build_id: row.get(12)?,
            last_build_status: row
                .get::<_, Option<String>>(13)?
                .and_then(|s| BuildStatus::from_str(&s)),
            ghcr_tag: row.get(14)?,
            created_at: row
                .get::<_, String>(15)
                .ok()
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(Utc::now),
            updated_at: row
                .get::<_, String>(16)
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
        let stats = db.get_stats("x86_64-Linux").unwrap();
        assert_eq!(stats.total_packages, 0);
    }

    #[test]
    fn test_package_crud() {
        let db = CacheDatabase::in_memory().unwrap();

        // Create
        let record = db
            .get_or_create_package("github.com.test.pkg", "testpkg", "x86_64-Linux")
            .unwrap();
        assert_eq!(record.pkg_name, "testpkg");

        // Read
        let found = db
            .get_package("github.com.test.pkg", "x86_64-Linux")
            .unwrap();
        assert!(found.is_some());

        // Update
        db.update_build_result(
            "github.com.test.pkg",
            "x86_64-Linux",
            "1.0.0",
            BuildStatus::Success,
            "build-123",
            Some("v1.0.0-x86_64-Linux"),
            Some("abc123"),
        )
        .unwrap();

        let updated = db
            .get_package("github.com.test.pkg", "x86_64-Linux")
            .unwrap()
            .unwrap();
        assert_eq!(updated.current_version, Some("1.0.0".to_string()));
        assert_eq!(updated.last_build_status, Some(BuildStatus::Success));
    }

    #[test]
    fn test_stats() {
        let db = CacheDatabase::in_memory().unwrap();

        db.get_or_create_package("pkg1", "pkg1", "x86_64-Linux")
            .unwrap();
        db.get_or_create_package("pkg2", "pkg2", "x86_64-Linux")
            .unwrap();

        db.update_build_result(
            "pkg1",
            "x86_64-Linux",
            "1.0",
            BuildStatus::Success,
            "b1",
            None,
            None,
        )
        .unwrap();

        let stats = db.get_stats("x86_64-Linux").unwrap();
        assert_eq!(stats.total_packages, 2);
        assert_eq!(stats.successful, 1);
    }
}
