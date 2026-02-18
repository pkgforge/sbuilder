pub mod builder;
pub mod checksum;
pub mod cleanup;
pub mod constant;
pub mod ghcr;
pub mod signing;
pub mod types;
pub mod utils;

use std::path::Path;

pub use sbuild_meta::format_size;

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

pub fn update_json_metadata(
    json_path: &Path,
    pkg_name: &str,
    binary_name: &str,
    ghcr_repo: &str,
    tag: &str,
    bsum: Option<&str>,
    shasum: Option<&str>,
    checksum_bsum: Option<&str>,
    binary_size: Option<u64>,
    ghcr_total_size: Option<u64>,
) -> Result<(), String> {
    let content =
        std::fs::read_to_string(json_path).map_err(|e| format!("Failed to read JSON: {}", e))?;

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
                ghcr_repo, tag, binary_name
            )),
        );

        if let Some(b) = bsum {
            obj.insert("bsum".to_string(), serde_json::json!(b));
        }
        if let Some(s) = shasum {
            obj.insert("shasum".to_string(), serde_json::json!(s));
        }
        if let Some(cb) = checksum_bsum {
            obj.insert("checksum_bsum".to_string(), serde_json::json!(cb));
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

    let updated = serde_json::to_string_pretty(&json)
        .map_err(|e| format!("Failed to serialize JSON: {}", e))?;

    std::fs::write(json_path, updated).map_err(|e| format!("Failed to write JSON: {}", e))?;

    Ok(())
}

pub use sbuild_meta::SBuildRecipe;

/// Read a validated SBUILD recipe from the output directory.
pub fn read_recipe_metadata(outdir: &Path) -> Option<SBuildRecipe> {
    let sbuild_path = outdir.join("SBUILD");
    SBuildRecipe::from_file(&sbuild_path).ok()
}

/// Fetch a recipe from a URL.
pub async fn fetch_recipe(url: &str) -> Result<String, String> {
    use std::time::Duration;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch recipe: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error {}: {}", response.status(), url));
    }

    response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))
}
