//! SQLite schema definitions

/// Current schema version
pub const SCHEMA_VERSION: i32 = 1;

/// SQL to create the database schema
pub const CREATE_SCHEMA: &str = r#"
-- Schema version tracking
CREATE TABLE IF NOT EXISTS schema_info (
    version INTEGER PRIMARY KEY,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    description TEXT
);

-- Core package tracking table
CREATE TABLE IF NOT EXISTS packages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pkg_id TEXT NOT NULL,
    pkg_name TEXT NOT NULL COLLATE NOCASE,
    pkg_family TEXT COLLATE NOCASE,
    build_script TEXT NOT NULL DEFAULT '',
    ghcr_pkg TEXT NOT NULL DEFAULT '',
    host_triplet TEXT NOT NULL,

    -- Current state
    current_version TEXT,
    upstream_version TEXT,
    is_outdated INTEGER DEFAULT 0,
    recipe_hash TEXT,

    -- Build info
    last_build_date TEXT,
    last_build_id TEXT,
    last_build_status TEXT CHECK(last_build_status IN ('success', 'failed', 'skipped', 'pending')),
    ghcr_tag TEXT,

    -- Timestamps
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),

    UNIQUE(pkg_id, host_triplet)
);

-- Build history (rolling window, keep last N builds)
CREATE TABLE IF NOT EXISTS build_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    package_id INTEGER NOT NULL,
    build_id TEXT NOT NULL,
    version TEXT NOT NULL,
    build_date TEXT NOT NULL,
    build_status TEXT CHECK(build_status IN ('success', 'failed', 'skipped')),
    duration_seconds INTEGER,
    artifact_size_bytes INTEGER,
    ghcr_tag TEXT,
    ghcr_digest TEXT,
    build_log_url TEXT,
    error_message TEXT,

    FOREIGN KEY (package_id) REFERENCES packages(id) ON DELETE CASCADE
);

-- Version comparison cache (avoid re-fetching upstream)
CREATE TABLE IF NOT EXISTS version_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    package_id INTEGER NOT NULL,
    upstream_source TEXT,
    upstream_version TEXT,
    checked_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL,

    FOREIGN KEY (package_id) REFERENCES packages(id) ON DELETE CASCADE,
    UNIQUE(package_id, upstream_source)
);

-- Failed package tracking (for retry logic)
CREATE TABLE IF NOT EXISTS failed_packages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    package_id INTEGER NOT NULL,
    failure_count INTEGER DEFAULT 1,
    last_failure_date TEXT NOT NULL,
    last_error_message TEXT,
    next_retry_date TEXT,

    FOREIGN KEY (package_id) REFERENCES packages(id) ON DELETE CASCADE,
    UNIQUE(package_id)
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_packages_host ON packages(host_triplet);
CREATE INDEX IF NOT EXISTS idx_packages_outdated ON packages(is_outdated) WHERE is_outdated = 1;
CREATE INDEX IF NOT EXISTS idx_packages_status ON packages(last_build_status);
CREATE INDEX IF NOT EXISTS idx_packages_pkg_name ON packages(pkg_name);
CREATE INDEX IF NOT EXISTS idx_build_history_date ON build_history(build_date);
CREATE INDEX IF NOT EXISTS idx_build_history_package ON build_history(package_id);
CREATE INDEX IF NOT EXISTS idx_version_cache_expires ON version_cache(expires_at);
CREATE INDEX IF NOT EXISTS idx_failed_packages_retry ON failed_packages(next_retry_date);
"#;

/// SQL for views
pub const CREATE_VIEWS: &str = r#"
-- View for packages needing rebuild
CREATE VIEW IF NOT EXISTS v_packages_needing_rebuild AS
SELECT p.*,
       bh.build_date as last_success_date,
       vc.upstream_version as cached_upstream_version
FROM packages p
LEFT JOIN build_history bh ON p.id = bh.package_id AND bh.build_status = 'success'
LEFT JOIN version_cache vc ON p.id = vc.package_id
LEFT JOIN failed_packages fp ON p.id = fp.package_id
WHERE (p.is_outdated = 1
       OR p.last_build_status IS NULL
       OR p.last_build_status = 'pending')
  AND (fp.next_retry_date IS NULL OR fp.next_retry_date <= datetime('now'))
ORDER BY p.pkg_family, p.pkg_name;

-- View for build statistics per host
CREATE VIEW IF NOT EXISTS v_build_stats AS
SELECT
    host_triplet,
    COUNT(*) as total_packages,
    SUM(CASE WHEN last_build_status = 'success' THEN 1 ELSE 0 END) as successful,
    SUM(CASE WHEN last_build_status = 'failed' THEN 1 ELSE 0 END) as failed,
    SUM(CASE WHEN last_build_status = 'pending' THEN 1 ELSE 0 END) as pending,
    SUM(CASE WHEN is_outdated = 1 THEN 1 ELSE 0 END) as outdated
FROM packages
GROUP BY host_triplet;
"#;
