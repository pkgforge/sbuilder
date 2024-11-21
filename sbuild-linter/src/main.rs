use std::{
    collections::{HashMap, HashSet},
    env,
    fs::File,
    hash::Hash,
    io::{BufRead, BufReader, BufWriter, Write},
};

use colored::Colorize;
use distro_pkg::DistroPkg;
use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer, Serialize,
};
use serde_yml::Value;
use validator::{is_valid_alpha, is_valid_category, is_valid_url, FIELD_VALIDATORS};

mod distro_pkg;
mod validator;

#[derive(Debug)]
struct ErrorDetails {
    field: String,
    message: String,
    line_number: usize,
}

#[derive(Serialize, Debug, Deserialize, Clone)]
struct BuildAsset {
    url: String,
    out: String,
}

#[derive(Serialize, Debug, Default)]
struct BuildConfig {
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

struct BuildConfigVisitor {
    sbuild_str: String,
    visited: HashSet<String>,
    errors: Vec<ErrorDetails>,
}

const VALID_PKG_TYPES: [&str; 9] = [
    "appbundle",
    "appimage",
    "archive",
    "dynamic",
    "flatimage",
    "gameimage",
    "nixappimage",
    "runimage",
    "static",
];
const VALID_CATEGORIES: &str = include_str!("categories");

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

impl BuildConfigVisitor {
    fn record_error(&mut self, field: String, message: String, line_number: usize) {
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
}

#[derive(Serialize, Debug, Deserialize, Default, Clone)]
struct XExec {
    disable_shellcheck: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pkgver: Option<String>,
    shell: String,
    run: String,
}

fn get_line_number_for_key(yaml_str: &str, key: &str) -> usize {
    let mut line_number = 0;
    for (index, line) in yaml_str.lines().enumerate() {
        if line.contains(key) {
            line_number = index + 1;
            break;
        }
    }
    line_number
}

fn highlight_error_line(yaml_str: &str, line_number: usize) {
    let context_range = 3;
    let start_line = if line_number > context_range {
        line_number - context_range
    } else {
        0
    };
    let end_line = if line_number + context_range < yaml_str.lines().count() {
        line_number + context_range
    } else {
        yaml_str.lines().count()
    };

    let lines: Vec<&str> = yaml_str
        .lines()
        .skip(start_line)
        .take(end_line - start_line)
        .collect();

    for (index, line) in lines.iter().enumerate() {
        let current_line_number = start_line + index + 1;
        if current_line_number == line_number {
            println!(
                "{}",
                format!("--> {}: {}", current_line_number, line)
                    .red()
                    .bold()
            );
        } else {
            println!("    {}: {}", current_line_number, line);
        }
    }
    println!();
}

fn deserialize_yaml(yaml_str: &str) -> Result<BuildConfig, serde_yml::Error> {
    let deserializer = serde_yml::Deserializer::from_str(yaml_str);
    let visitor = BuildConfigVisitor {
        sbuild_str: yaml_str.to_string(),
        visited: HashSet::new(),
        errors: Vec::new(),
    };
    deserializer.deserialize_map(visitor)
}

fn read_yaml_with_header(
    file_path: &str,
) -> Result<(Vec<String>, String), Box<dyn std::error::Error>> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    let mut header_lines = Vec::new();
    let mut yaml_content = String::new();
    let mut lines = reader.lines();

    if let Some(line) = lines.next() {
        let line = line?;
        if !line.trim_start().starts_with("#!/SBUILD") {
            return Err(format!("File must start with '#!/SBUILD', found: {}", line).into());
        }
        header_lines.push(line);
    } else {
        return Err("File is missing the required '#!/SBUILD' shebang".into());
    }

    if let Some(line) = lines.next() {
        let line = line?;
        if line.trim_start().starts_with('#') {
            header_lines.push(line);
        } else {
            yaml_content.push_str(&line);
            yaml_content.push('\n');
        }
    }

    for line in lines {
        let line = line?;
        yaml_content.push_str(&line);
        yaml_content.push('\n');
    }

    Ok((header_lines, yaml_content))
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <file-path>", args[0]);
        std::process::exit(1);
    }

    let file_path = &args[1];
    let (headers, yaml_str) = read_yaml_with_header(&file_path).unwrap();

    match deserialize_yaml(&yaml_str) {
        Ok(config) => {
            let output_path = format!("{}.validated", file_path);
            let file = File::create(&output_path).unwrap();
            let mut writer = BufWriter::new(&file);

            for line in headers {
                writeln!(writer, "{}", line).unwrap();
            }
            writeln!(writer).unwrap();
            serde_yml::to_writer(writer, &config).unwrap();
            println!("Validated YAML has been written to {}", output_path);
        }
        Err(e) => {
            eprintln!("{}", e.to_string());
        }
    };
}
