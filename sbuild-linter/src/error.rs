use colored::Colorize;

#[derive(Debug)]
pub struct ErrorDetails {
    pub field: String,
    pub message: String,
    pub line_number: usize,
}

pub fn highlight_error_line(yaml_str: &str, line_number: usize) {
    let context_range = 3;
    let start_line = if line_number > context_range {
        line_number - context_range
    } else {
        0
    };
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
            println!(
                "{}",
                format!("--> {}: {}", current_line_number, line)
                    .red()
                    .bold()
            );
        } else {
            println!("    {}: {}", current_line_number, line);
        }
    }
    println!();
}