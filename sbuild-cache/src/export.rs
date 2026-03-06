//! Export MongoDB data to SQLite for user consumption

use std::path::Path;

use crate::error::Result;
use crate::models::*;
use crate::mongo::MongoDatabase;
use crate::sqlite::CacheDatabase;

/// Export all data from MongoDB to a fresh SQLite file
pub async fn export_to_sqlite(mongo: &MongoDatabase, output_path: &Path) -> Result<()> {
    if output_path.exists() {
        std::fs::remove_file(output_path)?;
    }

    let sqlite_db = CacheDatabase::open(output_path)?;
    let packages = mongo.list_all_packages().await?;

    for pkg_doc in &packages {
        let record = PackageRecord {
            id: None,
            pkg_id: pkg_doc.pkg_id.clone(),
            pkg_name: pkg_doc.pkg_name.clone(),
            pkg_family: pkg_doc.pkg_family.clone(),
            build_script: String::new(),
            ghcr_pkg: String::new(),
            host_triplet: pkg_doc.host_triplet.clone(),
            current_version: pkg_doc.current_version.clone(),
            upstream_version: pkg_doc.upstream_version.clone(),
            is_outdated: pkg_doc.is_outdated,
            recipe_hash: pkg_doc.recipe_hash.clone(),
            base_version: pkg_doc.base_version.clone(),
            revision: pkg_doc.revision,
            last_build_date: pkg_doc
                .build_history
                .last()
                .map(|h| h.build_date.to_chrono()),
            last_build_id: pkg_doc
                .build_history
                .last()
                .and_then(|h| h.build_id.clone()),
            last_build_status: pkg_doc
                .build_history
                .last()
                .and_then(|h| BuildStatus::from_str(&h.build_status)),
            ghcr_tag: pkg_doc
                .build_history
                .last()
                .and_then(|h| h.ghcr_tag.clone()),
            created_at: pkg_doc.created_at.to_chrono(),
            updated_at: pkg_doc.updated_at.to_chrono(),
        };

        sqlite_db.import_package(&record)?;

        for hist in &pkg_doc.build_history {
            let entry = BuildHistoryEntry {
                id: None,
                package_id: 0,
                build_id: hist.build_id.clone().unwrap_or_default(),
                version: hist.version.clone(),
                build_date: hist.build_date.to_chrono(),
                build_status: BuildStatus::from_str(&hist.build_status)
                    .unwrap_or(BuildStatus::Pending),
                duration_seconds: hist.duration_seconds,
                artifact_size_bytes: None,
                ghcr_tag: hist.ghcr_tag.clone(),
                ghcr_digest: None,
                build_log_url: hist.build_log_url.clone(),
                error_message: hist.error_message.clone(),
            };

            sqlite_db.import_build_history(&pkg_doc.pkg_id, &pkg_doc.host_triplet, &entry)?;
        }
    }

    Ok(())
}
