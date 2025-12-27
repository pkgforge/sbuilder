//! SBUILD recipe parsing and handling

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::{Error, Result};

/// GHCR package information with path components
#[derive(Debug, Clone)]
pub struct GhcrPackageInfo {
    /// Full GHCR repository path (e.g., "pkgforge/bincache/hello/static")
    pub ghcr_path: String,
    /// Binary/package name (e.g., "hello")
    pub pkg_name: String,
    /// Package family/directory name (e.g., "hello")
    pub pkg_family: String,
    /// Recipe name without extension (e.g., "static", "appimage.cat.stable")
    pub recipe_name: String,
    /// Cache type ("bincache" or "pkgcache")
    pub cache_type: String,
}

impl GhcrPackageInfo {
    /// Get the GHCR URL
    pub fn ghcr_url(&self) -> String {
        format!("https://ghcr.io/{}", self.ghcr_path)
    }

    /// Get the package webpage URL
    pub fn pkg_webpage(&self, arch: &str) -> String {
        let arch_lower = arch.to_lowercase();
        format!(
            "https://pkgs.pkgforge.dev/repo/{}/{}/{}/{}",
            self.cache_type, arch_lower, self.pkg_family, self.recipe_name
        )
    }
}

/// License information in a recipe - can be a string or struct
#[derive(Debug, Clone, Serialize)]
pub struct License {
    pub id: String,
    #[serde(default)]
    pub url: Option<String>,
}

impl<'de> Deserialize<'de> for License {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum LicenseHelper {
            Simple(String),
            Struct { id: String, url: Option<String> },
        }

        match LicenseHelper::deserialize(deserializer)? {
            LicenseHelper::Simple(id) => Ok(License { id, url: None }),
            LicenseHelper::Struct { id, url } => Ok(License { id, url }),
        }
    }
}

/// Flexible string that can be a simple string, a map, or a sequence
#[derive(Debug, Clone, Serialize, Default)]
pub struct FlexibleString(pub String);

impl<'de> Deserialize<'de> for FlexibleString {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Simple(String),
            Map(HashMap<String, serde_yaml::Value>),
            Seq(Vec<serde_yaml::Value>),
        }

        match Helper::deserialize(deserializer)? {
            Helper::Simple(s) => Ok(FlexibleString(s)),
            Helper::Map(m) => {
                // Try to get _default or first value
                if let Some(default) = m.get("_default") {
                    if let Some(s) = default.as_str() {
                        return Ok(FlexibleString(s.to_string()));
                    }
                }
                // Get first string value
                for (_, v) in m {
                    if let Some(s) = v.as_str() {
                        return Ok(FlexibleString(s.to_string()));
                    }
                }
                Ok(FlexibleString(String::new()))
            }
            Helper::Seq(s) => {
                // Join sequence as string
                let joined = s
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                Ok(FlexibleString(joined))
            }
        }
    }
}

/// Optional flexible value for disabled_reason
#[derive(Debug, Clone, Serialize, Default)]
pub struct FlexibleOptional(pub Option<String>);

impl<'de> Deserialize<'de> for FlexibleOptional {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            None,
            Simple(String),
            Map(HashMap<String, serde_yaml::Value>),
            Seq(Vec<serde_yaml::Value>),
        }

        match Helper::deserialize(deserializer)? {
            Helper::None => Ok(FlexibleOptional(None)),
            Helper::Simple(s) => Ok(FlexibleOptional(Some(s))),
            Helper::Map(m) => {
                let joined = m
                    .values()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                Ok(FlexibleOptional(if joined.is_empty() {
                    None
                } else {
                    Some(joined)
                }))
            }
            Helper::Seq(s) => {
                let joined = s
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                Ok(FlexibleOptional(if joined.is_empty() {
                    None
                } else {
                    Some(joined)
                }))
            }
        }
    }
}

/// Execution configuration for building packages
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExecConfig {
    /// Build system identifier (e.g., "docker://rust", "host://soar-dl")
    #[serde(default)]
    pub bsys: Option<String>,

    /// Supported host architectures
    #[serde(default)]
    pub host: Vec<String>,

    /// Shell to use for execution
    #[serde(default)]
    pub shell: Option<String>,

    /// Script to determine package version
    #[serde(default)]
    pub pkgver: Option<String>,

    /// Main build script
    #[serde(default)]
    pub run: Option<String>,
}

/// Distribution package mappings
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DistroPkg {
    #[serde(default)]
    pub alpine: Option<Vec<String>>,
    #[serde(default)]
    pub archlinux: Option<serde_yaml::Value>,
    #[serde(default)]
    pub debian: Option<Vec<String>>,
    #[serde(default)]
    pub nixpkgs: Option<Vec<String>>,
}

/// Parsed SBUILD recipe
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SBuildRecipe {
    /// Whether the recipe is disabled
    #[serde(default, rename = "_disabled")]
    pub disabled: bool,

    /// Optional reason for being disabled (can be string, map, or sequence)
    #[serde(default, rename = "_disabled_reason")]
    pub disabled_reason: FlexibleOptional,

    /// Package name
    pub pkg: String,

    /// Unique package identifier (e.g., "github.com.author.repo")
    pub pkg_id: String,

    /// Package type (static, appimage, archive, etc.)
    #[serde(default)]
    pub pkg_type: Option<String>,

    /// Explicit version (managed by bot) - supports both "version" and "pkgver" field names
    #[serde(default, alias = "version")]
    pub pkgver: Option<String>,

    /// Package categories
    #[serde(default)]
    pub category: Vec<String>,

    /// Package description (can be string or map with per-binary descriptions)
    #[serde(default)]
    pub description: FlexibleString,

    /// Distribution package mappings
    #[serde(default)]
    pub distro_pkg: Option<DistroPkg>,

    /// Homepage URLs
    #[serde(default)]
    pub homepage: Vec<String>,

    /// License information
    #[serde(default)]
    pub license: Vec<License>,

    /// Package maintainers
    #[serde(default)]
    pub maintainer: Vec<String>,

    /// Notes and warnings
    #[serde(default)]
    pub note: Vec<String>,

    /// Executables/files provided by this package
    #[serde(default)]
    pub provides: Vec<String>,

    /// Repology package names for version tracking
    #[serde(default)]
    pub repology: Vec<String>,

    /// Source URLs
    #[serde(default)]
    pub src_url: Vec<String>,

    /// Tags for categorization
    #[serde(default)]
    pub tag: Vec<String>,

    /// Execution configuration
    #[serde(default)]
    pub x_exec: Option<ExecConfig>,
}

impl SBuildRecipe {
    /// Parse a recipe from YAML content
    pub fn from_yaml(content: &str) -> Result<Self> {
        serde_yaml::from_str(content).map_err(Error::Yaml)
    }

    /// Parse a recipe from a file path
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml(&content)
    }

    /// Check if this recipe supports a given architecture
    pub fn supports_arch(&self, arch: &str) -> bool {
        match &self.x_exec {
            Some(exec) => exec.host.iter().any(|h| h == arch),
            None => false,
        }
    }

    /// Get the pkgver script if available
    pub fn pkgver_script(&self) -> Option<&str> {
        self.x_exec.as_ref()?.pkgver.as_deref()
    }

    /// Check if recipe is disabled
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Get the GHCR package path for this recipe (simple version)
    pub fn ghcr_package(&self) -> String {
        format!("bincache/{}", self.pkg)
    }

    /// Extract unique package names from provides field
    ///
    /// Handles various formats:
    /// - "prog" -> prog (separate package)
    /// - "prog:alias" -> prog (alias for search)
    /// - "prog==symlink" -> prog (symlink at install)
    /// - "prog=>renamed" -> prog (rename at install)
    ///
    /// Returns deduplicated list of base package names
    pub fn get_provided_packages(&self) -> Vec<String> {
        if self.provides.is_empty() {
            return vec![self.pkg.clone()];
        }

        let mut seen = std::collections::HashSet::new();
        let mut packages = Vec::new();

        for entry in &self.provides {
            // Extract base name before any separator (:, ==, =>)
            let base = entry
                .split("=>")
                .next()
                .unwrap_or(entry)
                .split("==")
                .next()
                .unwrap_or(entry)
                .split(':')
                .next()
                .unwrap_or(entry)
                .to_string();

            if !base.is_empty() && seen.insert(base.clone()) {
                packages.push(base);
            }
        }

        if packages.is_empty() {
            vec![self.pkg.clone()]
        } else {
            packages
        }
    }

    /// GHCR package information including path components
    pub fn ghcr_packages_from_path(&self, recipe_path: &Path, ghcr_owner: &str) -> Vec<GhcrPackageInfo> {
        let mut packages = Vec::new();

        // Determine cache type based on path (bincache for binaries/, pkgcache for packages/)
        let path_str = recipe_path.to_string_lossy();
        let is_pkgcache = path_str.contains("/packages/") || path_str.starts_with("packages/");
        let cache_type = if is_pkgcache { "pkgcache" } else { "bincache" };

        // Get parent directory name (e.g., "hello" from "binaries/hello/static.yaml")
        let pkg_family = recipe_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or(&self.pkg)
            .to_string();

        // Get recipe name (filename without .yaml extension)
        // e.g., "static" from "static.yaml" or "appimage.cat.stable" from "appimage.cat.stable.yaml"
        let recipe_name = recipe_path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("static")
            .to_string();

        // Get unique package names from provides
        let provided_packages = self.get_provided_packages();

        // Generate GHCR paths for each package
        for pkg_name in provided_packages {
            // GHCR path: {owner}/{cache}/{pkg_family}/{recipe_name}
            // e.g., pkgforge/bincache/hello/static
            let ghcr_path = format!(
                "{}/{}/{}/{}",
                ghcr_owner, cache_type, pkg_family, recipe_name
            );

            packages.push(GhcrPackageInfo {
                ghcr_path,
                pkg_name,
                pkg_family: pkg_family.clone(),
                recipe_name: recipe_name.clone(),
                cache_type: cache_type.to_string(),
            });
        }

        packages
    }

    /// Get the build script URL
    pub fn build_script_url(&self) -> String {
        format!(
            "https://raw.githubusercontent.com/pkgforge/soarpkgs/refs/heads/main/binaries/{}/",
            self.pkg
        )
    }
}

/// Scan a directory for SBUILD recipes
pub fn scan_recipes(dir: &Path) -> Result<Vec<(std::path::PathBuf, SBuildRecipe)>> {
    let pattern = dir.join("**/*.yaml");
    let pattern_str = pattern.to_string_lossy();

    let mut recipes = Vec::new();

    for entry in glob::glob(&pattern_str)? {
        match entry {
            Ok(path) => {
                // Skip non-SBUILD yaml files
                if !path
                    .file_name()
                    .map(|n| n.to_string_lossy().contains(".yaml"))
                    .unwrap_or(false)
                {
                    continue;
                }

                match SBuildRecipe::from_file(&path) {
                    Ok(recipe) => recipes.push((path, recipe)),
                    Err(e) => {
                        tracing::warn!("Failed to parse recipe {:?}: {}", path, e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Glob error: {}", e);
            }
        }
    }

    Ok(recipes)
}

/// Filter recipes by architecture
pub fn filter_by_arch(
    recipes: Vec<(std::path::PathBuf, SBuildRecipe)>,
    arch: &str,
) -> Vec<(std::path::PathBuf, SBuildRecipe)> {
    recipes
        .into_iter()
        .filter(|(_, recipe)| recipe.supports_arch(arch))
        .collect()
}

/// Filter out disabled recipes
pub fn filter_enabled(
    recipes: Vec<(std::path::PathBuf, SBuildRecipe)>,
) -> Vec<(std::path::PathBuf, SBuildRecipe)> {
    recipes
        .into_iter()
        .filter(|(_, recipe)| !recipe.is_disabled())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_recipe() {
        let yaml = r#"
pkg: test
pkg_id: example.com.test
description: A test package
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        assert_eq!(recipe.pkg, "test");
        assert_eq!(recipe.pkg_id, "example.com.test");
    }

    #[test]
    fn test_parse_with_version() {
        let yaml = r#"
pkg: test
pkg_id: example.com.test
version: "1.2.3"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        assert_eq!(recipe.pkgver, Some("1.2.3".to_string()));
    }

    #[test]
    fn test_supports_arch() {
        let yaml = r#"
pkg: test
pkg_id: example.com.test
x_exec:
  host:
    - "x86_64-Linux"
    - "aarch64-Linux"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        assert!(recipe.supports_arch("x86_64-Linux"));
        assert!(recipe.supports_arch("aarch64-Linux"));
        assert!(!recipe.supports_arch("riscv64-Linux"));
    }

    #[test]
    fn test_ghcr_packages_from_path() {
        let yaml = r#"
pkg: batcat
pkg_id: github.com.sharkdp.bat
provides:
  - "bat==batcat"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let path = Path::new("binaries/bat/static.yaml");
        let packages = recipe.ghcr_packages_from_path(path, "pkgforge");

        // bat==batcat means bat is the package, batcat is symlink - only 1 entry
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].ghcr_path, "pkgforge/bincache/bat/static");
        assert_eq!(packages[0].pkg_name, "bat");
        assert_eq!(packages[0].recipe_name, "static");
    }

    #[test]
    fn test_ghcr_packages_multiple_binaries() {
        let yaml = r#"
pkg: myapp
pkg_id: example.com.myapp
provides:
  - "app1"
  - "app2"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let path = Path::new("binaries/myapp/static.yaml");
        let packages = recipe.ghcr_packages_from_path(path, "pkgforge");

        // Two separate packages - but same GHCR path (they're in the same recipe)
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].ghcr_path, "pkgforge/bincache/myapp/static");
        assert_eq!(packages[1].ghcr_path, "pkgforge/bincache/myapp/static");
    }

    #[test]
    fn test_ghcr_packages_with_symlinks_dedup() {
        let yaml = r#"
pkg: busybox
pkg_id: example.com.busybox
provides:
  - "busybox==whoami"
  - "busybox==ls"
  - "busybox:bbox"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let path = Path::new("binaries/busybox/static.yaml");
        let packages = recipe.ghcr_packages_from_path(path, "pkgforge");

        // All entries refer to busybox - should deduplicate to 1
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].ghcr_path, "pkgforge/bincache/busybox/static");
        assert_eq!(packages[0].pkg_name, "busybox");
    }

    #[test]
    fn test_ghcr_packages_pkgcache() {
        let yaml = r#"
pkg: 0ad
pkg_id: io.github.0ad
provides:
  - "0ad"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let path = Path::new("packages/0ad/appimage.0ad-matters.stable.yaml");
        let packages = recipe.ghcr_packages_from_path(path, "pkgforge");

        assert_eq!(packages.len(), 1);
        // New simplified format: {owner}/{cache}/{pkg_family}/{recipe_name}
        assert_eq!(packages[0].ghcr_path, "pkgforge/pkgcache/0ad/appimage.0ad-matters.stable");
        assert_eq!(packages[0].pkg_name, "0ad");
        assert_eq!(packages[0].cache_type, "pkgcache");
        assert_eq!(packages[0].recipe_name, "appimage.0ad-matters.stable");
    }

    #[test]
    fn test_get_provided_packages() {
        let yaml = r#"
pkg: test
pkg_id: example.com.test
provides:
  - "prog-a"
  - "prog-b:alias"
  - "prog-b==symlink"
  - "prog-c=>renamed"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let packages = recipe.get_provided_packages();

        // prog-a, prog-b (deduped), prog-c
        assert_eq!(packages.len(), 3);
        assert!(packages.contains(&"prog-a".to_string()));
        assert!(packages.contains(&"prog-b".to_string()));
        assert!(packages.contains(&"prog-c".to_string()));
    }
}
