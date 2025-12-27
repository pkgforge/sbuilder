//! Data models for the cache database

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Build status enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildStatus {
    Success,
    Failed,
    Pending,
    Skipped,
}

impl BuildStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            BuildStatus::Success => "success",
            BuildStatus::Failed => "failed",
            BuildStatus::Pending => "pending",
            BuildStatus::Skipped => "skipped",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "success" => Some(BuildStatus::Success),
            "failed" => Some(BuildStatus::Failed),
            "pending" => Some(BuildStatus::Pending),
            "skipped" => Some(BuildStatus::Skipped),
            _ => None,
        }
    }
}

impl std::fmt::Display for BuildStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Package record in the cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageRecord {
    pub id: Option<i64>,
    pub pkg_id: String,
    pub pkg_name: String,
    pub pkg_family: Option<String>,
    pub build_script: String,
    pub ghcr_pkg: String,
    pub host_triplet: String,

    // Current state
    pub current_version: Option<String>,
    pub upstream_version: Option<String>,
    pub is_outdated: bool,
    pub recipe_hash: Option<String>,

    // Build info
    pub last_build_date: Option<DateTime<Utc>>,
    pub last_build_id: Option<String>,
    pub last_build_status: Option<BuildStatus>,
    pub ghcr_tag: Option<String>,

    // Timestamps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PackageRecord {
    pub fn new(pkg_id: String, pkg_name: String, host_triplet: String) -> Self {
        let now = Utc::now();
        Self {
            id: None,
            pkg_id,
            pkg_name: pkg_name.clone(),
            pkg_family: Some(pkg_name),
            build_script: String::new(),
            ghcr_pkg: String::new(),
            host_triplet,
            current_version: None,
            upstream_version: None,
            is_outdated: false,
            recipe_hash: None,
            last_build_date: None,
            last_build_id: None,
            last_build_status: None,
            ghcr_tag: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Build history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildHistoryEntry {
    pub id: Option<i64>,
    pub package_id: i64,
    pub build_id: String,
    pub version: String,
    pub build_date: DateTime<Utc>,
    pub build_status: BuildStatus,
    pub duration_seconds: Option<i64>,
    pub artifact_size_bytes: Option<i64>,
    pub ghcr_tag: Option<String>,
    pub ghcr_digest: Option<String>,
    pub build_log_url: Option<String>,
    pub error_message: Option<String>,
}

/// Version cache entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionCacheEntry {
    pub id: Option<i64>,
    pub package_id: i64,
    pub upstream_source: Option<String>,
    pub upstream_version: String,
    pub checked_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Failed package tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedPackage {
    pub id: Option<i64>,
    pub package_id: i64,
    pub failure_count: i32,
    pub last_failure_date: DateTime<Utc>,
    pub last_error_message: Option<String>,
    pub next_retry_date: Option<DateTime<Utc>>,
}

/// Statistics for build operations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildStats {
    pub total_packages: i64,
    pub successful: i64,
    pub failed: i64,
    pub pending: i64,
    pub outdated: i64,
}

/// Reason for rebuilding a package
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "reason")]
pub enum RebuildReason {
    /// New package, never built before
    NewPackage,
    /// User forced rebuild
    Forced,
    /// Recipe content changed
    RecipeChanged {
        old_hash: String,
        new_hash: String,
    },
    /// Version field was updated (bot PR merged)
    VersionUpdated {
        old_version: String,
        new_version: String,
    },
    /// Previous build failed, retrying
    RetryFailed {
        attempt: i32,
        last_error: String,
    },
    /// Build is too old
    StaleBuild {
        last_build_days_ago: i64,
        threshold_days: i64,
    },
}

/// Decision about whether to rebuild a package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildDecision {
    pub should_rebuild: bool,
    pub reason: Option<RebuildReason>,
    pub priority: u8, // 1 = highest, 5 = lowest
}

impl RebuildDecision {
    pub fn skip() -> Self {
        Self {
            should_rebuild: false,
            reason: None,
            priority: 5,
        }
    }

    pub fn rebuild(reason: RebuildReason, priority: u8) -> Self {
        Self {
            should_rebuild: true,
            reason: Some(reason),
            priority,
        }
    }
}
