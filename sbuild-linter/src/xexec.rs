use std::{
    fs::File,
    io::{self, BufWriter, Write},
};

use serde::Deserialize;

use crate::escape_yaml_string;

#[derive(Debug, Deserialize, Default, Clone)]
pub struct XExec {
    disable_shellcheck: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pkgver: Option<String>,
    shell: String,
    run: String,
}

impl XExec {
    pub fn write_yaml(&self, writer: &mut BufWriter<File>, indent: usize) -> io::Result<()> {
        let indent_str = " ".repeat(indent);

        if let Some(disable_shellcheck) = self.disable_shellcheck {
            writeln!(
                writer,
                "{}disable_shellcheck: {}",
                indent_str, disable_shellcheck
            )?;
        }

        if let Some(ref pkgver) = self.pkgver {
            writeln!(writer, "{}pkgver: \"{}\"", indent_str, pkgver)?;
        }

        writeln!(writer, "{}shell: \"{}\"", indent_str, self.shell)?;
        writeln!(
            writer,
            "{}run: \"{}\"",
            indent_str,
            escape_yaml_string(&self.run)
        )?;

        Ok(())
    }
}
