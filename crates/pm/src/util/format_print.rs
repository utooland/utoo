use std::fmt;
use std::io;
use std::io::Write;

use colored::Colorize;
use term_size;

use crate::helper::migrate::MigrateResult;
use crate::service::pm_pack::PackResult;

impl fmt::Display for MigrateResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self
            .fields
            .iter()
            .filter(|(_, count)| *count > 0)
            .map(|(label, count)| format!("{count} {label}"))
            .collect();

        write!(
            f,
            "{} pnpm {} {}",
            "✓".green(),
            "→".dimmed(),
            parts.join(", ")
        )
    }
}

pub fn print_migrate_result(result: &MigrateResult) -> io::Result<()> {
    writeln!(io::stdout(), "{result}")
}

/// Write pack file listing and summary metadata to the given writer.
///
/// Fields with empty/zero values (e.g. dry-run pack with no tarball) are omitted.
/// Pass `shasum` separately because it is computed outside the pack step.
pub fn print_pack_details(
    w: &mut impl io::Write,
    result: &PackResult,
    shasum: Option<&str>,
) -> io::Result<()> {
    for (f, size) in &result.files {
        writeln!(w, "{} {f}", format_size(*size).dimmed())?;
    }
    writeln!(w)?;

    let mut row =
        |label: &str, val: &dyn std::fmt::Display| writeln!(w, "{} {val}", label.dimmed());
    row("Name:", &result.name.cyan())?;
    row("Version:", &result.version)?;
    row("Files:", &result.files.len())?;
    row("Unpacked Size:", &format_size(result.unpacked_size))?;
    if result.packed_size > 0 {
        row("Packed Size:", &format_size(result.packed_size))?;
    }
    if !result.integrity.is_empty() {
        row("Integrity:", &result.integrity)?;
    }
    if let Some(shasum) = shasum {
        row("Shasum:", &shasum)?;
    }
    writeln!(w)?;
    Ok(())
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1000;
    const MB: u64 = KB * 1000;

    if bytes < KB {
        format!("{} B", bytes)
    } else if bytes < MB {
        format!("{:.1} kB", bytes as f64 / KB as f64)
    } else {
        format!("{:.1} MB", bytes as f64 / MB as f64)
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
