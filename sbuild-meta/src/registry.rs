//! GHCR/OCI Registry client
//!
//! Provides functionality to interact with GitHub Container Registry
//! for fetching manifests, tags, and package metadata.

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

/// GitHub package info response
#[derive(Debug, Deserialize)]
pub struct GitHubPackageInfo {
    pub name: String,
    #[serde(default)]
    pub download_count: u64,
}

/// OCI registry client
#[derive(Clone)]
pub struct RegistryClient {
    client: reqwest::Client,
    github_token: Option<String>,
}

impl RegistryClient {
    /// Create a new registry client
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("sbuild-meta/0.1.0")
                .build()
                .expect("Failed to create HTTP client"),
            github_token: std::env::var("GITHUB_TOKEN").ok(),
        }
    }

    /// Create a new registry client with a specific GitHub token
    pub fn with_github_token(token: String) -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("sbuild-meta/0.1.0")
                .build()
                .expect("Failed to create HTTP client"),
            github_token: Some(token),
        }
    }

    /// Build headers for registry requests
    /// Uses anonymous bearer token (QQ== = base64 of "A") for public repos
    fn build_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer QQ=="),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "application/vnd.docker.distribution.manifest.v2+json, \
                 application/vnd.docker.distribution.manifest.list.v2+json, \
                 application/vnd.oci.image.manifest.v1+json, \
                 application/vnd.oci.image.index.v1+json, \
                 application/vnd.oci.artifact.manifest.v1+json"
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

    /// Get the latest tag for an architecture
    pub fn get_latest_arch_tag<'a>(tags: &'a [String], arch: &str) -> Option<&'a String> {
        Self::filter_tags_by_arch(tags, arch)
            .into_iter()
            .filter(|t| !t.starts_with("latest"))
            .max_by(|a, b| a.cmp(b)) // Version sort
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
            return Err(Error::ManifestNotFound(format!(
                "{}:{}",
                repository, tag
            )));
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
        format!("https://github.com/pkgforge/{}/pkgs/container/{}",
            repository.split('/').next().unwrap_or("bincache"),
            repository.split('/').last().unwrap_or(repository)
        )
    }

    /// Fetch download count for a package from GitHub API
    /// Repository format: "owner/repo/package" (e.g., "pkgforge/bincache/bat/static/bat")
    pub async fn fetch_download_count(&self, repository: &str) -> Result<u64> {
        let token = self.github_token.as_ref()
            .ok_or_else(|| Error::Registry("GitHub token required for download counts".into()))?;

        // Parse repository: "pkgforge/bincache/hello/static/hello" -> owner=pkgforge, package=bincache/hello/static/hello
        let parts: Vec<&str> = repository.splitn(2, '/').collect();
        if parts.len() < 2 {
            return Err(Error::Registry(format!("Invalid repository format: {}", repository)));
        }

        let owner = parts[0];
        let package_name = parts[1].replace('/', "%2F"); // URL encode slashes

        // Use GitHub packages API
        let url = format!(
            "https://api.github.com/orgs/{}/packages/container/{}",
            owner, package_name
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token))
                .map_err(|_| Error::Registry("Invalid token".into()))?,
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("x-github-api-version"),
            HeaderValue::from_static("2022-11-28"),
        );

        let response = self.client
            .get(&url)
            .headers(headers)
            .send()
            .await?;

        if !response.status().is_success() {
            // Try user packages API as fallback
            let user_url = format!(
                "https://api.github.com/users/{}/packages/container/{}",
                owner, package_name
            );

            let mut headers = HeaderMap::new();
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", token))
                    .map_err(|_| Error::Registry("Invalid token".into()))?,
            );
            headers.insert(
                ACCEPT,
                HeaderValue::from_static("application/vnd.github+json"),
            );

            let response = self.client
                .get(&user_url)
                .headers(headers)
                .send()
                .await?;

            if !response.status().is_success() {
                return Ok(0); // Return 0 if we can't get download count
            }

            let info: GitHubPackageInfo = response.json().await.map_err(Error::Http)?;
            return Ok(info.download_count);
        }

        let info: GitHubPackageInfo = response.json().await.map_err(Error::Http)?;
        Ok(info.download_count)
    }
}

impl Default for RegistryClient {
    fn default() -> Self {
        Self::new()
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
            "v1.0.0-x86_64-Linux".to_string(),
            "v1.0.0-aarch64-Linux".to_string(),
            "v1.1.0-x86_64-Linux".to_string(),
        ];

        let x86_tags = RegistryClient::filter_tags_by_arch(&tags, "x86_64-Linux");
        assert_eq!(x86_tags.len(), 2);

        let arm_tags = RegistryClient::filter_tags_by_arch(&tags, "aarch64-Linux");
        assert_eq!(arm_tags.len(), 1);
    }

    #[test]
    fn test_get_latest_arch_tag() {
        let tags = vec![
            "v1.0.0-x86_64-Linux".to_string(),
            "v1.1.0-x86_64-Linux".to_string(),
            "v1.0.5-x86_64-Linux".to_string(),
        ];

        let latest = RegistryClient::get_latest_arch_tag(&tags, "x86_64-Linux");
        assert_eq!(latest, Some(&"v1.1.0-x86_64-Linux".to_string()));
    }

    #[test]
    fn test_download_url() {
        let url = RegistryClient::get_download_url(
            "pkgforge/bincache/bat",
            "v0.24.0-x86_64-Linux",
            "bat",
        );
        assert!(url.contains("api.ghcr.pkgforge.dev"));
        assert!(url.contains("tag=v0.24.0-x86_64-Linux"));
    }
}
