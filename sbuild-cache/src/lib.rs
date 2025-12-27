//! sbuild-cache: Historical build cache for SBUILD packages
//!
//! This crate provides:
//! - SQLite-based build history tracking
//! - Version comparison caching
//! - Package status management
//! - Rebuild decision support

pub mod db;
pub mod error;
pub mod models;
pub mod schema;

pub use db::CacheDatabase;
pub use error::{Error, Result};
pub use models::*;
