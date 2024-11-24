use std::{
    fs::File,
    io::{self, BufRead, BufReader},
};

use indexmap::IndexMap;

fn extract_field_name(line: &str) -> Option<String> {
    line.split(':').next().map(|s| s.trim().to_string())
}

#[derive(Default, Debug)]
pub struct Comments {
    pub field_comments: IndexMap<String, Vec<String>>,
    pub header_comments: Vec<String>,
    pub field_order: Vec<String>,
}

impl Comments {
    pub fn new() -> Self {
        Self::default()
    }

    // this only works for root level fields
    // inner comments are assigned to adjacent lines, so won't work.
    pub fn parse_comments(&mut self, file_path: &str) -> io::Result<()> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        let mut current_comments = Vec::new();
        let mut shebang_added = false;

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();

            if trimmed.starts_with("#!/SBUILD") {
                if !shebang_added {
                    self.header_comments.push(trimmed.to_string());
                    shebang_added = true;
                }
                continue;
            }

            if trimmed.starts_with('#') {
                current_comments.push(line.to_string());
            } else if !trimmed.is_empty() {
                if let Some(field_name) = extract_field_name(trimmed) {
                    if !current_comments.is_empty() {
                        self.field_comments
                            .insert(field_name.clone(), current_comments.clone());
                        current_comments.clear();
                    }
                    self.field_order.push(field_name);
                }
            }
        }

        if !current_comments.is_empty() {
            self.header_comments.extend(current_comments);
        }

        Ok(())
    }
}
