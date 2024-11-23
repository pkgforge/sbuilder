use std::{
    fs::File,
    io::{self, BufWriter, Write},
};

use serde::Deserialize;

#[derive(Debug, Deserialize, Default, Clone)]
pub struct XExec {
    pub disable_shellcheck: Option<bool>,
    pub pkgver: Option<String>,
    pub shell: String,
    pub run: String,
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

        writeln!(writer, "{}shell: \"{}\"", indent_str, self.shell)?;

        if let Some(ref pkgver) = self.pkgver {
            writeln!(writer, "{}pkgver: |", indent_str)?;
            for line in pkgver.lines() {
                writeln!(writer, "{}  {}", indent_str, line)?;
            }
        }

        writeln!(writer, "{}run: |", indent_str)?;
        for line in self.run.lines() {
            writeln!(writer, "{}  {}", indent_str, line)?;
        }

        Ok(())
    }
}
