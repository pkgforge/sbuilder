//! sbuild-cache: Historical build cache for SBUILD packages
//!
//! This crate provides:
//! - SQLite-based build history tracking
//! - Version comparison caching
//! - Package status management
//! - Rebuild decision support

pub mod error;
pub mod export;
pub mod models;
pub mod mongo;
pub mod schema;
pub mod sqlite;

pub use error::{Error, Result};
pub use models::*;
pub use mongo::MongoDatabase;
pub use sqlite::CacheDatabase;
