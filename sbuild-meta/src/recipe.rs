//! SBUILD recipe parsing and handling

use saphyr::{LoadableYamlNode, YamlOwned};
use std::path::Path;

use crate::{Error, Result};

/// Sanitize a name to be OCI repository name compliant
/// OCI repository names must be lowercase and only contain [a-z0-9._-]
pub fn sanitize_oci_name(name: &str) -> String {
    // First replace ++ with pp (common convention: c++ -> cpp, g++ -> gpp)
    let name = name.replace("++", "pp");

    name.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '-' // Replace other invalid chars with -
            }
        })
        .collect::<String>()
        // Remove consecutive dashes
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

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
            "https://pkgs.pkgforge.dev/repo/soarpkgs/{}/{}/{}/{}",
            arch_lower, self.pkg_family, self.recipe_name, self.pkg_name
        )
    }
}

/// Per-package configuration for multi-package recipes
#[derive(Debug, Clone, Default)]
pub struct PackageConfig {
    pub provides: Vec<String>,
}

/// Execution configuration for building packages
#[derive(Debug, Clone, Default)]
pub struct ExecConfig {
    pub host: Vec<String>,
    pub arch: Vec<String>,
    pub os: Vec<String>,
    pub shell: Option<String>,
    pub pkgver: Option<String>,
    pub run: Option<String>,
}

/// Parsed SBUILD recipe
#[derive(Debug, Clone, Default)]
pub struct SBuildRecipe {
    pub disabled: bool,
    pub pkg: String,
    pub pkg_id: String,
    pub pkg_type: Option<String>,
    pub pkgver: Option<String>,
    pub remote_pkgver: Option<String>,
    pub category: Vec<String>,
    pub description: String,
    pub homepage: Vec<String>,
    pub license: Vec<String>,
    pub maintainer: Vec<String>,
    pub note: Vec<String>,
    pub provides: Vec<String>,
    pub packages: Vec<(String, PackageConfig)>,
    pub repology: Vec<String>,
    pub src_url: Vec<String>,
    pub tag: Vec<String>,
    pub snapshots: Vec<String>,
    pub ghcr_pkg: Option<String>,
    pub x_exec: Option<ExecConfig>,
}

fn get_str(yaml: &YamlOwned, key: &str) -> Option<String> {
    yaml.as_mapping_get(key)
        .and_then(|v: &YamlOwned| v.as_str())
        .map(|s| s.to_string())
}

fn get_bool(yaml: &YamlOwned, key: &str) -> Option<bool> {
    yaml.as_mapping_get(key)
        .and_then(|v: &YamlOwned| v.as_bool())
}

fn get_string_vec(yaml: &YamlOwned, key: &str) -> Vec<String> {
    yaml.as_mapping_get(key)
        .and_then(|v: &YamlOwned| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v: &YamlOwned| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn get_description(yaml: &YamlOwned) -> String {
    let Some(desc) = yaml.as_mapping_get("description") else {
        return String::new();
    };
    if let Some(s) = desc.as_str() {
        return s.to_string();
    }
    if let Some(map) = desc.as_mapping() {
        // Try _default first, then first value
        for (k, v) in map {
            if k.as_str() == Some("_default") {
                if let Some(s) = v.as_str() {
                    return s.to_string();
                }
            }
        }
        for (_, v) in map {
            if let Some(s) = v.as_str() {
                return s.to_string();
            }
        }
    }
    if let Some(seq) = desc.as_sequence() {
        return seq
            .iter()
            .filter_map(|v: &YamlOwned| v.as_str())
            .collect::<Vec<_>>()
            .join("; ");
    }
    String::new()
}

fn parse_exec_config(yaml: &YamlOwned) -> Option<ExecConfig> {
    let exec = yaml.as_mapping_get("x_exec")?;
    Some(ExecConfig {
        host: get_string_vec(exec, "host"),
        arch: get_string_vec(exec, "arch"),
        os: get_string_vec(exec, "os"),
        shell: get_str(exec, "shell"),
        pkgver: get_str(exec, "pkgver"),
        run: get_str(exec, "run"),
    })
}

fn parse_packages(yaml: &YamlOwned) -> Vec<(String, PackageConfig)> {
    let Some(pkgs) = yaml.as_mapping_get("packages") else {
        return Vec::new();
    };
    let Some(mapping) = pkgs.as_mapping() else {
        return Vec::new();
    };
    mapping
        .iter()
        .filter_map(|(k, v)| {
            let name = k.as_str()?.to_string();
            let provides = get_string_vec(v, "provides");
            Some((name, PackageConfig { provides }))
        })
        .collect()
}

impl SBuildRecipe {
    /// Parse a recipe from YAML content
    pub fn from_yaml(content: &str) -> Result<Self> {
        let docs = YamlOwned::load_from_str(content).map_err(|e| Error::Yaml(e.to_string()))?;
        let yaml = docs
            .into_iter()
            .next()
            .ok_or_else(|| Error::Yaml("Empty YAML document".into()))?;

        Ok(Self {
            disabled: get_bool(&yaml, "_disabled").unwrap_or(false),
            pkg: get_str(&yaml, "pkg").unwrap_or_default(),
            pkg_id: get_str(&yaml, "pkg_id").unwrap_or_default(),
            pkg_type: get_str(&yaml, "pkg_type"),
            pkgver: get_str(&yaml, "pkgver").or_else(|| get_str(&yaml, "version")),
            remote_pkgver: get_str(&yaml, "remote_pkgver"),
            category: get_string_vec(&yaml, "category"),
            description: get_description(&yaml),
            homepage: get_string_vec(&yaml, "homepage"),
            license: get_string_vec(&yaml, "license"),
            maintainer: get_string_vec(&yaml, "maintainer"),
            note: get_string_vec(&yaml, "note"),
            provides: get_string_vec(&yaml, "provides"),
            packages: parse_packages(&yaml),
            repology: get_string_vec(&yaml, "repology"),
            src_url: get_string_vec(&yaml, "src_url"),
            tag: get_string_vec(&yaml, "tag"),
            snapshots: get_string_vec(&yaml, "snapshots"),
            ghcr_pkg: get_str(&yaml, "ghcr_pkg"),
            x_exec: parse_exec_config(&yaml),
        })
    }

    /// Parse a recipe from a file path
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml(&content)
    }

    /// Check if this recipe supports a given architecture
    pub fn supports_arch(&self, arch: &str) -> bool {
        match &self.x_exec {
            Some(exec) => {
                // If host is specified, only check host (not arch/os)
                if !exec.host.is_empty() {
                    return exec.host.iter().any(|h| h.eq_ignore_ascii_case(arch));
                }

                // Parse arch string like "x86_64-linux" into arch and os parts
                let parts: Vec<&str> = arch.split('-').collect();
                if parts.len() >= 2 {
                    let target_arch = parts[0];
                    let target_os = parts[1];

                    let arch_match = exec.arch.is_empty()
                        || exec
                            .arch
                            .iter()
                            .any(|a| a.eq_ignore_ascii_case(target_arch));

                    let os_match = exec.os.is_empty()
                        || exec.os.iter().any(|o| o.eq_ignore_ascii_case(target_os));

                    return arch_match && os_match;
                }

                false
            }
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
        self.pkg.clone()
    }

    /// Extract unique package names from provides or packages field
    pub fn get_provided_packages(&self) -> Vec<String> {
        // If packages field is set, use its keys as package names
        if !self.packages.is_empty() {
            return self.packages.iter().map(|(name, _)| name.clone()).collect();
        }

        if self.provides.is_empty() {
            return vec![self.pkg.clone()];
        }

        let mut seen = std::collections::HashSet::new();
        let mut packages = Vec::new();

        for entry in &self.provides {
            // Skip binary-only entries (starting with @)
            if entry.starts_with('@') {
                continue;
            }

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

    /// Extract binary-only entries from provides field
    pub fn get_binaries(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut binaries = Vec::new();

        for entry in &self.provides {
            if let Some(bin_name) = entry.strip_prefix('@') {
                let bin_name = bin_name.to_string();
                if !bin_name.is_empty() && seen.insert(bin_name.clone()) {
                    binaries.push(bin_name);
                }
            }
        }

        binaries
    }

    /// Get the provides list for a specific package (from the packages field)
    pub fn get_package_provides(&self, pkg_name: &str) -> Option<&[String]> {
        self.packages
            .iter()
            .find(|(name, _)| name == pkg_name)
            .map(|(_, config)| config.provides.as_slice())
    }

    /// Check if this recipe uses the multi-package layout
    pub fn has_packages(&self) -> bool {
        !self.packages.is_empty()
    }

    /// GHCR package information including path components
    pub fn ghcr_packages_from_path(
        &self,
        recipe_path: &Path,
        ghcr_owner: &str,
    ) -> Vec<GhcrPackageInfo> {
        let mut packages = Vec::new();

        let pkg_family = recipe_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or(&self.pkg)
            .to_string();

        let recipe_name = recipe_path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("static")
            .to_string();

        let provided_packages = self.get_provided_packages();

        for pkg_name in provided_packages {
            let sanitized_pkg_name = sanitize_oci_name(&pkg_name);
            let ghcr_path = if let Some(ref custom_base) = self.ghcr_pkg {
                format!("{}/{}/{}", ghcr_owner, custom_base, sanitized_pkg_name)
            } else {
                format!(
                    "{}/{}/{}/{}",
                    ghcr_owner, pkg_family, recipe_name, sanitized_pkg_name
                )
            };

            packages.push(GhcrPackageInfo {
                ghcr_path,
                pkg_name,
                pkg_family: pkg_family.clone(),
                recipe_name: recipe_name.clone(),
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
                        log::warn!("Failed to parse recipe {:?}: {}", path, e);
                    }
                }
            }
            Err(e) => {
                log::warn!("Glob error: {}", e);
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
    - "x86_64-linux"
    - "aarch64-linux"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        assert!(recipe.supports_arch("x86_64-linux"));
        assert!(recipe.supports_arch("aarch64-linux"));
        assert!(!recipe.supports_arch("riscv64-linux"));
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

        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].ghcr_path, "pkgforge/bat/static/bat");
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

        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].ghcr_path, "pkgforge/myapp/static/app1");
        assert_eq!(packages[1].ghcr_path, "pkgforge/myapp/static/app2");
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

        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].ghcr_path, "pkgforge/busybox/static/busybox");
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
        assert_eq!(
            packages[0].ghcr_path,
            "pkgforge/0ad/appimage.0ad-matters.stable/0ad"
        );
        assert_eq!(packages[0].pkg_name, "0ad");
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

        assert_eq!(packages.len(), 3);
        assert!(packages.contains(&"prog-a".to_string()));
        assert!(packages.contains(&"prog-b".to_string()));
        assert!(packages.contains(&"prog-c".to_string()));
    }

    #[test]
    fn test_ghcr_packages_custom_ghcr_pkg() {
        let yaml = r#"
pkg: myapp
pkg_id: example.com.myapp
ghcr_pkg: "custom-cache/custom-pkg"
provides:
  - "app1"
  - "app2"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let path = Path::new("binaries/myapp/static.yaml");
        let packages = recipe.ghcr_packages_from_path(path, "pkgforge");

        assert_eq!(packages.len(), 2);
        assert_eq!(
            packages[0].ghcr_path,
            "pkgforge/custom-cache/custom-pkg/app1"
        );
        assert_eq!(
            packages[1].ghcr_path,
            "pkgforge/custom-cache/custom-pkg/app2"
        );
    }

    #[test]
    fn test_sanitize_oci_name() {
        assert_eq!(sanitize_oci_name("hello"), "hello");
        assert_eq!(sanitize_oci_name("Hello"), "hello");
        assert_eq!(sanitize_oci_name("c++filt"), "cppfilt");
        assert_eq!(sanitize_oci_name("g++"), "gpp");
        assert_eq!(sanitize_oci_name("clang++"), "clangpp");
        assert_eq!(sanitize_oci_name("foo@bar"), "foo-bar");
        assert_eq!(sanitize_oci_name("foo#bar"), "foo-bar");
        assert_eq!(sanitize_oci_name("ld.gold"), "ld.gold");
        assert_eq!(sanitize_oci_name("my_app"), "my_app");
    }

    #[test]
    fn test_ghcr_packages_with_special_chars() {
        let yaml = r#"
pkg: binutils
pkg_id: gnu.org.binutils
provides:
  - "c++filt"
  - "ld.gold"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let path = Path::new("binaries/binutils/static.yaml");
        let packages = recipe.ghcr_packages_from_path(path, "pkgforge");

        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].ghcr_path, "pkgforge/binutils/static/cppfilt");
        assert_eq!(packages[0].pkg_name, "c++filt");
        assert_eq!(packages[1].ghcr_path, "pkgforge/binutils/static/ld.gold");
        assert_eq!(packages[1].pkg_name, "ld.gold");
    }

    #[test]
    fn test_get_provided_packages_excludes_binaries() {
        let yaml = r#"
pkg: yazi
pkg_id: github.com.sxyazi.yazi
provides:
  - "yazi"
  - "@ya"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let packages = recipe.get_provided_packages();

        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0], "yazi");
    }

    #[test]
    fn test_get_provided_packages_mixed() {
        let yaml = r#"
pkg: myapp
pkg_id: example.com.myapp
provides:
  - "app1"
  - "@bin1"
  - "app2:alias"
  - "@bin2"
  - "app3=>rename"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let packages = recipe.get_provided_packages();

        assert_eq!(packages.len(), 3);
        assert!(packages.contains(&"app1".to_string()));
        assert!(packages.contains(&"app2".to_string()));
        assert!(packages.contains(&"app3".to_string()));
    }

    #[test]
    fn test_get_binaries() {
        let yaml = r#"
pkg: yazi
pkg_id: github.com.sxyazi.yazi
provides:
  - "yazi"
  - "@ya"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let binaries = recipe.get_binaries();

        assert_eq!(binaries.len(), 1);
        assert_eq!(binaries[0], "ya");
    }

    #[test]
    fn test_get_binaries_multiple() {
        let yaml = r#"
pkg: myapp
pkg_id: example.com.myapp
provides:
  - "app1"
  - "@bin1"
  - "app2"
  - "@bin2"
  - "@bin3"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let binaries = recipe.get_binaries();

        assert_eq!(binaries.len(), 3);
        assert!(binaries.contains(&"bin1".to_string()));
        assert!(binaries.contains(&"bin2".to_string()));
        assert!(binaries.contains(&"bin3".to_string()));
    }

    #[test]
    fn test_get_provided_packages_no_provides() {
        let yaml = r#"
pkg: test
pkg_id: example.com.test
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let packages = recipe.get_provided_packages();

        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0], "test");
    }

    #[test]
    fn test_get_binaries_empty() {
        let yaml = r#"
pkg: test
pkg_id: example.com.test
provides:
  - "app1"
  - "app2"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let binaries = recipe.get_binaries();

        assert!(binaries.is_empty());
    }

    #[test]
    fn test_ghcr_packages_excludes_binaries() {
        let yaml = r#"
pkg: yazi
pkg_id: github.com.sxyazi.yazi
provides:
  - "yazi"
  - "@ya"
"#;
        let recipe = SBuildRecipe::from_yaml(yaml).unwrap();
        let path = Path::new("binaries/yazi/static.yaml");
        let packages = recipe.ghcr_packages_from_path(path, "pkgforge");

        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].ghcr_path, "pkgforge/yazi/static/yazi");
        assert_eq!(packages[0].pkg_name, "yazi");
    }
}
