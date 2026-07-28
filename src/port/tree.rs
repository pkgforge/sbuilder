//! Loading a ports tree from disk.

use std::{fs, path::{Path, PathBuf}};

use crate::port::model::{PkgToml, VersionToml};

/// One package directory: its `pkg.toml` plus every pinned version file.
pub struct Package {
    pub dir: PathBuf,
    pub pkg: PkgToml,
    pub versions: Vec<VersionToml>,
}

/// Read every package under `<root>/packages`, sorted by directory name.
///
/// A malformed package is reported rather than aborting the scan, so one bad
/// file cannot hide the state of the rest of the tree.
pub fn load(root: &Path) -> (Vec<Package>, Vec<String>) {
    let mut packages = Vec::new();
    let mut errors = Vec::new();

    let mut dirs: Vec<PathBuf> = match fs::read_dir(root.join("packages")) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect(),
        Err(e) => {
            errors.push(format!("{}: {e}", root.join("packages").display()));
            return (packages, errors);
        }
    };
    dirs.sort();

    for dir in dirs {
        let name = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
        let pkg_path = dir.join("pkg.toml");
        let raw = match fs::read_to_string(&pkg_path) {
            Ok(s) => s,
            Err(_) => {
                errors.push(format!("{name}: no pkg.toml"));
                continue;
            }
        };
        let pkg: PkgToml = match toml::from_str(&raw) {
            Ok(p) => p,
            Err(e) => {
                errors.push(format!("{name}: pkg.toml unparseable: {e}"));
                continue;
            }
        };

        let mut version_paths: Vec<PathBuf> = fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|x| x == "toml")
                    && p.file_name().is_some_and(|f| f != "pkg.toml")
            })
            .collect();
        version_paths.sort();

        let mut versions = Vec::new();
        for vp in version_paths {
            let vname = vp.file_name().unwrap_or_default().to_string_lossy().to_string();
            match fs::read_to_string(&vp).map_err(|e| e.to_string()).and_then(|s| {
                toml::from_str::<VersionToml>(&s).map_err(|e| e.to_string())
            }) {
                Ok(v) => versions.push(v),
                Err(e) => errors.push(format!("{vname}: unparseable: {e}")),
            }
        }
        packages.push(Package { dir, pkg, versions });
    }

    (packages, errors)
}
