use std::{
    fs::File,
    io::{self, BufWriter, Write},
};

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct LicenseComplex {
    pub id: String,
    pub file: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub enum License {
    Simple(String),
    Complex(LicenseComplex),
}

impl License {
    pub fn write_yaml(&self, writer: &mut BufWriter<File>, indent: usize) -> io::Result<()> {
        let indent_str = " ".repeat(indent);

        match self {
            License::Simple(item) => {
                writeln!(writer, "{}  - \"{}\"", indent_str, item)?;
            }
            License::Complex(item) => {
                writeln!(writer, "{}  - id: \"{}\"", indent_str, item.id)?;
                if let Some(ref file) = item.file {
                    writeln!(writer, "{}    file: \"{}\"", indent_str, file)?;
                }
                if let Some(ref url) = item.url {
                    writeln!(writer, "{}    url: \"{}\"", indent_str, url)?;
                }
            }
        }

        Ok(())
    }
}
