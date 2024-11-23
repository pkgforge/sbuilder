use std::{
    collections::HashSet,
    env,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
};

use build_config::{visitor::BuildConfigVisitor, BuildConfig};
use serde::{Deserialize, Deserializer};

mod build_config;
mod distro_pkg;
mod error;
mod validator;
mod xexec;

#[derive(Debug, Deserialize, Clone)]
struct BuildAsset {
    url: String,
    out: String,
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
            let mut writer = BufWriter::new(file);

            for line in headers {
                writeln!(writer, "{}", line).unwrap();
            }
            config.write_yaml(&mut writer, 0).unwrap();
            println!("Validated YAML has been written to {}", output_path);
        }
        Err(e) => {
            eprintln!("{}", e.to_string());
        }
    };
}
