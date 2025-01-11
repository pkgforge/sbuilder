use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

use sbuild_linter::build_config::BuildConfig;

use crate::{
    constant::{MIN_DESKTOP_SIZE, MIN_ICON_SIZE, XML_MAGIC_BYTES},
    types::PackageType,
    utils::{calc_checksum, calc_magic_bytes, download},
};

pub struct Finalize {
    dir_path: PathBuf,
    build_config: BuildConfig,
    pkg_type: PackageType,
    fallback_icon: Option<PathBuf>,
    keep: bool,
}

impl Finalize {
    pub fn new<P: AsRef<Path>>(
        dir_path: P,
        build_config: BuildConfig,
        pkg_type: PackageType,
        keep: bool,
    ) -> Self {
        Self {
            dir_path: dir_path.as_ref().to_path_buf(),
            build_config,
            pkg_type,
            fallback_icon: None,
            keep,
        }
    }

    pub async fn update(&mut self) -> std::io::Result<()> {
        if !self.keep {
            self.cleanup_temp()?;
        }
        self.validate_files().await?;
        self.generate_checksum()?;
        Ok(())
    }

    async fn validate_files(&mut self) -> io::Result<()> {
        if matches!(self.pkg_type, PackageType::Static | PackageType::Dynamic) {
            return Ok(());
        };
        let build_config = &self.build_config;
        let pkg_name = &build_config.pkg;

        let provides = build_config.provides.clone();
        let provides = if build_config.x_exec.entrypoint.is_some() {
            vec![pkg_name.clone()]
        } else {
            provides.unwrap_or_else(|| vec![pkg_name.clone()])
        };

        for provide in provides {
            let cmd = provide
                .split_once(|c| c == ':' || c == '=')
                .map(|(p1, _)| p1.to_string())
                .unwrap_or_else(|| provide.to_string());

            self.validate_icon(&cmd).await?;
            self.validate_appstream(&cmd)?;
            self.validate_desktop(&cmd)?;
        }
        Ok(())
    }

    async fn validate_icon(&mut self, cmd: &str) -> io::Result<()> {
        let png_path = self.dir_path.join(format!("{}.png", cmd));
        let svg_path = self.dir_path.join(format!("{}.svg", cmd));

        let icon_valid = match (png_path.exists(), svg_path.exists()) {
            (true, _) => self.check_file_size(&png_path, MIN_ICON_SIZE)?,
            (_, true) => self.check_file_size(&svg_path, MIN_ICON_SIZE)?,
            _ => false,
        };

        if !icon_valid {
            if let Some(ref fallback_icon) = self.fallback_icon {
                fs::copy(fallback_icon, png_path)?;
            } else {
                let url = "https://raw.githubusercontent.com/pkgforge/soarpkgs/main/assets/pkg.png";
                download(&url, &png_path).await.unwrap();
                self.fallback_icon = Some(png_path);
            }
        }

        Ok(())
    }

    fn validate_appstream(&self, cmd: &str) -> io::Result<()> {
        let appdata_path = self.dir_path.join(format!("{}.appdata.xml", cmd));
        let metainfo_path = self.dir_path.join(format!("{}.metainfo.xml", cmd));

        for path in [metainfo_path, appdata_path].iter() {
            if path.exists() {
                let magic_bytes = calc_magic_bytes(path, 5);
                if magic_bytes == XML_MAGIC_BYTES {
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    fn validate_desktop(&self, cmd: &str) -> io::Result<()> {
        let desktop_path = self.dir_path.join(format!("{}.desktop", cmd));

        if !desktop_path.exists() || !self.check_file_size(&desktop_path, MIN_DESKTOP_SIZE)? {
            let desktop_content = self.generate_desktop_content(cmd);
            let mut file = File::create(&desktop_path)?;
            file.write_all(desktop_content.as_bytes())?;
        }

        Ok(())
    }

    fn check_file_size(&self, path: &Path, min_size: u64) -> io::Result<bool> {
        Ok(fs::metadata(path)?.len() >= min_size)
    }

    fn cleanup_temp(&self) -> std::io::Result<()> {
        let temp_dir = self.dir_path.join("SBUILD_TEMP");
        if temp_dir.exists() {
            fs::remove_dir_all(temp_dir)?;
        }
        Ok(())
    }

    fn generate_desktop_content(&self, cmd: impl Into<String>) -> String {
        format!(
            r#"[Desktop Entry]
Type=Application
Name={0}
Exec={0}
Icon={0}
Categories=Utility;
"#,
            cmd.into()
        )
    }

    fn generate_checksum(&self) -> std::io::Result<()> {
        let checksum_path = self.dir_path.join("CHECKSUM");
        if checksum_path.exists() {
            fs::remove_file(&checksum_path)?;
        }

        let mut checksum_file = fs::File::create(&checksum_path)?;
        for entry in fs::read_dir(&self.dir_path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path != checksum_path {
                let checksum = calc_checksum(&path);
                let rel_path = path.strip_prefix(&self.dir_path).unwrap_or(&path).display();
                writeln!(checksum_file, "{}:{}", rel_path, checksum)?;
            }
        }

        Ok(())
    }
}
