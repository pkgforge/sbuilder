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
    pkg_name: String,
    dir_path: PathBuf,
    build_config: BuildConfig,
    pkg_type: PackageType,
}

impl Finalize {
    pub fn new<P: AsRef<Path>>(
        pkg_name: String,
        dir_path: P,
        build_config: BuildConfig,
        pkg_type: PackageType,
    ) -> Self {
        Self {
            pkg_name,
            dir_path: dir_path.as_ref().to_path_buf(),
            build_config,
            pkg_type,
        }
    }

    pub async fn cleanup(&self) -> std::io::Result<()> {
        self.cleanup_temp()?;
        self.validate_files().await?;
        self.generate_checksum()?;
        Ok(())
    }

    async fn validate_files(&self) -> io::Result<()> {
        self.validate_icon().await?;
        self.validate_appstream()?;
        self.validate_desktop()?;
        Ok(())
    }

    async fn validate_icon(&self) -> io::Result<()> {
        let png_path = self.dir_path.join(format!("{}.png", self.pkg_name));
        let svg_path = self.dir_path.join(format!("{}.svg", self.pkg_name));

        let icon_valid = match (png_path.exists(), svg_path.exists()) {
            (true, _) => self.check_file_size(&png_path, MIN_ICON_SIZE)?,
            (_, true) => self.check_file_size(&svg_path, MIN_ICON_SIZE)?,
            _ => false,
        };

        if !icon_valid {
            let icon = match self.pkg_type {
                PackageType::Static | PackageType::Dynamic => "bin.default.png",
                _ => "pkg.default.png",
            };
            let url = format!("https://bin.pkgforge.dev/x86_64/{}", icon);

            download(&url, png_path).await.unwrap();
        }

        Ok(())
    }

    fn validate_appstream(&self) -> io::Result<()> {
        let appdata_path = self.dir_path.join(format!("{}.appdata.xml", self.pkg_name));
        let metainfo_path = self
            .dir_path
            .join(format!("{}.metainfo.xml", self.pkg_name));

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

    fn validate_desktop(&self) -> io::Result<()> {
        let desktop_path = self.dir_path.join(format!("{}.desktop", self.pkg_name));

        if !desktop_path.exists() || !self.check_file_size(&desktop_path, MIN_DESKTOP_SIZE)? {
            let desktop_content = self.generate_desktop_content();
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

    fn generate_desktop_content(&self) -> String {
        format!(
            r#"[Desktop Entry]
Type=Application
Name={}
Exec={}
Icon={}
Categories=Utility;
"#,
            self.pkg_name, self.build_config.pkg, self.pkg_name
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
