use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufWriter, Write},
};

use serde::{Deserialize, Serialize};
use serde_yml::Value;

use crate::{distro_pkg::DistroPkg, xexec::XExec, BuildAsset};

pub mod visitor;

#[derive(Serialize, Debug, Default)]
pub struct BuildConfig {
    _disabled: bool,
    pkg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pkg_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pkg_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pkgver: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_util: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_asset: Option<Vec<BuildAsset>>,
    category: Vec<String>,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    distro_pkg: Option<DistroPkg>,
    #[serde(skip_serializing_if = "Option::is_none")]
    homepage: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    maintainer: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provides: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repology: Option<Vec<String>>,
    src_url: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tag: Option<Vec<String>>,
    x_exec: XExec,
}

impl BuildConfig {
    fn from_value_map(values: &HashMap<String, Value>) -> Self {
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
        }
        if let Some(val) = values.get("pkg_type") {
            config.pkg_type = val.as_str().map(String::from);
        }
        if let Some(val) = values.get("build_util") {
            config.build_util = to_string_vec(val);
        }
        if let Some(val) = values.get("build_asset") {
            config.build_asset = val.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|asset| {
                        if let Some(map) = asset.as_mapping() {
                            Some(BuildAsset {
                                url: map
                                    .get(&Value::String("url".to_string()))
                                    .and_then(|v| v.as_str())
                                    .map(String::from)
                                    .unwrap_or_default(),
                                out: map
                                    .get(&Value::String("out".to_string()))
                                    .and_then(|v| v.as_str())
                                    .map(String::from)
                                    .unwrap_or_default(),
                            })
                        } else {
                            None
                        }
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

    pub fn write_yaml(&self, writer: &mut BufWriter<File>, indent: usize) -> io::Result<()> {
        let indent_str = " ".repeat(indent);

        writeln!(writer, "{}pkg: \"{}\"", indent_str, self.pkg)?;

        if let Some(ref pkg_id) = self.pkg_id {
            writeln!(writer, "{}pkg_id: \"{}\"", indent_str, pkg_id)?;
        }

        if let Some(ref pkg_type) = self.pkg_type {
            writeln!(writer, "{}pkg_type: \"{}\"", indent_str, pkg_type)?;
        }

        if let Some(ref pkgver) = self.pkgver {
            writeln!(writer, "{}pkgver: \"{}\"", indent_str, pkgver)?;
        }

        if let Some(ref build_util) = self.build_util {
            writeln!(writer, "{}build_util:", indent_str)?;
            for util in build_util {
                writeln!(writer, "{}  - \"{}\"", indent_str, util)?;
            }
        }

        if let Some(ref build_asset) = self.build_asset {
            writeln!(writer, "{}build_asset:", indent_str)?;
            for asset in build_asset {
                writeln!(writer, "{}  - url: \"{}\"", indent_str, asset.url)?;
                writeln!(writer, "{}    out: \"{}\"", indent_str, asset.out)?;
            }
        }

        writeln!(writer, "{}category:", indent_str)?;
        for cat in &self.category {
            writeln!(writer, "{}  - \"{}\"", indent_str, cat)?;
        }

        writeln!(
            writer,
            "{}description: \"{}\"",
            indent_str, self.description
        )?;

        if let Some(ref distro_pkg) = self.distro_pkg {
            writeln!(writer, "{}distro_pkg:", indent_str)?;
            distro_pkg.write_yaml(writer, indent)?;
        }

        if let Some(ref homepage) = self.homepage {
            writeln!(writer, "{}homepage:", indent_str)?;
            for url in homepage {
                writeln!(writer, "{}  - \"{}\"", indent_str, url)?;
            }
        }

        if let Some(ref maintainer) = self.maintainer {
            writeln!(writer, "{}maintainer:", indent_str)?;
            for m in maintainer {
                writeln!(writer, "{}  - \"{}\"", indent_str, m)?;
            }
        }

        if let Some(ref icon) = self.icon {
            writeln!(writer, "{}icon: \"{}\"", indent_str, icon)?;
        }

        if let Some(ref license) = self.license {
            writeln!(writer, "{}license:", indent_str)?;
            for l in license {
                writeln!(writer, "{}  - \"{}\"", indent_str, l)?;
            }
        }

        if let Some(ref note) = self.note {
            writeln!(writer, "{}note:", indent_str)?;
            for n in note {
                writeln!(writer, "{}  - \"{}\"", indent_str, n)?;
            }
        }

        if let Some(ref provides) = self.provides {
            writeln!(writer, "{}provides:", indent_str)?;
            for p in provides {
                writeln!(writer, "{}  - \"{}\"", indent_str, p)?;
            }
        }

        if let Some(ref repology) = self.repology {
            writeln!(writer, "{}repology:", indent_str)?;
            for r in repology {
                writeln!(writer, "{}  - \"{}\"", indent_str, r)?;
            }
        }

        writeln!(writer, "{}src_url:", indent_str)?;
        for url in &self.src_url {
            writeln!(writer, "{}  - \"{}\"", indent_str, url)?;
        }

        if let Some(ref tag) = self.tag {
            writeln!(writer, "{}tag:", indent_str)?;
            for t in tag {
                writeln!(writer, "{}  - \"{}\"", indent_str, t)?;
            }
        }

        writeln!(writer, "{}x_exec:", indent_str)?;
        self.x_exec.write_yaml(writer, indent + 2)?;

        Ok(())
    }
}
