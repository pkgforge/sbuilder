use std::collections::HashSet;

use serde_yml::{Mapping, Value};
use url::Url;

use crate::{
    build_config::visitor::BuildConfigVisitor, disabled::ComplexReason, error::Severity,
    VALID_ARCH, VALID_CATEGORIES, VALID_OS,
};

pub enum FieldType {
    Boolean,
    String,
    StringArray,
    BuildAsset,
    DistroPkg,
    XExec,
    Url,
    Description,
    Resource,
    License,
    DisabledReason,
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
            FieldType::Description => self.validate_description(value, visitor, line_number),
            FieldType::Resource => self.validate_resource(value, visitor, line_number),
            FieldType::License => self.validate_license(value, visitor, line_number),
            FieldType::DisabledReason => self.validate_disabled_reason(value, visitor, line_number),
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
            let mut seen = HashSet::new();
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
                .filter(|s| seen.insert(s.clone()))
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
                if valid_strings.len() != arr.len() {
                    visitor.record_error(
                        self.name.to_string(),
                        format!(
                            "'{}' field contains duplicates. Removed automatically..",
                            self.name
                        ),
                        line_number,
                        Severity::Warn,
                    );
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

    fn validate_description(
        &self,
        value: &Value,
        visitor: &mut BuildConfigVisitor,
        line_number: usize,
    ) -> Option<Value> {
        match value {
            Value::String(s) => {
                if s.trim().is_empty() {
                    visitor.record_error(
                        self.name.to_string(),
                        format!("'{}' field cannot be empty", self.name),
                        line_number,
                        Severity::Error,
                    );
                    None
                } else {
                    Some(value.clone())
                }
            }
            Value::Mapping(map) => {
                let mut valid = true;
                let mut validated_map = Mapping::new();

                if map.is_empty() {
                    visitor.record_error(
                        self.name.to_string(),
                        format!("'{}' field cannot be empty", self.name),
                        line_number,
                        Severity::Error,
                    );
                    return None;
                }

                for (key, val) in map {
                    let map_key = match key {
                        Value::String(s) => Some(s.to_string()),
                        Value::Bool(b) => Some(b.to_string()),
                        _ => None,
                    };

                    if let Some(key_str) = map_key {
                        if let Some(val_str) = val.as_str() {
                            if !val_str.trim().is_empty() {
                                validated_map.insert(
                                    Value::String(key_str),
                                    Value::String(val_str.to_string()),
                                );
                            } else {
                                visitor.record_error(
                                    format!("{}.{}", self.name, key_str),
                                    "Description value cannot be empty".to_string(),
                                    line_number,
                                    Severity::Error,
                                );
                                valid = false;
                            }
                        } else {
                            visitor.record_error(
                                format!("{}.{}", self.name, key_str),
                                "Description value must be a string".to_string(),
                                line_number,
                                Severity::Error,
                            );
                            valid = false;
                        }
                    } else {
                        visitor.record_error(
                            self.name.to_string(),
                            "Description key must be a string".to_string(),
                            line_number,
                            Severity::Error,
                        );
                        valid = false;
                    }
                }

                if valid && !validated_map.is_empty() {
                    Some(Value::Mapping(validated_map))
                } else {
                    None
                }
            }
            _ => {
                visitor.record_error(
                    self.name.to_string(),
                    format!(
                        "'{}' field must be either a string or a mapping of strings",
                        self.name
                    ),
                    line_number,
                    Severity::Error,
                );
                None
            }
        }
    }

    fn validate_disabled_reason(
        &self,
        value: &Value,
        visitor: &mut BuildConfigVisitor,
        line_number: usize,
    ) -> Option<Value> {
        match value {
            Value::String(_) => self.validate_string(value, visitor, line_number, false),
            Value::Sequence(_) => self.validate_string_array(value, visitor, line_number, false),
            Value::Mapping(map) => {
                let mut valid = true;
                let mut validated_map = Mapping::new();

                if map.len() != 1 {
                    visitor.record_error(
                        self.name.to_string(),
                        "'{}' field must contain exactly one key".to_string(),
                        line_number,
                        Severity::Error,
                    );
                    return None;
                }

                for (key, val) in map {
                    if let Some(key_str) = key.as_str() {
                        if let Value::Sequence(inner_seq) = val {
                            if let Some(inner_val) = inner_seq.first() {
                                if let Value::Mapping(inner_map) = inner_val {
                                    let mut complex_reason = ComplexReason {
                                        date: String::new(),
                                        pkg_id: None,
                                        reason: String::new(),
                                    };

                                    for (inner_key, inner_val) in inner_map {
                                        if let Some(inner_key_str) = inner_key.as_str() {
                                            match inner_key_str {
                                                "date" => {
                                                    if let Some(inner_val_str) = inner_val.as_str()
                                                    {
                                                        if !inner_val_str.trim().is_empty() {
                                                            complex_reason.date =
                                                                inner_val_str.to_string();
                                                        } else {
                                                            visitor.record_error(
                                                                format!(
                                                                    "{}.{}",
                                                                    self.name, key_str
                                                                ),
                                                                "Date cannot be empty".to_string(),
                                                                line_number,
                                                                Severity::Error,
                                                            );
                                                            valid = false;
                                                        }
                                                    } else {
                                                        visitor.record_error(
                                                            format!("{}.{}", self.name, key_str),
                                                            "Date must be a string".to_string(),
                                                            line_number,
                                                            Severity::Error,
                                                        );
                                                        valid = false;
                                                    }
                                                }
                                                "pkg_id" => {
                                                    if let Some(inner_val_str) = inner_val.as_str()
                                                    {
                                                        complex_reason.pkg_id =
                                                            Some(inner_val_str.to_string());
                                                    }
                                                }
                                                "reason" => {
                                                    if let Some(inner_val_str) = inner_val.as_str()
                                                    {
                                                        if !inner_val_str.trim().is_empty() {
                                                            complex_reason.reason =
                                                                inner_val_str.to_string();
                                                        } else {
                                                            visitor.record_error(
                                                                format!(
                                                                    "{}.{}",
                                                                    self.name, key_str
                                                                ),
                                                                "Reason cannot be empty"
                                                                    .to_string(),
                                                                line_number,
                                                                Severity::Error,
                                                            );
                                                            valid = false;
                                                        }
                                                    } else {
                                                        visitor.record_error(
                                                            format!("{}.{}", self.name, key_str),
                                                            "Reason must be a string".to_string(),
                                                            line_number,
                                                            Severity::Error,
                                                        );
                                                        valid = false;
                                                    }
                                                }
                                                _ => {
                                                    visitor.record_error(
                                                        format!("{}.{}", self.name, key_str),
                                                        "Invalid key".to_string(),
                                                        line_number,
                                                        Severity::Warn,
                                                    );
                                                    valid = false;
                                                }
                                            }
                                        } else {
                                            visitor.record_error(
                                                format!("{}.{}", self.name, key_str),
                                                "Key must be a string".to_string(),
                                                line_number,
                                                Severity::Error,
                                            );
                                            valid = false;
                                        }
                                    }

                                    if valid {
                                        validated_map.insert(
                                            Value::String(key_str.to_string()),
                                            Value::Mapping(inner_map.clone()),
                                        );
                                    }
                                } else {
                                    visitor.record_error(
                                        format!("{}.{}", self.name, key_str),
                                        "Value must be a mapping with disabled `date` and `reason`"
                                            .to_string(),
                                        line_number,
                                        Severity::Error,
                                    );
                                    valid = false;
                                }
                            }
                        }
                    } else {
                        visitor.record_error(
                            self.name.to_string(),
                            "Package name must be a string".to_string(),
                            line_number,
                            Severity::Error,
                        );
                        valid = false;
                    }
                }

                if valid && !validated_map.is_empty() {
                    Some(Value::Mapping(validated_map))
                } else {
                    None
                }
            }
            _ => {
                visitor.record_error(
                    self.name.to_string(),
                    format!(
                        "'{}' field must be either a string, sequence, or a mapping with `date` and `reason`",
                        self.name
                    ),
                    line_number,
                    Severity::Error,
                );
                None
            }
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

                        if let Some(url) = map.get(Value::String("url".to_string())) {
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

                        if let Some(out) = map.get(Value::String("out".to_string())) {
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

    fn validate_license(
        &self,
        value: &Value,
        visitor: &mut BuildConfigVisitor,
        line_number: usize,
    ) -> Option<Value> {
        if let Some(licenses) = value.as_sequence() {
            let validated_licenses: Vec<Value> = licenses
                .iter()
                .filter_map(|license| {
                    if let Some(map) = license.as_mapping() {
                        let mut valid = true;
                        let mut validated_license = Mapping::new();

                        if let Some(id) = map.get(Value::String("id".to_string())) {
                            if let Some(id_str) = id.as_str() {
                                if !id_str.trim().is_empty() {
                                    validated_license.insert(
                                        Value::String("id".to_string()),
                                        Value::String(id_str.to_string()),
                                    );
                                } else {
                                    visitor.record_error(
                                        format!("{}.url", &self.name),
                                        "License id cannot be empty".to_string(),
                                        line_number,
                                        Severity::Error,
                                    );
                                    valid = false;
                                }
                            } else {
                                visitor.record_error(
                                    format!("{}.id", &self.name),
                                    "License id must be a string".to_string(),
                                    line_number,
                                    Severity::Error,
                                );
                                valid = false;
                            }
                        } else {
                            visitor.record_error(
                                format!("{}.id", &self.name),
                                "License id is required".to_string(),
                                line_number,
                                Severity::Error,
                            );
                            valid = false;
                        }

                        if let Some(url) = map.get(Value::String("url".to_string())) {
                            if let Some(url_str) = url.as_str() {
                                if !url_str.trim().is_empty() {
                                    if !is_valid_url(url_str) {
                                        visitor.record_error(
                                            format!("{}.url", &self.name),
                                            format!("'{}' is not a valid URL.", url_str),
                                            line_number,
                                            Severity::Error,
                                        );
                                        valid = false;
                                    } else {
                                        validated_license.insert(
                                            Value::String("url".to_string()),
                                            Value::String(url_str.to_string()),
                                        );
                                    }
                                } else {
                                    visitor.record_error(
                                        format!("{}.url", &self.name),
                                        "URL cannot be empty".to_string(),
                                        line_number,
                                        Severity::Error,
                                    );
                                    valid = false;
                                }
                            } else {
                                visitor.record_error(
                                    format!("{}.url", &self.name),
                                    "URL must be a string".to_string(),
                                    line_number,
                                    Severity::Error,
                                );
                                valid = false;
                            }
                        }

                        if let Some(file) = map.get(Value::String("file".to_string())) {
                            if let Some(file_str) = file.as_str() {
                                if !file_str.trim().is_empty() {
                                    validated_license.insert(
                                        Value::String("file".to_string()),
                                        Value::String(file_str.to_string()),
                                    );
                                } else {
                                    visitor.record_error(
                                        format!("{}.file", &self.name),
                                        "'file' field cannot be empty".to_string(),
                                        line_number,
                                        Severity::Error,
                                    );
                                    valid = false;
                                }
                            } else {
                                visitor.record_error(
                                    format!("{}.file", &self.name),
                                    "'file' field must be a string".to_string(),
                                    line_number,
                                    Severity::Error,
                                );
                                valid = false;
                            }
                        }

                        if valid {
                            Some(Value::Mapping(validated_license))
                        } else {
                            None
                        }
                    } else if let Some(v_str) = license.as_str() {
                        if !v_str.trim().is_empty() {
                            Some(Value::String(v_str.to_string()))
                        } else {
                            visitor.record_error(
                                self.name.to_string(),
                                "'license' cannot be empty".to_string(),
                                line_number,
                                Severity::Error,
                            );
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();

            if !validated_licenses.is_empty() {
                Some(Value::Sequence(validated_licenses))
            } else {
                visitor.record_error(
                    "license".to_string(),
                    "No valid license found".to_string(),
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

    fn validate_resource(
        &self,
        value: &Value,
        visitor: &mut BuildConfigVisitor,
        line_number: usize,
    ) -> Option<Value> {
        if let Some(map) = value.as_mapping() {
            let mut valid = true;
            let mut validated_map = Mapping::new();

            if let Some(url) = map.get(Value::String("url".to_string())) {
                if let Some(url_str) = url.as_str() {
                    if !url_str.trim().is_empty() {
                        if !is_valid_url(url_str) {
                            visitor.record_error(
                                format!("{}.url", &self.name),
                                format!("'{}' is not a valid URL.", url_str),
                                line_number,
                                Severity::Error,
                            );
                            valid = false;
                        } else {
                            validated_map.insert(
                                Value::String("url".to_string()),
                                Value::String(url_str.to_string()),
                            );
                        }
                    } else {
                        visitor.record_error(
                            format!("{}.url", &self.name),
                            "URL cannot be empty".to_string(),
                            line_number,
                            Severity::Error,
                        );
                        valid = false;
                    }
                } else {
                    visitor.record_error(
                        format!("{}.url", &self.name),
                        "URL must be a string".to_string(),
                        line_number,
                        Severity::Error,
                    );
                    valid = false;
                }
            }

            if let Some(file) = map.get(Value::String("file".to_string())) {
                if let Some(file_str) = file.as_str() {
                    if !file_str.trim().is_empty() {
                        validated_map.insert(
                            Value::String("file".to_string()),
                            Value::String(file_str.to_string()),
                        );
                    } else {
                        visitor.record_error(
                            format!("{}.file", &self.name),
                            "'file' field cannot be empty".to_string(),
                            line_number,
                            Severity::Error,
                        );
                        valid = false;
                    }
                } else {
                    visitor.record_error(
                        format!("{}.file", &self.name),
                        "'file' field must be a string".to_string(),
                        line_number,
                        Severity::Error,
                    );
                    valid = false;
                }
            }

            if let Some(dir) = map.get(Value::String("dir".to_string())) {
                if let Some(dir_str) = dir.as_str() {
                    if !dir_str.trim().is_empty() {
                        validated_map.insert(
                            Value::String("dir".to_string()),
                            Value::String(dir_str.to_string()),
                        );
                    } else {
                        visitor.record_error(
                            format!("{}.dir", &self.name),
                            "'dir' field cannot be empty".to_string(),
                            line_number,
                            Severity::Error,
                        );
                        valid = false;
                    }
                } else {
                    visitor.record_error(
                        format!("{}.dir", &self.name),
                        "'dir' field must be a string".to_string(),
                        line_number,
                        Severity::Error,
                    );
                    valid = false;
                }
            }

            if valid {
                if validated_map.is_empty() {
                    visitor.record_error(
                        self.name.to_string(),
                        "Must contain atleast one of `url`, `file` or `dir`".to_string(),
                        line_number,
                        Severity::Error,
                    );
                    None
                } else {
                    Some(Value::Mapping(validated_map))
                }
            } else {
                None
            }
        } else {
            visitor.record_error(
                format!("{}", &self.name),
                "Must contain atleast one of `url`, `file` or `dir`".to_string(),
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

            if let Some(shell) = map.get(Value::String("shell".to_string())) {
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

            if let Some(run) = map.get(Value::String("run".to_string())) {
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

            if let Some(pkgver) = map.get(Value::String("pkgver".to_string())) {
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

            if let Some(entrypoint) = map.get(Value::String("entrypoint".to_string())) {
                if let Some(str_val) = entrypoint.as_str() {
                    validated_x_exec.insert(
                        Value::String("entrypoint".to_string()),
                        Value::String(str_val.to_string()),
                    );
                } else {
                    visitor.record_error(
                        "x_exec.entrypoint".to_string(),
                        "'entrypoint' must be a string".to_string(),
                        line_number,
                        Severity::Error,
                    );
                    valid = false;
                }
            }

            if let Some(arch) = map.get(Value::String("arch".to_string())) {
                if let Some(arr) = arch.as_sequence() {
                    let mut seen = HashSet::new();
                    let valid_strings: Vec<String> = arr
                        .iter()
                        .filter_map(|v| {
                            if let Some(s) = v.as_str() {
                                if !s.trim().is_empty() {
                                    Some(s.to_lowercase())
                                } else {
                                    None
                                }
                            } else {
                                if !v.is_null() {
                                    visitor.record_error(
                                        "x_exec.arch".to_string(),
                                        format!(
                                            "'{}.arch' must only contain sequence of strings",
                                            self.name
                                        ),
                                        line_number,
                                        Severity::Error,
                                    );
                                }
                                None
                            }
                        })
                        .filter(|s| seen.insert(s.clone()))
                        .collect();

                    for s in &valid_strings {
                        if !VALID_ARCH.contains(&s.as_str()) {
                            visitor.record_error(
                                "x_exec.arch".to_string(),
                                format!("'{}' is not a supported architecture.", s),
                                line_number,
                                Severity::Error,
                            );
                            valid = false;
                        }
                    }

                    if valid {
                        if valid_strings.len() != arr.len() {
                            visitor.record_error(
                                self.name.to_string(),
                                format!(
                                    "'{}.arch' field contains duplicates. Removed automatically..",
                                    self.name
                                ),
                                line_number,
                                Severity::Warn,
                            );
                        }
                        validated_x_exec.insert(
                            Value::String("arch".to_string()),
                            Value::Sequence(valid_strings.into_iter().map(Value::String).collect()),
                        );
                    }
                } else {
                    visitor.record_error(
                        "x_exec.arch".to_string(),
                        format!("'{}.arch' must be an array of strings", self.name),
                        line_number,
                        Severity::Error,
                    );
                    valid = false;
                }
            }

            if let Some(os) = map.get(Value::String("os".to_string())) {
                if let Some(arr) = os.as_sequence() {
                    let mut seen = HashSet::new();
                    let valid_strings: Vec<String> = arr
                        .iter()
                        .filter_map(|v| {
                            if let Some(s) = v.as_str() {
                                if !s.trim().is_empty() {
                                    Some(s.to_lowercase())
                                } else {
                                    None
                                }
                            } else {
                                if !v.is_null() {
                                    visitor.record_error(
                                        "x_exec.os".to_string(),
                                        format!(
                                            "'{}.os' must only contain sequence of strings",
                                            self.name
                                        ),
                                        line_number,
                                        Severity::Error,
                                    );
                                }
                                None
                            }
                        })
                        .filter(|s| seen.insert(s.clone()))
                        .collect();

                    for s in &valid_strings {
                        if !VALID_OS.contains(&s.as_str()) {
                            visitor.record_error(
                                "x_exec.os".to_string(),
                                format!("'{}' is not a supported OS.", s),
                                line_number,
                                Severity::Error,
                            );
                            valid = false;
                        }
                    }

                    if valid {
                        if valid_strings.len() != arr.len() {
                            visitor.record_error(
                                self.name.to_string(),
                                format!(
                                    "'{}.os' contains duplicates. Removed automatically..",
                                    self.name
                                ),
                                line_number,
                                Severity::Warn,
                            );
                        }
                        validated_x_exec.insert(
                            Value::String("os".to_string()),
                            Value::Sequence(valid_strings.into_iter().map(Value::String).collect()),
                        );
                    }
                } else {
                    visitor.record_error(
                        "x_exec.os".to_string(),
                        format!("'{}.os' must be an array of strings", self.name),
                        line_number,
                        Severity::Error,
                    );
                    valid = false;
                }
            }

            if let Some(host) = map.get(Value::String("host".to_string())) {
                if let Some(arr) = host.as_sequence() {
                    let mut seen = HashSet::new();
                    let valid_strings: Vec<String> = arr
                        .iter()
                        .filter_map(|v| {
                            if let Some(s) = v.as_str() {
                                if !s.trim().is_empty() {
                                    Some(s.to_lowercase())
                                } else {
                                    None
                                }
                            } else {
                                if !v.is_null() {
                                    visitor.record_error(
                                        "x_exec.host".to_string(),
                                        format!(
                                            "'{}.host' must only contain sequence of strings",
                                            self.name
                                        ),
                                        line_number,
                                        Severity::Error,
                                    );
                                }
                                None
                            }
                        })
                        .filter(|s| seen.insert(s.clone()))
                        .collect();

                    for s in &valid_strings {
                        let parts: Vec<&str> = s.split('-').collect();
                        if !(parts.len() == 2
                            && VALID_ARCH.contains(&parts[0])
                            && VALID_OS.contains(&parts[1]))
                        {
                            visitor.record_error(
                                "x_exec.host".to_string(),
                                format!("'{}' is not a supported `arch-os` combination.", s),
                                line_number,
                                Severity::Error,
                            );
                            valid = false;
                        }
                    }

                    if valid {
                        if valid_strings.len() != arr.len() {
                            visitor.record_error(
                                self.name.to_string(),
                                format!(
                                    "'{}.host' field contains duplicates. Removed automatically..",
                                    self.name
                                ),
                                line_number,
                                Severity::Warn,
                            );
                        }
                        validated_x_exec.insert(
                            Value::String("host".to_string()),
                            Value::Sequence(valid_strings.into_iter().map(Value::String).collect()),
                        );
                    }
                } else {
                    visitor.record_error(
                        "x_exec.host".to_string(),
                        format!("'{}.host' must be an array of strings", self.name),
                        line_number,
                        Severity::Error,
                    );
                    valid = false;
                }
            }

            if let Some(conflicts) = map.get(Value::String("conflicts".to_string())) {
                if let Some(arr) = conflicts.as_sequence() {
                    let mut seen = HashSet::new();
                    let valid_strings: Vec<String> = arr
                        .iter()
                        .filter_map(|v| {
                            if let Some(s) = v.as_str() {
                                if !s.trim().is_empty() {
                                    Some(s.to_lowercase())
                                } else {
                                    None
                                }
                            } else {
                                if !v.is_null() {
                                    visitor.record_error(
                                        "x_exec.conflicts".to_string(),
                                        format!(
                                            "'{}.conflicts' must only contain sequence of strings",
                                            self.name
                                        ),
                                        line_number,
                                        Severity::Error,
                                    );
                                }
                                None
                            }
                        })
                        .filter(|s| seen.insert(s.clone()))
                        .collect();

                    if valid {
                        if valid_strings.len() != arr.len() {
                            visitor.record_error(
                                self.name.to_string(),
                                format!(
                                    "'{}.conflicts' contains duplicates. Removed automatically..",
                                    self.name
                                ),
                                line_number,
                                Severity::Warn,
                            );
                        }
                        validated_x_exec.insert(
                            Value::String("conflicts".to_string()),
                            Value::Sequence(valid_strings.into_iter().map(Value::String).collect()),
                        );
                    }
                } else {
                    visitor.record_error(
                        "x_exec.conflicts".to_string(),
                        format!("'{}.conflicts' must be an array of strings", self.name),
                        line_number,
                        Severity::Error,
                    );
                    valid = false;
                }
            }

            if let Some(depends) = map.get(Value::String("depends".to_string())) {
                if let Some(arr) = depends.as_sequence() {
                    let mut seen = HashSet::new();
                    let valid_strings: Vec<String> = arr
                        .iter()
                        .filter_map(|v| {
                            if let Some(s) = v.as_str() {
                                if !s.trim().is_empty() {
                                    Some(s.to_lowercase())
                                } else {
                                    None
                                }
                            } else {
                                if !v.is_null() {
                                    visitor.record_error(
                                        "x_exec.depends".to_string(),
                                        format!(
                                            "'{}.depends' must only contain sequence of strings",
                                            self.name
                                        ),
                                        line_number,
                                        Severity::Error,
                                    );
                                }
                                None
                            }
                        })
                        .filter(|s| seen.insert(s.clone()))
                        .collect();

                    if valid {
                        if valid_strings.len() != arr.len() {
                            visitor.record_error(
                                self.name.to_string(),
                                format!(
                                    "'{}.depends' contains duplicates. Removed automatically..",
                                    self.name
                                ),
                                line_number,
                                Severity::Warn,
                            );
                        }
                        validated_x_exec.insert(
                            Value::String("depends".to_string()),
                            Value::Sequence(valid_strings.into_iter().map(Value::String).collect()),
                        );
                    }
                } else {
                    visitor.record_error(
                        "x_exec.depends".to_string(),
                        format!("'{}.depends' must be an array of strings", self.name),
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
    FieldValidator::new("_disabled_reason", FieldType::DisabledReason, false),
    FieldValidator::new("pkg", FieldType::String, true),
    FieldValidator::new("pkg_id", FieldType::String, false),
    FieldValidator::new("app_id", FieldType::String, false),
    FieldValidator::new("pkg_type", FieldType::String, false),
    FieldValidator::new("pkgver", FieldType::String, false),
    FieldValidator::new("version", FieldType::String, false), // Alias for pkgver
    FieldValidator::new("remote_pkgver", FieldType::String, false),
    FieldValidator::new("build_util", FieldType::StringArray, false),
    FieldValidator::new("build_asset", FieldType::BuildAsset, false),
    FieldValidator::new("category", FieldType::StringArray, false),
    FieldValidator::new("description", FieldType::Description, true),
    FieldValidator::new("distro_pkg", FieldType::DistroPkg, false),
    FieldValidator::new("homepage", FieldType::StringArray, false),
    FieldValidator::new("maintainer", FieldType::StringArray, false),
    FieldValidator::new("icon", FieldType::Resource, false),
    FieldValidator::new("desktop", FieldType::Resource, false),
    FieldValidator::new("license", FieldType::License, false),
    FieldValidator::new("note", FieldType::StringArray, false),
    FieldValidator::new("provides", FieldType::StringArray, false),
    FieldValidator::new("repology", FieldType::StringArray, false),
    FieldValidator::new("src_url", FieldType::StringArray, true),
    FieldValidator::new("tag", FieldType::StringArray, false),
    FieldValidator::new("ghcr_pkg", FieldType::String, false),
    FieldValidator::new("snapshots", FieldType::StringArray, false),
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
    let Ok(url) = Url::parse(value) else {
        return false;
    };

    let scheme = url.scheme();
    if scheme.is_empty() || !["http", "https", "ftp"].contains(&scheme) {
        return false;
    }

    return true;
}
