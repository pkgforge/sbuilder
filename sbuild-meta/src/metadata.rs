//! Package metadata generation
//!
//! Builds complete package metadata by combining:
//! - SBUILD recipe data
//! - OCI manifest annotations
//! - Registry information

use serde::{Deserialize, Serialize};
use crate::manifest::OciManifest;
use crate::recipe::SBuildRecipe;

/// Helper to skip serializing empty vectors
fn is_empty_vec<T>(v: &Option<Vec<T>>) -> bool {
    v.as_ref().map(|v| v.is_empty()).unwrap_or(true)
}

/// Helper to skip serializing empty strings
fn is_empty_string(s: &str) -> bool {
    s.is_empty()
}

/// Format bytes as human-readable string
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Complete package metadata (compatible with soarql RemotePackage)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackageMetadata {
    // Core identifiers - ordered to match expected format
    #[serde(rename = "_disabled", skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<u64>,

    #[serde(skip_serializing_if = "is_empty_string")]
    pub pkg: String,

    #[serde(skip_serializing_if = "is_empty_string")]
    pub pkg_id: String,

    #[serde(skip_serializing_if = "is_empty_string")]
    pub pkg_name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkg_family: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkg_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkg_webpage: Option<String>,

    #[serde(skip_serializing_if = "is_empty_string")]
    pub description: String,

    #[serde(skip_serializing_if = "is_empty_string")]
    pub version: String,

    // Download info
    #[serde(skip_serializing_if = "is_empty_string")]
    pub download_url: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_raw: Option<u64>,

    // GHCR info
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ghcr_pkg: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ghcr_size: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ghcr_size_raw: Option<u64>,

    #[serde(skip_serializing_if = "is_empty_vec")]
    pub ghcr_files: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ghcr_blob: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ghcr_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_url: Option<String>,

    // Source info
    #[serde(skip_serializing_if = "is_empty_vec")]
    pub src_url: Option<Vec<String>>,

    #[serde(skip_serializing_if = "is_empty_vec")]
    pub homepage: Option<Vec<String>>,

    #[serde(skip_serializing_if = "is_empty_vec")]
    pub license: Option<Vec<String>>,

    #[serde(skip_serializing_if = "is_empty_vec")]
    pub maintainer: Option<Vec<String>>,

    #[serde(skip_serializing_if = "is_empty_vec")]
    pub note: Option<Vec<String>>,

    #[serde(skip_serializing_if = "is_empty_vec")]
    pub tag: Option<Vec<String>>,

    // Checksums
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bsum: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub shasum: Option<String>,

    // Build info
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_date: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_gha: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_script: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_log: Option<String>,

    // Categories and provides
    #[serde(skip_serializing_if = "is_empty_vec")]
    pub category: Option<Vec<String>>,

    #[serde(skip_serializing_if = "is_empty_vec")]
    pub provides: Option<Vec<String>>,

    // Desktop/App info
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub desktop: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub appstream: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,

    // Flags - all as proper booleans
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub desktop_integration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub portable: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurse_provides: Option<bool>,

    // Additional
    #[serde(skip_serializing_if = "is_empty_vec")]
    pub snapshots: Option<Vec<String>>,

    #[serde(skip_serializing_if = "is_empty_vec")]
    pub replaces: Option<Vec<String>>,
}

impl PackageMetadata {
    /// Create metadata from SBUILD recipe (minimal, without registry data)
    pub fn from_recipe(recipe: &SBuildRecipe) -> Self {
        Self {
            pkg: recipe.pkg.clone(),
            pkg_id: recipe.pkg_id.clone(),
            pkg_name: recipe.pkg.clone(),
            pkg_family: Some(recipe.pkg.clone()),
            pkg_type: recipe.pkg_type.clone(),
            description: recipe.description.0.clone(),
            version: recipe.pkgver.clone().unwrap_or_default(),
            src_url: if recipe.src_url.is_empty() {
                None
            } else {
                Some(recipe.src_url.clone())
            },
            homepage: if recipe.homepage.is_empty() {
                None
            } else {
                Some(recipe.homepage.clone())
            },
            license: if recipe.license.is_empty() {
                None
            } else {
                Some(recipe.license.iter().map(|l| l.id.clone()).collect())
            },
            maintainer: if recipe.maintainer.is_empty() {
                None
            } else {
                Some(recipe.maintainer.clone())
            },
            note: if recipe.note.is_empty() {
                None
            } else {
                Some(recipe.note.clone())
            },
            tag: if recipe.tag.is_empty() {
                None
            } else {
                Some(recipe.tag.clone())
            },
            category: if recipe.category.is_empty() {
                None
            } else {
                Some(recipe.category.clone())
            },
            provides: if recipe.provides.is_empty() {
                None
            } else {
                Some(recipe.provides.clone())
            },
            disabled: if recipe.disabled { Some(true) } else { None },
            ..Default::default()
        }
    }

    /// Enrich metadata with OCI manifest data
    pub fn enrich_from_manifest(&mut self, manifest: &OciManifest, tag: &str) {
        // Get embedded JSON if available
        if let Ok(Some(pkg_json)) = manifest.get_package_json() {
            self.merge_from_json(&pkg_json);
        }

        // GHCR info
        self.ghcr_pkg = manifest.ghcr_pkg().map(|s| s.to_string());
        let size = manifest.total_size();
        self.ghcr_size_raw = Some(size);
        self.ghcr_size = Some(format_size(size));
        self.ghcr_files = Some(manifest.filenames().into_iter().map(|s| s.to_string()).collect());

        // Build info from annotations
        if self.build_id.is_none() {
            let build_id = manifest.build_id().map(|s| s.to_string());
            self.build_id = build_id.clone();

            // Generate GitHub Actions URL if we have a build ID
            if let Some(ref id) = build_id {
                // Try to determine the repo from ghcr_pkg
                if let Some(ref ghcr_pkg) = self.ghcr_pkg {
                    let cache_type = if ghcr_pkg.contains("pkgcache") { "pkgcache" } else { "bincache" };
                    self.build_gha = Some(format!(
                        "https://github.com/pkgforge/{}/actions/runs/{}",
                        cache_type, id
                    ));
                }
            }
        }

        // Generate blob reference for main binary
        if let Some(filename) = manifest.filenames().first() {
            self.ghcr_blob = manifest.get_blob_ref(filename);

            // Generate download URL and manifest URL
            if let Some(ref ghcr_pkg) = self.ghcr_pkg {
                let base = ghcr_pkg.split(':').next().unwrap_or(ghcr_pkg);
                let repo = base.replace("ghcr.io/", "");
                self.download_url = format!(
                    "https://api.ghcr.pkgforge.dev/{}?tag={}&download={}",
                    repo, tag, filename
                );
                self.manifest_url = Some(format!(
                    "https://api.ghcr.pkgforge.dev/{}?tag={}&manifest",
                    repo, tag
                ));
                // Size is usually same as ghcr_size for single binary packages
                self.size_raw = self.ghcr_size_raw;
                self.size = self.ghcr_size.clone();
            }
        }
    }

    /// Merge data from embedded JSON
    fn merge_from_json(&mut self, json: &serde_json::Value) {
        let get_str = |key: &str| -> Option<String> {
            json.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
        };

        let get_vec = |key: &str| -> Option<Vec<String>> {
            json.get(key).and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
        };

        // Override with manifest values if present
        if let Some(v) = get_str("version") {
            self.version = v;
        }
        if let Some(v) = get_str("description") {
            self.description = v;
        }
        if let Some(v) = get_str("build_date") {
            self.build_date = Some(v);
        }
        if let Some(v) = get_str("build_log") {
            self.build_log = Some(v);
        }
        if let Some(v) = get_str("build_script") {
            self.build_script = Some(v);
        }
        if let Some(v) = get_str("bsum") {
            self.bsum = Some(v);
        }
        if let Some(v) = get_str("shasum") {
            self.shasum = Some(v);
        }
        if let Some(v) = get_str("icon") {
            self.icon = Some(v);
        }
        if let Some(v) = get_str("desktop") {
            self.desktop = Some(v);
        }
        if let Some(v) = get_str("appstream") {
            self.appstream = Some(v);
        }

        // Array fields
        if self.provides.is_none() {
            self.provides = get_vec("provides");
        }
        if self.snapshots.is_none() {
            self.snapshots = get_vec("snapshots");
        }
    }

    /// Parse flags from notes and filter out internal flag messages
    /// Sets deprecated flag and removes [DEPRECATED], [EXTERNAL], [NO_INSTALL], [UNTRUSTED] messages
    pub fn parse_note_flags(&mut self) {
        if let Some(notes) = self.note.take() {
            // Check for deprecated flag before filtering
            if notes.iter().any(|n| n.contains("[DEPRECATED]")) {
                self.deprecated = Some(true);
            }

            // Filter out internal flag messages - these are for CI/internal use only
            let filtered: Vec<String> = notes
                .into_iter()
                .filter(|note| {
                    !note.contains("[DEPRECATED]")
                        && !note.contains("[EXTERNAL]")
                        && !note.contains("[NO_INSTALL]")
                        && !note.contains("[UNTRUSTED]")
                        && !note.contains("[DO NOT RUN]")
                })
                .collect();

            // Only set note if there are remaining notes after filtering
            if !filtered.is_empty() {
                self.note = Some(filtered);
            }
        }
    }

    /// Validate that required fields are present
    pub fn is_valid(&self) -> bool {
        !self.pkg.is_empty()
            && !self.pkg_id.is_empty()
            && !self.pkg_name.is_empty()
            && !self.description.is_empty()
            && !self.version.is_empty()
            && !self.download_url.is_empty()
    }
}

/// Builder for constructing PackageMetadata
pub struct MetadataBuilder {
    metadata: PackageMetadata,
}

impl MetadataBuilder {
    pub fn new(recipe: &SBuildRecipe) -> Self {
        Self {
            metadata: PackageMetadata::from_recipe(recipe),
        }
    }

    pub fn with_manifest(mut self, manifest: &OciManifest, tag: &str) -> Self {
        self.metadata.enrich_from_manifest(manifest, tag);
        self
    }

    pub fn build(mut self) -> PackageMetadata {
        self.metadata.parse_note_flags();
        self.metadata
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::SBuildRecipe;

    #[test]
    fn test_from_recipe() {
        let yaml = r#"
pkg: test
pkg_id: example.com.test
version: "1.0.0"
description: A test package
category:
  - Utility
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let metadata = PackageMetadata::from_recipe(&recipe);

        assert_eq!(metadata.pkg, "test");
        assert_eq!(metadata.pkg_id, "example.com.test");
        assert_eq!(metadata.version, "1.0.0");
    }

    #[test]
    fn test_parse_note_flags() {
        let mut metadata = PackageMetadata::default();
        metadata.note = Some(vec![
            "[DEPRECATED] Old package".to_string(),
            "[NO_INSTALL] Do not install".to_string(),
            "This is a real note".to_string(),
        ]);

        metadata.parse_note_flags();

        // Deprecated flag should be set
        assert_eq!(metadata.deprecated, Some(true));
        // Internal flag messages should be filtered out, only real notes remain
        assert_eq!(metadata.note, Some(vec!["This is a real note".to_string()]));
    }

    #[test]
    fn test_parse_note_flags_all_filtered() {
        let mut metadata = PackageMetadata::default();
        metadata.note = Some(vec![
            "[DEPRECATED] Old package".to_string(),
            "[DO NOT RUN] CI only".to_string(),
        ]);

        metadata.parse_note_flags();

        // All notes were internal flags, so note should be None
        assert_eq!(metadata.note, None);
    }
}
