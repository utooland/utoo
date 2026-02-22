use term_size;

pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} kB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub fn print_grid(items: Vec<String>) {
    let terminal_width = term_size::dimensions().map(|(w, _)| w).unwrap_or(80); // default width if unable to get terminal size
    tracing::debug!("Terminal size: {terminal_width}");

    let max_len = items.iter().map(|s| s.len()).max().unwrap_or(1);
    tracing::debug!("Max item length: {max_len}");

    let cols = find_optimal_columns(terminal_width, max_len);
    let rows = items.len().div_ceil(cols);
    let col_len = terminal_width / cols;

    tracing::debug!("Using {cols} columns, {rows} rows, column length {col_len}");

    for row in 0..rows {
        let line = build_row_line(&items, row, cols, col_len);
        println!("{line}");
    }
}

fn find_optimal_columns(terminal_width: usize, max_len: usize) -> usize {
    for &cols in &[12, 6, 4, 3, 2, 1] {
        if (terminal_width / max_len) >= cols || cols == 1 {
            return cols;
        }
    }
    1 // fallback to 1 column
}

fn build_row_line(items: &[String], row: usize, cols: usize, col_len: usize) -> String {
    let mut line = String::new();

    for col in 0..cols {
        let index = col + row * cols;

        if index >= items.len() {
            break;
        }

        let item = &items[index];
        line.push_str(item);

        // Add spaces to align columns, except for the last column
        if col < cols - 1 && col_len > item.len() {
            let spaces = " ".repeat(col_len - item.len());
            line.push_str(&spaces);
        }
    }

    line
}
