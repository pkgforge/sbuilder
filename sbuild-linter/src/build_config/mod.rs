use std::{
    fs::File,
    io::{self, BufWriter, Write},
};

use crate::{comments::Comments, description::Description, get_pkg_id, xexec::XExec, BuildAsset};

/// Per-package configuration for multi-package recipes
#[derive(Debug, Clone, Default)]
pub struct PackageConfig {
    pub provides: Vec<String>,
}

#[derive(Debug, Default)]
pub struct BuildConfig {
    pub _disabled: bool,
    pub pkg: String,
    pub pkg_id: String,
    pub pkg_type: Option<String>,
    pub pkgver: Option<String>,
    pub remote_pkgver: Option<String>,
    pub app_id: Option<String>,
    pub build_util: Option<Vec<String>>,
    pub build_asset: Option<Vec<BuildAsset>>,
    pub category: Vec<String>,
    pub description: Option<Description>,
    pub homepage: Option<Vec<String>>,
    pub maintainer: Option<Vec<String>>,
    pub license: Option<Vec<String>>,
    pub note: Option<Vec<String>>,
    pub provides: Option<Vec<String>>,
    pub packages: Option<Vec<(String, PackageConfig)>>,
    pub repology: Option<Vec<String>>,
    pub src_url: Vec<String>,
    pub tag: Option<Vec<String>>,
    pub ghcr_pkg: Option<String>,
    pub snapshots: Option<Vec<String>>,
    pub x_exec: XExec,
}

impl BuildConfig {
    pub fn set_pkg_id_from_src_url(&mut self) {
        if self.pkg_id.is_empty() && !self.src_url.is_empty() {
            self.pkg_id = get_pkg_id(&self.src_url[0]);
        }
    }

    pub fn write_yaml(
        &self,
        writer: &mut BufWriter<File>,
        indent: usize,
        comments: Comments,
    ) -> io::Result<()> {
        let indent_str = " ".repeat(indent);

        for c in &comments.header_comments {
            writeln!(writer, "{}", c)?;
        }

        let write_field_comments = |writer: &mut BufWriter<File>, field: &str| -> io::Result<()> {
            if let Some(comments) = comments.field_comments.get(field) {
                for comment in comments {
                    writeln!(writer, "{}", comment)?;
                }
            }
            Ok(())
        };

        write_field_comments(writer, "_disabled")?;
        writeln!(writer, "{}_disabled: {}\n", indent_str, self._disabled)?;

        write_field_comments(writer, "pkg")?;
        writeln!(writer, "{}pkg: \"{}\"", indent_str, self.pkg)?;

        write_field_comments(writer, "pkg_id")?;
        writeln!(writer, "{}pkg_id: \"{}\"", indent_str, self.pkg_id)?;

        write_field_comments(writer, "pkg_type")?;
        if let Some(ref pkg_type) = self.pkg_type {
            writeln!(writer, "{}pkg_type: \"{}\"", indent_str, pkg_type)?;
        }

        write_field_comments(writer, "pkgver")?;
        if let Some(ref pkgver) = self.pkgver {
            writeln!(writer, "{}pkgver: \"{}\"", indent_str, pkgver)?;
        }

        write_field_comments(writer, "remote_pkgver")?;
        if let Some(ref remote_pkgver) = self.remote_pkgver {
            writeln!(writer, "{}remote_pkgver: \"{}\"", indent_str, remote_pkgver)?;
        }

        write_field_comments(writer, "app_id")?;
        if let Some(ref app_id) = self.app_id {
            writeln!(writer, "{}app_id: \"{}\"", indent_str, app_id)?;
        }

        write_field_comments(writer, "build_util")?;
        if let Some(ref build_util) = self.build_util {
            writeln!(writer, "{}build_util:", indent_str)?;
            for util in build_util {
                writeln!(writer, "{}  - \"{}\"", indent_str, util)?;
            }
        }

        write_field_comments(writer, "build_asset")?;
        if let Some(ref build_asset) = self.build_asset {
            writeln!(writer, "{}build_asset:", indent_str)?;
            for asset in build_asset {
                writeln!(writer, "{}  - url: \"{}\"", indent_str, asset.url)?;
                writeln!(writer, "{}    out: \"{}\"", indent_str, asset.out)?;
            }
        }

        write_field_comments(writer, "ghcr_pkg")?;
        if let Some(ref ghcr_pkg) = self.ghcr_pkg {
            writeln!(writer, "{}ghcr_pkg: \"{}\"", indent_str, ghcr_pkg)?;
        }

        write_field_comments(writer, "category")?;
        writeln!(writer, "{}category:", indent_str)?;
        for cat in &self.category {
            writeln!(writer, "{}  - \"{}\"", indent_str, cat)?;
        }

        write_field_comments(writer, "description")?;
        self.description
            .clone()
            .unwrap()
            .write_yaml(writer, indent)?;

        write_field_comments(writer, "homepage")?;
        if let Some(ref homepage) = self.homepage {
            writeln!(writer, "{}homepage:", indent_str)?;
            for url in homepage {
                writeln!(writer, "{}  - \"{}\"", indent_str, url)?;
            }
        }

        write_field_comments(writer, "maintainer")?;
        if let Some(ref maintainer) = self.maintainer {
            writeln!(writer, "{}maintainer:", indent_str)?;
            for m in maintainer {
                writeln!(writer, "{}  - \"{}\"", indent_str, m)?;
            }
        }

        write_field_comments(writer, "license")?;
        if let Some(ref license) = self.license {
            writeln!(writer, "{}license:", indent_str)?;
            for lc in license {
                writeln!(writer, "{}  - \"{}\"", indent_str, lc)?;
            }
        }

        write_field_comments(writer, "note")?;
        if let Some(ref note) = self.note {
            writeln!(writer, "{}note:", indent_str)?;
            for n in note {
                writeln!(writer, "{}  - \"{}\"", indent_str, n)?;
            }
        }

        write_field_comments(writer, "provides")?;
        if let Some(ref provides) = self.provides {
            writeln!(writer, "{}provides:", indent_str)?;
            for p in provides {
                writeln!(writer, "{}  - \"{}\"", indent_str, p)?;
            }
        }

        write_field_comments(writer, "packages")?;
        if let Some(ref packages) = self.packages {
            writeln!(writer, "{}packages:", indent_str)?;
            for (name, pkg) in packages {
                writeln!(writer, "{}  {}:", indent_str, name)?;
                if !pkg.provides.is_empty() {
                    writeln!(writer, "{}    provides:", indent_str)?;
                    for p in &pkg.provides {
                        writeln!(writer, "{}      - \"{}\"", indent_str, p)?;
                    }
                }
            }
        }

        write_field_comments(writer, "repology")?;
        if let Some(ref repology) = self.repology {
            writeln!(writer, "{}repology:", indent_str)?;
            for r in repology {
                writeln!(writer, "{}  - \"{}\"", indent_str, r)?;
            }
        }

        write_field_comments(writer, "src_url")?;
        writeln!(writer, "{}src_url:", indent_str)?;
        for url in &self.src_url {
            writeln!(writer, "{}  - \"{}\"", indent_str, url)?;
        }

        write_field_comments(writer, "tag")?;
        if let Some(ref tag) = self.tag {
            writeln!(writer, "{}tag:", indent_str)?;
            for t in tag {
                writeln!(writer, "{}  - \"{}\"", indent_str, t)?;
            }
        }

        write_field_comments(writer, "snapshots")?;
        if let Some(ref snapshots) = self.snapshots {
            writeln!(writer, "{}snapshots:", indent_str)?;
            for s in snapshots {
                writeln!(writer, "{}  - \"{}\"", indent_str, s)?;
            }
        }

        write_field_comments(writer, "x_exec")?;
        writeln!(writer, "{}x_exec:", indent_str)?;
        self.x_exec.write_yaml(writer, indent + 2)?;

        Ok(())
    }
}
