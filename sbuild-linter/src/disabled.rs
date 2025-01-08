use std::{
    fs::File,
    io::{self, BufWriter, Write},
};

use indexmap::IndexMap;

#[derive(Debug, Clone)]
pub struct ComplexReason {
    pub date: String,
    pub pkg_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub enum DisabledReason {
    Simple(String),
    List(Vec<String>),
    Map(IndexMap<String, ComplexReason>),
}

impl DisabledReason {
    pub fn write_yaml(&self, writer: &mut BufWriter<File>, indent: usize) -> io::Result<()> {
        let indent_str = " ".repeat(indent);

        match self {
            DisabledReason::Simple(value) => {
                writeln!(writer, "{}_disabled_reason: \"{}\"", indent_str, value)?;
            }
            DisabledReason::List(items) => {
                writeln!(writer, "{}_disabled_reason:", indent_str)?;
                for item in items {
                    writeln!(writer, "{}  - \"{}\"", indent_str, item)?;
                }
            }
            DisabledReason::Map(map) => {
                writeln!(writer, "{}_disabled_reason:", indent_str)?;
                for (key, value) in map {
                    writeln!(writer, "{}  {}:", indent_str, key)?;
                    writeln!(writer, "{}    - date: \"{}\"", indent_str, value.date)?;
                    if let Some(ref pkg_id) = value.pkg_id {
                        writeln!(writer, "{}      pkg_id: \"{}\"", indent_str, pkg_id)?;
                    }
                    writeln!(writer, "{}      reason: \"{}\"", indent_str, value.reason)?;
                }
            }
        }

        Ok(())
    }
}
