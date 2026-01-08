//! OCI Manifest parsing
//!
//! Parses OCI image manifests to extract package metadata
//! stored in annotations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{Error, Result};

/// OCI manifest layer descriptor
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LayerDescriptor {
    /// Media type of the layer
    #[serde(rename = "mediaType")]
    pub media_type: String,

    /// Size in bytes
    pub size: u64,

    /// Content digest (sha256:...)
    pub digest: String,

    /// Layer annotations
    #[serde(default)]
    pub annotations: HashMap<String, String>,
}

impl LayerDescriptor {
    /// Get the filename from annotations
    pub fn filename(&self) -> Option<&str> {
        self.annotations
            .get("org.opencontainers.image.title")
            .map(|s| s.as_str())
    }
}

/// OCI image manifest
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OciManifest {
    /// Schema version
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,

    /// Media type
    #[serde(rename = "mediaType", default)]
    pub media_type: Option<String>,

    /// Config descriptor
    #[serde(default)]
    pub config: Option<LayerDescriptor>,

    /// Layer descriptors
    #[serde(default)]
    pub layers: Vec<LayerDescriptor>,

    /// Manifest annotations
    #[serde(default)]
    pub annotations: HashMap<String, String>,
}

impl OciManifest {
    /// Parse manifest from JSON string
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(Error::Json)
    }

    /// Get annotation value by key
    pub fn get_annotation(&self, key: &str) -> Option<&str> {
        self.annotations.get(key).map(|s| s.as_str())
    }

    /// Get the embedded package JSON from annotations
    pub fn get_package_json(&self) -> Result<Option<serde_json::Value>> {
        match self.annotations.get("dev.pkgforge.soar.json") {
            Some(json_str) => {
                let value: serde_json::Value = serde_json::from_str(json_str)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Get GHCR package identifier from annotations
    pub fn ghcr_pkg(&self) -> Option<&str> {
        self.get_annotation("dev.pkgforge.soar.ghcr_pkg")
    }

    /// Get build action URL from annotations
    pub fn build_action(&self) -> Option<&str> {
        self.get_annotation("dev.pkgforge.soar.build_gha")
    }

    /// Get build ID from annotations
    pub fn build_id(&self) -> Option<&str> {
        self.get_annotation("dev.pkgforge.soar.build_id")
    }

    /// Get total size of all layers
    pub fn total_size(&self) -> u64 {
        self.layers.iter().map(|l| l.size).sum()
    }

    /// Get human-readable size
    pub fn total_size_human(&self) -> String {
        format_bytes(self.total_size())
    }

    /// Get list of filenames in manifest
    pub fn filenames(&self) -> Vec<&str> {
        self.layers
            .iter()
            .filter_map(|l| l.filename())
            .collect()
    }

    /// Get layer by filename
    pub fn get_layer_by_filename(&self, filename: &str) -> Option<&LayerDescriptor> {
        self.layers.iter().find(|l| l.filename() == Some(filename))
    }

    /// Get blob reference for a file (ghcr_pkg@digest format)
    pub fn get_blob_ref(&self, filename: &str) -> Option<String> {
        let ghcr_pkg = self.ghcr_pkg()?;
        let base_pkg = ghcr_pkg.split(':').next()?;
        let layer = self.get_layer_by_filename(filename)?;
        Some(format!("{}@{}", base_pkg, layer.digest))
    }
}

/// Format bytes into human-readable string
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[unit_idx])
    } else {
        format!("{:.2} {}", size, UNITS[unit_idx])
    }
}

/// Extract metadata from manifest annotations into structured format
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestMetadata {
    pub ghcr_pkg: Option<String>,
    pub build_action: Option<String>,
    pub build_id: Option<String>,
    pub build_date: Option<String>,
    pub build_log: Option<String>,
    pub version: Option<String>,
    pub pkg_name: Option<String>,
    pub description: Option<String>,
    pub total_size: u64,
    pub files: Vec<String>,
}

impl ManifestMetadata {
    /// Extract metadata from an OCI manifest
    pub fn from_manifest(manifest: &OciManifest) -> Self {
        // Try to get embedded JSON first
        let pkg_json = manifest.get_package_json().ok().flatten();

        let get_json_field = |field: &str| -> Option<String> {
            pkg_json
                .as_ref()
                .and_then(|j| j.get(field))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        };

        Self {
            ghcr_pkg: manifest.ghcr_pkg().map(|s| s.to_string()),
            build_action: manifest.build_action().map(|s| s.to_string()),
            build_id: manifest.build_id().map(|s| s.to_string()),
            build_date: get_json_field("build_date"),
            build_log: get_json_field("build_log"),
            version: get_json_field("version"),
            pkg_name: get_json_field("pkg_name"),
            description: get_json_field("description"),
            total_size: manifest.total_size(),
            files: manifest.filenames().into_iter().map(|s| s.to_string()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_manifest() {
        let json = r#"{
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "layers": [
                {
                    "mediaType": "application/octet-stream",
                    "size": 1024,
                    "digest": "sha256:abc123",
                    "annotations": {
                        "org.opencontainers.image.title": "mybin"
                    }
                }
            ],
            "annotations": {
                "dev.pkgforge.soar.ghcr_pkg": "ghcr.io/pkgforge/mybin:v1.0"
            }
        }"#;

        let manifest = OciManifest::from_json(json).unwrap();
        assert_eq!(manifest.schema_version, 2);
        assert_eq!(manifest.layers.len(), 1);
        assert_eq!(manifest.ghcr_pkg(), Some("ghcr.io/pkgforge/mybin:v1.0"));
        assert_eq!(manifest.filenames(), vec!["mybin"]);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
    }

    #[test]
    fn test_total_size() {
        let json = r#"{
            "schemaVersion": 2,
            "layers": [
                {"mediaType": "application/octet-stream", "size": 100, "digest": "sha256:a"},
                {"mediaType": "application/octet-stream", "size": 200, "digest": "sha256:b"}
            ]
        }"#;

        let manifest = OciManifest::from_json(json).unwrap();
        assert_eq!(manifest.total_size(), 300);
    }
}
