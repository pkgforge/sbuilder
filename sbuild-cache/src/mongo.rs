//! MongoDB backend for the build cache

use bson::{doc, Bson, Document};
use chrono::{Duration, Utc};
use mongodb::{
    options::{ClientOptions, FindOneOptions, FindOptions, IndexOptions, UpdateOptions},
    Client, Collection, IndexModel,
};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::models::*;

/// MongoDB document for a package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageDocument {
    pub pkg_id: String,
    pub pkg_name: String,
    #[serde(default)]
    pub pkg_family: Option<String>,
    pub host_triplet: String,
    #[serde(default)]
    pub current_version: Option<String>,
    #[serde(default)]
    pub upstream_version: Option<String>,
    #[serde(default)]
    pub is_outdated: bool,
    #[serde(default)]
    pub recipe_hash: Option<String>,
    #[serde(default)]
    pub base_version: Option<String>,
    #[serde(default)]
    pub revision: i32,
    #[serde(default)]
    pub build_history: Vec<BuildHistoryDocument>,
    pub created_at: bson::DateTime,
    pub updated_at: bson::DateTime,
}

/// MongoDB subdocument for build history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildHistoryDocument {
    #[serde(default)]
    pub build_id: Option<String>,
    pub version: String,
    pub build_date: bson::DateTime,
    pub build_status: String,
    #[serde(default)]
    pub ghcr_tag: Option<String>,
    #[serde(default)]
    pub recipe_hash: Option<String>,
    #[serde(default)]
    pub duration_seconds: Option<i64>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub build_log_url: Option<String>,
}

/// MongoDB cache database
pub struct MongoDatabase {
    collection: Collection<PackageDocument>,
    raw_collection: Collection<Document>,
}

impl MongoDatabase {
    /// Connect to MongoDB using a connection URI
    pub async fn connect(uri: &str) -> Result<Self> {
        let options = ClientOptions::parse(uri).await?;
        let client = Client::with_options(options)?;

        let db_name = client
            .default_database()
            .map(|db| db.name().to_string())
            .unwrap_or_else(|| "sbuild_cache".to_string());

        let db = client.database(&db_name);
        let collection = db.collection::<PackageDocument>("packages");
        let raw_collection = db.collection::<Document>("packages");

        let mongo_db = Self {
            collection,
            raw_collection,
        };
        mongo_db.ensure_indexes().await?;
        Ok(mongo_db)
    }

    /// Create indexes for efficient queries
    async fn ensure_indexes(&self) -> Result<()> {
        let unique_index = IndexModel::builder()
            .keys(doc! { "pkg_id": 1, "host_triplet": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();

        let name_index = IndexModel::builder()
            .keys(doc! { "pkg_name": 1, "host_triplet": 1 })
            .build();

        let outdated_index = IndexModel::builder()
            .keys(doc! { "is_outdated": 1 })
            .build();

        self.raw_collection
            .create_indexes(vec![unique_index, name_index, outdated_index])
            .await?;

        Ok(())
    }

    /// Get or create a package record
    pub async fn get_or_create_package(
        &self,
        pkg_id: &str,
        pkg_name: &str,
        host_triplet: &str,
    ) -> Result<PackageRecord> {
        let now = Utc::now();
        let filter = doc! { "pkg_id": pkg_id, "host_triplet": host_triplet };
        let update = doc! {
            "$setOnInsert": {
                "pkg_id": pkg_id,
                "pkg_name": pkg_name,
                "pkg_family": Bson::Null,
                "host_triplet": host_triplet,
                "current_version": Bson::Null,
                "upstream_version": Bson::Null,
                "is_outdated": false,
                "recipe_hash": Bson::Null,
                "base_version": Bson::Null,
                "revision": 0_i32,
                "build_history": Bson::Array(vec![]),
                "created_at": bson::DateTime::from_chrono(now),
                "updated_at": bson::DateTime::from_chrono(now),
            }
        };
        let options = UpdateOptions::builder().upsert(true).build();

        self.raw_collection
            .update_one(filter, update)
            .with_options(options)
            .await?;

        self.get_package(pkg_id, host_triplet)
            .await?
            .ok_or_else(|| Error::Other("Failed to create package".to_string()))
    }

    /// Get a package by ID and host
    pub async fn get_package(
        &self,
        pkg_id: &str,
        host_triplet: &str,
    ) -> Result<Option<PackageRecord>> {
        let filter = doc! { "pkg_id": pkg_id, "host_triplet": host_triplet };
        let result = self.collection.find_one(filter).await?;
        Ok(result.map(|d| pkg_doc_to_record(&d)))
    }

    /// Find packages by name (or pkg_id suffix) and host
    pub async fn find_packages_by_name(
        &self,
        name: &str,
        host_triplet: &str,
    ) -> Result<Vec<PackageRecord>> {
        let escaped = regex_escape(name);
        let filter = doc! {
            "$or": [
                { "pkg_name": name, "host_triplet": host_triplet },
                { "pkg_id": { "$regex": format!(r"\.{}$", escaped) }, "host_triplet": host_triplet },
            ]
        };
        let options = FindOptions::builder().sort(doc! { "pkg_id": 1 }).build();

        let mut cursor = self.collection.find(filter).with_options(options).await?;
        let mut results = Vec::new();
        while cursor.advance().await? {
            let doc = cursor.deserialize_current()?;
            results.push(pkg_doc_to_record(&doc));
        }
        Ok(results)
    }

    /// Update package after a build
    pub async fn update_build_result(
        &self,
        pkg_id: &str,
        host_triplet: &str,
        version: &str,
        status: BuildStatus,
        build_id: Option<&str>,
        ghcr_tag: Option<&str>,
        recipe_hash: Option<&str>,
        base_version: Option<&str>,
        revision: i32,
        duration_seconds: Option<i64>,
        error_message: Option<&str>,
        build_log_url: Option<&str>,
    ) -> Result<()> {
        if version.is_empty() || version == "unknown" {
            return Err(Error::Other(format!(
                "Invalid version '{}': must not be empty or 'unknown'",
                version
            )));
        }

        let now = Utc::now();
        let filter = doc! { "pkg_id": pkg_id, "host_triplet": host_triplet };

        let history_entry = doc! {
            "build_id": build_id,
            "version": version,
            "build_date": bson::DateTime::from_chrono(now),
            "build_status": status.as_str(),
            "ghcr_tag": ghcr_tag,
            "recipe_hash": recipe_hash,
            "duration_seconds": duration_seconds,
            "error_message": error_message,
            "build_log_url": build_log_url,
        };

        let update = doc! {
            "$set": {
                "current_version": version,
                "is_outdated": false,
                "recipe_hash": recipe_hash,
                "base_version": base_version,
                "revision": revision,
                "updated_at": bson::DateTime::from_chrono(now),
            },
            "$push": {
                "build_history": {
                    "$each": [history_entry],
                    "$slice": -3_i32,
                }
            }
        };

        self.raw_collection.update_one(filter, update).await?;
        Ok(())
    }

    /// Get the next revision number for a package version
    pub async fn get_revision(
        &self,
        pkg_id: &str,
        host_triplet: &str,
        base_version: &str,
    ) -> Result<i32> {
        let filter = doc! { "pkg_id": pkg_id, "host_triplet": host_triplet };
        let options = FindOneOptions::builder()
            .projection(doc! { "base_version": 1, "revision": 1 })
            .build();

        let result = self
            .raw_collection
            .find_one(filter)
            .with_options(options)
            .await?;

        match result {
            Some(doc) => {
                let stored_base = doc.get_str("base_version").ok();
                let stored_revision = doc.get_i32("revision").unwrap_or(0);

                if stored_base == Some(base_version) {
                    Ok(stored_revision + 1)
                } else {
                    Ok(0)
                }
            }
            None => Ok(0),
        }
    }

    /// Mark package as outdated
    pub async fn mark_outdated(
        &self,
        pkg_id: &str,
        host_triplet: &str,
        upstream_version: &str,
    ) -> Result<()> {
        let now = Utc::now();
        let filter = doc! { "pkg_id": pkg_id, "host_triplet": host_triplet };
        let update = doc! {
            "$set": {
                "is_outdated": true,
                "upstream_version": upstream_version,
                "updated_at": bson::DateTime::from_chrono(now),
            }
        };

        self.raw_collection.update_one(filter, update).await?;
        Ok(())
    }

    /// Update recipe hash for a package
    pub async fn update_recipe_hash(
        &self,
        pkg_id: &str,
        host_triplet: &str,
        hash: &str,
    ) -> Result<()> {
        let now = Utc::now();
        let filter = doc! { "pkg_id": pkg_id, "host_triplet": host_triplet };
        let update = doc! {
            "$set": {
                "recipe_hash": hash,
                "updated_at": bson::DateTime::from_chrono(now),
            }
        };

        self.raw_collection.update_one(filter, update).await?;
        Ok(())
    }

    /// Get packages needing rebuild for a host
    pub async fn get_packages_needing_rebuild(
        &self,
        host_triplet: &str,
    ) -> Result<Vec<PackageRecord>> {
        // First: outdated or never built
        let filter = doc! {
            "host_triplet": host_triplet,
            "$or": [
                { "is_outdated": true },
                { "build_history": { "$size": 0 } },
                { "build_history": { "$exists": false } },
            ]
        };
        let options = FindOptions::builder().sort(doc! { "pkg_name": 1 }).build();

        let mut cursor = self.collection.find(filter).with_options(options).await?;
        let mut results = Vec::new();
        while cursor.advance().await? {
            let doc = cursor.deserialize_current()?;
            results.push(pkg_doc_to_record(&doc));
        }

        // Also check for failed packages with retry allowed
        let all_filter = doc! {
            "host_triplet": host_triplet,
            "is_outdated": false,
        };
        let mut all_cursor = self
            .collection
            .find(all_filter)
            .with_options(FindOptions::builder().sort(doc! { "pkg_name": 1 }).build())
            .await?;

        while all_cursor.advance().await? {
            let pkg_doc = all_cursor.deserialize_current()?;
            if let Some(last) = pkg_doc.build_history.last() {
                if last.build_status == "failed"
                    && is_retry_allowed_from_history(&pkg_doc.build_history)
                {
                    results.push(pkg_doc_to_record(&pkg_doc));
                }
            }
        }

        Ok(results)
    }

    /// Get build statistics for a host
    pub async fn get_stats(&self, host_triplet: &str) -> Result<BuildStats> {
        let pipeline = vec![
            doc! { "$match": { "host_triplet": host_triplet } },
            doc! {
                "$group": {
                    "_id": Bson::Null,
                    "total": { "$sum": 1 },
                    "successful": {
                        "$sum": {
                            "$cond": [
                                { "$eq": [{ "$last": "$build_history.build_status" }, "success"] },
                                1, 0
                            ]
                        }
                    },
                    "failed": {
                        "$sum": {
                            "$cond": [
                                { "$eq": [{ "$last": "$build_history.build_status" }, "failed"] },
                                1, 0
                            ]
                        }
                    },
                    "pending": {
                        "$sum": {
                            "$cond": [
                                { "$or": [
                                    { "$eq": [{ "$size": { "$ifNull": ["$build_history", []] } }, 0] },
                                    { "$eq": [{ "$last": "$build_history.build_status" }, "pending"] },
                                ]},
                                1, 0
                            ]
                        }
                    },
                    "outdated": {
                        "$sum": {
                            "$cond": ["$is_outdated", 1, 0]
                        }
                    },
                }
            },
        ];

        let mut cursor = self.raw_collection.aggregate(pipeline).await?;

        if cursor.advance().await? {
            let doc = cursor.deserialize_current()?;
            Ok(BuildStats {
                total_packages: doc.get_i32("total").unwrap_or(0) as i64,
                successful: doc.get_i32("successful").unwrap_or(0) as i64,
                failed: doc.get_i32("failed").unwrap_or(0) as i64,
                pending: doc.get_i32("pending").unwrap_or(0) as i64,
                outdated: doc.get_i32("outdated").unwrap_or(0) as i64,
            })
        } else {
            Ok(BuildStats::default())
        }
    }

    /// Check if retry is allowed for a package
    pub async fn is_retry_allowed(&self, pkg_id: &str, host_triplet: &str) -> Result<bool> {
        let filter = doc! { "pkg_id": pkg_id, "host_triplet": host_triplet };
        let result = self.collection.find_one(filter).await?;

        match result {
            Some(doc) => Ok(is_retry_allowed_from_history(&doc.build_history)),
            None => Ok(true),
        }
    }

    /// List packages with optional status filter
    pub async fn list_packages(
        &self,
        host_triplet: &str,
        status_filter: Option<BuildStatus>,
        include_outdated: bool,
    ) -> Result<Vec<PackageRecord>> {
        let mut filter = doc! { "host_triplet": host_triplet };

        match (&status_filter, include_outdated) {
            (Some(status), true) => {
                filter.insert(
                    "$or",
                    vec![
                        doc! {
                            "$expr": {
                                "$eq": [
                                    { "$arrayElemAt": ["$build_history.build_status", -1] },
                                    status.as_str()
                                ]
                            }
                        },
                        doc! { "is_outdated": true },
                    ],
                );
            }
            (Some(status), false) => {
                filter.insert(
                    "$expr",
                    doc! {
                        "$eq": [
                            { "$arrayElemAt": ["$build_history.build_status", -1] },
                            status.as_str()
                        ]
                    },
                );
            }
            (None, true) => {
                filter.insert("is_outdated", true);
            }
            (None, false) => {}
        }

        let options = FindOptions::builder().sort(doc! { "pkg_name": 1 }).build();

        let mut cursor = self.collection.find(filter).with_options(options).await?;
        let mut results = Vec::new();
        while cursor.advance().await? {
            let doc = cursor.deserialize_current()?;
            results.push(pkg_doc_to_record(&doc));
        }
        Ok(results)
    }

    /// List all packages (for export)
    pub async fn list_all_packages(&self) -> Result<Vec<PackageDocument>> {
        let options = FindOptions::builder()
            .sort(doc! { "pkg_id": 1, "host_triplet": 1 })
            .build();

        let mut cursor = self.collection.find(doc! {}).with_options(options).await?;
        let mut results = Vec::new();
        while cursor.advance().await? {
            results.push(cursor.deserialize_current()?);
        }
        Ok(results)
    }

    /// Get recent build history
    pub async fn get_recent_builds(
        &self,
        host_triplet: &str,
        limit: i64,
    ) -> Result<Vec<(PackageRecord, BuildHistoryEntry)>> {
        let pipeline = vec![
            doc! { "$match": { "host_triplet": host_triplet } },
            doc! { "$unwind": "$build_history" },
            doc! { "$sort": { "build_history.build_date": -1 } },
            doc! { "$limit": limit },
        ];

        let mut cursor = self.raw_collection.aggregate(pipeline).await?;
        let mut results = Vec::new();

        while cursor.advance().await? {
            let doc: Document = cursor.deserialize_current()?;
            // Deserialize the parent as a partial PackageDocument
            let pkg_id = doc.get_str("pkg_id").unwrap_or_default();
            let pkg_name = doc.get_str("pkg_name").unwrap_or_default();
            let host = doc.get_str("host_triplet").unwrap_or_default();

            let record = PackageRecord {
                id: None,
                pkg_id: pkg_id.to_string(),
                pkg_name: pkg_name.to_string(),
                pkg_family: doc.get_str("pkg_family").ok().map(|s| s.to_string()),
                build_script: String::new(),
                ghcr_pkg: String::new(),
                host_triplet: host.to_string(),
                current_version: doc.get_str("current_version").ok().map(|s| s.to_string()),
                upstream_version: doc.get_str("upstream_version").ok().map(|s| s.to_string()),
                is_outdated: doc.get_bool("is_outdated").unwrap_or(false),
                recipe_hash: doc.get_str("recipe_hash").ok().map(|s| s.to_string()),
                base_version: doc.get_str("base_version").ok().map(|s| s.to_string()),
                revision: doc.get_i32("revision").unwrap_or(0),
                last_build_date: None,
                last_build_id: None,
                last_build_status: None,
                ghcr_tag: None,
                created_at: doc
                    .get_datetime("created_at")
                    .ok()
                    .map(|dt| dt.to_chrono())
                    .unwrap_or_else(Utc::now),
                updated_at: doc
                    .get_datetime("updated_at")
                    .ok()
                    .map(|dt| dt.to_chrono())
                    .unwrap_or_else(Utc::now),
            };

            if let Ok(hist_doc) = doc.get_document("build_history") {
                let entry = BuildHistoryEntry {
                    id: None,
                    package_id: 0,
                    build_id: hist_doc
                        .get_str("build_id")
                        .ok()
                        .unwrap_or_default()
                        .to_string(),
                    version: hist_doc.get_str("version").unwrap_or_default().to_string(),
                    build_date: hist_doc
                        .get_datetime("build_date")
                        .ok()
                        .map(|dt| dt.to_chrono())
                        .unwrap_or_else(Utc::now),
                    build_status: hist_doc
                        .get_str("build_status")
                        .ok()
                        .and_then(BuildStatus::from_str)
                        .unwrap_or(BuildStatus::Pending),
                    duration_seconds: hist_doc
                        .get_i64("duration_seconds")
                        .ok()
                        .or_else(|| hist_doc.get_i32("duration_seconds").ok().map(|v| v as i64)),
                    artifact_size_bytes: None,
                    ghcr_tag: hist_doc.get_str("ghcr_tag").ok().map(|s| s.to_string()),
                    ghcr_digest: None,
                    build_log_url: hist_doc
                        .get_str("build_log_url")
                        .ok()
                        .map(|s| s.to_string()),
                    error_message: hist_doc
                        .get_str("error_message")
                        .ok()
                        .map(|s| s.to_string()),
                };
                results.push((record, entry));
            }
        }
        Ok(results)
    }
}

/// Check if retry is allowed based on build history
fn is_retry_allowed_from_history(history: &[BuildHistoryDocument]) -> bool {
    let consecutive_failures = history
        .iter()
        .rev()
        .take_while(|entry| entry.build_status == "failed")
        .count();

    if consecutive_failures == 0 {
        return true;
    }

    // Exponential backoff: 1h, 2h, 4h
    let backoff_hours = std::cmp::min(1_i64 << (consecutive_failures - 1), 24);

    if let Some(last) = history.last() {
        let last_date = last.build_date.to_chrono();
        let next_retry = last_date + Duration::hours(backoff_hours);
        Utc::now() >= next_retry
    } else {
        true
    }
}

/// Escape special regex characters
fn regex_escape(s: &str) -> String {
    let specials = [
        '.', '^', '$', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|', '\\',
    ];
    let mut result = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        if specials.contains(&c) {
            result.push('\\');
        }
        result.push(c);
    }
    result
}

/// Convert a PackageDocument to a PackageRecord
fn pkg_doc_to_record(doc: &PackageDocument) -> PackageRecord {
    let last_build = doc.build_history.last();

    PackageRecord {
        id: None,
        pkg_id: doc.pkg_id.clone(),
        pkg_name: doc.pkg_name.clone(),
        pkg_family: doc.pkg_family.clone(),
        build_script: String::new(),
        ghcr_pkg: String::new(),
        host_triplet: doc.host_triplet.clone(),
        current_version: doc.current_version.clone(),
        upstream_version: doc.upstream_version.clone(),
        is_outdated: doc.is_outdated,
        recipe_hash: doc.recipe_hash.clone(),
        base_version: doc.base_version.clone(),
        revision: doc.revision,
        last_build_date: last_build.map(|h| h.build_date.to_chrono()),
        last_build_id: last_build.and_then(|h| h.build_id.clone()),
        last_build_status: last_build.and_then(|h| BuildStatus::from_str(&h.build_status)),
        ghcr_tag: last_build.and_then(|h| h.ghcr_tag.clone()),
        created_at: doc.created_at.to_chrono(),
        updated_at: doc.updated_at.to_chrono(),
    }
}
