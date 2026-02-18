use std::collections::HashSet;

use colored::Colorize;
use indexmap::IndexMap;
use saphyr::MarkedYamlOwned;
use url::Url;

use crate::{
    build_config::BuildConfig,
    description::Description,
    error::{highlight_error_line, ErrorDetails, Severity},
    logger::TaskLogger,
    xexec::XExec,
    BuildAsset, VALID_ARCH, VALID_CATEGORIES, VALID_OS, VALID_PKG_TYPES,
};

pub struct ValidationContext {
    yaml_str: String,
    logger: TaskLogger,
    errors: Vec<ErrorDetails>,
    visited: HashSet<String>,
}

impl ValidationContext {
    pub fn new(yaml_str: &str, logger: TaskLogger) -> Self {
        Self {
            yaml_str: yaml_str.to_string(),
            logger,
            errors: Vec::new(),
            visited: HashSet::new(),
        }
    }

    fn line_of(node: &MarkedYamlOwned) -> usize {
        let line = node.span.start.line();
        if line != 0 {
            return line;
        }
        // Saphyr reports line 0 for block-style containers (sequences/mappings
        // that start on a new line after their key). Fall back to first child's line.
        if let Some(seq) = node.data.as_sequence() {
            if let Some(first) = seq.first() {
                return first.span.start.line();
            }
        }
        if let Some(map) = node.data.as_mapping() {
            if let Some((first_key, _)) = map.iter().next() {
                return first_key.span.start.line();
            }
        }
        0
    }

    fn error(&mut self, field: &str, message: &str, line: usize) {
        self.errors.push(ErrorDetails {
            field: field.to_string(),
            message: message.to_string(),
            line_number: line,
            severity: Severity::Error,
        });
    }

    fn warn(&mut self, field: &str, message: &str, line: usize) {
        self.errors.push(ErrorDetails {
            field: field.to_string(),
            message: message.to_string(),
            line_number: line,
            severity: Severity::Warn,
        });
    }

    fn expect_bool(&mut self, node: &MarkedYamlOwned, field: &str) -> Option<bool> {
        let line = Self::line_of(node);
        if let Some(b) = node.data.as_bool() {
            Some(b)
        } else {
            self.error(field, &format!("'{}' field must be a boolean", field), line);
            None
        }
    }

    fn expect_string(&mut self, node: &MarkedYamlOwned, field: &str) -> Option<String> {
        let line = Self::line_of(node);
        if let Some(s) = node.data.as_str() {
            Some(s.to_string())
        } else {
            self.error(field, &format!("'{}' field must be a string", field), line);
            None
        }
    }

    fn expect_non_empty_string(&mut self, node: &MarkedYamlOwned, field: &str) -> Option<String> {
        let line = Self::line_of(node);
        if let Some(s) = node.data.as_str() {
            if s.trim().is_empty() {
                self.error(field, &format!("'{}' field cannot be empty", field), line);
                None
            } else {
                Some(s.to_string())
            }
        } else {
            self.error(field, &format!("'{}' field must be a string", field), line);
            None
        }
    }

    fn expect_string_array(
        &mut self,
        node: &MarkedYamlOwned,
        field: &str,
        required: bool,
    ) -> Option<Vec<String>> {
        let line = Self::line_of(node);
        if let Some(seq) = node.data.as_sequence() {
            let mut seen = HashSet::new();
            let valid: Vec<String> = seq
                .iter()
                .filter_map(|v| {
                    if let Some(s) = v.data.as_str() {
                        if !s.trim().is_empty() {
                            Some(s.to_string())
                        } else {
                            None
                        }
                    } else {
                        if !v.data.is_null() {
                            self.error(
                                field,
                                &format!("'{}' field must only contain sequence of strings", field),
                                Self::line_of(v),
                            );
                        }
                        None
                    }
                })
                .filter(|s| seen.insert(s.clone()))
                .collect();

            if valid.is_empty() {
                if required {
                    self.error(
                        field,
                        &format!("'{}' field must contain at least 1 non-empty string", field),
                        line,
                    );
                }
                None
            } else {
                if valid.len() != seq.len() {
                    self.warn(
                        field,
                        &format!(
                            "'{}' field contains duplicates. Removed automatically..",
                            field
                        ),
                        line,
                    );
                }
                Some(valid)
            }
        } else {
            if required {
                self.error(field, &format!("'{}' field must be an array", field), line);
            }
            None
        }
    }

    fn mapping_get<'a>(node: &'a MarkedYamlOwned, key: &str) -> Option<&'a MarkedYamlOwned> {
        node.data.as_mapping_get(key)
    }

    fn validate_packages(
        &mut self,
        node: &MarkedYamlOwned,
    ) -> Option<Vec<(String, crate::build_config::PackageConfig)>> {
        let line = Self::line_of(node);
        let Some(mapping) = node.data.as_mapping() else {
            self.error("packages", "'packages' must be a mapping.", line);
            return None;
        };

        let mut packages = Vec::new();
        for (key_node, val_node) in mapping {
            let Some(pkg_name) = key_node.data.as_str() else {
                self.error(
                    "packages",
                    "Package name must be a string.",
                    Self::line_of(key_node),
                );
                continue;
            };

            let pkg_line = Self::line_of(val_node);
            let Some(pkg_map) = val_node.data.as_mapping() else {
                self.error(
                    &format!("packages.{}", pkg_name),
                    "Package entry must be a mapping.",
                    pkg_line,
                );
                continue;
            };

            let provides = pkg_map
                .iter()
                .find(|(k, _)| k.data.as_str() == Some("provides"))
                .and_then(|(_, v)| {
                    v.data.as_sequence().map(|seq| {
                        seq.iter()
                            .filter_map(|item| item.data.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                })
                .unwrap_or_default();

            if provides.is_empty() {
                self.warn(
                    &format!("packages.{}", pkg_name),
                    &format!("Package '{}' has no provides.", pkg_name),
                    pkg_line,
                );
            }

            packages.push((
                pkg_name.to_string(),
                crate::build_config::PackageConfig { provides },
            ));
        }

        if packages.is_empty() {
            self.error("packages", "'packages' must not be empty.", line);
            return None;
        }

        Some(packages)
    }

    fn validate_x_exec(&mut self, node: &MarkedYamlOwned) -> Option<XExec> {
        let line = Self::line_of(node);
        if node.data.as_mapping().is_none() {
            self.error("x_exec", "Must be an object", line);
            return None;
        }

        let mut valid = true;
        let mut x_exec = XExec::default();

        // container (optional)
        if let Some(container_node) = Self::mapping_get(node, "container") {
            if let Some(s) = self.expect_non_empty_string(container_node, "x_exec.container") {
                x_exec.container = Some(s);
            }
        }

        // shell (required)
        if let Some(shell_node) = Self::mapping_get(node, "shell") {
            if let Some(s) = self.expect_non_empty_string(shell_node, "x_exec.shell") {
                if x_exec.container.is_some() || which::which_global(&s).is_ok() {
                    x_exec.shell = s;
                } else {
                    self.error(
                        "x_exec.shell",
                        &format!("{} is not installed.", s),
                        Self::line_of(shell_node),
                    );
                    valid = false;
                }
            } else {
                valid = false;
            }
        } else {
            self.error("x_exec", "Missing required 'shell' field", line);
            valid = false;
        }

        // run (required)
        if let Some(run_node) = Self::mapping_get(node, "run") {
            if let Some(s) = self.expect_non_empty_string(run_node, "x_exec.run") {
                x_exec.run = s;
            } else {
                valid = false;
            }
        } else {
            self.error("x_exec", "Missing required 'run' field", line);
            valid = false;
        }

        // pkgver (optional)
        if let Some(pkgver_node) = Self::mapping_get(node, "pkgver") {
            if let Some(s) = self.expect_string(pkgver_node, "x_exec.pkgver") {
                x_exec.pkgver = Some(s);
            } else {
                valid = false;
            }
        }

        // entrypoint (optional)
        if let Some(ep_node) = Self::mapping_get(node, "entrypoint") {
            if let Some(s) = self.expect_string(ep_node, "x_exec.entrypoint") {
                x_exec.entrypoint = Some(s);
            } else {
                valid = false;
            }
        }

        // arch (optional)
        if let Some(arch_node) = Self::mapping_get(node, "arch") {
            if let Some(arr) = self.expect_lowered_string_array(arch_node, "x_exec.arch") {
                for s in &arr {
                    if !VALID_ARCH.contains(&s.as_str()) {
                        self.error(
                            "x_exec.arch",
                            &format!("'{}' is not a supported architecture.", s),
                            Self::line_of(arch_node),
                        );
                        valid = false;
                    }
                }
                if valid {
                    x_exec.arch = Some(arr);
                }
            } else {
                valid = false;
            }
        }

        // os (optional)
        if let Some(os_node) = Self::mapping_get(node, "os") {
            if let Some(arr) = self.expect_lowered_string_array(os_node, "x_exec.os") {
                for s in &arr {
                    if !VALID_OS.contains(&s.as_str()) {
                        self.error(
                            "x_exec.os",
                            &format!("'{}' is not a supported OS.", s),
                            Self::line_of(os_node),
                        );
                        valid = false;
                    }
                }
                if valid {
                    x_exec.os = Some(arr);
                }
            } else {
                valid = false;
            }
        }

        // host (optional)
        if let Some(host_node) = Self::mapping_get(node, "host") {
            if let Some(arr) = self.expect_lowered_string_array(host_node, "x_exec.host") {
                for s in &arr {
                    let parts: Vec<&str> = s.split('-').collect();
                    if !(parts.len() == 2
                        && VALID_ARCH.contains(&parts[0])
                        && VALID_OS.contains(&parts[1]))
                    {
                        self.error(
                            "x_exec.host",
                            &format!("'{}' is not a supported `arch-os` combination.", s),
                            Self::line_of(host_node),
                        );
                        valid = false;
                    }
                }
                if valid {
                    x_exec.host = Some(arr);
                }
            } else {
                valid = false;
            }
        }

        // conflicts (optional)
        if let Some(conflicts_node) = Self::mapping_get(node, "conflicts") {
            if let Some(arr) = self.expect_lowered_string_array(conflicts_node, "x_exec.conflicts")
            {
                x_exec.conflicts = Some(arr);
            }
        }

        // depends (optional)
        if let Some(depends_node) = Self::mapping_get(node, "depends") {
            if let Some(arr) = self.expect_lowered_string_array(depends_node, "x_exec.depends") {
                x_exec.depends = Some(arr);
            }
        }

        if valid {
            Some(x_exec)
        } else {
            None
        }
    }

    fn expect_lowered_string_array(
        &mut self,
        node: &MarkedYamlOwned,
        field: &str,
    ) -> Option<Vec<String>> {
        let line = Self::line_of(node);
        if let Some(seq) = node.data.as_sequence() {
            let mut seen = HashSet::new();
            let valid: Vec<String> = seq
                .iter()
                .filter_map(|v| {
                    if let Some(s) = v.data.as_str() {
                        if !s.trim().is_empty() {
                            Some(s.to_lowercase())
                        } else {
                            None
                        }
                    } else {
                        if !v.data.is_null() {
                            self.error(
                                field,
                                &format!("'{}' must only contain sequence of strings", field),
                                Self::line_of(v),
                            );
                        }
                        None
                    }
                })
                .filter(|s| seen.insert(s.clone()))
                .collect();

            if valid.len() != seq.len() {
                self.warn(
                    field,
                    &format!("'{}' contains duplicates. Removed automatically..", field),
                    line,
                );
            }
            Some(valid)
        } else {
            self.error(
                field,
                &format!("'{}' must be an array of strings", field),
                line,
            );
            None
        }
    }

    fn validate_description(&mut self, node: &MarkedYamlOwned) -> Option<Description> {
        let line = Self::line_of(node);
        match node.data.as_str() {
            Some(s) => {
                if s.trim().is_empty() {
                    self.error("description", "'description' field cannot be empty", line);
                    None
                } else {
                    Some(Description::Simple(s.to_string()))
                }
            }
            None => {
                if let Some(map) = node.data.as_mapping() {
                    if map.is_empty() {
                        self.error("description", "'description' field cannot be empty", line);
                        return None;
                    }
                    let mut valid = true;
                    let mut result = IndexMap::new();

                    for (k, v) in map {
                        let key_str = match k.data.as_str() {
                            Some(s) => s.to_string(),
                            None => {
                                if let Some(b) = k.data.as_bool() {
                                    b.to_string()
                                } else {
                                    self.error(
                                        "description",
                                        "Description key must be a string",
                                        Self::line_of(k),
                                    );
                                    valid = false;
                                    continue;
                                }
                            }
                        };

                        if let Some(val_str) = v.data.as_str() {
                            if !val_str.trim().is_empty() {
                                result.insert(key_str, val_str.to_string());
                            } else {
                                self.error(
                                    &format!("description.{}", key_str),
                                    "Description value cannot be empty",
                                    Self::line_of(v),
                                );
                                valid = false;
                            }
                        } else {
                            self.error(
                                &format!("description.{}", key_str),
                                "Description value must be a string",
                                Self::line_of(v),
                            );
                            valid = false;
                        }
                    }

                    if valid && !result.is_empty() {
                        Some(Description::Map(result))
                    } else {
                        None
                    }
                } else {
                    self.error(
                        "description",
                        "'description' field must be either a string or a mapping of strings",
                        line,
                    );
                    None
                }
            }
        }
    }

    fn validate_build_asset(&mut self, node: &MarkedYamlOwned) -> Option<Vec<BuildAsset>> {
        let line = Self::line_of(node);
        let seq = match node.data.as_sequence() {
            Some(s) => s,
            None => {
                self.error("build_asset", "Must be an array of build assets", line);
                return None;
            }
        };

        let mut assets = Vec::new();
        for asset_node in seq {
            let asset_line = Self::line_of(asset_node);
            if asset_node.data.as_mapping().is_none() {
                self.error(
                    "build_asset",
                    "Each build asset must be an object",
                    asset_line,
                );
                continue;
            }

            let mut valid = true;
            let mut url = String::new();
            let mut out = String::new();

            if let Some(url_node) = Self::mapping_get(asset_node, "url") {
                if let Some(u) = self.expect_non_empty_string(url_node, "build_asset.url") {
                    if !u.contains("${") && !is_valid_url(&u) {
                        self.error(
                            "build_asset.url",
                            &format!("'{}' is not a valid URL.", u),
                            Self::line_of(url_node),
                        );
                        valid = false;
                    } else {
                        url = u;
                    }
                } else {
                    valid = false;
                }
            } else {
                self.error("build_asset", "Missing required 'url' field", asset_line);
                valid = false;
            }

            if let Some(out_node) = Self::mapping_get(asset_node, "out") {
                if let Some(o) = self.expect_non_empty_string(out_node, "build_asset.out") {
                    out = o;
                } else {
                    valid = false;
                }
            } else {
                self.error("build_asset", "Missing required 'out' field", asset_line);
                valid = false;
            }

            if valid {
                assets.push(BuildAsset { url, out });
            }
        }

        if assets.is_empty() {
            self.error("build_asset", "No valid build assets found", line);
            None
        } else {
            Some(assets)
        }
    }

    pub fn validate(&mut self, doc: &MarkedYamlOwned) -> Option<BuildConfig> {
        let map = match doc.data.as_mapping() {
            Some(m) => m,
            None => {
                self.error("root", "YAML document must be a mapping", 0);
                self.report_errors();
                return None;
            }
        };

        let mut config = BuildConfig::default();
        let mut has_disabled = false;
        let mut has_pkg = false;
        let mut has_description = false;
        let mut has_src_url = false;
        let mut has_x_exec = false;

        for (key_node, val_node) in map {
            let key = match key_node.data.as_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            let line = Self::line_of(key_node);

            if self.visited.contains(&key) {
                self.error(&key, &format!("'{}' field is duplicated", key), line);
                continue;
            }
            self.visited.insert(key.clone());

            match key.as_str() {
                "_disabled" => {
                    if let Some(b) = self.expect_bool(val_node, "_disabled") {
                        config._disabled = b;
                        has_disabled = true;
                    }
                }
                "pkg" => {
                    if let Some(v) = self.expect_non_empty_string(val_node, "pkg") {
                        if !is_valid_alpha(&v) {
                            self.error(
                                "pkg",
                                &format!(
                                    "Invalid 'pkg': '{}'. Value should only contain alphanumeric, +, -, _, .",
                                    v
                                ),
                                line,
                            );
                        }
                        config.pkg = v;
                        has_pkg = true;
                    }
                }
                "pkg_id" => {
                    if let Some(v) = self.expect_non_empty_string(val_node, "pkg_id") {
                        if !is_valid_alpha(&v) {
                            self.error(
                                "pkg_id",
                                &format!(
                                    "Invalid 'pkg_id': '{}'. Value should only contain alphanumeric, +, -, _, .",
                                    v
                                ),
                                line,
                            );
                        }
                        config.pkg_id = v;
                    }
                }
                "app_id" => {
                    if let Some(v) = self.expect_non_empty_string(val_node, "app_id") {
                        if !is_valid_alpha(&v) {
                            self.error(
                                "app_id",
                                &format!(
                                    "Invalid 'app_id': '{}'. Value should only contain alphanumeric, +, -, _, .",
                                    v
                                ),
                                line,
                            );
                        }
                        config.app_id = Some(v);
                    }
                }
                "pkg_type" => {
                    if let Some(v) = self.expect_non_empty_string(val_node, "pkg_type") {
                        if !VALID_PKG_TYPES.contains(&v.as_str()) {
                            self.error(
                                "pkg_type",
                                &format!(
                                    "Invalid 'pkg_type': '{}'. Valid values are: {:?}",
                                    v, VALID_PKG_TYPES
                                ),
                                line,
                            );
                        }
                        config.pkg_type = Some(v);
                    }
                }
                "pkgver" | "version" => {
                    if let Some(v) = self.expect_string(val_node, &key) {
                        if !v.trim().is_empty() {
                            config.pkgver = Some(v);
                        }
                    }
                }
                "remote_pkgver" => {
                    if let Some(v) = self.expect_string(val_node, "remote_pkgver") {
                        if !v.trim().is_empty() {
                            config.remote_pkgver = Some(v);
                        }
                    }
                }
                "build_util" => {
                    config.build_util = self.expect_string_array(val_node, "build_util", false);
                }
                "build_asset" => {
                    config.build_asset = self.validate_build_asset(val_node);
                }
                "build_deps" => {
                    config.build_deps = self.expect_string_array(val_node, "build_deps", false);
                }
                "category" => {
                    if let Some(cats) = self.expect_string_array(val_node, "category", false) {
                        for c in &cats {
                            if !is_valid_category(c) {
                                self.error(
                                    "category",
                                    &format!(
                                        "Invalid 'category': '{}' is not a valid category.",
                                        c
                                    ),
                                    line,
                                );
                            }
                        }
                        config.category = cats;
                    }
                }
                "description" => {
                    if let Some(desc) = self.validate_description(val_node) {
                        config.description = Some(desc);
                        has_description = true;
                    }
                }
                "homepage" => {
                    if let Some(urls) = self.expect_string_array(val_node, "homepage", false) {
                        for u in &urls {
                            if !is_valid_url(u) {
                                self.error(
                                    "homepage",
                                    &format!("Invalid 'homepage': '{}' is not a valid URL.", u),
                                    line,
                                );
                            }
                        }
                        config.homepage = Some(urls);
                    }
                }
                "maintainer" => {
                    config.maintainer = self.expect_string_array(val_node, "maintainer", false);
                }
                "license" => {
                    config.license = self.expect_string_array(val_node, "license", false);
                }
                "note" => {
                    config.note = self.expect_string_array(val_node, "note", false);
                }
                "provides" => {
                    config.provides = self.expect_string_array(val_node, "provides", false);
                }
                "packages" => {
                    config.packages = self.validate_packages(val_node);
                }
                "repology" => {
                    config.repology = self.expect_string_array(val_node, "repology", false);
                }
                "src_url" => {
                    if let Some(urls) = self.expect_string_array(val_node, "src_url", true) {
                        for u in &urls {
                            if !is_valid_url(u) {
                                self.error(
                                    "src_url",
                                    &format!("Invalid 'src_url': '{}' is not a valid URL.", u),
                                    line,
                                );
                            }
                        }
                        config.src_url = urls;
                        has_src_url = true;
                    }
                }
                "tag" => {
                    config.tag = self.expect_string_array(val_node, "tag", false);
                }
                "ghcr_pkg" => {
                    if let Some(v) = self.expect_string(val_node, "ghcr_pkg") {
                        if !v.trim().is_empty() {
                            config.ghcr_pkg = Some(v);
                        }
                    }
                }
                "snapshots" => {
                    config.snapshots = self.expect_string_array(val_node, "snapshots", false);
                }
                "x_exec" => {
                    if let Some(x) = self.validate_x_exec(val_node) {
                        config.x_exec = x;
                        has_x_exec = true;
                    }
                }
                unknown => {
                    self.warn(
                        unknown,
                        &format!("'{}' is not a valid field.", unknown),
                        line,
                    );
                }
            }
        }

        // Check required fields
        if !has_disabled {
            self.error("_disabled", "Missing required field: _disabled", 0);
        }
        if !has_pkg {
            self.error("pkg", "Missing required field: pkg", 0);
        }
        if !has_description {
            self.error("description", "Missing required field: description", 0);
        }
        if !has_src_url {
            self.error("src_url", "Missing required field: src_url", 0);
        }
        if !has_x_exec {
            self.error("x_exec", "Missing required field: x_exec", 0);
        }

        // Set default category if empty
        if config.category.is_empty() {
            config.category = vec!["Utility".to_string()];
        }

        // Derive pkg_id from src_url if not explicitly set
        config.set_pkg_id_from_src_url();

        if self.has_fatal_errors() {
            self.report_errors();
            None
        } else {
            self.report_errors();
            Some(config)
        }
    }

    fn has_fatal_errors(&self) -> bool {
        self.errors
            .iter()
            .any(|e| matches!(e.severity, Severity::Error))
    }

    fn report_errors(&self) {
        if self.errors.is_empty() {
            return;
        }

        let fatal_count = self
            .errors
            .iter()
            .filter(|e| matches!(e.severity, Severity::Error))
            .count();
        let warn_count = self.errors.len() - fatal_count;

        for error in &self.errors {
            let is_fatal = matches!(error.severity, Severity::Error);
            if is_fatal {
                self.logger.error(&format!(
                    "{} -> {}",
                    error.field.bold(),
                    error.message.red()
                ));
            } else {
                self.logger.warn(&format!(
                    "{} -> {}",
                    error.field.bold(),
                    error.message.yellow()
                ));
            }
            if error.line_number != 0 {
                highlight_error_line(&self.yaml_str, error.line_number, is_fatal, &self.logger);
            }
        }

        if fatal_count > 0 {
            self.logger.custom_error(&format!(
                "{}{} found during deserialization.",
                format!("{} error(s)", fatal_count).red(),
                if warn_count > 0 {
                    format!(" & {} warning(s)", warn_count).yellow()
                } else {
                    "".yellow()
                }
            ));
        } else {
            self.logger.custom_error(&format!(
                "{} found during deserialization.",
                format!("{} warning(s)", warn_count).yellow()
            ));
        }
    }
}

pub fn is_valid_alpha(value: &str) -> bool {
    value
        .chars()
        .all(|c| c.is_alphanumeric() || c == '+' || c == '-' || c == '_' || c == '.')
}

pub fn is_valid_category(value: &str) -> bool {
    VALID_CATEGORIES.lines().any(|line| line.trim() == value)
}

pub fn is_valid_url(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };

    let scheme = url.scheme();
    if scheme.is_empty() || !["http", "https", "ftp"].contains(&scheme) {
        return false;
    }

    true
}
