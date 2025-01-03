use std::{
    fs::File,
    io::{self, BufWriter, Write},
};

#[derive(Debug, Clone)]
pub struct Resource {
    pub url: Option<String>,
    pub file: Option<String>,
    pub dir: Option<String>,
}

impl Resource {
    pub fn write_yaml(&self, writer: &mut BufWriter<File>, indent: usize) -> io::Result<()> {
        let indent_str = " ".repeat(indent);

        if let Some(ref value) = self.url {
            writeln!(writer, "{}  url: \"{}\"", indent_str, value)?;
        }
        if let Some(ref value) = self.file {
            writeln!(writer, "{}  file: \"{}\"", indent_str, value)?;
        }
        if let Some(ref value) = self.dir {
            writeln!(writer, "{}  dir: \"{}\"", indent_str, value)?;
        }

        Ok(())
    }
}
