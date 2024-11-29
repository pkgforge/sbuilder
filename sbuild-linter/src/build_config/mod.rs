use std::{
    fs::File,
    io::{self, BufWriter, Write},
};

use indexmap::IndexMap;
use serde::Deserialize;
use serde_yml::Value;

use crate::{comments::Comments, distro_pkg::DistroPkg, get_pkg_id, xexec::XExec, BuildAsset};

pub mod visitor;

#[derive(Debug, Default)]
pub struct BuildConfig {
    pub _disabled: bool,
    pub pkg: String,
    pub pkg_id: Option<String>,
    pub pkg_type: Option<String>,
    pub pkgver: Option<String>,
    pub app_id: Option<String>,
    pub build_util: Option<Vec<String>>,
    pub build_asset: Option<Vec<BuildAsset>>,
    pub category: Vec<String>,
    pub description: String,
    pub distro_pkg: Option<DistroPkg>,
    pub homepage: Option<Vec<String>>,
    pub maintainer: Option<Vec<String>>,
    pub icon: Option<String>,
    pub desktop: Option<String>,
    pub license: Option<Vec<String>>,
    pub note: Option<Vec<String>>,
    pub provides: Option<Vec<String>>,
    pub repology: Option<Vec<String>>,
    pub src_url: Vec<String>,
    pub tag: Option<Vec<String>>,
    pub x_exec: XExec,
}

impl BuildConfig {
    fn from_value_map(values: &IndexMap<String, Value>) -> Self {
        let mut config = BuildConfig::default();

        let to_string_vec = |value: &Value| -> Option<Vec<String>> {
            value.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        };

        config._disabled = values.get("_disabled").unwrap().as_bool().unwrap();
        config.pkg = values.get("pkg").unwrap().as_str().unwrap().to_string();
        if let Some(val) = values.get("pkg_id") {
            config.pkg_id = val.as_str().map(String::from);
        } else {
            config.pkg_id = Some(get_pkg_id(
                &to_string_vec(values.get("src_url").unwrap()).unwrap()[0],
            ));
        }
        if let Some(val) = values.get("pkg_type") {
            config.pkg_type = val.as_str().map(String::from);
        }
        if let Some(val) = values.get("app_id") {
            config.app_id = val.as_str().map(String::from);
        } else {
            config.app_id = Some(get_pkg_id(
                &to_string_vec(values.get("src_url").unwrap()).unwrap()[0],
            ));
        }
        if let Some(val) = values.get("build_util") {
            config.build_util = to_string_vec(val);
        }
        if let Some(val) = values.get("build_asset") {
            config.build_asset = val.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|asset| {
                        asset.as_mapping().map(|map| BuildAsset {
                            url: map
                                .get(Value::String("url".to_string()))
                                .and_then(|v| v.as_str())
                                .map(String::from)
                                .unwrap_or_default(),
                            out: map
                                .get(Value::String("out".to_string()))
                                .and_then(|v| v.as_str())
                                .map(String::from)
                                .unwrap_or_default(),
                        })
                    })
                    .collect()
            });
        }
        if let Some(val) = values.get("category") {
            config.category = to_string_vec(val).unwrap_or(vec!["Utility".to_string()]);
        }
        config.description = values
            .get("description")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        if let Some(val) = values.get("distro_pkg") {
            config.distro_pkg = DistroPkg::deserialize(val.clone()).ok();
        }
        if let Some(val) = values.get("homepage") {
            config.homepage = to_string_vec(val);
        }
        if let Some(val) = values.get("icon") {
            config.icon = val.as_str().map(String::from);
        }
        if let Some(val) = values.get("desktop") {
            config.desktop = val.as_str().map(String::from);
        }
        if let Some(val) = values.get("license") {
            config.license = to_string_vec(val);
        }
        if let Some(val) = values.get("maintainer") {
            config.maintainer = to_string_vec(val);
        }
        if let Some(val) = values.get("note") {
            config.note = to_string_vec(val);
        }
        if let Some(val) = values.get("provides") {
            config.provides = to_string_vec(val);
        }
        if let Some(val) = values.get("repology") {
            config.repology = to_string_vec(val);
        }
        config.src_url = to_string_vec(values.get("src_url").unwrap()).unwrap();
        if let Some(val) = values.get("tag") {
            config.tag = to_string_vec(val);
        }
        config.x_exec = XExec::deserialize(values.get("x_exec").unwrap()).unwrap();

        config
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
        if let Some(ref pkg_id) = self.pkg_id {
            writeln!(writer, "{}pkg_id: \"{}\"", indent_str, pkg_id)?;
        }

        write_field_comments(writer, "pkg_type")?;
        if let Some(ref pkg_type) = self.pkg_type {
            writeln!(writer, "{}pkg_type: \"{}\"", indent_str, pkg_type)?;
        }

        write_field_comments(writer, "pkgver")?;
        if let Some(ref pkgver) = self.pkgver {
            writeln!(writer, "{}pkgver: \"{}\"", indent_str, pkgver)?;
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

        write_field_comments(writer, "category")?;
        writeln!(writer, "{}category:", indent_str)?;
        for cat in &self.category {
            writeln!(writer, "{}  - \"{}\"", indent_str, cat)?;
        }

        write_field_comments(writer, "description")?;
        writeln!(
            writer,
            "{}description: \"{}\"",
            indent_str, self.description
        )?;

        write_field_comments(writer, "distro_pkg")?;
        if let Some(ref distro_pkg) = self.distro_pkg {
            writeln!(writer, "{}distro_pkg:", indent_str)?;
            distro_pkg.write_yaml(writer, indent)?;
        }

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

        write_field_comments(writer, "icon")?;
        if let Some(ref icon) = self.icon {
            writeln!(writer, "{}icon: \"{}\"", indent_str, icon)?;
        }

        write_field_comments(writer, "desktop")?;
        if let Some(ref desktop) = self.desktop {
            writeln!(writer, "{}desktop: \"{}\"", indent_str, desktop)?;
        }

        write_field_comments(writer, "license")?;
        if let Some(ref license) = self.license {
            writeln!(writer, "{}license:", indent_str)?;
            for l in license {
                writeln!(writer, "{}  - \"{}\"", indent_str, l)?;
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

        write_field_comments(writer, "x_exec")?;
        writeln!(writer, "{}x_exec:", indent_str)?;
        self.x_exec.write_yaml(writer, indent + 2)?;

        Ok(())
    }
}
