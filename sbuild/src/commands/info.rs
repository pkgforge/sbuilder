use clap::{Parser, ValueEnum};
use colored::Colorize;
use saphyr::{LoadableYamlNode, YamlOwned};
use sbuild::fetch_recipe;

#[derive(Parser)]
#[command(about = "Get information about an SBUILD recipe")]
pub struct InfoArgs {
    #[arg(required = true)]
    pub recipe: String,

    #[arg(long)]
    pub check_host: Option<String>,

    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    #[arg(long)]
    pub field: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

fn yaml_to_json(yaml: &YamlOwned) -> serde_json::Value {
    if let Some(s) = yaml.as_str() {
        serde_json::Value::String(s.to_string())
    } else if let Some(b) = yaml.as_bool() {
        serde_json::Value::Bool(b)
    } else if let Some(i) = yaml.as_integer() {
        serde_json::json!(i)
    } else if let Some(f) = yaml.as_floating_point() {
        serde_json::json!(f)
    } else if yaml.is_null() {
        serde_json::Value::Null
    } else if let Some(seq) = yaml.as_sequence() {
        serde_json::Value::Array(seq.iter().map(yaml_to_json).collect())
    } else if let Some(map) = yaml.as_mapping() {
        let obj: serde_json::Map<String, serde_json::Value> = map
            .iter()
            .filter_map(|(k, v)| {
                let key = k.as_str().map(|s| s.to_string())?;
                Some((key, yaml_to_json(v)))
            })
            .collect();
        serde_json::Value::Object(obj)
    } else {
        serde_json::Value::Null
    }
}

fn yaml_value_to_string(yaml: &YamlOwned) -> String {
    if let Some(s) = yaml.as_str() {
        s.to_string()
    } else if let Some(b) = yaml.as_bool() {
        b.to_string()
    } else if let Some(i) = yaml.as_integer() {
        i.to_string()
    } else if let Some(f) = yaml.as_floating_point() {
        f.to_string()
    } else {
        serde_json::to_string_pretty(&yaml_to_json(yaml)).unwrap_or_default()
    }
}

fn get_str(yaml: &YamlOwned, key: &str) -> Option<String> {
    yaml.as_mapping_get(key)
        .and_then(|v: &YamlOwned| v.as_str())
        .map(|s| s.to_string())
}

fn get_hosts(yaml: &YamlOwned) -> Option<Vec<String>> {
    let x_exec = yaml.as_mapping_get("x_exec")?;
    let host = x_exec.as_mapping_get("host")?;
    let seq = host.as_sequence()?;
    Some(
        seq.iter()
            .filter_map(|h: &YamlOwned| h.as_str().map(|s| s.to_string()))
            .collect(),
    )
}

pub async fn run(args: InfoArgs) -> Result<(), String> {
    let content = if args.recipe.starts_with("http://") || args.recipe.starts_with("https://") {
        fetch_recipe(&args.recipe).await?
    } else {
        std::fs::read_to_string(&args.recipe)
            .map_err(|e| format!("Failed to read recipe: {}", e))?
    };

    let docs =
        YamlOwned::load_from_str(&content).map_err(|e| format!("Failed to parse YAML: {}", e))?;
    let yaml = docs
        .into_iter()
        .next()
        .ok_or_else(|| "Empty YAML document".to_string())?;

    if let Some(ref check_host) = args.check_host {
        if let Some(host_list) = get_hosts(&yaml) {
            let is_supported = host_list.iter().any(|h| h.eq_ignore_ascii_case(check_host));

            if !is_supported {
                let supported: Vec<&str> = host_list.iter().map(|s| s.as_str()).collect();
                eprintln!("Recipe does not support host: {}", check_host);
                eprintln!("Supported hosts: {:?}", supported);
                std::process::exit(1);
            }

            println!("Host {} is supported", check_host);
            return Ok(());
        } else {
            println!("Host {} is supported (no restrictions)", check_host);
            return Ok(());
        }
    }

    if let Some(ref field) = args.field {
        let value = match field.as_str() {
            "pkg" => get_str(&yaml, "pkg"),
            "pkg_id" => get_str(&yaml, "pkg_id"),
            "pkg_name" => get_str(&yaml, "pkg_name"),
            "pkg_type" => get_str(&yaml, "pkg_type"),
            "description" => get_str(&yaml, "description"),
            "version" => get_str(&yaml, "version"),
            "hosts" => get_hosts(&yaml).map(|h| h.join(",")),
            _ => yaml
                .as_mapping_get(field)
                .map(|v: &YamlOwned| yaml_value_to_string(v)),
        };

        match value {
            Some(v) => {
                println!("{}", v);
                Ok(())
            }
            None => {
                eprintln!("Field '{}' not found", field);
                std::process::exit(1);
            }
        }
    } else {
        match args.format {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(&yaml_to_json(&yaml))
                    .map_err(|e| format!("Failed to convert to JSON: {}", e))?;
                println!("{}", json);
            }
            OutputFormat::Text => {
                println!(
                    "{}: {}",
                    "pkg".bright_cyan(),
                    get_str(&yaml, "pkg").as_deref().unwrap_or("N/A")
                );
                println!(
                    "{}: {}",
                    "pkg_id".bright_cyan(),
                    get_str(&yaml, "pkg_id").as_deref().unwrap_or("N/A")
                );
                println!(
                    "{}: {}",
                    "pkg_name".bright_cyan(),
                    get_str(&yaml, "pkg_name").as_deref().unwrap_or("N/A")
                );
                println!(
                    "{}: {}",
                    "pkg_type".bright_cyan(),
                    get_str(&yaml, "pkg_type").as_deref().unwrap_or("N/A")
                );
                println!(
                    "{}: {}",
                    "description".bright_cyan(),
                    get_str(&yaml, "description").as_deref().unwrap_or("N/A")
                );

                if let Some(host_list) = get_hosts(&yaml) {
                    println!("{}: {}", "hosts".bright_cyan(), host_list.join(", "));
                } else {
                    println!("{}: {}", "hosts".bright_cyan(), "all (no restrictions)");
                }
            }
        }
        Ok(())
    }
}
