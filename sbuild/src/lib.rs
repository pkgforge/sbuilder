pub mod builder;
pub mod checksum;
pub mod cleanup;
pub mod constant;
pub mod ghcr;
pub mod signing;
pub mod types;
pub mod utils;

use std::path::Path;

pub fn parse_ghcr_path(recipe_path: &str) -> Option<(String, String)> {
    let path_part = if recipe_path.contains("/binaries/") {
        recipe_path.split("/binaries/").last()
    } else if recipe_path.contains("/packages/") {
        recipe_path.split("/packages/").last()
    } else if recipe_path.starts_with("binaries/") {
        recipe_path.strip_prefix("binaries/")
    } else if recipe_path.starts_with("packages/") {
        recipe_path.strip_prefix("packages/")
    } else {
        return None;
    };

    let path_part = path_part?;

    let parts: Vec<&str> = path_part.split('/').collect();
    if parts.len() < 2 {
        return None;
    }

    let pkg_family = parts[0].to_string();
    let filename = parts[1];

    let recipe_name = filename
        .strip_suffix(".yaml")
        .or_else(|| filename.strip_suffix(".yml"))
        .unwrap_or(filename)
        .to_string();

    Some((pkg_family, recipe_name))
}

pub fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

pub fn sanitize_oci_name(name: &str) -> String {
    let name = name.replace("++", "pp");

    name.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn update_json_metadata(
    json_path: &Path,
    pkg_name: &str,
    ghcr_repo: &str,
    tag: &str,
    bsum: Option<&str>,
    shasum: Option<&str>,
    binary_size: Option<u64>,
    ghcr_total_size: Option<u64>,
) -> Result<(), String> {
    let content = std::fs::read_to_string(json_path).map_err(|e| format!("Failed to read JSON: {}", e))?;

    let mut json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse JSON: {}", e))?;

    if let Some(obj) = json.as_object_mut() {
        obj.insert("pkg_name".to_string(), serde_json::json!(pkg_name));

        obj.insert(
            "ghcr_pkg".to_string(),
            serde_json::json!(format!("ghcr.io/{}:{}", ghcr_repo, tag)),
        );

        obj.insert(
            "ghcr_url".to_string(),
            serde_json::json!(format!("https://ghcr.io/{}", ghcr_repo)),
        );

        obj.insert(
            "download_url".to_string(),
            serde_json::json!(format!(
                "https://api.ghcr.pkgforge.dev/{}?tag={}&download={}",
                ghcr_repo, tag, pkg_name
            )),
        );

        if let Some(b) = bsum {
            obj.insert("bsum".to_string(), serde_json::json!(b));
        }
        if let Some(s) = shasum {
            obj.insert("shasum".to_string(), serde_json::json!(s));
        }

        if let Some(s) = binary_size {
            obj.insert("size".to_string(), serde_json::json!(format_size(s)));
            obj.insert("size_raw".to_string(), serde_json::json!(s));
        }

        if let Some(s) = ghcr_total_size {
            obj.insert("ghcr_size".to_string(), serde_json::json!(format_size(s)));
            obj.insert("ghcr_size_raw".to_string(), serde_json::json!(s));
        }
    }

    let updated = serde_json::to_string_pretty(&json).map_err(|e| format!("Failed to serialize JSON: {}", e))?;

    std::fs::write(json_path, updated).map_err(|e| format!("Failed to write JSON: {}", e))?;

    Ok(())
}

#[derive(Debug, Default)]
pub struct SbuildMetadata {
    pub pkg: String,
    pub pkg_id: String,
    pub pkg_type: Option<String>,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub ghcr_pkg: Option<String>,
    pub provides: Vec<String>,
}

impl SbuildMetadata {
    pub fn get_provided_packages(&self) -> Vec<String> {
        if self.provides.is_empty() {
            return vec![];
        }

        let mut seen = std::collections::HashSet::new();
        let mut packages = Vec::new();

        for entry in &self.provides {
            if entry.starts_with('@') {
                continue;
            }

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

        packages
    }

    pub fn get_extra_binaries(&self) -> Vec<String> {
        if self.provides.is_empty() {
            return vec![];
        }

        let mut seen = std::collections::HashSet::new();
        let mut binaries = Vec::new();

        for entry in &self.provides {
            if !entry.starts_with('@') {
                continue;
            }

            let name = entry
                .strip_prefix('@')
                .unwrap_or(entry)
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

            if !name.is_empty() && seen.insert(name.clone()) {
                binaries.push(name);
            }
        }

        binaries
    }
}

pub fn read_sbuild_metadata(outdir: &Path) -> Option<SbuildMetadata> {
    let sbuild_path = outdir.join("SBUILD");
    let content = std::fs::read_to_string(&sbuild_path).ok()?;

    let yaml: serde_yml::Value = serde_yml::from_str(&content).ok()?;
    let map = yaml.as_mapping()?;

    let pkg = map.get("pkg").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    let pkg_id = map.get("pkg_id").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    let pkg_type = map.get("pkg_type").and_then(|v| v.as_str()).map(String::from);

    let description = map.get("description").and_then(|v| {
        if let Some(s) = v.as_str() {
            Some(s.to_string())
        } else if let Some(m) = v.as_mapping() {
            m.get("short").and_then(|s| s.as_str()).map(String::from)
        } else {
            None
        }
    });

    let homepage = map.get("homepage").and_then(|v| {
        if let Some(arr) = v.as_sequence() {
            arr.first().and_then(|s| s.as_str()).map(String::from)
        } else {
            v.as_str().map(String::from)
        }
    });

    let license = map.get("license").and_then(|v| {
        if let Some(arr) = v.as_sequence() {
            let licenses: Vec<String> = arr
                .iter()
                .filter_map(|item| {
                    if let Some(s) = item.as_str() {
                        Some(s.to_string())
                    } else if let Some(m) = item.as_mapping() {
                        m.get("id").and_then(|id| id.as_str()).map(String::from)
                    } else {
                        None
                    }
                })
                .collect();
            if licenses.is_empty() { None } else { Some(licenses.join(", ")) }
        } else {
            v.as_str().map(String::from)
        }
    });

    let ghcr_pkg = map.get("ghcr_pkg").and_then(|v| v.as_str()).map(String::from);

    let provides: Vec<String> = map
        .get("provides")
        .and_then(|v| v.as_sequence())
        .map(|arr| {
            let mut seen = std::collections::HashSet::new();
            arr.iter()
                .filter_map(|item| item.as_str())
                .filter_map(|s| {
                    let pkg_name = s.split(|c| c == ':' || c == '=').next().unwrap_or(s).to_string();
                    if seen.insert(pkg_name.clone()) { Some(pkg_name) } else { None }
                })
                .collect()
        })
        .unwrap_or_default();

    Some(SbuildMetadata {
        pkg,
        pkg_id,
        pkg_type,
        description,
        homepage,
        license,
        ghcr_pkg,
        provides,
    })
}
