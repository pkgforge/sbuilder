use clap::{Parser, ValueEnum};
use colored::Colorize;
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

pub async fn run(args: InfoArgs) -> Result<(), String> {
    let content = if args.recipe.starts_with("http://") || args.recipe.starts_with("https://") {
        fetch_recipe(&args.recipe).await?
    } else {
        std::fs::read_to_string(&args.recipe)
            .map_err(|e| format!("Failed to read recipe: {}", e))?
    };

    let yaml: serde_yml::Value =
        serde_yml::from_str(&content).map_err(|e| format!("Failed to parse YAML: {}", e))?;

    if let Some(ref check_host) = args.check_host {
        let hosts = yaml
            .get("x_exec")
            .and_then(|x| x.get("host"))
            .and_then(|h| h.as_sequence());

        if let Some(host_list) = hosts {
            let supported: Vec<&str> = host_list.iter().filter_map(|h| h.as_str()).collect();

            let is_supported = supported.iter().any(|h| h.eq_ignore_ascii_case(check_host));

            if !is_supported {
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
            "pkg" => yaml
                .get("pkg")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            "pkg_id" => yaml
                .get("pkg_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            "pkg_name" => yaml
                .get("pkg_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            "pkg_type" => yaml
                .get("pkg_type")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            "description" => yaml
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            "version" => yaml
                .get("version")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            "hosts" => yaml
                .get("x_exec")
                .and_then(|x| x.get("host"))
                .and_then(|h| h.as_sequence())
                .map(|hosts| {
                    hosts
                        .iter()
                        .filter_map(|h| h.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                }),
            _ => yaml.get(field).map(|v| match v {
                serde_yml::Value::String(s) => s.clone(),
                serde_yml::Value::Bool(b) => b.to_string(),
                serde_yml::Value::Number(n) => n.to_string(),
                _ => serde_yml::to_string(v).unwrap_or_default(),
            }),
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
                let json = serde_json::to_string_pretty(&yaml)
                    .map_err(|e| format!("Failed to convert to JSON: {}", e))?;
                println!("{}", json);
            }
            OutputFormat::Text => {
                println!(
                    "{}: {}",
                    "pkg".bright_cyan(),
                    yaml.get("pkg").and_then(|v| v.as_str()).unwrap_or("N/A")
                );
                println!(
                    "{}: {}",
                    "pkg_id".bright_cyan(),
                    yaml.get("pkg_id").and_then(|v| v.as_str()).unwrap_or("N/A")
                );
                println!(
                    "{}: {}",
                    "pkg_name".bright_cyan(),
                    yaml.get("pkg_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("N/A")
                );
                println!(
                    "{}: {}",
                    "pkg_type".bright_cyan(),
                    yaml.get("pkg_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("N/A")
                );
                println!(
                    "{}: {}",
                    "description".bright_cyan(),
                    yaml.get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("N/A")
                );

                if let Some(hosts) = yaml
                    .get("x_exec")
                    .and_then(|x| x.get("host"))
                    .and_then(|h| h.as_sequence())
                {
                    let host_list: Vec<&str> = hosts.iter().filter_map(|h| h.as_str()).collect();
                    println!("{}: {}", "hosts".bright_cyan(), host_list.join(", "));
                } else {
                    println!("{}: {}", "hosts".bright_cyan(), "all (no restrictions)");
                }
            }
        }
        Ok(())
    }
}
