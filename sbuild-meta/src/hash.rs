//! Recipe hashing for change detection
//!
//! Computes a deterministic hash of SBUILD recipe content,
//! ignoring whitespace, empty lines, and comments for stability.

use crate::Result;

/// Compute a normalized hash of recipe content.
///
/// The normalization process:
/// 1. Removes empty lines
/// 2. Removes comment-only lines (starting with #)
/// 3. Trims whitespace from each line
/// 4. Optionally excludes the `version` field for rebuild detection
///
/// This ensures minor formatting changes don't trigger rebuilds.
pub fn compute_recipe_hash(content: &str) -> String {
    compute_recipe_hash_internal(content, false)
}

/// Compute hash excluding the version field.
///
/// Used for detecting recipe changes that should trigger rebuilds,
/// where version changes are handled separately by the bot.
pub fn compute_recipe_hash_excluding_version(content: &str) -> String {
    compute_recipe_hash_internal(content, true)
}

fn compute_recipe_hash_internal(content: &str, exclude_version: bool) -> String {
    let normalized: String = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            // Skip empty lines
            if trimmed.is_empty() {
                return false;
            }
            // Skip comment-only lines (but keep shebang)
            if trimmed.starts_with('#') && !trimmed.starts_with("#!") {
                return false;
            }
            // Optionally skip version field
            if exclude_version && trimmed.starts_with("version:") {
                return false;
            }
            true
        })
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join("\n");

    blake3::hash(normalized.as_bytes()).to_hex().to_string()
}

/// Verify that a hash matches the expected value.
pub fn verify_hash(content: &str, expected: &str) -> bool {
    compute_recipe_hash(content) == expected
}

/// Compute hash from a file path.
pub fn hash_file(path: &std::path::Path) -> Result<String> {
    let content = std::fs::read_to_string(path)?;
    Ok(compute_recipe_hash(&content))
}

/// Compute hash from a file, excluding version field.
pub fn hash_file_excluding_version(path: &std::path::Path) -> Result<String> {
    let content = std::fs::read_to_string(path)?;
    Ok(compute_recipe_hash_excluding_version(&content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_consistency() {
        let content = "pkg: test\ndescription: foo";
        // Hash should be consistent for same content
        assert_eq!(
            compute_recipe_hash(content),
            compute_recipe_hash("pkg: test\ndescription: foo")
        );
    }

    #[test]
    fn test_hash_ignores_empty_lines() {
        let content1 = "pkg: test\n\n\ndescription: foo";
        let content2 = "pkg: test\ndescription: foo";
        assert_eq!(compute_recipe_hash(content1), compute_recipe_hash(content2));
    }

    #[test]
    fn test_hash_ignores_comments() {
        let content1 = "pkg: test\n# This is a comment\ndescription: foo";
        let content2 = "pkg: test\ndescription: foo";
        assert_eq!(compute_recipe_hash(content1), compute_recipe_hash(content2));
    }

    #[test]
    fn test_hash_preserves_shebang() {
        let content1 = "#!/SBUILD ver @v1.0.0\npkg: test";
        let content2 = "pkg: test";
        assert_ne!(compute_recipe_hash(content1), compute_recipe_hash(content2));
    }

    #[test]
    fn test_hash_excludes_version() {
        let content1 = "pkg: test\nversion: 1.0.0\ndescription: foo";
        let content2 = "pkg: test\nversion: 2.0.0\ndescription: foo";
        assert_eq!(
            compute_recipe_hash_excluding_version(content1),
            compute_recipe_hash_excluding_version(content2)
        );
    }
}
