//! Tooling for the soarpkgs package format.
//!
//! Packages are declarative and hash-pinned: a client resolves one by parsing
//! alone, so the hash can live in the repository rather than being measured
//! after the artifact is built.

pub mod port;
