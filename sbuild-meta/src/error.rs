use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON parsing failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML parsing failed: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Registry error: {0}")]
    Registry(String),

    #[error("Manifest not found: {0}")]
    ManifestNotFound(String),

    #[error("Recipe error: {0}")]
    Recipe(String),

    #[error("No pkgver script in recipe")]
    NoPkgver,

    #[error("pkgver execution failed: {0}")]
    PkgverFailed(String),

    #[error("Version parse error: {0}")]
    VersionParse(String),

    #[error("Glob pattern error: {0}")]
    Glob(#[from] glob::PatternError),

    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
