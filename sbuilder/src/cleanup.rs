use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::{
    constant::PNG_MAGIC_BYTES,
    utils::{calc_checksum, calc_magic_bytes},
};

pub struct FileCleanup {
    pkg_name: String,
    dir_path: PathBuf,
}

impl FileCleanup {
    pub fn new<P: AsRef<Path>>(pkg_name: String, dir_path: P) -> Self {
        Self {
            pkg_name,
            dir_path: dir_path.as_ref().to_path_buf(),
        }
    }

    pub fn cleanup(&self) -> std::io::Result<()> {
        self.setup_icons()?;
        self.setup_desktop_file()?;
        self.setup_appstream_file()?;
        self.cleanup_temp()?;
        self.generate_checksum()?;
        Ok(())
    }

    fn read_dir_entries(&self) -> std::io::Result<HashMap<String, Vec<PathBuf>>> {
        let mut file_map: HashMap<String, Vec<PathBuf>> = HashMap::new();

        for entry in fs::read_dir(&self.dir_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                file_map.entry(ext).or_default().push(path);
            }
        }

        Ok(file_map)
    }

    fn setup_icons(&self) -> std::io::Result<()> {
        let files = self.read_dir_entries()?;
        let has_diricon = files
            .values()
            .flatten()
            .any(|p| p.to_string_lossy().contains("DirIcon"));

        if has_diricon {
            let diricon_path = self.dir_path.join(format!("{}.DirIcon", self.pkg_name));
            if diricon_path.exists() {
                let magic_bytes = calc_magic_bytes(&diricon_path, 8);
                let new_ext = if magic_bytes == PNG_MAGIC_BYTES {
                    "png"
                } else {
                    "svg"
                };
                let new_path = self.dir_path.join(format!("{}.{}", self.pkg_name, new_ext));
                fs::rename(diricon_path, new_path)?;

                for ext in ["png", "svg"] {
                    if let Some(icons) = files.get(ext) {
                        for icon in icons {
                            if !icon.to_string_lossy().contains("DirIcon") {
                                fs::remove_file(icon)?;
                            }
                        }
                    }
                }
            }
        } else if let Some(png_files) = files.get("png") {
            if !png_files.is_empty() {
                let new_path = self.dir_path.join(format!("{}.png", self.pkg_name));
                fs::rename(&png_files[0], new_path)?;

                for path in png_files.iter().skip(1) {
                    fs::remove_file(path)?;
                }
                if let Some(svg_files) = files.get("svg") {
                    for path in svg_files {
                        fs::remove_file(path)?;
                    }
                }
            } else if let Some(svg_files) = files.get("svg") {
                if !svg_files.is_empty() {
                    let new_path = self.dir_path.join(format!("{}.svg", self.pkg_name));
                    fs::rename(&svg_files[0], new_path)?;

                    for path in svg_files.iter().skip(1) {
                        fs::remove_file(path)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn setup_desktop_file(&self) -> std::io::Result<()> {
        let files = self.read_dir_entries()?;

        if let Some(desktop_files) = files.get("desktop") {
            let target_path = self.dir_path.join(format!("{}.desktop", self.pkg_name));

            for path in desktop_files {
                if path != &target_path {
                    if !target_path.exists() {
                        fs::rename(path, &target_path)?;
                    } else {
                        fs::remove_file(path)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn setup_appstream_file(&self) -> std::io::Result<()> {
        let files = self.read_dir_entries()?;
        let xml_files: Vec<_> = files.get("xml").cloned().unwrap_or_default();

        if xml_files.is_empty() {
            return Ok(());
        }

        let mut has_metadata = false;
        for path in &xml_files {
            let filename = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("")
                .to_lowercase();

            if filename.contains("metainfo") {
                has_metadata = true;
                let target = self
                    .dir_path
                    .join(format!("{}.metainfo.xml", self.pkg_name));
                if path != &target {
                    fs::rename(path, target)?;
                }
                break;
            } else if filename.contains("appdata") {
                has_metadata = true;
                let target = self.dir_path.join(format!("{}.appdata.xml", self.pkg_name));
                if path != &target {
                    fs::rename(path, target)?;
                }
                break;
            }
        }

        if !has_metadata && !xml_files.is_empty() {
            let target = self
                .dir_path
                .join(format!("{}.metainfo.xml", self.pkg_name));
            fs::rename(&xml_files[0], target)?;
        }

        for path in xml_files.iter().skip(1) {
            if path.exists() {
                fs::remove_file(path)?;
            }
        }

        Ok(())
    }

    fn cleanup_temp(&self) -> std::io::Result<()> {
        let temp_dir = self.dir_path.join("SBUILD_TEMP");
        if temp_dir.exists() {
            fs::remove_dir_all(temp_dir)?;
        }
        Ok(())
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
