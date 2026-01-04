//! Checksum generation utilities
//!
//! Provides functions to compute BLAKE3 and SHA256 checksums for files.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use blake3::Hasher as Blake3Hasher;
use sha2::{Digest, Sha256};

/// Compute BLAKE3 hash of a file
pub fn b3sum<P: AsRef<Path>>(path: P) -> std::io::Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Blake3Hasher::new();

    let mut buffer = [0u8; 65536]; // 64KB buffer
    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

/// Compute SHA256 hash of a file
pub fn sha256sum<P: AsRef<Path>>(path: P) -> std::io::Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();

    let mut buffer = [0u8; 65536]; // 64KB buffer
    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}

/// Compute both BLAKE3 and SHA256 checksums
pub fn compute_checksums<P: AsRef<Path>>(path: P) -> std::io::Result<Checksums> {
    let path = path.as_ref();
    Ok(Checksums {
        b3sum: b3sum(path)?,
        sha256: sha256sum(path)?,
    })
}

/// Container for file checksums
#[derive(Debug, Clone)]
pub struct Checksums {
    pub b3sum: String,
    pub sha256: String,
}

impl Checksums {
    /// Write checksums to files alongside the original file
    pub fn write_to_files<P: AsRef<Path>>(&self, base_path: P) -> std::io::Result<()> {
        let base = base_path.as_ref();
        let filename = base.file_name().unwrap_or_default().to_string_lossy();

        // Write b3sum file
        let b3sum_path = base.with_extension(format!(
            "{}.b3sum",
            base.extension().unwrap_or_default().to_string_lossy()
        ));
        std::fs::write(&b3sum_path, format!("{}  {}\n", self.b3sum, filename))?;

        // Write sha256sum file
        let sha256_path = base.with_extension(format!(
            "{}.sha256",
            base.extension().unwrap_or_default().to_string_lossy()
        ));
        std::fs::write(&sha256_path, format!("{}  {}\n", self.sha256, filename))?;

        Ok(())
    }
}

/// Generate CHECKSUM file with all files in a directory
pub fn generate_checksum_file<P: AsRef<Path>>(dir: P) -> std::io::Result<String> {
    let dir = dir.as_ref();
    let mut lines = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            let filename = path.file_name().unwrap_or_default().to_string_lossy();
            // Skip existing checksum files and the CHECKSUM file itself
            if filename.ends_with(".b3sum")
                || filename.ends_with(".sha256")
                || filename == "CHECKSUM"
            {
                continue;
            }

            let b3 = b3sum(&path)?;
            let sha = sha256sum(&path)?;
            lines.push(format!("BLAKE3: {} {}", b3, filename));
            lines.push(format!("SHA256: {} {}", sha, filename));
        }
    }

    lines.sort();
    let content = lines.join("\n");

    // Write CHECKSUM file
    let checksum_path = dir.join("CHECKSUM");
    std::fs::write(&checksum_path, &content)?;

    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_b3sum() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        file.flush().unwrap();

        let hash = b3sum(file.path()).unwrap();
        // Known BLAKE3 hash of "hello world"
        assert_eq!(
            hash,
            "d74981efa70a0c880b8d8c1985d075dbcbf679b99a5f9914e5aaf96b831a9e24"
        );
    }

    #[test]
    fn test_sha256sum() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        file.flush().unwrap();

        let hash = sha256sum(file.path()).unwrap();
        // Known SHA256 hash of "hello world"
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }
}
