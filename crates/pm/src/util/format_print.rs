use std::fmt;
use std::fmt::Write as _;
use std::io;
use std::io::Write;

use colored::Colorize;
use petgraph::graph::NodeIndex;
use term_size;
use utoo_ruborist::registry::{RegistryError, ResolveError};

use crate::helper::migrate::MigrateResult;
use crate::service::dependency_graph::{DepTreeNode, LockGraphService};
use crate::service::pm_pack::PackResult;
use crate::util::logger::format_elapsed_time;

pub use package_view::print_package_info;

/// Print `prompt` (no trailing newline), flush stdout, and read one line from
/// stdin. Returns `true` when the user answers `y`/`Y`.
pub fn confirm(prompt: &str) -> io::Result<bool> {
    print!("{prompt}");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    Ok(input.trim().to_lowercase() == "y")
}

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
    let chain = resolve_chain(err)?;

    let mut out = String::from("required by:");
    for (i, (name, version)) in chain.iter().enumerate() {
        if i == 0 {
            write!(out, "\n  {name}@{version}")
                .expect("writing a dependency chain to String cannot fail");
        } else {
            let indent = "    ".repeat(i - 1);
            write!(out, "\n  {indent}└── {name}@{version}")
                .expect("writing a dependency chain to String cannot fail");
        }
    }
    Some(out)
}

pub fn resolve_chain(err: &anyhow::Error) -> Option<&[(String, String)]> {
    err.chain()
        .find_map(|cause| cause.downcast_ref::<ResolveError<RegistryError>>())
        .and_then(|re| match re {
            ResolveError::WithChain { chain, .. } => Some(chain.as_slice()),
            _ => None,
        })
}

/// `"3 packages installed"` / `"1 package uninstalled"` — the count line
/// printed when an install/uninstall finishes.
pub fn pluralized_package_count(count: usize, verb: &str) -> String {
    format!(
        "{count} package{} {verb}",
        if count == 1 { "" } else { "s" }
    )
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

/// One slow script in a [`print_script_heartbeat`] batch.
pub struct HeartbeatScript<'a> {
    pub label: &'a str,
    pub secs: u64,
    pub last_line: &'a str,
}

/// Persistent heartbeat for long-running *silent* dependency scripts (e.g.
/// `puppeteer postinstall`). The spinner already ticks the longest one's elapsed
/// time in place; this adds a scroll-up record with each script's latest output
/// line (`↳ …`) so a stuck download is visible instead of looking hung, plus a
/// pointer to the run log so the user knows where to dig in. When several are
/// stuck in parallel they're all listed, so you see which ones — not one at a
/// time as each finishes. All dimmed — a reassurance, not an alarm.
///
/// `log_path` is the run-wide `utoo=debug` trace, not this script's stdout: a
/// silent script's captured output is written there (tagged with its package
/// name) once it completes, so `grep <name> <path>` recovers it after the fact.
/// (A *failing* script prints its output to the console inline instead.) Mid-run
/// the live `↳` is the real-time signal; the path tells you where the record
/// lands.
pub fn print_script_heartbeat(scripts: &[HeartbeatScript<'_>], log_path: Option<&std::path::Path>) {
    let logs = log_path
        .map(|p| format!(" — logs: {}", p.display()))
        .unwrap_or_default();

    if let [only] = scripts {
        println!(
            "{}",
            format!("⏳ {} still running [{}s]{logs}", only.label, only.secs).dimmed()
        );
        if !only.last_line.is_empty() {
            println!("{}", format!("  ↳ {}", only.last_line).dimmed());
        }
        return;
    }

    println!(
        "{}",
        format!("⏳ {} scripts still running{logs}", scripts.len()).dimmed()
    );
    for s in scripts {
        let tail = if s.last_line.is_empty() {
            String::new()
        } else {
            format!("  ↳ {}", s.last_line)
        };
        println!(
            "{}",
            format!("  • {} [{}s]{tail}", s.label, s.secs).dimmed()
        );
    }
}

/// Completion line for a streamed install hook, e.g. `✓ prepare [12.3s]`.
/// Gives every hook a uniform end-of-run marker with its wall time.
pub fn print_hook_done(workspace: Option<&str>, label: &str, elapsed: std::time::Duration) {
    let tag = match workspace {
        Some(ws) => format!("[{ws}] {label}"),
        None => label.to_string(),
    };
    println!(
        "{} {} {}",
        "✓".green(),
        tag,
        format_elapsed_time(elapsed).dimmed()
    );
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

/// Recursively print a dependency tree built by
/// [`crate::service::dependency_graph::build_dep_tree`], highlighting the
/// queried package names in yellow.
pub fn print_dep_tree(
    node: &DepTreeNode,
    graph: &LockGraphService,
    prefix: &str,
    is_last: bool,
    highlight: &[&str],
) {
    let is_root = node.index == NodeIndex::end();
    if !is_root {
        let branch = if is_last {
            "└──"
        } else {
            "├───┬"
        };
        if let Some(pkg) = graph.get_graph().node_weight(node.index) {
            let name = pkg.name();
            let version = pkg.version();
            let is_highlight = highlight.iter().any(|&h| name.starts_with(h));
            let display = format!("{}@{}", name, version);
            if is_highlight {
                if !pkg.path.is_empty() {
                    println!("{}{} {} -> {}", prefix, branch, display.yellow(), pkg.path);
                } else {
                    println!("{prefix}{branch} {}", display.yellow());
                }
            } else if !pkg.path.is_empty() {
                println!("{}{} {} -> {}", prefix, branch, display, pkg.path);
            } else {
                println!("{prefix}{branch} {display}");
            }
        }
    }
    let len = node.children.len();
    for (i, child) in node.children.values().enumerate() {
        let is_last_child = i == len - 1;
        let new_prefix = if is_root {
            String::new()
        } else {
            format!("{}{}", prefix, if is_last { "    " } else { "│   " })
        };
        print_dep_tree(child, graph, &new_prefix, is_last_child, highlight);
    }
}

/// Display helpers for `ut view`.
mod package_view {
    use anyhow::Result;
    use chrono::Utc;
    use colored::Colorize;
    use utoo_ruborist::manifest::{FullManifest, VersionManifest};

    use super::print_grid;

    /// Print package information in npm view style format using strong types
    pub fn print_package_info(
        full_manifest: &FullManifest,
        version_manifest: &VersionManifest,
    ) -> Result<()> {
        tracing::debug!("Target version: {}", version_manifest.core.version);

        print_package_header(full_manifest, version_manifest);
        print_package_description(full_manifest, version_manifest);
        print_keywords(full_manifest, version_manifest);
        print_dist_info(version_manifest);
        print_author_info(full_manifest, version_manifest);
        print_repository_info(full_manifest, version_manifest);
        print_bugs_info(full_manifest, version_manifest);
        print_dependencies(version_manifest);
        print_maintainers(full_manifest);
        print_dist_tags(full_manifest);
        print_publish_info(full_manifest, version_manifest);

        Ok(())
    }

    fn print_package_header(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
        // Use description from version manifest or fallback to full manifest
        let description = version_manifest
            .description
            .as_deref()
            .or(full_manifest.description.as_deref())
            .unwrap_or("");

        // Use license from version manifest or fallback to full manifest
        let license = version_manifest
            .core
            .license
            .as_deref()
            .or(full_manifest.license.as_deref())
            .unwrap_or("UNLICENSED");

        let deps_count = version_manifest
            .core
            .dependencies
            .as_ref()
            .map(|d| d.len())
            .unwrap_or(0);
        let versions_count = full_manifest.versions.len();

        let deps_str = if deps_count == 0 {
            "none".to_string()
        } else {
            deps_count.to_string()
        };

        println!(
            "\n{}@{} | {} | deps: {} | versions: {}",
            version_manifest.core.name.bright_blue().bold(),
            version_manifest.core.version.bright_green(),
            license.yellow(),
            deps_str.cyan(),
            versions_count.to_string().magenta()
        );

        if !description.is_empty() {
            println!("{}", description.white());
        }
    }

    fn print_package_description(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
        // Use homepage from version manifest or fallback to full manifest
        let homepage = version_manifest
            .homepage
            .as_ref()
            .or(full_manifest.homepage.as_ref());

        if let Some(homepage) = homepage {
            println!("{}", homepage.blue().underline());
        }
        println!();
    }

    fn print_keywords(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
        // Use keywords from version manifest or fallback to full manifest
        let keywords = version_manifest
            .keywords
            .as_ref()
            .or(full_manifest.keywords.as_ref())
            .filter(|k| !k.is_empty());

        if let Some(keywords) = keywords {
            let keyword_str = keywords.join(", ");
            println!("{} {}", "keywords:".bright_cyan(), keyword_str.white());
        }
    }

    fn print_dist_info(version_manifest: &VersionManifest) {
        if let Some(tarball) = &version_manifest.core.dist.tarball {
            println!("\n{}", "dist".bright_yellow().bold());
            println!("{} {}", ".tarball:".cyan(), tarball.blue().underline());
        }

        if let Some(shasum) = &version_manifest.core.dist.shasum {
            println!("{} {}", ".shasum:".cyan(), shasum.green());
        }

        if let Some(integrity) = &version_manifest.core.dist.integrity {
            println!("{} {}", ".integrity:".cyan(), integrity.green());
        }

        if let Some(unpacked_size) = version_manifest.core.dist.unpacked_size {
            let size_mb = unpacked_size as f64 / 1024.0 / 1024.0;
            println!(
                "{} {} MB",
                ".unpackedSize:".cyan(),
                format!("{size_mb:.1}").yellow()
            );
        }
    }

    fn print_author_info(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
        // Use author from version manifest or fallback to full manifest
        let author = version_manifest
            .author
            .as_ref()
            .or(full_manifest.author.as_ref());

        if let Some(author) = author {
            let author_line = match &author.email {
                Some(email) => format!(
                    "\n{} {} <{}>",
                    "author:".bright_magenta(),
                    author.name.white(),
                    email.blue()
                ),
                None => format!("\n{} {}", "author:".bright_magenta(), author.name.white()),
            };
            println!("{author_line}");
        }
    }

    fn print_repository_info(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
        // Use repository from version manifest or fallback to full manifest
        let repository = version_manifest
            .repository
            .as_ref()
            .or(full_manifest.repository.as_ref());

        if let Some(repo) = repository {
            println!(
                "{} {}:{}",
                "repository:".bright_magenta(),
                repo.repo_type.green(),
                repo.url.blue().underline()
            );
        }
    }

    fn print_bugs_info(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
        // Use bugs from version manifest or fallback to full manifest
        let bugs = version_manifest
            .bugs
            .as_ref()
            .or(full_manifest.bugs.as_ref());

        if let Some(bugs) = bugs {
            println!(
                "{} {}",
                "bugs:".bright_magenta(),
                bugs.url.blue().underline()
            );
        }
    }

    fn print_dependencies(version_manifest: &VersionManifest) {
        if let Some(dependencies) = version_manifest
            .core
            .dependencies
            .as_ref()
            .filter(|d| !d.is_empty())
        {
            println!(
                "\n{} {}",
                "dependencies:".bright_yellow().bold(),
                dependencies.len().to_string().white()
            );

            let show_count = 24;
            let show_deps: Vec<String> = dependencies
                .iter()
                .take(show_count)
                .map(|(dep_name, dep_version)| format!("{}: {}", dep_name.blue(), dep_version))
                .collect();

            print_grid(show_deps);

            if dependencies.len() > show_count {
                println!(
                    "(... and {} more.)",
                    (dependencies.len() - show_count).to_string().white()
                );
            }
        }
    }

    fn print_maintainers(full_manifest: &FullManifest) {
        if let Some(maintainers) = full_manifest.maintainers.as_ref().filter(|m| !m.is_empty()) {
            println!("\n{}", "maintainers:".bright_yellow().bold());

            for maintainer in maintainers {
                let maintainer_line = match &maintainer.email {
                    Some(email) => format!("- {} <{}>", maintainer.name.blue(), email.white()),
                    None => format!("- {}", maintainer.name.blue()),
                };
                println!("{maintainer_line}");
            }
        }
    }

    fn print_dist_tags(full_manifest: &FullManifest) {
        if !full_manifest.dist_tags.is_empty() {
            println!("\n{}", "dist-tags:".bright_yellow().bold());

            let tags: Vec<String> = full_manifest
                .dist_tags
                .iter()
                .map(|(tag, version)| format!("{}: {}", tag.blue(), version))
                .collect();

            print_grid(tags);
        }
    }

    fn print_publish_info(full_manifest: &FullManifest, version_manifest: &VersionManifest) {
        // Get publish time from time info
        if let Some(time_str) = full_manifest.time.get(&version_manifest.core.version)
            && let Ok(published_time) = chrono::DateTime::parse_from_rfc3339(time_str)
        {
            let time_str = format_time_ago(published_time.with_timezone(&Utc));
            let publish_line = format_publish_line(&time_str, version_manifest);
            println!("\n{publish_line}");
        }
    }

    fn format_time_ago(published_time: chrono::DateTime<Utc>) -> String {
        let now = Utc::now();
        let duration = now.signed_duration_since(published_time);

        match duration.num_days() {
            days if days > 365 => "over a year ago".to_string(),
            days if days > 30 => format!("{} months ago", days / 30),
            days if days > 0 => format!("{days} days ago"),
            _ => match duration.num_hours() {
                hours if hours > 0 => format!("{hours} hours ago"),
                _ => format!("{} minutes ago", duration.num_minutes()),
            },
        }
    }

    fn format_publish_line(time_str: &str, version_manifest: &VersionManifest) -> String {
        match &version_manifest.npm_user {
            Some(npm_user) => match &npm_user.email {
                Some(email) => format!(
                    "{} {} by {} <{}>",
                    "published",
                    time_str.cyan(),
                    npm_user.name.blue(),
                    email.white()
                ),
                None => format!(
                    "{} {} by {}",
                    "published",
                    time_str.cyan(),
                    npm_user.name.blue()
                ),
            },
            None => format!("{} {}", "published", time_str.cyan()),
        }
    }

    #[cfg(test)]
    mod tests {
        use utoo_ruborist::manifest::Dist;

        use super::*;

        #[test]
        fn test_print_package_info() {
            // Create test data
            let full_manifest = FullManifest {
                name: "test-package".to_string(),
                maintainers: Some(vec![]),
                ..Default::default()
            };

            let version_manifest = VersionManifest {
                core: utoo_ruborist::manifest::CoreVersionManifest {
                    name: "test-package".to_string(),
                    version: "1.0.0".to_string(),
                    dist: Dist::default(),
                    ..Default::default()
                },
                description: Some("A test package".to_string()),
                ..Default::default()
            };

            // This test just ensures the function doesn't panic
            let result = print_package_info(&full_manifest, &version_manifest);
            assert!(result.is_ok());
        }
    }
}
