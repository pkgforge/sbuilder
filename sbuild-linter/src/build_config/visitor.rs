use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
};

use colored::Colorize;
use serde::{
    de::{self, Visitor},
    Deserialize,
};
use serde_yml::Value;

use crate::{
    distro_pkg::DistroPkg,
    error::{highlight_error_line, ErrorDetails},
    get_line_number_for_key,
    validator::{is_valid_alpha, is_valid_category, is_valid_url, FIELD_VALIDATORS},
    VALID_PKG_TYPES,
};

use super::BuildConfig;

pub struct BuildConfigVisitor {
    pub sbuild_str: String,
    pub visited: HashSet<String>,
    pub errors: Vec<ErrorDetails>,
}

impl BuildConfigVisitor {
    fn validate_distro_pkg_duplicates(
        &mut self,
        distro_pkg: &DistroPkg,
        field_path: &str,
        line_number: usize,
    ) {
        match distro_pkg {
            DistroPkg::List(list) => {
                self.check_duplicate_values(list, field_path, line_number);
            }
            DistroPkg::InnerNode(map) => {
                for (key, value) in map {
                    let new_path = if field_path.is_empty() {
                        key.clone()
                    } else {
                        format!("distro_pkg.{}.{}", field_path, key)
                    };

                    if !self.visited.insert(new_path.clone()) {
                        self.record_error(
                            new_path.clone(),
                            format!("'{}' field is duplicated", new_path),
                            line_number,
                        );
                        continue;
                    }

                    match value {
                        DistroPkg::List(list) => {
                            self.check_duplicate_values(list, &new_path, line_number);
                        }
                        DistroPkg::InnerNode(inner_map) => {
                            self.validate_distro_pkg_duplicates(
                                &DistroPkg::InnerNode(inner_map.clone()),
                                &new_path,
                                line_number,
                            );
                        }
                    }
                }
            }
        }
    }

    fn check_duplicate_values<T: Eq + Hash + Clone + std::fmt::Display>(
        &mut self,
        list: &[T],
        field: &str,
        line_number: usize,
    ) {
        let mut seen = HashSet::new();
        for item in list {
            if !seen.insert(item.clone()) {
                self.record_error(
                    field.to_string(),
                    format!("Duplicate value '{}' found in {}", item, field),
                    line_number,
                );
            }
        }
    }
}

impl BuildConfigVisitor {
    pub fn record_error(&mut self, field: String, message: String, line_number: usize) {
        let entry = self.errors.iter_mut().find(|e| e.field == field);
        match entry {
            Some(error_details) => {
                error_details.line_number = line_number;
            }
            None => {
                self.errors.push(ErrorDetails {
                    field,
                    message,
                    line_number,
                });
            }
        }
    }

    fn print_error(&self, error: &ErrorDetails) {
        eprintln!("{} -> {}", error.field.bold(), error.message.red());
        if error.line_number != 0 {
            highlight_error_line(&self.sbuild_str, error.line_number);
        }
    }
}

impl<'de> Visitor<'de> for BuildConfigVisitor {
    type Value = BuildConfig;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a sbuild config")
    }

    fn visit_map<A>(mut self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut values = HashMap::new();

        while let Some((key, value)) = map.next_entry::<String, Value>()? {
            let line_number = get_line_number_for_key(&self.sbuild_str, &key);

            if self.visited.contains(&key) {
                self.record_error(
                    key.clone(),
                    format!("'{}' field is duplicated", key),
                    line_number,
                );
                continue;
            }

            if let Some(validator) = FIELD_VALIDATORS.iter().find(|v| v.name == key) {
                if let Some(validated_value) =
                    validator.validate(&value, &mut self, line_number, validator.required)
                {
                    if key == "distro_pkg" {
                        if let Ok(distro_pkg) = DistroPkg::deserialize(validated_value.clone()) {
                            self.validate_distro_pkg_duplicates(&distro_pkg, "", line_number);
                        }
                    }
                    if key == "pkg" || key == "pkg_id" || key == "app_id" {
                        if let Some(value) = validated_value.as_str() {
                            if !is_valid_alpha(value) {
                                self.record_error(key.clone(), format!("Invalid '{}': '{}'. Value should only contain alphanumeric, +, -, _, .", key, value), line_number);
                            }
                        }
                    }
                    if key == "category" {
                        if let Some(value) = validated_value.as_sequence() {
                            for v in value {
                                let val = v.as_str().unwrap();
                                if !is_valid_category(val) {
                                    self.record_error(
                                        key.clone(),
                                        format!(
                                            "Invalid '{}': '{}' is not a valid category.",
                                            key, val
                                        ),
                                        line_number,
                                    );
                                }
                            }
                        }
                    }
                    if key == "pkg_type" {
                        if let Some(pkg_type) = validated_value.as_str() {
                            if !VALID_PKG_TYPES.contains(&pkg_type) {
                                self.record_error(
                                    key.clone(),
                                    format!(
                                        "Invalid '{}': '{}'. Valid values are: {:?}",
                                        key, pkg_type, VALID_PKG_TYPES
                                    ),
                                    line_number,
                                );
                            }
                        }
                    }
                    if key == "homepage" || key == "src_url" {
                        if let Some(value) = validated_value.as_sequence() {
                            for v in value {
                                let val = v.as_str().unwrap();
                                if !is_valid_url(val) {
                                    self.record_error(
                                        key.clone(),
                                        format!("Invalid '{}': '{}' is not a valid URL.", key, val),
                                        line_number,
                                    );
                                }
                            }
                        }
                    }
                    values.insert(key.clone(), validated_value);
                }
                self.visited.insert(key);
            } else {
                if FIELD_VALIDATORS.iter().find(|k| k.name == key).is_none() {
                    self.record_error(
                        key.clone(),
                        format!("'{}' is not a valid field.", key),
                        line_number,
                    );
                }
            }
        }

        for validator in FIELD_VALIDATORS {
            if validator.required && !self.visited.contains(validator.name) {
                self.record_error(
                    validator.name.to_string(),
                    format!("Missing required field: {}", validator.name),
                    0,
                );
            }
        }

        if !self.errors.is_empty() {
            for error in &self.errors {
                self.print_error(error);
            }
            return Err(de::Error::custom(format!(
                "{} error(s) found during deserialization.",
                self.errors.len()
            )));
        }

        Ok(BuildConfig::from_value_map(&values))
    }
}
