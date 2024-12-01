use std::fmt::Display;

pub enum OutputStream {
    Stdout(String),
    Stderr(String),
}

pub enum PackageType {
    Static,
    Dynamic,
    AppImage,
    FlatImage,
}

impl Display for PackageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageType::Static => write!(f, "static"),
            PackageType::Dynamic => write!(f, "dynamic"),
            PackageType::AppImage => write!(f, "appimage"),
            PackageType::FlatImage => write!(f, "flatimage"),
        }
    }
}

#[derive(Default, Clone)]
pub struct SoarEnv {
    pub bin_path: String,
    pub cache_path: String,
}
