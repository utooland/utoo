use std::fmt;
use std::io;
use std::io::Write;

use colored::Colorize;
use term_size;
use utoo_ruborist::registry::{RegistryError, ResolveError};

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

/// Scan an anyhow error's cause chain for a `ResolveError::WithChain` and,
/// if found, return a tree-decorated "required by" section in the `ut list`
/// style. Returns `None` if the error is not a dependency-chain error.
///
/// Kept separate from `ResolveError::Display` because tree-drawing with box
/// characters is a CLI presentation concern.
pub fn format_resolve_chain(err: &anyhow::Error) -> Option<String> {
    let chain = err
        .chain()
        .find_map(|cause| cause.downcast_ref::<ResolveError<RegistryError>>())
        .and_then(|re| match re {
            ResolveError::WithChain { chain, .. } => Some(chain),
            _ => None,
        })?;

    let mut out = String::from("required by:");
    for (i, (name, version)) in chain.iter().enumerate() {
        if i == 0 {
            out.push_str(&format!("\n  {name}@{version}"));
        } else {
            let indent = "    ".repeat(i - 1);
            out.push_str(&format!("\n  {indent}└── {name}@{version}"));
        }
    }
    Some(out)
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

// ---------------------------------------------------------------------------
// Multi-workspace run display helpers
// ---------------------------------------------------------------------------

/// Print the `> ...` line before script execution. When `workspace` is set
/// the line is tagged with `[ws]` so multi-workspace output is distinguishable.
pub fn announce_script(workspace: Option<&str>, script_content: &str, script_args_str: &str) {
    let trailing = if script_args_str.is_empty() {
        String::new()
    } else {
        format!(" {script_args_str}")
    };
    match workspace {
        Some(label) => println!(
            "{} {} {}{}",
            ">".bright_cyan(),
            format!("[{label}]").cyan(),
            script_content,
            trailing
        ),
        None => {
            println!("> {script_content}{trailing}");
            println!();
        }
    }
}

/// Header printed once before a multi-workspace run. Shows the script name,
/// total workspace count, layer count, and a truncated listing per layer.
pub fn print_multi_workspace_header(script_name: &str, layers: &[Vec<String>]) {
    let total: usize = layers.iter().map(|l| l.len()).sum();
    let layer_count = layers.len();
    println!(
        "{} Running {} in {} workspace{}, {} layer{}",
        ">".bright_cyan(),
        script_name.green(),
        total,
        if total == 1 { "" } else { "s" },
        layer_count,
        if layer_count == 1 { "" } else { "s" },
    );

    const MAX_NAMES: usize = 5;
    for (layer_index, layer) in layers.iter().enumerate() {
        let (shown, rest) = if layer.len() > MAX_NAMES {
            (&layer[..MAX_NAMES], layer.len() - MAX_NAMES)
        } else {
            (layer.as_slice(), 0)
        };
        let names = shown
            .iter()
            .map(|n| n.cyan().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = if rest > 0 {
            format!(" {} +{rest} more", "…".bright_black())
        } else {
            String::new()
        };
        println!(
            "  {} {}{}",
            format!("{}:", layer_index + 1).bright_black(),
            names,
            suffix
        );
    }
    println!();
}

/// Print the `▶ N/M` layer separator before each layer (only when multiple layers).
pub fn print_layer_separator(layer_index: usize, layer_count: usize) {
    if layer_count > 1 {
        println!("{} {}/{}", "▶".bright_cyan(), layer_index + 1, layer_count);
    }
}

/// Print a single workspace result with ✓/✗ mark, the command header,
/// and any captured body output.
pub fn print_workspace_result(header: &str, body: &[u8], success: bool) {
    let mark = if success {
        "✓".green().to_string()
    } else {
        "✗".red().to_string()
    };
    let headline = header.trim_end();
    println!("{} {}", mark, headline);
    if !body.is_empty() {
        print!("{}", String::from_utf8_lossy(body));
    }
}
