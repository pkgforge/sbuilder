use colored::Colorize;

use crate::logger::Logger;

#[derive(Debug)]
pub enum Severity {
    Warn,
    Error,
}

#[derive(Debug)]
pub struct ErrorDetails {
    pub field: String,
    pub message: String,
    pub line_number: usize,
    pub severity: Severity,
}

pub fn highlight_error_line(yaml_str: &str, line_number: usize, is_fatal: bool, logger: &Logger) {
    let context_range = 3;
    let start_line = line_number.saturating_sub(context_range);
    let end_line = if line_number + context_range < yaml_str.lines().count() {
        line_number + context_range
    } else {
        yaml_str.lines().count()
    };

    let lines: Vec<&str> = yaml_str
        .lines()
        .skip(start_line)
        .take(end_line - start_line)
        .collect();

    for (index, line) in lines.iter().enumerate() {
        let current_line_number = start_line + index + 1;
        if current_line_number == line_number {
            let msg = format!("--> {}: {}", current_line_number, line);
            logger.custom_error(&format!(
                "{}",
                if is_fatal {
                    msg.red().bold()
                } else {
                    msg.yellow().bold()
                }
            ));
        } else {
            logger.custom_error(&format!("    {}: {}", current_line_number, line));
        }
    }
    logger.custom_error("");
}
