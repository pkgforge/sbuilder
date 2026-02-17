//! sbuild-meta: Metadata generator for SBUILD packages
//!
//! This crate provides tools for:
//! - Fetching OCI manifests from GHCR
//! - Generating package metadata from SBUILD recipes
//! - Recipe hashing for change detection
//! - Version comparison and update detection
//! - Historical cache management

pub mod error;
pub mod hash;
pub mod manifest;
pub mod metadata;
pub mod recipe;
pub mod registry;

pub use error::{Error, Result};
pub use hash::compute_recipe_hash;
pub use manifest::OciManifest;
pub use metadata::{format_size, PackageMetadata};
pub use recipe::{sanitize_oci_name, GhcrPackageInfo, SBuildRecipe};
pub use registry::RegistryClient;
