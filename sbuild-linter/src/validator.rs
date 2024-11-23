use std::collections::HashSet;

use serde_yml::{Mapping, Value};

use crate::{build_config::visitor::BuildConfigVisitor, error::Severity, VALID_CATEGORIES};

pub enum FieldType {
    Boolean,
    String,
    StringArray,
    BuildAsset,
    DistroPkg,
    XExec,
    Url,
}

pub struct FieldValidator {
    pub name: &'static str,
    field_type: FieldType,
    pub required: bool,
}

impl FieldValidator {
    const fn new(name: &'static str, field_type: FieldType, required: bool) -> Self {
        Self {
            name,
            field_type,
            required,
        }
    }

    pub fn validate(
        &self,
        value: &Value,
        visitor: &mut BuildConfigVisitor,
        line_number: usize,
        required: bool,
    ) -> Option<Value> {
        match &self.field_type {
            FieldType::Boolean => self.validate_boolean(value, visitor, line_number),
            FieldType::String => self.validate_string(value, visitor, line_number, required),
            FieldType::StringArray => {
                self.validate_string_array(value, visitor, line_number, required)
            }
            FieldType::BuildAsset => self.validate_build_asset(value, visitor, line_number),
            FieldType::DistroPkg => self.validate_distro_pkg(value, visitor, line_number),
            FieldType::XExec => self.validate_x_exec(value, visitor, line_number),
            FieldType::Url => self.validate_url(value, visitor, line_number, required),
        }
    }

    fn validate_boolean(
        &self,
        value: &Value,
        visitor: &mut BuildConfigVisitor,
        line_number: usize,
    ) -> Option<Value> {
        if value.is_bool() {
            Some(value.clone())
        } else {
            visitor.record_error(
                self.name.to_string(),
                format!("'{}' field must be a boolean", self.name),
                line_number,
                Severity::Error,
            );
            None
        }
    }

    fn validate_string(
        &self,
        value: &Value,
        visitor: &mut BuildConfigVisitor,
        line_number: usize,
        required: bool,
    ) -> Option<Value> {
        if let Some(s) = value.as_str() {
            if s.trim().is_empty() {
                if required {
                    visitor.record_error(
                        self.name.to_string(),
                        format!("'{}' field cannot be empty", self.name),
                        line_number,
                        Severity::Error,
                    );
                }
                None
            } else {
                Some(Value::String(s.to_string()))
            }
        } else {
            if required {
                visitor.record_error(
                    self.name.to_string(),
                    format!("'{}' field must be a string", self.name),
                    line_number,
                    Severity::Error,
                );
            }
            None
        }
    }

    fn validate_url(
        &self,
        value: &Value,
        visitor: &mut BuildConfigVisitor,
        line_number: usize,
        required: bool,
    ) -> Option<Value> {
        if let Some(Value::String(v)) = self.validate_string(value, visitor, line_number, required)
        {
            if is_valid_url(&v) {
                Some(Value::String(v))
            } else {
                visitor.record_error(
                    self.name.to_string(),
                    format!("'{}' field must be a valid URL", self.name),
                    line_number,
                    Severity::Error,
                );
                None
            }
        } else {
            None
        }
    }

    fn validate_string_array(
        &self,
        value: &Value,
        visitor: &mut BuildConfigVisitor,
        line_number: usize,
        required: bool,
    ) -> Option<Value> {
        if let Some(arr) = value.as_sequence() {
            let valid_strings: Vec<String> = arr
                .iter()
                .filter_map(|v| {
                    if let Some(s) = v.as_str() {
                        if !s.trim().is_empty() {
                            Some(s.to_string())
                        } else {
                            None
                        }
                    } else {
                        if !v.is_null() {
                            visitor.record_error(
                                self.name.to_string(),
                                format!(
                                    "'{}' field must only contain sequence of strings",
                                    self.name
                                ),
                                line_number,
                                Severity::Error,
                            );
                        }
                        None
                    }
                })
                .collect();

            if valid_strings.is_empty() {
                if required {
                    visitor.record_error(
                        self.name.to_string(),
                        format!(
                            "'{}' field must contain at least 1 non-empty string",
                            self.name
                        ),
                        line_number,
                        Severity::Error,
                    );
                }
                None
            } else {
                let mut seen = HashSet::new();
                for value in &valid_strings {
                    if !seen.insert(value) {
                        visitor.record_error(
                            self.name.to_string(),
                            format!(
                                "'{}' field must contain unique sequence of strings. Found duplicate '{}'",
                                self.name, value
                            ),
                            line_number,
                            Severity::Error
                        );
                        return None;
                    }
                }
                Some(Value::Sequence(
                    valid_strings.into_iter().map(Value::String).collect(),
                ))
            }
        } else {
            if required {
                visitor.record_error(
                    self.name.to_string(),
                    format!("'{}' field must be an array", self.name),
                    line_number,
                    Severity::Error,
                );
            }
            None
        }
    }

    fn validate_distro_pkg(
        &self,
        value: &Value,
        visitor: &mut BuildConfigVisitor,
        line_number: usize,
    ) -> Option<Value> {
        if value.is_mapping() {
            Some(value.clone())
        } else {
            visitor.record_error(
                self.name.to_string(),
                format!("'{}' field must be an object", self.name),
                line_number,
                Severity::Error,
            );
            None
        }
    }

    fn validate_build_asset(
        &self,
        value: &Value,
        visitor: &mut BuildConfigVisitor,
        line_number: usize,
    ) -> Option<Value> {
        if let Some(assets) = value.as_sequence() {
            let validated_assets: Vec<Value> = assets
                .iter()
                .filter_map(|asset| {
                    if let Some(map) = asset.as_mapping() {
                        let mut valid = true;
                        let mut validated_asset = Mapping::new();

                        if let Some(url) = map.get(&Value::String("url".to_string())) {
                            if let Some(url_str) = url.as_str() {
                                if !url_str.trim().is_empty() {
                                    if !is_valid_url(url_str) {
                                        visitor.record_error(
                                            "build_asset.url".to_string(),
                                            format!("'{}' is not a valid URL.", url_str),
                                            line_number,
                                            Severity::Error,
                                        );
                                        valid = false;
                                    } else {
                                        validated_asset.insert(
                                            Value::String("url".to_string()),
                                            Value::String(url_str.to_string()),
                                        );
                                    }
                                } else {
                                    visitor.record_error(
                                        "build_asset.url".to_string(),
                                        "URL cannot be empty".to_string(),
                                        line_number,
                                        Severity::Error,
                                    );
                                    valid = false;
                                }
                            } else {
                                visitor.record_error(
                                    "build_asset.url".to_string(),
                                    "URL must be a string".to_string(),
                                    line_number,
                                    Severity::Error,
                                );
                                valid = false;
                            }
                        } else {
                            visitor.record_error(
                                "build_asset".to_string(),
                                "Missing required 'url' field".to_string(),
                                line_number,
                                Severity::Error,
                            );
                            valid = false;
                        }

                        if let Some(out) = map.get(&Value::String("out".to_string())) {
                            if let Some(out_str) = out.as_str() {
                                if !out_str.trim().is_empty() {
                                    validated_asset.insert(
                                        Value::String("out".to_string()),
                                        Value::String(out_str.to_string()),
                                    );
                                } else {
                                    visitor.record_error(
                                        "build_asset.out".to_string(),
                                        "'out' field cannot be empty".to_string(),
                                        line_number,
                                        Severity::Error,
                                    );
                                    valid = false;
                                }
                            } else {
                                visitor.record_error(
                                    "build_asset.out".to_string(),
                                    "'out' field must be a string".to_string(),
                                    line_number,
                                    Severity::Error,
                                );
                                valid = false;
                            }
                        } else {
                            visitor.record_error(
                                "build_asset".to_string(),
                                "Missing required 'out' field".to_string(),
                                line_number,
                                Severity::Error,
                            );
                            valid = false;
                        }

                        if valid {
                            Some(Value::Mapping(validated_asset))
                        } else {
                            None
                        }
                    } else {
                        visitor.record_error(
                            "build_asset".to_string(),
                            "Each build asset must be an object".to_string(),
                            line_number,
                            Severity::Error,
                        );
                        None
                    }
                })
                .collect();

            if !validated_assets.is_empty() {
                Some(Value::Sequence(validated_assets))
            } else {
                visitor.record_error(
                    "build_asset".to_string(),
                    "No valid build assets found".to_string(),
                    line_number,
                    Severity::Error,
                );
                None
            }
        } else {
            visitor.record_error(
                "build_asset".to_string(),
                "Must be an array of build assets".to_string(),
                line_number,
                Severity::Error,
            );
            None
        }
    }

    fn validate_x_exec(
        &self,
        value: &Value,
        visitor: &mut BuildConfigVisitor,
        line_number: usize,
    ) -> Option<Value> {
        if let Some(map) = value.as_mapping() {
            let mut valid = true;
            let mut validated_x_exec = Mapping::new();

            if let Some(shell) = map.get(&Value::String("shell".to_string())) {
                if let Some(shell_str) = shell.as_str() {
                    if !shell_str.trim().is_empty() {
                        if which::which_global(shell_str).is_ok() {
                            validated_x_exec.insert(
                                Value::String("shell".to_string()),
                                Value::String(shell_str.to_string()),
                            );
                        } else {
                            visitor.record_error(
                                "x_exec.shell".to_string(),
                                format!("{} is not installed.", shell_str),
                                line_number,
                                Severity::Error,
                            );
                        }
                    } else {
                        visitor.record_error(
                            "x_exec.shell".to_string(),
                            "Shell cannot be empty".to_string(),
                            line_number,
                            Severity::Error,
                        );
                        valid = false;
                    }
                } else {
                    visitor.record_error(
                        "x_exec.shell".to_string(),
                        "Shell must be a string".to_string(),
                        line_number,
                        Severity::Error,
                    );
                    valid = false;
                }
            } else {
                visitor.record_error(
                    "x_exec".to_string(),
                    "Missing required 'shell' field".to_string(),
                    line_number,
                    Severity::Error,
                );
                valid = false;
            }

            if let Some(run) = map.get(&Value::String("run".to_string())) {
                if let Some(run_str) = run.as_str() {
                    if !run_str.trim().is_empty() {
                        validated_x_exec.insert(
                            Value::String("run".to_string()),
                            Value::String(run_str.to_string()),
                        );
                    } else {
                        visitor.record_error(
                            "x_exec.run".to_string(),
                            "'run' field cannot be empty".to_string(),
                            line_number,
                            Severity::Error,
                        );
                        valid = false;
                    }
                } else {
                    visitor.record_error(
                        "x_exec.run".to_string(),
                        "'run' field must be a string".to_string(),
                        line_number,
                        Severity::Error,
                    );
                    valid = false;
                }
            } else {
                visitor.record_error(
                    "x_exec".to_string(),
                    "Missing required 'run' field".to_string(),
                    line_number,
                    Severity::Error,
                );
                valid = false;
            }

            if let Some(pkgver) = map.get(&Value::String("pkgver".to_string())) {
                if let Some(str_val) = pkgver.as_str() {
                    validated_x_exec.insert(
                        Value::String("pkgver".to_string()),
                        Value::String(str_val.to_string()),
                    );
                } else {
                    visitor.record_error(
                        "x_exec.pkgver".to_string(),
                        "'pkgver' must be a string".to_string(),
                        line_number,
                        Severity::Error,
                    );
                    valid = false;
                }
            }

            if valid {
                Some(Value::Mapping(validated_x_exec))
            } else {
                None
            }
        } else {
            visitor.record_error(
                "x_exec".to_string(),
                "Must be an object".to_string(),
                line_number,
                Severity::Error,
            );
            None
        }
    }
}

pub const FIELD_VALIDATORS: &[FieldValidator] = &[
    FieldValidator::new("_disabled", FieldType::Boolean, true),
    FieldValidator::new("pkg", FieldType::String, true),
    FieldValidator::new("pkg_id", FieldType::String, false),
    FieldValidator::new("app_id", FieldType::String, false),
    FieldValidator::new("pkg_type", FieldType::String, false),
    FieldValidator::new("pkgver", FieldType::String, false),
    FieldValidator::new("build_util", FieldType::StringArray, false),
    FieldValidator::new("build_asset", FieldType::BuildAsset, false),
    FieldValidator::new("category", FieldType::StringArray, false),
    FieldValidator::new("description", FieldType::String, true),
    FieldValidator::new("distro_pkg", FieldType::DistroPkg, false),
    FieldValidator::new("homepage", FieldType::StringArray, false),
    FieldValidator::new("maintainer", FieldType::StringArray, false),
    FieldValidator::new("icon", FieldType::Url, false),
    FieldValidator::new("desktop", FieldType::Url, false),
    FieldValidator::new("license", FieldType::StringArray, false),
    FieldValidator::new("note", FieldType::StringArray, false),
    FieldValidator::new("provides", FieldType::StringArray, false),
    FieldValidator::new("repology", FieldType::StringArray, false),
    FieldValidator::new("src_url", FieldType::StringArray, true),
    FieldValidator::new("tag", FieldType::StringArray, false),
    FieldValidator::new("x_exec", FieldType::XExec, true),
];

pub fn is_valid_alpha(value: &str) -> bool {
    value
        .chars()
        .all(|c| c.is_alphanumeric() || c == '+' || c == '-' || c == '_' || c == '.')
}

pub fn is_valid_category(value: &str) -> bool {
    VALID_CATEGORIES.lines().any(|line| line.trim() == value)
}

pub fn is_valid_url(value: &str) -> bool {
    if let Some((scheme, rest)) = value.split_once("://") {
        if scheme.is_empty() || !["http", "https", "ftp"].contains(&scheme) {
            return false;
        }

        let mut parts = rest.splitn(2, '/'); // Split host and the rest of the path
        let host = parts.next().unwrap_or("");
        let remainder = parts.next().unwrap_or("");

        if host.is_empty()
            || !host.contains('.')
            || !host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':')
        {
            return false;
        }

        if !remainder
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ":/.-_?=&".contains(c))
        {
            return false;
        }

        true
    } else {
        false
    }
}
