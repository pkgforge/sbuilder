use std::fmt::Display;

pub enum OutputStream {
    Stdout(String),
    Stderr(String),
}

#[derive(Clone, PartialEq, Eq)]
pub enum PackageType {
    Static,
    Dynamic,
    AppImage,
    FlatImage,
    NixAppImage,
    Unknown,
}

impl Display for PackageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageType::Static => write!(f, "static"),
            PackageType::Dynamic => write!(f, "dynamic"),
            PackageType::AppImage => write!(f, "appimage"),
            PackageType::NixAppImage => write!(f, "nixappimage"),
            PackageType::FlatImage => write!(f, "flatimage"),
            PackageType::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Default, Clone)]
pub struct SoarEnv {
    pub bin_path: String,
    pub cache_path: String,
}
