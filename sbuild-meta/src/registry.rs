//! GHCR/OCI Registry client
//!
//! Provides functionality to interact with GitHub Container Registry
//! for fetching manifests, tags, and package metadata.

use std::cmp::Ordering;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use serde::Deserialize;

use crate::{Error, Result};

const GHCR_API_BASE: &str = "https://ghcr.io/v2";

/// Tag list response from registry
#[derive(Debug, Deserialize)]
pub struct TagList {
    pub name: String,
    pub tags: Vec<String>,
}

/// OCI registry client
#[derive(Clone)]
pub struct RegistryClient {
    client: reqwest::Client,
}

impl RegistryClient {
    /// Create a new registry client
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("sbuild-meta/0.1.0")
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Build headers for registry requests
    /// Uses anonymous bearer token (QQ== = base64 of "A") for public repos
    fn build_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer QQ=="));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "application/vnd.docker.distribution.manifest.v2+json, \
                 application/vnd.docker.distribution.manifest.list.v2+json, \
                 application/vnd.oci.image.manifest.v1+json, \
                 application/vnd.oci.image.index.v1+json, \
                 application/vnd.oci.artifact.manifest.v1+json",
            ),
        );
        headers
    }

    /// List tags for a repository
    pub async fn list_tags(&self, repository: &str) -> Result<TagList> {
        let url = format!("{}/{}/tags/list", GHCR_API_BASE, repository);

        let response = self
            .client
            .get(&url)
            .headers(Self::build_headers())
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::Registry(format!(
                "Failed to list tags for {}: {}",
                repository,
                response.status()
            )));
        }

        response.json().await.map_err(Error::Http)
    }

    /// Filter tags for a specific architecture (case-insensitive)
    pub fn filter_tags_by_arch<'a>(tags: &'a [String], arch: &str) -> Vec<&'a String> {
        let arch_lower = arch.to_lowercase();
        tags.iter()
            .filter(|tag| {
                let tag_lower = tag.to_lowercase();
                // Filter out srcbuild tags and match architecture
                !tag_lower.contains("srcbuild") && tag_lower.contains(&arch_lower)
            })
            .collect()
    }

    /// Get the latest tag for an architecture using version-aware comparison
    pub fn get_latest_arch_tag<'a>(tags: &'a [String], arch: &str) -> Option<&'a String> {
        Self::filter_tags_by_arch(tags, arch)
            .into_iter()
            .filter(|t| !t.starts_with("latest"))
            .max_by(|a, b| version_compare_tags(a, b, arch))
    }

    /// Fetch manifest for a specific tag
    pub async fn fetch_manifest(&self, repository: &str, tag: &str) -> Result<String> {
        let url = format!("{}/{}/manifests/{}", GHCR_API_BASE, repository, tag);

        let response = self
            .client
            .get(&url)
            .headers(Self::build_headers())
            .send()
            .await?;

        if response.status().as_u16() == 404 {
            return Err(Error::ManifestNotFound(format!("{}:{}", repository, tag)));
        }

        if !response.status().is_success() {
            return Err(Error::Registry(format!(
                "Failed to fetch manifest for {}:{}: {}",
                repository,
                tag,
                response.status()
            )));
        }

        response.text().await.map_err(Error::Http)
    }

    /// Fetch manifest as parsed JSON
    pub async fn fetch_manifest_json(
        &self,
        repository: &str,
        tag: &str,
    ) -> Result<serde_json::Value> {
        let manifest_str = self.fetch_manifest(repository, tag).await?;
        serde_json::from_str(&manifest_str).map_err(Error::Json)
    }

    /// Check if a package exists in the registry
    pub async fn package_exists(&self, repository: &str) -> bool {
        match self.list_tags(repository).await {
            Ok(tags) => !tags.tags.is_empty(),
            Err(_) => false,
        }
    }

    /// Get download URL for a package
    pub fn get_download_url(repository: &str, tag: &str, filename: &str) -> String {
        format!(
            "https://api.ghcr.pkgforge.dev/{}?tag={}&download={}",
            repository, tag, filename
        )
    }

    /// Get GHCR web URL for a package
    pub fn get_ghcr_url(repository: &str) -> String {
        format!(
            "https://github.com/pkgforge/{}/pkgs/container/{}",
            repository.split('/').next().unwrap_or("bincache"),
            repository.split('/').last().unwrap_or(repository)
        )
    }
}

impl Default for RegistryClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract the version portion from a tag by stripping the `-{arch}` suffix.
/// e.g. "v1.2.3-x86_64-linux" -> "v1.2.3", "2026.2.23-aarch64-linux" -> "2026.2.23"
fn extract_version_from_tag<'a>(tag: &'a str, arch: &str) -> &'a str {
    let arch_suffix = format!("-{}", arch);
    tag.strip_suffix(&arch_suffix)
        .or_else(|| {
            // case-insensitive fallback
            let lower = tag.to_lowercase();
            let suffix_lower = arch_suffix.to_lowercase();
            if lower.ends_with(&suffix_lower) {
                Some(&tag[..tag.len() - arch_suffix.len()])
            } else {
                None
            }
        })
        .unwrap_or(tag)
}

/// Parse a version string into a semver::Version, handling common non-semver formats.
/// Strips leading 'v'/'V', handles calver (2026.2.23), release suffixes (-r1), etc.
fn parse_version_lenient(version: &str) -> Option<semver::Version> {
    let v = version
        .strip_prefix('v')
        .or(version.strip_prefix('V'))
        .unwrap_or(version);

    // Check for -rN release revision suffix first (e.g. "0.16.1-r2")
    // These should sort HIGHER than the base version, but semver treats
    // pre-release as LOWER, so we handle them specially.
    let (base_str, revision) = extract_release_revision(v);

    // Split on first '-' to separate version from extra info (dates, hashes, etc.)
    let (numeric_base, extra) = match base_str.find('-') {
        Some(idx) => (&base_str[..idx], Some(&base_str[idx + 1..])),
        None => (base_str, None),
    };

    // Split base into numeric parts
    let parts: Vec<u64> = numeric_base
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();

    if parts.is_empty() {
        return None;
    }

    let major = parts[0];
    let minor = if parts.len() > 1 { parts[1] } else { 0 };
    let mut patch = if parts.len() > 2 { parts[2] } else { 0 };

    // Bump patch by revision so r2 > r1 > base
    if let Some(rev) = revision {
        patch += rev;
    }

    let mut ver = semver::Version::new(major, minor, patch);

    // Any extra info (dates, hashes) goes into pre-release so it sorts LOWER
    // than the clean version.
    if let Some(extra_str) = extra {
        let normalized: String = extra_str
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '.' {
                    c
                } else {
                    '.'
                }
            })
            .collect();
        if let Ok(pre) = semver::Prerelease::new(&normalized) {
            ver.pre = pre;
        }
    }

    Some(ver)
}

/// Extract a trailing -rN release revision from a version string.
/// Returns (base_without_rN, Some(N)) or (original, None).
fn extract_release_revision(v: &str) -> (&str, Option<u64>) {
    if let Some(idx) = v.rfind("-r") {
        let after = &v[idx + 2..];
        if let Ok(rev) = after.parse::<u64>() {
            return (&v[..idx], Some(rev));
        }
    }
    (v, None)
}

/// Compare two tags by their version, using semver-aware sorting.
fn version_compare_tags(a: &str, b: &str, arch: &str) -> Ordering {
    let ver_a = extract_version_from_tag(a, arch);
    let ver_b = extract_version_from_tag(b, arch);

    match (parse_version_lenient(ver_a), parse_version_lenient(ver_b)) {
        (Some(va), Some(vb)) => va.cmp(&vb),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => a.cmp(b), // fallback to lexicographic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_tags_by_arch() {
        let tags = vec![
            "latest".to_string(),
            "srcbuild-20241227".to_string(),
            "v1.0.0-x86_64-linux".to_string(),
            "v1.0.0-aarch64-linux".to_string(),
            "v1.1.0-x86_64-linux".to_string(),
        ];

        let x86_tags = RegistryClient::filter_tags_by_arch(&tags, "x86_64-linux");
        assert_eq!(x86_tags.len(), 2);

        let arm_tags = RegistryClient::filter_tags_by_arch(&tags, "aarch64-linux");
        assert_eq!(arm_tags.len(), 1);
    }

    #[test]
    fn test_get_latest_arch_tag() {
        let tags = vec![
            "v1.0.0-x86_64-linux".to_string(),
            "v1.1.0-x86_64-linux".to_string(),
            "v1.0.5-x86_64-linux".to_string(),
        ];

        let latest = RegistryClient::get_latest_arch_tag(&tags, "x86_64-linux");
        assert_eq!(latest, Some(&"v1.1.0-x86_64-linux".to_string()));
    }

    #[test]
    fn test_get_latest_calver() {
        let tags = vec![
            "2026.1.8-x86_64-linux".to_string(),
            "2026.1.12-x86_64-linux".to_string(),
            "2026.2.9-x86_64-linux".to_string(),
            "2026.2.17-x86_64-linux".to_string(),
            "2026.2.23-x86_64-linux".to_string(),
        ];

        let latest = RegistryClient::get_latest_arch_tag(&tags, "x86_64-linux");
        assert_eq!(latest, Some(&"2026.2.23-x86_64-linux".to_string()));
    }

    #[test]
    fn test_get_latest_with_old_date_tags() {
        let tags = vec![
            "v0.16.1-2026-01-01_1767256526-x86_64-linux".to_string(),
            "0.16.1-v0.16.1-2026-01-22_1769071133-x86_64-linux".to_string(),
            "0.16.1-x86_64-linux".to_string(),
            "0.16.1-r1-x86_64-linux".to_string(),
            "0.16.1-r2-x86_64-linux".to_string(),
        ];

        let latest = RegistryClient::get_latest_arch_tag(&tags, "x86_64-linux");
        assert_eq!(latest, Some(&"0.16.1-r2-x86_64-linux".to_string()));
    }

    #[test]
    fn test_get_latest_release_revisions() {
        let tags = vec![
            "1.0.0-x86_64-linux".to_string(),
            "1.0.0-r1-x86_64-linux".to_string(),
            "1.0.0-r2-x86_64-linux".to_string(),
        ];

        let latest = RegistryClient::get_latest_arch_tag(&tags, "x86_64-linux");
        assert_eq!(latest, Some(&"1.0.0-r2-x86_64-linux".to_string()));
    }

    #[test]
    fn test_version_compare_v_prefix_vs_no_prefix() {
        let tags = vec![
            "v0.16.1-x86_64-linux".to_string(),
            "0.16.2-x86_64-linux".to_string(),
        ];

        let latest = RegistryClient::get_latest_arch_tag(&tags, "x86_64-linux");
        assert_eq!(latest, Some(&"0.16.2-x86_64-linux".to_string()));
    }

    #[test]
    fn test_two_component_version() {
        let tags = vec![
            "1.5-x86_64-linux".to_string(),
            "1.12-x86_64-linux".to_string(),
        ];

        let latest = RegistryClient::get_latest_arch_tag(&tags, "x86_64-linux");
        assert_eq!(latest, Some(&"1.12-x86_64-linux".to_string()));
    }

    #[test]
    fn test_download_url() {
        let url = RegistryClient::get_download_url(
            "pkgforge/bincache/bat",
            "v0.24.0-x86_64-linux",
            "bat",
        );
        assert!(url.contains("api.ghcr.pkgforge.dev"));
        assert!(url.contains("tag=v0.24.0-x86_64-linux"));
    }
}
