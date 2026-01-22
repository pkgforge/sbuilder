//! Package signing utilities using minisign
//!
//! Provides functions to sign build artifacts with minisign.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SignError {
    #[error("minisign not found - install minisign to sign packages")]
    MinisignNotFound,

    #[error("minisign key not found or invalid")]
    KeyNotFound,

    #[error("signing failed: {0}")]
    SignFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Minisign signer for package artifacts
pub struct Signer {
    key_path: Option<String>,
    key_data: Option<String>,
    password: Option<String>,
}

impl Signer {
    /// Create a new signer with key file path
    pub fn with_key_file<P: AsRef<Path>>(path: P) -> Self {
        Self {
            key_path: Some(path.as_ref().to_string_lossy().to_string()),
            key_data: None,
            password: None,
        }
    }

    /// Create a new signer with key data (for CI environments)
    pub fn with_key_data(key: String) -> Self {
        Self {
            key_path: None,
            key_data: Some(key),
            password: None,
        }
    }

    /// Set the password for the private key
    pub fn with_password(mut self, password: Option<String>) -> Self {
        self.password = password;
        self
    }

    /// Check if minisign is available
    pub fn check_minisign() -> Result<(), SignError> {
        if which::which("minisign").is_err() {
            return Err(SignError::MinisignNotFound);
        }
        Ok(())
    }

    /// Sign a file, creating a .sig file alongside it
    pub fn sign<P: AsRef<Path>>(&self, file: P) -> Result<(), SignError> {
        let file_path = file.as_ref();

        // Prepare key file if using key data
        let temp_key = if let Some(ref key_data) = self.key_data {
            let temp_path = std::env::temp_dir().join("minisign_key.tmp");
            std::fs::write(&temp_path, key_data)?;
            Some(temp_path)
        } else {
            None
        };

        let key_path = temp_key
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .or_else(|| self.key_path.clone())
            .ok_or(SignError::KeyNotFound)?;

        let mut child = Command::new("minisign")
            .args([
                "-S", // Sign
                "-s",
                &key_path, // Secret key
                "-m",
                &file_path.to_string_lossy(), // File to sign
                "-x",
                &format!("{}.sig", file_path.display()), // Output signature
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Write password to stdin if provided
        if let Some(ref password) = self.password {
            if let Some(mut stdin) = child.stdin.take() {
                writeln!(stdin, "{}", password)?;
            }
        }

        let output = child.wait_with_output()?;

        // Clean up temp key
        if let Some(temp_path) = temp_key {
            std::fs::remove_file(temp_path).ok();
        }

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SignError::SignFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// Sign all files in a directory (recursively)
    pub fn sign_directory<P: AsRef<Path>>(&self, dir: P) -> Result<Vec<String>, SignError> {
        let dir = dir.as_ref();
        let mut signed = Vec::new();

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                // Recursively sign subdirectories
                signed.extend(self.sign_directory(&path)?);
            } else if path.is_file() {
                let filename = path.file_name().unwrap_or_default().to_string_lossy();

                // Skip signature files and checksums
                if filename.ends_with(".sig")
                    || filename.ends_with(".b3sum")
                    || filename.ends_with(".sha256")
                    || filename == "CHECKSUM"
                {
                    continue;
                }

                self.sign(&path)?;
                signed.push(path.to_string_lossy().to_string());
            }
        }

        Ok(signed)
    }
}

/// Verify a signature
pub fn verify<P: AsRef<Path>>(file: P, pubkey: &str) -> Result<bool, SignError> {
    Signer::check_minisign()?;

    let file_path = file.as_ref();
    let sig_path = format!("{}.sig", file_path.display());

    // Write pubkey to temp file
    let temp_pub = std::env::temp_dir().join("minisign_pub.tmp");
    std::fs::write(&temp_pub, pubkey)?;

    let output = Command::new("minisign")
        .args([
            "-V", // Verify
            "-p",
            &temp_pub.to_string_lossy(),
            "-m",
            &file_path.to_string_lossy(),
            "-x",
            &sig_path,
        ])
        .output()?;

    std::fs::remove_file(temp_pub).ok();

    Ok(output.status.success())
}
