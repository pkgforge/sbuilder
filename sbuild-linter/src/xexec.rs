use std::{
    fs::File,
    io::{self, BufWriter, Write},
};

use serde::Deserialize;

#[derive(Debug, Deserialize, Default, Clone)]
pub struct XExec {
    pub arch: Option<Vec<String>>,
    pub os: Option<Vec<String>>,
    pub host: Option<Vec<String>>,
    pub conflicts: Option<Vec<String>>,
    pub depends: Option<Vec<String>>,
    pub entrypoint: Option<String>,
    pub pkgver: Option<String>,
    pub shell: String,
    pub run: String,
}

impl XExec {
    pub fn write_yaml(&self, writer: &mut BufWriter<File>, indent: usize) -> io::Result<()> {
        let indent_str = " ".repeat(indent);

        if let Some(ref arch) = self.arch {
            writeln!(writer, "{}arch:", indent_str)?;
            for a in arch {
                writeln!(writer, "{}  - \"{}\"", indent_str, a)?;
            }
        }
        if let Some(ref os) = self.os {
            writeln!(writer, "{}os:", indent_str)?;
            for o in os {
                writeln!(writer, "{}  - \"{}\"", indent_str, o)?;
            }
        }
        if let Some(ref host) = self.host {
            writeln!(writer, "{}host:", indent_str)?;
            for h in host {
                writeln!(writer, "{}  - \"{}\"", indent_str, h)?;
            }
        }
        if let Some(ref conflicts) = self.conflicts {
            writeln!(writer, "{}conflicts:", indent_str)?;
            for c in conflicts {
                writeln!(writer, "{}  - \"{}\"", indent_str, c)?;
            }
        }
        if let Some(ref depends) = self.depends {
            writeln!(writer, "{}depends:", indent_str)?;
            for d in depends {
                writeln!(writer, "{}  - \"{}\"", indent_str, d)?;
            }
        }
        if let Some(ref entrypoint) = self.entrypoint {
            writeln!(writer, "{}entrypoint: \"{}\"", indent_str, entrypoint)?;
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
