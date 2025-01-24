use std::{
    fs::File,
    io::{self, BufWriter, Write},
};

use indexmap::IndexMap;

#[derive(Debug, Clone)]
pub enum Description {
    Simple(String),
    Map(IndexMap<String, String>),
}

impl Description {
    pub fn write_yaml(&self, writer: &mut BufWriter<File>, indent: usize) -> io::Result<()> {
        let indent_str = " ".repeat(indent);

        match self {
            Description::Simple(value) => {
                writeln!(writer, "{}description: \"{}\"", indent_str, value)?;
            }
            Description::Map(map) => {
                writeln!(writer, "{}description:", indent_str)?;
                for (key, value) in map {
                    let key = if key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                        key.to_string()
                    } else {
                        format!("\"{}\"", key)
                    };
                    writeln!(writer, "{}  {}: \"{}\"", indent_str, key, value)?;
                }
            }
        }

        Ok(())
    }
}
