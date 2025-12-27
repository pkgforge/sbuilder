//! GHCR (GitHub Container Registry) push utilities
//!
//! Handles pushing built packages to GHCR using the OCI registry API.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GhcrError {
    #[error("oras command not found - install oras to push packages")]
    OrasNotFound,

    #[error("GHCR authentication failed: {0}")]
    AuthFailed(String),

    #[error("Push failed: {0}")]
    PushFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// GHCR package metadata for annotations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageAnnotations {
    pub pkg: String,
    pub pkg_id: String,
    pub pkg_type: Option<String>,
    pub version: String,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub build_date: String,
    pub build_id: Option<String>,
    pub build_gha: Option<String>,
    pub build_script: Option<String>,
}

/// GHCR client for pushing packages
pub struct GhcrClient {
    token: String,
    registry: String,
}

impl GhcrClient {
    pub fn new(token: String) -> Self {
        Self {
            token,
            registry: "ghcr.io".to_string(),
        }
    }

    /// Check if oras is available
    pub fn check_oras() -> Result<(), GhcrError> {
        if which::which("oras").is_err() {
            return Err(GhcrError::OrasNotFound);
        }
        Ok(())
    }

    /// Login to GHCR
    pub fn login(&self) -> Result<(), GhcrError> {
        let output = Command::new("oras")
            .args(["login", &self.registry, "-u", "token", "-p", &self.token])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GhcrError::AuthFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// Push a package to GHCR
    pub fn push<P: AsRef<Path>>(
        &self,
        files: &[P],
        repository: &str,
        tag: &str,
        annotations: &PackageAnnotations,
    ) -> Result<String, GhcrError> {
        let target = format!("{}/{}:{}", self.registry, repository, tag);

        let mut cmd = Command::new("oras");
        cmd.arg("push")
            .arg("--disable-path-validation")
            .arg("--config")
            .arg("/dev/null:application/vnd.oci.empty.v1+json");

        // Add standard annotations
        let annotation_map = self.build_annotations(annotations);
        for (key, value) in &annotation_map {
            cmd.arg("--annotation").arg(format!("{}={}", key, value));
        }

        // Add target
        cmd.arg(&target);

        // Add files
        for file in files {
            let path = file.as_ref();
            if path.exists() {
                cmd.arg(path);
            }
        }

        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GhcrError::PushFailed(stderr.to_string()));
        }

        Ok(target)
    }

    /// Build OCI annotations from package metadata
    fn build_annotations(&self, meta: &PackageAnnotations) -> HashMap<String, String> {
        let mut annotations = HashMap::new();

        // Standard OCI annotations
        annotations.insert(
            "org.opencontainers.image.created".to_string(),
            meta.build_date.clone(),
        );
        if let Some(ref desc) = meta.description {
            annotations.insert("org.opencontainers.image.description".to_string(), desc.clone());
        }
        if let Some(ref license) = meta.license {
            annotations.insert("org.opencontainers.image.licenses".to_string(), license.clone());
        }
        annotations.insert(
            "org.opencontainers.image.title".to_string(),
            meta.pkg.clone(),
        );
        if let Some(ref url) = meta.homepage {
            annotations.insert("org.opencontainers.image.url".to_string(), url.clone());
        }
        annotations.insert("org.opencontainers.image.vendor".to_string(), "pkgforge".to_string());
        annotations.insert(
            "org.opencontainers.image.version".to_string(),
            meta.version.clone(),
        );

        // pkgforge-specific annotations
        annotations.insert("dev.pkgforge.soar.pkg".to_string(), meta.pkg.clone());
        annotations.insert("dev.pkgforge.soar.pkg_id".to_string(), meta.pkg_id.clone());
        if let Some(ref pkg_type) = meta.pkg_type {
            annotations.insert("dev.pkgforge.soar.pkg_type".to_string(), pkg_type.clone());
        }
        annotations.insert("dev.pkgforge.soar.version".to_string(), meta.version.clone());
        annotations.insert("dev.pkgforge.soar.push_date".to_string(), meta.build_date.clone());

        if let Some(ref build_id) = meta.build_id {
            annotations.insert("dev.pkgforge.soar.build_id".to_string(), build_id.clone());
        }
        if let Some(ref build_gha) = meta.build_gha {
            annotations.insert("dev.pkgforge.soar.build_gha".to_string(), build_gha.clone());
        }
        if let Some(ref build_script) = meta.build_script {
            annotations.insert("dev.pkgforge.soar.build_script".to_string(), build_script.clone());
        }

        annotations
    }
}

/// Generate GHCR repository path from recipe info
pub fn ghcr_path(
    cache_type: &str,
    pkg_family: &str,
    pkg_type: &str,
    source: &str,
    variant: &str,
    pkg_name: &str,
) -> String {
    format!(
        "pkgforge/{}/{}/{}/{}/{}/{}",
        cache_type, pkg_family, pkg_type, source, variant, pkg_name
    )
}

/// Generate GHCR tag from version and architecture
pub fn ghcr_tag(version: &str, arch: &str) -> String {
    format!("{}-{}", version, arch.to_lowercase())
}
